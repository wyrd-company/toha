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

fn formal() -> String {
    support::formal_name(Path::new("tests/fixtures/text-basic/template"))
}

/// Runs the scripted route `apply TEMPLATE PATH --answers FILE`.
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

/// The scripted route's decoded diagnostic: the `message`/`errors` of the JSON
/// result document on standard output.
fn message(output: &Output) -> String {
    support::diagnostic_text(&support::first_document(&output.stdout))
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn answers_file_that_is_not_json_is_a_document_error() {
    let folder = tempfile::tempdir().unwrap();
    let answers = write(folder.path(), "answers.yml", "name: Item\n");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "document");
    assert!(
        message(&output).contains("invalid JSON: expected ident at line 1 column 2"),
        "{}",
        message(&output)
    );
}

#[test]
fn missing_answers_file_names_the_file_as_input() {
    let folder = tempfile::tempdir().unwrap();
    let answers = folder.path().join("missing.json");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["kind"], "input");
    assert!(
        message(&output).contains(&format!(
            "{}: cannot read answers document:",
            answers.display()
        )),
        "{}",
        message(&output)
    );
}

#[test]
fn answers_document_that_is_not_an_object_is_a_document_error() {
    let folder = tempfile::tempdir().unwrap();
    let answers = write(folder.path(), "answers.json", "[\"Item\"]");
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["kind"], "document");
    assert!(
        message(&output).contains("expected a JSON object with \"template\" and \"answers\""),
        "{}",
        message(&output)
    );
}

#[test]
fn standard_input_that_is_not_json_is_a_document_error_on_stderr() {
    // `continue PATH -` is an agent route; its faults are on standard error.
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
    assert!(
        stderr(&output).contains("invalid JSON: expected ident at line 1 column 2"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn envelope_answers_with_an_invalid_question_id_is_a_document_error() {
    let folder = tempfile::tempdir().unwrap();
    let answers = write(
        folder.path(),
        "answers.json",
        &format!(
            "{{\"template\": {:?}, \"answers\": {{\"Bad Key\": \"Item\"}}}}",
            formal()
        ),
    );
    let output = apply_with(&answers);
    assert_eq!(output.status.code(), Some(1));
    let document = support::first_document(&output.stdout);
    assert_eq!(document["kind"], "document");
    assert!(message(&output).contains("Bad Key"), "{}", message(&output));
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
