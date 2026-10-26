// ---
// relationships:
//   implements: project-updates
// ---
//! Prepared template upgrades with explicit host inputs and an injected hook runner.
use crate::applied::{
    Application, Base, Cleanliness, MergeError, MergeOptions, Merged, Project, RecordInputs,
    UpdateReport,
};
use crate::{
    Id, RawAnswer, RawAnswers, Template,
    context::{EnvironmentDecision, InvocationContext},
    interview::Seed,
    resolve::{Paths as Dirs, ResolvedTemplate},
    staging::StagedRecord,
};
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::path::Path;
/// Choices for one update apply or preview. Never stored in a staged record.
#[derive(Debug, Default, Clone, Copy)]
pub struct UpdateOptions {
    /// Explicitly replace a drifted managed region in this invocation.
    pub force: bool,
    pub dry_run: bool,
}
/// The base an update merges from.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum UpdateBase {
    /// The application matching this source and target.
    Update,
    /// An empty base: the whole render is merged, three-way against nothing.
    Baseline,
}
#[non_exhaustive]
#[derive(Debug)]
pub enum UpdateError {
    Admission(Box<crate::operations::OperationError>),
    WithWarnings {
        warnings: Vec<String>,
        source: Box<UpdateError>,
    },
    Error(String),
    Resolve(crate::resolve::ResolveError),
    Requirements(crate::MissingRequirements),
    Project(crate::applied::ProjectError),
    NotGit,
    DirtyTarget,
    StaleApplication,
    InvalidApplication,
    Base {
        template: String,
        revision: crate::applied::Revision,
        reason: String,
    },
}
impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WithWarnings { source, .. } => source.fmt(f),
            Self::Admission(e) => e.fmt(f),
            Self::Error(e) => e.fmt(f),
            Self::Resolve(e) => write!(f, "{e}"),
            Self::Requirements(e) => e.fmt(f),
            Self::Project(e) => e.fmt(f),
            Self::NotGit => {
                f.write_str("the target is not inside a git repository; an update needs git")
            }
            Self::DirtyTarget => f.write_str(
                "the target has uncommitted changes; commit or discard them before an update",
            ),
            Self::StaleApplication => f.write_str("the applied record entry changed"),
            Self::InvalidApplication => f.write_str("invalid staged application"),
            Self::Base {
                template,
                revision,
                reason,
            } => write!(
                f,
                "cannot render merge base for {template} at {revision}: {reason}"
            ),
        }
    }
}
impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WithWarnings { source, .. } => Some(source.as_ref()),
            Self::Admission(e) => Some(e.as_ref()),
            Self::Resolve(e) => Some(e),
            Self::Requirements(e) => Some(e),
            Self::Project(e) => Some(e),
            _ => None,
        }
    }
}
#[derive(Debug)]
pub struct Environment<'a, S: crate::context::FixedEnvironmentSource> {
    pub config: &'a crate::config::Config,
    pub registry: &'a crate::registry::Registry,
    pub cwd: &'a Path,
    pub now: jiff::Zoned,
    pub host: crate::context::HostFacts,
    pub is_admin: bool,
    pub source: &'a mut S,
}
/// The shared pieces after resolving the base and template: project, merge base,
/// selected revision, render context, recorded submissions and trust grant.
/// It retains a thread-local Git handle; construct and consume it in one worker.
pub struct Prepared {
    pub project: Project,
    pub merge_base: Base,
    pub base_application: Option<Application>,
    pub resolved: ResolvedTemplate,
    pub template: Template,
    pub now: jiff::Zoned,
    pub context: InvocationContext,
    pub configured_defaults: RawAnswers,
    pub recorded: Vec<IndexMap<Id, RawAnswer>>,
    pub trusted: bool,
    pub warnings: Vec<String>,
}

