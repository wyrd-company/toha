use super::*;
use crate::snapshot::Project;
use crate::staging::canonical_target;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn hash_object(repo: &Path, bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_owned()
}

/// Build a snapshot ref whose files/ holds `files` (target-relative path, bytes),
/// with the given id and target. Origins are all toha for merge tests.
fn make_snapshot(dir: &Path, id: &str, target: &str, files: &[(&str, &[u8])]) {
    let index = dir.join(format!(".idx-{id}"));
    let _ = std::fs::remove_file(&index);
    let git_idx = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_INDEX_FILE", &index)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "U")
            .env("GIT_AUTHOR_EMAIL", "u@x.invalid")
            .env("GIT_COMMITTER_NAME", "U")
            .env("GIT_COMMITTER_EMAIL", "u@x.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let paths_json: String = files
        .iter()
        .map(|(p, _)| format!("    \"{p}\": {{ \"origin\": \"toha\" }}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let json = format!(
        "{{\n  \"snapshot\": 1,\n  \"id\": \"{id}\",\n  \"template\": \"forge:catalog/receipt@stable\",\n  \"source\": \"forge:catalog/receipt\",\n  \"commit\": \"8be0d41c2f0000000000000000000000000000a1\",\n  \"target\": \"{target}\",\n  \"created\": \"2026-09-30T04:12:00Z\",\n  \"generated\": \"2026-03-14T09:26:53+00:00[UTC]\",\n  \"project\": {{ \"commit\": \"a41c0de00000000000000000000000000000beef\", \"branch\": \"main\" }},\n  \"built_from\": null,\n  \"submissions\": [],\n  \"paths\": {{\n{paths_json}\n  }}\n}}"
    );
    let json_oid = hash_object(dir, json.as_bytes());
    git_idx(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{json_oid},snapshot.json"),
    ]);
    for (p, bytes) in files {
        let oid = hash_object(dir, bytes);
        git_idx(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{oid},files/{p}"),
        ]);
    }
    let tree = git_idx(&["write-tree"]);
    let commit = git(dir, &["commit-tree", &tree, "-m", "snapshot"]);
    git(
        dir,
        &["update-ref", &format!("refs/toha/snapshots/{id}"), &commit],
    );
    let _ = std::fs::remove_file(&index);
}

fn open(dir: &Path, sub: &str) -> Project {
    let target = canonical_target(&dir.join(sub)).unwrap();
    Project::open(&target).unwrap().unwrap()
}

const BASE_ID: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6E";
const NEW_ID: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6F";

#[test]
fn a_clean_three_way_merge_keeps_operator_and_template_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    // Operator's committed state: line 1 edited by operator.
    write(&root.join("app/both.txt"), b"ONE\ntwo\nthree\nfour\nfive\n");
    write(&root.join("app/removed.txt"), b"bye\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    // Base = old template output; new = new template output (line 5 changed, removed.txt dropped, added.txt new).
    make_snapshot(
        root,
        BASE_ID,
        "app",
        &[
            ("both.txt", b"one\ntwo\nthree\nfour\nfive\n"),
            ("removed.txt", b"bye\n"),
        ],
    );
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[
            ("both.txt", b"one\ntwo\nthree\nfour\nFIVE\n"),
            ("added.txt", b"new\n"),
        ],
    );

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap();
    assert!(
        matches!(result, Merged::Written { .. }),
        "clean merge writes"
    );

    // Operator's line 1 and template's line 5 both survive; no conflict markers.
    let both = std::fs::read_to_string(root.join("app/both.txt")).unwrap();
    assert_eq!(
        both, "ONE\ntwo\nthree\nfour\nFIVE\n",
        "both edits merged: {both:?}"
    );
    // Template add present; operator's removed.txt deleted by the template.
    assert_eq!(
        std::fs::read_to_string(root.join("app/added.txt")).unwrap(),
        "new\n"
    );
    assert!(
        !root.join("app/removed.txt").exists(),
        "template removed an unedited file"
    );
    // git sees a normal (unconflicted) staged state.
    let status = git(root, &["status", "--porcelain"]);
    assert!(!status.contains("UU"), "no conflicts: {status}");
}

