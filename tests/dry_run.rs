// ---
// relationships:
//   implements: architecture
// ---
#[allow(dead_code)]
mod support;

use std::{path::Path, process::Output};

fn template(dir: &Path, hooks: &str) -> String {
    std::fs::create_dir(dir.join("template")).unwrap();
    std::fs::write(dir.join("template/note.txt"), "{{ title }}\n").unwrap();
    std::fs::write(
        dir.join("template.yml"),
        format!(
            "name: sample\n\
             interview:\n  \
             - {{ id: title, type: text, prompt: Title? }}\n  \
             - message: 'Node {{{{ title }}}}'\n\
             messages:\n  \
             before-apply: 'Before {{{{ title }}}}'\n  \
             after-apply: 'After {{{{ title }}}}'\n\
             {hooks}"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("answers.json"), "{\"title\":\"Sample\"}").unwrap();
    support::folder_address(dir)
}

fn apply(folder: &Path, target: &Path, extra: &[&str]) -> Output {
    let isolation = tempfile::tempdir().unwrap();
    support::isolated_command(isolation.path())
        .arg("apply")
        .arg(support::folder_address(folder))
        .arg(target)
        .arg("--answers")
        .arg(folder.join("answers.json"))
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn dry_run_shows_interview_and_before_apply_messages_but_not_after_apply() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &["--dry-run"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines,
        ["Node Sample", "Before Sample", "create note.txt"],
        "{stdout}"
    );
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[test]
fn untrusted_hooks_dry_run_does_not_show_after_apply() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "hooks:\n  - run: [ tool, call ]\n");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(3), "{stdout}");
    assert!(stdout.contains("Before Sample"), "{stdout}");
    assert!(stdout.contains("create note.txt"), "{stdout}");
    assert!(!stdout.contains("After Sample"), "{stdout}");
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[test]
fn apply_shows_after_apply_after_writing() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert_eq!(stdout.lines().last(), Some("After Sample"), "{stdout}");
}
