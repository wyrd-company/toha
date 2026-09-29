// ---
// relationships:
//   implements: command-line-interface
// ---
//! Refusals of valid requests made at the wrong time or in the wrong form name
//! the commands that do what the caller meant.
#[allow(dead_code)]
mod support;
use std::{
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

struct Case {
    state: tempfile::TempDir,
    target: tempfile::TempDir,
}
impl Case {
    fn new() -> Self {
        Self {
            state: tempfile::tempdir().unwrap(),
            target: tempfile::tempdir().unwrap(),
        }
    }
    fn target(&self) -> &str {
        self.target.path().to_str().unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        support::isolated_command(self.state.path())
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    /// Writes an identity-bearing envelope around a bare answers map for the
    /// scripted route, and returns its path.
    fn envelope(&self, formal: &str, answers: &str) -> String {
        let inner: serde_json::Value = serde_json::from_str(answers).unwrap();
        let document = serde_json::json!({ "template": formal, "answers": inner });
        let path = self.state.path().join("answers.json");
        std::fs::write(&path, document.to_string()).unwrap();
        path.to_str().unwrap().to_string()
    }
}

fn fixture_template(name: &str) -> PathBuf {
    Path::new("tests/fixtures")
        .join(name)
        .join("template")
        .canonicalize()
        .unwrap()
}
fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn untrusted_hooks_of_a_folder_report_planned_untrusted() {
    // The scripted route reports untrusted hooks structurally: a `planned`
    // document with `trusted: false` and exit 3.
    let case = Case::new();
    let template = support::folder_address(&fixture_template("hooks-untrusted"));
    let formal = support::formal_name(&fixture_template("hooks-untrusted"));
    let answers = case.envelope(&formal, "{}");
    let output = case.run(&["apply", "--answers", &answers, &template, case.target()]);
    assert_code(&output, 3);
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned");
    assert_eq!(document["trusted"], false);
}

#[test]
fn untrusted_hooks_of_an_installed_template_report_planned_untrusted() {
    let case = Case::new();
    let template = support::folder_address(&fixture_template("hooks-untrusted"));
    let formal = support::formal_name(&fixture_template("hooks-untrusted"));
    assert_code(&case.run(&["templates", "add", &template]), 0);
    let answers = case.envelope(&formal, "{}");
    let output = case.run(&[
        "apply",
        "--answers",
        &answers,
        "hooks-untrusted",
        case.target(),
    ]);
    assert_code(&output, 3);
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned");
    assert_eq!(document["trusted"], false);
}

#[test]
fn conflicting_files_are_a_conflict_error() {
    let case = Case::new();
    support::copy_tree(
        Path::new("tests/fixtures/conflict/existing"),
        case.target.path(),
    );
    let template = support::folder_address(&fixture_template("conflict"));
    let formal = support::formal_name(&fixture_template("conflict"));
    let answers = case.envelope(
        &formal,
        &std::fs::read_to_string("tests/fixtures/conflict/answers.json").unwrap(),
    );
    let output = case.run(&["apply", "--answers", &answers, &template, case.target()]);
    assert_code(&output, 1);
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "conflict");
    assert!(
        document["message"]
            .as_str()
            .unwrap()
            .contains("conflicting files"),
        "{document}"
    );
}

#[test]
fn conflicting_files_are_listed_one_plain_path_per_line() {
    let case = Case::new();
    let folder = case.state.path().join("nested");
    std::fs::create_dir_all(folder.join("template/dir")).unwrap();
    std::fs::write(folder.join("template.yml"), "name: nested\n").unwrap();
    std::fs::write(folder.join("template/file.txt"), "new").unwrap();
    std::fs::write(folder.join("template/dir/other.txt"), "new").unwrap();
    std::fs::create_dir_all(case.target.path().join("dir")).unwrap();
    std::fs::write(case.target.path().join("file.txt"), "old").unwrap();
    std::fs::write(case.target.path().join("dir/other.txt"), "old").unwrap();
    let formal = support::formal_name(&folder);
    let answers = case.envelope(&formal, "{}");
    let template = support::folder_address(&folder);
    let output = case.run(&["apply", "--answers", &answers, &template, case.target()]);
    assert_code(&output, 1);
    // The conflict error document's message lists one indented path per line.
    let message = support::first_document(&output.stdout)["message"]
        .as_str()
        .unwrap()
        .to_string();
    let lines: Vec<&str> = message.lines().collect();
    assert_eq!(lines[0], "conflicting files:", "{message}");
    let mut listed = lines[1..3].to_vec();
    listed.sort();
    assert_eq!(listed, ["  dir/other.txt", "  file.txt"], "{message}");
}
