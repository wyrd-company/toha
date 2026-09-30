use super::*;
use crate::template::Template;
use indexmap::IndexMap;
use serde_json::json;

fn inline(yaml: &str) -> (tempfile::TempDir, Template) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("template")).unwrap();
    std::fs::write(folder.path().join("template.yml"), yaml).unwrap();
    let template = Template::load(folder.path()).unwrap();
    (folder, template)
}

fn seed() -> Seed {
    let dir = tempfile::tempdir().unwrap();
    let target = crate::staging::canonical_target(dir.path()).unwrap();
    Seed {
        now: "2026-03-14T09:26:53+00:00[UTC]".parse().unwrap(),
        defaults: IndexMap::new(),
        context: crate::context::InvocationContext::for_target(target),
    }
}

fn id(s: &str) -> Id {
    Id::parse(s).unwrap()
}

fn sub(pairs: &[(&str, serde_json::Value)]) -> IndexMap<Id, RawAnswer> {
    pairs
        .iter()
        .map(|(k, v)| (id(k), RawAnswer(v.clone())))
        .collect()
}

const TWO_QUESTIONS: &str = r#"
name: t
description: d
interview:
  - id: name
    type: text
    prompt: Name
    required: true
    validate:
      min: 3
  - id: color
    type: text
    prompt: Colour
"#;

#[test]
fn raw_recorded_submissions_replay_to_completion() {
    let (_f, template) = inline(TWO_QUESTIONS);
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("color", json!("blue")),
    ])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), false).unwrap();
    match result {
        Replay::Completed {
            completed,
            submissions,
        } => {
            assert_eq!(
                completed.answers[&id("name")].to_json(),
                json!("sample-value")
            );
            assert_eq!(submissions.len(), 1, "one batch submitted");
            assert_eq!(submissions[0][&id("name")].0, json!("sample-value"));
        }
        _ => panic!("expected completion"),
    }
}

#[test]
fn resume_with_no_staged_submissions_matches_replay() {
    let (_f, template) = inline(TWO_QUESTIONS);
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("color", json!("blue")),
    ])];
    let result = replay_resume(&template, seed(), &recorded, &[], false).unwrap();
    match result {
        Replay::Completed { submissions, .. } => {
            assert_eq!(submissions[0][&id("name")].0, json!("sample-value"));
        }
        _ => panic!("expected completion"),
    }
}

#[test]
fn resume_replays_the_staged_submissions_and_does_not_reuse_recorded() {
    let (_f, template) = inline(TWO_QUESTIONS);
    // The base recorded sample-value/blue; the staged session already answered other-value/red.
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("color", json!("blue")),
    ])];
    let staged = vec![sub(&[
        ("name", json!("other-value")),
        ("color", json!("red")),
    ])];
    let result = replay_resume(&template, seed(), &recorded, &staged, false).unwrap();
    match result {
        Replay::Completed {
            completed,
            submissions,
        } => {
            assert_eq!(submissions.len(), 1, "only the staged batch was submitted");
            assert_eq!(submissions[0][&id("name")].0, json!("other-value"));
            assert_eq!(completed.answers[&id("color")].to_json(), json!("red"));
        }
        _ => panic!("expected completion from the staged submissions"),
    }
}