#[test]
fn a_template_removed_file_edited_by_the_operator_is_a_modify_delete_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/gone.txt"), b"bye\nuser edit\n"); // operator edited it
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    make_snapshot(root, BASE_ID, "app", &[("gone.txt", b"bye\n")]);
    make_snapshot(root, NEW_ID, "app", &[("keep.txt", b"k\n")]); // template dropped gone.txt

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap();

    // The operator's bytes stay on disk; git reports a delete/modify conflict.
    assert_eq!(
        std::fs::read_to_string(root.join("app/gone.txt")).unwrap(),
        "bye\nuser edit\n"
    );
    let status = git(root, &["status", "--porcelain"]);
    assert!(
        status.contains("gone.txt"),
        "gone.txt is unmerged: {status}"
    );
    let unmerged = git(root, &["ls-files", "-u"]);
    assert!(
        unmerged.contains("gone.txt"),
        "gone.txt has conflict stages: {unmerged}"
    );
}

#[test]
fn an_occupied_path_refuses_and_cleans_up_the_candidate_ref() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    // An untracked file occupies the path the template adds.
    write(&root.join("app/added.txt"), b"operator's own\n");

    make_snapshot(root, BASE_ID, "app", &[("keep.txt", b"k\n")]);
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[("keep.txt", b"k\n"), ("added.txt", b"template\n")],
    );

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    assert!(
        matches!(result, Err(MergeError::Occupied(_))),
        "refuses the occupied path"
    );

    // The operator's file is untouched, and the candidate ref was removed (31).
    assert_eq!(
        std::fs::read_to_string(root.join("app/added.txt")).unwrap(),
        "operator's own\n"
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(refs.contains(BASE_ID), "base ref preserved");
    assert!(
        !refs.contains(NEW_ID),
        "candidate ref removed on unsuccessful exit: {refs}"
    );
}

fn read_snapshot(project: &Project, id: &str) -> Snapshot {
    let sid: SnapshotId = id.parse().unwrap();
    project.find(&sid.to_string()).unwrap()
}

#[test]
fn a_target_changed_since_the_start_refuses_with_nothing_written() {
    // Behavior 23: a drifted target fails the locked re-check; nothing is written
    // and the candidate ref is removed.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/both.txt"), b"base\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    make_snapshot(root, BASE_ID, "app", &[("both.txt", b"base\n")]);
    make_snapshot(root, NEW_ID, "app", &[("both.txt", b"new-template\n")]);

    // The operator dirties the target after HEAD was recorded.
    write(&root.join("app/both.txt"), b"operator is editing\n");

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    assert!(
        matches!(result, Err(MergeError::Changed)),
        "refuses a changed target: {result:?}"
    );

    // Nothing was written and the candidate ref is gone.
    assert_eq!(
        std::fs::read_to_string(root.join("app/both.txt")).unwrap(),
        "operator is editing\n"
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(!refs.contains(NEW_ID), "candidate ref removed on refusal");
}

#[test]
fn a_write_failure_rolls_back_and_leaves_the_index_unwritten() {
    // Behavior 24: a failure part-way through writing rolls back the files
    // already written, removes created files, and never writes the index.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    // The template adds two files; the second cannot be written because the
    // operator has an untracked *file* where its parent directory must go.
    make_snapshot(root, BASE_ID, "app", &[("keep.txt", b"k\n")]);
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[
            ("keep.txt", b"k\n"),
            ("a.txt", b"added first\n"),
            ("sub/b.txt", b"needs a directory\n"),
        ],
    );
    // `app/sub` is a file, so creating `app/sub/` for `sub/b.txt` fails.
    write(&root.join("app/sub"), b"operator's file, not a directory\n");

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    assert!(result.is_err(), "the write fails: {result:?}");

    // a.txt was written first, then rolled back (removed as a created file).
    assert!(
        !root.join("app/a.txt").exists(),
        "the first add was rolled back"
    );
    // The operator's file at app/sub is untouched.
    assert_eq!(
        std::fs::read_to_string(root.join("app/sub")).unwrap(),
        "operator's file, not a directory\n"
    );
    // The index was never written: keep.txt is the only tracked file, unstaged of a.txt.
    let staged = git(root, &["diff", "--cached", "--name-only"]);
    assert!(
        !staged.contains("a.txt"),
        "index unwritten, a.txt not staged: {staged}"
    );
    // The candidate ref is removed.
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(
        !refs.contains(NEW_ID),
        "candidate ref removed after rollback"
    );
}

