// ---
// relationships:
//   implements: architecture
// ---
//! The acceptance scenario (behavior 1): apply version A in one clone, commit,
//! edit a line the new version also changes; in a second clone that fetched the
//! snapshot, `snapshots list` marks the base and `apply --from` updates a file,
//! adds a file, and leaves the edited file conflicted with diff3 markers and
//! index stages a plain commit refuses; after resolving, a re-apply reports
//! already-current. Two real clones fetch the snapshot over a bare remote.

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Write version A of the template into `dir`: a README with a line the operator
/// and version B both change, a file version B updates, and one question.
fn version_a(dir: &Path) {
    std::fs::create_dir_all(dir.join("template/src")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: app\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("template/README.md"),
        "{{ name }}\nline-2\nline-3\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/src/app.txt"), "app-a\n").unwrap();
}

/// Change the template into version B in place (the same source path): the README
/// line the operator also edited changes, the app file changes, a file is added.
fn version_b(dir: &Path) {
    std::fs::write(
        dir.join("template/README.md"),
        "{{ name }}\nline-2\nline-3-template\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/src/app.txt"), "app-b\n").unwrap();
    std::fs::write(dir.join("template/src/health.txt"), "ok\n").unwrap();
}

#[test]
fn the_two_clone_acceptance_scenario() {
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("template");
    version_a(&template);
    let formal = support::formal_name(&template);
    let address = support::folder_address(&template.canonicalize().unwrap());

    // A bare origin and the first clone.
    let origin = work.path().join("origin.git");
    git_ok(
        work.path(),
        &["init", "--bare", "--quiet", origin.to_str().unwrap()],
    );
    // The clone checks out the origin's default branch, so point it at `main`.
    git_ok(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let clone1 = work.path().join("clone1");
    std::fs::create_dir(&clone1).unwrap();
    git_ok(&clone1, &["init", "--quiet"]);
    git_ok(
        &clone1,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    std::fs::write(clone1.join("seed"), "seed\n").unwrap();
    git_ok(&clone1, &["add", "."]);
    git_ok(&clone1, &["commit", "--quiet", "-m", "seed"]);
    git_ok(&clone1, &["branch", "-M", "main"]);

    // Apply version A and record a snapshot.
    let answers = iso.path().join("a.json");
    std::fs::write(
        &answers,
        serde_json::to_vec(
            &serde_json::json!({ "template": formal, "answers": { "name": "sample-value" } }),
        )
        .unwrap(),
    )
    .unwrap();
    let mut apply = support::isolated_command(iso.path());
    apply
        .arg("apply")
        .arg(&address)
        .arg(&clone1)
        .arg("--answers")
        .arg(&answers);
    let document = support::first_document(&apply.output().unwrap().stdout);
    assert!(
        document["snapshot"]["id"].is_string(),
        "version A recorded a snapshot: {document}"
    );

    // Commit the generated files, then edit the README line version B also changes.
    git_ok(&clone1, &["add", "."]);
    git_ok(&clone1, &["commit", "--quiet", "-m", "apply A"]);
    std::fs::write(
        clone1.join("README.md"),
        "sample-value\nline-2\nline-3-operator\n",
    )
    .unwrap();
    git_ok(&clone1, &["add", "."]);
    git_ok(&clone1, &["commit", "--quiet", "-m", "operator edit"]);

    // Publish the branch and the snapshot ref to the origin.
    git_ok(&clone1, &["push", "--quiet", "-u", "origin", "main"]);
    git_ok(
        &clone1,
        &[
            "push",
            "--quiet",
            "origin",
            "refs/toha/snapshots/*:refs/toha/snapshots/*",
        ],
    );

    // The second clone fetches the snapshot after `toha init`.
    let clone2 = work.path().join("clone2");
    git_ok(
        work.path(),
        &[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            clone2.to_str().unwrap(),
        ],
    );
    let mut init = support::isolated_command(iso.path());
    init.arg("init").arg(&clone2);
    assert!(init.output().unwrap().status.success(), "toha init");
    git_ok(&clone2, &["fetch", "--quiet", "origin"]);

    // `snapshots list` marks the fetched snapshot as a likely base.
    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots").arg("list").arg(&clone2).arg("--json");
    let listed = support::first_document(&list.output().unwrap().stdout);
    let entries = listed["snapshots"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "the snapshot fetched: {listed}");
    let snapshot = entries[0]["id"].as_str().unwrap().to_owned();
    assert!(
        entries[0]["mark"].is_string(),
        "marked a likely base: {listed}"
    );

    // Version B, then update the second clone from the snapshot.
    version_b(&template);
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(&address)
        .arg(&clone2)
        .arg("--from")
        .arg(&snapshot);
    let output = update.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "update: {output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Conflicted README.md (content)"), "{text}");
    assert!(
        text.contains("Resolve conflicts in these files before committing."),
        "{text}"
    );
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");

    // The template's clean change reached src/app.txt and the new file was added.
    assert_eq!(
        std::fs::read_to_string(clone2.join("src/app.txt")).unwrap(),
        "app-b\n"
    );
    assert_eq!(
        std::fs::read_to_string(clone2.join("src/health.txt")).unwrap(),
        "ok\n"
    );

    // The README carries diff3 markers, holds the operator's bytes at stage 2, and
    // a plain commit refuses it.
    let readme = std::fs::read_to_string(clone2.join("README.md")).unwrap();
    assert!(
        readme.contains("<<<<<<<") && readme.contains("|||||||") && readme.contains(">>>>>>>"),
        "diff3 markers: {readme}"
    );
    assert!(
        readme.contains("line-3-operator"),
        "operator bytes present: {readme}"
    );
    let status = git(&clone2, &["status", "--porcelain=v1"]);
    assert!(
        String::from_utf8_lossy(&status.stdout).contains("UU README.md"),
        "unmerged README: {}",
        String::from_utf8_lossy(&status.stdout)
    );
    let commit = git(&clone2, &["commit", "-m", "should fail"]);
    assert!(
        !commit.status.success(),
        "a plain commit must refuse the conflict"
    );

    // Resolve, commit; the committed tree equals the working tree.
    std::fs::write(
        clone2.join("README.md"),
        "sample-value\nline-2\nline-3-resolved\n",
    )
    .unwrap();
    git_ok(&clone2, &["add", "."]);
    git_ok(&clone2, &["commit", "--quiet", "-m", "resolve"]);

    // The update saved a new snapshot of the render; applying from it reports
    // already-current and changes no byte.
    let mut list2 = support::isolated_command(iso.path());
    list2
        .arg("snapshots")
        .arg("list")
        .arg(&clone2)
        .arg("--json");
    let listed2 = support::first_document(&list2.output().unwrap().stdout);
    let newest = listed2["snapshots"][0]["id"].as_str().unwrap().to_owned();
    let readme_before = std::fs::read_to_string(clone2.join("README.md")).unwrap();
    let mut current = support::isolated_command(iso.path());
    current
        .arg("apply")
        .arg(&address)
        .arg(&clone2)
        .arg("--from")
        .arg(&newest);
    let output = current.output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Already current."), "{text}");
    assert!(
        text.contains("No snapshot saved: already current."),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(clone2.join("README.md")).unwrap(),
        readme_before,
        "already-current changes no byte"
    );
}