#[test]
fn the_staged_consumption_pops_the_queue_front_in_order() {
    // The helpers `replay_resume` itself uses: a recording that carries three
    // values for one id queues them in order, and consuming a staged answer for
    // that id pops the front each time — so the third value is what a later batch
    // takes. Unrelated, empty, and absent-id consumptions are no-ops (never panic).
    let recorded = vec![
        sub(&[("q", json!("one"))]),
        sub(&[("q", json!("two"))]),
        sub(&[("q", json!("three"))]),
    ];
    let mut queue = super::build_queue(&recorded);
    assert_eq!(queue[&id("q")].len(), 3, "three values queued in order");
    assert_eq!(queue[&id("q")].front().unwrap().0, json!("one"));

    // Consuming two staged answers for `q` advances the front one -> two -> three.
    super::consume_staged(&mut queue, &sub(&[("q", json!("one"))]));
    assert_eq!(
        queue[&id("q")].front().unwrap().0,
        json!("two"),
        "front advanced after the first consumption"
    );
    super::consume_staged(&mut queue, &sub(&[("q", json!("two"))]));
    assert_eq!(
        queue[&id("q")].front().unwrap().0,
        json!("three"),
        "the third value remains after consuming twice"
    );
    assert_eq!(queue[&id("q")].len(), 1);

    // An unrelated staged id does not touch `q`.
    super::consume_staged(&mut queue, &sub(&[("unrelated", json!("x"))]));
    assert_eq!(queue[&id("q")].len(), 1, "an unrelated id is a no-op");

    // An empty staged submission is a no-op.
    super::consume_staged(&mut queue, &sub(&[]));
    assert_eq!(
        queue[&id("q")].len(),
        1,
        "an empty staged submission is a no-op"
    );

    // Draining the last value, then consuming an absent id, does not panic.
    super::consume_staged(&mut queue, &sub(&[("q", json!("three"))]));
    assert!(queue[&id("q")].is_empty(), "queue drained");
    super::consume_staged(&mut queue, &sub(&[("q", json!("gone"))]));
    assert!(
        queue[&id("q")].is_empty(),
        "consuming an absent id is a no-op"
    );
}

#[test]
fn a_resume_continues_through_a_later_recorded_batch_after_a_new_question() {
    // Dependent batches: the base asked `a`, then `b` (b's prompt references a, so
    // b is a later batch). The update adds a new early-required `x`, so the first
    // batch {x, a} cannot auto-complete and the engine pauses at the new question.
    // A staged answer completes that batch; resuming through drive_update_resume
    // continues through the LATER recorded batch {b} to completion. Assert the
    // submission order and count, the final answers, and that the staged batch is
    // submitted exactly once (not replayed twice on resume).
    let yaml = "name: t\n\
                interview:\n\
                \x20 - id: x\n\
                \x20   type: text\n\
                \x20   prompt: X\n\
                \x20   required: true\n\
                \x20 - id: a\n\
                \x20   type: text\n\
                \x20   prompt: A\n\
                \x20   required: true\n\
                \x20 - id: b\n\
                \x20   type: text\n\
                \x20   prompt: \"B for {{ a }}\"\n\
                \x20   required: true\n";
    let (_f, template) = inline(yaml);
    // The base recorded `a`, then `b`, across two batches (b depended on a).
    let recorded = vec![sub(&[("a", json!("a-val"))]), sub(&[("b", json!("b-val"))])];

    // With no staged answers the first batch {x, a} cannot auto-complete (x is new
    // and unrecorded), so the engine pauses there and does not reach `b`.
    match drive_update_resume(&template, seed(), &recorded, &[], false).unwrap() {
        UpdateDrive::Ask { pending, .. } => {
            let asks: Vec<String> = pending
                .batch()
                .items
                .iter()
                .filter_map(|i| match i {
                    crate::interview::Item::Prompt(p) => Some(p.id.as_str().to_owned()),
                    _ => None,
                })
                .collect();
            assert!(
                asks.contains(&"x".to_owned()),
                "pauses at the new early question: {asks:?}"
            );
            assert!(
                !asks.contains(&"b".to_owned()),
                "b is a later batch (depends on a), not asked yet: {asks:?}"
            );
        }
        _ => panic!("expected an Ask at the new question"),
    }

    // The agent stages the first batch, then resumes through the later batch.
    let staged = vec![sub(&[("x", json!("x-val")), ("a", json!("a-val"))])];
    match drive_update_resume(&template, seed(), &recorded, &staged, false).unwrap() {
        UpdateDrive::Completed {
            completed,
            submissions,
        } => {
            // Order and count: the staged batch, then the later recorded batch.
            assert_eq!(
                submissions.len(),
                2,
                "the staged batch then the later recorded batch: {submissions:?}"
            );
            assert_eq!(submissions[0][&id("x")].0, json!("x-val"));
            assert_eq!(submissions[0][&id("a")].0, json!("a-val"));
            assert_eq!(
                submissions[1][&id("b")].0,
                json!("b-val"),
                "the later recorded batch replayed on resume"
            );
            // The staged batch is submitted exactly once, not duplicated on resume.
            assert_eq!(
                submissions
                    .iter()
                    .filter(|s| s.contains_key(&id("x")))
                    .count(),
                1,
                "the staged batch is submitted exactly once"
            );
            // Final answers reflect all three, once each.
            assert_eq!(completed.answers[&id("x")].to_json(), json!("x-val"));
            assert_eq!(completed.answers[&id("a")].to_json(), json!("a-val"));
            assert_eq!(completed.answers[&id("b")].to_json(), json!("b-val"));
        }
        _ => panic!("expected completion through the later batch"),
    }
}

