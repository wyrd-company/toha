use super::*;
use crate::applied::{FrozenNow, Project};
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
    if args.first() == Some(&"init") {
        git(repo, &["config", "core.autocrlf", "false"]);
    }
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

thread_local! { static APPLICATIONS: std::cell::RefCell<std::collections::BTreeMap<(std::path::PathBuf,String),Application>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) }; }
fn make_snapshot(dir: &Path, id: &str, target: &str, files: &[(&str, &[u8])]) {
    let entries: Vec<_> = files
        .iter()
        .map(|(path, bytes)| (*path, *bytes, EntryKind::Blob))
        .collect();
    make_snapshot_entries(dir, id, target, &entries);
}

fn make_snapshot_entries(dir: &Path, id: &str, target: &str, files: &[(&str, &[u8], EntryKind)]) {
    let mut paths = Vec::new();
    let mut entries = std::collections::BTreeMap::new();
    for (path, bytes, kind) in files {
        let oid = gix::ObjectId::from_hex(hash_object(dir, bytes).as_bytes()).unwrap();
        paths.push(
            crate::applied::PathOwnership::new(
                crate::TargetPath::parse(path).unwrap(),
                Origin::Toha,
            )
            .with_blob(oid)
            .with_mode(*kind),
        );
        entries.insert((*path).to_owned(), (oid, *kind));
    }
    let mut application = Application::new(
        "forge:catalog/receipt@stable".into(),
        Revision::Commit(
            crate::applied::CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
        RepoPath::parse(target).unwrap(),
        FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
        Default::default(),
        Default::default(),
        vec![],
        paths,
    );
    application.files = entries;
    APPLICATIONS.with(|apps| {
        apps.borrow_mut().insert(
            (
                dir.canonicalize().expect("canonical fixture repository"),
                id.to_owned(),
            ),
            application,
        )
    });
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
        &Base::Application(base),
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
        &Base::Application(base),
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
        &Base::Application(base),
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
    assert!(refs.is_empty(), "no refs are created");
    assert!(
        !refs.contains(NEW_ID),
        "candidate ref removed on unsuccessful exit: {refs}"
    );
}

fn read_snapshot(project: &Project, id: &str) -> Application {
    let root = project
        .repo()
        .workdir()
        .unwrap()
        .canonicalize()
        .expect("canonical fixture repository");
    APPLICATIONS.with(|apps| {
        apps.borrow()
            .get(&(root, id.to_owned()))
            .expect("fixture snapshot")
            .clone()
    })
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
        &Base::Application(base),
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
        &Base::Application(base),
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
        &Base::Application(base),
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
        &Base::Application(base),
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
        refs.is_empty(),
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
        &Base::Application(base),
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
        &Base::Application(base),
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
        &Base::Application(base),
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
    assert!(refs.is_empty(), "candidate removed, base preserved");
    assert!(
        root.join(".git/index.lock").exists(),
        "the foreign lock is left in place"
    );
}

// --------------------------------------------------------------------------
// The shared candidate-tree builder end to end (checkout -> apply -> capture -> merge).
// --------------------------------------------------------------------------

fn capture_inputs_id(_head: &str, _seed: u8) -> crate::applied::capture::CaptureInputs {
    crate::applied::capture::CaptureInputs {
        template: "forge:catalog/receipt@stable".to_owned(),
        revision: Revision::Commit(
            crate::applied::CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
        generated: FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
        answers: Default::default(),
        context: Default::default(),
        parts: vec![],
    }
}

fn capture_inputs(head: &str) -> crate::applied::capture::CaptureInputs {
    capture_inputs_id(head, 9)
}

fn snapshot_inputs() -> crate::applied::RecordInputs {
    use crate::applied::record::{CommitId, Revision};
    crate::applied::RecordInputs {
        template: "forge:catalog/receipt@stable".to_owned(),
        revision: Revision::Commit(
            CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
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
        record: None,
        files,
        edits: vec![],
        conflicts: vec![],
        hooks: vec![],
        before_apply: None,
        after_apply: None,
        result_seed: vec![],
        git: None,
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
        &Base::Application(base),
        plan,
        capture_inputs(&head),
        true,
        &runner,
    )
    .unwrap();

    assert_eq!(
        project
            .repo()
            .find_blob(new.files["both.txt"].0)
            .unwrap()
            .data,
        b"one\ntwo\nthree\nfour\nFIVE\n"
    );

    // Merge the update into the operator: line 1 (operator) and line 5 (template) both apply.
    let base2 = read_snapshot(&project, BASE_ID);
    let result = merge_into_worktree(
        &project,
        &Base::Application(base2),
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
        &Base::Application(read_snapshot(&project, BASE_ID)),
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
    assert!(git(root, &["for-each-ref", "refs/toha/"]).is_empty());
    let result = merge_into_worktree(
        &project,
        &Base::Application(read_snapshot(&project, BASE_ID)),
        &new,
        &MergeOptions {
            trusted: true,
            dry_run: false,
        },
    )
    .unwrap();

    assert!(matches!(result, Merged::Written { .. }));
    let record = project.read_record().unwrap();
    assert_eq!(record.applications()[0].to_value(), new.to_value());
    assert!(git(root, &["for-each-ref", "refs/toha/"]).is_empty());

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
        &Base::Application(read_snapshot(&project, BASE_ID)),
        plan_of(vec![plan_file(src.path(), "app.txt", "v1\n")]),
        capture_inputs_id(&head, 5),
        true,
        &crate::hook::ProcessRunner,
    )
    .unwrap();

    // Behavior 16: the new snapshot carries the frozen `generated` instant.
    assert_eq!(
        new.generated().to_string(),
        "2026-03-14T09:26:53+00:00[UTC]"
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
        &Base::Application(read_snapshot(&project, BASE_ID)),
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
        &Base::Application(read_snapshot(&project, BASE_ID)),
        plan,
        capture_inputs_id(&head, 7),
        false,
        &FailingRunner,
    )
    .unwrap();

    assert_eq!(
        project
            .repo()
            .find_blob(new.files["app.txt"].0)
            .unwrap()
            .data,
        b"v2\n"
    );

    let result = merge_into_worktree(
        &project,
        &Base::Application(read_snapshot(&project, BASE_ID)),
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
    assert!(git(root, &["for-each-ref", "refs/toha/"]).is_empty());
    assert!(!root.join(".toha.json").exists());
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
        &Base::Application(read_snapshot(&project, BASE_ID)),
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
    assert!(refs.is_empty(), "no Toha refs");
    // Only the base ref exists (the candidate was never published).
    assert!(refs.is_empty(), "no candidate ref saved: {refs}");
}

#[test]
fn atomic_write_preserves_an_unrelated_temporary_filename() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("note.md"), b"old output");
    write(&dir.path().join("note.toha-tmp"), b"operator data");
    super::write_atomically(
        dir.path(),
        "note.md",
        b"new output",
        gix::index::entry::Mode::FILE,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("note.md")).unwrap(),
        b"new output"
    );
    assert_eq!(
        std::fs::read(dir.path().join("note.toha-tmp")).unwrap(),
        b"operator data"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn symlink_write_failures_restore_nodes_record_and_index() {
    // Cover retarget, addition and deletion, including dangling links. Fail only
    // after a real write, and retain an operator file outside the target.
    for operation in ["retarget", "add", "delete", "refused"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        write(&root.join("app/z.txt"), b"before\n");
        write(&root.join("outside.txt"), b"operator\n");
        if operation != "add" {
            std::os::unix::fs::symlink("first", root.join("app/a.txt")).unwrap();
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "example"]);
        let mut base_files = vec![("z.txt", b"before\n".as_slice(), EntryKind::Blob)];
        if operation != "add" {
            base_files.push(("a.txt", b"first", EntryKind::Link));
        }
        let mut new_files = if operation == "refused" {
            vec![("z.txt", b"invalid\0target".as_slice(), EntryKind::Link)]
        } else {
            vec![("z.txt", b"after\n".as_slice(), EntryKind::Blob)]
        };
        if operation != "delete" {
            new_files.push(("a.txt", b"second", EntryKind::Link));
        }
        make_snapshot_entries(root, BASE_ID, "app", &base_files);
        make_snapshot_entries(root, NEW_ID, "app", &new_files);
        let project = open(root, "app");
        let base = read_snapshot(&project, BASE_ID);
        let new = read_snapshot(&project, NEW_ID);
        project.write_application(base.clone()).unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "record"]);
        let record_before = std::fs::read(root.join(".toha.json")).unwrap();
        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        // Deletions run after writes, so fail after both z.txt and the deletion.
        if operation != "refused" {
            super::inject_fail::arm(if operation == "delete" { 2 } else { 1 });
        }
        let result = merge_into_worktree(
            &project,
            &Base::Application(base),
            &new,
            &MergeOptions {
                trusted: true,
                dry_run: false,
            },
        );
        super::inject_fail::disarm();
        if operation == "refused" {
            assert!(
                matches!(result, Err(MergeError::Io(ref message)) if !message.contains("injected")),
                "{result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(MergeError::Io(ref message)) if message == "injected transaction failure after push"),
                "{operation}: {result:?}"
            );
            let observed = super::inject_fail::observed();
            assert!(
                observed.iter().any(|(path, bytes)| path == "app/a.txt"
                    && if operation == "delete" {
                        bytes.is_none()
                    } else {
                        bytes.as_deref() == Some(b"second")
                    }),
                "write/deletion reached: {operation}: {observed:?}"
            );
        }
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before,
            "{operation}"
        );
        assert_eq!(
            std::fs::read(root.join(".toha.json")).unwrap(),
            record_before,
            "{operation}"
        );
        if operation == "add" {
            assert!(std::fs::symlink_metadata(root.join("app/a.txt")).is_err());
        } else {
            assert_eq!(
                std::fs::read_link(root.join("app/a.txt")).unwrap(),
                Path::new("first"),
                "{operation}"
            );
        }
        assert_eq!(std::fs::read(root.join("app/z.txt")).unwrap(), b"before\n");
        assert_eq!(
            std::fs::read(root.join("outside.txt")).unwrap(),
            b"operator\n"
        );
        assert!(
            git(root, &["status", "--porcelain"]).is_empty(),
            "{operation}"
        );
    }
}