/// Open the project, resolve the base application and template, enforce the update
/// preconditions and identity (17), and build the render context and trust
/// grant. Shared by the one-shot update route and the staged update flow.
/// How `prepare` grants the environment and computes trust: an `apply` merges
/// now; a `stage` captures the grant for later batches.
#[non_exhaustive]
#[derive(Debug)]
pub enum PrepareMode {
    Apply,
    Stage,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare(
    base: &UpdateBase,
    template: &str,
    target: &crate::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    mode: PrepareMode,
    interactive: bool,
    environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
) -> Result<Prepared, UpdateError> {
    Preparation::new(
        base,
        template,
        target,
        dirs,
        environment.config,
        environment.registry,
        environment.cwd,
    )?
    .finish(trust, mode, interactive, environment)
}

/// Resolved update inputs with the merge base, requirements and configured defaults admitted.
/// Construct this before acquiring fallible clock or environment inputs. Like [`Prepared`],
/// it retains a request-local Git handle and must stay inside its blocking worker.
pub struct Preparation {
    project: Project,
    merge_base: Base,
    base_application: Option<Application>,
    resolved: ResolvedTemplate,
    template: Template,
    target: crate::staging::CanonicalTarget,
    prior_part_question: bool,
    configured_defaults: RawAnswers,
    warnings: Vec<String>,
}
impl Preparation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base: &UpdateBase,
        template: &str,
        target: &crate::staging::CanonicalTarget,
        dirs: &Dirs,
        config: &crate::config::Config,
        registry: &crate::registry::Registry,
        cwd: &Path,
    ) -> Result<Self, UpdateError> {
        let project = open_project(target)?;
        let resolved = crate::resolve::resolve_template(template, config, registry, dirs, cwd)
            .map_err(UpdateError::Resolve)?;
        let base_application = match base {
            UpdateBase::Update => Some(
                project
                    .application(&crate::applied::source_identity(&resolved.formal_name))
                    .map_err(|e| UpdateError::Error(e.to_string()))?,
            ),
            UpdateBase::Baseline => None,
        };
        let (merge_base, base_application, prior_part_question) = restore_merge_base(
            &project,
            base_application,
            target,
            config,
            registry,
            dirs,
            cwd,
        )?;
        // The scripted route (`--answers`) reports a missing requirement as its
        // `requirement` error document; the other routes report it as text.
        let template = match Template::load(&resolved.folder) {
            Ok(value) => value,
            Err(error) => return Err(UpdateError::Error(error.to_string())),
        };
        if let Err(error) = crate::requirement::check(&template) {
            return Err(UpdateError::Requirements(error));
        }

        let resolution = crate::interview::configured_defaults(
            &resolved.formal_name,
            &template,
            &config.presets,
            &config.template_defaults,
        )
        .map_err(|e| UpdateError::Error(e.to_string()))?;
        let (configured_defaults, warnings) = resolution.into_flat_defaults();
        Ok(Self {
            project,
            merge_base,
            base_application,
            resolved,
            template,
            target: target.clone(),
            prior_part_question,
            configured_defaults,
            warnings,
        })
    }

    pub fn finish(
        self,
        trust: bool,
        mode: PrepareMode,
        interactive: bool,
        environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
    ) -> Result<Prepared, UpdateError> {
        let Self {
            project,
            merge_base,
            base_application,
            resolved,
            template,
            target,
            prior_part_question,
            configured_defaults,
            warnings,
        } = self;
        let runtime_now = environment.now.clone();
        let now = match &base_application {
            Some(application) => application.generated().get().clone(),
            None => runtime_now,
        };

        // The environment decision and trust grant. An apply grants env values when
        // the template is already trusted (an approval or `--trust`). A stage
        // captures the fixed five when `--trust` is given, discovers the references
        // upfront, and refuses before any batch when one needs trust it lacks, so the
        // grant carries through every later batch and the apply.
        let (decision, trusted) = match mode {
            PrepareMode::Apply => {
                let trusted = crate::operations::effective_trust(trust, &resolved, &template);
                (EnvironmentDecision::grant_if_needed(trusted), trusted)
            }
            PrepareMode::Stage => {
                let decision = if trust {
                    EnvironmentDecision::CarryStageGrant
                } else {
                    EnvironmentDecision::RequireStageGrant
                };
                (decision, trust)
            }
        };
        let context = match crate::operations::invocation_context(
            &target,
            &resolved,
            &template,
            decision,
            environment.host.clone(),
            crate::context::ExecutionFacts::new(environment.is_admin, interactive),
            environment.source,
        ) {
            Ok(value) => value,
            Err(error) => {
                return Err(UpdateError::WithWarnings {
                    warnings,
                    source: Box::new(UpdateError::Admission(Box::new(error))),
                });
            }
        };

        let recorded = match &base_application {
            Some(application) => candidate_submissions(application, &template, prior_part_question)
                .map_err(UpdateError::Error)?,
            None => Vec::new(),
        };

        Ok(Prepared {
            project,
            merge_base,
            base_application,
            resolved,
            template,
            now,
            context,
            configured_defaults,
            recorded,
            trusted,
            warnings,
        })
    }
}
impl std::fmt::Debug for Preparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preparation")
            .field("resolved", &self.resolved)
            .field("target", &self.target)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

