// ---
// relationships:
//   implements: template-format
// ---
#[allow(dead_code)]
mod support;

use std::{fs, path::Path, process::Output};

fn apply(template: &Path, target: &Path, extra: &[&str]) -> Output {
    let isolation = tempfile::tempdir().unwrap();
    let answers = isolation.path().join("answers.json");
    fs::write(
        &answers,
        support::envelope_text(&support::formal_name(template), &serde_json::json!({})),
    )
    .unwrap();
    support::isolated_command(isolation.path())
        .arg("apply")
        .arg(template)
        .arg(target)
        .arg("--answers")
        .arg(answers)
        .args(extra)
        .output()
        .unwrap()
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/directories/template")
}

#[test]
fn creates_nested_and_repeated_directories_and_preserves_existing_contents() {
    let target = tempfile::tempdir().unwrap();
    fs::create_dir_all(target.path().join("groups/alpha")).unwrap();
    fs::write(target.path().join("groups/alpha/keep.txt"), "keep").unwrap();
    let result = apply(&fixture(), target.path(), &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    for path in ["empty/nested", "groups/alpha", "groups/beta"] {
        assert!(target.path().join(path).is_dir(), "{path}");
    }
    assert!(!target.path().join("skipped").exists());
    assert_eq!(
        fs::read_to_string(target.path().join("groups/alpha/keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn dry_run_lists_missing_directories_without_writing() {
    let target = tempfile::tempdir().unwrap();
    let result = apply(&fixture(), target.path(), &["--dry-run"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let document = support::first_document(&result.stdout);
    let plan = support::plan_text_from_document(&document);
    assert!(plan.contains("create empty/nested"), "{document}");
    assert!(fs::read_dir(target.path()).unwrap().next().is_none());
}

fn local_template(root: &Path, rules: &str) -> std::path::PathBuf {
    let template = root.join("template");
    fs::create_dir_all(template.join("template")).unwrap();
    fs::write(
        template.join("template.yml"),
        format!("name: sample\n{rules}\n"),
    )
    .unwrap();
    template
}

#[test]
fn rejects_file_collisions_even_with_force_before_any_write() {
    for collision in ["empty", "empty/nested"] {
        let target = tempfile::tempdir().unwrap();
        let path = target.path().join(collision);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "keep").unwrap();
        let result = apply(&fixture(), target.path(), &["--force"]);
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(path).unwrap(), "keep");
        assert!(!target.path().join("groups").exists());
    }
}

#[test]
fn rejects_unsafe_paths_and_planned_file_directory_collisions() {
    for path in ["../outside", ".git/objects", "/absolute"] {
        let root = tempfile::tempdir().unwrap();
        let template = local_template(root.path(), &format!("directories:\n  - path: '{path}'"));
        let target = tempfile::tempdir().unwrap();
        assert!(
            !apply(&template, target.path(), &[]).status.success(),
            "{path}"
        );
        assert!(fs::read_dir(target.path()).unwrap().next().is_none());
    }
    let root = tempfile::tempdir().unwrap();
    let template = local_template(root.path(), "directories:\n  - path: node/child");
    fs::write(template.join("template/node"), "file").unwrap();
    let target = tempfile::tempdir().unwrap();
    let result = apply(&template, target.path(), &[]);
    assert!(!result.status.success());
    assert!(fs::read_dir(target.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_directory_and_ancestor_without_writing_outside() {
    for path in ["empty", "empty/nested"] {
        let target = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = target.path().join(path);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let result = apply(&fixture(), target.path(), &["--force"]);
        assert!(!result.status.success());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(!target.path().join("groups").exists());
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn baseline_creates_directories_and_preview_does_not() {
    let root = tempfile::tempdir().unwrap();
    let template = local_template(root.path(), "directories:\n  - path: empty/nested");
    let target = tempfile::tempdir().unwrap();
    git(target.path(), &["init", "-q"]);
    fs::write(target.path().join("note.txt"), "seed").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "-qm", "seed"]);
    let result = apply(&template, target.path(), &["--baseline", "--dry-run"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(!target.path().join("empty").exists());
    let result = apply(&template, target.path(), &["--baseline"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(target.path().join("empty/nested").is_dir());
}

#[test]
fn apply_rechecks_a_directory_occupied_by_a_file_after_planning() {
    let folder = fixture();
    let template = toha::Template::load(&folder).unwrap();
    let target = tempfile::tempdir().unwrap();
    let target = toha::staging::canonical_target(target.path()).unwrap();
    let toha::Interview::Complete(completed) = toha::Interview::start(
        &template,
        toha::Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
            context: toha::context::InvocationContext::for_target(target.clone()),
        },
    )
    .unwrap() else {
        panic!("no interview questions")
    };
    let plan = toha::Plan::build(&template, &completed, &target).unwrap();
    fs::write(target.as_path().join("empty"), "occupied").unwrap();
    assert!(
        plan.apply(
            &target,
            toha::ApplyOptions {
                force: true,
                trusted: true
            },
            &toha::hook::RecordingRunner::new()
        )
        .is_err()
    );
    assert!(!target.as_path().join("note.txt").exists());
}

#[test]
fn update_recreates_a_missing_directory_even_when_files_are_current() {
    let root = tempfile::tempdir().unwrap();
    let template = local_template(root.path(), "directories:\n  - path: empty/nested");
    fs::write(template.join("template/note.txt"), "sample").unwrap();
    let address = support::versioned_template(&template);
    let iso = tempfile::tempdir().unwrap();
    let answers = iso.path().join("answers.json");
    fs::write(
        &answers,
        support::envelope_text(&address, &serde_json::json!({})),
    )
    .unwrap();
    let target = tempfile::tempdir().unwrap();
    git(target.path(), &["init", "-q"]);
    fs::write(target.path().join("seed.txt"), "seed").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "-qm", "seed"]);
    let run = |args: &[&str]| {
        let result = support::isolated_command(iso.path())
            .arg("apply")
            .arg(&address)
            .arg(target.path())
            .arg("--answers")
            .arg(&answers)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        support::first_document(&result.stdout)
    };
    run(&["--baseline"]);
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "-qm", "baseline"]);
    fs::remove_dir_all(target.path().join("empty")).unwrap();
    let preview = run(&["--update", "--dry-run"]);
    assert_eq!(preview["status"], "planned", "{preview}");
    assert!(!target.path().join("empty").exists());
    let applied = run(&["--update"]);
    assert_eq!(applied["status"], "applied", "{applied}");
    assert!(target.path().join("empty/nested").is_dir());
    assert_eq!(
        fs::read_to_string(target.path().join("note.txt")).unwrap(),
        "sample"
    );
}

fn git_bytes(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
fn reserved_record_directories_refuse_before_any_transaction_writes() {
    for record in [".toha.json", "metadata/applied.json", "app/applied.json"] {
        for subtarget in ["", "app"] {
            let relative = if subtarget == "app" && record.starts_with("app/") {
                &record[4..]
            } else if subtarget == "app" {
                continue;
            } else {
                record
            };
            for suffix in ["", "/child"] {
                for flags in [
                    vec![],
                    vec!["--dry-run"],
                    vec!["--baseline"],
                    vec!["--baseline", "--dry-run"],
                ] {
                    let root = tempfile::tempdir().unwrap();
                    let template = local_template(
                        root.path(),
                        &format!("directories:\n  - path: {relative}{suffix}"),
                    );
                    fs::write(template.join("template/note.txt"), "sample").unwrap();
                    let target = tempfile::tempdir().unwrap();
                    git(target.path(), &["init", "-q"]);
                    fs::write(target.path().join("seed.txt"), "seed").unwrap();
                    git(target.path(), &["add", "."]);
                    git(target.path(), &["commit", "-qm", "seed"]);
                    let destination = target.path().join(subtarget);
                    fs::create_dir_all(&destination).unwrap();
                    let index = fs::read(target.path().join(".git/index")).unwrap();
                    let refs = git_bytes(target.path(), &["show-ref"]);
                    let answers = root.path().join("answers.json");
                    fs::write(
                        &answers,
                        support::envelope_text(
                            &support::formal_name(&template),
                            &serde_json::json!({}),
                        ),
                    )
                    .unwrap();
                    let result = support::isolated_command(root.path())
                        .env("TOHA_RECORD", record)
                        .arg("apply")
                        .arg(&template)
                        .arg(&destination)
                        .arg("--answers")
                        .arg(answers)
                        .args(&flags)
                        .output()
                        .unwrap();
                    assert_eq!(
                        result.status.code(),
                        Some(1),
                        "{record} {subtarget} {suffix} {flags:?}: {}",
                        String::from_utf8_lossy(&result.stdout)
                    );
                    assert!(
                        format!(
                            "{}{}",
                            String::from_utf8_lossy(&result.stdout),
                            String::from_utf8_lossy(&result.stderr)
                        )
                        .contains("reserved"),
                        "{result:?}"
                    );
                    assert_eq!(fs::read(target.path().join(".git/index")).unwrap(), index);
                    assert_eq!(git_bytes(target.path(), &["show-ref"]), refs);
                    assert!(!target.path().join(record).exists());
                    assert!(!destination.join("note.txt").exists());
                }
            }
        }
    }
}

#[test]
fn existing_directories_are_omitted_from_preview_and_apply_reports() {
    let root = tempfile::tempdir().unwrap();
    let template = local_template(
        root.path(),
        "directories:\n  - path: existing\n  - path: new",
    );
    let target = tempfile::tempdir().unwrap();
    fs::create_dir(target.path().join("existing")).unwrap();
    for flags in [vec!["--dry-run"], vec![]] {
        let result = apply(&template, target.path(), &flags);
        assert!(result.status.success());
        let doc = support::first_document(&result.stdout);
        let text = doc.to_string();
        assert!(!text.contains("existing"), "{doc}");
        assert!(text.contains("new"), "{doc}");
    }
}

#[cfg(unix)]
#[test]
fn directory_permission_failure_preserves_baseline_and_update_state() {
    use std::os::unix::fs::PermissionsExt;
    for update in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let template = local_template(
            root.path(),
            "directories:\n  - path: created/nested\n  - path: locked/new",
        );
        fs::write(template.join("template/note.txt"), "sample").unwrap();
        let address = support::versioned_template(&template);
        let iso = tempfile::tempdir().unwrap();
        let answers = iso.path().join("answers.json");
        fs::write(
            &answers,
            support::envelope_text(&address, &serde_json::json!({})),
        )
        .unwrap();
        let target = tempfile::tempdir().unwrap();
        git(target.path(), &["init", "-q"]);
        fs::create_dir(target.path().join("locked")).unwrap();
        fs::write(target.path().join("locked/keep.txt"), "keep").unwrap();
        git(target.path(), &["add", "."]);
        git(target.path(), &["commit", "-qm", "seed"]);
        let run = |flag: &str| {
            support::isolated_command(iso.path())
                .arg("apply")
                .arg(&address)
                .arg(target.path())
                .arg("--answers")
                .arg(&answers)
                .arg(flag)
                .output()
                .unwrap()
        };
        if update {
            let initial = run("--baseline");
            assert!(
                initial.status.success(),
                "{}",
                String::from_utf8_lossy(&initial.stdout)
            );
            git(target.path(), &["commit", "-qam", "baseline"]);
            fs::remove_dir_all(target.path().join("created")).unwrap();
            fs::remove_dir(target.path().join("locked/new")).unwrap();
        }
        let index = fs::read(target.path().join(".git/index")).unwrap();
        let refs = git_bytes(target.path(), &["show-ref"]);
        let record = fs::read(target.path().join(".toha.json")).ok();
        let note = fs::read(target.path().join("note.txt")).ok();
        fs::set_permissions(
            target.path().join("locked"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        // Prove this host enforces the fixture's real filesystem permission failure.
        assert!(fs::create_dir(target.path().join("locked/probe")).is_err());
        let result = run(if update { "--update" } else { "--baseline" });
        fs::set_permissions(
            target.path().join("locked"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(
            format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            )
            .contains("Permission denied"),
            "{result:?}"
        );
        assert_eq!(fs::read(target.path().join(".git/index")).unwrap(), index);
        assert_eq!(git_bytes(target.path(), &["show-ref"]), refs);
        assert_eq!(fs::read(target.path().join(".toha.json")).ok(), record);
        assert_eq!(fs::read(target.path().join("note.txt")).ok(), note);
        assert_eq!(
            fs::read_to_string(target.path().join("locked/keep.txt")).unwrap(),
            "keep"
        );
        assert!(!target.path().join("created").exists());
        assert!(!target.path().join("locked/new").exists());
        assert!(!target.path().join(".git/index.lock").exists());
    }
}

#[test]
fn library_report_omits_existing_directories() {
    let root = tempfile::tempdir().unwrap();
    let folder = local_template(
        root.path(),
        "directories:\n  - path: existing\n  - path: new",
    );
    let template = toha::Template::load(&folder).unwrap();
    let target = tempfile::tempdir().unwrap();
    fs::create_dir(target.path().join("existing")).unwrap();
    let target = toha::staging::canonical_target(target.path()).unwrap();
    let toha::Interview::Complete(completed) = toha::Interview::start(
        &template,
        toha::Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
            context: toha::context::InvocationContext::for_target(target.clone()),
        },
    )
    .unwrap() else {
        panic!("no questions")
    };
    let plan = toha::Plan::build(&template, &completed, &target).unwrap();
    let toha::Applied::Written { files, .. } = plan
        .apply(
            &target,
            toha::ApplyOptions {
                force: false,
                trusted: true,
            },
            &toha::hook::RecordingRunner::new(),
        )
        .unwrap()
    else {
        panic!("trusted")
    };
    assert_eq!(
        files.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["new"]
    );
}
