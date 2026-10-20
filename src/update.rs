// ---
// relationships:
//   implements: architecture
// ---
//! The template-update route: `apply --update` and `apply --baseline`.
//!
//! An update re-renders a template and merges the difference into the target,
//! three-way against a base. `--update` reads the base from a stored application and
//! replays its recorded answers through the unchanged interview engine; the
//! `--answers` envelope supplies overrides and `--reanswer` re-asks every answer.
//! `--baseline` merges the whole render against an empty base, so unrelated local
//! edits survive. An update refuses a dirty target, an application taken for another
//! target, and a template whose source is not the application's. At a
//! terminal a one-shot update prompts the questions the recorded answers do not
//! settle; the agent stages it with `stage --update` (see `update_stage`).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::{Value, json};
use std::io::IsTerminal;
use toha::context::InvocationContext;
use toha::hook::ProcessRunner;
use toha::interview::Seed;

use toha::applied::{
    Action, Application, Base, Change, Cleanliness, ConflictKind, MergeOptions, Merged, Project,
    RecordInputs, RepoPath, UpdateDrive, drive_update, drive_update_resume, merge_apply_reported,
};
use toha::protocol::Context;
use toha::template::{Id, Template};
use toha::{RawAnswer, RawAnswers};

use crate::cli::resolve::ResolvedTemplate;
use crate::{
    Dirs, EnvironmentDecision, Outcome, apply_environment_grant, build_context, load_template,
    read_answers_text, requirement_error, resolution, setup,
};

/// The base an update merges from.
#[derive(Clone)]
pub enum UpdateBase {
    /// The application matching this source and target.
    Update,
    /// An empty base: the whole render is merged, three-way against nothing.
    Baseline,
}

/// `apply --update PATH [--answers FILE] [--reanswer]` and `apply --baseline
/// TEMPLATE PATH --answers FILE`.
#[allow(clippy::too_many_arguments)]
pub fn run_update(
    base: UpdateBase,
    template_arg: Option<String>,
    path: &Path,
    answers: Option<String>,
    reanswer: bool,
    dry_run: bool,
    trust: bool,
    input: &[PathBuf],
    parts: toha::parts::Selection,
    dirs: &Dirs,
) -> Outcome {
    let (target, _store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let mut prep = match prepare(
        &base,
        template_arg,
        &target,
        dirs,
        trust,
        PrepareMode::Apply,
        false,
        answers.is_some(),
    ) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // The override answers from `--answers`, identity-checked against the
    // resolved template.
    let overrides = match &answers {
        Some(file) => {
            let text = match read_answers_text(file) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error),
            };
            match toha::protocol::document_answers(&prep.resolved.formal_name, &text) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error.to_string()),
            }
        }
        None => IndexMap::new(),
    };

    if let Err(error) = crate::admit_parts(&mut prep.template, parts, None, &overrides) {
        return Outcome::Usage(error);
    }
    if let Err(error) =
        toha::document::prepare(&mut prep.template, target.as_path(), input, &overrides)
    {
        let saved = crate::StagedRecord::new_with_context(
            prep.context.clone(),
            prep.resolved.commit.clone(),
            prep.resolved.named,
            prep.now.to_string(),
            vec![],
        );
        let context = crate::context(&target, &saved);
        return crate::document_input_error(error, answers.as_ref().map(|_| &context));
    }

    // The person route (no `--answers`, a terminal) prompts the questions the
    // recorded answers do not settle, then merges; the script and agent route
    // reports them.
    if answers.is_none() && std::io::stdin().is_terminal() {
        return person_update(prep, &target, reanswer, dry_run);
    }

    let seed = Seed {
        now: prep.now.clone(),
        defaults: prep.configured_defaults.clone(),
        context: prep.context.clone(),
    };
    let driven = match drive_update(&prep.template, seed, &prep.recorded, overrides, reanswer) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let (completed, submissions) = match driven {
        UpdateDrive::Completed {
            completed,
            submissions,
        } => (completed, submissions),
        UpdateDrive::Ask {
            pending,
            rejections,
            ..
        } => {
            // The script route cannot prompt. A batch the replay could not
            // complete is reported as `questions` (exit 4), the same document the
            // scripted apply returns. Without `--answers` this is the person
            // route; its `stage`/`continue` continuation is the update staging
            // flow, so a one-shot person update with unsettled questions refuses.
            if answers.is_none() {
                return Outcome::Error(
                    "the update has questions the recorded answers do not settle; \
                     stage it with `stage --update` to answer them"
                        .to_owned(),
                );
            }
            let saved = crate::StagedRecord::new_with_context(
                prep.context.clone(),
                prep.resolved.commit.clone(),
                prep.resolved.named,
                prep.now.to_string(),
                vec![],
            );
            let ctx = crate::context(&target, &saved);
            return Outcome::Document(
                toha::protocol::batch_document(pending.batch(), &ctx, Some(&rejections)),
                4,
            );
        }
        UpdateDrive::Ended(ended) => {
            eprintln!("{}", crate::guidance::flow_ended(&ended));
            return Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ));
        }
    };

    // A flow `dry-run` composes with the CLI `--dry-run` by union.
    let dry_run = effective_dry_run(dry_run, &completed);
    finish_merge(
        prep,
        &target,
        completed,
        submissions,
        dry_run,
        answers.is_some(),
    )
}

