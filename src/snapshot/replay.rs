// ---
// relationships:
//   implements: architecture
// ---
//! The update replay adapter.
//!
//! An update does not reuse the staged-replay walk, which treats a rejected
//! recorded answer as fatal, submits recorded batches whole, and cannot override
//! an answer. This adapter drives the unchanged interview engine from the base
//! snapshot's recorded submissions: it turns them into a per-id queue in recorded
//! order (so a looped question replays its values in order), applies the route's
//! overrides first, and for each pending batch takes the next queued value for
//! every question, submitting when every required question has one. A recorded
//! value the new version rejects is removed and the question treated as
//! unanswered, with the rejection kept for the message. At the first batch it
//! cannot complete it hands the batch to the route, whose defaults are the
//! recorded raw values. `--reanswer` skips the auto-submission entirely.

use std::collections::{HashMap, VecDeque};

use indexmap::IndexMap;

use crate::interview::{
    AnswerError, CheckError, Completed, Ended, EvalError, Interview, Item, Pending, RawAnswer,
    RawAnswers, Rejections, Seed,
};
use crate::template::{Id, Template};

/// The outcome of driving the interview from recorded submissions.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Replay<'a> {
    /// The interview completed; `submissions` are the raw values actually
    /// submitted, in batch order, for the new snapshot to record.
    Completed {
        completed: Completed,
        submissions: Vec<IndexMap<Id, RawAnswer>>,
    },
    /// The first batch replay could not complete. The route takes over: the
    /// person is prompted with the recorded values as defaults; the script route
    /// reports `questions`; the agent route stages the interview.
    Ask {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
    },
    /// A flow `stop`/`abort` ended the interview.
    Ended(Ended),
}

/// Drive the interview for an update from the base's `recorded` submissions, with
/// route `overrides` (already verified) applied first. `seed` carries the frozen
/// instant and context; its defaults are set here to the recorded/override
/// values so the route falls back to them.
pub(crate) fn replay<'a>(
    template: &'a Template,
    mut seed: Seed,
    recorded: &[IndexMap<Id, RawAnswer>],
    overrides: RawAnswers,
    reanswer: bool,
) -> Result<Replay<'a>, EvalError> {
    // A per-id queue of recorded values, in recorded order.
    let mut queue: HashMap<Id, VecDeque<RawAnswer>> = HashMap::new();
    for submission in recorded {
        for (id, raw) in submission {
            queue.entry(id.clone()).or_default().push_back(raw.clone());
        }
    }
    // The route's overrides replace an id's queued value; an override for an id
    // the new version does not ask is never consumed, so it is dropped.
    for (id, raw) in &overrides {
        queue.insert(id.clone(), VecDeque::from([raw.clone()]));
    }

    // The route falls back to the recorded (then overridden) values as defaults.
    let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
    for submission in recorded {
        for (id, raw) in submission {
            defaults.insert(id.clone(), raw.clone());
        }
    }
    for (id, raw) in overrides {
        defaults.insert(id, raw);
    }
    seed.defaults = defaults;

    let mut interview = Interview::start(template, seed)?;
    let mut submissions = Vec::new();
    loop {
        let pending = match interview {
            Interview::Complete(completed) => {
                return Ok(Replay::Completed {
                    completed,
                    submissions,
                });
            }
            Interview::Ended(ended) => return Ok(Replay::Ended(ended)),
            Interview::Asking(pending) => pending,
        };

        // `--reanswer` offers every batch to the route with the recorded defaults.
        if reanswer {
            return Ok(Replay::Ask {
                pending: Box::new(pending),
                rejections: Vec::new(),
            });
        }

        let prompts: Vec<(Id, bool)> = pending
            .batch()
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Prompt(prompt) => Some((prompt.id.clone(), prompt.constraints.required)),
                Item::Message(_) => None,
            })
            .collect();

        let mut raw: RawAnswers = IndexMap::new();
        let mut rejections = Rejections::new();
        let mut complete = true;
        for (id, required) in &prompts {
            match queue.get(id).and_then(|values| values.front()).cloned() {
                Some(value) => match pending.check(id, value.clone()) {
                    Ok(_) => {
                        raw.insert(id.clone(), value);
                    }
                    Err(CheckError::Rejected(rejection)) => {
                        rejections.push(rejection);
                        if let Some(values) = queue.get_mut(id) {
                            values.pop_front();
                        }
                        if *required {
                            complete = false;
                        }
                    }
                    Err(CheckError::Eval(error)) => return Err(error),
                },
                None => {
                    if *required {
                        complete = false;
                    }
                }
            }
        }

        if !complete {
            return Ok(Replay::Ask {
                pending: Box::new(pending),
                rejections,
            });
        }

        // Consume the queued values this batch used, then submit it.
        for id in raw.keys() {
            if let Some(values) = queue.get_mut(id) {
                values.pop_front();
            }
        }
        submissions.push(raw.clone());
        match pending.answer(raw) {
            Ok(next) => interview = next,
            Err(AnswerError::Rejected {
                pending,
                rejections,
            }) => {
                return Ok(Replay::Ask {
                    pending: Box::new(pending),
                    rejections,
                });
            }
            Err(AnswerError::Eval(error)) => return Err(error),
        }
    }
}

/// The outcome of driving an update interview, for a crate caller and the
/// command driver: the three cases of the crate-private adapter, carried over
/// already-public interview types. The adapter itself ([`Replay`], [`replay`])
/// stays crate-private; this is the seam a driver consumes.
#[allow(clippy::large_enum_variant)]
pub enum UpdateDrive<'a> {
    /// The interview completed; `submissions` are the raw values actually
    /// submitted, in batch order, for the new snapshot to record.
    Completed {
        completed: Completed,
        submissions: Vec<IndexMap<Id, RawAnswer>>,
    },
    /// The first batch the replay could not complete. The route takes over: the
    /// person is prompted with the recorded values as defaults; the script route
    /// reports `questions`; the agent route stages the interview.
    Ask {
        pending: Pending<'a>,
        rejections: Rejections,
    },
    /// A flow `stop`/`abort` ended the interview.
    Ended(Ended),
}

/// Drive an update interview from a base snapshot's `recorded` submissions, with
/// the route's `overrides` applied first and `reanswer` handing every batch to
/// the route. This is the driver-facing seam over the crate-private replay
/// adapter: it keeps the adapter's queue mechanics internal and returns the
/// typed outcome the person, script, and agent routes branch on.
pub fn drive_update<'a>(
    template: &'a Template,
    seed: Seed,
    recorded: &[IndexMap<Id, RawAnswer>],
    overrides: RawAnswers,
    reanswer: bool,
) -> Result<UpdateDrive<'a>, EvalError> {
    Ok(
        match replay(template, seed, recorded, overrides, reanswer)? {
            Replay::Completed {
                completed,
                submissions,
            } => UpdateDrive::Completed {
                completed,
                submissions,
            },
            Replay::Ask {
                pending,
                rejections,
            } => UpdateDrive::Ask {
                pending: *pending,
                rejections,
            },
            Replay::Ended(ended) => UpdateDrive::Ended(ended),
        },
    )
}

#[cfg(test)]
mod tests;
