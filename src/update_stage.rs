// ---
// relationships:
//   implements: architecture
// ---
//! The staged template-update flow: `stage --from`/`--baseline`/`--reanswer`,
//! then `continue PATH` and `apply PATH`.
//!
//! Staging an update drives the update replay adapter from the base snapshot's
//! recorded answers and saves a [`StagedRecord`] that holds the base, the
//! `reanswer` flag, and the batches submitted so far. Without `--async` the
//! person route prompts the unsettled questions, saving each batch; with it the
//! agent route reports the first unsettled batch. A `continue` resumes through
//! the adapter, replaying the staged submissions and answering the next batch.
//! `apply PATH` resumes a completed staged update and merges it from the base.
//! The record is identity-bearing and keyed by the canonical target, so recovery
//! is unchanged.

use std::io::{self, IsTerminal};
use std::path::Path;

use indexmap::IndexMap;
use serde_json::Value;
use toha::RawAnswer;
use toha::interview::Seed;
use toha::snapshot::{Snapshot, SnapshotId, UpdateDrive, drive_update, drive_update_resume};
use toha::template::Id;

use crate::cli::guidance;
use crate::staging::{CanonicalTarget, StagedRecord, Store};
use crate::update::{self, PrepareMode, Prepared, UpdateBase};
use crate::{AgentOutcome, Dirs, Outcome, context, instructions, setup};

/// A staged baseline update stores this reserved base marker. It begins with
/// `@`, which a ULID (Crockford base32) never contains, so it never collides
/// with a snapshot id and is validated as the one non-id value on resume.
pub(crate) const BASELINE_MARKER: &str = "@baseline";

/// Whether a staged record is an update (it carries a base).
pub fn is_staged_update(saved: &StagedRecord) -> bool {
    saved.base.is_some()
}

/// The stored base value for a record: the full snapshot id, or the baseline
/// marker.
fn stored_base(base: &UpdateBase, base_snapshot: Option<&Snapshot>) -> String {
    match base {
        UpdateBase::From(_) => base_snapshot
            .expect("--from resolves a snapshot")
            .id()
            .to_string(),
        UpdateBase::Baseline => BASELINE_MARKER.to_owned(),
    }
}

/// Reconstruct the update base a record staged, validating the stored value as a
/// snapshot id or the baseline marker.
fn base_of(saved: &StagedRecord) -> Result<UpdateBase, String> {
    let value = saved.base.as_deref().ok_or("record is not an update")?;
    if value == BASELINE_MARKER {
        Ok(UpdateBase::Baseline)
    } else {
        value
            .parse::<SnapshotId>()
            .map(|_| UpdateBase::From(value.to_owned()))
            .map_err(|err| format!("staged base {value:?} is not a snapshot id: {err}"))
    }
}

/// The interview's raw submissions, in batch order.
type Submissions = Vec<IndexMap<Id, RawAnswer>>;

