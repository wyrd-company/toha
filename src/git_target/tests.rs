use super::*;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A `file://` URL for `path`, which is not canonicalized: a canonical Windows
/// path carries a verbatim prefix that a git URL cannot hold.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// A bare repository with no reference.
fn empty_remote(dir: &Path) -> String {
    git(dir, &["init", "--quiet", "--bare", "remote.git"]);
    file_url(&dir.join("remote.git"))
}

/// A bare repository with one commit on `main`.
fn remote_with_history(dir: &Path) -> String {
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(work.join("README.md"), "seed\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "seed"]);
    git(
        dir,
        &["clone", "--quiet", "--bare", "work", "remote-history.git"],
    );
    file_url(&dir.join("remote-history.git"))
}

fn target(dir: &Path) -> CanonicalTarget {
    crate::staging::canonical_target(dir).unwrap()
}

#[test]
fn default_branch_replaces_unset_and_master_with_main() {
    assert_eq!(default_branch(None), "main");
    assert_eq!(default_branch(Some("")), "main");
    assert_eq!(default_branch(Some("master")), "main");
    assert_eq!(default_branch(Some("main")), "main");
    assert_eq!(default_branch(Some("trunk")), "trunk");
}

#[test]
fn probe_reports_an_empty_remote() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(probe(&empty_remote(dir.path())), RemoteState::Empty);
}

#[test]
fn probe_reports_a_remote_with_history() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        probe(&remote_with_history(dir.path())),
        RemoteState::HasHistory
    );
}

#[test]
fn probe_reports_an_unreachable_remote_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let url = file_url(dir.path()).replace("file://", "https://user:secret@127.0.0.1:9/");
    match probe(&url) {
        RemoteState::Unreachable(message) => {
            assert!(!message.contains("secret"), "{message}");
            assert!(!message.contains("user:"), "{message}");
        }
        other => panic!("expected unreachable, got {other:?}"),
    }
}

#[test]
fn preflight_stops_for_a_remote_with_history() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let planned = PlannedGit {
        remote: Some("https://user:secret@example.invalid/sample.git".into()),
    };
    let error =
        preflight_with(&planned, &target(&project), &|_| RemoteState::HasHistory).unwrap_err();
    let message = error.to_string();
    assert!(matches!(error, GitTargetError::RemoteHasHistory { .. }));
    assert!(message.contains("clone it first"), "{message}");
    assert!(!message.contains("secret"), "{message}");
}

#[test]
fn preflight_warns_and_continues_for_an_unreachable_remote() {
    let dir = tempfile::tempdir().unwrap();
    let planned = PlannedGit {
        remote: Some("https://example.invalid/sample.git".into()),
    };
    match preflight_with(&planned, &target(dir.path()), &|_| {
        RemoteState::Unreachable("refused".into())
    })
    .unwrap()
    {
        Preflight::Initialize {
            remote,
            warning: Some(warning),
        } => {
            assert_eq!(
                remote.as_deref(),
                Some("https://example.invalid/sample.git")
            );
            assert!(warning.starts_with("warning: "), "{warning}");
            assert!(warning.ends_with("refused"), "{warning}");
        }
        other => panic!("expected a warning, got {other:?}"),
    }
}

#[test]
fn preflight_initializes_without_probing_when_no_remote_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let planned = PlannedGit { remote: None };
    let preflight = preflight_with(&planned, &target(dir.path()), &|_| panic!("probed")).unwrap();
    assert!(matches!(
        preflight,
        Preflight::Initialize {
            remote: None,
            warning: None
        }
    ));
}

#[test]
fn preflight_leaves_a_target_inside_a_repository_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "--quiet"]);
    // A subdirectory the apply has not created yet still resolves its repository.
    let nested = dir.path().join("not-yet/created");
    let planned = PlannedGit {
        remote: Some("https://example.invalid/sample.git".into()),
    };
    let preflight = preflight_with(&planned, &target(&nested), &|_| panic!("probed")).unwrap();
    assert!(matches!(preflight, Preflight::Existing));
}

#[test]
fn initialize_names_a_branch_and_adds_origin() {
    let dir = tempfile::tempdir().unwrap();
    let url = "https://example.invalid/sample.git";
    let branch = initialize(dir.path(), Some(url)).unwrap();
    let repo = gix::open(dir.path()).unwrap();
    let head = repo.head_name().unwrap().unwrap();
    assert_eq!(head.as_bstr(), format!("refs/heads/{branch}").as_str());
    let config = repo.config_snapshot();
    assert_eq!(config.string("remote.origin.url").unwrap().to_string(), url);
    assert_eq!(
        config.string("remote.origin.fetch").unwrap().to_string(),
        "+refs/heads/*:refs/remotes/origin/*"
    );
}

#[test]
fn initialize_without_a_remote_adds_none() {
    let dir = tempfile::tempdir().unwrap();
    initialize(dir.path(), None).unwrap();
    let repo = gix::open(dir.path()).unwrap();
    assert!(repo.find_remote("origin").is_err());
}

#[test]
fn report_lines_put_the_warning_first_and_hide_credentials() {
    let report = GitReport {
        setup: GitSetup::Initialized {
            branch: "main".into(),
            remote: Some("https://user:secret@example.invalid/sample.git".into()),
        },
        warning: Some("warning: sample".into()),
    };
    let lines = report.lines();
    assert_eq!(lines[0], "warning: sample");
    assert_eq!(
        lines[1],
        "Initialized git repository on branch main with remote origin https://example.invalid/sample.git"
    );
    assert_eq!(
        PlannedGit {
            remote: Some("https://user:secret@example.invalid/sample.git".into())
        }
        .preview_line(),
        "git init (remote origin https://example.invalid/sample.git)"
    );
}

#[test]
fn a_relative_local_remote_is_probed_from_the_target() {
    let target = Path::new("/sample/work/project");
    assert_eq!(
        Path::new(&probe_location("../history.git", target)),
        Path::new("/sample/work/history.git")
    );
    assert_eq!(
        Path::new(&probe_location("./remotes/sample.git", target)),
        target.join("remotes/sample.git")
    );
    // A `..` after a named step is left for the file system to resolve.
    assert_eq!(
        Path::new(&probe_location("../alias/../history.git", target)),
        Path::new("/sample/work/alias/../history.git")
    );
    for url in [
        "https://example.invalid/sample.git",
        "git@example.invalid:sample.git",
        "file:///sample/history.git",
        "/sample/history.git",
    ] {
        assert_eq!(probe_location(url, target), url);
    }
}

#[test]
fn a_verbatim_drive_path_is_probed_in_its_plain_form() {
    assert_eq!(
        without_verbatim(Path::new(r"\\?\C:\sample\history.git")),
        Some(std::path::PathBuf::from(r"C:\sample\history.git"))
    );
    assert_eq!(without_verbatim(Path::new(r"\\?\UNC\host\share")), None);
    assert_eq!(without_verbatim(Path::new("/sample/history.git")), None);
}
