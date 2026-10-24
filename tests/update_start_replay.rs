// ---
// relationships:
//   verifies: project-updates
// ---
//! Command regressions for update start outcomes and admission order.
#[allow(dead_code)]
mod support;
use serde_json::json;
use std::path::{Path, PathBuf};

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
fn command(root: &Path) -> std::process::Command {
    let mut cmd = support::isolated_command(root);
    cmd.current_dir(root)
        .env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]");
    cmd
}
fn show(name: &str, out: &std::process::Output) {
    eprintln!(
        "PROBE {name}: code={:?}\n--stdout--\n{}\n--stderr--\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
const LABEL: &str = "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true, validate: {min: 3}}\n";

/// Template + committed target with a baseline application (label=example).
fn baselined(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    baselined_answers(body, json!({"label": "example"}))
}
fn baselined_answers(
    body: &str,
    initial: serde_json::Value,
) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    applied_answers(body, initial, true)
}
fn applied_answers(
    body: &str,
    initial: serde_json::Value,
    baseline: bool,
) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let source = root.path().join("source");
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(source.join("template.yml"), body).unwrap();
    std::fs::write(source.join("template/result.txt"), "{{ label }}\n").unwrap();
    git(&target, &["init", "-q"]);
    std::fs::write(target.join("README.md"), "operator\n").unwrap();
    git(&target, &["add", "."]);
    git(&target, &["commit", "-qm", "fixture"]);
    let address = support::versioned_template(&source);
    let answers = root.path().join("baseline.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers": initial}).to_string(),
    )
    .unwrap();
    let mut apply = command(root.path());
    apply.arg("apply").arg(&address).arg(&target);
    if baseline {
        apply.arg("--baseline");
    }
    let out = apply.arg("--answers").arg(&answers).output().unwrap();
    show("baseline", &out);
    assert!(out.status.success(), "{out:?}");
    git(&target, &["add", "."]);
    git(&target, &["commit", "-qm", "baseline"]);
    (root, target, source, address)
}
fn answers_file(root: &Path, name: &str, address: &str, answers: serde_json::Value) -> PathBuf {
    let file = root.join(name);
    std::fs::write(
        &file,
        json!({"template": address, "answers": answers}).to_string(),
    )
    .unwrap();
    file
}

#[test]
fn probe_update_rejected_override_writes_nothing() {
    let (root, target, source, address) = baselined(LABEL);
    let record_before = std::fs::read(target.join(".toha.json")).unwrap();
    let result_before = std::fs::read(target.join("result.txt")).unwrap();
    std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
    let address = support::refresh_template(&address);
    let bad = answers_file(root.path(), "bad.json", &address, json!({"label": "x"}));
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--answers")
        .arg(&bad)
        .output()
        .unwrap();
    show("update-rejected-override", &out);
    assert!(
        !target.join("extra.txt").exists(),
        "probe_rejected_override_wrote_extra"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        record_before,
        "probe_rejected_override_record_changed"
    );
    assert_eq!(
        std::fs::read(target.join("result.txt")).unwrap(),
        result_before,
        "probe_rejected_override_result_changed"
    );
    assert!(
        walk(&support::staged_dir(root.path())).is_empty(),
        "probe_rejected_override_stage_unchanged"
    );
    assert_eq!(out.status.code(), Some(4), "probe_rejected_override_exit");
    let doc = support::first_document(&out.stdout);
    assert!(
        doc["errors"].get("label").is_some(),
        "probe_rejected_override_errors: {doc}"
    );
}