/// Restore the pieces a staged-update resume needs from its record: reopen the
/// project, re-resolve the template and base application, restore the identity-
/// bearing context (not rebuilt), and recompute the trust grant for the apply.
/// The target's cleanliness is re-checked so a merge never runs on a dirty tree.
pub fn resume_prepared(
    saved: &StagedRecord,
    base: &UpdateBase,
    target: &crate::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
) -> Result<Prepared, UpdateError> {
    let admission = admit_resume(saved, base, target)?;
    resume_admitted(admission, saved, target, dirs, trust, environment)
}

/// Repository and pinned application admitted before other host inputs are acquired.
pub struct ResumeAdmission {
    project: Project,
    base_application: Option<Application>,
}
pub fn admit_resume(
    saved: &StagedRecord,
    base: &UpdateBase,
    target: &crate::staging::CanonicalTarget,
) -> Result<ResumeAdmission, UpdateError> {
    let project = open_project(target)?;

    let base_application = match base {
        UpdateBase::Update => {
            let application = project
                .application(&crate::applied::source_identity(&saved.template))
                .map_err(|e| UpdateError::Error(e.to_string()))?;
            let pinned: Value = serde_json::from_str(saved.base.as_deref().unwrap_or(""))
                .map_err(|_| UpdateError::InvalidApplication)?;
            if application.to_value() != pinned {
                return Err(UpdateError::StaleApplication);
            }
            Some(application)
        }
        UpdateBase::Baseline => None,
    };
    Ok(ResumeAdmission {
        project,
        base_application,
    })
}

pub fn resume_admitted(
    admission: ResumeAdmission,
    saved: &StagedRecord,
    target: &crate::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
) -> Result<Prepared, UpdateError> {
    let ResumeAdmission {
        project,
        base_application,
    } = admission;
    let cwd = environment.cwd;
    let config = environment.config;
    let registry = environment.registry;
    let (merge_base, base_application, prior_part_question) = restore_merge_base(
        &project,
        base_application,
        target,
        config,
        registry,
        dirs,
        cwd,
    )?;

    let resolved = match crate::resolve::resume_template(
        &saved.template,
        &saved.commit,
        saved.named,
        config,
        registry,
        dirs,
        cwd,
    ) {
        Ok(value) => value,
        Err(error) => return Err(UpdateError::Resolve(error)),
    };
    let mut template = match Template::load(&resolved.folder).map_err(|e| e.to_string()) {
        Ok(value) => value,
        Err(error) => return Err(UpdateError::Error(error)),
    };

    crate::requirement::check(&template).map_err(UpdateError::Requirements)?;
    template.part_selection = saved.part_selection.clone();
    let resolution = crate::interview::configured_defaults(
        &resolved.formal_name,
        &template,
        &config.presets,
        &config.template_defaults,
    )
    .map_err(|e| UpdateError::Error(e.to_string()))?;
    let (configured_defaults, warnings) = resolution.into_flat_defaults();
    let now = match saved.now.parse::<jiff::Zoned>() {
        Ok(value) => value,
        Err(error) => return Err(UpdateError::Error(error.to_string())),
    };
    let context = match saved.invocation_context(target) {
        Ok(value) => value,
        Err(error) => return Err(UpdateError::Error(error.to_string())),
    };
    let trusted = crate::operations::effective_trust(trust, &resolved, &template);
    let recorded = match &base_application {
        Some(application) => candidate_submissions(application, &template, prior_part_question)
            .map_err(UpdateError::Error)?,
        None => Vec::new(),
    };
    Ok(Prepared {
        project,
        merge_base,
        base_application,
        resolved,
        template,
        now,
        context,
        configured_defaults,
        recorded,
        trusted,
        warnings,
    })
}