/// The person route for a one-shot update: prompt the questions the recorded
/// answers do not settle — re-driving through the adapter each round so a batch
/// the recorded answers cover is never prompted — then merge.
fn person_update(
    prep: Prepared,
    target: &toha::staging::CanonicalTarget,
    reanswer: bool,
    dry_run: bool,
) -> Outcome {
    let mut staged: Vec<IndexMap<Id, RawAnswer>> = Vec::new();
    loop {
        let seed = Seed {
            now: prep.now.clone(),
            defaults: prep.configured_defaults.clone(),
            context: prep.context.clone(),
        };
        let driven =
            match drive_update_resume(&prep.template, seed, &prep.recorded, &staged, reanswer) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error.to_string()),
            };
        match driven {
            UpdateDrive::Completed {
                completed,
                submissions,
            } => {
                // A flow `dry-run` composes with the CLI `--dry-run` by union.
                let dry_run = effective_dry_run(dry_run, &completed);
                return finish_merge(prep, target, completed, submissions, dry_run, false);
            }
            UpdateDrive::Ask { pending, .. } => {
                let submission =
                    match crate::terminal::prompt_batch(&pending, &mut crate::terminal::InquireAsk)
                    {
                        Ok(submission) => submission,
                        Err(error) => return Outcome::Error(error),
                    };
                staged.push(submission);
            }
            UpdateDrive::Ended(ended) => {
                eprintln!("{}", crate::guidance::flow_ended(&ended));
                return Outcome::Error(format!(
                    "the template ended the interview: {:?}",
                    ended.kind()
                ));
            }
        }
    }
}

/// The shared pieces an update route needs after resolving its base and
/// template: the opened project, the merge base, the resolved template and its
/// render context, the base's recorded submissions, and the trust grant.
pub(crate) struct Prepared {
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
}

