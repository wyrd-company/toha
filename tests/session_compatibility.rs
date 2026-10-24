// ---
// relationships:
//   verifies: project-updates
// ---
//! Command compatibility regressions for shared session adapters.
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
fn fixture(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
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
    (root, target, source)
}
fn show(name: &str, out: &std::process::Output) {
    eprintln!(
        "PROBE {name}: code={:?}\n--stdout--\n{}\n--stderr--\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

const ABORTABLE: &str = "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - flow: abort\n    when: label == 'cancel'\n";

#[test]
fn update_start_abort_writes_nothing() {
    update_start_end_writes_nothing("abort");
}
#[test]
fn update_start_stop_writes_nothing() {
    update_start_end_writes_nothing("stop");
}
fn update_start_end_writes_nothing(flow: &str) {
    let (root, target, source) =
        fixture(&ABORTABLE.replace("flow: abort", &format!("flow: {flow}")));
    let address = support::versioned_template(&source);
    let answers = root.path().join("baseline.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers": {"label": "example"}}).to_string(),
    )
    .unwrap();
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--baseline")
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    show("baseline", &out);
    assert!(out.status.success());
    git(&target, &["add", "."]);
    git(&target, &["commit", "-qm", "baseline"]);
    let record_before = std::fs::read(target.join(".toha.json")).unwrap();
    let result_before = std::fs::read(target.join("result.txt")).unwrap();
    std::fs::write(source.join("template/extra.txt"), "extra\n").unwrap();
    let address = support::refresh_template(&address);
    let cancel = root.path().join("cancel.json");
    std::fs::write(
        &cancel,
        json!({"template": address, "answers": {"label": "cancel"}}).to_string(),
    )
    .unwrap();
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--answers")
        .arg(&cancel)
        .output()
        .unwrap();
    show("update-abort-override", &out);
    assert!(
        !target.join("extra.txt").exists(),
        "probe_update_abort_override_wrote_extra"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        record_before,
        "probe_update_abort_override_record_changed"
    );
    assert_eq!(
        std::fs::read(target.join("result.txt")).unwrap(),
        result_before,
        "update_start_abort_target_changed"
    );
    assert!(
        walk(&support::staged_dir(root.path())).is_empty(),
        "update_start_abort_left_stage"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "probe_update_abort_override_exit"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the template ended the interview"),
        "probe_update_abort_override_message"
    );
}

#[test]
fn probe_scripted_parts_before_document_input() {
    let (root, target, source) = fixture(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n",
    );
    let address = support::versioned_template(&source);
    let answers = root.path().join("answers.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers": {"label": "example"}}).to_string(),
    )
    .unwrap();
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--answers")
        .arg(&answers)
        .arg("--with")
        .arg("extra")
        .arg("--input")
        .arg(root.path().join("missing-input.json"))
        .output()
        .unwrap();
    show("scripted-parts-and-input", &out);
    assert_eq!(
        out.status.code(),
        Some(2),
        "probe_scripted_parts_usage_exit"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("parts question"),
        "probe_scripted_parts_usage_first"
    );
}

#[test]
fn probe_seed_pin_diagnostic_text() {
    let (root, target, source) = fixture(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n",
    );
    let address = support::versioned_template(&source);
    let out = command(root.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .arg("--async")
        .output()
        .unwrap();
    show("stage", &out);
    assert_eq!(out.status.code(), Some(4));
    let dir = support::staged_dir(root.path());
    let files: Vec<_> = walk(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    record["seed"] = json!("elsewhere");
    std::fs::write(&files[0], serde_json::to_vec(&record).unwrap()).unwrap();
    let out = command(root.path())
        .arg("continue")
        .arg(&target)
        .output()
        .unwrap();
    show("continue-seed-pin", &out);
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("staged --like has no copied application; abort and stage again"),
        "probe_seed_pin_text"
    );
}
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if !dir.exists() {
        return out;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn probe_staged_update_rejection_keeps_full_cli_batch() {
    let (root, target, source) = fixture(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n",
    );
    let address = support::versioned_template(&source);
    let answers = root.path().join("baseline.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers": {"label": "example"}}).to_string(),
    )
    .unwrap();
    let out = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--baseline")
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    git(&target, &["add", "."]);
    git(&target, &["commit", "-qm", "baseline"]);
    std::fs::write(source.join("template.yml"), "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - {id: color, type: text, prompt: Color, required: true, validate: {min: 2}}\n").unwrap();
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
    show("stage-update", &out);
    assert_eq!(out.status.code(), Some(4));
    let staged = support::first_document(&out.stdout);
    let bad = root.path().join("bad.json");
    std::fs::write(
        &bad,
        json!({"template": address, "answers": {"color": "x"}}).to_string(),
    )
    .unwrap();
    let out = command(root.path())
        .arg("continue")
        .arg(&target)
        .arg(&bad)
        .output()
        .unwrap();
    show("continue-rejected", &out);
    assert_eq!(out.status.code(), Some(4));
    let rejected = support::first_document(&out.stdout);
    let keys = |d: &serde_json::Value| {
        d["schema"]["properties"]
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
    };
    eprintln!(
        "PROBE staged keys={:?} rejected keys={:?}",
        keys(&staged),
        keys(&rejected)
    );
    assert_eq!(
        keys(&rejected),
        keys(&staged),
        "probe_rejected_update_batch_shape"
    );
}

