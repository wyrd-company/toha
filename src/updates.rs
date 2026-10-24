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
/// The base an update merges from.
#[derive(Clone)]
pub enum UpdateBase {
    /// The application matching this source and target.
    Update,
    /// An empty base: the whole render is merged, three-way against nothing.
    Baseline,
}
pub enum UpdateError {
    Error(String),
    Resolve(crate::resolve::ResolveError),
    Requirements(crate::MissingRequirements),
    Project(crate::applied::ProjectError),
    NotGit,
    DirtyTarget,
    StaleApplication,
    InvalidApplication,
    MissingTemplate,
    Base {
        template: String,
        revision: crate::applied::Revision,
        reason: String,
    },
}
impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error(e) => e.fmt(f),
            Self::Resolve(e) => write!(f, "{e:?}"),
            Self::Requirements(e) => e.fmt(f),
            Self::Project(e) => e.fmt(f),
            Self::NotGit=>f.write_str("the target is not inside a git repository; an update needs git"),
            Self::DirtyTarget=>f.write_str("the target has uncommitted changes; commit or discard them before an update"),
            Self::StaleApplication=>f.write_str("the applied record entry changed; run stage TEMPLATE PATH --update"),
            Self::InvalidApplication=>f.write_str("invalid staged application; run stage TEMPLATE PATH --update"),
            Self::MissingTemplate=>f.write_str("apply --update and --baseline need a template operand: apply TEMPLATE PATH --update"),
            Self::Base{template,revision,reason}=>write!(f,"cannot render merge base for {template} at {revision:?}: {reason}; use apply TEMPLATE PATH --baseline to re-baseline"),
        }
    }
}
impl std::fmt::Debug for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for UpdateError {}
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
pub enum PrepareMode {
    Apply,
    Stage,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare(
    base: &UpdateBase,
    template_arg: Option<String>,
    target: &crate::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    mode: PrepareMode,
    interactive: bool,
    environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
) -> Result<Prepared, UpdateError> {
    let project = open_project(target)?;
    let cwd = environment.cwd;
    let config = environment.config;
    let registry = environment.registry;
    let arg = template_arg.ok_or(UpdateError::MissingTemplate)?;
    let resolved = crate::resolve::resolve_template(&arg, config, registry, dirs, cwd)
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
        target,
        &resolved,
        &template,
        decision,
        environment.host.clone(),
        crate::context::ExecutionFacts::new(environment.is_admin, interactive),
        environment.source,
    ) {
        Ok(value) => value,
        Err(error) => return Err(UpdateError::Error(error)),
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
    .map_err(|e| failure(format!("{e:?}")))?;
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
        if self.context.target() != target {
            return Err(MergeError::Changed);
        }
        let revision =
            crate::resolve::applied_revision(&self.resolved.formal_name, &self.resolved.commit)
                .map_err(|e| MergeError::Io(e.to_string()))?;
        crate::applied::merge_apply_reported(
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
                dry_run: effective_dry_run(dry_run, completed),
            },
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
        serde_json::from_str::<Value>(value).map_err(|_| {
            "invalid staged application; run stage TEMPLATE PATH --update".to_owned()
        })?;
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
    pub prepared: Prepared,
    pub record: StagedRecord,
    pub target: crate::staging::CanonicalTarget,
}

#[derive(Debug, thiserror::Error)]
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
    /// Admit current document inputs once and pin the base before returning a first batch.
    #[allow(clippy::result_large_err)]
    pub fn start(
        mut prepared: Prepared,
        target: crate::staging::CanonicalTarget,
        base: UpdateBase,
        reanswer: bool,
        parts: crate::parts::Selection,
        inputs: &[std::path::PathBuf],
        answers: &RawAnswers,
    ) -> Result<Self, SessionError> {
        if prepared.context.target() != &target {
            return Err(crate::operations::OperationError::Input(
                "prepared update does not match the target".into(),
            )
            .into());
        }
        crate::document::prepare(&mut prepared.template, target.as_path(), inputs, answers)
            .map_err(crate::operations::OperationError::from)?;
        crate::operations::admit_parts(&mut prepared.template, parts, None, answers)
            .map_err(UpdateError::Error)?;
        let base = stored_base(&base, prepared.base_application.as_ref());
        let mut record = record(&prepared, &base, reanswer, vec![]);
        let driven = crate::applied::drive_update(
            &prepared.template,
            Seed {
                now: prepared.now.clone(),
                defaults: prepared.configured_defaults.clone(),
                context: prepared.context.clone(),
            },
            &prepared.recorded,
            answers.clone(),
            reanswer,
        )?;
        match driven {
            crate::applied::UpdateDrive::Completed { submissions, .. }
            | crate::applied::UpdateDrive::Ask { submissions, .. } => {
                record.submissions = to_stored(submissions);
                crate::operations::save_document_answers(&prepared.template, &mut record);
            }
            crate::applied::UpdateDrive::Ended(_) => {}
        }
        Ok(Self {
            prepared,
            record,
            target,
        })
    }

    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn resume(
        mut record: StagedRecord,
        target: crate::staging::CanonicalTarget,
        paths: &Dirs,
        trust: bool,
        inputs: &[std::path::PathBuf],
        incoming: &RawAnswers,
        environment: &mut Environment<impl crate::context::FixedEnvironmentSource>,
    ) -> Result<Self, SessionError> {
        let base = base_of(&record).map_err(UpdateError::Error)?;
        let mut prepared = resume_prepared(&record, &base, &target, paths, trust, environment)?;
        crate::operations::prepare_staged_documents(
            &mut prepared.template,
            target.as_path(),
            inputs,
            incoming,
            &mut record,
        )
        .map_err(crate::operations::OperationError::from)?;
        Ok(Self {
            prepared,
            record,
            target,
        })
    }

    fn replay(&self) -> Result<crate::applied::UpdateDrive<'_>, SessionError> {
        let staged = to_raw_staged(&self.record.submissions).map_err(UpdateError::Error)?;
        Ok(crate::applied::drive_update_resume(
            &self.prepared.template,
            Seed {
                now: self.prepared.now.clone(),
                defaults: self.prepared.configured_defaults.clone(),
                context: self.prepared.context.clone(),
            },
            &self.prepared.recorded,
            &staged,
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
            &self.prepared.configured_defaults,
            self.record.reanswer,
        )?;
        let mut document = crate::protocol::batch_document(
            pending.batch(),
            &crate::protocol::Context::new(&self.target, &self.record),
            Some(&rejections),
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
                    &self.prepared.configured_defaults,
                    self.record.reanswer,
                )?;
                covered.extend(answers);
                submitted = covered;
                match pending.answer(submitted.clone()) {
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
            return Ok(state);
        }
        self.record.submissions.extend(to_stored(vec![submitted]));
        self.state()
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
        let preview = effective_dry_run(dry_run, completed);
        let result = self
            .prepared
            .finish(&self.target, completed, preview, runner)?;
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
