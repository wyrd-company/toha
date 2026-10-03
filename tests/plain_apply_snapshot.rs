// ---
// relationships:
//   implements: architecture
// ---
//! An ordinary `apply` into a git target records a snapshot when the target is
//! clean, has a commit, and the apply changes a path; otherwise it applies as
//! before and names why it saved none.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

use serde_json::Value;

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

fn write_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
}

fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

fn envelope(dir: &Path, name: &str, formal: &str, answers: Value) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": answers });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    path
}

/// Run a scripted `apply ADDRESS TARGET --answers ENV` and return its document.
fn apply(iso: &Path, address: &str, target: &Path, env: &Path) -> Value {
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--answers")
        .arg(env);
    let output = command.output().unwrap();
    assert!(output.status.success(), "apply failed: {output:?}");
    support::first_document(&output.stdout)
}

#[test]
fn a_clean_apply_saves_a_snapshot() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let document = apply(iso.path(), &address, target.path(), &env);
    assert!(
        document["snapshot"]["id"].is_string(),
        "expected a saved snapshot: {document}"
    );

    // The snapshot is listed against the target.
    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let listed = support::first_document(&list.output().unwrap().stdout);
    assert_eq!(listed["snapshots"].as_array().unwrap().len(), 1);
}

#[test]
fn an_apply_outside_git_skips_with_not_git() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    // No git init: the target is a plain directory.

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let document = apply(iso.path(), &address, target.path(), &env);
    assert_eq!(document["snapshot"]["skipped"], "not-git", "{document}");
    // The apply still wrote the file.
    assert!(target.path().join("greeting.txt").exists());
}

#[test]
fn a_dirty_apply_skips_with_dirty() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());
    // An uncommitted change before the apply makes the target dirty.
    std::fs::write(target.path().join("dirty.txt"), "uncommitted\n").unwrap();

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let document = apply(iso.path(), &address, target.path(), &env);
    assert_eq!(document["snapshot"]["skipped"], "dirty", "{document}");
}

#[test]
fn the_agent_apply_path_route_saves_a_snapshot() {
    // stage -> continue (complete) -> apply PATH: the agent resume route applies
    // the completed staged interview and saves a snapshot, reported as a line.
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());

    // Stage the interview (the required question is left pending).
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--async")
        .env("TOHA_NOW", "2020-06-15T12:00:00+00:00[UTC]");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "stage: {staged:?}");

    // Answer it through the agent continue route, completing the interview.
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env);
    let continued = cont.output().unwrap();
    assert!(continued.status.success(), "continue: {continued:?}");

    // Apply the completed staged interview: it writes and saves a snapshot.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "apply: {applied:?}");
    let stdout = String::from_utf8_lossy(&applied.stdout);
    assert!(
        stdout.contains("saved snapshot"),
        "expected a saved-snapshot line: {stdout}"
    );

    // The snapshot is listed against the target.
    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let listed = support::first_document(&list.output().unwrap().stdout);
    assert_eq!(listed["snapshots"].as_array().unwrap().len(), 1, "{listed}");
}

#[cfg(unix)]
#[test]
fn an_exec_bit_only_change_still_saves() {
    // A re-apply that changes only a captured path's executable bit — its content
    // byte-identical to HEAD — is a change: the save test compares entry kind,
    // not only content, so this is not mistaken for nothing changed.
    use std::os::unix::fs::PermissionsExt;

    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // First apply saves greeting.txt (non-executable); commit it clean.
    let first = apply(iso.path(), &address, target.path(), &env);
    assert!(first["snapshot"]["id"].is_string(), "{first}");
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "applied"]);

    // The source file gains the executable bit; its content is unchanged.
    let source = template_dir.path().join("template/greeting.txt");
    let mut mode = std::fs::metadata(&source).unwrap().permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(&source, mode).unwrap();

    // The forced re-apply writes identical bytes but a new mode: a change.
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--answers")
        .arg(&env)
        .arg("--force");
    let output = command.output().unwrap();
    assert!(output.status.success(), "re-apply failed: {output:?}");
    let second = support::first_document(&output.stdout);
    assert!(
        second["snapshot"]["id"].is_string(),
        "an exec-bit change must save: {second}"
    );
}

#[test]
fn a_re_apply_that_changes_nothing_skips() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // First apply saves; commit it clean.
    let first = apply(iso.path(), &address, target.path(), &env);
    assert!(first["snapshot"]["id"].is_string(), "{first}");
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "applied"]);

    // The same apply (forced, since the file exists) writes identical bytes:
    // nothing changed, so no snapshot.
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--answers")
        .arg(&env)
        .arg("--force");
    let output = command.output().unwrap();
    assert!(output.status.success(), "re-apply failed: {output:?}");
    let second = support::first_document(&output.stdout);
    assert_eq!(second["snapshot"]["skipped"], "nothing-changed", "{second}");
}

#[test]
fn a_genuine_open_fault_warns_and_still_applies() {
    // A bare repository has no working tree, so opening it to save a snapshot
    // fails with a genuine fault (ProjectError::Bare) — distinct from "not a git
    // repo". The apply still applies and names the fault on standard error,
    // instead of silently degrading to "not-git".
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    git(target.path(), &["init", "--quiet", "--bare"]);

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--answers")
        .arg(&env);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "the apply still applies: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("working tree"),
        "the genuine open fault is named on stderr: {stderr}"
    );
    assert!(
        target.path().join("greeting.txt").exists(),
        "the file was written"
    );
}
