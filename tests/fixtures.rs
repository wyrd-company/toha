// ---
// relationships:
//   implements: architecture
// ---
mod support;

use std::{fs, path::Path};
use toha::{
    AnswerError, Applied, ApplyOptions, Id, Interview, Plan, RawAnswer, RawAnswers, Seed, Template,
};

fn run(fixture: &Path, target: &Path) -> Result<Vec<String>, (u8, String)> {
    let template =
        Template::load(&fixture.join("template")).map_err(|error| (1, error.to_string()))?;
    let json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(fixture.join("answers.json")).unwrap()).unwrap();
    let mut raw: RawAnswers = json
        .into_iter()
        .map(|(key, value)| (Id::parse(&key).unwrap(), RawAnswer(value)))
        .collect();
    let mut interview = Interview::start(
        &template,
        Seed {
            now: support::expectation(fixture).now.parse().unwrap(),
            defaults: indexmap::IndexMap::new(),
        },
    )
    .map_err(|error| (1, error.to_string()))?;
    let completed = loop {
        match interview {
            Interview::Complete(completed) => break completed,
            Interview::Asking(pending) => {
                interview =
                    pending
                        .answer(std::mem::take(&mut raw))
                        .map_err(|error| match error {
                            AnswerError::Rejected { rejections, .. } => (
                                4,
                                rejections
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            ),
                            AnswerError::Eval(error) => (1, error.to_string()),
                        })?;
            }
        }
    };
    let plan =
        Plan::build(&template, &completed, target).map_err(|error| (1, error.to_string()))?;
    match plan
        .apply(target, ApplyOptions::default())
        .map_err(|error| (1, error.to_string()))?
    {
        Applied::Written(_) => Ok(completed.messages),
    }
}

#[test]
fn every_fixture_through_library() {
    for fixture in support::fixtures() {
        let target = tempfile::tempdir().unwrap();
        let expect = support::expectation(&fixture);
        let result = run(&fixture, target.path());
        let name = fixture.file_name().unwrap().to_string_lossy();
        match result {
            Ok(messages) => {
                assert_eq!(expect.exit, 0, "{name}");
                assert_eq!(messages, expect.messages, "{name}");
                support::assert_tree(target.path(), &fixture.join("expected"));
            }
            Err((exit, text)) => {
                assert_eq!(exit, expect.exit, "{name}: {text}");
                for part in expect.error_contains {
                    assert!(text.contains(&part), "{name}: missing {part:?} in {text:?}");
                }
                assert!(
                    fs::read_dir(target.path()).unwrap().next().is_none(),
                    "{name}: wrote files on error"
                );
            }
        }
    }
}

#[test]
fn rejected_batch_keeps_answers_unrecorded() {
    let template = Template::load(Path::new("tests/fixtures/text-basic/template")).unwrap();
    let Interview::Asking(pending) = Interview::start(
        &template,
        Seed {
            now: support::expectation(Path::new("tests/fixtures/text-basic"))
                .now
                .parse()
                .unwrap(),
            defaults: indexmap::IndexMap::new(),
        },
    )
    .unwrap() else {
        panic!("expected batch")
    };
    let mut bad = RawAnswers::new();
    bad.insert(
        Id::parse("name").unwrap(),
        RawAnswer(serde_json::json!("Item")),
    );
    bad.insert(
        Id::parse("other").unwrap(),
        RawAnswer(serde_json::json!("x")),
    );
    let AnswerError::Rejected {
        pending,
        rejections,
    } = pending.answer(bad).unwrap_err()
    else {
        panic!("expected rejection")
    };
    assert_eq!(rejections.len(), 1);
    assert_eq!(pending.batch().items.len(), 1);
    let result = pending.answer(RawAnswers::new());
    assert!(result.is_err(), "rejected answer must not be retained");
}

#[test]
fn conflict_refuses_every_write() {
    let fixture = Path::new("tests/fixtures/text-basic");
    let target = tempfile::tempdir().unwrap();
    fs::write(target.path().join("Item.txt"), "existing").unwrap();
    let result = run(fixture, target.path());
    assert_eq!(result.as_ref().err().map(|(exit, _)| *exit), Some(1));
    assert_eq!(
        fs::read_to_string(target.path().join("Item.txt")).unwrap(),
        "existing"
    );
    assert!(!target.path().join("nested").exists());
}