/// The interview's raw submissions as the string-keyed maps a record stores.
fn to_stored(submissions: Vec<IndexMap<Id, RawAnswer>>) -> Vec<IndexMap<String, Value>> {
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
fn record(
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
    record
}

/// `stage --from ID`/`--baseline` `[--reanswer]` `PATH` `[--async FILE]`. The
/// person route (no `--async`) prompts and saves each batch; the agent route
/// reports the batch.
#[allow(clippy::too_many_arguments)]
pub fn stage(
    base: UpdateBase,
    template_arg: Option<String>,
    path: &Path,
    async_out: Option<Option<String>>,
    reanswer: bool,
    trust: bool,
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    match store.load(&target) {
        Ok(Some(_)) => {
            return Outcome::Error(format!(
                "an interview is already staged at {}; abort it before staging an update",
                path.display()
            ));
        }
        Ok(None) => {}
        Err(error) => return Outcome::Error(error.to_string()),
    }
    // Without `--async` this is the person route, which prompts; with it, the
    // agent route, which reports the batch. The person route needs a terminal.
    let interactive = async_out.is_none();
    if interactive && !io::stdin().is_terminal() {
        return Outcome::Error(guidance::no_terminal(
            template_arg.as_deref().unwrap_or("the template"),
            path,
        ));
    }
    let prep = match update::prepare(
        &base,
        template_arg,
        &target,
        dirs,
        trust,
        PrepareMode::Stage,
        interactive,
    ) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let base_value = stored_base(&base, prep.base_snapshot.as_ref());
    let seed = Seed {
        now: prep.now.clone(),
        defaults: prep.configured_defaults.clone(),
        context: prep.context.clone(),
    };
    let driven = match drive_update(
        &prep.template,
        seed,
        &prep.recorded,
        IndexMap::new(),
        reanswer,
    ) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };

    match driven {
        UpdateDrive::Completed { submissions, .. } => {
            let saved = record(&prep, &base_value, reanswer, submissions);
            if let Err(error) = store.save(&target, &saved) {
                return Outcome::Error(error.to_string());
            }
            // The update needs no questions; point the caller to `apply PATH`.
            if interactive {
                Outcome::Written(vec![guidance::continue_complete(path)])
            } else {
                Outcome::Agent {
                    document: None,
                    instructions: instructions(&AgentOutcome::Complete, path),
                    code: 0,
                }
            }
        }
        UpdateDrive::Ask {
            pending,
            rejections,
            submissions,
        } => {
            let saved = record(&prep, &base_value, reanswer, submissions.clone());
            if let Err(error) = store.save(&target, &saved) {
                return Outcome::Error(error.to_string());
            }
            if interactive {
                // The person route prompts the remaining questions, saving each
                // batch, then points to apply.
                return continue_person(saved, prep, submissions, &target, &store, path);
            }
            let ctx = context(&target, &saved);
            let document = toha::protocol::batch_document(pending.batch(), &ctx, Some(&rejections));
            report_batch(document, async_out, path, &prep.resolved.formal_name)
        }
        UpdateDrive::Ended(ended) => {
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ))
        }
    }
}

/// Emit a staged batch on the agent route: write it to FILE when one is given,
/// else to standard output, with the answer instructions.
fn report_batch(
    document: Value,
    async_out: Option<Option<String>>,
    path: &Path,
    formal: &str,
) -> Outcome {
    let instructions = instructions(&AgentOutcome::Batch { template: formal }, path);
    match async_out {
        Some(Some(file)) => {
            if let Err(error) = std::fs::write(
                &file,
                serde_json::to_vec_pretty(&document).expect("JSON value"),
            ) {
                return Outcome::Error(error.to_string());
            }
            Outcome::Agent {
                document: None,
                instructions,
                code: 4,
            }
        }
        _ => Outcome::Agent {
            document: Some(document),
            instructions,
            code: 4,
        },
    }
}

/// Resolve the pieces a staged-update resume needs, restoring the identity-
/// bearing context from the record (not rebuilding it) and re-resolving the
/// template and base snapshot. Returns the `Prepared` a merge consumes and the
/// reconstructed base.
fn resume_prep(
    saved: &StagedRecord,
    target: &CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
) -> Result<(Prepared, UpdateBase, Submissions), Outcome> {
    let base = base_of(saved).map_err(Outcome::Error)?;
    let (prep, staged) = update::resume_prepared(saved, &base, target, dirs, trust)?;
    Ok((prep, base, staged))
}

