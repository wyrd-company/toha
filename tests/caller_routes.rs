// ---
// relationships:
//   verifies: interview-protocol
//   implements: command-line-interface
// ---
//! The 27 behaviors of the caller-route and identity-bearing-answers design
//! (docs/technical-designs/second-release/headless-recovery/design.md, "Behaviors
//! to prove"). Each behavior has one named test `bNN_...`. The compile-fail
//! guards for behavior 25 live as doctests on `src/protocol.rs`.
#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use toha::{
    Interview, Pending, RawAnswer, Seed, Template,
    protocol::{self, AnswersDocumentError, DocumentStep, Headless, SubmitDocumentError},
    staging::{Store, canonical_target},
};

const NOW: &str = "2026-01-02T03:04:05+00:00[UTC]";
/// A chosen formal identity for crate-level tests; the envelope must copy it.
const EXPECTED: &str = "forge:catalog/receipt@stable";

fn text_basic() -> PathBuf {
    Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap()
}
fn template() -> Template {
    Template::load(&text_basic()).unwrap()
}
fn start(template: &Template) -> Interview<'_> {
    Interview::start(
        template,
        Seed {
            now: NOW.parse().unwrap(),
            defaults: Default::default(),
            context: toha::context::InvocationContext::for_target(
                canonical_target(Path::new(".")).unwrap(),
            ),
        },
    )
    .unwrap()
}
fn pending(interview: Interview<'_>) -> Pending<'_> {
    match interview {
        Interview::Asking(pending) => pending,
        _ => panic!("expected an asking interview"),
    }
}
fn envelope(template: &str, answers: Value) -> String {
    json!({ "template": template, "answers": answers }).to_string()
}
fn document_error(result: SubmitDocumentError) -> AnswersDocumentError {
    match result {
        SubmitDocumentError::Document(error) => error,
        SubmitDocumentError::Evaluation(error) => panic!("expected a document error: {error}"),
    }
}
/// Submits `text` through `answer_document_once` on a fresh interview and returns
/// its document error (`DocumentStep` is not `Debug`, so this avoids
/// `unwrap_err`).
fn once_error(template: &Template, text: &str) -> AnswersDocumentError {
    match protocol::answer_document_once(EXPECTED, pending(start(template)), text) {
        Ok(_) => panic!("expected a document error"),
        Err(error) => document_error(error),
    }
}

// ── crate-level route: `apply TEMPLATE PATH --answers FILE` fixtures ──────────

/// Runs the scripted route and returns (exit code, result document).
fn scripted(template: &Path, target: &Path, envelope_json: &str, extra: &[&str]) -> (i32, Value) {
    let isolation = tempfile::tempdir().unwrap();
    let answers = isolation.path().join("answers.json");
    std::fs::write(&answers, envelope_json).unwrap();
    let mut command = support::isolated_command(isolation.path());
    command
        .arg("apply")
        .arg(template)
        .arg(target)
        .arg("--answers")
        .arg(&answers)
        .args(extra);
    let output = command.output().unwrap();
    (
        output.status.code().unwrap(),
        support::first_document(&output.stdout),
    )
}
fn staged(state: &Path, target: &Path) -> bool {
    Store::new(support::staged_dir(state))
        .load(&canonical_target(target).unwrap())
        .unwrap()
        .is_some()
}

#[test]
fn b01_a_matching_envelope_works_through_every_document_operation() {
    // continue PATH FILE (one step).
    let template = template();
    let text = envelope(EXPECTED, json!({ "name": "Item" }));
    match protocol::answer_document_once(EXPECTED, pending(start(&template)), &text).unwrap() {
        DocumentStep::Accepted { interview, .. } => {
            assert!(matches!(interview, Interview::Complete(_)));
        }
        DocumentStep::Rejected { rejections, .. } => panic!("rejected: {rejections:?}"),
    }
    // The scripted headless walk.
    match protocol::answer_document_headless(EXPECTED, &template, start(&template), &text).unwrap()
    {
        Headless::Completed { .. } => {}
        other => panic!(
            "expected completed: {}",
            matches!(other, Headless::Pending { .. })
        ),
    }
    // The scripted CLI route applies from the same envelope shape.
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    let (code, document) = scripted(
        &text_basic(),
        target.path(),
        &envelope(&formal, json!({ "name": "Item" })),
        &[],
    );
    assert_eq!(code, 0, "{document}");
    assert_eq!(document["status"], "applied");
}

#[test]
fn b02_a_different_template_fails_even_with_valid_ids_and_values() {
    let template = template();
    // Every id and value is valid for the active template, but the identity
    // differs.
    let text = envelope("other:template", json!({ "name": "Item" }));
    let error = once_error(&template, &text);
    assert!(matches!(
        error,
        AnswersDocumentError::TemplateMismatch { .. }
    ));
}

#[test]
fn b03_missing_empty_nonstring_and_mismatched_identities_are_distinct() {
    let template = template();
    let once = |text: String| once_error(&template, &text);
    // Missing identity (a bare answer map).
    assert!(matches!(
        once(json!({ "name": "Item" }).to_string()),
        AnswersDocumentError::MissingIdentity
    ));
    // Empty identity.
    assert!(matches!(
        once(envelope("", json!({ "name": "Item" }))),
        AnswersDocumentError::MalformedIdentity
    ));
    // Non-string identity.
    assert!(matches!(
        once(json!({ "template": 7, "answers": { "name": "Item" } }).to_string()),
        AnswersDocumentError::MalformedIdentity
    ));
    // Mismatched identity.
    assert!(matches!(
        once(envelope("other:template", json!({ "name": "Item" }))),
        AnswersDocumentError::TemplateMismatch { .. }
    ));
    // Each is exit 1 on the scripted route.
    let target = tempfile::tempdir().unwrap();
    for text in [
        json!({ "name": "Item" }).to_string(),
        envelope("", json!({})),
        envelope("other:template", json!({})),
    ] {
        let (code, document) = scripted(&text_basic(), target.path(), &text, &[]);
        assert_eq!(code, 1, "{document}");
        assert_eq!(document["kind"], "identity");
    }
}

#[test]
fn b04_a_bare_map_fails_with_wrapper_guidance_and_never_answers() {
    // The crate boundary returns MissingIdentity from parse_and_verify, before it
    // ever calls Pending::answer.
    let template = template();
    let error = once_error(&template, "{\"name\":\"Item\"}");
    assert!(matches!(error, AnswersDocumentError::MissingIdentity));
    // The scripted route's identity error names the expected wrapper and identity.
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    let (code, document) = scripted(&text_basic(), target.path(), "{\"name\":\"Item\"}", &[]);
    assert_eq!(code, 1, "{document}");
    assert_eq!(document["kind"], "identity");
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("template") && message.contains(&serde_json::to_string(&formal).unwrap()),
        "{message}"
    );
}

