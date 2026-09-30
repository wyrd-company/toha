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
    let id = project.capture(root, &plan, None, inputs(&head)).unwrap();

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
    let id = project.capture(root, &plan, None, inputs(&head)).unwrap();

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

// --------------------------------------------------------------------------
// Base-aware capture: carry-forward (8) and retraction (12).
// --------------------------------------------------------------------------

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

/// Build a base snapshot ref with explicit per-path origin JSON. `files` is
/// (target-relative path, bytes, origin-json-fragment).
fn make_base(root: &Path, id: &str, target: &str, files: &[(&str, &[u8], &str)]) {
    let index = root.join(format!(".idx-{id}"));
    let _ = std::fs::remove_file(&index);
    let gi = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
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
        .map(|(p, _, origin)| format!("    \"{p}\": {origin}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let json = format!(
        "{{\n  \"snapshot\": 1,\n  \"id\": \"{id}\",\n  \"template\": \"forge:catalog/receipt@stable\",\n  \"source\": \"forge:catalog/receipt\",\n  \"commit\": \"8be0d41c2f0000000000000000000000000000a1\",\n  \"target\": \"{target}\",\n  \"created\": \"2026-09-30T04:12:00Z\",\n  \"generated\": \"2026-03-14T09:26:53+00:00[UTC]\",\n  \"project\": {{ \"commit\": \"a41c0de00000000000000000000000000000beef\", \"branch\": \"main\" }},\n  \"built_from\": null,\n  \"submissions\": [],\n  \"paths\": {{\n{paths_json}\n  }}\n}}"
    );
    let json_oid = hash_object(root, json.as_bytes());
    gi(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{json_oid},snapshot.json"),
    ]);
    for (p, bytes, _) in files {
        let oid = hash_object(root, bytes);
        gi(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{oid},files/{p}"),
        ]);
    }
    let tree = gi(&["write-tree"]);
    let commit = git(root, &["commit-tree", &tree, "-m", "snapshot"]);
    git(
        root,
        &["update-ref", &format!("refs/toha/snapshots/{id}"), &commit],
    );
    let _ = std::fs::remove_file(&index);
}

const BASE_ID: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6E";

fn find_base(project: &Project) -> crate::snapshot::Snapshot {
    project.find(BASE_ID).unwrap()
}

fn open(root: &Path, sub: &str) -> Project {
    let target = canonical_target(&root.join(sub)).unwrap();
    Project::open(&target).unwrap().unwrap()
}

#[test]
fn a_hook_file_unchanged_by_the_new_version_is_carried_forward() {
    // Behavior 8: a base hook file the new apply neither targets nor changes is
    // carried forward from the base, not deleted.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(&root.join("app/gen.txt"), b"hook output\n");
    write(&root.join("app/main.txt"), b"v1\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    // Base recorded gen.txt as a hook file and main.txt as a whole file.
    make_base(
        root,
        BASE_ID,
        "app",
        &[
            ("gen.txt", b"hook output\n", "{ \"origin\": \"hook\" }"),
            ("main.txt", b"v1\n", "{ \"origin\": \"toha\" }"),
        ],
    );

    // The new plan produces main.txt only; gen.txt is unchanged on disk.
    let project = open(root, "app");
    let base = find_base(&project);
    let plan = plan_of(vec![whole("main.txt")]);
    let id = project
        .capture(root, &plan, Some(&base), inputs(&head))
        .unwrap();

    let listing = git(root, &["ls-tree", "-r", "--name-only", &snap_ref(&id)]);
    assert!(
        listing.contains("files/gen.txt"),
        "the hook file is carried forward, not deleted"
    );
    let json: serde_json::Value = serde_json::from_str(&git(
        root,
        &["show", &format!("{}:snapshot.json", snap_ref(&id))],
    ))
    .unwrap();
    assert_eq!(
        json["paths"]["gen.txt"]["origin"], "hook",
        "carried forward with its hook origin"
    );
    assert_eq!(json["built_from"], BASE_ID, "records its base");
    // Its content is the base's.
    assert_eq!(
        git(root, &["show", &format!("{}:files/gen.txt", snap_ref(&id))]),
        "hook output"
    );
}

#[test]
fn a_dropped_region_injection_is_retracted_from_the_captured_file() {
    // Behavior 12: an injection the new version drops is retracted; the region's
    // markers and body are removed and the operator's surrounding content stays.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);

    // Build a file with a managed region via the injection resolver.
    let edit = crate::inject::PlannedRegionEdit {
        path: TargetPath::parse("conf.txt").unwrap(),
        region: crate::inject::RegionKey::parse("features").unwrap(),
        body: "FEATURE=on\n".to_owned(),
        marker: crate::inject::MarkerStyle::line("#"),
        anchor: None,
        create: true,
        source: None,
    };
    let injected = match crate::inject::resolve_region_edit(Some(b"top\nbottom\n"), &edit).unwrap()
    {
        crate::inject::EditResolution::Write(bytes) => bytes,
        other => panic!("expected write: {other:?}"),
    };
    write(&root.join("app/conf.txt"), &injected);
    write(&root.join("app/main.txt"), b"v1\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "operator"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    // Base owned the `features` region of conf.txt; new plan drops conf.txt.
    make_base(
        root,
        BASE_ID,
        "app",
        &[
            (
                "conf.txt",
                &injected,
                "{ \"origin\": \"edit\", \"regions\": [\"features\"], \"values\": [] }",
            ),
            ("main.txt", b"v1\n", "{ \"origin\": \"toha\" }"),
        ],
    );

    let project = open(root, "app");
    let base = find_base(&project);
    let plan = plan_of(vec![whole("main.txt")]); // conf.txt not produced
    // The candidate-tree builder retracts uncovered base ownership before capture.
    let plan_paths = super::group_plan(&plan);
    super::retract_uncovered(root, &RepoPath::parse("app").unwrap(), &plan_paths, &base).unwrap();
    let id = project
        .capture(root, &plan, Some(&base), inputs(&head))
        .unwrap();

    // conf.txt is captured with the region removed and the operator's lines kept.
    let captured = git(
        root,
        &["show", &format!("{}:files/conf.txt", snap_ref(&id))],
    );
    assert!(
        !captured.contains("toha:region"),
        "region markers removed: {captured:?}"
    );
    assert!(
        captured.contains("top") && captured.contains("bottom"),
        "operator lines kept: {captured:?}"
    );
    let json: serde_json::Value = serde_json::from_str(&git(
        root,
        &["show", &format!("{}:snapshot.json", snap_ref(&id))],
    ))
    .unwrap();
    assert_eq!(
        json["paths"]["conf.txt"]["origin"], "edit",
        "captured as an edit that now owns nothing"
    );
}