#[test]
fn probe_update_rejected_new_question_keeps_errors() {
    let (root, target, source, address) = baselined(LABEL);
    let record_before = std::fs::read(target.join(".toha.json")).unwrap();
    let result_before = std::fs::read(target.join("result.txt")).unwrap();
    std::fs::write(source.join("template.yml"), "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true, validate: {min: 3}}\n  - {id: color, type: text, prompt: Color, required: true, validate: {min: 2}}\n").unwrap();
    std::fs::write(source.join("template/color.txt"), "{{ color }}\n").unwrap();
    let address = support::refresh_template(&address);
    let bad = answers_file(root.path(), "bad.json", &address, json!({"color": "x"}));
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--answers")
        .arg(&bad)
        .output()
        .unwrap();
    show("update-rejected-new-question", &out);
    assert_eq!(out.status.code(), Some(4), "probe_new_question_exit");
    assert!(
        !target.join("color.txt").exists(),
        "probe_new_question_wrote"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        record_before,
        "probe_new_question_record_unchanged"
    );
    assert_eq!(
        std::fs::read(target.join("result.txt")).unwrap(),
        result_before,
        "probe_new_question_file_unchanged"
    );
    assert!(
        walk(&support::staged_dir(root.path())).is_empty(),
        "probe_new_question_stage_unchanged"
    );
    let doc = support::first_document(&out.stdout);
    assert!(
        doc["errors"].get("color").is_some(),
        "probe_new_question_errors_lost: {doc}"
    );
}

#[test]
fn probe_update_part_flag_usage_exit() {
    let (root, target, source, address) = baselined(LABEL);
    std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
    let address = support::refresh_template(&address);
    let ok = answers_file(root.path(), "ok.json", &address, json!({}));
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--answers")
        .arg(&ok)
        .arg("--with")
        .arg("extra")
        .output()
        .unwrap();
    show("update-with-no-parts-question", &out);
    let apply_code = out.status.code();
    let apply_parts = String::from_utf8_lossy(&out.stderr).contains("parts question");
    let out = command(root.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--async")
        .arg("--with")
        .arg("extra")
        .output()
        .unwrap();
    show("stage-update-with-no-parts-question", &out);
    let stage_code = out.status.code();
    assert!(!target.join("extra.txt").exists(), "probe_part_flag_wrote");
    assert_eq!(apply_code, Some(2), "probe_update_part_flag_usage_exit");
    assert!(apply_parts, "probe_update_part_flag_message");
    assert_eq!(
        stage_code,
        Some(2),
        "probe_stage_update_part_flag_usage_exit"
    );
}

#[test]
fn probe_update_parts_before_document_input() {
    let (root, target, source, address) = baselined(LABEL);
    std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
    let address = support::refresh_template(&address);
    let ok = answers_file(root.path(), "ok.json", &address, json!({}));
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--answers")
        .arg(&ok)
        .arg("--with")
        .arg("extra")
        .arg("--input")
        .arg(root.path().join("missing-input.json"))
        .output()
        .unwrap();
    show("update-parts-and-input", &out);
    assert_eq!(out.status.code(), Some(2), "probe_update_parts_first_exit");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("parts question"),
        "probe_update_parts_first_message"
    );
}

