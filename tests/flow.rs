// ---
// relationships:
//   implements: template-format
//   verifies: interview-protocol
// ---
//! The interview flow-control node (`stop`/`abort`/`dry-run`/`skip`), proved at
//! the crate, wire (headless), and staged-replay routes. The terminal and
//! direct-apply routes, and the abort staged-record removal, are proved against
//! the binary in `flow_cli.rs`.
#![allow(clippy::bool_assert_comparison)]

use indexmap::IndexMap;
use serde_json::{Value, json};
use toha::{
    Answer, AnswerError, Disposition, EndKind, Id, Interview, RawAnswer, RawAnswers, Seed,
    Template,
    protocol::{self, Context, Headless},
    staging::{StagedRecord, canonical_target},
};

const NOW: &str = "2026-01-02T03:04:05+00:00[UTC]";

fn tpl(yaml: &str) -> (tempfile::TempDir, Template) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), yaml).unwrap();
    let template = Template::load(dir.path()).unwrap();
    (dir, template)
}

fn load_error(yaml: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), yaml).unwrap();
    Template::load(dir.path()).unwrap_err().to_string()
}

fn seed() -> Seed {
    Seed {
        now: NOW.parse().unwrap(),
        defaults: IndexMap::new(),
    }
}

fn start(template: &Template) -> Interview<'_> {
    Interview::start(template, seed()).unwrap()
}

fn raw(document: Value) -> RawAnswers {
    document
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (Id::parse(key).unwrap(), RawAnswer(value.clone())))
        .collect()
}

/// Answers the current batch of an asking interview.
fn answer<'a>(interview: Interview<'a>, document: Value) -> Interview<'a> {
    let Interview::Asking(pending) = interview else {
        panic!("expected an asking interview");
    };
    pending.answer(raw(document)).unwrap()
}

fn id(name: &str) -> Id {
    Id::parse(name).unwrap()
}

/// A staged record for `submissions` at an existing target directory.
fn staged(target: &std::path::Path, submissions: Vec<IndexMap<String, Value>>) -> StagedRecord {
    let canonical = canonical_target(target).unwrap();
    StagedRecord::new(
        &canonical,
        "sample".into(),
        String::new(),
        false,
        NOW.into(),
        submissions,
    )
}

fn context(target: &std::path::Path, record: &StagedRecord) -> Context {
    Context::new(&canonical_target(target).unwrap(), record)
}

// ---------------------------------------------------------------------------
// Declaration / load (behaviors 1–4)
// ---------------------------------------------------------------------------

#[test]
fn b1_unknown_action_is_a_load_error_naming_the_allowed_set() {
    let error = load_error("name: sample\ninterview:\n  - flow: teleport\n");
    assert!(
        error.contains("flow action must be stop, abort, dry-run, or { skip: rest | group }"),
        "{error}"
    );
}

#[test]
fn b2_top_level_skip_group_is_a_load_error_naming_skip_rest() {
    let error = load_error("name: sample\ninterview:\n  - flow: { skip: group }\n");
    assert!(
        error.contains("skip: group needs an enclosing group") && error.contains("{ skip: rest }"),
        "{error}"
    );
}

#[test]
fn b3_flow_beside_a_node_key_is_a_load_error() {
    let error =
        load_error("name: sample\ninterview:\n  - { flow: stop, id: x, type: text, prompt: X }\n");
    assert!(
        error.contains("flow is a node kind; it cannot be combined with"),
        "{error}"
    );
}

#[test]
fn b4_no_flow_node_keeps_ordinary_completion_bytes() {
    // An ordinary completion carries no `disposition` and never ends, so its
    // wire bytes are unchanged by the feature.
    let (dir, template) =
        tpl("name: sample\ninterview:\n  - { id: proceed, type: confirm, prompt: Ready? }\n");
    let done = answer(start(&template), json!({ "proceed": true }));
    let Interview::Complete(completed) = done else {
        panic!("expected complete");
    };
    assert_eq!(completed.disposition(), Disposition::Proceed);
    let document = protocol::complete_document(
        &completed,
        &context(dir.path(), &staged(dir.path(), vec![])),
    );
    assert_eq!(document["status"], "complete");
    assert!(
        document.get("disposition").is_none(),
        "an ordinary completion has no disposition: {document}"
    );
}

