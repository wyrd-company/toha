// ---
// relationships:
//   implements: architecture
// ---
#[allow(dead_code)]
mod support;

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

fn template() -> String {
    support::folder_address(
        &Path::new("tests/fixtures/text-basic/template")
            .canonicalize()
            .unwrap(),
    )
}

fn apply_with(answers: &Path) -> Output {
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    support::isolated_command(isolation.path())
        .arg("apply")
        .arg(template())
        .arg(target.path())
        .arg("--answers")
        .arg(answers)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn answers_file_that_is_not_json_names_the_file_and_the_format() {
    let folder = tempfile::tempdir().unwrap();
    let answers = write(folder.path(), "answers.yml", "name: Item\n");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(
        stderr.contains(&format!(
            "{}: not a JSON answers document: expected ident at line 1 column 2",
            answers.display()
        )),
        "{stderr}"
    );
}

#[test]
fn missing_answers_file_names_the_file() {
    let folder = tempfile::tempdir().unwrap();
    let answers = folder.path().join("missing.json");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(
        stderr.contains(&format!(
            "{}: cannot read answers document:",
            answers.display()
        )),
        "{stderr}"
    );
}

#[test]
fn answers_file_that_is_not_an_object_names_the_file_and_the_shape() {
    let folder = tempfile::tempdir().unwrap();
    let answers = write(folder.path(), "answers.json", "[\"Item\"]");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(
        stderr.contains(&format!(
            "{}: not a JSON answers document: expected an object keyed by question id",
            answers.display()
        )),
        "{stderr}"
    );
}

#[test]
fn answers_from_standard_input_are_named_stdin() {
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let stage = support::isolated_command(isolation.path())
        .arg("stage")
        .arg(template())
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4), "{}", stderr(&stage));
    let mut child = support::isolated_command(isolation.path())
        .arg("continue")
        .arg(target.path())
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"name: Item\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("stdin: not a JSON answers document: expected ident at line 1 column 2"),
        "{stderr}"
    );
}