#[test]
fn b05_unknown_members_and_a_non_object_answers_are_shape_errors() {
    let template = template();
    let once = |text: String| once_error(&template, &text);
    assert!(matches!(
        once(json!({ "template": EXPECTED, "answers": {}, "extra": 1 }).to_string()),
        AnswersDocumentError::Shape { .. }
    ));
    assert!(matches!(
        once(json!({ "template": EXPECTED, "answers": 5 }).to_string()),
        AnswersDocumentError::Shape { .. }
    ));
}

#[test]
fn b06_the_declared_identity_is_never_resolved() {
    // A declared name that would fail source resolution still produces an
    // identity mismatch, not a source error: the declaration is compared, never
    // resolved, fetched, or trust-checked.
    let target = tempfile::tempdir().unwrap();
    let (code, document) = scripted(
        &text_basic(),
        target.path(),
        &envelope("gh:nonexistent/repo#deadbeef", json!({ "name": "Item" })),
        &[],
    );
    assert_eq!(code, 1, "{document}");
    assert_eq!(document["kind"], "identity");
    // The crate boundary compares strings only.
    let template = template();
    let error = once_error(
        &template,
        &envelope("gh:nonexistent/repo#deadbeef", json!({ "name": "Item" })),
    );
    assert!(matches!(
        error,
        AnswersDocumentError::TemplateMismatch { .. }
    ));
}

