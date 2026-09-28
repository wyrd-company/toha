// ---
// relationships:
//   implements: architecture
// ---
#[allow(dead_code)]
mod support;

use std::{fs, path::Path};
use toha::{
    AnswerError, Applied, ApplyOptions, Id, Interview, Plan, RawAnswer, RawAnswers, Seed, Template,
    hook::RecordingRunner,
};

fn run(fixture: &Path, target: &Path) -> Result<Vec<String>, (u8, String)> {
    let target = toha::staging::canonical_target(target).map_err(|error| (1, error.to_string()))?;
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
            // A flow stop/abort ends without a plan; report its messages, exit 0.
            Interview::Ended(ended) => return Ok(ended.messages),
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
    let expect = support::expectation(fixture);
    let plan =
        Plan::build(&template, &completed, &target).map_err(|error| (1, error.to_string()))?;
    assert_eq!(
        plan.before_apply,
        expect.before_apply,
        "{}",
        fixture.display()
    );
    assert_eq!(
        plan.after_apply,
        expect.after_apply,
        "{}",
        fixture.display()
    );
    let runner = expect
        .fail_hook
        .map(RecordingRunner::fail_at)
        .unwrap_or_default();
    let result = if expect.options.dry_run {
        Ok(Applied::Written {
            files: vec![],
            hooks_run: 0,
            after_apply: None,
        })
    } else {
        plan.apply(
            &target,
            ApplyOptions {
                force: expect.options.force,
                trusted: expect.options.trust,
            },
            &runner,
        )
        .map_err(|error| (1, error.to_string()))
    };
    let calls = runner.calls();
    let wanted: Vec<_> = expect
        .hooks
        .iter()
        .map(|h| (h.argv.clone(), h.cwd.clone()))
        .collect();
    assert_eq!(calls, wanted, "{}", fixture.display());
    match result? {
        Applied::Written { .. } => Ok(completed.messages),
        Applied::NeedsTrust(_) => Err((3, "hooks will not run without --trust".into())),
    }
}

fn planning_error(folder: &Path) -> String {
    let template = Template::load(folder).unwrap();
    let Interview::Complete(completed) = Interview::start(
        &template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
        },
    )
    .unwrap() else {
        panic!("expected complete interview")
    };
    let target = tempfile::tempdir().unwrap();
    let target = toha::staging::canonical_target(target.path()).unwrap();
    Plan::build(&template, &completed, &target)
        .unwrap_err()
        .to_string()
}

#[test]
fn file_rule_faults_retain_support_path_field_expression_and_engine_text() {
    let when = Path::new("tests/fixtures/err-files-when-eval/template");
    let error = planning_error(when);
    assert!(
        error.starts_with(&format!(
            "{}: template error in files[0].when `'bad' | dateformat`: ",
            when.join("part.txt").canonicalize().unwrap().display()
        )),
        "{error}"
    );
    assert!(
        error.contains("invalid operation: expected four digit year"),
        "{error}"
    );

    let each = Path::new("tests/fixtures/err-each-not-array/template");
    let error = planning_error(each);
    assert_eq!(
        error,
        format!(
            "{}: template error in files[0].each `42`: expected array",
            each.join("part.txt").canonicalize().unwrap().display()
        )
    );

    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(folder.path().join("part.txt"), "content").unwrap();
    fs::write(
        folder.path().join("template.yml"),
        "name: sample\nfiles:\n  - each: '[\"bad\"] as item'\n    source: part.txt\n    path: '{{ item | dateformat }}.txt'\n",
    )
    .unwrap();
    let error = planning_error(folder.path());
    assert!(
        error.starts_with(&format!(
            "{}: template error in files[0].path `{{{{ item | dateformat }}}}.txt`: ",
            folder.path().join("part.txt").display()
        )),
        "{error}"
    );
    assert!(
        error.contains("invalid operation: expected four digit year"),
        "{error}"
    );
}

#[test]
fn ordinary_path_fault_names_exact_segment_and_content_fault_stays_unchanged() {
    let folder = tempfile::tempdir().unwrap();
    let segment = "{{ label | dateformat }}";
    let source = folder
        .path()
        .join("template")
        .join(segment)
        .join("note.txt");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "content").unwrap();
    fs::write(
        folder.path().join("template.yml"),
        "name: sample\ndata: { label: bad }\n",
    )
    .unwrap();
    let error = planning_error(folder.path());
    assert!(
        error.starts_with(&format!(
            "{}: template error in path `{segment}`: ",
            source.display()
        )),
        "{error}"
    );
    assert!(
        error.contains("invalid operation: expected four digit year"),
        "{error}"
    );

    let content_folder = tempfile::tempdir().unwrap();
    fs::create_dir(content_folder.path().join("template")).unwrap();
    let content = content_folder.path().join("template/body.txt");
    fs::write(&content, "{{ 'bad' | dateformat }}").unwrap();
    fs::write(content_folder.path().join("template.yml"), "name: sample\n").unwrap();
    let error = planning_error(content_folder.path());
    assert!(
        error.starts_with(&format!("{}: invalid operation", content.display())),
        "{error}"
    );
    assert!(!error.contains("template error in path"), "{error}");
}

