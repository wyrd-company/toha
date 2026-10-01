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
fn an_unrelated_dirty_file_in_the_target_fails_the_locked_re_check() {
    // Behavior 23, the re-check's UNIQUE job: the locked re-check refuses when the
    // target is dirty in a file the plan does not write. The per-file drift check
    // is scoped to written paths, so only the whole-target re-check catches this;
    // nothing is written.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/both.txt"), b"base\n");
    write(&root.join("app/unrelated.txt"), b"committed\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    make_snapshot(
        root,
        BASE_ID,
        "app",
        &[("both.txt", b"base\n"), ("unrelated.txt", b"committed\n")],
    );
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[
            ("both.txt", b"new-template\n"),
            ("unrelated.txt", b"committed\n"),
        ],
    );

    // Dirty a file the update's plan does not touch. The per-file drift check for
    // both.txt would pass; only the whole-target re-check sees this.
    write(
        &root.join("app/unrelated.txt"),
        b"operator is editing unrelated\n",
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
        matches!(result, Err(MergeError::Changed)),
        "unrelated dirt fails the re-check: {result:?}"
    );
    // The plan target was not written and the candidate ref is removed.
    assert_eq!(
        std::fs::read_to_string(root.join("app/both.txt")).unwrap(),
        "base\n",
        "nothing written"
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(!refs.contains(NEW_ID), "candidate ref removed on refusal");
}

#[test]
fn a_dirty_target_refuses_at_the_prewrite_recheck_and_writes_nothing() {
    // Behavior 23: an untracked path git does not ignore makes the target dirty,
    // so the locked re-check refuses with `Changed` before `write_all` runs —
    // nothing is written and no rollback is exercised. (Genuine rollback after a
    // real write is proved by the sibling
    // `a_failure_after_a_write_but_before_commit_rolls_the_transaction_back`.)
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);

    // The template would add two files, but the operator has an untracked file at
    // `app/sub`, so the target is dirty before the update even plans a write.
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
    // `app/sub` is an untracked file: the locked re-check sees the dirty target.
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
    assert!(
        matches!(result, Err(MergeError::Changed)),
        "the dirty target refuses at the locked re-check before any write: {result:?}"
    );

    // Nothing was written: the re-check refused before `write_all`, so the
    // template's first add never reached disk (not written, not rolled back).
    assert!(
        !root.join("app/a.txt").exists(),
        "nothing was written: a.txt never reached disk"
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
        "candidate ref removed on the re-check refusal"
    );
}

#[test]
fn a_failure_after_a_write_but_before_commit_rolls_the_transaction_back() {
    // Behavior 24, isolating rollback itself: with a CLEAN tree the locked
    // re-check passes and the write transaction proceeds, so a failure injected
    // immediately after the first file is written (after done.push, before the
    // index is committed) must exercise rollback. We prove the write actually
    // happened, then that rollback restored the owned bytes, left the on-disk
    // index and outside-target entries untouched, and removed the candidate ref.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/a.txt"), b"one\n");
    write(&root.join("app/b.txt"), b"one\n");
    write(&root.join("outside.txt"), b"outside\n"); // an entry outside the target
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let index_before = std::fs::read(root.join(".git/index")).unwrap();

    // Base = operator's committed content; new changes both files (both written).
    make_snapshot(
        root,
        BASE_ID,
        "app",
        &[("a.txt", b"one\n"), ("b.txt", b"one\n")],
    );
    make_snapshot(
        root,
        NEW_ID,
        "app",
        &[("a.txt", b"two\n"), ("b.txt", b"two\n")],
    );

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);

    // Fail after the first push: a.txt is written (BTreeMap order), then abort.
    super::inject_fail::arm(1);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    );
    super::inject_fail::disarm();
    assert!(
        result.is_err(),
        "the injected transaction failure aborts: {result:?}"
    );

    // The raw on-disk index was never written by the aborted transaction — read
    // it BEFORE any git command (git status would refresh its stat cache). This
    // proves outside-target entries are intact byte-for-byte.
    assert_eq!(
        std::fs::read(root.join(".git/index")).unwrap(),
        index_before,
        "the on-disk index is unchanged (transaction never committed it)"
    );

    // The write ACTUALLY occurred: at the moment of failure a.txt held the
    // template bytes on disk (proving we reached write, not a pre-write refusal).
    let observed = super::inject_fail::observed();
    assert!(
        observed
            .iter()
            .any(|(p, b)| p == "app/a.txt" && b.as_deref() == Some(b"two\n".as_ref())),
        "a.txt was written before rollback: {observed:?}"
    );

    // Rollback restored the owned bytes: a.txt is back to HEAD, b.txt was never
    // reached, and the outside-target file is intact.
    assert_eq!(
        std::fs::read_to_string(root.join("app/a.txt")).unwrap(),
        "one\n",
        "a.txt restored to HEAD by rollback"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("app/b.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("outside.txt")).unwrap(),
        "outside\n"
    );
    // Nothing is left modified — worktree is clean (index and worktree agree).
    let status = git(root, &["status", "--porcelain"]);
    assert!(
        status.is_empty(),
        "worktree clean after rollback: {status:?}"
    );

    // The candidate ref was cleaned up (ref-map restored to just the base).
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(
        !refs.contains(NEW_ID) && refs.contains(BASE_ID),
        "candidate ref removed, base preserved: {refs}"
    );
}