/// A staged record's stored submissions as the interview's raw answers.
pub fn to_raw_staged(
    stored: &[IndexMap<String, Value>],
) -> Result<Vec<IndexMap<Id, RawAnswer>>, String> {
    stored
        .iter()
        .map(|submission| {
            submission
                .iter()
                .map(|(key, value)| {
                    Id::parse(key)
                        .map(|id| (id, RawAnswer(value.clone())))
                        .map_err(|message| format!("staged answer id {key:?}: {message}"))
                })
                .collect::<Result<IndexMap<_, _>, _>>()
        })
        .collect()
}

/// Whether an update route's merge is a dry run: the invocation's `--dry-run`
/// composes with a completed interview's own `flow: dry-run` disposition by
/// union, either suppresses the write, matching the scripted and agent-apply
/// routes (`main.rs`'s `scripted_completed` and fresh-apply resume).
pub fn effective_dry_run(dry_run: bool, completed: &crate::Completed) -> bool {
    dry_run || matches!(completed.step(), crate::Step::Plan { apply: false })
}

fn candidate_submissions(
    application: &Application,
    template: &Template,
    prior_part_question: bool,
) -> Result<Vec<IndexMap<Id, RawAnswer>>, String> {
    let documents = template.document_question_ids();
    Ok(recorded_submissions(application, template)?
        .into_iter()
        .map(|mut submission| {
            submission.retain(|id, _| {
                !documents.contains(id)
                    && (prior_part_question || template.part_question().is_none_or(|q| &q.id != id))
            });
            submission
        })
        .filter(|submission| !submission.is_empty())
        .collect())
}

/// The base application's recorded submissions, as the interview's raw answers.
fn recorded_submissions(
    application: &Application,
    template: &Template,
) -> Result<Vec<IndexMap<Id, RawAnswer>>, String> {
    let mut submissions: Vec<IndexMap<Id, RawAnswer>> = application
        .submissions()
        .iter()
        .map(|submission| {
            submission
                .iter()
                .map(|(key, value)| {
                    Id::parse(key)
                        .map(|id| (id, RawAnswer(value.clone())))
                        .map_err(|message| format!("recorded answer id {key:?}: {message}"))
                })
                .collect::<Result<IndexMap<_, _>, _>>()
        })
        .collect::<Result<_, _>>()?;
    if let Some(q) = template.part_question() {
        submissions.push(IndexMap::from([(
            q.id.clone(),
            RawAnswer(json!(application.parts)),
        )]));
    }
    Ok(submissions)
}

#[allow(clippy::too_many_arguments)]
fn restore_merge_base(
    project: &Project,
    application: Option<Application>,
    target: &crate::staging::CanonicalTarget,
    config: &crate::config::Config,
    registry: &crate::registry::Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<(Base, Option<Application>, bool), UpdateError> {
    let Some(mut application) = application else {
        return Ok((Base::Empty, None, false));
    };
    let failure_source = application.source().to_owned();
    let failure_revision = application.revision().clone();
    let failure = |reason: String| UpdateError::Base {
        template: failure_source.clone(),
        revision: failure_revision.clone(),
        reason,
    };
    if matches!(
        application.revision(),
        crate::applied::Revision::Unversioned
    ) {
        return Err(failure("the template has no reproducible revision".into()));
    }
    let resolved = crate::resolve::resume_application_template(
        application.template(),
        application.revision(),
        config,
        registry,
        dirs,
        cwd,
    )
    .map_err(|e| failure(e.to_string()))?;
    let template = Template::load(&resolved.folder).map_err(|e| failure(e.to_string()))?;
    let context = InvocationContext::new(
        target.clone(),
        crate::context::SelectedTemplate::new(
            template.name.clone(),
            application.template().into(),
            vec![],
            resolved.source.clone(),
        ),
        crate::context::HostFacts::new(String::new(), String::new(), None, None, vec![]),
        crate::context::ExecutionFacts::new(false, false),
        crate::context::EnvironmentSnapshot::Unavailable,
    )
    .map_err(|e| failure(e.to_string()))?
    .with_recorded(application.context.clone());
    if template
        .record_context(Some(&context))
        .keys()
        .any(|key| !application.context.contains_key(key))
    {
        return Err(failure(
            "a referenced render context value is missing from the record".into(),
        ));
    }
    let seed = Seed {
        now: application.generated().get().clone(),
        defaults: IndexMap::new(),
        context,
    };
    let mut interview =
        crate::Interview::start(&template, seed).map_err(|e| failure(e.to_string()))?;
    let mut remaining = recorded_submissions(&application, &template)
        .map_err(failure)?
        .into_iter()
        .flatten()
        .collect::<IndexMap<_, _>>();
    let completed = loop {
        match interview {
            crate::Interview::Complete(completed) => break completed,
            crate::Interview::Ended(_) => {
                return Err(failure("the recorded interview ended".into()));
            }
            crate::Interview::Asking(pending) => {
                let values = pending
                    .batch()
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        crate::interview::Item::Prompt(p) => {
                            remaining.shift_remove(&p.id).map(|v| (p.id.clone(), v))
                        }
                        _ => None,
                    })
                    .collect::<IndexMap<_, _>>();
                if values.is_empty() {
                    return Err(failure("a recorded question is unanswered".into()));
                }
                interview = pending
                    .answer(values)
                    .map_err(|_| failure("a recorded answer was rejected".into()))?;
            }
        }
    };
    let plan =
        crate::Plan::build(&template, &completed, target).map_err(|e| failure(e.to_string()))?;
    project
        .restore_base(&mut application, plan)
        .map_err(|e| failure(e.to_string()))?;
    Ok((
        Base::Application(application.clone()),
        Some(application),
        template.part_question().is_some(),
    ))
}

