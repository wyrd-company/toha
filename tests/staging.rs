// ---
// relationships:
//   implements: interview-protocol
// ---
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use toha::{
    AnswerError, Applied, ApplyOptions, Interview, Plan, Seed, Template,
    hook::RecordingRunner,
    protocol::{self, Context, Headless},
    staging::{StagedRecord, Store, canonical_target},
};

fn schema_validator() -> jsonschema::Validator {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(protocol::protocol_schema())
        .unwrap()
}
fn context(target: &Path, template: impl Into<String>, commit: Option<String>) -> Context {
    let target = canonical_target(target).unwrap();
    let record = StagedRecord {
        target: target.as_path().to_owned(),
        template: template.into(),
        commit: commit.unwrap_or_default(),
        named: false,
        now: "2026-01-02T03:04:05+00:00[UTC]".into(),
        submissions: vec![],
    };
    Context::new(&target, &record)
}
#[test]
fn optional_null_validates_against_batch_schema() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(folder.path().join("template.yml"), "name: sample\ninterview:\n  - { id: text, type: text, prompt: Text? }\n  - { id: confirm, type: confirm, prompt: Confirm? }\n  - { id: choices, type: multiselect, prompt: Choices?, options: [one] }\n  - { id: select, type: select, prompt: Select?, options: [one] }\n").unwrap();
    let template = Template::load(folder.path()).unwrap();
    let Interview::Asking(pending) = Interview::start(
        &template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
        },
    )
    .unwrap() else {
        panic!("expected questions")
    };
    let batch = protocol::batch_document(
        pending.batch(),
        &context(Path::new("target"), "sample", None),
        None,
    );
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&batch["schema"])
        .unwrap();
    let answers = json!({"text": null, "confirm": null, "choices": null, "select": null});
    assert!(validator.is_valid(&answers), "{batch}");
}
fn valid(document: &Value, validator: &jsonschema::Validator) {
    if let Err(error) = validator.validate(document) {
        panic!("invalid protocol document: {error}: {document}");
    }
    if document["status"] == "questions" {
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&document["schema"])
            .unwrap();
    }
}
fn command(state: &Path) -> Command {
    support::isolated_command(state)
}
fn output_document(
    output: &std::process::Output,
    code: i32,
    validator: &jsonschema::Validator,
) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    valid(&value, validator);
    value
}
fn send(mut cmd: Command, answers: &Value) -> std::process::Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn fixture_answers(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path.join("answers.json")).unwrap()).unwrap()
}
fn batch_answers(batch: &Value, answers: &Value) -> Value {
    let mut object = serde_json::Map::new();
    for id in batch["schema"]["properties"].as_object().unwrap().keys() {
        if let Some(value) = answers.get(id) {
            object.insert(id.clone(), value.clone());
        }
    }
    Value::Object(object)
}
fn one_shot(path: &Path, target: &Path) -> Value {
    let template = Template::load(&path.join("template")).unwrap();
    let seed = Seed {
        now: support::expectation(path).now.parse().unwrap(),
        defaults: Default::default(),
    };
    let interview = Interview::start(&template, seed).unwrap();
    let raw = protocol::parse_answers(&fixture_answers(path).to_string()).unwrap();
    let mut interview = interview;
    let mut raw = raw;
    let completed = loop {
        match interview {
            Interview::Complete(completed) => break completed,
            Interview::Asking(pending) => {
                interview = pending
                    .answer(std::mem::take(&mut raw))
                    .unwrap_or_else(|e| match e {
                        AnswerError::Rejected { rejections, .. } => {
                            panic!("one-shot rejection: {rejections:?}")
                        }
                        AnswerError::Eval(e) => panic!("one-shot evaluation: {e}"),
                    });
            }
        }
    };
    protocol::complete_document(
        &completed,
        &context(
            target,
            path.join("template")
                .canonicalize()
                .unwrap()
                .to_string_lossy(),
            None,
        ),
    )
}
#[test]
fn every_success_fixture_through_staged_cli() {
    let validator = schema_validator();
    for fixture in support::fixtures() {
        let expect = support::expectation(&fixture);
        if expect.exit != 0
            || expect.options.dry_run
            || !expect.cli
            || (!expect.hooks.is_empty() && fixture.file_name().unwrap() != "script-cli")
        {
            continue;
        }
        let state = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        support::copy_tree(&fixture.join("existing"), target.path());
        let template = fixture.join("template").canonicalize().unwrap();
        let initial = command(state.path())
            .args([
                "stage",
                support::folder_address(&template).as_str(),
                target.path().to_str().unwrap(),
                "--async",
            ])
            .env("TOHA_NOW", &expect.now)
            .output()
            .unwrap();
        let mut document = output_document(
            &initial,
            if initial.status.code() == Some(0) {
                0
            } else {
                4
            },
            &validator,
        );
        let answers = fixture_answers(&fixture);
        let mut batches = 0;
        while document["status"] == "questions" {
            batches += 1;
            assert!(batches < 20, "staged loop: {}", fixture.display());
            let submission = batch_answers(&document, &answers);
            let mut cmd = command(state.path());
            cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
            let output = send(cmd, &submission);
            document = output_document(
                &output,
                if output.status.code() == Some(0) {
                    0
                } else {
                    4
                },
                &validator,
            );
            assert!(
                document.get("errors").is_none(),
                "{}: {document}",
                fixture.display()
            );
        }
        assert_eq!(
            document["answers"],
            one_shot(&fixture, target.path())["answers"],
            "{}",
            fixture.display()
        );
        let mut apply = command(state.path());
        apply.args(["apply", target.path().to_str().unwrap()]);
        if expect.options.force {
            apply.arg("--force");
        }
        if expect.options.trust {
            apply.arg("--trust");
        }
        let output = apply.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: {}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
        assert!(
            !support::staged_dir(state.path())
                .read_dir()
                .unwrap()
                .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "json")),
            "{}",
            fixture.display()
        );
    }
}
#[test]
fn every_success_fixture_through_library_replay() {
    for fixture in support::fixtures() {
        let expect = support::expectation(&fixture);
        if expect.exit != 0 || expect.options.dry_run {
            continue;
        }
        let target = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        support::copy_tree(&fixture.join("existing"), target.path());
        let template = Template::load(&fixture.join("template")).unwrap();
        let formal = fixture
            .join("template")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let canonical = canonical_target(target.path()).unwrap();
        let mut record = StagedRecord {
            target: canonical.as_path().to_owned(),
            template: formal.clone(),
            commit: String::new(),
            named: false,
            now: expect.now.clone(),
            submissions: vec![],
        };
        let store = Store::new(state.path().to_path_buf());
        store.save(&canonical, &record).unwrap();
        let raw = protocol::parse_answers(&fixture_answers(&fixture).to_string()).unwrap();
        let Headless::Completed {
            completed,
            accepted,
        } = protocol::answer_headless(&template, record.replay(&template).unwrap(), raw).unwrap()
        else {
            panic!("fixture did not complete: {}", fixture.display());
        };
        for submission in accepted {
            record.submissions.push(submission);
            store.save(&canonical, &record).unwrap();
        }
        let Interview::Complete(replayed) = store
            .load(&canonical)
            .unwrap()
            .unwrap()
            .replay(&template)
            .unwrap()
        else {
            panic!("replay incomplete: {}", fixture.display());
        };
        let ctx = Context::new(&canonical, &record);
        assert_eq!(
            protocol::complete_document(&replayed, &ctx),
            protocol::complete_document(&completed, &ctx),
            "{}",
            fixture.display()
        );
        let plan = Plan::build(&template, &replayed, &canonical).unwrap();
        let runner = RecordingRunner::default();
        let result = plan
            .apply(
                &canonical,
                ApplyOptions {
                    force: expect.options.force,
                    trusted: expect.options.trust,
                },
                &runner,
            )
            .unwrap();
        assert!(
            matches!(result, Applied::Written { .. }),
            "{}",
            fixture.display()
        );
        support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
    }
}
#[test]
fn rejected_continue_preserves_record_and_abort_is_idempotent() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let initial = command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(initial.status.code(), Some(4));
    let store = Store::new(support::staged_dir(state.path()));
    let target_path = canonical_target(target.path()).unwrap();
    let before = fs::read(store.path_for(&target_path)).unwrap();
    let mut cmd = command(state.path());
    cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
    let output = send(cmd, &json!({}));
    assert_eq!(output.status.code(), Some(4));
    assert!(
        serde_json::from_slice::<Value>(&output.stdout)
            .unwrap()
            .get("errors")
            .is_some()
    );
    assert_eq!(fs::read(store.path_for(&target_path)).unwrap(), before);
    for _ in 0..2 {
        let output = command(state.path())
            .args(["abort", target.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
    }
}
#[test]
fn missing_required_stages_then_continues_and_applies() {
    let fixture = Path::new("tests/fixtures/text-basic");
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = fixture.join("template").canonicalize().unwrap();
    let empty = state.path().join("empty.json");
    fs::write(&empty, "{}").unwrap();
    let output = command(state.path())
        .args([
            "apply",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--answers",
            empty.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let store = Store::new(support::staged_dir(state.path()));
    assert!(
        store
            .load(&canonical_target(target.path()).unwrap())
            .unwrap()
            .is_some()
    );
    let mut cmd = command(state.path());
    cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
    let output = send(cmd, &fixture_answers(fixture));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = command(state.path())
        .args(["apply", target.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    support::assert_tree(target.path(), &fixture.join("expected"), fixture);
}
#[test]
fn duplicate_stage_and_missing_staged_apply() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let template_address = support::folder_address(&template);
    let args = [
        "stage",
        template_address.as_str(),
        target.path().to_str().unwrap(),
        "--async",
    ];
    assert_eq!(
        command(state.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .code(),
        Some(4)
    );
    assert_eq!(
        command(state.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        command(state.path())
            .args([
                "apply",
                support::folder_address(&template).as_str(),
                target.path().to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .code(),
        Some(4)
    );
    assert_eq!(
        command(state.path())
            .args(["abort", target.path().to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        command(state.path())
            .args(["apply", target.path().to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
}
#[test]
fn canonical_nonexistent_target_normalizes_components() {
    let root = tempfile::tempdir().unwrap();
    let expected = root.path().canonicalize().unwrap().join("b/c");
    assert_eq!(
        canonical_target(&root.path().join("a/../b/./c"))
            .unwrap()
            .as_path(),
        expected.as_path()
    );
}

#[test]
fn canonical_targets_have_one_stable_spelling_for_every_path_class() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("existing");
    fs::create_dir(&existing).unwrap();
    let existing_with_separator = PathBuf::from(format!(
        "{}{}",
        existing.display(),
        std::path::MAIN_SEPARATOR
    ));
    assert_eq!(
        canonical_target(&existing).unwrap(),
        canonical_target(&existing_with_separator).unwrap()
    );
    assert_eq!(
        canonical_target(&existing)
            .unwrap()
            .as_path()
            .to_string_lossy(),
        existing.canonicalize().unwrap().to_string_lossy()
    );

    let current = std::env::current_dir().unwrap();
    assert_eq!(
        canonical_target(Path::new(".")).unwrap().as_path(),
        current.canonicalize().unwrap()
    );
    let current_name = current.file_name().unwrap();
    let parent_relative = Path::new("..").join(current_name).join("new-target");
    assert_eq!(
        canonical_target(&parent_relative).unwrap().as_path(),
        current.canonicalize().unwrap().join("new-target")
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&existing, root.path().join("alias")).unwrap();
        assert_eq!(
            canonical_target(&root.path().join("alias/child"))
                .unwrap()
                .as_path(),
            existing.canonicalize().unwrap().join("child")
        );
        assert_eq!(
            canonical_target(Path::new("/")).unwrap().as_path(),
            Path::new("/")
        );
    }
}

#[test]
fn target_consumers_do_not_construct_a_second_identity() {
    for (name, source) in [
        ("apply", include_str!("../src/apply.rs")),
        ("plan", include_str!("../src/plan.rs")),
        ("protocol", include_str!("../src/protocol.rs")),
    ] {
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(
            !production.contains("canonical_target("),
            "{name} must consume CanonicalTarget instead of normalizing again"
        );
    }
}

#[test]
fn legacy_separator_key_migrates_only_after_a_successful_save() {
    let state = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let target = canonical_target(target_dir.path()).unwrap();
    let mut legacy_text = target.as_path().as_os_str().to_os_string();
    legacy_text.push(std::path::MAIN_SEPARATOR_STR);
    let legacy_key = format!(
        "{:x}.json",
        Sha256::digest(Path::new(&legacy_text).to_string_lossy().as_bytes())
    );
    let legacy_path = state.path().join(legacy_key);
    let record = StagedRecord {
        target: PathBuf::from(&legacy_text),
        template: "sample".into(),
        commit: String::new(),
        named: false,
        now: "2026-01-02T03:04:05+00:00[UTC]".into(),
        submissions: vec![],
    };
    fs::write(&legacy_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let store = Store::new(state.path().to_owned());
    assert!(store.load(&target).unwrap().is_some());

    let canonical_path = store.path_for(&target);
    fs::create_dir(&canonical_path).unwrap();
    assert!(store.save(&target, &record).is_err());
    assert!(legacy_path.exists());
    fs::remove_dir(&canonical_path).unwrap();

    store.save(&target, &record).unwrap();
    assert!(!legacy_path.exists());
    let stored: StagedRecord = serde_json::from_slice(&fs::read(&canonical_path).unwrap()).unwrap();
    assert_eq!(stored.target, target.as_path());
}
#[test]
fn explicit_null_differs_from_missing_in_one_shot_and_staged() {
    let fixture = Path::new("tests/fixtures/text-default");
    let template = Template::load(&fixture.join("template")).unwrap();
    let seed = || Seed {
        now: support::expectation(fixture).now.parse().unwrap(),
        defaults: Default::default(),
    };
    for (second, expected) in [(None, json!("abc")), (Some(Value::Null), Value::Null)] {
        let mut values = serde_json::Map::new();
        values.insert("first".into(), json!("ABC"));
        if let Some(value) = second.clone() {
            values.insert("second".into(), value);
        }
        let raw = protocol::parse_answers(&Value::Object(values.clone()).to_string()).unwrap();
        let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
            panic!()
        };
        let mut interview = pending.answer(raw).unwrap();
        let one_shot = loop {
            match interview {
                Interview::Complete(completed) => break completed,
                Interview::Asking(pending) => {
                    interview = pending.answer(Default::default()).unwrap()
                }
            }
        };
        assert_eq!(
            one_shot
                .answers
                .get(&toha::Id::parse("second").unwrap())
                .unwrap()
                .to_json(),
            expected
        );
        let state = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let template_path = fixture.join("template").canonicalize().unwrap();
        let initial = command(state.path())
            .args([
                "stage",
                support::folder_address(&template_path).as_str(),
                target.path().to_str().unwrap(),
                "--async",
            ])
            .env("TOHA_NOW", support::expectation(fixture).now)
            .output()
            .unwrap();
        assert_eq!(initial.status.code(), Some(4));
        let first_batch: Value = serde_json::from_slice(&initial.stdout).unwrap();
        assert!(first_batch["schema"]["properties"].get("first").is_some());
        let mut cmd = command(state.path());
        cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
        let output = send(cmd, &json!({"first":"ABC"}));
        assert_eq!(output.status.code(), Some(4));
        let second_batch: Value = serde_json::from_slice(&output.stdout).unwrap();
        let Interview::Asking(one_pending) = Interview::start(&template, seed()).unwrap() else {
            panic!()
        };
        let Interview::Asking(one_pending) = one_pending
            .answer(protocol::parse_answers(r#"{"first":"ABC"}"#).unwrap())
            .unwrap()
        else {
            panic!()
        };
        let ctx = context(target.path(), template_path.to_string_lossy(), None);
        assert_eq!(
            second_batch,
            protocol::batch_document(one_pending.batch(), &ctx, None)
        );
        let submission = if let Some(value) = second {
            json!({"second":value})
        } else {
            json!({})
        };
        let mut cmd = command(state.path());
        cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
        let output = send(cmd, &submission);
        assert_eq!(output.status.code(), Some(0));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["answers"]["second"], expected);
    }
}
#[test]
fn skipped_group_has_same_batch_boundary_after_replay() {
    let fixture = Path::new("tests/fixtures/branching-example");
    let template = Template::load(&fixture.join("template")).unwrap();
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template_path = fixture.join("template").canonicalize().unwrap();
    let initial = command(state.path())
        .args([
            "stage",
            support::folder_address(&template_path).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(initial.status.code(), Some(4));
    let first: Value = serde_json::from_slice(&initial.stdout).unwrap();
    let first_answers = batch_answers(
        &first,
        &json!({"site_name":"Example", "theme":"light", "search":false}),
    );
    let mut cmd = command(state.path());
    cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
    let output = send(cmd, &first_answers);
    assert_eq!(output.status.code(), Some(4));
    let next: Value = serde_json::from_slice(&output.stdout).unwrap();
    let store = Store::new(support::staged_dir(state.path()));
    let canonical = canonical_target(target.path()).unwrap();
    let saved = store.load(&canonical).unwrap().unwrap();
    let Interview::Asking(replayed) = saved.replay(&template).unwrap() else {
        panic!()
    };
    let ctx = Context::new(&canonical, &saved);
    assert_eq!(next, protocol::batch_document(replayed.batch(), &ctx, None));
    assert!(next["schema"]["properties"].get("search_engine").is_none());
}
#[test]
fn evaluation_failure_exits_one_and_preserves_staged_bytes() {
    let fixture = Path::new("tests/fixtures/err-default-render");
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = fixture.join("template").canonicalize().unwrap();
    let initial = command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(initial.status.code(), Some(4));
    let store = Store::new(support::staged_dir(state.path()));
    let record_path = store.path_for(&canonical_target(target.path()).unwrap());
    let before = fs::read(&record_path).unwrap();
    let mut cmd = command(state.path());
    cmd.args(["continue", target.path().to_str().unwrap(), "-"]);
    let output = send(cmd, &json!({"first":"value"}));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("second.default"));
    assert_eq!(fs::read(record_path).unwrap(), before);
}
#[test]
fn async_file_pending_apply_and_invalid_answers_codes() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let batch_file = state.path().join("batch.json");
    let stage = command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
            batch_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4));
    assert!(stage.stdout.is_empty());
    let batch: Value = serde_json::from_slice(&fs::read(batch_file).unwrap()).unwrap();
    valid(&batch, &schema_validator());
    let pending = command(state.path())
        .args(["apply", target.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(pending.status.code(), Some(4));
    assert_eq!(
        serde_json::from_slice::<Value>(&pending.stdout).unwrap(),
        batch
    );
    for document in ["not json", r#"{"Invalid":true}"#] {
        let bad = state.path().join("bad.json");
        fs::write(&bad, document).unwrap();
        let output = command(state.path())
            .args([
                "continue",
                target.path().to_str().unwrap(),
                bad.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
    }
    let store = Store::new(support::staged_dir(state.path()));
    assert_eq!(
        store
            .load(&canonical_target(target.path()).unwrap())
            .unwrap()
            .unwrap()
            .submissions
            .len(),
        0
    );
}
#[test]
fn staged_dry_run_and_needs_trust_keep_record() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/hooks-untrusted/template")
        .canonicalize()
        .unwrap();
    let stage = command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(0));
    let store = Store::new(support::staged_dir(state.path()));
    let target_path = canonical_target(target.path()).unwrap();
    for (flags, expected) in [(vec!["--dry-run"], 0), (vec![], 3)] {
        let mut cmd = command(state.path());
        cmd.args(["apply", target.path().to_str().unwrap()])
            .args(flags);
        let output = cmd.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(store.load(&target_path).unwrap().is_some());
    }
}
#[test]
fn stage_with_no_questions_emits_complete_and_saves() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(
        folder.path().join("template.yml"),
        "name: empty\nsource: .\n",
    )
    .unwrap();
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let output = command(state.path())
        .args([
            "stage",
            folder.path().to_str().unwrap(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    valid(&value, &schema_validator());
    assert_eq!(value["status"], "complete");
    let store = Store::new(support::staged_dir(state.path()));
    assert!(
        store
            .load(&canonical_target(target.path()).unwrap())
            .unwrap()
            .is_some()
    );
}
#[test]
fn headless_ignores_defined_skipped_id_but_rejects_unknown_id() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(folder.path().join("template.yml"), "name: sample\ninterview:\n  - id: enabled\n    type: confirm\n    prompt: Enabled?\n  - id: hidden\n    type: text\n    prompt: Hidden?\n    when: enabled\n").unwrap();
    let template = Template::load(folder.path()).unwrap();
    let seed = || Seed {
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        defaults: Default::default(),
    };
    let interview = Interview::start(&template, seed()).unwrap();
    let raw = protocol::parse_answers(r#"{"enabled":false,"hidden":"ignored"}"#).unwrap();
    let Headless::Completed { completed, .. } =
        protocol::answer_headless(&template, interview, raw).unwrap()
    else {
        panic!()
    };
    let baseline = protocol::answer_headless(
        &template,
        Interview::start(&template, seed()).unwrap(),
        protocol::parse_answers(r#"{"enabled":false}"#).unwrap(),
    )
    .unwrap();
    let Headless::Completed {
        completed: baseline,
        ..
    } = baseline
    else {
        panic!()
    };
    assert_eq!(completed.answers, baseline.answers);
    let interview = Interview::start(&template, seed()).unwrap();
    let raw = protocol::parse_answers(r#"{"enabled":false,"unknown":"bad"}"#).unwrap();
    let Headless::Pending { rejections, .. } =
        protocol::answer_headless(&template, interview, raw).unwrap()
    else {
        panic!()
    };
    assert!(rejections.iter().any(|r| r.id.as_str() == "unknown"));
    let empty_folder = tempfile::tempdir().unwrap();
    fs::write(
        empty_folder.path().join("template.yml"),
        "name: empty\nsource: .\n",
    )
    .unwrap();
    let empty = Template::load(empty_folder.path()).unwrap();
    let result = protocol::answer_headless(
        &empty,
        Interview::start(&empty, seed()).unwrap(),
        protocol::parse_answers(r#"{"unknown":"bad"}"#).unwrap(),
    );
    assert!(result.is_err());
}
#[test]
fn replay_stores_raw_answer_before_non_idempotent_format() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(folder.path().join("template.yml"), "name: sample\ninterview:\n  - id: value\n    type: text\n    prompt: Value?\n    format: value ~ 'x'\n").unwrap();
    let template = Template::load(folder.path()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let canonical = canonical_target(target.path()).unwrap();
    let record = StagedRecord {
        target: canonical.as_path().to_owned(),
        template: folder.path().to_string_lossy().into_owned(),
        commit: String::new(),
        named: false,
        now: "2026-01-02T03:04:05+00:00[UTC]".into(),
        submissions: vec![indexmap::indexmap! { "value".into() => json!("a") }],
    };
    let store = Store::new(state.path().to_path_buf());
    store.save(&canonical, &record).unwrap();
    let Interview::Complete(completed) = store
        .load(&canonical)
        .unwrap()
        .unwrap()
        .replay(&template)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        completed
            .answers
            .get(&toha::Id::parse("value").unwrap())
            .unwrap()
            .to_json(),
        json!("ax")
    );
    assert_eq!(
        store.load(&canonical).unwrap().unwrap().submissions[0]["value"],
        json!("a")
    );
}
#[test]
fn folder_template_context_has_null_commit() {
    let validator = schema_validator();
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let output = command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    let document = output_document(&output, 4, &validator);
    assert_eq!(document["context"]["commit"], Value::Null, "{document}");
}
#[test]
fn optional_select_enum_lists_only_its_options() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(
        folder.path().join("template.yml"),
        "name: sample\ninterview:\n  - { id: optional, type: select, prompt: Optional?, options: [one, two] }\n  - { id: needed, type: select, prompt: Needed?, options: [one, two], required: true }\n",
    )
    .unwrap();
    let template = Template::load(folder.path()).unwrap();
    let Interview::Asking(pending) = Interview::start(
        &template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
        },
    )
    .unwrap() else {
        panic!("expected questions")
    };
    let batch = protocol::batch_document(
        pending.batch(),
        &context(Path::new("target"), "sample", None),
        None,
    );
    let properties = &batch["schema"]["properties"];
    assert_eq!(
        properties["optional"]["anyOf"],
        json!([{"enum": ["one", "two"]}, {"type": "null"}]),
        "{batch}"
    );
    assert!(properties["optional"].get("enum").is_none(), "{batch}");
    assert_eq!(
        properties["needed"]["enum"],
        json!(["one", "two"]),
        "{batch}"
    );
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&batch["schema"])
        .unwrap();
    for (optional, valid) in [
        (json!("one"), true),
        (Value::Null, true),
        (json!("three"), false),
    ] {
        assert_eq!(
            validator.is_valid(&json!({"optional": optional, "needed": "two"})),
            valid,
            "{optional}"
        );
    }
}
