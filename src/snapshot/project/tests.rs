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
  "project": {{ "commit": "a41c0de00000000000000000000000000000beef", "branch": "main" }},
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