/// Open the project, resolve the base application and template, enforce the update
/// preconditions and identity (17), and build the render context and trust
/// grant. Shared by the one-shot update route and the staged update flow.
/// How `prepare` grants the environment and computes trust: an `apply` merges
/// now; a `stage` captures the grant for later batches.
pub(crate) enum PrepareMode {
    Apply,
    Stage,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    base: &UpdateBase,
    template_arg: Option<String>,
    target: &toha::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    mode: PrepareMode,
    interactive: bool,
    scripted: bool,
) -> Result<Prepared, Outcome> {
    let project = match Project::open(target) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Err(Outcome::Error(
                "the target is not inside a git repository; an update needs git".to_owned(),
            ));
        }
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };

    project
        .ensure_record_clean()
        .map_err(|e| Outcome::Error(e.to_string()))?;
    match project.cleanliness() {
        Ok(Cleanliness::Clean) => {}
        Ok(Cleanliness::Dirty { .. }) => {
            return Err(Outcome::Error(
                "the target has uncommitted changes; commit or discard them before an update"
                    .into(),
            ));
        }
        Err(error) => return Err(Outcome::Error(error.to_string())),
    }
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;
    let arg = template_arg.ok_or_else(|| {
        Outcome::Error(
            "apply --update and --baseline need a template operand: apply TEMPLATE PATH --update"
                .into(),
        )
    })?;
    let resolved = crate::cli::resolve::resolve_template(&arg, &config, &registry, dirs, &cwd)
        .map_err(crate::resolve_error)?;
    let base_application = match base {
        UpdateBase::Update => Some(
            project
                .application(&source_of(&resolved.formal_name))
                .map_err(|e| Outcome::Error(e.to_string()))?,
        ),
        UpdateBase::Baseline => None,
    };
    let (merge_base, base_application, prior_part_question) = restore_merge_base(
        &project,
        base_application,
        target,
        &config,
        &registry,
        dirs,
        &cwd,
    )?;
    // The scripted route (`--answers`) reports a missing requirement as its
    // `requirement` error document; the other routes report it as text.
    let template = match Template::load(&resolved.folder) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };
    if let Err(error) = toha::requirement::check(&template) {
        return Err(if scripted {
            requirement_error(&error)
        } else {
            Outcome::Error(error.to_string())
        });
    }

    let (resolution, runtime_now) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };
    let (configured_defaults, _) = resolution.into_flat_defaults();
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
            let trusted = apply_environment_grant(trust, &resolved, &template);
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
    let context = match build_context(target, &resolved, &template, decision, interactive) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };

    let recorded = match &base_application {
        Some(application) => candidate_submissions(application, &template, prior_part_question)
            .map_err(Outcome::Error)?,
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
    })
}

/// Restore the pieces a staged-update resume needs from its record: reopen the
/// project, re-resolve the template and base application, restore the identity-
/// bearing context (not rebuilt), and recompute the trust grant for the apply.
/// The target's cleanliness is re-checked so a merge never runs on a dirty tree.
pub(crate) fn resume_prepared(
    saved: &crate::StagedRecord,
    base: &UpdateBase,
    target: &toha::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
) -> Result<Prepared, Outcome> {
    let project = match Project::open(target) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Err(Outcome::Error(
                "the target is not inside a git repository; an update needs git".to_owned(),
            ));
        }
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };
    project
        .ensure_record_clean()
        .map_err(|e| Outcome::Error(e.to_string()))?;
    match project.cleanliness() {
        Ok(Cleanliness::Clean) => {}
        Ok(Cleanliness::Dirty { .. }) => {
            return Err(Outcome::Error(
                "the target has uncommitted changes; commit or discard them before an update"
                    .to_owned(),
            ));
        }
        Err(error) => return Err(Outcome::Error(error.to_string())),
    }

    let base_application = match base {
        UpdateBase::Update => {
            let application = project
                .application(&source_of(&saved.template))
                .map_err(|e| Outcome::Error(e.to_string()))?;
            let pinned: Value =
                serde_json::from_str(saved.base.as_deref().unwrap_or("")).map_err(|_| {
                    Outcome::Error(
                        "invalid staged application; run stage TEMPLATE PATH --update".into(),
                    )
                })?;
            if application.to_value() != pinned {
                return Err(Outcome::Error(
                    "the applied record entry changed; run stage TEMPLATE PATH --update".into(),
                ));
            }
            Some(application)
        }
        UpdateBase::Baseline => None,
    };
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;
    let (merge_base, base_application, prior_part_question) = restore_merge_base(
        &project,
        base_application,
        target,
        &config,
        &registry,
        dirs,
        &cwd,
    )?;

    let resolved = match crate::cli::resolve::resume_template(
        &saved.template,
        &saved.commit,
        saved.named,
        &config,
        &registry,
        dirs,
        &cwd,
    ) {
        Ok(value) => value,
        Err(error) => return Err(crate::resolve_error(error)),
    };
    let mut template = match load_template(&resolved) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };

    template.part_selection = saved.part_selection.clone();
    let (resolution, _) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };
    let (configured_defaults, _) = resolution.into_flat_defaults();
    let now = match saved.now.parse::<jiff::Zoned>() {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };
    let context = match saved.invocation_context(target) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };
    let trusted = apply_environment_grant(trust, &resolved, &template);
    let recorded = match &base_application {
        Some(application) => candidate_submissions(application, &template, prior_part_question)
            .map_err(Outcome::Error)?,
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
    })
}