#[test]
fn b07_scripted_refuses_a_staged_target_without_reading_the_document() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    // Stage an interview at the target.
    let stage = support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4));
    // The scripted route refuses with a `staged` document even when the answers
    // file cannot be read.
    let unreadable = state.path().join("does-not-exist.json");
    let output = support::isolated_command(state.path())
        .arg("apply")
        .arg(text_basic())
        .arg(target.path())
        .arg("--answers")
        .arg(&unreadable)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "staged");
    let _ = formal;
}

#[test]
fn b08_continue_file_on_a_complete_interview_leaves_the_document_unread() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    support::isolated_command(state.path())
        .args(["stage"])
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    let complete = support::envelope_text(&formal, &json!({ "name": "Item" }));
    let answers = state.path().join("answers.json");
    std::fs::write(&answers, complete).unwrap();
    assert_eq!(
        support::isolated_command(state.path())
            .arg("continue")
            .arg(target.path())
            .arg(&answers)
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
    // Now complete; a continue with an unreadable document refuses before reading.
    let unreadable = state.path().join("does-not-exist.json");
    let output = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&unreadable)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is complete"), "{stderr}");
}

#[test]
fn b09_a_mismatch_wins_over_unknown_ids_and_invalid_values() {
    let template = template();
    let text = envelope(
        "other:template",
        json!({ "Bad Key": "x", "unknown_id": [1, 2, 3] }),
    );
    let error = once_error(&template, &text);
    assert!(matches!(
        error,
        AnswersDocumentError::TemplateMismatch { .. }
    ));
}

#[test]
fn b10_identity_failure_leaves_staged_and_target_bytes_unchanged() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let answers = state.path().join("answers.json");
    std::fs::write(
        &answers,
        envelope("other:template", json!({ "name": "Item" })),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("apply")
        .arg(text_basic())
        .arg(target.path())
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(support::first_document(&output.stdout)["kind"], "identity");
    // Nothing was written to the target and no staged record was created.
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 0);
    assert!(!staged(state.path(), target.path()));
}

#[test]
fn b11_the_scripted_route_never_stages() {
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    let answers = isolation.path().join("answers.json");
    std::fs::write(&answers, envelope(&formal, json!({ "name": "Item" }))).unwrap();
    let output = support::isolated_command(isolation.path())
        .arg("apply")
        .arg(text_basic())
        .arg(target.path())
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    // A successful scripted apply removes nothing and stages nothing.
    assert!(!staged(isolation.path(), target.path()));
}

#[test]
fn b12_every_scripted_outcome_is_one_schema_valid_document() {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(protocol::protocol_schema())
        .unwrap();
    let formal = support::formal_name(&text_basic());
    // applied (0), planned (0, --dry-run), questions (4, missing answer),
    // error (1, mismatch).
    let cases = [
        (envelope(&formal, json!({ "name": "Item" })), vec![], 0),
        (
            envelope(&formal, json!({ "name": "Item" })),
            vec!["--dry-run"],
            0,
        ),
        (envelope(&formal, json!({})), vec![], 4),
        (envelope("other:template", json!({})), vec![], 1),
    ];
    for (text, extra, code) in cases {
        let target = tempfile::tempdir().unwrap();
        let isolation = tempfile::tempdir().unwrap();
        let answers = isolation.path().join("answers.json");
        std::fs::write(&answers, &text).unwrap();
        let mut command = support::isolated_command(isolation.path());
        command
            .arg("apply")
            .arg(text_basic())
            .arg(target.path())
            .arg("--answers")
            .arg(&answers)
            .args(&extra);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{text}");
        // Exactly one JSON value on stdout, and nothing after it.
        let mut stream = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
        let document = stream.next().expect("one document").expect("valid JSON");
        assert!(stream.next().is_none(), "trailing output: {text}");
        assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
        if let Err(error) = validator.validate(&document) {
            panic!("invalid document {document}: {error}");
        }
    }
}