#[test]
fn probe_stage_update_initial_errors_member() {
    let (root, target, source, address) = baselined(LABEL);
    std::fs::write(source.join("template.yml"), "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true, validate: {min: 3}}\n  - {id: color, type: text, prompt: Color, required: true}\n").unwrap();
    std::fs::write(source.join("template/color.txt"), "{{ color }}\n").unwrap();
    let address = support::refresh_template(&address);
    let out = command(root.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--async")
        .output()
        .unwrap();
    show("stage-update-initial", &out);
    assert_eq!(out.status.code(), Some(4));
    let doc = support::first_document(&out.stdout);
    eprintln!(
        "PROBE stage-update-initial has_errors_member={}",
        doc.get("errors").is_some()
    );
    assert!(
        doc.get("errors").is_some(),
        "probe_stage_update_initial_errors_member"
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[test]
fn update_start_valid_and_ended_outcomes_cover_recorded_and_new_questions() {
    for new_question in [false, true] {
        for value in ["changed", "abort", "stop"] {
            let (root, target, source, address) = baselined(LABEL);
            let record_before = std::fs::read(target.join(".toha.json")).unwrap();
            let result_before = std::fs::read(target.join("result.txt")).unwrap();
            let id = if new_question { "color" } else { "label" };
            let extra = if new_question {
                "  - {id: color, type: text, prompt: Color, required: true, validate: {min: 3}}\n"
            } else {
                ""
            };
            std::fs::write(source.join("template.yml"), format!("{LABEL}{extra}  - flow: abort\n    when: {id} == 'abort'\n  - flow: stop\n    when: {id} == 'stop'\n")).unwrap();
            std::fs::write(
                source.join("template/extra.txt"),
                format!("{{{{ {id} }}}}\n"),
            )
            .unwrap();
            let address = support::refresh_template(&address);
            let answers = answers_file(root.path(), "answers.json", &address, json!({id: value}));
            let out = command(root.path())
                .args(["apply", &address])
                .arg(&target)
                .args(["--update", "--answers"])
                .arg(&answers)
                .output()
                .unwrap();
            show(&format!("matrix-{new_question}-{value}"), &out);
            if value == "changed" {
                assert!(out.status.success(), "cli_start_valid_outcome");
                assert_eq!(
                    std::fs::read_to_string(target.join("extra.txt")).unwrap(),
                    "changed\n",
                    "cli_start_valid_override_used"
                );
            } else {
                assert!(
                    !target.join("extra.txt").exists(),
                    "cli_start_ended_no_write"
                );
                assert_eq!(
                    std::fs::read(target.join(".toha.json")).unwrap(),
                    record_before,
                    "cli_start_ended_record_unchanged"
                );
                assert_eq!(
                    std::fs::read(target.join("result.txt")).unwrap(),
                    result_before,
                    "cli_start_ended_file_unchanged"
                );
                assert_eq!(out.status.code(), Some(1), "cli_start_ended_exit");
            }
            assert!(
                walk(&support::staged_dir(root.path())).is_empty(),
                "cli_start_matrix_stage_unchanged"
            );
        }
    }
}

#[test]
fn staged_update_documents_still_precede_parts() {
    let (root, target, source, address) = baselined(LABEL);
    std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
    let address = support::refresh_template(&address);
    let out = command(root.path())
        .args(["stage", &address])
        .arg(&target)
        .args(["--update", "--async", "--with", "extra", "--input"])
        .arg(root.path().join("missing-input.json"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "cli_stage_update_document_precedes_parts"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing-input.json"));
    assert!(walk(&support::staged_dir(root.path())).is_empty());
}

#[test]
fn update_replays_optional_nulls_and_refuses_new_required_nulls() {
    for required in [None, Some("remote"), Some("verbose")] {
        let body = |required: Option<&str>| {
            format!(
                "{LABEL}  - {{id: remote, type: git, prompt: Remote, required: {}}}\n  - {{id: verbose, type: confirm, prompt: Verbose, required: {}}}\n",
                required == Some("remote"),
                required == Some("verbose")
            )
        };
        let (root, target, source, address) = applied_answers(
            &body(None),
            json!({"label": "example", "remote": null, "verbose": null}),
            false,
        );
        // The first application explicitly records empty optional answers.
        let record_before = std::fs::read(target.join(".toha.json")).unwrap();
        let result_before = std::fs::read(target.join("result.txt")).unwrap();
        std::fs::write(source.join("template.yml"), body(required)).unwrap();
        std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
        let address = support::refresh_template(&address);
        let answers = answers_file(root.path(), "answers.json", &address, json!({}));
        let out = command(root.path())
            .args(["apply", &address])
            .arg(&target)
            .args(["--update", "--answers"])
            .arg(&answers)
            .output()
            .unwrap();
        show("null-replay", &out);
        if let Some(id) = required {
            assert!(
                !target.join("extra.txt").exists(),
                "cli_required_null_no_write"
            );
            assert_eq!(
                std::fs::read(target.join(".toha.json")).unwrap(),
                record_before,
                "cli_required_null_record_unchanged"
            );
            assert_eq!(
                std::fs::read(target.join("result.txt")).unwrap(),
                result_before
            );
            assert_eq!(out.status.code(), Some(4), "cli_required_null_exit");
            assert!(
                support::first_document(&out.stdout)["errors"]
                    .get(id)
                    .is_some(),
                "cli_required_null_structured_error"
            );
        } else {
            assert!(out.status.success(), "cli_optional_null_replay_admitted");
            let applied = toha::applied::AppliedRecord::parse(
                &std::fs::read(target.join(".toha.json")).unwrap(),
            )
            .unwrap();
            let submissions = applied.applications()[0].submissions();
            for id in ["remote", "verbose"] {
                assert_eq!(
                    submissions.iter().find_map(|s| s.get(id)),
                    Some(&json!(null)),
                    "cli_optional_null_persisted"
                );
            }
        }
        assert!(walk(&support::staged_dir(root.path())).is_empty());
    }
}
