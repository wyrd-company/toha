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
//! edits survive. This slice drives the headless script and agent route, where
//! `--answers` names the answers document; the interactive person route and the
//! staged agent continuation land in later slices.

use std::path::Path;

use indexmap::IndexMap;
use serde_json::{Value, json};
use toha::RawAnswer;
use toha::hook::ProcessRunner;
use toha::interview::Seed;
use toha::snapshot::{
    Action, Base, Change, CommitId, ConflictKind, FrozenNow, MergeOptions, Merged, Project,
    Revision, Snapshot, SnapshotInputs, UpdateDrive, drive_update, merge_apply,
};
use toha::template::Id;

use crate::{
    Dirs, EnvironmentDecision, Outcome, apply_environment_grant, build_context, load_template,
    read_answers_text, resolution, setup,
};

/// The base an update merges from.
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
    // 1. Open the target's repository (which strips drivers in memory).
    let (target, _store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let project = match Project::open(&target) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Outcome::Error(
                "the target is not inside a git repository; an update needs git".to_owned(),
            );
        }
        Err(error) => return Outcome::Error(error.to_string()),
    };

    // 2. Resolve the base snapshot and the template it named.
    let (merge_base, base_snapshot) = match &base {
        UpdateBase::From(prefix) => match project.find(prefix) {
            Ok(snapshot) => (Base::Snapshot(snapshot.clone()), Some(snapshot)),
            Err(error) => return Outcome::Error(error.to_string()),
        },
        UpdateBase::Baseline => (Base::Empty, None),
    };

    let cwd = match std::env::current_dir() {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let (config, registry) = match cli_context(dirs, &cwd) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // The template comes from the base snapshot for `--from`, and from the
    // command operand for `--baseline`.
    let resolved = match &base_snapshot {
        Some(snapshot) => {
            let formal = snapshot.template().to_owned();
            let commit = match snapshot.revision() {
                Revision::Commit(commit) => commit.as_str().to_owned(),
                Revision::Unversioned => String::new(),
            };
            match crate::cli::resolve::resume_template(
                &formal, &commit, false, &config, &registry, dirs, &cwd,
            ) {
                Ok(value) => value,
                Err(error) => return crate::resolve_error(error),
            }
        }
        None => {
            let Some(arg) = template_arg else {
                return Outcome::Error(
                    "apply --baseline needs a template operand: apply TEMPLATE PATH --baseline"
                        .to_owned(),
                );
            };
            match crate::cli::resolve::resolve_template(&arg, &config, &registry, dirs, &cwd) {
                Ok(value) => value,
                Err(error) => return crate::resolve_error(error),
            }
        }
    };
    let template = match load_template(&resolved) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };

    // 3. The frozen instant: carried from the base for `--from` so a
    //    date-rendering template stays byte-stable; the runtime instant for a
    //    baseline.
    let (_resolution, runtime_now) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let now = match &base_snapshot {
        Some(snapshot) => snapshot.generated().get().clone(),
        None => runtime_now,
    };

    // 4. The interview context, the same a fresh apply builds.
    let decision =
        EnvironmentDecision::grant_if_needed(apply_environment_grant(trust, &resolved, &template));
    let context = match build_context(&target, &resolved, &template, decision, false) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };

    // 5. The recorded submissions to replay, and the override answers.
    let recorded = match &base_snapshot {
        Some(snapshot) => match recorded_submissions(snapshot) {
            Ok(value) => value,
            Err(error) => return Outcome::Error(error),
        },
        None => Vec::new(),
    };
    let overrides = match &answers {
        Some(file) => {
            let text = match read_answers_text(file) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error),
            };
            match toha::protocol::verify_answers(&resolved.formal_name, &text) {
                Ok(value) => value,
                Err(error) => return Outcome::Error(error.to_string()),
            }
        }
        None => IndexMap::new(),
    };

    // 6. Drive the interview from the recorded answers. The context is cloned
    //    into the seed so an unfinished batch can still report through a record.
    let seed = Seed {
        now: now.clone(),
        defaults: IndexMap::new(),
        context: context.clone(),
    };
    let driven = match drive_update(&template, seed, &recorded, overrides, reanswer) {
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
        } => {
            // The script route cannot prompt. A batch the replay could not
            // complete — a new required question the base did not answer, a
            // recorded value the new version rejects, or `--reanswer` — is
            // reported as `questions` (exit 4), the same document the scripted
            // apply returns. Without `--answers` this is the person route, which
            // prompts; that continuation is not yet wired.
            if answers.is_none() {
                return Outcome::Error(
                    "the update has questions the recorded answers do not settle; \
                     the interactive person route is not yet available"
                        .to_owned(),
                );
            }
            let saved = crate::StagedRecord::new_with_context(
                context,
                resolved.commit.clone(),
                resolved.named,
                now.to_string(),
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

    // 7. Build the snapshot inputs and merge.
    let revision = match &base {
        UpdateBase::From(_) => base_snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision().clone())
            .unwrap_or(Revision::Unversioned),
        UpdateBase::Baseline => {
            if resolved.commit.is_empty() {
                Revision::Unversioned
            } else {
                match CommitId::parse(&resolved.commit) {
                    Ok(commit) => Revision::Commit(commit),
                    Err(error) => return Outcome::Error(error.to_string()),
                }
            }
        }
    };
    let generated = match &base_snapshot {
        Some(snapshot) => snapshot.generated().clone(),
        None => FrozenNow::new(seed_now(&completed)),
    };
    let inputs = SnapshotInputs {
        template: resolved.formal_name.clone(),
        revision,
        generated,
        submissions: submissions.into_iter().map(raw_to_values).collect(),
    };
    let options = MergeOptions {
        trusted: trust,
        dry_run,
    };

    match merge_apply(
        &project,
        merge_base,
        &template,
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

/// The frozen instant a completed interview projected for `now`.
fn seed_now(completed: &toha::Completed) -> jiff::Zoned {
    completed.now.clone()
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
