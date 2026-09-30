// ---
// relationships:
//   implements: architecture
// ---
//! The template-update route: `apply --from ID` and `apply --baseline`.
//!
//! An update re-renders a template and merges the difference into the target,
//! three-way against a base. `--from` reads the base from a stored snapshot and
//! replays its recorded answers through the unchanged interview engine; the
//! `--answers` envelope supplies overrides and `--reanswer` re-asks every answer.
//! `--baseline` merges the whole render against an empty base, so unrelated local
//! edits survive. An update refuses a dirty target, a snapshot taken for another
//! target, and a template whose source is not the snapshot's (identity 17). At a
//! terminal a one-shot update prompts the questions the recorded answers do not
//! settle; the agent stages it with `stage --from` (see `update_stage`).

use std::path::Path;

use indexmap::IndexMap;
use serde_json::{Value, json};
use std::io::IsTerminal;
use toha::context::InvocationContext;
use toha::hook::ProcessRunner;
use toha::interview::Seed;

use toha::snapshot::{
    Action, Base, Change, Cleanliness, CommitId, ConflictKind, FrozenNow, MergeOptions, Merged,
    Project, Revision, Snapshot, SnapshotInputs, UpdateDrive, drive_update, drive_update_resume,
    merge_apply,
};
use toha::template::{Id, Template};
use toha::{RawAnswer, RawAnswers};

use crate::cli::resolve::ResolvedTemplate;
use crate::{
    Dirs, EnvironmentDecision, Outcome, apply_environment_grant, build_context, load_template,
    read_answers_text, resolution, setup,
};

/// The base an update merges from.
#[derive(Clone)]
pub enum UpdateBase {
    /// A stored snapshot, named by id or id prefix.
    From(String),
    /// An empty base: the whole render is merged, three-way against nothing.
    Baseline,
}

/// `apply --from ID PATH [--answers FILE] [--reanswer]` and `apply --baseline
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
    dirs: &Dirs,
) -> Outcome {
    let (target, _store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let prep = match prepare(
        &base,
        template_arg,
        &target,
        dirs,
        trust,
        PrepareMode::Apply,
        false,
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
            match toha::protocol::verify_answers(&prep.resolved.formal_name, &text) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error.to_string()),
            }
        }
        None => IndexMap::new(),
    };

    // The person route (no `--answers`, a terminal) prompts the questions the
    // recorded answers do not settle, then merges; the script and agent route
    // reports them.
    if answers.is_none() && std::io::stdin().is_terminal() {
        return person_update(prep, &base, reanswer, dry_run);
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
                     stage it with `stage --from` to answer them"
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

    finish_merge(prep, &base, completed, submissions, dry_run)
}

/// The person route for a one-shot update: prompt the questions the recorded
/// answers do not settle — re-driving through the adapter each round so a batch
/// the recorded answers cover is never prompted — then merge.
fn person_update(prep: Prepared, base: &UpdateBase, reanswer: bool, dry_run: bool) -> Outcome {
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
            } => return finish_merge(prep, base, completed, submissions, dry_run),
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
    pub base_snapshot: Option<Snapshot>,
    pub resolved: ResolvedTemplate,
    pub template: Template,
    pub now: jiff::Zoned,
    pub context: InvocationContext,
    pub configured_defaults: RawAnswers,
    pub recorded: Vec<IndexMap<Id, RawAnswer>>,
    pub trusted: bool,
}

/// Open the project, resolve the base snapshot and template, enforce the update
/// preconditions and identity (17), and build the render context and trust
/// grant. Shared by the one-shot update route and the staged update flow.
/// How `prepare` grants the environment and computes trust: an `apply` merges
/// now; a `stage` captures the grant for later batches.
pub(crate) enum PrepareMode {
    Apply,
    Stage,
}