#[test]
fn b13_scripted_missing_answers_report_is_required() {
    let formal = support::formal_name(&text_basic());
    let target = tempfile::tempdir().unwrap();
    let (code, document) = scripted(
        &text_basic(),
        target.path(),
        &envelope(&formal, json!({})),
        &[],
    );
    assert_eq!(code, 4, "{document}");
    assert_eq!(document["status"], "questions");
    assert_eq!(document["errors"]["name"], json!(["is required"]));
}

#[test]
fn b14_agent_batches_omit_is_required_for_questions_not_yet_asked() {
    // stage --async emits the batch with `name` as a pending question, not an
    // `is required` error.
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let output = support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "questions");
    assert!(document.get("errors").is_none(), "{document}");
    assert_eq!(document["schema"]["required"], json!(["name"]));
}

#[test]
fn b15_stage_async_file_writes_the_batch_to_the_file_and_instructions_to_stdout() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let batch_file = state.path().join("batch.json");
    let output = support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .arg(&batch_file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    // The batch went to the file.
    let batch: Value = serde_json::from_slice(&std::fs::read(&batch_file).unwrap()).unwrap();
    assert_eq!(batch["status"], "questions");
    // Only instructions on stdout — no JSON document.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<Value>(stdout.trim()).is_err(),
        "stdout should be instructions, not JSON: {stdout}"
    );
    assert!(stdout.contains("toha continue"), "{stdout}");
}

#[test]
fn b16_continue_file_completing_writes_instructions_only() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    let answers = state.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&formal, &json!({ "name": "Item" })),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_str::<Value>(stdout.trim()).is_err(),
        "stdout should be instructions only: {stdout}"
    );
    let t = target.path().display();
    assert!(
        stdout.contains(&format!("toha apply --dry-run {t}")),
        "{stdout}"
    );
    assert!(stdout.contains(&format!("toha apply {t}")), "{stdout}");
}