#[test]
fn update_accepted_continue_and_incomplete_apply_omit_empty_errors() {
    let (root, target, source) = fixture(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n",
    );
    let address = support::versioned_template(&source);
    let answers = root.path().join("baseline.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers":{"label":"example"}}).to_string(),
    )
    .unwrap();
    let baseline = command(root.path())
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--baseline")
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    assert!(baseline.status.success(), "{baseline:?}");
    git(&target, &["add", "."]);
    git(&target, &["commit", "-qm", "baseline"]);
    std::fs::write(source.join("template.yml"), "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - {id: color, type: text, prompt: Color, required: true}\n  - id: note\n    type: text\n    prompt: Note\n    required: true\n    when: color == 'blue'\n").unwrap();
    let address = support::refresh_template(&address);
    let staged = command(root.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .arg("--update")
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(4), "{staged:?}");
    std::fs::write(
        &answers,
        json!({"template":address, "answers":{"color":"blue"}}).to_string(),
    )
    .unwrap();
    let continued = command(root.path())
        .arg("continue")
        .arg(&target)
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(continued.status.code(), Some(4), "{continued:?}");
    let doc = support::first_document(&continued.stdout);
    assert!(doc["schema"]["properties"].get("note").is_some());
    assert!(
        doc.get("errors").is_none(),
        "cli_update_accepted_omits_empty_errors"
    );
    let before = std::fs::read(target.join(".toha.json")).unwrap();
    let pending = command(root.path())
        .arg("apply")
        .arg(&target)
        .output()
        .unwrap();
    assert_eq!(pending.status.code(), Some(4), "{pending:?}");
    let doc = support::first_document(&pending.stdout);
    assert!(doc["schema"]["properties"].get("note").is_some());
    assert!(
        doc.get("errors").is_none(),
        "cli_update_incomplete_apply_omits_empty_errors"
    );
    assert_eq!(std::fs::read(target.join(".toha.json")).unwrap(), before);
}

#[test]
fn update_resolves_and_admits_before_clock_but_config_before_operand() {
    let (root, target, source) = fixture("name: sample\ninterview: []\n");
    let missing = root.path().join("missing-template");
    let out = command(root.path())
        .env("TOHA_NOW", "invalid")
        .arg("apply")
        .arg(&missing)
        .arg(&target)
        .arg("--baseline")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        err.contains("missing-template") && !err.contains("TOHA_NOW"),
        "cli_update_resolution_before_clock: {out:?}"
    );
    let out = command(root.path())
        .env("TOHA_NOW", "invalid")
        .arg("apply")
        .arg(&source)
        .arg(&target)
        .arg("--update")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("TOHA_NOW"),
        "cli_update_application_before_clock: {out:?}"
    );
    std::fs::write(
        source.join("template.yml"),
        "name: sample\nrequires: [missing-fixture-program]\ninterview: []\n",
    )
    .unwrap();
    let out = command(root.path())
        .env("TOHA_NOW", "invalid")
        .arg("apply")
        .arg(&source)
        .arg(&target)
        .arg("--baseline")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("missing-fixture-program"),
        "cli_update_requirements_before_clock: {out:?}"
    );
    std::fs::create_dir_all(root.path().join("config/toha")).unwrap();
    std::fs::write(root.path().join("config/toha/config.yml"), "presets: [\n").unwrap();
    let out = command(root.path())
        .arg("apply")
        .arg(&target)
        .arg("--update")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("need a template operand"),
        "cli_update_config_before_operand: {out:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("config"),
        "cli_update_config_refusal: {out:?}"
    );
}
