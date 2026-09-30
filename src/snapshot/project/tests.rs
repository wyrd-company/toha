use super::*;
use crate::staging::canonical_target;
use std::path::Path;
use std::process::Command;

/// Run a git command in `repo` for fixture setup; git is not under test here.
fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A repository with one commit that puts `content` at `app/file.txt`.
fn repo_with_app(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(&dir.join("app/file.txt"), b"one\ntwo\n");
    write(&dir.join("README.md"), b"readme\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

#[cfg(test)]
impl Project {
    /// Test-only: whether a config key resolves in the (driver-stripped) config.
    fn config_present(&self, key: &str) -> bool {
        self.repo.config_snapshot().string(key).is_some()
    }
}

fn open_at(path: &Path) -> Option<Project> {
    let target = canonical_target(path).expect("canonical target");
    Project::open(&target).expect("open")
}

#[test]
fn open_returns_none_outside_a_git_repository() {
    let dir = tempfile::tempdir().unwrap();
    assert!(open_at(dir.path()).is_none());
}

#[test]
fn open_strips_named_drivers_but_keeps_top_level_keys() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    git(
        dir.path(),
        &["config", "merge.secret.driver", "false %O %A %B"],
    );
    git(dir.path(), &["config", "filter.secret.clean", "cat"]);
    git(dir.path(), &["config", "filter.secret.smudge", "cat"]);
    git(dir.path(), &["config", "merge.conflictStyle", "diff3"]);

    let project = open_at(dir.path()).expect("in git");
    assert!(
        !project.config_present("merge.secret.driver"),
        "merge driver removed"
    );
    assert!(
        !project.config_present("filter.secret.clean"),
        "filter clean removed"
    );
    assert!(
        !project.config_present("filter.secret.smudge"),
        "filter smudge removed"
    );
    assert!(
        project.config_present("merge.conflictStyle"),
        "top-level merge key preserved"
    );
}

#[test]
fn a_clean_target_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let project = open_at(&dir.path().join("app")).expect("in git");
    assert_eq!(project.cleanliness().unwrap(), Cleanliness::Clean);
}

#[test]
fn dirt_inside_the_target_is_detected_and_dirt_outside_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    // Unstaged modification inside the target.
    write(&dir.path().join("app/file.txt"), b"one\ntwo\nthree\n");
    // Untracked file outside the target.
    write(&dir.path().join("elsewhere.txt"), b"x\n");

    let project = open_at(&dir.path().join("app")).expect("in git");
    let Cleanliness::Dirty { paths } = project.cleanliness().unwrap() else {
        panic!("expected dirty");
    };
    assert!(paths.iter().any(|p| p.as_str() == "app/file.txt"));
    assert!(
        !paths.iter().any(|p| p.as_str().contains("elsewhere")),
        "dirt outside the target does not count: {paths:?}"
    );
}

#[test]
fn a_root_target_sees_the_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    write(&dir.path().join("README.md"), b"changed\n");
    let project = open_at(dir.path()).expect("in git");
    assert_eq!(project.target(), &RepoPath::root());
    let Cleanliness::Dirty { paths } = project.cleanliness().unwrap() else {
        panic!("expected dirty");
    };
    assert!(paths.iter().any(|p| p.as_str() == "README.md"));
}

// --------------------------------------------------------------------------
// Slice 2b: snapshot ref listing, find, and git-level forgery refusal.
// --------------------------------------------------------------------------