impl Prepared {
    /// Preview or apply using the caller's runner. Flow dry-run always wins over a write request.
    pub fn finish(
        self,
        target: &crate::staging::CanonicalTarget,
        completed: &crate::Completed,
        dry_run: bool,
        runner: &dyn crate::hook::HookRunner,
    ) -> Result<(Merged, UpdateReport), MergeError> {
        self.finish_with_options(
            target,
            completed,
            UpdateOptions {
                dry_run,
                ..Default::default()
            },
            runner,
        )
    }

    /// Preview or apply with an explicit, invocation-local region force choice.
    pub fn finish_with_options(
        self,
        target: &crate::staging::CanonicalTarget,
        completed: &crate::Completed,
        options: UpdateOptions,
        runner: &dyn crate::hook::HookRunner,
    ) -> Result<(Merged, UpdateReport), MergeError> {
        if self.context.target() != target {
            return Err(MergeError::WrongTarget);
        }
        let revision =
            crate::resolve::applied_revision(&self.resolved.formal_name, &self.resolved.commit)
                .map_err(|e| MergeError::Io(e.to_string()))?;
        crate::applied::merge_apply_reported_with_force(
            &self.project,
            self.merge_base,
            &self.template,
            completed,
            RecordInputs {
                template: self.resolved.formal_name,
                revision,
            },
            MergeOptions {
                trusted: self.trusted,
                dry_run: effective_dry_run(options.dry_run, completed),
            },
            options.force,
            runner,
        )
    }
}

/// The baseline marker is distinct from a serialized application object.
pub const BASELINE_MARKER: &str = "@baseline";

/// Whether a staged record is an update (it carries a base).
pub fn is_staged_update(saved: &StagedRecord) -> bool {
    saved.base.is_some()
}

/// The stored base value for a staged record: the copied application, or the baseline
/// marker.
pub fn stored_base(base: &UpdateBase, base_application: Option<&Application>) -> String {
    match base {
        UpdateBase::Update => serde_json::to_string(
            &base_application
                .expect("--update resolves an application")
                .to_value(),
        )
        .expect("application serializes"),
        UpdateBase::Baseline => BASELINE_MARKER.to_owned(),
    }
}
pub fn base_of(saved: &StagedRecord) -> Result<UpdateBase, String> {
    let value = saved.base.as_deref().ok_or("record is not an update")?;
    if value == BASELINE_MARKER {
        Ok(UpdateBase::Baseline)
    } else {
        serde_json::from_str::<Value>(value)
            .map_err(|_| "invalid staged application".to_owned())?;
        Ok(UpdateBase::Update)
    }
}

/// The interview's raw submissions, in batch order.
pub type Submissions = Vec<IndexMap<Id, RawAnswer>>;

/// The interview's raw submissions as the string-keyed maps a record stores.
pub fn to_stored(submissions: Vec<IndexMap<Id, RawAnswer>>) -> Vec<IndexMap<String, Value>> {
    submissions
        .into_iter()
        .map(|submission| {
            submission
                .into_iter()
                .map(|(id, raw)| (id.as_str().to_owned(), raw.0))
                .collect()
        })
        .collect()
}

