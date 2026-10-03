// ---
// relationships:
//   implements: architecture
// ---
//! The data-driven update fixture harness. Each `tests/fixtures/update-*` holds
//! `template-a/` and `template-b/` (two versions at one source path),
//! `answers.json` (the bare answers), an `edits/` overlay with a `removal.txt`
//! list, `expected/` (the clean worktree bytes after the update), and
//! `expect.yml` (the status, the conflicted paths, and their index stages). The
//! runner applies A, overlays the operator's edits, applies B with `--from`, and
//! checks the merge result, the worktree, and `git ls-files -s` stages.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

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

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// Copy the template source at `from` into the working template `dir`, replacing
/// any previous version, so both versions share one source path.
fn install_version(from: &Path, dir: &Path) {
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();
    support::copy_tree(from, dir);
}

#[test]
fn the_update_conflict_fixture_merges_as_expect_yml_states() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update-conflict");
    let expect: serde_json::Value =
        serde_norway::from_str(&std::fs::read_to_string(fixture.join("expect.yml")).unwrap())
            .unwrap();
    let bare_answers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.join("answers.json")).unwrap())
            .unwrap();

    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("template");
    install_version(&fixture.join("template-a"), &template);
    let formal = support::formal_name(&template);
    let address = support::folder_address(&template.canonicalize().unwrap());

    // A git target with the seed file the operator later removes.
    let target = work.path().join("target");
    std::fs::create_dir(&target).unwrap();
    git(&target, &["init", "--quiet"]);
    git(&target, &["config", "core.autocrlf", "false"]);
    std::fs::write(target.join("seed-to-remove.txt"), "seed\n").unwrap();
    git(&target, &["add", "."]);
    git(&target, &["commit", "--quiet", "-m", "seed"]);

    // Apply version A and record a snapshot.
    let envelope = iso.path().join("answers.json");
    std::fs::write(
        &envelope,
        serde_json::to_vec(&serde_json::json!({ "template": formal, "answers": bare_answers }))
            .unwrap(),
    )
    .unwrap();
    let mut apply = support::isolated_command(iso.path());
    apply
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--answers")
        .arg(&envelope);
    let applied = support::first_document(&apply.output().unwrap().stdout);
    let snapshot = applied["snapshot"]["id"].as_str().unwrap().to_owned();
    git(&target, &["add", "."]);
    git(&target, &["commit", "--quiet", "-m", "apply A"]);

    // Overlay the operator's edits and removals, then commit.
    let edits = fixture.join("edits");
    for entry in std::fs::read_dir(&edits).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "removal.txt" {
            continue;
        }
        std::fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
    }
    for line in std::fs::read_to_string(edits.join("removal.txt"))
        .unwrap()
        .lines()
    {
        let line = line.trim();
        if !line.is_empty() {
            std::fs::remove_file(target.join(line)).unwrap();
        }
    }
    git(&target, &["add", "-A"]);
    git(&target, &["commit", "--quiet", "-m", "operator edits"]);

    // Version B at the same source, then update from the snapshot.
    install_version(&fixture.join("template-b"), &template);
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(&address)
        .arg(&target)
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&envelope);
    let output = update.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "update: {output:?}");
    let document = support::first_document(&output.stdout);

    // Status and conflicted paths match expect.yml.
    assert_eq!(document["status"], expect["status"], "{document}");
    let conflicted: Vec<&str> = document["merge"]["conflicted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let expected_conflicted: Vec<&str> = expect["conflicted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(conflicted, expected_conflicted, "conflicted paths");

    // The unmerged index carries the expected stages for each conflicted path.
    for (path, stages) in expect["unmerged-stages"].as_object().unwrap() {
        let listing = git_stdout(&target, &["ls-files", "-s", path]);
        let seen: Vec<i64> = listing
            .lines()
            .map(|line| line.split_whitespace().nth(2).unwrap().parse().unwrap())
            .collect();
        let want: Vec<i64> = stages
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(seen, want, "index stages for {path}: {listing}");
    }

    // Stage 2 of the conflicted path holds the operator's bytes.
    if let Some(needle) = expect["stage-2-contains"].as_str() {
        let path = expected_conflicted[0];
        let stage2 = git_stdout(&target, &["cat-file", "-p", &format!(":2:{path}")]);
        assert!(stage2.contains(needle), "stage 2 of {path}: {stage2}");
    }

    // The conflicted worktree file carries diff3 markers.
    if expect["markers"].as_bool() == Some(true) {
        let readme = std::fs::read_to_string(target.join(expected_conflicted[0])).unwrap();
        assert!(
            readme.contains("<<<<<<<") && readme.contains("|||||||") && readme.contains(">>>>>>>"),
            "diff3 markers: {readme}"
        );
    }

    // Every clean file in expected/ matches the worktree byte for byte.
    let expected_dir = fixture.join("expected");
    check_expected(&expected_dir, &expected_dir, &target);
}

/// Assert every file under `dir` (rooted at `base`) equals the target's byte for
/// byte.
fn check_expected(base: &Path, dir: &Path, target: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            check_expected(base, &path, target);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            let want = std::fs::read_to_string(&path).unwrap();
            let got = std::fs::read_to_string(target.join(rel))
                .unwrap_or_else(|_| panic!("missing {rel:?} in target"));
            assert_eq!(got, want, "worktree file {rel:?}");
        }
    }
}