/// Run git capturing stdout, with an optional dedicated index file for building
/// trees without touching the working index.
fn git_out(repo: &Path, index: Option<&Path>, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let out = cmd
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
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
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
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Build a tree object from explicit `git mktree` entry lines
/// (`<mode> SP <type> SP <sha> TAB <name>`), so a test can forge a malformed
/// top-level snapshot tree the index-based `make_snapshot` cannot express (a
/// `snapshot.json` subtree, a `files` blob). Returns the tree id.
fn mktree(repo: &Path, spec: &str) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("mktree")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("git mktree spawns");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(spec.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "git mktree: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Commit a forged top-level tree as a parentless snapshot commit under a ULID
/// ref, so the read boundary meets exactly the tree shape under test.
fn forge_snapshot_ref(repo: &Path, id: &str, top_tree: &str) {
    let commit = git_out(repo, None, &["commit-tree", top_tree, "-m", "snapshot"]);
    git_out(
        repo,
        None,
        &["update-ref", &format!("refs/toha/snapshots/{id}"), &commit],
    );
}

struct SnapshotFile {
    path: &'static str,
    bytes: &'static [u8],
    mode: &'static str,
}

/// Build a snapshot commit and ref by git plumbing. `parent` and `extra_entries`
/// and `gitlink` let a test forge a malformed snapshot. Returns the commit id.
fn make_snapshot(
    dir: &Path,
    id: &str,
    target: &str,
    files: &[SnapshotFile],
    parent: Option<&str>,
    extra_top_entry: bool,
    gitlink: Option<&str>,
) -> String {
    make_snapshot_pc(
        dir,
        id,
        target,
        PROJECT_COMMIT,
        files,
        parent,
        extra_top_entry,
        gitlink,
    )
}

const PROJECT_COMMIT: &str = "a41c0de00000000000000000000000000000beef";

/// As [`make_snapshot`], but with an explicit recorded project commit, for
/// ancestry-based likely-base tests.
#[allow(clippy::too_many_arguments)]
fn make_snapshot_pc(
    dir: &Path,
    id: &str,
    target: &str,
    project_commit: &str,
    files: &[SnapshotFile],
    parent: Option<&str>,
    extra_top_entry: bool,
    gitlink: Option<&str>,
) -> String {
    let index = dir.join(format!(".idx-{id}"));
    let _ = std::fs::remove_file(&index);
    // snapshot.json with paths equal to the files (all origin toha).
    let paths_json: String = files
        .iter()
        .map(|f| format!("    \"{}\": {{ \"origin\": \"toha\" }}", f.path))
        .collect::<Vec<_>>()
        .join(",\n");
    let json = format!(
        r#"{{
  "snapshot": 1,
  "id": "{id}",
  "template": "forge:catalog/receipt@stable",
  "source": "forge:catalog/receipt",
  "commit": "8be0d41c2f0000000000000000000000000000a1",
  "target": "{target}",
  "created": "2026-09-30T04:12:00Z",
  "generated": "2026-03-14T09:26:53+00:00[UTC]",
  "project": {{ "commit": "{project_commit}", "branch": "main" }},
  "built_from": null,
  "submissions": [],
  "paths": {{
{paths_json}
  }}
}}"#
    );
    let json_oid = hash_object(dir, json.as_bytes());
    git_out(
        dir,
        Some(&index),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{json_oid},snapshot.json"),
        ],
    );
    for file in files {
        let oid = hash_object(dir, file.bytes);
        git_out(
            dir,
            Some(&index),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{},{oid},files/{}", file.mode, file.path),
            ],
        );
    }
    if extra_top_entry {
        let oid = hash_object(dir, b"surprise\n");
        git_out(
            dir,
            Some(&index),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{oid},extra.txt"),
            ],
        );
    }
    if let Some(commit) = gitlink {
        git_out(
            dir,
            Some(&index),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{commit},files/sub"),
            ],
        );
    }
    let tree = git_out(dir, Some(&index), &["write-tree"]);
    let mut commit_args = vec![
        "commit-tree".to_string(),
        tree,
        "-m".to_string(),
        "snapshot".to_string(),
    ];
    if let Some(parent) = parent {
        commit_args.push("-p".to_string());
        commit_args.push(parent.to_string());
    }
    let commit_argv: Vec<&str> = commit_args.iter().map(String::as_str).collect();
    let commit = git_out(dir, None, &commit_argv);
    git_out(
        dir,
        None,
        &["update-ref", &format!("refs/toha/snapshots/{id}"), &commit],
    );
    let _ = std::fs::remove_file(&index);
    commit
}

const ID_A: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6E";
const ID_B: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6F";

fn one_file() -> Vec<SnapshotFile> {
    vec![SnapshotFile {
        path: "app.txt",
        bytes: b"one\n",
        mode: "100644",
    }]
}

