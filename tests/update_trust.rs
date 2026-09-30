// ---
// relationships:
//   implements: architecture
// ---
//! An update runs its template's hooks under an established registry approval,
//! without a fresh `--trust`, and an unapproved one previews the plan. This exercises
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
    let env_first = envelope(
        &root,
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let baseline = toha(
        &root,
        &[
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--baseline",
            "--answers",
            env_first.to_str().unwrap(),
        ],
    );
    assert_eq!(baseline.status.code(), Some(0), "baseline: {baseline:?}");
    let document = support::first_document(&baseline.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    assert!(
        target.path().join("ran.txt").exists(),
        "the hook must run under the approval"
    );
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // Update through the approved alias, without --trust. An update whose hooks
    // were not trusted returns a needs-trust refusal before it builds anything;
    // reaching `applied` proves the hooks ran under the established approval.
    let env_revised = envelope(
        &root,
        "revised.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
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
            env_revised.to_str().unwrap(),
        ],
    );
    assert_eq!(update.status.code(), Some(0), "update: {update:?}");
    let document = support::first_document(&update.stdout);
    assert_eq!(
        document["status"], "applied",
        "the update must apply under the established approval, not refuse: {document}"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello revised-value\n"
    );
}

#[test]
fn an_unapproved_template_update_plans_without_running_hooks() {
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
    let env_first = envelope(
        &root,
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let output = toha(
        &root,
        &[
            "apply",
            &address,
            target.path().to_str().unwrap(),
            "--baseline",
            "--answers",
            env_first.to_str().unwrap(),
        ],
    );
    // An unapproved hooked update previews the plan and runs nothing: `planned`
    // with `trusted: false`, and the hook marker is never created.
    assert_eq!(
        output.status.code(),
        Some(0),
        "planned is exit 0: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    assert_eq!(document["trusted"], false, "{document}");
    assert!(!target.path().join("ran.txt").exists());
}