/// `continue PATH [FILE]` for a staged update.
pub fn continue_staged(
    saved: StagedRecord,
    path: &Path,
    answers: Option<String>,
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let (prep, _base, staged) = match resume_prep(&saved, &target, dirs, false) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // The person route prompts each remaining batch, re-driving through the
    // adapter so a batch the recorded answers cover is not prompted.
    let Some(answers_file) = answers else {
        return continue_person(saved, prep, staged, &target, &store, path);
    };

    // Agent route: drive to the pending batch and answer one document.
    let seed = Seed {
        now: prep.now.clone(),
        defaults: prep.configured_defaults.clone(),
        context: prep.context.clone(),
    };
    let driven = match drive_update_resume(
        &prep.template,
        seed,
        &prep.recorded,
        &staged,
        saved.reanswer,
    ) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let pending = match driven {
        // `continue PATH FILE` on a complete interview refuses before reading the
        // document.
        UpdateDrive::Completed { .. } => {
            return Outcome::Error(guidance::complete_answers_unused(path));
        }
        UpdateDrive::Ask { pending, .. } => pending,
        UpdateDrive::Ended(ended) => {
            if let Err(error) = store.remove(&target) {
                return Outcome::Error(error.to_string());
            }
            eprintln!("{}", guidance::flow_ended(&ended));
            return Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ));
        }
    };

    let text = match crate::read_answers_text(&answers_file) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    match toha::protocol::answer_document_once(&prep.resolved.formal_name, pending, &text) {
        Ok(toha::protocol::DocumentStep::Accepted { submission, .. }) => {
            advance(saved, prep, staged, submission, &target, &store, path)
        }
        Ok(toha::protocol::DocumentStep::Rejected {
            pending,
            rejections,
        }) => {
            let ctx = context(&target, &saved);
            Outcome::Document(
                toha::protocol::batch_document(pending.batch(), &ctx, Some(&rejections)),
                4,
            )
        }
        Err(error) => Outcome::Error(error.to_string()),
    }
}

/// Append an accepted agent submission to the staged update, re-drive to the
/// next batch or completion, save, and report.
fn advance(
    mut saved: StagedRecord,
    prep: Prepared,
    mut staged: Submissions,
    submission: IndexMap<String, Value>,
    target: &CanonicalTarget,
    store: &Store,
    path: &Path,
) -> Outcome {
    let raw: IndexMap<Id, RawAnswer> = match submission
        .iter()
        .map(|(key, value)| Id::parse(key).map(|id| (id, RawAnswer(value.clone()))))
        .collect::<Result<_, _>>()
    {
        Ok(value) => value,
        Err(message) => return Outcome::Error(message),
    };
    staged.push(raw);
    let seed = Seed {
        now: prep.now.clone(),
        defaults: prep.configured_defaults.clone(),
        context: prep.context.clone(),
    };
    let driven = match drive_update_resume(
        &prep.template,
        seed,
        &prep.recorded,
        &staged,
        saved.reanswer,
    ) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    match driven {
        UpdateDrive::Completed { submissions, .. } => {
            saved.submissions = to_stored(submissions);
            if let Err(error) = store.save(target, &saved) {
                return Outcome::Error(error.to_string());
            }
            Outcome::Agent {
                document: None,
                instructions: instructions(&AgentOutcome::Complete, path),
                code: 0,
            }
        }
        UpdateDrive::Ask {
            pending,
            submissions,
            ..
        } => {
            saved.submissions = to_stored(submissions);
            if let Err(error) = store.save(target, &saved) {
                return Outcome::Error(error.to_string());
            }
            let ctx = context(target, &saved);
            Outcome::Agent {
                document: Some(toha::protocol::batch_document(pending.batch(), &ctx, None)),
                instructions: instructions(
                    &AgentOutcome::Batch {
                        template: &prep.resolved.formal_name,
                    },
                    path,
                ),
                code: 4,
            }
        }
        UpdateDrive::Ended(ended) => {
            if let Err(error) = store.remove(target) {
                return Outcome::Error(error.to_string());
            }
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ))
        }
    }
}

