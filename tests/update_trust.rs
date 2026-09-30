// ---
// relationships:
//   implements: architecture
// ---
//! An update runs its template's hooks under an established registry approval,
//! without a fresh `--trust`, and an unapproved one still refuses. This exercises
//! the trust preflight through the real installed-template routes.

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;

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

fn toha(root: &TempDir, args: &[&str]) -> std::process::Output {
    support::isolated_command(root.path())
        .args(args)
        .output()
        .unwrap()
}

/// A folder template with one question, a rendered file, and a hook that writes
/// `ran.txt`, so a hook run is observable in the target after a merge.
fn write_template(folder: &Path) {
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: trustdemo\ndescription: A greeting with a hook\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\nhooks:\n  - run: [ touch, ran.txt ]\n",
    )
    .unwrap();
}

fn envelope(
    root: &TempDir,
    name: &str,
    formal: &str,
    value: serde_json::Value,
) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": value });
    let path = root.path().join(name);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    path
}

#[test]
fn an_approved_template_updates_and_runs_hooks_without_trust() {
    let root = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    let folder = template_dir.path().canonicalize().unwrap();
    let formal = support::formal_name(&folder);

    // Install and approve the template.
    let add = toha(
        &root,
        &[
            "templates",
            "add",
            folder.to_str().unwrap(),
            "--alias",
            "demo",
        ],
    );
    assert_eq!(add.status.code(), Some(0), "add: {add:?}");
    let trust = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(trust.status.code(), Some(0), "trust: {trust:?}");

    // A git target.
    git(target.path(), &["init", "--quiet"]);
    fs::write(target.path().join("README.md"), "seed\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "seed"]);

    // Baseline through the approved alias, without --trust: the hook runs.
    let env_alice = envelope(
        &root,
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );
    let baseline = toha(
        &root,
        &[
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--baseline",
            "--answers",
            env_alice.to_str().unwrap(),
        ],
    );
    assert_eq!(baseline.status.code(), Some(0), "baseline: {baseline:?}");
    let document = support::first_document(&baseline.stdout);
    assert_eq!(document["update"], "applied", "{document}");
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
    assert!(
        target.path().join("ran.txt").exists(),
        "the hook must run under the approval"
    );
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // Update through the approved alias, without --trust. An update whose hooks
    // were not trusted returns a needs-trust refusal before it builds anything;
    // reaching `applied` proves the hooks ran under the established approval.
    let env_bob = envelope(
        &root,
        "bob.json",
        &formal,
        serde_json::json!({ "name": "Bob" }),
    );
    let update = toha(
        &root,
        &[
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--from",
            &snapshot,
            "--answers",
            env_bob.to_str().unwrap(),
        ],
    );
    assert_eq!(update.status.code(), Some(0), "update: {update:?}");
    let document = support::first_document(&update.stdout);
    assert_eq!(
        document["update"], "applied",
        "the update must apply under the established approval, not refuse: {document}"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Bob\n"
    );
}

#[test]
fn an_unapproved_template_update_refuses_hooks() {
    let root = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    let folder = template_dir.path().canonicalize().unwrap();
    let formal = support::formal_name(&folder);
    let address = support::folder_address(&folder);

    git(target.path(), &["init", "--quiet"]);
    fs::write(target.path().join("README.md"), "seed\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "seed"]);

    // No install, no approval: a baseline with hooks refuses without --trust.
    let env_alice = envelope(
        &root,
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );
    let output = toha(
        &root,
        &[
            "apply",
            &address,
            target.path().to_str().unwrap(),
            "--baseline",
            "--answers",
            env_alice.to_str().unwrap(),
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(0),
        "unapproved hooks should refuse"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--trust"),
        "expected a trust refusal: {stderr}"
    );
    assert!(!target.path().join("ran.txt").exists());
}