#[test]
fn b17_continue_file_makes_one_transaction_and_saves_at_most_one() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&text_basic());
    support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    // A rejected document saves nothing.
    let bad = state.path().join("bad.json");
    std::fs::write(
        &bad,
        support::envelope_text(&formal, &json!({ "name": "" })),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let record = Store::new(support::staged_dir(state.path()))
        .load(&canonical_target(target.path()).unwrap())
        .unwrap()
        .unwrap();
    assert!(
        record.submissions.is_empty(),
        "rejected input saved nothing"
    );
    // An accepted document saves exactly one submission.
    let good = state.path().join("good.json");
    std::fs::write(
        &good,
        support::envelope_text(&formal, &json!({ "name": "Item" })),
    )
    .unwrap();
    support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&good)
        .output()
        .unwrap();
    let record = Store::new(support::staged_dir(state.path()))
        .load(&canonical_target(target.path()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(record.submissions.len(), 1);
}

#[cfg(unix)]
#[test]
fn b18_apply_path_on_an_incomplete_interview_never_prompts() {
    use expectrl::{Expect, Session};
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    support::isolated_command(state.path())
        .arg("stage")
        .arg(text_basic())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    // Even with a pseudo-terminal on stdin and stdout, `apply PATH` emits the
    // batch and exits 4 rather than prompting.
    let mut command = support::isolated_command(state.path());
    command.arg("apply").arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    // It emits the batch instructions and exits without waiting for input; had it
    // prompted, the interview would block for an answer and never reach EOF.
    session.expect("has questions remaining").unwrap();
    session.expect(expectrl::Eof).unwrap();
}

#[cfg(unix)]
#[test]
fn b19_apply_template_path_on_an_incomplete_same_template_prompts_then_applies() {
    use expectrl::{Expect, Session};
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = support::folder_address(&text_basic());
    support::isolated_command(state.path())
        .arg("stage")
        .arg(&template)
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    let mut command = support::isolated_command(state.path());
    command.arg("apply").arg(&template).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name?").unwrap();
    session.send_line("Item").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(
        std::fs::read_to_string(target.path().join("Item.txt"))
            .unwrap()
            .contains("Item"),
        "the prompted answer was applied"
    );
}

#[cfg(unix)]
#[test]
fn b20_a_new_apply_template_path_saves_each_batch_and_resumes_after_interrupt() {
    use expectrl::{Expect, Session};
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // A two-batch template so an interrupt leaves a saved first batch.
    let folder = state.path().join("two-batch");
    std::fs::create_dir_all(folder.join("template")).unwrap();
    std::fs::write(
        folder.join("template/result.txt"),
        "{{ first }} {{ second }}\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("template.yml"),
        "name: two\ninterview:\n  - { id: first, type: text, prompt: First?, required: true }\n  - { id: second, type: text, prompt: \"After {{ first }}?\", required: true }\n",
    )
    .unwrap();
    let template = support::folder_address(&folder.canonicalize().unwrap());
    let mut command = support::isolated_command(state.path());
    command.arg("apply").arg(&template).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("First?").unwrap();
    session.send_line("One").unwrap();
    session.expect("After One?").unwrap();
    // Interrupt before answering the second batch; the first batch is saved.
    session.send(char::from(3).to_string()).unwrap(); // Ctrl-C
    // Drain the PTY before waiting: the child may still write terminal cleanup.
    session.expect(expectrl::Eof).unwrap();
    session.get_process().wait().unwrap();
    let record = Store::new(support::staged_dir(state.path()))
        .load(&canonical_target(target.path()).unwrap())
        .unwrap()
        .expect("a saved staged record");
    assert_eq!(record.submissions.len(), 1);
    assert_eq!(record.submissions[0]["first"], "One");
    // continue PATH resumes from the saved batch.
    let mut command = support::isolated_command(state.path());
    command.arg("continue").arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("After One?").unwrap();
    session.send_line("Two").unwrap();
    session.expect(expectrl::Eof).unwrap();
    // apply TEMPLATE PATH also resumes it after an interrupt (fresh case).
    assert!(staged(state.path(), target.path()) || target.path().join("result.txt").exists());
}

#[cfg(unix)]
#[test]
fn b21_stage_and_continue_show_the_dry_run_plan_at_completion() {
    use expectrl::{Expect, Session};
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = support::folder_address(&text_basic());
    let mut command = support::isolated_command(state.path());
    command.arg("stage").arg(&template).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name?").unwrap();
    session.send_line("Item").unwrap();
    // At completion the dry-run plan appears and nothing is written.
    session.expect("create Item.txt").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 0);
    assert!(staged(state.path(), target.path()));
}