#[test]
fn planning_fault_is_byte_equal_through_direct_staged_and_crate_routes() {
    let template_path = Path::new("tests/fixtures/err-files-when-eval/template")
        .canonicalize()
        .unwrap();
    let target = tempfile::tempdir().unwrap();
    let answers = target.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();

    let direct_state = tempfile::tempdir().unwrap();
    let direct = support::isolated_command(direct_state.path())
        .arg("apply")
        .arg(support::folder_address(&template_path))
        .arg(target.path())
        .args(["--answers"])
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(direct.status.code(), Some(1));
    let direct_error = String::from_utf8(direct.stderr).unwrap();

    let staged_state = tempfile::tempdir().unwrap();
    let staged = support::isolated_command(staged_state.path())
        .arg("stage")
        .arg(support::folder_address(&template_path))
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(0));
    let applied = support::isolated_command(staged_state.path())
        .arg("apply")
        .arg(target.path())
        .output()
        .unwrap();
    assert_eq!(applied.status.code(), Some(1));
    assert_eq!(String::from_utf8(applied.stderr).unwrap(), direct_error);

    let template = Template::load(&template_path).unwrap();
    let Interview::Complete(completed) = Interview::start(
        &template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
        },
    )
    .unwrap() else {
        panic!("expected complete interview")
    };
    let canonical = toha::staging::canonical_target(target.path()).unwrap();
    let crate_error = Plan::build(&template, &completed, &canonical)
        .unwrap_err()
        .to_string();
    assert_eq!(direct_error, format!("{crate_error}\n"));
}

#[test]
fn every_fixture_through_library() {
    for fixture in support::fixtures() {
        let target = tempfile::tempdir().unwrap();
        let expect = support::expectation(&fixture);
        support::copy_tree(&fixture.join("existing"), target.path());
        let result = run(&fixture, target.path());
        let name = fixture.file_name().unwrap().to_string_lossy();
        match result {
            Ok(messages) => {
                assert_eq!(expect.exit, 0, "{name}");
                assert_eq!(messages, expect.messages, "{name}");
                support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
            }
            Err((exit, text)) => {
                assert_eq!(exit, expect.exit, "{name}: {text}");
                for part in expect.error_contains {
                    assert!(text.contains(&part), "{name}: missing {part:?} in {text:?}");
                }
                if fixture.join("expected").exists() {
                    support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
                } else {
                    assert!(
                        fs::read_dir(target.path()).unwrap().next().is_none(),
                        "{name}: wrote files on error"
                    );
                }
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
            Interview::Ended(_) => panic!("unexpected flow end"),
            Interview::Asking(p) => state = p.answer(RawAnswers::new()).unwrap(),
        }
    };
    let mut state = Interview::start(&template, seed()).unwrap();
    let batches = loop {
        match state {
            Interview::Complete(c) => break c.answers,
            Interview::Ended(_) => panic!("unexpected flow end"),
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

#[test]
fn false_when_waits_for_all_node_references() {
    for name in [
        "skip-boundary",
        "skip-prompt-boundary",
        "group-skip-boundary",
    ] {
        let fixture = Path::new("tests/fixtures").join(name);
        let template = Template::load(&fixture.join("template")).unwrap();
        let seed = Seed {
            now: support::expectation(&fixture).now.parse().unwrap(),
            defaults: indexmap::IndexMap::new(),
        };
        let Interview::Asking(first_batch) = Interview::start(&template, seed).unwrap() else {
            panic!("{name}: first batch")
        };
        let ids: Vec<_> = first_batch
            .batch()
            .items
            .iter()
            .filter_map(|item| match item {
                toha::Item::Prompt(prompt) => Some(prompt.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            ["first"],
            "{name}: skipped node ran ahead of its references"
        );
        let Interview::Asking(second_batch) = first_batch
            .answer(
                [(
                    Id::parse("first").unwrap(),
                    RawAnswer(serde_json::json!("ready")),
                )]
                .into_iter()
                .collect(),
            )
            .unwrap()
        else {
            panic!("{name}: second batch")
        };
        let ids: Vec<_> = second_batch
            .batch()
            .items
            .iter()
            .filter_map(|item| match item {
                toha::Item::Prompt(prompt) => Some(prompt.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["last"], "{name}: skipped default was not recorded");
    }
}

#[test]
fn check_preserves_format_evaluation_failure() {
    let fixture = Path::new("tests/fixtures/err-format-eval");
    let template = Template::load(&fixture.join("template")).unwrap();
    let seed = Seed {
        now: support::expectation(fixture).now.parse().unwrap(),
        defaults: indexmap::IndexMap::new(),
    };
    let Interview::Asking(pending) = Interview::start(&template, seed).unwrap() else {
        panic!()
    };
    let error = pending
        .check(
            &Id::parse("item").unwrap(),
            RawAnswer(serde_json::json!("ok")),
        )
        .unwrap_err();
    let toha::CheckError::Eval(error) = error else {
        panic!("format conversion must be evaluation failure")
    };
    assert_eq!(error.field, "format");
}

#[test]
fn false_group_waits_for_all_skipped_defaults_before_rendering_any() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(
        folder.path().join("template.yml"),
        r#"
name: sample
interview:
  - id: first
    type: text
    prompt: First?
  - group: hidden
    when: "false"
    nodes:
      - id: early
        type: text
        prompt: Early?
        default: "{{ 'value' | nope }}"
      - id: late
        type: text
        prompt: Late?
        default: "{{ first }}"
"#,
    )
    .unwrap();
    let template = Template::load(folder.path()).unwrap();
    let seed = Seed {
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        defaults: indexmap::IndexMap::new(),
    };
    let Interview::Asking(pending) = Interview::start(&template, seed).unwrap() else {
        panic!()
    };
    let ids: Vec<_> = pending
        .batch()
        .items
        .iter()
        .filter_map(|item| match item {
            toha::Item::Prompt(prompt) => Some(prompt.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["first"]);
}

#[test]
fn every_documented_example_loads() {
    for entry in fs::read_dir("docs/examples").unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            Template::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