// ---------------------------------------------------------------------------
// Stop / abort (behaviors 5–9)
// ---------------------------------------------------------------------------

const GATE: &str = "name: sample
interview:
  - { id: proceed, type: confirm, prompt: Ready? }
  - flow: stop
    when: \"not proceed\"
    label: declined at gate
  - { id: mode, type: text, prompt: Mode? }
";

#[test]
fn b5_stop_fires_when_true_and_ends_the_interview() {
    let (_dir, template) = tpl(GATE);
    let ended = answer(start(&template), json!({ "proceed": false }));
    let Interview::Ended(ended) = ended else {
        panic!("expected ended");
    };
    assert_eq!(ended.kind(), EndKind::Stop);
    assert_eq!(ended.label(), Some("declined at gate"));
}

#[test]
fn b6_stop_does_not_fire_when_false_and_the_interview_continues() {
    let (_dir, template) = tpl(GATE);
    // proceed=true skips the stop; `mode` is asked next.
    let next = answer(start(&template), json!({ "proceed": true }));
    let Interview::Asking(pending) = next else {
        panic!("expected asking mode");
    };
    let ids: Vec<_> = pending
        .batch()
        .items
        .iter()
        .filter_map(|item| match item {
            toha::Item::Prompt(prompt) => Some(prompt.id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["mode"]);
}

#[test]
fn b7_stop_renders_no_later_questions_default() {
    // The `default` of a question after the stop is invalid, but the stop ends
    // the interview before that question is reached, so it never faults.
    // `n`'s default `c` is not among its options, which faults only when the
    // question is reached and its default is rendered.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: proceed, type: confirm, prompt: Ready? }
  - flow: stop
    when: \"not proceed\"
  - { id: n, type: select, prompt: Pick?, options: [a, b], default: c }
");
    let ended = answer(start(&template), json!({ "proceed": false }));
    assert!(matches!(ended, Interview::Ended(e) if e.kind() == EndKind::Stop));
}

#[test]
fn b8_abort_replays_to_ended_abort() {
    // The engine yields `Ended{Abort}`; the driver performs the removal, proved
    // in flow_cli.rs.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: cancel, type: confirm, prompt: Cancel? }
  - flow: abort
    when: cancel
");
    let target = tempfile::tempdir().unwrap();
    let record = staged(
        target.path(),
        vec![IndexMap::from([("cancel".into(), json!(true))])],
    );
    let replayed = record.replay(&template).unwrap();
    let Interview::Ended(ended) = replayed else {
        panic!("expected ended");
    };
    assert_eq!(ended.kind(), EndKind::Abort);
}

#[test]
fn b9_abort_on_a_fresh_interview_ends_without_a_record() {
    let (_dir, template) = tpl(
        "name: sample\ninterview:\n  - { id: cancel, type: confirm, prompt: Cancel? }\n  - flow: abort\n    when: cancel\n",
    );
    let ended = answer(start(&template), json!({ "cancel": true }));
    assert!(matches!(ended, Interview::Ended(e) if e.kind() == EndKind::Abort));
}

// ---------------------------------------------------------------------------
// Dry-run (behaviors 12–13; 13's no-write union is proved in flow_cli.rs)
// ---------------------------------------------------------------------------

#[test]
fn b12_dry_run_completes_with_the_dry_run_disposition_and_same_answers() {
    let flow = "name: sample
interview:
  - { id: mode, type: select, options: [apply, preview], prompt: Mode? }
  - flow: dry-run
    when: \"mode == 'preview'\"
  - { id: name, type: text, prompt: Name? }
";
    let plain = "name: sample
interview:
  - { id: mode, type: select, options: [apply, preview], prompt: Mode? }
  - { id: name, type: text, prompt: Name? }
";
    let answers = json!({ "mode": "preview", "name": "x" });
    let (_d1, dry) = tpl(flow);
    let Interview::Complete(dry) = drive_headless(&dry, answers.clone()) else {
        panic!("expected complete");
    };
    assert_eq!(dry.disposition(), Disposition::DryRun);
    assert_eq!(dry.step(), toha::Step::Plan { apply: false });

    let (_d2, plain) = tpl(plain);
    let Interview::Complete(proceed) = drive_headless(&plain, answers) else {
        panic!("expected complete");
    };
    assert_eq!(proceed.disposition(), Disposition::Proceed);
    // The dry-run plans the full answer set, identical to the proceed plan.
    assert_eq!(dry.answers, proceed.answers);
}

/// Runs the whole document through the headless driver.
fn drive_headless<'a>(template: &'a Template, document: Value) -> Interview<'a> {
    match protocol::answer_headless(template, start(template), raw(document)).unwrap() {
        Headless::Completed { completed, .. } => Interview::Complete(completed),
        Headless::Ended { ended, .. } => Interview::Ended(ended),
        Headless::Pending { pending, .. } => Interview::Asking(*pending),
    }
}