#[cfg(unix)]
#[test]
fn refused_symlink_creation_preserves_existing_operator_node() {
    let dir = tempfile::tempdir().unwrap();
    let full = dir.path().join("node");
    for link in [false, true] {
        if link {
            std::os::unix::fs::symlink("operator", &full).unwrap();
        } else {
            std::fs::write(&full, b"operator\n").unwrap();
        }
        let result = write_atomically(
            dir.path(),
            "node",
            b"invalid\0target",
            gix::index::entry::Mode::SYMLINK,
        );
        assert!(matches!(result, Err(MergeError::Io(_))), "{result:?}");
        if link {
            assert_eq!(std::fs::read_link(&full).unwrap(), Path::new("operator"));
        } else {
            assert_eq!(std::fs::read(&full).unwrap(), b"operator\n");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        std::fs::remove_file(&full).unwrap();
    }
}

#[test]
fn symlink_blob_bytes_bypass_worktree_text_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join(".gitattributes"), b"link text eol=crlf\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "example"]);
    let repo = gix::open(root).unwrap();
    let (mut pipeline, _) = repo.filter_pipeline(None).unwrap();
    let id = repo.write_blob(b"target\n").unwrap().detach();
    // Prove that this attribute would transform a regular file's bytes.
    assert_eq!(
        worktree_bytes(
            &repo,
            &mut pipeline,
            id,
            "link",
            gix::index::entry::Mode::FILE
        )
        .unwrap(),
        b"target\r\n"
    );
    assert_eq!(
        worktree_bytes(
            &repo,
            &mut pipeline,
            id,
            "link",
            gix::index::entry::Mode::SYMLINK
        )
        .unwrap(),
        b"target\n"
    );
}