#[test]
fn the_write_index_drops_the_stale_cache_tree_extension() {
    // remove_tree() is required: gix-index writes the TREE (cache-tree) extension
    // as-is, so once the transaction replaces index entries the stale cache-tree
    // must be dropped — otherwise a later git operation trusts it and computes a
    // tree that disagrees with the working tree. Seed a populated cache-tree, run
    // the merge, and assert the on-disk TREE extension is gone, read via gix
    // BEFORE any git command could rebuild it.
    use gix::hash::Kind;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/a.txt"), b"one\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    // Seed a populated cache-tree into the on-disk index.
    git(root, &["write-tree"]);
    let idx = root.join(".git/index");
    let seeded = gix::index::File::at(&idx, Kind::Sha1, false, Default::default()).unwrap();
    assert!(
        seeded.tree().is_some(),
        "the seeded index carries a populated cache-tree"
    );

    make_snapshot(root, BASE_ID, "app", &[("a.txt", b"one\n")]);
    make_snapshot(root, NEW_ID, "app", &[("a.txt", b"two\n")]);
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
    assert!(matches!(result, Merged::Written { .. }), "the merge writes");

    // Read the DISK index via gix, before any git command could rebuild the
    // cache-tree. The write transaction must have dropped the stale extension.
    let after = gix::index::File::at(&idx, Kind::Sha1, false, Default::default()).unwrap();
    assert!(
        after.tree().is_none(),
        "the write transaction dropped the stale cache-tree extension (remove_tree)"
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

// --------------------------------------------------------------------------
// The shared candidate-tree builder end to end (checkout -> apply -> capture -> merge).
// --------------------------------------------------------------------------

fn capture_inputs_id(head: &str, seed: u8) -> crate::snapshot::capture::CaptureInputs {
    use crate::snapshot::record::{CommitId, FrozenNow, ProjectPoint, Revision, Timestamp};
    crate::snapshot::capture::CaptureInputs {
        id: SnapshotId::from_parts(seed as u64, [seed; 10]),
        template: "forge:catalog/receipt@stable".to_owned(),
        revision: Revision::Commit(
            CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
        generated: FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
        created: Timestamp::parse("2026-09-30T04:12:00Z").unwrap(),
        project: ProjectPoint::new(CommitId::parse(head).unwrap(), Some("main".to_owned())),
        submissions: vec![],
    }
}

fn capture_inputs(head: &str) -> crate::snapshot::capture::CaptureInputs {
    capture_inputs_id(head, 9)
}

fn snapshot_inputs() -> crate::snapshot::SnapshotInputs {
    use crate::snapshot::record::{CommitId, FrozenNow, Revision};
    crate::snapshot::SnapshotInputs {
        template: "forge:catalog/receipt@stable".to_owned(),
        revision: Revision::Commit(
            CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
        generated: FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
        submissions: vec![],
    }
}

fn plan_file(src_dir: &Path, path: &str, content: &str) -> crate::plan::PlannedFile {
    // Plan::apply reads the source file's metadata for the executable bit, so the
    // source must exist; a plain template file suffices.
    let source = src_dir.join(path);
    write(&source, content.as_bytes());
    crate::plan::PlannedFile {
        path: crate::plan::TargetPath::parse(path).unwrap(),
        content: crate::plan::Content::Rendered(content.to_owned()),
        source,
    }
}

fn plan_of(files: Vec<crate::plan::PlannedFile>) -> crate::plan::Plan {
    crate::plan::Plan {
        files,
        edits: vec![],
        conflicts: vec![],
        hooks: vec![],
        before_apply: None,
        after_apply: None,
        result_seed: vec![],
    }
}

#[test]
fn the_candidate_builder_and_merge_apply_a_from_update_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    // Operator committed template v1 output, then edited line 1 of both.txt.
    write(&root.join("app/both.txt"), b"ONE\ntwo\nthree\nfour\nfive\n");
    write(&root.join("app/main.txt"), b"keep\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    // Base snapshot = template v1 output.
    make_snapshot(
        root,
        BASE_ID,
        "app",
        &[
            ("both.txt", b"one\ntwo\nthree\nfour\nfive\n"),
            ("main.txt", b"keep\n"),
        ],
    );

    // Template v2 changes line 5 of both.txt.
    let src = tempfile::tempdir().unwrap();
    let plan = plan_of(vec![
        plan_file(src.path(), "both.txt", "one\ntwo\nthree\nfour\nFIVE\n"),
        plan_file(src.path(), "main.txt", "keep\n"),
    ]);

    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let runner = crate::hook::ProcessRunner;
    let new = super::build_candidate(
        &project,
        &Base::Snapshot(base),
        plan,
        capture_inputs(&head),
        true,
        &runner,
    )
    .unwrap();

    // The new snapshot captured template v2's both.txt (built through the checkout).
    assert_eq!(
        git(
            root,
            &[
                "show",
                &format!("refs/toha/snapshots/{}:files/both.txt", new.id())
            ]
        ),
        "one\ntwo\nthree\nfour\nFIVE",
    );

    // Merge the update into the operator: line 1 (operator) and line 5 (template) both apply.
    let base2 = read_snapshot(&project, BASE_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(base2),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap();
    assert!(matches!(result, Merged::Written { .. }));
    assert_eq!(
        std::fs::read_to_string(root.join("app/both.txt")).unwrap(),
        "ONE\ntwo\nthree\nfour\nFIVE\n",
        "operator's line 1 and template's line 5 both merged",
    );
}

#[test]
fn a_successful_update_preserves_the_committed_snapshot_ref_and_reports_it() {
    // Behavior 31, post-commit side: a successful write is NOT an unsuccessful
    // exit, so its candidate ref is preserved (not cleaned up), the reported
    // `Written.snapshot` equals the surviving ref, and the committed index tree
    // equals the working tree for the target (`git commit` succeeds cleanly).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/both.txt"), b"ONE\ntwo\nthree\nfour\nfive\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    make_snapshot(
        root,
        BASE_ID,
        "app",
        &[("both.txt", b"one\ntwo\nthree\nfour\nfive\n")],
    );

    let src = tempfile::tempdir().unwrap();
    let project = open(root, "app");
    // Template v2 changes only line 5 — a clean merge with the operator's line 1.
    let new = super::build_candidate(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        plan_of(vec![plan_file(
            src.path(),
            "both.txt",
            "one\ntwo\nthree\nfour\nFIVE\n",
        )]),
        capture_inputs(&head),
        true,
        &crate::hook::ProcessRunner,
    )
    .unwrap();
    let new_id = *new.id();

    let before = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(
        before.contains(&new_id.to_string()) && before.contains(BASE_ID),
        "the candidate ref exists before the merge: {before}"
    );

    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap();

    // The reported snapshot equals the captured candidate.
    match &result {
        Merged::Written { snapshot, .. } => {
            assert_eq!(*snapshot, new_id, "the report names the committed snapshot")
        }
        other => panic!("expected a written update, got {other:?}"),
    }

    // Post-commit: the candidate ref is preserved (a successful exit never cleans
    // it up), alongside the base — no ref was lost by the successful commit.
    let after = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(
        after.contains(&new_id.to_string()),
        "the committed snapshot ref is preserved: {after}"
    );
    assert!(
        after.contains(BASE_ID),
        "the base ref is preserved: {after}"
    );

    // The committed result is consistent: staging the merged worktree and
    // committing succeeds with no unmerged paths, and the merged bytes are on disk.
    assert_eq!(
        std::fs::read_to_string(root.join("app/both.txt")).unwrap(),
        "ONE\ntwo\nthree\nfour\nFIVE\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "apply update"]);
    let committed = git(root, &["show", "HEAD:app/both.txt"]);
    assert_eq!(
        committed, "ONE\ntwo\nthree\nfour\nFIVE",
        "the committed tree equals the merged working tree",
    );
}

#[test]
fn an_already_current_update_reports_no_change_and_carries_the_frozen_instant() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/app.txt"), b"v1\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    make_snapshot(root, BASE_ID, "app", &[("app.txt", b"v1\n")]);

    let project = open(root, "app");
    let src = tempfile::tempdir().unwrap();

    // A plan that reproduces the same content builds a snapshot equal to the base.
    let new = super::build_candidate(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        plan_of(vec![plan_file(src.path(), "app.txt", "v1\n")]),
        capture_inputs_id(&head, 5),
        true,
        &crate::hook::ProcessRunner,
    )
    .unwrap();

    // Behavior 16: the new snapshot carries the frozen `generated` instant.
    assert_eq!(
        new.generated().to_string(),
        snapshot_inputs().generated.to_string()
    );
    // Behavior 15: it is already current.
    assert!(
        super::is_already_current(
            &project,
            &read_snapshot(&project, BASE_ID),
            &new,
            &snapshot_inputs()
        )
        .unwrap()
    );

    // A different (edited) template output is NOT already current.
    let src2 = tempfile::tempdir().unwrap();
    let new2 = super::build_candidate(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        plan_of(vec![plan_file(src2.path(), "app.txt", "v2\n")]),
        capture_inputs_id(&head, 6),
        true,
        &crate::hook::ProcessRunner,
    )
    .unwrap();
    assert!(
        !super::is_already_current(
            &project,
            &read_snapshot(&project, BASE_ID),
            &new2,
            &snapshot_inputs()
        )
        .unwrap()
    );
}

#[test]
fn a_dry_run_previews_the_change_and_saves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/app.txt"), b"v1\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    make_snapshot(root, BASE_ID, "app", &[("app.txt", b"v1\n")]);

    let project = open(root, "app");
    let src = tempfile::tempdir().unwrap();
    // Behavior 20: a dry run runs NO hooks. The plan carries a hook that always
    // fails, and the runner would error if it were invoked — so building the
    // candidate with `run_hooks = false` must skip it and succeed. (If a dry run
    // ran the hook, build_candidate would return the hook's failure here.)
    let mut plan = plan_of(vec![plan_file(src.path(), "app.txt", "v2\n")]);
    plan.hooks.push(failing_hook());
    let new = super::build_candidate(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        plan,
        capture_inputs_id(&head, 7),
        false,
        &FailingRunner,
    )
    .unwrap();

    // The candidate's own captured tree carries the new render, not the base's
    // stale bytes: a dry run must still produce an accurate preview, it just
    // must not run the hook.
    let captured = git(
        root,
        &[
            "show",
            &format!("{SNAPSHOT_REF_PREFIX}{}:files/app.txt", new.id()),
        ],
    );
    assert_eq!(
        captured, "v2",
        "the candidate must capture the new render, not the base's v1"
    );

    let result = merge_into_worktree(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: true,
        },
    )
    .unwrap();
    assert!(
        matches!(result, Merged::Planned { .. }),
        "a dry run previews"
    );
    // The operator's working tree is untouched.
    assert_eq!(
        std::fs::read_to_string(root.join("app/app.txt")).unwrap(),
        "v1\n"
    );
    // A dry run saves nothing: removing the candidate (as merge_apply does) leaves no ref.
    let published = super::published_commit(&project, new.id()).unwrap();
    super::remove_candidate(&project, new.id(), published).unwrap();
    assert!(!git(root, &["for-each-ref", "refs/toha/snapshots/"]).contains(&new.id().to_string()));
}

struct FailingRunner;
impl crate::hook::HookRunner for FailingRunner {
    fn run(
        &self,
        _hook: &crate::plan::PlannedHook,
        _target: &Path,
    ) -> Result<crate::hook::HookOutcome, crate::hook::HookError> {
        Ok(crate::hook::HookOutcome {
            success: false,
            code: Some(1),
            stdout: None,
            stderr: None,
        })
    }
}

fn failing_hook() -> crate::plan::Planned<crate::plan::PlannedHook> {
    crate::plan::Planned::Ready(crate::plan::PlannedHook {
        program: crate::plan::PlannedProgram::Run(vec!["true".to_owned()]),
        cwd: None,
        template_root: std::path::PathBuf::new(),
        id: None,
        capture: crate::template::Capture::default(),
        allow_failure: false,
        parse_json: false,
        status_id: None,
    })
}

#[test]
fn a_hook_failure_leaves_the_project_unchanged_and_saves_no_ref() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/app.txt"), b"v1\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    make_snapshot(root, BASE_ID, "app", &[("app.txt", b"v1\n")]);

    let project = open(root, "app");
    let src = tempfile::tempdir().unwrap();
    let mut plan = plan_of(vec![plan_file(src.path(), "app.txt", "v2\n")]);
    plan.hooks.push(failing_hook());

    // The hook fails in the throwaway checkout.
    let result = super::build_candidate(
        &project,
        &Base::Snapshot(read_snapshot(&project, BASE_ID)),
        plan,
        capture_inputs_id(&head, 8),
        true,
        &FailingRunner,
    );
    assert!(
        result.is_err(),
        "a hook failure aborts the build: {result:?}"
    );

    // The operator's project is unchanged (only the throwaway checkout was touched),
    // and no new candidate ref was saved.
    assert_eq!(
        std::fs::read_to_string(root.join("app/app.txt")).unwrap(),
        "v1\n"
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(refs.contains(BASE_ID), "base preserved");
    // Only the base ref exists (the candidate was never published).
    assert_eq!(refs.lines().count(), 1, "no candidate ref saved: {refs}");
}