#[test]
fn b12_dry_run_disposition_is_re_derived_across_batches() {
    // The dry-run flow fires in an early batch (its outcome is not stored on the
    // pending interview) and must re-fire on the final walk so the completion
    // still carries it. This is why the flow node is not `visited`-gated.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: mode, type: select, options: [apply, preview], prompt: Mode? }
  - flow: dry-run
    when: \"mode == 'preview'\"
  - { id: name, type: text, prompt: Name? }
");
    // Batch 1 asks `mode` (the flow blocks on the pending reference).
    let asking = answer(start(&template), json!({ "mode": "preview" }));
    // Batch 2 asks `name`; the dry-run has fired but is not stored on `Pending`.
    let done = answer(asking, json!({ "name": "x" }));
    let Interview::Complete(completed) = done else {
        panic!("expected complete");
    };
    assert_eq!(completed.disposition(), Disposition::DryRun);
}

#[test]
fn b12_dry_run_disposition_rides_the_wire_complete_document() {
    let (dir, template) = tpl(
        "name: sample\ninterview:\n  - { id: mode, type: text, prompt: Mode? }\n  - flow: dry-run\n",
    );
    let Interview::Complete(completed) = drive_headless(&template, json!({ "mode": "x" })) else {
        panic!("expected complete");
    };
    let document = protocol::complete_document(
        &completed,
        &context(dir.path(), &staged(dir.path(), vec![])),
    );
    assert_eq!(document["disposition"], "dry-run");
}

// ---------------------------------------------------------------------------
// Skip scope + nesting (behaviors 14–16)
// ---------------------------------------------------------------------------