#[cfg(unix)]
#[test]
fn a_dangling_operator_link_is_occupied_before_any_update_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/keep.txt"), b"k\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    // An untracked file occupies the path the template adds.
    std::os::unix::fs::symlink("missing-operator-target", root.join("app/added.txt")).unwrap();

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
        &Base::Application(base),
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
        std::fs::read_link(root.join("app/added.txt")).unwrap(),
        std::path::Path::new("missing-operator-target")
    );
    let refs = git(root, &["for-each-ref", "refs/toha/snapshots/"]);
    assert!(refs.is_empty(), "no refs are created");
    assert!(
        !refs.contains(NEW_ID),
        "candidate ref removed on unsuccessful exit: {refs}"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_fixture_keys_follow_canonical_repository_identity() {
    let container = tempfile::tempdir().unwrap();
    let real = container.path().join("real");
    std::fs::create_dir(&real).unwrap();
    git(&real, &["init", "-q", "-b", "main"]);
    write(&real.join("seed.txt"), b"seed\n");
    git(&real, &["add", "."]);
    git(&real, &["commit", "-q", "-m", "seed"]);
    let alias = container.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    make_snapshot(&alias, BASE_ID, "app", &[("note.txt", b"base\n")]);
    let project = open(&alias, "app");
    let application = read_snapshot(&project, BASE_ID);
    assert_eq!(application.target().as_str(), "app");
}

#[test]
fn normalized_application_identity_preserves_symlink_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "core.autocrlf", "true"]);
    write(&root.join(".gitattributes"), b"app/link text eol=crlf\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "attributes"]);
    make_snapshot_entries(
        root,
        BASE_ID,
        "app",
        &[("link", b"target\n", EntryKind::Link)],
    );
    make_snapshot_entries(
        root,
        NEW_ID,
        "app",
        &[("link", b"target\r\n", EntryKind::Link)],
    );
    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    assert_ne!(
        normalized_files(&project, &base).unwrap(),
        normalized_files(&project, &new).unwrap(),
        "link targets are byte identities even with text attributes"
    );
}