/// `continue PATH` (person route): re-drive the staged update to the next batch,
/// prompt it, and save, until the update completes. Re-driving each round keeps
/// the borrowed pending local, and replays the staged submissions so a batch the
/// recorded answers cover is never prompted.
fn continue_person(
    mut saved: StagedRecord,
    prep: Prepared,
    mut staged: Submissions,
    target: &CanonicalTarget,
    store: &Store,
    path: &Path,
) -> Outcome {
    use crate::terminal;
    if !io::stdin().is_terminal() {
        return Outcome::Error(guidance::continue_no_terminal(path));
    }
    loop {
        let seed = Seed {
            now: prep.now.clone(),
            defaults: prep.configured_defaults.clone(),
            context: prep.context.clone(),
        };
        let driven = match drive_update_resume(
            &prep.template,
            seed,
            &prep.recorded,
            &staged,
            saved.reanswer,
        ) {
            Ok(value) => value,
            Err(error) => return Outcome::Error(error.to_string()),
        };
        match driven {
            UpdateDrive::Completed { submissions, .. } => {
                saved.submissions = to_stored(submissions);
                if let Err(error) = store.save(target, &saved) {
                    return Outcome::Error(error.to_string());
                }
                // The staged update is complete; point to apply.
                return Outcome::Written(vec![guidance::continue_complete(path)]);
            }
            UpdateDrive::Ended(ended) => {
                if let Err(error) = store.remove(target) {
                    return Outcome::Error(error.to_string());
                }
                eprintln!("{}", guidance::flow_ended(&ended));
                return Outcome::Document(
                    toha::protocol::ended_document(&ended, &context(target, &saved)),
                    0,
                );
            }
            UpdateDrive::Ask {
                pending,
                submissions,
                ..
            } => {
                // Save the auto-submitted batches so far, then prompt this one.
                saved.submissions = to_stored(submissions);
                if let Err(error) = store.save(target, &saved) {
                    return Outcome::Error(error.to_string());
                }
                let submission = match terminal::prompt_batch(&pending, &mut terminal::InquireAsk) {
                    Ok(submission) => submission,
                    Err(error) => return Outcome::Error(error),
                };
                staged.push(submission);
            }
        }
    }
}

/// `apply PATH` for a staged update: resume it and, when complete, merge from
/// the base, removing the record only once that merge actually writes;
/// when incomplete, report the pending batch.
pub fn apply_staged(
    saved: StagedRecord,
    path: &Path,
    dry_run: bool,
    trust: bool,
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error),
    };
    let (prep, base, staged) = match resume_prep(&saved, &target, dirs, trust) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let seed = Seed {
        now: prep.now.clone(),
        defaults: prep.configured_defaults.clone(),
        context: prep.context.clone(),
    };
    let driven = match drive_update_resume(
        &prep.template,
        seed,
        &prep.recorded,
        &staged,
        saved.reanswer,
    ) {
        Ok(value) => value,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    match driven {
        UpdateDrive::Completed {
            completed,
            submissions,
        } => {
            // A flow `dry-run` composes with the CLI `--dry-run` by union; the
            // staged-record retention gate below must use the same effective
            // value as the merge itself.
            let dry_run = update::effective_dry_run(dry_run, &completed);
            let outcome =
                update::finish_merge(prep, &base, &target, completed, submissions, dry_run);
            // A successful, non-preview merge consumes the staged interview; a
            // dry run previews it and leaves the staged interview in place so the
            // real apply remains available.
            if outcome.is_success() && !dry_run {
                if let Err(error) = store.remove(&target) {
                    return Outcome::Error(error.to_string());
                }
            }
            outcome
        }
        UpdateDrive::Ask { pending, .. } => {
            // Not yet complete: report the pending batch, as `apply PATH` does for
            // a normal staged interview.
            let ctx = context(&target, &saved);
            Outcome::Agent {
                document: Some(toha::protocol::batch_document(pending.batch(), &ctx, None)),
                instructions: instructions(
                    &AgentOutcome::ApplyIncomplete {
                        template: &saved.template,
                    },
                    path,
                ),
                code: 4,
            }
        }
        UpdateDrive::Ended(ended) => {
            if let Err(error) = store.remove(&target) {
                return Outcome::Error(error.to_string());
            }
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ))
        }
    }
}
