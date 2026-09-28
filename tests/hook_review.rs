// ---
// relationships:
//   implements: architecture
// ---
//! Binary-level trust behaviors: approval binds to a template's executable
//! surface, so an update or edit that changes the hooks or their scripts
//! withdraws trust until the template is approved again.
#![cfg(unix)]
#[allow(dead_code)]
mod support;

use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

/// Writes a folder template with one top-level `script: hook.sh` hook. The
/// script writes `ran.txt` into the target, so a run is observable. `hooks`
/// is the YAML body under `hooks:`; `script` is the body of `hook.sh`.
fn write_template(folder: &Path, hooks: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template/keep.txt"), "kept\n").unwrap();
    fs::write(
        folder.join("template.yml"),
        format!("name: reviewdemo\nsource: template\nhooks:\n{hooks}"),
    )
    .unwrap();
    let path = folder.join("hook.sh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

const RAN_MARKER: &str = "ran.txt";

/// The default hook body: create the marker so a run is observable.
fn marker_script() -> &'static str {
    "#!/bin/sh\n: > ran.txt\n"
}

fn add_trusted(root: &TempDir, folder: &Path) {
    let output = support::isolated_command(root.path())
        .args([
            "templates",
            "add",
            folder.to_str().unwrap(),
            "--alias",
            "demo",
            "--trust",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "add: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Applies the approved template `demo` into a fresh target with an empty
/// answers document, returning (exit code, stderr, whether the hook ran).
fn apply_demo(root: &TempDir) -> (Option<i32>, String, bool) {
    let target = tempfile::tempdir_in(root.path()).unwrap();
    let answers = target.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let output = support::isolated_command(root.path())
        .args([
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let ran = target.path().join(RAN_MARKER).exists();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        ran,
    )
}

#[test]
fn approval_invalidates_when_the_executable_surface_changes() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    let original_hooks = "  - script: hook.sh\n";
    write_template(&folder, original_hooks, marker_script());
    add_trusted(&root, &folder);

    // No change: the live surface matches the approval, so the hook runs.
    let (code, _stderr, ran) = apply_demo(&root);
    assert_eq!(code, Some(0), "unchanged approved template must run hooks");
    assert!(ran, "the approved hook must run");

    // Documentation-only change: a new source file leaves the surface, and so
    // the approval, intact.
    fs::write(folder.join("template/extra.txt"), "docs\n").unwrap();
    let (code, _stderr, ran) = apply_demo(&root);
    assert_eq!(code, Some(0), "a doc-only change keeps approval");
    assert!(ran, "a doc-only change keeps the hook running");

    // Hook-node change: adding an argument changes the node, so trust lapses.
    write_template(
        &folder,
        "  - script: hook.sh\n    args: [ marker ]\n",
        marker_script(),
    );
    let (code, stderr, ran) = apply_demo(&root);
    assert_eq!(code, Some(3), "a hook-line change needs review: {stderr}");
    assert!(!ran, "an unreviewed hook must not run");
    assert!(
        stderr.contains("hooks changed since approval"),
        "the listing marks the change: {stderr}"
    );

    // Script-byte change: restore the node, change only the script's bytes.
    write_template(
        &folder,
        original_hooks,
        "#!/bin/sh\n: > ran.txt\n# edited\n",
    );
    let (code, stderr, ran) = apply_demo(&root);
    assert_eq!(code, Some(3), "a script-byte change needs review: {stderr}");
    assert!(!ran, "an unreviewed script must not run");
    assert!(
        stderr.contains("hooks changed since approval"),
        "the listing marks the change: {stderr}"
    );
}

#[test]
fn the_gate_is_answer_independent() {
    // The digest reads the declared hook node, not the rendered command, so two
    // approved runs whose answers differ both keep trust and run the hook.
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    // A text question the hook passes as a Jinja argument (not a literal path,
    // so it is outside the hashed surface).
    fs::write(
        folder.join("template.yml"),
        "name: reviewdemo\nsource: template\ninterview:\n  - id: who\n    type: text\n    prompt: who\nhooks:\n  - script: hook.sh\n    args: [ \"{{ who }}\" ]\n",
    )
    .unwrap();
    add_trusted(&root, &folder);

    for answer in ["alpha", "beta"] {
        let target = tempfile::tempdir_in(root.path()).unwrap();
        let answers = target.path().join("answers.json");
        fs::write(&answers, format!("{{\"who\": \"{answer}\"}}")).unwrap();
        let output = support::isolated_command(root.path())
            .args([
                "apply",
                "demo",
                target.path().to_str().unwrap(),
                "--answers",
                answers.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "answer {answer} must stay trusted: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            target.path().join(RAN_MARKER).exists(),
            "the approved hook runs for answer {answer}"
        );
    }
}

#[test]
fn staged_resume_recomputes_trust_at_apply() {
    // A staged interview resumed in a later process recomputes the gate against
    // the installed content, so an edit after staging withdraws trust.
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    add_trusted(&root, &folder);

    // Stage the interview in one process; it completes and saves at the target.
    let target = tempfile::tempdir_in(root.path()).unwrap();
    let batch = target.path().join("batch.json");
    let staged = support::isolated_command(root.path())
        .args([
            "stage",
            "demo",
            target.path().to_str().unwrap(),
            "--async",
            batch.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        staged.status.code(),
        Some(0),
        "stage: {}",
        String::from_utf8_lossy(&staged.stderr)
    );

    // Change the script after staging; the resumed apply re-gates against the
    // installed content and refuses the now-unreviewed hook.
    write_template(
        &folder,
        "  - script: hook.sh\n",
        "#!/bin/sh\n: > ran.txt\n# edited\n",
    );
    let output = support::isolated_command(root.path())
        .args(["apply", target.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "a resumed apply re-gates: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!target.path().join(RAN_MARKER).exists());
}

#[test]
fn legacy_trusted_registry_loads_and_needs_review() {
    // A registry written before approval digests (`trusted: true`, no approval)
    // must load and grant no trust: the whole registry stays readable and a
    // hooked template reads as needs-review (Decision 4, no migration).
    let root = TempDir::new().unwrap();
    let folder = root.path().join("tpl");
    write_template(&folder, "  - script: hook.sh\n", marker_script());
    let canonical = folder.canonicalize().unwrap();
    let user_dir = support::user_data_dir(root.path());
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(
        user_dir.join("templates.yml"),
        format!(
            "templates:\n  \"{formal}\":\n    name: reviewdemo\n    source: \"{src}\"\n    path: \"{src}\"\n    aliases: [ demo ]\n    trusted: true\n",
            formal = canonical.display(),
            src = canonical.display(),
        ),
    )
    .unwrap();

    // The legacy registry loads and lists the template as untrusted.
    let list = support::isolated_command(root.path())
        .args(["templates", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        list.status.code(),
        Some(0),
        "a legacy registry must still load: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&list.stdout)).unwrap();
    // The unfiltered listing also carries the presentation-only bundled demo
    // descriptor (D3); count the persisted registry entries, not that row.
    let entries: Vec<_> = json
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e.get("bundled") != Some(&serde_json::Value::Bool(true)))
        .collect();
    assert_eq!(entries.len(), 1, "the entry loads");
    assert_eq!(
        entries[0]["trusted"], false,
        "legacy trusted grants no trust"
    );

    // Applying the hooked template needs review, not a run.
    let target = tempfile::tempdir_in(root.path()).unwrap();
    let answers = target.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let output = support::isolated_command(root.path())
        .args([
            "apply",
            "demo",
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "legacy trust must not run hooks: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!target.path().join(RAN_MARKER).exists());
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

/// Commits the current worktree of `repo` on branch `main`.
fn commit(repo: &Path, message: &str) {
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", message]);
}

#[test]
fn update_keeps_trust_across_docs_and_drops_it_across_hooks() {
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
    commit(&repo, "initial");
    let url = support::file_url(&repo);

    let add = support::isolated_command(root.path())
        .args(["templates", "add", &url, "--alias", "git", "--trust"])
        .output()
        .unwrap();
    assert_eq!(
        add.status.code(),
        Some(0),
        "add: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    // A documentation-only commit keeps approval across update.
    fs::write(repo.join("template/keep.txt"), "kept and edited\n").unwrap();
    commit(&repo, "docs");
    let update = support::isolated_command(root.path())
        .args(["templates", "update", "git"])
        .output()
        .unwrap();
    let update_out = String::from_utf8_lossy(&update.stdout).into_owned();
    assert_eq!(update.status.code(), Some(0), "update: {update_out}");
    assert!(
        !update_out.contains("approval lapsed"),
        "a doc-only update keeps approval: {update_out}"
    );
    let list = support::isolated_command(root.path())
        .args(["templates", "list", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&list.stdout)).unwrap();
    assert_eq!(json[0]["trusted"], true, "still trusted after a doc update");

    // A hook-changing commit lapses approval; update reports it and the next
    // apply exits 3.
    fs::write(
        repo.join("template.yml"),
        "name: gitdemo\nsource: template\nhooks:\n  - script: hook.sh\n    args: [ marker ]\n",
    )
    .unwrap();
    commit(&repo, "hooks");
    let update = support::isolated_command(root.path())
        .args(["templates", "update", "git"])
        .output()
        .unwrap();
    let update_out = String::from_utf8_lossy(&update.stdout).into_owned();
    assert_eq!(update.status.code(), Some(0), "update: {update_out}");
    assert!(
        update_out.contains("approval lapsed"),
        "a hook-changing update lapses approval: {update_out}"
    );
    let list = support::isolated_command(root.path())
        .args(["templates", "list", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&list.stdout)).unwrap();
    assert_eq!(json[0]["trusted"], false, "not trusted after a hook update");
}