#[test]
fn an_already_current_normalized_update_writes_no_loose_objects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join(".gitattributes"), b"app/app.txt text eol=lf\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "attributes"]);
    // Both captured forms normalize to a third blob that is not in the
    // operator repository. Comparing them must not store that third blob.
    make_snapshot(root, BASE_ID, "app", &[("app.txt", b"one\r\ntwo\n")]);
    make_snapshot(root, NEW_ID, "app", &[("app.txt", b"one\ntwo\r\n")]);
    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    let new = read_snapshot(&project, NEW_ID);
    assert_ne!(base.files, new.files);
    let normalized = gix::objs::compute_hash(
        project.repo().object_hash(),
        gix::objs::Kind::Blob,
        b"one\ntwo\n",
    )
    .unwrap();
    assert!(
        project
            .repo()
            .try_find_object(normalized)
            .unwrap()
            .is_none()
    );
    let before = git(root, &["count-objects", "-v"]);
    // This check is shared by dry-run and real updates before either merges.
    assert!(is_already_current(&project, &base, &new, &snapshot_inputs()).unwrap());
    assert_eq!(git(root, &["count-objects", "-v"]), before);
    assert!(
        project
            .repo()
            .try_find_object(normalized)
            .unwrap()
            .is_none()
    );
}

#[test]
fn merge_apply_keeps_an_already_current_normalized_update_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        &root.join(".gitattributes"),
        b"app/app.txt text=auto eol=lf\n",
    );
    write(&root.join("app/app.txt"), b"operator bytes\r\n");
    git(root, &["add", "."]);
    // Preserve a historical CRLF index entry. Auto-text capture respects this
    // entry, whereas normalized_files deliberately compares with an empty index.
    let operator_blob = hash_object(root, b"operator bytes\r\n");
    git(
        root,
        &[
            "update-index",
            "--cacheinfo",
            "100644",
            &operator_blob,
            "app/app.txt",
        ],
    );
    git(root, &["commit", "-qm", "operator"]);
    // The base and candidate have different captured bytes, but both normalize
    // to a third blob absent from the repository.
    make_snapshot(root, BASE_ID, "app", &[("app.txt", b"one\r\ntwo\n")]);
    let project = open(root, "app");
    let base = read_snapshot(&project, BASE_ID);
    project.write_application(base.clone()).unwrap();
    git(root, &["add", ".toha.json"]);
    git(root, &["commit", "-qm", "record"]);

    let source = tempfile::tempdir().unwrap();
    write(
        &source.path().join("template.yml"),
        b"name: example\ninterview: []\nstatic: ['**']\n",
    );
    let source_file = source.path().join("template/app.txt");
    write(&source_file, b"one\ntwo\r\n");
    let template = crate::template::Template::load(source.path()).unwrap();
    let seed = crate::interview::Seed {
        now: "2026-03-14T09:26:53+00:00[UTC]".parse().unwrap(),
        defaults: Default::default(),
        context: crate::context::InvocationContext::for_target(
            canonical_target(&root.join("app")).unwrap(),
        ),
    };
    let crate::interview::Interview::Complete(completed) =
        crate::interview::Interview::start(&template, seed).unwrap()
    else {
        panic!("the template has no questions");
    };
    let normalized = gix::objs::compute_hash(
        project.repo().object_hash(),
        gix::objs::Kind::Blob,
        b"one\ntwo\n",
    )
    .unwrap();
    assert!(
        project
            .repo()
            .try_find_object(normalized)
            .unwrap()
            .is_none()
    );
    let paths = [
        root.join("app/app.txt"),
        source_file,
        root.join(".gitattributes"),
        root.join(".toha.json"),
        root.join(".git/index"),
    ];
    let before = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    let modes_before = paths
        .each_ref()
        .map(|path| std::fs::metadata(path).unwrap().permissions());
    let refs_before = git(root, &["for-each-ref"]);
    let staged_before = git(root, &["ls-files", "--stage"]);
    let lock = root.join(".git/index.lock");
    assert!(!lock.exists());

    let outcome = merge_apply(
        &project,
        Base::Application(base),
        &template,
        &completed,
        snapshot_inputs(),
        MergeOptions {
            trusted: true,
            dry_run: false,
        },
        &crate::hook::ProcessRunner,
    )
    .unwrap();

    assert!(matches!(outcome, Merged::AlreadyCurrent), "{outcome:?}");
    assert_eq!(git(root, &["for-each-ref"]), refs_before);
    assert_eq!(git(root, &["ls-files", "--stage"]), staged_before);
    for ((path, bytes), mode) in paths.iter().zip(before).zip(modes_before) {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions(),
            mode,
            "{}",
            path.display()
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }
    assert!(!lock.exists(), "the index lock remains absent");
    // Candidate capture may write objects; only the comparison's normalized
    // blob must remain absent.
    assert!(
        project
            .repo()
            .try_find_object(normalized)
            .unwrap()
            .is_none()
    );
}
