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
use toha::hook::ProcessRunner;
use toha::interview::Seed;

use toha::RawAnswer;
use toha::applied::{
    Action, Change, ConflictKind, Merged, RepoPath, UpdateDrive, drive_update, drive_update_resume,
};
use toha::protocol::Context;
use toha::template::Id;

use crate::{Dirs, Outcome, read_answers_text, requirement_error, setup};

pub use toha::updates::UpdateBase;

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

pub use toha::updates::{PrepareMode, Prepared};
fn update_error(error: toha::updates::UpdateError, scripted: bool) -> Outcome {
    match error {
        toha::updates::UpdateError::Error(e) => Outcome::Error(e),
        toha::updates::UpdateError::Resolve(e) => crate::resolve_error(e),
        toha::updates::UpdateError::Requirements(e) => {
            if scripted {
                requirement_error(&e)
            } else {
                Outcome::Error(e.to_string())
            }
        }
        other => Outcome::Error(other.to_string()),
    }
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
    toha::updates::open_project(target).map_err(|e| update_error(e, false))?;
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;
    let now = crate::runtime_now().map_err(Outcome::Error)?;
    let mut source = crate::HostEnvironmentSource;
    let mut environment = toha::updates::Environment {
        config: &config,
        registry: &registry,
        cwd: &cwd,
        now,
        host: toha::context::HostFacts::capture(),
        is_admin: crate::is_admin(),
        source: &mut source,
    };
    let result = toha::updates::prepare(
        base,
        template_arg,
        target,
        dirs,
        trust,
        mode,
        interactive,
        &mut environment,
    )
    .map_err(|e| update_error(e, scripted))?;
    crate::report_config_warnings(&result.warnings);
    Ok(result)
}
pub(crate) fn resume_prepared(
    saved: &crate::StagedRecord,
    base: &UpdateBase,
    target: &toha::staging::CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
) -> Result<Prepared, Outcome> {
    toha::updates::open_project(target).map_err(|e| update_error(e, false))?;
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli_context(dirs, &cwd)?;
    let now = crate::runtime_now().map_err(Outcome::Error)?;
    let mut source = crate::HostEnvironmentSource;
    let mut environment = toha::updates::Environment {
        config: &config,
        registry: &registry,
        cwd: &cwd,
        now,
        host: toha::context::HostFacts::capture(),
        is_admin: crate::is_admin(),
        source: &mut source,
    };
    let result = toha::updates::resume_prepared(saved, base, target, dirs, trust, &mut environment)
        .map_err(|e| update_error(e, false))?;
    crate::report_config_warnings(&result.warnings);
    Ok(result)
}
pub use toha::updates::{effective_dry_run, to_raw_staged};

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
    let runner: &dyn toha::hook::HookRunner = if scripted {
        &crate::ScriptedRunner
    } else {
        &ProcessRunner
    };
    match prep.finish(target, &completed, dry_run, runner) {
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
