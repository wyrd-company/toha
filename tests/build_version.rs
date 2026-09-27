// ---
// relationships:
//   implements: architecture
// ---
#[path = "../build/version.rs"]
mod version;

use std::{fs, path::Path, process::Command};

const FALLBACK: &str = "0.0.0";

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository at `path` with one commit tagged `tag`.
fn tagged_repository(path: &Path, tag: &str) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.name", "Sample"]);
    git(path, &["config", "user.email", "sample@example.invalid"]);
    fs::write(path.join("sample.txt"), "sample").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-q", "-m", "sample"]);
    git(path, &["tag", "-a", tag, "-m", tag]);
}

#[test]
fn work_tree_root_uses_the_tag_at_head() {
    let root = tempfile::tempdir().unwrap();
    tagged_repository(root.path(), "1.2.3");
    let resolution = version::resolve(root.path(), FALLBACK);
    assert_eq!(resolution.version, "1.2.3");
    assert!(resolution.warning.is_none());
}

#[test]
fn root_inside_another_repository_uses_the_fallback() {
    let parent = tempfile::tempdir().unwrap();
    tagged_repository(parent.path(), "9.9.9");
    let root = parent.path().join("package");
    fs::create_dir_all(&root).unwrap();
    let resolution = version::resolve(&root, FALLBACK);
    assert_eq!(resolution.version, FALLBACK);
    assert!(resolution.watched.is_empty());
}

#[test]
fn root_without_a_repository_uses_the_fallback() {
    let root = tempfile::tempdir().unwrap();
    let resolution = version::resolve(root.path(), FALLBACK);
    assert_eq!(resolution.version, FALLBACK);
    assert!(resolution.watched.is_empty());
}
