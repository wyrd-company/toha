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
