// ---
// relationships:
//   implements: command-line-interface
// ---
#[allow(dead_code)]
mod support;

use std::{path::Path, process::Output};

/// A template folder whose name starts with `-`, with answers for it.
fn template(root: &Path) -> std::path::PathBuf {
    let folder = root.join("-sample");
    std::fs::create_dir_all(folder.join("template")).unwrap();
    std::fs::write(folder.join("template/note.txt"), "{{ title }}\n").unwrap();
    std::fs::write(
        folder.join("template.yml"),
        "name: sample\ninterview:\n  - { id: title, type: text, prompt: Title? }\n",
    )
    .unwrap();
    std::fs::write(root.join("answers.json"), "{\"title\":\"Sample\"}").unwrap();
    folder
}

fn apply(cwd: &Path, args: &[&str]) -> Output {
    let isolation = tempfile::tempdir().unwrap();
    support::isolated_command(isolation.path())
        .current_dir(cwd)
        .arg("apply")
        .args(args)
        .output()
        .unwrap()
}

fn assert_written(output: &Output, target: &Path) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.join("note.txt")).unwrap(),
        "Sample\n"
    );
}

#[test]
fn template_and_path_after_double_dash() {
    let root = tempfile::tempdir().unwrap();
    template(root.path());
    let output = apply(
        root.path(),
        &["--answers", "answers.json", "--", "./-sample", "out"],
    );
    assert_written(&output, &root.path().join("out"));
}

#[test]
fn double_dash_before_answers_value_and_operands() {
    let root = tempfile::tempdir().unwrap();
    template(root.path());
    let output = apply(
        root.path(),
        &["-A", "answers.json", "--force", "--", "./-sample", "out"],
    );
    assert_written(&output, &root.path().join("out"));
}

#[test]
fn template_before_and_path_after_double_dash() {
    let root = tempfile::tempdir().unwrap();
    template(root.path());
    let output = apply(
        root.path(),
        &["./-sample", "--answers", "answers.json", "--", "out"],
    );
    assert_written(&output, &root.path().join("out"));
}

#[test]
fn only_operand_after_double_dash_is_the_path() {
    let root = tempfile::tempdir().unwrap();
    let output = apply(root.path(), &["--", "out"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("no staged interview; template folder is required")
    );
}

#[test]
fn template_whose_name_starts_with_a_dash() {
    let root = tempfile::tempdir().unwrap();
    template(root.path());
    let output = apply(
        root.path(),
        &["--answers", "answers.json", "--", "-sample", "out"],
    );
    // A bare name is looked up as an installed template, not as a flag.
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("template not found: -sample"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn no_operands_is_a_usage_error() {
    let root = tempfile::tempdir().unwrap();
    let output = apply(root.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("<PATH>"));
}

#[test]
fn three_operands_is_a_usage_error() {
    let root = tempfile::tempdir().unwrap();
    let output = apply(root.path(), &["a", "b", "c"]);
    assert_eq!(output.status.code(), Some(2));
}