#[test]
fn an_answer_for_a_removed_question_is_dropped() {
    let (_f, template) = inline(TWO_QUESTIONS);
    // `old` is not a question in this template; it is never consumed.
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("old", json!("x")),
        ("color", json!("blue")),
    ])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), false).unwrap();
    assert!(
        matches!(result, Replay::Completed { .. }),
        "dropped answer does not block completion"
    );
}

#[test]
fn a_recorded_value_the_new_version_rejects_asks_with_the_rejection() {
    let (_f, template) = inline(TWO_QUESTIONS);
    // "Al" is shorter than the new min of 3.
    let recorded = vec![sub(&[("name", json!("Al")), ("color", json!("blue"))])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), false).unwrap();
    match result {
        Replay::Ask { rejections, .. } => {
            assert!(
                rejections.iter().any(|r| r.id == id("name")),
                "the rejection names the question: {rejections:?}"
            );
        }
        _ => panic!("expected an ask"),
    }
}

#[test]
fn an_answers_override_replaces_the_recorded_value() {
    let (_f, template) = inline(TWO_QUESTIONS);
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("color", json!("blue")),
    ])];
    let overrides: RawAnswers = sub(&[("name", json!("revised-value"))]);
    let result = replay(&template, seed(), &recorded, overrides, false).unwrap();
    match result {
        Replay::Completed { submissions, .. } => {
            assert_eq!(
                submissions[0][&id("name")].0,
                json!("revised-value"),
                "override wins"
            );
        }
        _ => panic!("expected completion"),
    }
}

#[test]
fn reanswer_offers_every_batch_to_the_route() {
    let (_f, template) = inline(TWO_QUESTIONS);
    let recorded = vec![sub(&[
        ("name", json!("sample-value")),
        ("color", json!("blue")),
    ])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), true).unwrap();
    match result {
        Replay::Ask {
            pending,
            rejections,
            ..
        } => {
            assert!(rejections.is_empty(), "reanswer is not a rejection");
            // The recorded value is offered as the default.
            let name_default = pending.batch().items.iter().find_map(|i| match i {
                crate::interview::Item::Prompt(p) if p.id == id("name") => p.default.as_ref(),
                _ => None,
            });
            assert!(name_default.is_some(), "the recorded value is the default");
        }
        _ => panic!("expected an ask for reanswer"),
    }
}

const TWO_BATCHES: &str = r#"
name: t
description: d
interview:
  - id: name
    type: text
    prompt: Name
    required: true
  - id: more
    type: confirm
    prompt: More?
  - id: extra
    type: text
    prompt: Extra
    when: more
"#;

#[test]
fn recorded_submissions_replay_across_batches_in_order() {
    let (_f, template) = inline(TWO_BATCHES);
    // First batch answers name + more; the second (gated by `more`) answers extra.
    let recorded = vec![
        sub(&[("name", json!("sample-value")), ("more", json!(true))]),
        sub(&[("extra", json!("gravy"))]),
    ];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), false).unwrap();
    match result {
        Replay::Completed {
            completed,
            submissions,
        } => {
            assert_eq!(completed.answers[&id("extra")].to_json(), json!("gravy"));
            assert_eq!(submissions.len(), 2, "two batches submitted in order");
        }
        other => panic!(
            "expected completion, got a different outcome: {}",
            matches!(other, Replay::Ask { .. })
        ),
    }
}