#[cfg(unix)]
#[test]
fn target_symlink_cannot_redirect_output() {
    use std::os::unix::fs::symlink;
    let fixture = Path::new("tests/fixtures/text-basic");
    let target = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), target.path().join("nested")).unwrap();
    let result = run(fixture, target.path());
    assert!(
        result
            .as_ref()
            .err()
            .is_some_and(|(_, message)| message.contains("symlink"))
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[test]
fn basic_example_batch_by_batch_matches_single_submission() {
    let fixture = Path::new("tests/fixtures/basic-example");
    let template = Template::load(&fixture.join("template")).unwrap();
    let values: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(fixture.join("answers.json")).unwrap()).unwrap();
    let all: RawAnswers = values
        .into_iter()
        .map(|(k, v)| (Id::parse(&k).unwrap(), RawAnswer(v)))
        .collect();
    let seed = || Seed {
        now: support::expectation(fixture).now.parse().unwrap(),
        defaults: indexmap::IndexMap::new(),
    };
    let Interview::Asking(single) = Interview::start(&template, seed()).unwrap() else {
        panic!()
    };
    let mut state = single.answer(all.clone()).unwrap();
    let one = loop {
        match state {
            Interview::Complete(c) => break c.answers,
            Interview::Asking(p) => state = p.answer(RawAnswers::new()).unwrap(),
        }
    };
    let mut state = Interview::start(&template, seed()).unwrap();
    let batches = loop {
        match state {
            Interview::Complete(c) => break c.answers,
            Interview::Asking(p) => {
                let ids: Vec<_> = p
                    .batch()
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        toha::Item::Prompt(prompt) => Some(prompt.id.clone()),
                        _ => None,
                    })
                    .collect();
                let part: RawAnswers = all
                    .iter()
                    .filter(|(id, _)| ids.contains(id))
                    .map(|(id, v)| (id.clone(), v.clone()))
                    .collect();
                state = p.answer(part).unwrap();
            }
        }
    };
    assert_eq!(one, batches);
}

#[test]
fn configured_default_replaces_question_default() {
    let template = Template::load(Path::new("tests/fixtures/text-default/template")).unwrap();
    let mut defaults = indexmap::IndexMap::new();
    defaults.insert(
        Id::parse("second").unwrap(),
        RawAnswer(serde_json::json!("Configured")),
    );
    let Interview::Asking(pending) = Interview::start(
        &template,
        Seed {
            now: support::expectation(Path::new("tests/fixtures/text-default"))
                .now
                .parse()
                .unwrap(),
            defaults,
        },
    )
    .unwrap() else {
        panic!()
    };
    let Interview::Complete(completed) = pending
        .answer(
            [(
                Id::parse("first").unwrap(),
                RawAnswer(serde_json::json!("Input")),
            )]
            .into_iter()
            .collect(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        completed.answers.get(&Id::parse("second").unwrap()),
        Some(&toha::Answer::Text("Configured".into()))
    );
}

#[test]
fn configured_default_does_not_evaluate_question_default() {
    let template = Template::load(Path::new("tests/fixtures/err-default-render/template")).unwrap();
    let mut defaults = indexmap::IndexMap::new();
    defaults.insert(
        Id::parse("second").unwrap(),
        RawAnswer(serde_json::json!("Configured")),
    );
    let seed = Seed {
        now: support::expectation(Path::new("tests/fixtures/err-default-render"))
            .now
            .parse()
            .unwrap(),
        defaults,
    };
    let Interview::Asking(pending) = Interview::start(&template, seed).unwrap() else {
        panic!()
    };
    let mut answers = RawAnswers::new();
    answers.insert(
        Id::parse("first").unwrap(),
        RawAnswer(serde_json::json!("Input")),
    );
    let Interview::Complete(completed) = pending.answer(answers).unwrap() else {
        panic!()
    };
    assert_eq!(
        completed.answers.get(&Id::parse("second").unwrap()),
        Some(&toha::Answer::Text("Configured".into()))
    );
}