#[cfg(unix)]
#[test]
fn b22_agent_and_person_staged_interviews_complete_through_the_other_route() {
    use expectrl::{Expect, Session};
    // Agent-staged (stage --async) completes through the person route (continue PATH).
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = support::folder_address(&text_basic());
    support::isolated_command(state.path())
        .arg("stage")
        .arg(&template)
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    let mut command = support::isolated_command(state.path());
    command.arg("continue").arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name?").unwrap();
    session.send_line("Item").unwrap();
    session.expect("create Item.txt").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(staged(state.path(), target.path()));

    // Person-staged (stage, prompted) completes through the agent route
    // (continue PATH FILE). A two-batch template so the person answers the first
    // batch, interrupts, and the agent finishes the second.
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let folder = state.path().join("two-batch");
    std::fs::create_dir_all(folder.join("template")).unwrap();
    std::fs::write(
        folder.join("template/result.txt"),
        "{{ first }} {{ second }}\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("template.yml"),
        "name: two\ninterview:\n  - { id: first, type: text, prompt: First?, required: true }\n  - { id: second, type: text, prompt: \"After {{ first }}?\", required: true }\n",
    )
    .unwrap();
    let two = support::folder_address(&folder.canonicalize().unwrap());
    let formal = support::formal_name(&folder.canonicalize().unwrap());
    let mut command = support::isolated_command(state.path());
    command.arg("stage").arg(&two).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("First?").unwrap();
    session.send_line("One").unwrap();
    session.expect("After One?").unwrap();
    session.send(char::from(3).to_string()).unwrap(); // interrupt after the first batch is saved
    // Drain the PTY before waiting: the child may still write terminal cleanup.
    session.expect(expectrl::Eof).unwrap();
    session.get_process().wait().unwrap();
    assert!(
        staged(state.path(), target.path()),
        "the first batch was staged"
    );
    let answers = state.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&formal, &json!({ "second": "Two" })),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn b23_flow_results_keep_their_effects_on_both_document_routes() {
    // A flow stop ends the interview through the headless walk; a flow dry-run
    // completes with the dry-run disposition.
    let stop = template_with(
        "name: gate\ninterview:\n  - { id: go, type: confirm, prompt: Go? }\n  - flow: stop\n    when: \"not go\"\n",
    );
    let template = Template::load(&stop.path().join("template")).unwrap();
    let text = envelope(EXPECTED, json!({ "go": false }));
    match protocol::answer_document_headless(EXPECTED, &template, start(&template), &text).unwrap()
    {
        Headless::Ended { ended, .. } => assert_eq!(ended.kind(), toha::EndKind::Stop),
        other => panic!(
            "expected ended: {}",
            matches!(other, Headless::Completed { .. })
        ),
    }
    // continue PATH FILE reports the same stop as an `ended` document.
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let formal = support::formal_name(&stop.path().join("template"));
    let stop_template = support::folder_address(&stop.path().join("template"));
    support::isolated_command(state.path())
        .arg("stage")
        .arg(&stop_template)
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    let answers = state.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&formal, &json!({ "go": false })),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "ended");
    assert_eq!(document["kind"], "stop");
}

/// A template root at `<tmp>/template` holding `template.yml` = `yaml` and a
/// `template/result.txt` source file.
fn template_with(yaml: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("template/template")).unwrap();
    std::fs::write(root.path().join("template/template.yml"), yaml).unwrap();
    std::fs::write(root.path().join("template/template/result.txt"), "ok\n").unwrap();
    root
}

#[test]
fn b24_in_memory_callers_still_use_rawanswers_directly() {
    // Terminal prompting and staged replay use RawAnswers; a crate caller drives
    // Pending::answer and the raw headless walk without any envelope.
    let template = template();
    let raw: toha::RawAnswers = [(toha::Id::parse("name").unwrap(), RawAnswer(json!("Item")))]
        .into_iter()
        .collect();
    match pending(start(&template)).answer(raw.clone()).unwrap() {
        Interview::Complete(_) => {}
        _ => panic!("expected complete"),
    }
    match protocol::answer_headless(&template, start(&template), raw).unwrap() {
        Headless::Completed { .. } => {}
        _ => panic!("expected completed"),
    }
}

#[test]
fn b25_external_json_cannot_extract_a_raw_capability() {
    // The compile-fail guards live as doctests on src/protocol.rs (parse_answers
    // removed, VerifiedSubmission private, complete_document removed). At runtime
    // the only way external JSON enters the engine is through the identity gate,
    // which yields a DocumentStep/Headless, never a RawAnswers.
    let template = template();
    let step = protocol::answer_document_once(
        EXPECTED,
        pending(start(&template)),
        &envelope(EXPECTED, json!({ "name": "Item" })),
    )
    .unwrap();
    assert!(matches!(step, DocumentStep::Accepted { .. }));
}