/// Build the staged record for an update from `prep`, the base value, the
/// `reanswer` flag, and the submissions accepted so far.
pub fn record(
    prep: &Prepared,
    base: &str,
    reanswer: bool,
    submissions: Vec<IndexMap<Id, RawAnswer>>,
) -> StagedRecord {
    let mut record = StagedRecord::new_with_context(
        prep.context.clone(),
        prep.resolved.commit.clone(),
        prep.resolved.named,
        prep.now.to_string(),
        to_stored(submissions),
    );
    record.base = Some(base.to_owned());
    record.reanswer = reanswer;
    record.part_selection = prep.template.part_selection.clone();
    crate::operations::save_document_answers(&prep.template, &mut record);
    record
}

/// One request's Git-bound operation. It is not `Send` with the current Git features.
/// Move owned [`StagedRecord`] inputs into a blocking worker, construct and consume
/// this operation there, and return its `Send` record/response across the worker boundary.
/// Serialize `record` and rebuild through `resume` for each subsequent request.
pub struct UpdateSession {
    ended: Option<crate::Ended>,
    pub prepared: Prepared,
    pub record: StagedRecord,
    pub target: crate::staging::CanonicalTarget,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    #[error("{0}")]
    Update(#[from] UpdateError),
    #[error("{0}")]
    Operation(#[from] crate::operations::OperationError),
    #[error("{0}")]
    Staging(#[from] crate::staging::StagingError),
    #[error("{0}")]
    Merge(#[from] MergeError),
    #[error("{0}")]
    Evaluation(#[from] crate::EvalError),
}
impl UpdateSession {
    /// Admit current document inputs and parts, then submit `overrides` together with
    /// the recorded answers. The owned record retains initial overrides, including
    /// rejected values, so state and structured rejections survive reconstruction.
    /// Accepted submissions and any flow end are retained.
    /// Unlike [`crate::operations::Session::start`]'s admission-only answers,
    /// these values also answer the update interview. Use [`Self::state`] before
    /// acquiring more input or applying; an ended interview must not be applied.
    #[allow(clippy::result_large_err)]
    pub fn start(
        mut prepared: Prepared,
        target: crate::staging::CanonicalTarget,
        base: UpdateBase,
        reanswer: bool,
        parts: crate::parts::Selection,
        inputs: &[std::path::PathBuf],
        overrides: &RawAnswers,
    ) -> Result<Self, SessionError> {
        if prepared.context.target() != &target {
            return Err(crate::operations::OperationError::Input(
                "prepared update does not match the target".into(),
            )
            .into());
        }
        crate::document::prepare(&mut prepared.template, target.as_path(), inputs, overrides)
            .map_err(crate::operations::OperationError::from)?;
        crate::operations::admit_parts(&mut prepared.template, parts, None, overrides)
            .map_err(crate::operations::OperationError::Parts)?;
        let base = stored_base(&base, prepared.base_application.as_ref());
        let mut record = record(&prepared, &base, reanswer, vec![]);
        record.update_overrides = overrides
            .iter()
            .map(|(id, raw)| (id.to_string(), raw.0.clone()))
            .collect();
        let driven = crate::applied::drive_update(
            &prepared.template,
            Seed {
                now: prepared.now.clone(),
                defaults: prepared.configured_defaults.clone(),
                context: prepared.context.clone(),
            },
            &prepared.recorded,
            overrides.clone(),
            reanswer,
        )?;
        let mut ended = None;
        match driven {
            crate::applied::UpdateDrive::Completed { submissions, .. } => {
                record.submissions = to_stored(submissions);
            }
            crate::applied::UpdateDrive::Ask { submissions, .. } => {
                record.submissions = to_stored(submissions);
            }
            crate::applied::UpdateDrive::Ended(end) => {
                ended = Some(end);
            }
        };
        crate::operations::save_document_answers(&prepared.template, &mut record);
        Ok(Self {
            prepared,
            record,
            target,
            ended,
        })
    }

    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn resume(
        record: StagedRecord,
        target: crate::staging::CanonicalTarget,
        paths: &Dirs,
        trust: bool,
        environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
    ) -> Result<Self, SessionError> {
        let base = base_of(&record).map_err(UpdateError::Error)?;
        let prepared = resume_prepared(&record, &base, &target, paths, trust, environment)?;
        Self::from_prepared(prepared, record, target)
    }

    /// Attach an admitted replay record to the matching prepared operation without new file reads.
    pub fn from_prepared(
        mut prepared: Prepared,
        record: StagedRecord,
        target: crate::staging::CanonicalTarget,
    ) -> Result<Self, SessionError> {
        record.invocation_context(&target)?;
        if prepared.context.target() != &target
            || record.template != prepared.resolved.formal_name
            || record.commit != prepared.resolved.commit
        {
            return Err(crate::operations::OperationError::Input(
                "prepared update does not match the replay record and target".into(),
            )
            .into());
        }
        crate::document::restore_missing(
            &mut prepared.template,
            &crate::operations::recorded_answers(&record),
        )
        .map_err(UpdateError::Error)?;
        Ok(Self {
            prepared,
            record,
            target,
            ended: None,
        })
    }
    pub fn admit_documents(
        &mut self,
        inputs: &[std::path::PathBuf],
        incoming: &RawAnswers,
    ) -> Result<(), SessionError> {
        crate::operations::prepare_staged_documents(
            &mut self.prepared.template,
            self.target.as_path(),
            inputs,
            incoming,
            &mut self.record,
        )
        .map_err(crate::operations::OperationError::from)?;
        Ok(())
    }
    /// Save replay-normalized accepted progress. Refusal/preview retain the record; an end consumes it.
    pub fn persist(&mut self, store: &crate::staging::Store) -> Result<(), SessionError> {
        let submissions = match self.replay()? {
            crate::applied::UpdateDrive::Completed { submissions, .. }
            | crate::applied::UpdateDrive::Ask { submissions, .. } => Some(submissions),
            crate::applied::UpdateDrive::Ended(_) => None,
        };
        match submissions {
            Some(submissions) => {
                self.record.submissions = to_stored(submissions);
                crate::operations::save_document_answers(&self.prepared.template, &mut self.record);
                store.save(&self.target, &self.record)?;
            }
            _ => {
                store.remove(&self.target)?;
            }
        }
        Ok(())
    }

    fn overrides(&self) -> Result<RawAnswers, SessionError> {
        Ok(
            to_raw_staged(std::slice::from_ref(&self.record.update_overrides))
                .map_err(UpdateError::Error)?
                .into_iter()
                .flatten()
                .collect(),
        )
    }

    pub fn replay(&self) -> Result<crate::applied::UpdateDrive<'_>, SessionError> {
        if let Some(ended) = &self.ended {
            return Ok(crate::applied::UpdateDrive::Ended(ended.clone()));
        }
        let staged = to_raw_staged(&self.record.submissions).map_err(UpdateError::Error)?;
        Ok(crate::applied::drive_update_resume_with_overrides(
            &self.prepared.template,
            Seed {
                now: self.prepared.now.clone(),
                defaults: self.prepared.configured_defaults.clone(),
                context: self.prepared.context.clone(),
            },
            &self.prepared.recorded,
            &staged,
            &self.overrides()?,
            self.record.reanswer,
        )?)
    }

    pub fn state(&self) -> Result<crate::operations::SessionState, SessionError> {
        use crate::{applied::UpdateDrive, operations::SessionState};
        Ok(match self.replay()? {
            UpdateDrive::Completed { completed, .. } => SessionState::Completed(completed),
            UpdateDrive::Ended(ended) => SessionState::Ended(ended),
            UpdateDrive::Ask {
                pending,
                rejections,
                submissions,
            } => self.response(&pending, rejections, &submissions)?,
        })
    }

    fn response(
        &self,
        pending: &crate::Pending<'_>,
        rejections: crate::Rejections,
        submissions: &[RawAnswers],
    ) -> Result<crate::operations::SessionState, SessionError> {
        let covered = crate::applied::covered_answers(
            pending,
            &self.prepared.recorded,
            submissions,
            &self.overrides()?,
            &self.prepared.configured_defaults,
            self.record.reanswer,
        )?;
        let mut document = crate::protocol::batch_document(
            pending.batch(),
            &crate::protocol::Context::new(&self.target, &self.record),
            (!rejections.is_empty()).then_some(&rejections),
        );
        for id in covered.keys() {
            document["schema"]["properties"]
                .as_object_mut()
                .expect("batch properties")
                .remove(id.as_str());
            document["schema"]["required"]
                .as_array_mut()
                .expect("batch required")
                .retain(|value| value.as_str() != Some(id.as_str()));
        }
        Ok(crate::operations::SessionState::Asking {
            document,
            rejections,
        })
    }

    #[allow(clippy::result_large_err)]
    pub fn answer(
        &mut self,
        answers: RawAnswers,
    ) -> Result<crate::operations::SessionState, SessionError> {
        let submitted;
        let rejected = match self.replay()? {
            crate::applied::UpdateDrive::Ask {
                pending,
                submissions,
                ..
            } => {
                let mut covered = crate::applied::covered_answers(
                    &pending,
                    &self.prepared.recorded,
                    &submissions,
                    &self.overrides()?,
                    &self.prepared.configured_defaults,
                    self.record.reanswer,
                )?;
                covered.extend(answers);
                submitted = covered;
                match pending.answer(submitted.clone()) {
                    Ok(crate::Interview::Ended(ended)) if ended.kind() == crate::EndKind::Abort => {
                        Some(crate::operations::SessionState::Ended(ended))
                    }
                    Ok(_) => None,
                    Err(crate::AnswerError::Rejected {
                        pending,
                        rejections,
                    }) => Some(self.response(&pending, rejections, &submissions)?),
                    Err(crate::AnswerError::Eval(e)) => return Err(e.into()),
                }
            }
            _ => {
                return Err(
                    UpdateError::Error("submission after completed interview".into()).into(),
                );
            }
        };
        if let Some(state) = rejected {
            if let crate::operations::SessionState::Ended(ended) = &state {
                self.ended = Some(ended.clone());
            }
            return Ok(state);
        }
        self.record.submissions.extend(to_stored(vec![submitted]));
        let state = self.state()?;
        if let crate::operations::SessionState::Ended(ended) = &state {
            self.ended = Some(ended.clone());
        }
        Ok(state)
    }

    /// Consume the loaded request while leaving its persisted record intact on preview/refusal/failure.
    #[allow(clippy::result_large_err)]
    pub fn apply(
        self,
        completed: &crate::Completed,
        dry_run: bool,
        runner: &dyn crate::hook::HookRunner,
        store: Option<&crate::staging::Store>,
    ) -> Result<(Merged, UpdateReport), SessionError> {
        self.apply_with_options(
            completed,
            UpdateOptions {
                dry_run,
                ..Default::default()
            },
            runner,
            store,
        )
    }

    /// Apply or preview with a force choice for this request only. Trust and
    /// persisted interview progress do not authorize replacement of region drift.
    #[allow(clippy::result_large_err)]
    pub fn apply_with_options(
        self,
        completed: &crate::Completed,
        options: UpdateOptions,
        runner: &dyn crate::hook::HookRunner,
        store: Option<&crate::staging::Store>,
    ) -> Result<(Merged, UpdateReport), SessionError> {
        let preview = effective_dry_run(options.dry_run, completed);
        let result = self
            .prepared
            .finish_with_options(&self.target, completed, options, runner)?;
        if !preview
            && matches!(result.0, Merged::Written { .. } | Merged::AlreadyCurrent)
            && let Some(store) = store
        {
            store.remove(&self.target)?;
        }
        Ok(result)
    }
}

/// Check repository/record cleanliness before resolving other run inputs.
pub fn open_project(target: &crate::staging::CanonicalTarget) -> Result<Project, UpdateError> {
    let project = match Project::open(target) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Err(UpdateError::NotGit);
        }
        Err(error) => return Err(UpdateError::Project(error)),
    };

    project
        .ensure_record_clean()
        .map_err(UpdateError::Project)?;
    match project.cleanliness() {
        Ok(Cleanliness::Clean) => {}
        Ok(Cleanliness::Dirty { .. }) => {
            return Err(UpdateError::DirtyTarget);
        }
        Err(error) => return Err(UpdateError::Project(error)),
    }
    Ok(project)
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("resolved", &self.resolved)
            .field("context", &self.context)
            .field("trusted", &self.trusted)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for UpdateSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateSession")
            .field("prepared", &self.prepared)
            .field("record", &self.record)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ResumeAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResumeAdmission")
            .field("base_application", &self.base_application)
            .finish_non_exhaustive()
    }
}