#[test]
fn snapshots_lists_valid_target_snapshots_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    make_snapshot(dir.path(), ID_A, "app", &one_file(), None, false, None);
    make_snapshot(dir.path(), ID_B, "app", &one_file(), None, false, None);
    // A snapshot for another target is excluded from this target's listing.
    make_snapshot(
        dir.path(),
        "01JA2B8M4R0C7W1Y5F3H9K2S70",
        "other",
        &one_file(),
        None,
        false,
        None,
    );

    let project = open_at(&dir.path().join("app")).expect("in git");
    let listed = project.snapshots().unwrap();
    let ids: Vec<String> = listed
        .iter()
        .filter_map(|l| match l {
            Listed::Valid(s) => Some(s.id().to_string()),
            Listed::Invalid { .. } => None,
        })
        .collect();
    assert_eq!(
        ids,
        vec![ID_B.to_string(), ID_A.to_string()],
        "newest first, other target excluded"
    );
}

#[test]
fn find_resolves_a_unique_prefix_and_reports_ambiguity_and_absence() {
    // ID_A and ID_C share only the first ten characters, so a twelve-character
    // prefix distinguishes them while the shared six-character prefix does not.
    const ID_C: &str = "01JA2B8M4RZZ7W1Y5F3H9K2S6E";
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    make_snapshot(dir.path(), ID_A, "app", &one_file(), None, false, None);
    make_snapshot(dir.path(), ID_C, "app", &one_file(), None, false, None);
    let project = open_at(&dir.path().join("app")).expect("in git");

    // A prefix that distinguishes the two resolves.
    assert_eq!(project.find(&ID_A[..12]).unwrap().id().to_string(), ID_A);
    // Their shared prefix is ambiguous.
    assert!(matches!(
        project.find(&ID_A[..10]),
        Err(SnapshotError::Ambiguous { .. })
    ));
    // A prefix nothing matches is unknown.
    assert!(matches!(
        project.find("ZZZZZZ"),
        Err(SnapshotError::Unknown(_))
    ));
}

#[test]
fn a_snapshot_commit_with_a_parent_is_listed_invalid() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let head = git_out(dir.path(), None, &["rev-parse", "HEAD"]);
    make_snapshot(
        dir.path(),
        ID_A,
        "app",
        &one_file(),
        Some(&head),
        false,
        None,
    );
    let project = open_at(&dir.path().join("app")).expect("in git");
    let listed = project.snapshots().unwrap();
    assert!(
        listed
            .iter()
            .any(|l| matches!(l, Listed::Invalid { reason, .. } if reason.contains("parent"))),
        "parent snapshot is invalid: {listed:?}"
    );
}

#[test]
fn an_extra_top_level_tree_entry_is_listed_invalid() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    make_snapshot(dir.path(), ID_A, "app", &one_file(), None, true, None);
    let project = open_at(&dir.path().join("app")).expect("in git");
    assert!(project.snapshots().unwrap().iter().any(
        |l| matches!(l, Listed::Invalid { reason, .. } if reason.contains("unexpected entry"))
    ));
}

#[test]
fn a_gitlink_under_files_is_listed_invalid() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let head = git_out(dir.path(), None, &["rev-parse", "HEAD"]);
    make_snapshot(
        dir.path(),
        ID_A,
        "app",
        &one_file(),
        None,
        false,
        Some(&head),
    );
    let project = open_at(&dir.path().join("app")).expect("in git");
    assert!(project.snapshots().unwrap().iter().any(
        |l| matches!(l, Listed::Invalid { reason, .. } if reason.contains("non-regular entry"))
    ));
}

#[test]
fn a_ref_that_is_not_a_ulid_is_listed_invalid() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let head = git_out(dir.path(), None, &["rev-parse", "HEAD"]);
    git_out(
        dir.path(),
        None,
        &["update-ref", "refs/toha/snapshots/not-a-ulid", &head],
    );
    let project = open_at(dir.path()).expect("in git");
    assert!(
        project
            .snapshots()
            .unwrap()
            .iter()
            .any(|l| matches!(l, Listed::Invalid { .. }))
    );
}