/// A staged record's stored submissions as the interview's raw answers.
pub(crate) fn to_raw_staged(
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
pub(crate) fn effective_dry_run(dry_run: bool, completed: &toha::Completed) -> bool {
    dry_run || matches!(completed.step(), toha::Step::Plan { apply: false })
}

/// Build the application inputs from a completed update interview and merge them
/// into the project, mapping the merge result to an `Outcome`.
pub(crate) fn finish_merge(
    prep: Prepared,
    target: &toha::staging::CanonicalTarget,
    completed: toha::Completed,
    _submissions: Vec<IndexMap<Id, RawAnswer>>,
    dry_run: bool,
    scripted: bool,
) -> Outcome {
    // The result documents carry the invocation context; build it from `prep`
    // before the merge consumes the base.
    let saved = crate::StagedRecord::new_with_context(
        prep.context.clone(),
        prep.resolved.commit.clone(),
        prep.resolved.named,
        prep.now.to_string(),
        vec![],
    );
    let ctx = crate::context(target, &saved);
    let base_id = prep
        .base_application
        .as_ref()
        .map(|application| application.target().to_string());

    // Record the revision actually rendered, including a newly selected content
    // revision when the caller names the current template explicitly.
    let revision = match crate::cli::resolve::applied_revision(
        &prep.resolved.formal_name,
        &prep.resolved.commit,
    ) {
        Ok(revision) => revision,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let metadata = crate::parts_output::metadata(
        prep.base_application.as_ref().map(|a| a.revision()),
        &revision,
        prep.base_application
            .as_ref()
            .map(|a| a.parts.as_slice())
            .unwrap_or(&[]),
        &prep.template.selected_parts(&completed),
    );
    let inputs = RecordInputs {
        template: prep.resolved.formal_name.clone(),
        revision,
    };
    let options = MergeOptions {
        trusted: prep.trusted,
        dry_run,
    };
    let runner: &dyn toha::hook::HookRunner = if scripted {
        &crate::ScriptedRunner
    } else {
        &ProcessRunner
    };
    match merge_apply_reported(
        &prep.project,
        prep.merge_base,
        &prep.template,
        &completed,
        inputs,
        options,
        runner,
    ) {
        Ok((merged, report)) => {
            let outcome = match merged_outcome(merged, &ctx, base_id.as_deref()) {
                Outcome::Document(document, code) => {
                    Outcome::Document(crate::with_application(document, &metadata), code)
                }
                other => other,
            };
            if scripted {
                outcome
            } else {
                crate::update_output::text_outcome(outcome, &report, dry_run)
            }
        }
        Err(error) => Outcome::Error(error.to_string()),
    }
}

/// Load the command context, mapping a resolve error to an `Outcome`.
fn cli_context(
    dirs: &Dirs,
    cwd: &Path,
) -> Result<(toha::config::Config, toha::registry::Registry), Outcome> {
    crate::cli::resolve::load_context(dirs, cwd).map_err(crate::resolve_error)
}

/// A formal name's source identity: the name without its `@reference`, the same
/// value an application records as its `source`.
fn source_of(formal: &str) -> String {
    toha::applied::source_identity(formal)
}

/// Only ordinary answers are replayed into a candidate. Documents are admitted
/// from this run's inputs; recorded documents belong to the merge base alone.
/// Parts replay only if the applied revision actually asked a part question.
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

/// Map a merge result to the caller's result document. An update reports the
/// canonical `applied`/`planned`/`already-current` document with its `merge`
/// member (the changed paths and any conflicts) and, when it saved one, a
/// `record` member.
fn merged_outcome(merged: Merged, ctx: &Context, _base_id: Option<&str>) -> Outcome {
    match merged {
        Merged::AlreadyCurrent => {
            let document = toha::protocol::shell("already-current", ctx);
            Outcome::Document(document, 0)
        }
        // An untrusted update previews the plan and writes nothing, like a dry run.
        Merged::NeedsTrust => {
            let mut document = toha::protocol::shell("planned", ctx);
            document["files"] = json!([]);
            document["messages"] = json!([]);
            document["hooks"] = json!([]);
            document["trusted"] = json!(false);
            document["merge"] = json!({ "changes": [], "conflicted": [] });
            Outcome::Document(document, 0)
        }
        Merged::Planned { changes } => {
            let mut document = toha::protocol::shell("planned", ctx);
            document["files"] = json!([]);
            document["messages"] = json!([]);
            document["hooks"] = json!([]);
            document["trusted"] = json!(true);
            document["merge"] = merge_member(&changes, &[]);
            Outcome::Document(document, 0)
        }
        Merged::Written {
            record,
            changes,
            conflicted,
        } => {
            let mut document = toha::protocol::shell("applied", ctx);
            document["files"] = json!([]);
            document["messages"] = json!([]);
            document["hooks"] = json!([]);
            document["record"] = json!({ "path": record });
            document["merge"] = merge_member(&changes, &conflicted);
            Outcome::Document(document, 0)
        }
    }
}

/// The `merge` member: the changed paths with their actions, and the conflicted
/// paths the operator must resolve.
fn merge_member(changes: &[Change], conflicted: &[RepoPath]) -> Value {
    json!({
        "changes": changes.iter().map(change_json).collect::<Vec<_>>(),
        "conflicted": conflicted.iter().map(|path| path.to_string()).collect::<Vec<_>>(),
    })
}

/// One merge change as `{ path, action, conflict? }`.
fn change_json(change: &Change) -> Value {
    let (action, conflict) = match change.action {
        Action::Added => ("added", None),
        Action::Updated => ("updated", None),
        Action::Merged => ("merged", None),
        Action::Deleted => ("deleted", None),
        Action::NotPreviewed => ("not-previewed", None),
        Action::Conflicted(kind) => ("conflicted", Some(conflict_name(kind))),
    };
    let mut object = json!({ "path": change.path.to_string(), "action": action });
    if let Some(conflict) = conflict {
        object["conflict"] = json!(conflict);
    }
    object
}

/// The stable name of a conflict kind.
fn conflict_name(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => "content",
        ConflictKind::AddAdd => "add-add",
        ConflictKind::ModifyDelete => "modify-delete",
        ConflictKind::FileDirectory => "file-directory",
        ConflictKind::Binary => "binary",
        ConflictKind::Driver => "driver",
    }
}

#[allow(clippy::too_many_arguments)]
fn restore_merge_base(
    project: &Project,
    application: Option<Application>,
    target: &toha::staging::CanonicalTarget,
    config: &toha::config::Config,
    registry: &toha::registry::Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<(Base, Option<Application>, bool), Outcome> {
    let Some(mut application) = application else {
        return Ok((Base::Empty, None, false));
    };
    let failure_source = application.source().to_owned();
    let failure_revision = application.revision().clone();
    let failure = |message: String| {
        Outcome::Error(format!(
            "cannot render merge base for {failure_source} at {failure_revision:?}: {message}; use apply TEMPLATE PATH --baseline to re-baseline"
        ))
    };
    if matches!(application.revision(), toha::applied::Revision::Unversioned) {
        return Err(failure("the template has no reproducible revision".into()));
    }
    let resolved = crate::cli::resolve::resume_application_template(
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
        toha::context::SelectedTemplate::new(
            template.name.clone(),
            application.template().into(),
            vec![],
            resolved.source.clone(),
        ),
        toha::context::HostFacts::new(String::new(), String::new(), None, None, vec![]),
        toha::context::ExecutionFacts::new(false, false),
        toha::context::EnvironmentSnapshot::Unavailable,
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
        toha::Interview::start(&template, seed).map_err(|e| failure(e.to_string()))?;
    let mut remaining = recorded_submissions(&application, &template)
        .map_err(failure)?
        .into_iter()
        .flatten()
        .collect::<IndexMap<_, _>>();
    let completed = loop {
        match interview {
            toha::Interview::Complete(completed) => break completed,
            toha::Interview::Ended(_) => {
                return Err(failure("the recorded interview ended".into()));
            }
            toha::Interview::Asking(pending) => {
                let values = pending
                    .batch()
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        toha::interview::Item::Prompt(p) => {
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
        toha::Plan::build(&template, &completed, target).map_err(|e| failure(e.to_string()))?;
    project
        .restore_base(&mut application, plan)
        .map_err(|e| failure(e.to_string()))?;
    Ok((
        Base::Application(application.clone()),
        Some(application),
        template.part_question().is_some(),
    ))
}