fn action_of(result: &Merged, path: &str) -> Option<Action> {
    if let Merged::Written { changes, .. } = result {
        changes
            .iter()
            .find(|c| c.path.as_str() == path)
            .map(|c| c.action)
    } else {
        None
    }
}

fn run_merge(root: &Path) -> Merged {
    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap()
}

#[test]
fn an_add_add_conflict_keeps_operator_bytes_and_is_classified() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    write(&root.join("app/conf.txt"), b"operator\n"); // operator added conf.txt
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    make_snapshot(root, BASE_ID, "app", &[("keep.txt", b"k\n")]); // base has no conf.txt
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[("keep.txt", b"k\n"), ("conf.txt", b"template\n")],
    );

    let result = run_merge(root);
    assert_eq!(
        action_of(&result, "app/conf.txt"),
        Some(Action::Conflicted(ConflictKind::AddAdd))
    );
    assert_eq!(
        std::fs::read_to_string(root.join("app/conf.txt")).unwrap(),
        "operator\n",
        "operator bytes kept"
    );
    assert!(
        git(root, &["ls-files", "-u"]).contains("conf.txt"),
        "conflict stages present"
    );
}

#[test]
fn a_modify_delete_conflict_is_classified() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/gone.txt"), b"bye\nuser edit\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(root, BASE_ID, "app", &[("gone.txt", b"bye\n")]);
    make_snapshot(root, NEW_ID, "app", &[("keep.txt", b"k\n")]);

    let result = run_merge(root);
    assert_eq!(
        action_of(&result, "app/gone.txt"),
        Some(Action::Conflicted(ConflictKind::ModifyDelete))
    );
    assert_eq!(
        std::fs::read_to_string(root.join("app/gone.txt")).unwrap(),
        "bye\nuser edit\n"
    );
}

#[test]
fn a_binary_changed_on_both_sides_is_classified_and_keeps_operator_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/logo.bin"), b"\x89BIN\x00user\x00");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(root, BASE_ID, "app", &[("logo.bin", b"\x89BIN\x00v1\x00")]);
    make_snapshot(root, NEW_ID, "app", &[("logo.bin", b"\x89BIN\x00v2\x00")]);

    let result = run_merge(root);
    assert_eq!(
        action_of(&result, "app/logo.bin"),
        Some(Action::Conflicted(ConflictKind::Binary))
    );
    assert_eq!(
        std::fs::read(root.join("app/logo.bin")).unwrap(),
        b"\x89BIN\x00user\x00",
        "operator bytes kept"
    );
    let unmerged = git(root, &["ls-files", "-u"]);
    assert!(
        unmerged.contains(" 1\tapp/logo.bin") && unmerged.contains(" 3\tapp/logo.bin"),
        "stages 1/2/3: {unmerged}"
    );
}

#[test]
fn a_file_replaced_by_a_directory_is_a_file_directory_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main", "-q"]);
    write(&root.join("app/item"), b"operator file\n"); // operator keeps it a file
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(root, BASE_ID, "app", &[("item", b"base file\n")]);
    // The template turns `item` into a directory.
    make_snapshot(root, NEW_ID, "app", &[("item/sub.txt", b"now a dir\n")]);

    let result = run_merge(root);
    // `app/item` (the operator's file) is a file/directory conflict.
    assert_eq!(
        action_of(&result, "app/item"),
        Some(Action::Conflicted(ConflictKind::FileDirectory))
    );
    assert_eq!(
        std::fs::read_to_string(root.join("app/item")).unwrap(),
        "operator file\n",
        "operator file kept"
    );
}