#[test]
fn a_snapshot_json_that_is_a_subtree_is_listed_invalid() {
    // Behavior 30 read boundary: `snapshot.json` must be a regular blob. A
    // subtree at that name is rejected with `SnapshotJsonNotBlob`.
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let root = dir.path();
    let app_oid = hash_object(root, b"one\n");
    let files_tree = mktree(root, &format!("100644 blob {app_oid}\tapp.txt\n"));
    let dummy = hash_object(root, b"x\n");
    let sj_subtree = mktree(root, &format!("100644 blob {dummy}\tx\n"));
    let top = mktree(
        root,
        &format!("040000 tree {files_tree}\tfiles\n040000 tree {sj_subtree}\tsnapshot.json\n"),
    );
    forge_snapshot_ref(root, ID_A, &top);

    let project = open_at(root).expect("in git");
    let listed = project.snapshots().unwrap();
    assert!(
        listed.iter().any(|l| matches!(
            l,
            Listed::Invalid { reason, .. } if reason.contains("not a regular file")
        )),
        "snapshot.json as a subtree is rejected: {listed:?}"
    );
}

#[test]
fn a_files_entry_that_is_a_blob_is_listed_invalid() {
    // Behavior 30 read boundary: `files` must be a tree. A blob at that name is
    // rejected with `FilesNotTree`.
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let root = dir.path();
    let files_blob = hash_object(root, b"not a tree\n");
    let sj = hash_object(root, b"{}\n");
    let top = mktree(
        root,
        &format!("100644 blob {files_blob}\tfiles\n100644 blob {sj}\tsnapshot.json\n"),
    );
    forge_snapshot_ref(root, ID_A, &top);

    let project = open_at(root).expect("in git");
    let listed = project.snapshots().unwrap();
    assert!(
        listed.iter().any(|l| matches!(
            l,
            Listed::Invalid { reason, .. } if reason.contains("files/ is not a tree")
        )),
        "files/ as a blob is rejected: {listed:?}"
    );
}

#[test]
fn a_snapshot_tree_without_snapshot_json_is_listed_invalid() {
    // Behavior 30 read boundary: a tree with no `snapshot.json` entry is rejected
    // with `MissingSnapshotJson`.
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let root = dir.path();
    let app_oid = hash_object(root, b"one\n");
    let files_tree = mktree(root, &format!("100644 blob {app_oid}\tapp.txt\n"));
    let top = mktree(root, &format!("040000 tree {files_tree}\tfiles\n"));
    forge_snapshot_ref(root, ID_A, &top);

    let project = open_at(root).expect("in git");
    let listed = project.snapshots().unwrap();
    assert!(
        listed.iter().any(|l| matches!(
            l,
            Listed::Invalid { reason, .. } if reason.contains("missing snapshot.json")
        )),
        "a tree without snapshot.json is rejected: {listed:?}"
    );
}

// --------------------------------------------------------------------------
// Slice 2b: likely bases and removal.
// --------------------------------------------------------------------------

fn valid_snapshots(project: &Project) -> Vec<crate::snapshot::Snapshot> {
    project
        .snapshots()
        .unwrap()
        .into_iter()
        .filter_map(|l| match l {
            Listed::Valid(s) => Some(s),
            Listed::Invalid { .. } => None,
        })
        .collect()
}

#[test]
fn likely_base_is_marked_by_ancestry_when_a_project_commit_is_an_ancestor_of_head() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let first = git_out(dir.path(), None, &["rev-parse", "HEAD"]);
    // Advance HEAD so `first` is a strict ancestor.
    write(&dir.path().join("app/file.txt"), b"one\ntwo\nthree\n");
    git(dir.path(), &["commit", "-qam", "second"]);

    // ID_A records an ancestor commit; ID_B records an unrelated commit.
    make_snapshot_pc(
        dir.path(),
        ID_A,
        "app",
        &first,
        &one_file(),
        None,
        false,
        None,
    );
    make_snapshot_pc(
        dir.path(),
        ID_B,
        "app",
        "deadbeef00000000000000000000000000000000",
        &one_file(),
        None,
        false,
        None,
    );

    let project = open_at(&dir.path().join("app")).expect("in git");
    let marks = project.likely_bases(&valid_snapshots(&project)).unwrap();
    assert_eq!(marks.len(), 1, "one mark per source");
    assert_eq!(marks[0].id.to_string(), ID_A);
    assert_eq!(marks[0].by, LikelyBaseBy::Ancestry);
}

