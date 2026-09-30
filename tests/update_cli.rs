// ---
// relationships:
//   implements: architecture
// ---
//! The template-update route through the binary: `apply --baseline` records the
//! first snapshot, `apply --from ID` re-renders and merges from it, a re-render
//! that equals the base reports already-current, and an unrelated local edit
//! survives the merge.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

use serde_json::Value;

/// Run `git ARGS...` inside `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A folder template with one text question and one rendered file.
fn write_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
}

/// A fresh git target with one commit.
fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

/// Write the identity envelope naming `formal` around `answers`.
fn envelope(dir: &Path, name: &str, formal: &str, answers: Value) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": answers });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    path
}

#[test]
fn baseline_records_then_from_updates_and_detects_current() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());

    // 1. `apply --baseline` renders the whole template and records the first
    //    snapshot. The greeting file appears with the answered name.
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let output = command.output().unwrap();
    assert!(output.status.success(), "baseline failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "applied");
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Alice\n"
    );

    // Commit the applied file so the next update merges from a clean tree.
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // 2. A re-render with the same answers, from that snapshot, is already
    //    current: nothing changes.
    let mut same = support::isolated_command(iso.path());
    same.arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_alice);
    let output = same.output().unwrap();
    assert!(
        output.status.success(),
        "already-current failed: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "already-current", "{document}");

    // 3. An unrelated local edit is committed, then an update with a changed
    //    answer merges: the greeting is rewritten and the unrelated edit, held on
    //    HEAD (the merge's operator side), survives.
    std::fs::write(target.path().join("notes.txt"), "local\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "local notes"]);
    let env_bob = envelope(
        iso.path(),
        "bob.json",
        &formal,
        serde_json::json!({ "name": "Bob" }),
    );
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_bob);
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Bob\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("notes.txt")).unwrap(),
        "local\n",
        "the unrelated local file must survive the merge"
    );

    // 4. The new snapshot is listed alongside the first.
    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let output = list.output().unwrap();
    assert!(output.status.success());
    let document = support::first_document(&output.stdout);
    assert_eq!(document["snapshots"].as_array().unwrap().len(), 2);
}

#[test]
fn from_an_unknown_snapshot_is_an_error() {
    let iso = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    target_repo(target.path());

    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg("00000000000000000000000000");
    let output = command.output().unwrap();
    assert!(!output.status.success(), "unknown --from should refuse");
}
