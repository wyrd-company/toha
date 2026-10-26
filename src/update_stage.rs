// ---
// relationships:
//   implements: architecture
// ---
//! Command input, prompting and output for the shared update session lifecycle.
use crate::staging::{CanonicalTarget, StagedRecord, Store};
use crate::update::{self, PrepareMode, UpdateBase};
use crate::{AgentOutcome, Dirs, Outcome, cli::guidance, context, instructions, setup};
use serde_json::Value;
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};
pub use toha::updates::is_staged_update;
use toha::{operations::SessionState, updates::UpdateSession};

#[allow(clippy::too_many_arguments)]
pub fn stage(
    base: UpdateBase,
    template: Option<String>,
    path: &Path,
    output: Option<Option<String>>,
    reanswer: bool,
    trust: bool,
    input: &[PathBuf],
    parts: toha::parts::Selection,
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    match store.load(&target) {
        Ok(Some(_)) => {
            return Outcome::Error(format!(
                "an interview is already staged at {}; abort it before staging an update",
                path.display()
            ));
        }
        Ok(None) => {}
        Err(e) => return Outcome::Error(e.to_string()),
    }
    let interactive = output.is_none();
    if interactive && !io::stdin().is_terminal() {
        return Outcome::Error(guidance::no_terminal(
            template.as_deref().unwrap_or("the template"),
            path,
        ));
    }
    let prep = match update::prepare(
        &base,
        template,
        &target,
        dirs,
        trust,
        PrepareMode::Stage,
        interactive,
        false,
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut session = match UpdateSession::start(
        prep,
        target,
        base,
        reanswer,
        parts,
        input,
        &Default::default(),
    ) {
        Ok(v) => v,
        Err(e) => return session_error(e),
    };
    if interactive {
        return person(&mut session, &store, path);
    }
    if let Err(e) = session.persist(&store) {
        return session_error(e);
    }
    report_initial(&session, output, path)
}
fn session_error(e: toha::updates::SessionError) -> Outcome {
    match e {
        toha::updates::SessionError::Operation(e) => crate::operation_error(e, None),
        e => Outcome::Error(e.to_string()),
    }
}
/// The CLI presents the full batch, including recorded defaults. Embedded callers
/// may use SessionState's projection containing only uncovered questions.
fn questions(session: &UpdateSession) -> Result<Value, toha::updates::SessionError> {
    let rejections = match session.state()? {
        SessionState::Asking { rejections, .. } => rejections,
        _ => vec![],
    };
    full_batch(session, &rejections)
}
fn full_batch(
    session: &UpdateSession,
    rejections: &toha::Rejections,
) -> Result<Value, toha::updates::SessionError> {
    match session.replay()? {
        toha::applied::UpdateDrive::Ask { pending, .. } => Ok(toha::protocol::batch_document(
            pending.batch(),
            &context(&session.target, &session.record),
            (!rejections.is_empty()).then_some(rejections),
        )),
        _ => unreachable!("asking update session"),
    }
}
fn report_initial(session: &UpdateSession, output: Option<Option<String>>, path: &Path) -> Outcome {
    report_with_questions(session, output, path, true)
}
fn report(session: &UpdateSession, output: Option<Option<String>>, path: &Path) -> Outcome {
    report_with_questions(session, output, path, false)
}
fn report_with_questions(
    session: &UpdateSession,
    output: Option<Option<String>>,
    path: &Path,
    initial: bool,
) -> Outcome {
    match session.state() {
        Ok(SessionState::Completed(_)) => Outcome::Agent {
            document: None,
            instructions: instructions(&AgentOutcome::Complete, path),
            code: 0,
        },
        Ok(SessionState::Ended(ended)) => {
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ))
        }
        Ok(SessionState::Asking { .. }) => {
            let mut doc = match questions(session) {
                Ok(v) => v,
                Err(e) => return session_error(e),
            };
            if initial {
                doc.as_object_mut()
                    .expect("batch document")
                    .entry("errors")
                    .or_insert_with(|| serde_json::json!({}));
            }
            let instructions = instructions(
                &AgentOutcome::Batch {
                    template: &session.record.template,
                },
                path,
            );
            match output.flatten() {
                Some(file) => {
                    match std::fs::write(file, serde_json::to_vec_pretty(&doc).unwrap()) {
                        Ok(()) => Outcome::Agent {
                            document: None,
                            instructions,
                            code: 4,
                        },
                        Err(e) => Outcome::Error(e.to_string()),
                    }
                }
                None => Outcome::Agent {
                    document: Some(doc),
                    instructions,
                    code: 4,
                },
            }
        }
        Err(e) => session_error(e),
        _ => unreachable!("known session state"),
    }
}
fn resume(
    saved: StagedRecord,
    target: CanonicalTarget,
    dirs: &Dirs,
    trust: bool,
) -> Result<UpdateSession, Outcome> {
    let base = toha::updates::base_of(&saved).map_err(Outcome::Error)?;
    let prep = update::resume_prepared(&saved, &base, &target, dirs, trust)?;
    UpdateSession::from_prepared(prep, saved, target).map_err(session_error)
}
pub fn continue_staged(
    saved: StagedRecord,
    path: &Path,
    answers: Option<String>,
    input: &[PathBuf],
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let mut session = match resume(saved, target, dirs, false) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if answers.is_some() {
        match session.state() {
            Ok(SessionState::Completed(_)) => {
                return Outcome::Error(guidance::complete_answers_unused(path));
            }
            Ok(SessionState::Ended(ended)) => {
                if let Err(e) = session.persist(&store) {
                    return session_error(e);
                }
                return Outcome::Document(
                    toha::protocol::ended_document(
                        &ended,
                        &context(&session.target, &session.record),
                    ),
                    0,
                );
            }
            Err(e) => return session_error(e),
            _ => {}
        }
    }
    let incoming = match answers.as_deref().map(crate::read_answers_text).transpose() {
        Ok(Some(text)) => match toha::protocol::document_answers(&session.record.template, &text) {
            Ok(v) => v,
            Err(e) => return Outcome::Error(e.to_string()),
        },
        Ok(None) => Default::default(),
        Err(e) => return Outcome::Error(e),
    };
    if let Err(e) = session.admit_documents(input, &incoming) {
        return session_error(e);
    }
    if answers.is_none() {
        return person(&mut session, &store, path);
    }
    match session.state() {
        Ok(SessionState::Completed(_)) if incoming.is_empty() => {
            if let Err(e) = session.persist(&store) {
                return session_error(e);
            }
            return report(&session, None, path);
        }
        Ok(SessionState::Completed(_)) => {
            return Outcome::Error(guidance::complete_answers_unused(path));
        }
        Ok(SessionState::Ended(ended)) => {
            return Outcome::Document(
                toha::protocol::ended_document(&ended, &context(&session.target, &session.record)),
                0,
            );
        }
        Err(e) => return session_error(e),
        _ => {}
    }
    let state = match session.answer(incoming) {
        Ok(v) => v,
        Err(e) => return session_error(e),
    };
    if let SessionState::Asking { rejections, .. } = &state
        && !rejections.is_empty()
    {
        return match full_batch(&session, rejections) {
            Ok(document) => Outcome::Document(document, 4),
            Err(e) => session_error(e),
        };
    }
    if let Err(e) = session.persist(&store) {
        return session_error(e);
    }
    if let SessionState::Ended(ended) = state {
        eprintln!("{}", guidance::flow_ended(&ended));
        return Outcome::Error(format!(
            "the template ended the interview: {:?}",
            ended.kind()
        ));
    }
    report(&session, None, path)
}
fn person(session: &mut UpdateSession, store: &Store, path: &Path) -> Outcome {
    loop {
        if let Err(e) = session.persist(store) {
            return session_error(e);
        }
        match session.state() {
            Ok(SessionState::Completed(_)) => {
                return Outcome::Written(vec![guidance::continue_complete(path)]);
            }
            Ok(SessionState::Ended(ended)) => {
                eprintln!("{}", guidance::flow_ended(&ended));
                return Outcome::Document(
                    toha::protocol::ended_document(
                        &ended,
                        &context(&session.target, &session.record),
                    ),
                    0,
                );
            }
            Err(e) => return session_error(e),
            _ => {}
        }
        if !io::stdin().is_terminal() {
            return Outcome::Error(guidance::continue_no_terminal(path));
        }
        let submission = match session.replay() {
            Ok(toha::applied::UpdateDrive::Ask { pending, .. }) => {
                match crate::terminal::prompt_batch(&pending, &mut crate::terminal::InquireAsk) {
                    Ok(v) => v,
                    Err(e) => return Outcome::Error(e),
                }
            }
            Err(e) => return session_error(e),
            _ => unreachable!("asking session"),
        };
        match session.answer(submission) {
            Ok(SessionState::Ended(ended)) => {
                if let Err(e) = session.persist(store) {
                    return session_error(e);
                }
                eprintln!("{}", guidance::flow_ended(&ended));
                return Outcome::Document(
                    toha::protocol::ended_document(
                        &ended,
                        &context(&session.target, &session.record),
                    ),
                    0,
                );
            }
            Err(e) => return session_error(e),
            _ => {}
        }
    }
}
pub fn apply_staged(
    saved: StagedRecord,
    path: &Path,
    options: toha::updates::UpdateOptions,
    trust: bool,
    input: &[PathBuf],
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let mut session = match resume(saved, target, dirs, trust) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = session.admit_documents(input, &Default::default()) {
        return session_error(e);
    }
    if let Err(e) = session.persist(&store) {
        return session_error(e);
    }
    match session.state() {
        Ok(SessionState::Completed(done)) => {
            update::finish_merge(session, done, options, false, Some(&store))
        }
        Ok(SessionState::Asking { .. }) => {
            let document = match questions(&session) {
                Ok(v) => v,
                Err(e) => return session_error(e),
            };
            Outcome::Agent {
                document: Some(document),
                instructions: instructions(
                    &AgentOutcome::ApplyIncomplete {
                        template: &session.record.template,
                    },
                    path,
                ),
                code: 4,
            }
        }
        Ok(SessionState::Ended(ended)) => {
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Error(format!(
                "the template ended the interview: {:?}",
                ended.kind()
            ))
        }
        Err(e) => session_error(e),
        _ => unreachable!("known session state"),
    }
}