pub(crate) fn prepare(
    base: &UpdateBase,
    template_arg: Option<String>,
    target: &toha::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
    mode: PrepareMode,
    interactive: bool,
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

    let (merge_base, base_snapshot) = match base {
        UpdateBase::From(prefix) => match project.find(prefix) {
            Ok(snapshot) => (Base::Snapshot(snapshot.clone()), Some(snapshot)),
            Err(error) => return Err(Outcome::Error(error.to_string())),
        },
        UpdateBase::Baseline => (Base::Empty, None),
    };

    // Preconditions: the target must be clean, and a base snapshot must have been
    // taken for this same target (identity 17, target side).
    if let Some(snapshot) = &base_snapshot {
        if snapshot.target() != project.target() {
            return Err(Outcome::Error(format!(
                "the snapshot was taken for target {:?}, not {:?}",
                snapshot.target().as_str(),
                project.target().as_str()
            )));
        }
    }
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

    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;

    let resolved = match (&base_snapshot, &template_arg) {
        (Some(snapshot), None) => {
            let formal = snapshot.template().to_owned();
            let commit = match snapshot.revision() {
                Revision::Commit(commit) => commit.as_str().to_owned(),
                Revision::Unversioned => String::new(),
            };
            match crate::cli::resolve::resume_template(
                &formal, &commit, false, &config, &registry, dirs, &cwd,
            ) {
                Ok(value) => value,
                Err(error) => return Err(crate::resolve_error(error)),
            }
        }
        (Some(snapshot), Some(arg)) => {
            let resolved =
                match crate::cli::resolve::resolve_template(arg, &config, &registry, dirs, &cwd) {
                    Ok(value) => value,
                    Err(error) => return Err(crate::resolve_error(error)),
                };
            if source_of(&resolved.formal_name) != snapshot.source() {
                return Err(Outcome::Error(format!(
                    "the template's source {:?} is not the snapshot's source {:?}",
                    source_of(&resolved.formal_name),
                    snapshot.source()
                )));
            }
            resolved
        }
        (None, Some(arg)) => {
            match crate::cli::resolve::resolve_template(arg, &config, &registry, dirs, &cwd) {
                Ok(value) => value,
                Err(error) => return Err(crate::resolve_error(error)),
            }
        }
        (None, None) => {
            return Err(Outcome::Error(
                "apply --baseline needs a template operand: apply TEMPLATE PATH --baseline"
                    .to_owned(),
            ));
        }
    };
    let template = match load_template(&resolved) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };

    let (resolution, runtime_now) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };
    let (configured_defaults, _) = resolution.into_flat_defaults();
    let now = match &base_snapshot {
        Some(snapshot) => snapshot.generated().get().clone(),
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

    let recorded = match &base_snapshot {
        Some(snapshot) => recorded_submissions(snapshot).map_err(Outcome::Error)?,
        None => Vec::new(),
    };

    Ok(Prepared {
        project,
        merge_base,
        base_snapshot,
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
/// project, re-resolve the template and base snapshot, restore the identity-
/// bearing context (not rebuilt), and recompute the trust grant for the apply.
/// The target's cleanliness is re-checked so a merge never runs on a dirty tree.
pub(crate) fn resume_prepared(
    saved: &crate::StagedRecord,
    base: &UpdateBase,
    target: &toha::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
) -> Result<(Prepared, Vec<IndexMap<Id, RawAnswer>>), Outcome> {
    let project = match Project::open(target) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Err(Outcome::Error(
                "the target is not inside a git repository; an update needs git".to_owned(),
            ));
        }
        Err(error) => return Err(Outcome::Error(error.to_string())),
    };
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

    let (merge_base, base_snapshot) = match base {
        UpdateBase::From(id) => match project.find(id) {
            Ok(snapshot) => (Base::Snapshot(snapshot.clone()), Some(snapshot)),
            Err(error) => return Err(Outcome::Error(error.to_string())),
        },
        UpdateBase::Baseline => (Base::Empty, None),
    };
    if let Some(snapshot) = &base_snapshot {
        if snapshot.target() != project.target() {
            return Err(Outcome::Error(format!(
                "the snapshot was taken for target {:?}, not {:?}",
                snapshot.target().as_str(),
                project.target().as_str()
            )));
        }
    }

    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;
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
    let template = match load_template(&resolved) {
        Ok(value) => value,
        Err(error) => return Err(Outcome::Error(error)),
    };

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
    let recorded = match &base_snapshot {
        Some(snapshot) => recorded_submissions(snapshot).map_err(Outcome::Error)?,
        None => Vec::new(),
    };
    let staged = to_raw_staged(&saved.submissions).map_err(Outcome::Error)?;

    Ok((
        Prepared {
            project,
            merge_base,
            base_snapshot,
            resolved,
            template,
            now,
            context,
            configured_defaults,
            recorded,
            trusted,
        },
        staged,
    ))
}

/// A staged record's stored submissions as the interview's raw answers.
fn to_raw_staged(
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

/// Build the snapshot inputs from a completed update interview and merge them
/// into the project, mapping the merge result to an `Outcome`.
pub(crate) fn finish_merge(
    prep: Prepared,
    base: &UpdateBase,
    completed: toha::Completed,
    submissions: Vec<IndexMap<Id, RawAnswer>>,
    dry_run: bool,
) -> Outcome {
    let revision = match base {
        UpdateBase::From(_) => prep
            .base_snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision().clone())
            .unwrap_or(Revision::Unversioned),
        UpdateBase::Baseline => {
            if prep.resolved.commit.is_empty() {
                Revision::Unversioned
            } else {
                match CommitId::parse(&prep.resolved.commit) {
                    Ok(commit) => Revision::Commit(commit),
                    Err(error) => return Outcome::Error(error.to_string()),
                }
            }
        }
    };
    let generated = match &prep.base_snapshot {
        Some(snapshot) => snapshot.generated().clone(),
        None => FrozenNow::new(completed.now.clone()),
    };
    let inputs = SnapshotInputs {
        template: prep.resolved.formal_name.clone(),
        revision,
        generated,
        submissions: submissions.into_iter().map(raw_to_values).collect(),
    };
    let options = MergeOptions {
        trusted: prep.trusted,
        dry_run,
    };
    match merge_apply(
        &prep.project,
        prep.merge_base,
        &prep.template,
        &completed,
        inputs,
        options,
        &ProcessRunner,
    ) {
        Ok(merged) => merged_outcome(merged, dry_run),
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
/// value a snapshot records as its `source`.
fn source_of(formal: &str) -> &str {
    formal.split_once('@').map_or(formal, |(source, _)| source)
}

/// The base snapshot's recorded submissions, as the interview's raw answers.
fn recorded_submissions(snapshot: &Snapshot) -> Result<Vec<IndexMap<Id, RawAnswer>>, String> {
    snapshot
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
        .collect()
}

/// One replayed submission as the id→value map a snapshot records.
fn raw_to_values(submission: IndexMap<Id, RawAnswer>) -> IndexMap<Id, Value> {
    submission
        .into_iter()
        .map(|(id, raw)| (id, raw.0))
        .collect()
}

/// Map a merge result to the caller's outcome document.
fn merged_outcome(merged: Merged, dry_run: bool) -> Outcome {
    match merged {
        Merged::AlreadyCurrent => {
            Outcome::Document(json!({ "update": "already-current", "changes": [] }), 0)
        }
        Merged::NeedsTrust => Outcome::Error(
            "the update's template has hooks; re-run with --trust to run them".to_owned(),
        ),
        Merged::Planned { changes } => Outcome::Document(
            json!({ "update": "planned", "changes": changes_json(&changes) }),
            0,
        ),
        Merged::Written {
            snapshot,
            changes,
            conflicted,
        } => {
            let _ = dry_run;
            Outcome::Document(
                json!({
                    "update": "applied",
                    "snapshot": snapshot.to_string(),
                    "changes": changes_json(&changes),
                    "conflicted": conflicted
                        .iter()
                        .map(|path| path.to_string())
                        .collect::<Vec<_>>(),
                }),
                0,
            )
        }
    }
}

/// The changes of a merge as a JSON array, each naming its path and action.
fn changes_json(changes: &[Change]) -> Vec<Value> {
    changes
        .iter()
        .map(|change| {
            json!({
                "path": change.path.to_string(),
                "action": action_name(change.action),
            })
        })
        .collect()
}

/// The stable name of a merge action for the result document.
fn action_name(action: Action) -> Value {
    match action {
        Action::Added => json!("added"),
        Action::Updated => json!("updated"),
        Action::Merged => json!("merged"),
        Action::Deleted => json!("deleted"),
        Action::NotPreviewed => json!("not-previewed"),
        Action::Conflicted(kind) => json!({ "conflicted": conflict_name(kind) }),
    }
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