#[test]
fn b14_skip_rest_skips_every_remaining_question_including_groups() {
    let (_dir, template) = tpl("name: sample
interview:
  - { id: minimal, type: confirm, prompt: Minimal? }
  - flow: { skip: rest }
    when: minimal
  - { id: name, type: text, prompt: Name? }
  - group: extra
    nodes:
      - { id: deep, type: text, prompt: Deep? }
");
    let done = answer(start(&template), json!({ "minimal": true }));
    let Interview::Complete(completed) = done else {
        panic!("expected complete");
    };
    // Every skipped question records its empty/default answer.
    assert_eq!(completed.answers[&id("name")], Answer::None);
    assert_eq!(completed.answers[&id("deep")], Answer::None);
}

#[test]
fn b15_skip_group_skips_only_the_enclosing_groups_remaining_siblings() {
    let (_dir, template) = tpl("name: sample
interview:
  - group: telemetry
    nodes:
      - { id: configure_now, type: confirm, prompt: Configure now? }
      - flow: { skip: group }
        when: \"not configure_now\"
      - { id: telemetry_key, type: text, prompt: Key? }
  - { id: after, type: text, prompt: After? }
");
    let next = answer(start(&template), json!({ "configure_now": false }));
    // The group's remaining `telemetry_key` is skipped, but `after` still asks.
    let Interview::Asking(pending) = next else {
        panic!("expected asking after");
    };
    let ids: Vec<_> = pending
        .batch()
        .items
        .iter()
        .filter_map(|item| match item {
            toha::Item::Prompt(prompt) => Some(prompt.id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["after"]);
    let done = answer(Interview::Asking(pending), json!({ "after": "z" }));
    let Interview::Complete(completed) = done else {
        panic!("expected complete");
    };
    assert_eq!(completed.answers[&id("telemetry_key")], Answer::None);
}

#[test]
fn b16_held_answer_for_a_skipped_question_warns_it_was_not_used() {
    // A staged answer for `name`, then a `skip: rest` skips `name`.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: minimal, type: confirm, prompt: Minimal? }
  - flow: { skip: rest }
    when: minimal
  - { id: name, type: text, prompt: Name? }
");
    let target = tempfile::tempdir().unwrap();
    let record = staged(
        target.path(),
        vec![IndexMap::from([
            ("name".into(), json!("held")),
            ("minimal".into(), json!(true)),
        ])],
    );
    let Interview::Complete(completed) = record.replay(&template).unwrap() else {
        panic!("expected complete");
    };
    assert!(
        completed
            .messages
            .iter()
            .any(|m| m.contains("answer for \"name\" was not used")),
        "{:?}",
        completed.messages
    );
}

// ---------------------------------------------------------------------------
// Composition / determinism (behaviors 17–20)
// ---------------------------------------------------------------------------

#[test]
fn b17_flow_under_a_false_when_does_not_fire() {
    let (_dir, template) = tpl(
        "name: sample\ninterview:\n  - { id: proceed, type: confirm, prompt: Ready? }\n  - flow: stop\n    when: \"not proceed\"\n",
    );
    // proceed=true: the stop's `when` is false, so it does not fire; the
    // interview completes normally.
    let done = answer(start(&template), json!({ "proceed": true }));
    assert!(matches!(done, Interview::Complete(_)));
}

#[test]
fn b17_flow_under_an_ancestor_skip_does_not_fire() {
    // A stop inside a group whose `when` is false must not end the interview.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: gate, type: confirm, prompt: Gate? }
  - group: g
    when: gate
    nodes:
      - flow: stop
  - { id: after, type: text, prompt: After? }
");
    let next = answer(start(&template), json!({ "gate": false }));
    // The group is inactive, so its stop is inert; `after` is asked.
    assert!(matches!(next, Interview::Asking(_)));
}

#[test]
fn b18_same_batch_readiness_blocks_a_flow_until_its_reference_commits() {
    let (_dir, template) = tpl(
        "name: sample\ninterview:\n  - { id: proceed, type: confirm, prompt: Ready? }\n  - flow: stop\n    when: \"not proceed\"\n",
    );
    // The very first batch asks `proceed`; the stop is not yet ready and never
    // appears as a batch item.
    let Interview::Asking(pending) = start(&template) else {
        panic!("expected asking");
    };
    let has_prompt = pending
        .batch()
        .items
        .iter()
        .any(|item| matches!(item, toha::Item::Prompt(p) if p.id.as_str() == "proceed"));
    assert!(has_prompt);
    // Only after `proceed` commits does the stop fire.
    let ended = answer(Interview::Asking(pending), json!({ "proceed": false }));
    assert!(matches!(ended, Interview::Ended(e) if e.kind() == EndKind::Stop));
}

#[test]
fn b19_replay_reproduces_the_end_and_records_no_new_field() {
    let (_dir, template) = tpl(GATE);
    let target = tempfile::tempdir().unwrap();
    let record = staged(
        target.path(),
        vec![IndexMap::from([("proceed".into(), json!(false))])],
    );
    // Replay is deterministic: the same stored submission yields the same stop.
    let Interview::Ended(ended) = record.replay(&template).unwrap() else {
        panic!("expected ended");
    };
    assert_eq!(ended.kind(), EndKind::Stop);
    // The staged record gains no action/disposition/end field.
    let value = serde_json::to_value(&record).unwrap();
    let keys: std::collections::BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "commit",
            "named",
            "now",
            "submissions",
            "target",
            "template"
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn b20_a_configured_default_can_fire_a_flow_action() {
    use toha::config::{ConfigEntry, ConfigLayer, ConfigOrigin, DefaultSource};
    // `enabled` has no prompt shown here; its configured default `true` fires
    // the stop through the flow's `when`.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: enabled, type: confirm, prompt: Enabled? }
  - flow: stop
    when: enabled
  - { id: after, type: text, prompt: After? }
");
    let mappings = [(
        "sample".to_string(),
        [(
            id("enabled"),
            ConfigEntry {
                value: DefaultSource::Literal(json!(true)),
                origin: ConfigOrigin {
                    layer: ConfigLayer::User,
                    path: "user.yml".into(),
                },
            },
        )]
        .into_iter()
        .collect(),
    )]
    .into_iter()
    .collect();
    let resolution =
        toha::interview::configured_defaults("sample", &template, &Default::default(), &mappings)
            .unwrap();
    assert!(resolution.warnings().is_empty());
    let interview = resolution.start(&template, NOW.parse().unwrap()).unwrap();
    // `enabled` is still asked (a default is not an answer); answering it with
    // its default value fires the stop.
    let ended = answer(interview, json!({ "enabled": true }));
    assert!(matches!(ended, Interview::Ended(e) if e.kind() == EndKind::Stop));
}

// ---------------------------------------------------------------------------
// Negative / atomicity (behaviors 21–23)
// ---------------------------------------------------------------------------

#[test]
fn b21_a_document_that_rejects_and_aborts_returns_rejected_and_ends_nothing() {
    // `cancel` triggers abort; `code` in the same document is invalid. The
    // rejection wins: the answer returns Rejected, not Ended.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: code, type: text, prompt: Code?, validate: { min: 2 } }
  - flow: abort
    when: \"code == 'x'\"
");
    let Interview::Asking(pending) = start(&template) else {
        panic!("expected asking");
    };
    // `code = x` is too short (min 2) and would also satisfy the abort's `when`.
    let error = pending.answer(raw(json!({ "code": "x" }))).unwrap_err();
    assert!(
        matches!(error, AnswerError::Rejected { .. }),
        "expected rejected"
    );
}

#[test]
fn b22_a_stop_before_a_proven_skipped_early_answer_drops_the_early_error() {
    // The document answers `proceed=false` (firing the stop) and carries an
    // invalid early answer for `code`, a question the stop prevents reaching.
    // The stop stands and the early error is dropped.
    let (_dir, template) = tpl("name: sample
interview:
  - { id: proceed, type: confirm, prompt: Ready? }
  - flow: stop
    when: \"not proceed\"
  - { id: code, type: text, prompt: Code?, validate: { min: 2 } }
");
    let Interview::Asking(pending) = start(&template) else {
        panic!("expected asking");
    };
    let next = pending
        .answer(raw(json!({ "proceed": false, "code": "x" })))
        .unwrap();
    assert!(matches!(next, Interview::Ended(e) if e.kind() == EndKind::Stop));
}

#[test]
fn b23_a_submission_after_a_terminal_interview_is_a_replay_error() {
    let (_dir, template) = tpl(GATE);
    let target = tempfile::tempdir().unwrap();
    // The first submission ends the interview; a second submission is invalid.
    let record = staged(
        target.path(),
        vec![
            IndexMap::from([("proceed".into(), json!(false))]),
            IndexMap::from([("mode".into(), json!("late"))]),
        ],
    );
    let error = record.replay(&template).unwrap_err().to_string();
    assert!(
        error.contains("submission after completed interview"),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// Driver parity (design "Driver parity"): headless and staged emit byte-equal
// `ended` documents for identical submissions.
// ---------------------------------------------------------------------------

#[test]
fn ended_documents_match_across_headless_and_staged() {
    let (_dir, template) = tpl(GATE);
    let target = tempfile::tempdir().unwrap();
    let submission = IndexMap::from([("proceed".into(), json!(false))]);
    let record = staged(target.path(), vec![submission.clone()]);
    let ctx = context(target.path(), &record);

    let Interview::Ended(from_staged) = record.replay(&template).unwrap() else {
        panic!("staged did not end");
    };
    let Headless::Ended {
        ended: from_headless,
        ..
    } = protocol::answer_headless(
        &template,
        start(&template),
        raw(json!({ "proceed": false })),
    )
    .unwrap()
    else {
        panic!("headless did not end");
    };

    assert_eq!(
        protocol::ended_document(&from_staged, &ctx),
        protocol::ended_document(&from_headless, &ctx),
    );
}
