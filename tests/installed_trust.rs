// ---
// relationships:
//   implements: command-line-interface
// ---
//! Managing trust for an already-installed template: `templates trust` records
//! the current installed executable surface without fetching or moving the
//! commit, and `templates untrust` revokes it, persisting a denial so a lower
//! registry layer's approval cannot reappear. Neither re-adds the template.
#![cfg(unix)]
#[allow(dead_code)]
mod support;

use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

/// Writes a folder template with one top-level `script: hook.sh` hook whose
/// script creates `ran.txt`, so a run is observable. `hooks` is the YAML body
/// under `hooks:`; `script` is the body of `hook.sh`.
fn write_template(folder: &Path, hooks: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template/keep.txt"), "kept\n").unwrap();
    fs::write(
        folder.join("template.yml"),
        format!("name: trustdemo\nsource: template\nhooks:\n{hooks}"),
    )
    .unwrap();
    let path = folder.join("hook.sh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

const RAN_MARKER: &str = "ran.txt";

fn marker_script() -> &'static str {
    "#!/bin/sh\n: > ran.txt\n"
}

fn toha(root: &TempDir, args: &[&str]) -> std::process::Output {
    support::isolated_command(root.path())
        .args(args)
        .output()
        .unwrap()
}

fn code_of(output: &std::process::Output) -> Option<i32> {
    output.status.code()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The user registry as parsed YAML.
fn registry(root: &TempDir) -> serde_json::Value {
    let path = support::user_data_dir(root.path()).join("templates.yml");
    serde_norway::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The one installed entry's object (these tests install exactly one template).
fn sole_entry(root: &TempDir) -> serde_json::Value {
    let reg = registry(root);
    let templates = reg["templates"].as_object().unwrap();
    assert_eq!(templates.len(), 1, "exactly one installed template");
    templates.values().next().unwrap().clone()
}

/// Adds the folder template with the alias `demo`, without approving it.
fn add_untrusted(root: &TempDir, folder: &Path) {
    let output = toha(
        root,
        &[
            "templates",
            "add",
            folder.to_str().unwrap(),
            "--alias",
            "demo",
        ],
    );
    assert_eq!(code_of(&output), Some(0), "add: {}", stderr(&output));
}

/// Applies `demo` into a fresh target with an empty answers document; returns
/// (exit code, whether the hook ran).
fn apply_demo(root: &TempDir) -> (Option<i32>, bool) {
    let target = tempfile::tempdir_in(root.path()).unwrap();
    let answers = target.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let output = toha(
        root,
        &[
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
    );
    (code_of(&output), target.path().join(RAN_MARKER).exists())
}

#[test]
fn re_trust_after_a_surface_change_re_records_the_current_digest() {
    // Idempotency runs through the one trust rule, not a "has an approval"
    // check: after the installed hooks change, a fresh grant re-records the new
    // surface and reports `trusted`, and the changed hooks run again.
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    add_untrusted(&root, &folder);

    let first = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(code_of(&first), Some(0), "trust: {}", stderr(&first));
    let first_digest = sole_entry(&root)["approval"].as_str().unwrap().to_owned();

    // Change the installed hook node so the surface digest differs.
    write_template(
        &folder,
        "  - script: hook.sh\n    args: [ marker ]\n",
        marker_script(),
    );

    // The changed surface is not "already trusted"; the grant re-records it.
    let again = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(code_of(&again), Some(0), "re-trust: {}", stderr(&again));
    assert!(
        stdout(&again).contains("trusted") && !stdout(&again).contains("already"),
        "a changed surface re-records: {}",
        stdout(&again)
    );
    let second_digest = sole_entry(&root)["approval"].as_str().unwrap().to_owned();
    assert_ne!(
        first_digest, second_digest,
        "the current surface digest is recorded"
    );

    let (code, ran) = apply_demo(&root);
    assert_eq!(code, Some(0), "the re-approved hooks run");
    assert!(ran, "the re-approved hook runs");
}

#[test]
fn grant_then_revoke_round_trip_on_an_installed_template() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    add_untrusted(&root, &folder);

    // Untrusted: the installed hook does not run.
    let (code, ran) = apply_demo(&root);
    assert_eq!(
        code,
        Some(3),
        "an untrusted installed template needs review"
    );
    assert!(!ran, "an unreviewed hook must not run");

    // Grant records the installed surface digest and echoes it on stderr; the
    // installed content is unchanged.
    let before = sole_entry(&root);
    let grant = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(code_of(&grant), Some(0), "trust: {}", stderr(&grant));
    assert!(stdout(&grant).contains("trusted"), "{}", stdout(&grant));
    assert!(
        stderr(&grant).contains("approved surface"),
        "the grant echoes the approved surface: {}",
        stderr(&grant)
    );
    let after = sole_entry(&root);
    let digest = after["approval"].as_str().expect("an approval digest");
    assert!(digest.starts_with("sha256:"), "{digest}");
    assert_eq!(after["path"], before["path"], "path is unchanged");
    assert_eq!(after["source"], before["source"], "source is unchanged");
    assert_eq!(
        after.get("commit"),
        before.get("commit"),
        "commit is unchanged"
    );
    assert!(
        after.get("trusted").is_none(),
        "a grant leaves no denial: {after}"
    );

    // The approved hook now runs.
    let (code, ran) = apply_demo(&root);
    assert_eq!(code, Some(0), "an approved template runs its hooks");
    assert!(ran, "the approved hook runs");

    // A second grant with an unchanged surface is idempotent and writes nothing.
    let unchanged = sole_entry(&root);
    let again = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(code_of(&again), Some(0));
    assert!(
        stdout(&again).contains("already trusted"),
        "{}",
        stdout(&again)
    );
    assert_eq!(sole_entry(&root), unchanged, "no write on already trusted");

    // Revoke clears the approval and persists a denial.
    let revoke = toha(&root, &["templates", "untrust", "demo"]);
    assert_eq!(code_of(&revoke), Some(0), "untrust: {}", stderr(&revoke));
    assert!(stdout(&revoke).contains("untrusted"), "{}", stdout(&revoke));
    let revoked = sole_entry(&root);
    assert!(
        revoked.get("approval").is_none(),
        "approval is cleared: {revoked}"
    );
    assert_eq!(
        revoked["trusted"], false,
        "a denial is persisted: {revoked}"
    );

    // The revoked hook no longer runs.
    let (code, ran) = apply_demo(&root);
    assert_eq!(code, Some(3), "a revoked template needs review again");
    assert!(!ran, "the revoked hook must not run");

    // A second revoke is idempotent and writes nothing.
    let tombstoned = sole_entry(&root);
    let again = toha(&root, &["templates", "untrust", "demo"]);
    assert_eq!(code_of(&again), Some(0));
    assert!(
        stdout(&again).contains("already untrusted"),
        "{}",
        stdout(&again)
    );
    assert_eq!(
        sole_entry(&root),
        tombstoned,
        "no write on already untrusted"
    );
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn grant_reads_the_installed_surface_without_fetching_or_moving_the_commit() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    let repo = root.path().join("remote");
    fs::create_dir_all(repo.join("template")).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("template/keep.txt"), "kept\n").unwrap();
    fs::write(
        repo.join("template.yml"),
        "name: gitdemo\nsource: template\nhooks:\n  - script: hook.sh\n",
    )
    .unwrap();
    let script = repo.join("hook.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    let url = support::file_url(&repo);

    // Install without approving.
    let add = toha(&root, &["templates", "add", &url, "--alias", "git"]);
    assert_eq!(code_of(&add), Some(0), "add: {}", stderr(&add));
    let installed_commit = sole_entry(&root)["commit"].as_str().unwrap().to_owned();

    // Advance the remote after install; a grant that fetched would move the
    // commit.
    fs::write(repo.join("template/keep.txt"), "kept and edited\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "advance"]);

    let grant = toha(&root, &["templates", "trust", "git"]);
    assert_eq!(code_of(&grant), Some(0), "trust: {}", stderr(&grant));
    let after = sole_entry(&root);
    assert_eq!(
        after["commit"].as_str().unwrap(),
        installed_commit,
        "trust does not fetch or move the installed commit"
    );
    assert!(
        after["approval"].as_str().unwrap().starts_with("sha256:"),
        "the installed surface is recorded"
    );
}

#[test]
fn untrust_reads_nothing_and_a_missing_surface_cannot_be_trusted() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    let add = toha(
        &root,
        &[
            "templates",
            "add",
            folder.to_str().unwrap(),
            "--alias",
            "demo",
            "--trust",
        ],
    );
    assert_eq!(code_of(&add), Some(0), "add --trust: {}", stderr(&add));

    // The installed script vanishes: the executable surface can no longer be
    // read.
    fs::remove_file(folder.join("hook.sh")).unwrap();

    // untrust reads nothing on disk, so it still succeeds and persists a denial.
    let revoke = toha(&root, &["templates", "untrust", "demo"]);
    assert_eq!(
        code_of(&revoke),
        Some(0),
        "untrust reads nothing: {}",
        stderr(&revoke)
    );
    assert_eq!(sole_entry(&root)["trusted"], false, "a denial is persisted");

    // trust cannot approve a surface it cannot read; it fails and the registry
    // is unchanged.
    let before = sole_entry(&root);
    let grant = toha(&root, &["templates", "trust", "demo"]);
    assert_eq!(
        code_of(&grant),
        Some(1),
        "an unreadable surface refuses trust: {}",
        stderr(&grant)
    );
    assert_eq!(sole_entry(&root), before, "a failed trust writes nothing");
}

#[test]
fn trust_and_untrust_refuse_a_template_outside_the_user_registry() {
    let root = TempDir::new().unwrap();
    let discovered = root.path().join(".templates/discovered");
    fs::create_dir_all(discovered.join("template")).unwrap();
    fs::write(discovered.join("template.yml"), "name: discovered\n").unwrap();
    for verb in ["trust", "untrust"] {
        let output = support::isolated_command(root.path())
            .args(["templates", verb, "discovered"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(code_of(&output), Some(1), "{verb} on a discovered template");
        assert!(
            stderr(&output).contains("not installed in user registry: discovered"),
            "{verb} names the user-registry boundary: {}",
            stderr(&output)
        );
    }
}

#[test]
fn trust_of_an_ambiguous_name_is_refused_with_the_retry() {
    let root = TempDir::new().unwrap();
    for name in ["first", "second"] {
        let folder = root.path().join(name);
        write_template(&folder, "  - script: hook.sh\n", marker_script());
        let add = toha(&root, &["templates", "add", folder.to_str().unwrap()]);
        assert_eq!(code_of(&add), Some(0), "add {name}: {}", stderr(&add));
    }
    // Both templates share the short name `trustdemo`.
    let output = toha(&root, &["templates", "trust", "trustdemo"]);
    assert_eq!(
        code_of(&output),
        Some(5),
        "an ambiguous name: {}",
        stderr(&output)
    );
    let names = support::suggested(&stderr(&output), "to use ");
    assert_eq!(names[0], "templates", "the retry names templates trust");
    assert_eq!(names[1], "trust");
}
