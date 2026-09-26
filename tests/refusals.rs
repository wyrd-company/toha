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
    /// Writes an answers document and returns its path.
    fn answers(&self, document: &str) -> String {
        let path = self.state.path().join("answers.json");
        std::fs::write(&path, document).unwrap();
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
fn assert_stderr_names(output: &Output, commands: &[String]) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    for command in commands {
        assert!(
            stderr.contains(command.as_str()),
            "stderr does not name `{command}`:\n{stderr}"
        );
    }
}

#[test]
fn untrusted_hooks_name_the_command_with_trust() {
    let case = Case::new();
    let template = support::folder_address(&fixture_template("hooks-untrusted"));
    let answers = case.answers("{}");
    let output = case.run(&["apply", "--answers", &answers, &template, case.target()]);
    assert_code(&output, 3);
    assert_stderr_names(
        &output,
        &[
            "hooks will not run without --trust".into(),
            format!(
                "toha apply --answers {answers} --trust {template} {}",
                case.target()
            ),
        ],
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("templates add"),
        "registry trust does not apply to a folder given directly"
    );
}

#[test]
fn untrusted_hooks_of_installed_template_name_registry_trust() {
    let case = Case::new();
    let template = support::folder_address(&fixture_template("hooks-untrusted"));
    assert_code(&case.run(&["templates", "add", &template]), 0);
    let answers = case.answers("{}");
    let output = case.run(&[
        "apply",
        "--answers",
        &answers,
        "hooks-untrusted",
        case.target(),
    ]);
    assert_code(&output, 3);
    assert_stderr_names(
        &output,
        &[
            format!(
                "toha apply --answers {answers} --trust hooks-untrusted {}",
                case.target()
            ),
            format!("toha templates add --trust {template}"),
        ],
    );
}

#[test]
fn conflicting_files_name_the_command_with_force() {
    let case = Case::new();
    support::copy_tree(
        Path::new("tests/fixtures/conflict/existing"),
        case.target.path(),
    );
    let template = support::folder_address(&fixture_template("conflict"));
    let answers =
        case.answers(&std::fs::read_to_string("tests/fixtures/conflict/answers.json").unwrap());
    let output = case.run(&["apply", "--answers", &answers, &template, case.target()]);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            "conflicting files".into(),
            format!(
                "toha apply --answers {answers} --force {template} {}",
                case.target()
            ),
        ],
    );
}
