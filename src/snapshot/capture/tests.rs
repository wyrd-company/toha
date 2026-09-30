use super::*;
use crate::plan::{Content, Plan, PlannedFile};
use crate::snapshot::Project;
use crate::snapshot::record::CommitId;
use crate::staging::canonical_target;
use std::path::{Path, PathBuf};
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

fn whole(path: &str) -> PlannedFile {
    PlannedFile {
        path: TargetPath::parse(path).unwrap(),
        content: Content::Rendered(String::new()),
        source: PathBuf::new(),
    }
}

fn plan_of(files: Vec<PlannedFile>) -> Plan {
    Plan {
        files,
        edits: vec![],
        conflicts: vec![],
        hooks: vec![],
        before_apply: None,
        after_apply: None,
        result_seed: vec![],
    }
}

fn inputs(head_commit: &str) -> CaptureInputs {
    CaptureInputs {
        id: SnapshotId::from_parts(1, [7; 10]),
        template: "forge:catalog/receipt@stable".to_owned(),
        revision: Revision::Commit(
            CommitId::parse("8be0d41c2f0000000000000000000000000000a1").unwrap(),
        ),
        generated: FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
        created: Timestamp::parse("2026-09-30T04:12:00Z").unwrap(),
        project: ProjectPoint::new(
            CommitId::parse(head_commit).unwrap(),
            Some("main".to_owned()),
        ),
        submissions: vec![],
    }
}

fn snap_ref(id: &SnapshotId) -> String {
    format!("refs/toha/snapshots/{id}")
}

#[test]
fn captures_plan_targets_and_hook_changes_excluding_ignored_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    // HEAD holds app/whole.txt so an identical plan target proves behavior 9.
    write(&root.join("app/whole.txt"), b"keep\n");
    write(&root.join(".gitignore"), b"*.log\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    // Applied state on disk: an unchanged plan target, a hook-created file, and
    // an ignored file the hook also dropped.
    write(&root.join("app/whole.txt"), b"keep\n"); // identical to HEAD
    write(&root.join("app/generated.txt"), b"made by hook\n"); // hook, new
    write(&root.join("app/debug.log"), b"ignored\n"); // ignored, new

    let target = canonical_target(&root.join("app")).unwrap();
    let project = Project::open(&target).unwrap().unwrap();
    let plan = plan_of(vec![whole("whole.txt")]);
    let id = project.capture(&plan, inputs(&head)).unwrap();

    let listing = git(root, &["ls-tree", "-r", "--name-only", &snap_ref(&id)]);
    let files: Vec<&str> = listing.lines().collect();
    assert!(
        files.contains(&"files/whole.txt"),
        "identical plan target is captured (behavior 9)"
    );
    assert!(
        files.contains(&"files/generated.txt"),
        "hook file is captured (behavior 7)"
    );
    assert!(
        !files.contains(&"files/debug.log"),
        "ignored untracked file is left out (behavior 7)"
    );

    // snapshot.json records the origins.
    let json = git(root, &["show", &format!("{}:snapshot.json", snap_ref(&id))]);
    assert!(json.contains("\"whole.txt\""));
    assert!(json.contains("\"generated.txt\""));
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(doc["paths"]["whole.txt"]["origin"], "toha");
    assert_eq!(doc["paths"]["generated.txt"]["origin"], "hook");
}

#[test]
fn captures_symlink_executable_and_crlf_stored_form() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join(".gitattributes"), b"*.crlf text\n");
    write(&root.join("app/keep.txt"), b"x\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    // Applied plan targets: an executable and a CRLF text file.
    write(&root.join("app/script.sh"), b"#!/bin/sh\necho hi\n");
    std::fs::set_permissions(
        root.join("app/script.sh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    write(&root.join("app/data.crlf"), b"a\r\nb\r\n");
    // A hook-created symbolic link.
    std::os::unix::fs::symlink("keep.txt", root.join("app/link")).unwrap();

    let target = canonical_target(&root.join("app")).unwrap();
    let project = Project::open(&target).unwrap().unwrap();
    let plan = plan_of(vec![whole("script.sh"), whole("data.crlf")]);
    let id = project.capture(&plan, inputs(&head)).unwrap();

    // Modes: executable is 100755, the symlink is 120000.
    let ls = git(root, &["ls-tree", "-r", &snap_ref(&id)]);
    let mode_of = |name: &str| -> String {
        ls.lines()
            .find(|l| l.ends_with(name))
            .map(|l| l.split_whitespace().next().unwrap().to_owned())
            .unwrap_or_default()
    };
    assert_eq!(
        mode_of("files/script.sh"),
        "100755",
        "executable bit recorded"
    );
    assert_eq!(
        mode_of("files/link"),
        "120000",
        "symbolic link recorded as a link"
    );

    // The CRLF file is captured in its stored (LF) form.
    let stored = git(
        root,
        &["show", &format!("{}:files/data.crlf", snap_ref(&id))],
    );
    assert_eq!(stored, "a\nb", "text=auto CRLF captured as stored LF form");
}
