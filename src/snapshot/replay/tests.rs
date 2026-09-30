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
    let recorded = vec![sub(&[("name", json!("Alice")), ("color", json!("blue"))])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), false).unwrap();
    match result {
        Replay::Completed {
            completed,
            submissions,
        } => {
            assert_eq!(completed.answers[&id("name")].to_json(), json!("Alice"));
            assert_eq!(submissions.len(), 1, "one batch submitted");
            assert_eq!(submissions[0][&id("name")].0, json!("Alice"));
        }
        _ => panic!("expected completion"),
    }
}

#[test]
fn an_answer_for_a_removed_question_is_dropped() {
    let (_f, template) = inline(TWO_QUESTIONS);
    // `old` is not a question in this template; it is never consumed.
    let recorded = vec![sub(&[
        ("name", json!("Alice")),
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
    let recorded = vec![sub(&[("name", json!("Alice")), ("color", json!("blue"))])];
    let overrides: RawAnswers = sub(&[("name", json!("Robert"))]);
    let result = replay(&template, seed(), &recorded, overrides, false).unwrap();
    match result {
        Replay::Completed { submissions, .. } => {
            assert_eq!(
                submissions[0][&id("name")].0,
                json!("Robert"),
                "override wins"
            );
        }
        _ => panic!("expected completion"),
    }
}

#[test]
fn reanswer_offers_every_batch_to_the_route() {
    let (_f, template) = inline(TWO_QUESTIONS);
    let recorded = vec![sub(&[("name", json!("Alice")), ("color", json!("blue"))])];
    let result = replay(&template, seed(), &recorded, IndexMap::new(), true).unwrap();
    match result {
        Replay::Ask {
            pending,
            rejections,
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
        sub(&[("name", json!("Alice")), ("more", json!(true))]),
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