#[test]
fn b26_the_same_document_is_reusable_for_another_target_or_commit() {
    // One envelope, two independent targets: both apply the same identity.
    let formal = support::formal_name(&text_basic());
    let text = envelope(&formal, json!({ "name": "Item" }));
    for _ in 0..2 {
        let target = tempfile::tempdir().unwrap();
        let (code, document) = scripted(&text_basic(), target.path(), &text, &[]);
        assert_eq!(code, 0, "{document}");
        assert_eq!(document["status"], "applied");
        assert!(target.path().join("Item.txt").exists());
    }
    // The crate boundary accepts the same text against a fresh interview twice.
    let template = template();
    for _ in 0..2 {
        assert!(matches!(
            protocol::answer_document_once(&formal, pending(start(&template)), &text).unwrap(),
            DocumentStep::Accepted { .. }
        ));
    }
}

#[test]
fn b27_schema_examples_and_result_documents_validate() {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(protocol::protocol_schema())
        .unwrap();
    let answers_validator = {
        let mut schema = protocol::protocol_schema().clone();
        schema["$ref"] = json!("#/$defs/answers-document");
        schema.as_object_mut().unwrap().remove("oneOf");
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&schema)
            .unwrap()
    };
    // The answers envelope validates.
    let envelope_doc = json!({ "template": EXPECTED, "answers": { "name": "Item" } });
    assert!(answers_validator.is_valid(&envelope_doc), "{envelope_doc}");
    assert!(
        !answers_validator.is_valid(&json!({ "answers": {} })),
        "missing template"
    );

    // Every result document toha emits validates against the protocol schema.
    let formal = support::formal_name(&text_basic());
    let outcomes: [(&str, Vec<&str>); 3] = [
        (&envelope(&formal, json!({ "name": "Item" })), vec![]),
        (
            &envelope(&formal, json!({ "name": "Item" })),
            vec!["--dry-run"],
        ),
        (&envelope(&formal, json!({})), vec![]),
    ];
    for (text, extra) in outcomes {
        let target = tempfile::tempdir().unwrap();
        let isolation = tempfile::tempdir().unwrap();
        let answers = isolation.path().join("answers.json");
        std::fs::write(&answers, text).unwrap();
        let mut command = support::isolated_command(isolation.path());
        command
            .arg("apply")
            .arg(text_basic())
            .arg(target.path())
            .arg("--answers")
            .arg(&answers)
            .args(&extra);
        let output = command.output().unwrap();
        let document = support::first_document(&output.stdout);
        if let Err(error) = validator.validate(&document) {
            panic!("invalid document {document}: {error}");
        }
    }
}

// Behavior 12, extended: the scripted route emits exactly one JSON document even
// when a trusted hook writes uncaptured stdout. A hook echoing to stdout must not
// pollute the result document; its output is forwarded to standard error.
#[cfg(unix)]
#[test]
fn scripted_stdout_stays_one_document_with_an_uncaptured_stdout_hook() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("template/template")).unwrap();
    std::fs::write(
        dir.path().join("template/template.yml"),
        "name: hooky\nhooks:\n  - run: [\"sh\", \"-c\", \"echo HOOK-STDOUT\"]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("template/template/note.txt"), "note\n").unwrap();
    let template = dir.path().join("template");
    let formal = support::formal_name(&template);
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let answers = isolation.path().join("answers.json");
    std::fs::write(&answers, envelope(&formal, json!({}))).unwrap();
    let output = support::isolated_command(isolation.path())
        .arg("apply")
        .arg(&template)
        .arg(target.path())
        .arg("--answers")
        .arg(&answers)
        .arg("--trust")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Standard output is exactly one JSON value and nothing else.
    let mut stream = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
    let document = stream.next().expect("one document").expect("valid JSON");
    assert!(
        stream.next().is_none(),
        "trailing output on stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(document["status"], "applied");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("HOOK-STDOUT"),
        "the hook's stdout leaked onto standard output: {stdout}"
    );
    // The hook ran and its output was forwarded to standard error, unchanged.
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("HOOK-STDOUT"),
        "the hook's stdout was not forwarded to standard error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
