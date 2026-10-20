// ---
// relationships:
//   implements: architecture
// ---
//! The non-interactive snapshot routes through the binary: `init`, `snapshots
//! list`, and `snapshots clean --remove`. Each drives a real git repository so
//! the driver-stripping open and the ref writes run end to end.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

/// Run `git ARGS...` inside `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A fresh repository with one commit and an `origin` remote.
fn repo_with_origin(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    git(
        dir,
        &["remote", "add", "origin", "https://example.invalid/x.git"],
    );
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

#[test]
fn init_adds_fetch_then_is_idempotent() {
    let isolation = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    repo_with_origin(target.path());

    let mut command = support::isolated_command(isolation.path());
    command.arg("init").arg(target.path());
    let output = command.output().unwrap();
    assert!(output.status.success(), "init failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("added fetch"), "unexpected: {stdout}");

    // The refspec now lives in the repository config.
    let config = std::fs::read_to_string(target.path().join(".git/config")).unwrap();
    assert!(
        config.contains("refs/toha/snapshots/*:refs/toha/snapshots/*"),
        "config missing fetch refspec: {config}"
    );

    // A second init reports the refspec is already present.
    let mut again = support::isolated_command(isolation.path());
    again.arg("init").arg(target.path());
    let output = again.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already fetches"), "unexpected: {stdout}");
}

#[test]
fn init_refuses_without_a_remote() {
    let isolation = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    git(target.path(), &["init", "--quiet"]);

    let mut command = support::isolated_command(isolation.path());
    command.arg("init").arg(target.path());
    let output = command.output().unwrap();
    assert!(!output.status.success(), "init should refuse");
}

#[test]
fn init_refuses_outside_git() {
    let isolation = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    let mut command = support::isolated_command(isolation.path());
    command.arg("init").arg(target.path());
    let output = command.output().unwrap();
    assert!(!output.status.success(), "init should refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("git"), "unexpected: {stderr}");
}

#[test]
fn list_reports_no_snapshots_as_text_and_json() {
    let isolation = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    repo_with_origin(target.path());

    let mut text = support::isolated_command(isolation.path());
    text.arg("snapshots").arg("list").arg(target.path());
    let output = text.output().unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no snapshots"),
        "unexpected: {output:?}"
    );

    let mut json = support::isolated_command(isolation.path());
    json.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let output = json.output().unwrap();
    assert!(output.status.success());
    let document = support::first_document(&output.stdout);
    assert_eq!(document["snapshots"].as_array().unwrap().len(), 0);
}

#[test]
fn clean_remove_reports_not_found_and_push_commands() {
    let isolation = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    repo_with_origin(target.path());

    // A well-formed id that names no snapshot is reported as not found, never
    // as an error; nothing is pushed on the caller's behalf.
    let request = isolation.path().join("remove.json");
    std::fs::write(&request, r#"{ "remove": ["00000000000000000000000000"] }"#).unwrap();

    let mut command = support::isolated_command(isolation.path());
    command
        .arg("snapshots")
        .arg("clean")
        .arg(target.path())
        .arg("--remove")
        .arg(&request);
    let output = command.output().unwrap();
    assert!(output.status.success(), "clean failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["removed"].as_array().unwrap().len(), 0);
    assert_eq!(
        document["not_found"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["00000000000000000000000000"]
    );
    // No push is performed; the caller is handed the exact commands instead.
    assert_eq!(document["push_commands"].as_array().unwrap().len(), 0);
}

/// The snapshot schema, with `$ref` set to validate against `def`.

#[test]
fn legacy_latest_id_and_prefix_selectors_copy_answers_and_survive_staging() {
    use toha::snapshot::{
        CommitId, FrozenNow, ProjectPoint, RepoPath, Revision, SnapshotDoc, SnapshotId, Timestamp,
    };
    let isolation = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    repo_with_origin(target.path());
    std::fs::create_dir(source.path().join("template")).unwrap();
    std::fs::write(source.path().join("template.yml"), "name: example\ninterview:\n  - id: label\n    type: text\n    prompt: Label\n    required: true\n").unwrap();
    std::fs::write(source.path().join("template/result.txt"), "{{ label }}\n").unwrap();
    let address = support::formal_name(source.path());
    let repo = gix::open(target.path()).unwrap();
    let id: SnapshotId = "01J9Z8XR7Q0V8BCFKJ5Y03CDAB".parse().unwrap();
    let doc = SnapshotDoc::new(
        id,
        address.clone(),
        Revision::Unversioned,
        RepoPath::root(),
        Timestamp::now(),
        FrozenNow::parse("2026-01-02T03:04:05+00:00[UTC]").unwrap(),
        ProjectPoint::new(
            CommitId::parse(&repo.head_id().unwrap().to_string()).unwrap(),
            None,
        ),
        None,
        vec![indexmap::IndexMap::from([(
            "label".into(),
            serde_json::json!("seeded"),
        )])],
        vec![],
    );
    let blob = repo.write_blob(doc.to_json_bytes()).unwrap().detach();
    let mut tree = repo
        .edit_tree(gix::ObjectId::empty_tree(repo.object_hash()))
        .unwrap();
    tree.upsert("snapshot.json", gix::objs::tree::EntryKind::Blob, blob)
        .unwrap();
    let tree = tree.write().unwrap().detach();
    let signature = gix::actor::Signature {
        name: "Example User".into(),
        email: "user@example.invalid".into(),
        time: gix::date::Time::now_utc(),
    };
    let mut first = gix::date::parse::TimeBuf::default();
    let mut second = gix::date::parse::TimeBuf::default();
    repo.commit_as(
        signature.to_ref(&mut first),
        signature.to_ref(&mut second),
        format!("refs/toha/snapshots/{id}").as_str(),
        "example",
        tree,
        Vec::<gix::ObjectId>::new(),
    )
    .unwrap();
    let out = support::isolated_command(isolation.path())
        .args(["snapshots", "list"])
        .arg(target.path())
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains(&id.to_string()));
    for (index, selector) in ["latest", &id.to_string(), "01J9Z8"]
        .into_iter()
        .enumerate()
    {
        let path = target.path().join(format!("generated-{index}"));
        let out = support::isolated_command(isolation.path())
            .arg("stage")
            .arg(&address)
            .arg(&path)
            .args(["--async", "--like", selector])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(4), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("seeded"),
            "{out:?}"
        );
        let answers = isolation.path().join("accepted.json");
        std::fs::write(
            &answers,
            serde_json::json!({"template":address,"answers":{}}).to_string(),
        )
        .unwrap();
        let out = support::isolated_command(isolation.path())
            .arg("continue")
            .arg(&path)
            .arg(&answers)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let out = support::isolated_command(isolation.path())
            .arg("apply")
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(
            std::fs::read_to_string(path.join("result.txt")).unwrap(),
            "seeded\n"
        );
    }
    let remove = isolation.path().join("remove.json");
    std::fs::write(
        &remove,
        serde_json::json!({"remove":[id.to_string()]}).to_string(),
    )
    .unwrap();
    let out = support::isolated_command(isolation.path())
        .args(["snapshots", "clean"])
        .arg(target.path())
        .arg("--remove")
        .arg(&remove)
        .arg("--force")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        repo.find_reference(format!("refs/toha/snapshots/{id}").as_str())
            .is_err()
    );
}