#[test]
fn likely_base_falls_back_to_content_when_no_project_commit_is_an_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    // The target app/ holds app/file.txt == "one\ntwo\n" at HEAD.
    let matching = &[SnapshotFile {
        path: "file.txt",
        bytes: b"one\ntwo\n",
        mode: "100644",
    }];
    let other = &[SnapshotFile {
        path: "file.txt",
        bytes: b"different\n",
        mode: "100644",
    }];
    let unrelated = "deadbeef00000000000000000000000000000000";
    make_snapshot_pc(dir.path(), ID_A, "app", unrelated, other, None, false, None);
    make_snapshot_pc(
        dir.path(),
        ID_B,
        "app",
        unrelated,
        matching,
        None,
        false,
        None,
    );

    let project = open_at(&dir.path().join("app")).expect("in git");
    let marks = project.likely_bases(&valid_snapshots(&project)).unwrap();
    assert_eq!(marks.len(), 1);
    assert_eq!(
        marks[0].id.to_string(),
        ID_B,
        "the content-matching snapshot wins"
    );
    assert_eq!(marks[0].by, LikelyBaseBy::Content);
}

#[test]
fn remove_deletes_named_snapshots_and_reports_unknown_ids() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    // Two snapshots of the same source so removing one does not empty it.
    make_snapshot(dir.path(), ID_A, "app", &one_file(), None, false, None);
    make_snapshot(dir.path(), ID_B, "app", &one_file(), None, false, None);

    let project = open_at(&dir.path().join("app")).expect("in git");
    let a: crate::snapshot::SnapshotId = ID_A.parse().unwrap();
    let missing: crate::snapshot::SnapshotId = "01JA2B8M4R0C7W1Y5F3H9K2S70".parse().unwrap();
    let removed = project.remove(&[a, missing], false).unwrap();
    assert_eq!(removed.removed, vec![a]);
    assert_eq!(removed.not_found, vec![missing]);
    // The ref is gone; the sibling snapshot remains.
    let remaining = valid_snapshots(&project);
    assert!(remaining.iter().all(|s| s.id() != &a));
    assert_eq!(remaining.len(), 1);
}

#[test]
fn remove_refuses_to_empty_a_source_without_force() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    make_snapshot(dir.path(), ID_A, "app", &one_file(), None, false, None);
    make_snapshot(dir.path(), ID_B, "app", &one_file(), None, false, None);
    let project = open_at(&dir.path().join("app")).expect("in git");
    let a: crate::snapshot::SnapshotId = ID_A.parse().unwrap();
    let b: crate::snapshot::SnapshotId = ID_B.parse().unwrap();

    // Both belong to the same source; removing both empties it.
    assert!(matches!(
        project.remove(&[a, b], false),
        Err(SnapshotError::WholeSource { .. })
    ));
    // Nothing was removed.
    assert_eq!(valid_snapshots(&project).len(), 2);
    // With force it proceeds.
    let removed = project.remove(&[a, b], true).unwrap();
    assert_eq!(removed.removed.len(), 2);
    assert_eq!(valid_snapshots(&project).len(), 0);
}

// --------------------------------------------------------------------------
// Slice 2b: toha init (add_fetch).
// --------------------------------------------------------------------------

#[test]
fn add_fetch_adds_the_refspec_once_and_writes_no_push_setting() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    git(
        dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    let project = open_at(dir.path()).expect("in git");

    let first = project.add_fetch("origin").unwrap();
    assert!(first.added, "the refspec is newly added");
    assert_eq!(
        first.refspec,
        "+refs/toha/snapshots/*:refs/toha/snapshots/*"
    );

    // Idempotent: a second call adds nothing.
    let second = project.add_fetch("origin").unwrap();
    assert!(!second.added, "the refspec is already present");

    let config = std::fs::read_to_string(dir.path().join(".git/config")).unwrap();
    let occurrences = config
        .matches("refs/toha/snapshots/*:refs/toha/snapshots/*")
        .count();
    assert_eq!(
        occurrences, 1,
        "the refspec appears exactly once:\n{config}"
    );
    assert!(
        !config.contains("push"),
        "no push setting is written:\n{config}"
    );
    // The pre-existing fetch refspec for origin is preserved.
    assert!(config.contains("+refs/heads/*:refs/remotes/origin/*"));
}

#[test]
fn add_fetch_refuses_a_missing_remote() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_app(dir.path());
    let project = open_at(dir.path()).expect("in git");
    assert!(matches!(
        project.add_fetch("origin"),
        Err(ProjectError::RemoteMissing(_))
    ));
}
