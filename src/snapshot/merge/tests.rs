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