#[test]
fn a_driver_attributed_path_is_a_driver_conflict_keeping_operator_bytes() {
    // Behavior 29 (merge side): a path whose attributes name a merge driver is
    // never merged; the operator's bytes stay and it becomes a driver conflict
    // with stages 1/2/3 — no driver program runs (drivers are stripped on open).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "merge.secret.driver", "false %O %A %B"]);
    write(&root.join(".gitattributes"), b"*.cf merge=secret\n");
    write(&root.join("app/conf.cf"), b"operator\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    make_snapshot(root, BASE_ID, "app", &[("conf.cf", b"base\n")]);
    make_snapshot(root, NEW_ID, "app", &[("conf.cf", b"template\n")]);

    let result = run_merge(root);
    assert_eq!(
        action_of(&result, "app/conf.cf"),
        Some(Action::Conflicted(ConflictKind::Driver)),
        "attributed path is a driver conflict"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("app/conf.cf")).unwrap(),
        "operator\n",
        "operator bytes kept"
    );
    let unmerged = git(root, &["ls-files", "-u"]);
    assert!(
        unmerged.contains(" 1\tapp/conf.cf")
            && unmerged.contains(" 2\tapp/conf.cf")
            && unmerged.contains(" 3\tapp/conf.cf"),
        "stages 1/2/3 present: {unmerged}"
    );
}

/// Set up an occupied-path failure scenario with a base and new snapshot plus a
/// foreign third snapshot, returning the project ready to merge.
fn occupied_scenario(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    write(&root.join("app/added.txt"), b"operator's own\n"); // occupies the add path
    make_snapshot(root, BASE_ID, "app", &[("keep.txt", b"k\n")]);
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[("keep.txt", b"k\n"), ("added.txt", b"template\n")],
    );
}

const FOREIGN_ID: &str = "01JA2B8M4R0C7W1Y5F3H9K2S71";

#[test]
fn behavior_31_ref_map_unchanged_except_candidate_on_unsuccessful_exit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    occupied_scenario(root);
    // A foreign snapshot of another source must survive the failed merge.
    make_snapshot(root, FOREIGN_ID, "app", &[("keep.txt", b"k\n")]);

    let before = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    assert!(matches!(result, Err(MergeError::Occupied(_))));

    let after = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    // The candidate ref is gone; base and the foreign ref remain unchanged.
    assert!(
        before.contains(NEW_ID) && !after.contains(NEW_ID),
        "candidate removed"
    );
    assert!(after.contains(BASE_ID), "base preserved");
    assert!(after.contains(FOREIGN_ID), "foreign snapshot preserved");
    // Only the candidate line differs.
    let before_no_cand: Vec<&str> = before.lines().filter(|l| !l.contains(NEW_ID)).collect();
    assert_eq!(
        before_no_cand,
        after.lines().collect::<Vec<_>>(),
        "ref map unchanged except the candidate"
    );
}

#[test]
fn behavior_31_cleanup_preserves_a_ref_replaced_since_publication() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(root, NEW_ID, "app", &[("keep.txt", b"k\n")]);

    let project = open(root, "app");
    let id: SnapshotId = NEW_ID.parse().unwrap();
    let current = super::published_commit(&project, &id).unwrap();
    let unrelated = gix::ObjectId::from_hex(git(root, &["rev-parse", "HEAD"]).as_bytes()).unwrap();
    assert_ne!(current, unrelated, "the commits differ");

    // Cleanup with a stale "published" commit (a concurrent replacement) preserves the ref.
    super::remove_candidate(&project, &id, unrelated).unwrap();
    assert!(
        git(root, &["for-each-ref", "refs/toha/snapshots/"]).contains(NEW_ID),
        "a ref replaced since publication is preserved"
    );

    // Cleanup with the real published commit removes it.
    super::remove_candidate(&project, &id, current).unwrap();
    assert!(
        !git(root, &["for-each-ref", "refs/toha/snapshots/"]).contains(NEW_ID),
        "the exact candidate is removed"
    );
}

#[test]
fn behavior_31_a_held_index_lock_refuses_and_removes_the_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/both.txt"), b"base\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(root, BASE_ID, "app", &[("both.txt", b"base\n")]);
    make_snapshot(root, NEW_ID, "app", &[("both.txt", b"template\n")]);

    // Another program holds the index lock.
    std::fs::write(root.join(".git/index.lock"), b"").unwrap();

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    assert!(result.is_err(), "a held index lock refuses: {result:?}");
    // Nothing written, candidate removed, base preserved, foreign lock untouched.
    assert_eq!(
        std::fs::read_to_string(root.join("app/both.txt")).unwrap(),
        "base\n"
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(
        !refs.contains(NEW_ID) && refs.contains(BASE_ID),
        "candidate removed, base preserved"
    );
    assert!(
        root.join(".git/index.lock").exists(),
        "the foreign lock is left in place"
    );
}
