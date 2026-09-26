// ---
// relationships:
//   implements: command-line-interface
// ---
//! Commands given at the wrong time for the staged state of a target: `apply`
//! and `stage` with a template while an interview is staged, `continue` on a
//! complete interview, and `continue`, `apply`, and `abort` with nothing staged.
#[allow(dead_code)]
mod support;
use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};
use toha::staging::{Store, canonical_target};

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
    fn send(&self, args: &[&str], input: &str) -> Output {
        let mut child = support::isolated_command(self.state.path())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn staged(&self) -> bool {
        Store::new(support::staged_dir(self.state.path()))
            .load(&canonical_target(self.target.path()).unwrap())
            .unwrap()
            .is_some()
    }
    /// Stages `template` and returns the question batch.
    fn stage_incomplete(&self, template: &str) -> Value {
        let output = self.run(&["stage", template, self.target(), "--async"]);
        assert_code(&output, 4);
        serde_json::from_slice(&output.stdout).unwrap()
    }
    /// Stages the text-basic template, completes it, and returns the complete document.
    fn stage_complete(&self) -> Value {
        self.stage_incomplete(&text_basic());
        let output = self.send(&["continue", self.target(), "-"], r#"{"name":"Item"}"#);
        assert_code(&output, 0);
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn fixture_template(name: &str) -> PathBuf {
    Path::new("tests/fixtures")
        .join(name)
        .join("template")
        .canonicalize()
        .unwrap()
}
fn text_basic() -> String {
    support::folder_address(&fixture_template("text-basic"))
}
fn text_default() -> String {
    support::folder_address(&fixture_template("text-default"))
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
fn apply_with_template_applies_complete_staged_interview_of_that_template() {
    let case = Case::new();
    case.stage_complete();
    let output = case.run(&["apply", &text_basic(), case.target()]);
    assert_code(&output, 0);
    support::assert_tree(
        case.target.path(),
        Path::new("tests/fixtures/text-basic/expected"),
        Path::new("tests/fixtures/text-basic"),
    );
    assert!(!case.staged());
}

#[test]
fn apply_with_untrusted_hooks_template_dry_runs_staged_interview() {
    let case = Case::new();
    let template = support::folder_address(&fixture_template("hooks-untrusted"));
    assert_code(
        &case.run(&["stage", &template, case.target(), "--async"]),
        0,
    );
    let output = case.run(&["apply", &template, case.target()]);
    assert_code(&output, 3);
    assert_stderr_names(&output, &["hooks will not run without --trust".into()]);
    assert!(case.staged());
}

#[test]
fn apply_with_template_emits_batch_of_incomplete_staged_interview() {
    let case = Case::new();
    let batch = case.stage_incomplete(&text_basic());
    let output = case.run(&["apply", &text_basic(), case.target()]);
    assert_code(&output, 4);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        batch
    );
    assert_stderr_names(
        &output,
        &[
            format!("toha continue {}", case.target()),
            format!("toha apply {}", case.target()),
        ],
    );
    assert!(case.staged());
}

#[test]
fn apply_with_template_answers_staged_interview_then_applies() {
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"name":"Item"}"#).unwrap();
    let output = case.run(&[
        "apply",
        &text_basic(),
        case.target(),
        "--answers",
        answers.to_str().unwrap(),
    ]);
    assert_code(&output, 0);
    support::assert_tree(
        case.target.path(),
        Path::new("tests/fixtures/text-basic/expected"),
        Path::new("tests/fixtures/text-basic"),
    );
    assert!(!case.staged());
}

#[test]
fn apply_with_other_template_names_both_templates_and_both_intents() {
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let output = case.run(&["apply", &text_default(), case.target()]);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            text_basic(),
            text_default(),
            format!("toha abort {}", case.target()),
            format!("toha apply {} {}", text_default(), case.target()),
            format!("toha continue {}", case.target()),
            format!("toha apply {}", case.target()),
        ],
    );
    assert!(case.staged());
}

#[test]
fn continue_on_complete_interview_prints_complete_document() {
    let case = Case::new();
    let complete = case.stage_complete();
    let output = case.run(&["continue", case.target()]);
    assert_code(&output, 0);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        complete
    );
    assert_stderr_names(&output, &[format!("toha apply {}", case.target())]);
    assert!(case.staged());
}

#[test]
fn stage_of_staged_template_names_next_step() {
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let output = case.run(&["stage", &text_basic(), case.target(), "--async"]);
    assert_code(&output, 1);
    assert_stderr_names(&output, &[format!("toha continue {}", case.target())]);

    let case = Case::new();
    case.stage_complete();
    let output = case.run(&["stage", &text_basic(), case.target(), "--async"]);
    assert_code(&output, 1);
    assert_stderr_names(&output, &[format!("toha apply {}", case.target())]);
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("toha continue"),
        "a complete interview has nothing to continue"
    );
}

#[test]
fn stage_of_other_template_names_both_templates_and_both_intents() {
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let output = case.run(&["stage", &text_default(), case.target(), "--async"]);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            text_basic(),
            text_default(),
            format!("toha abort {}", case.target()),
            format!("toha stage {} {} --async", text_default(), case.target()),
            format!("toha continue {}", case.target()),
            format!("toha apply {}", case.target()),
        ],
    );
}

#[test]
fn commands_on_unstaged_target_name_the_commands_that_start_an_interview() {
    let case = Case::new();
    let start = [
        format!("toha stage <TEMPLATE> {}", case.target()),
        format!("toha apply <TEMPLATE> {}", case.target()),
    ];
    for output in [
        case.run(&["continue", case.target()]),
        case.send(&["continue", case.target(), "-"], "{}"),
        case.run(&["apply", case.target()]),
    ] {
        assert_code(&output, 1);
        assert_stderr_names(&output, &start);
    }
    let output = case.run(&["abort", case.target()]);
    assert_code(&output, 0);
    assert_stderr_names(&output, &start);
}

#[test]
fn answers_for_complete_staged_interview_are_refused_with_next_step() {
    let case = Case::new();
    case.stage_complete();
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"name":"Other"}"#).unwrap();
    for args in [
        vec!["continue", case.target(), answers.to_str().unwrap()],
        vec![
            "apply",
            &text_basic(),
            case.target(),
            "--answers",
            answers.to_str().unwrap(),
        ],
    ] {
        let output = case.run(&args);
        assert_code(&output, 1);
        assert_stderr_names(
            &output,
            &[
                format!("toha apply {}", case.target()),
                format!("toha abort {}", case.target()),
            ],
        );
        assert!(case.staged());
    }
}

#[test]
fn apply_with_template_dry_runs_staged_interview_without_changes() {
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"name":"Item"}"#).unwrap();
    let output = case.run(&[
        "apply",
        &text_basic(),
        case.target(),
        "--answers",
        answers.to_str().unwrap(),
        "--dry-run",
    ]);
    assert_code(&output, 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("create Item.txt"));
    assert_eq!(std::fs::read_dir(case.target.path()).unwrap().count(), 0);
    let record = Store::new(support::staged_dir(case.state.path()))
        .load(&canonical_target(case.target.path()).unwrap())
        .unwrap()
        .unwrap();
    assert!(record.submissions.is_empty());
}

/// A template whose second question renders from the first, so a document
/// that answers only the first leaves a second batch.
fn two_batch_template(root: &Path) -> String {
    let folder = root.join("two-batch");
    std::fs::create_dir_all(folder.join("template")).unwrap();
    std::fs::write(
        folder.join("template/result.txt"),
        "{{ first }} {{ second }}\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("template.yml"),
        "name: two-batch\ninterview:\n  - { id: first, type: text, prompt: First?, required: true }\n  - { id: second, type: text, prompt: \"After {{ first }}?\", required: true }\n",
    )
    .unwrap();
    support::folder_address(&folder.canonicalize().unwrap())
}

/// Runs `apply <template> <target> --answers {"first":"One"} --dry-run` and
/// checks that it emits the second batch.
fn dry_run_partial(case: &Case, template: &str) {
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"first":"One"}"#).unwrap();
    let output = case.run(&[
        "apply",
        template,
        case.target(),
        "--answers",
        answers.to_str().unwrap(),
        "--dry-run",
    ]);
    assert_code(&output, 4);
    let batch: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        batch["schema"]["properties"].get("second").is_some(),
        "{batch}"
    );
}

#[test]
fn dry_run_with_partial_answers_stages_nothing() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    dry_run_partial(&case, &template);
    assert!(!case.staged(), "a dry run stages nothing");
}

#[test]
fn dry_run_with_partial_answers_records_nothing_in_staged_interview() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    dry_run_partial(&case, &template);
    let record = Store::new(support::staged_dir(case.state.path()))
        .load(&canonical_target(case.target.path()).unwrap())
        .unwrap()
        .unwrap();
    assert!(
        record.submissions.is_empty(),
        "a dry run records no answers: {:?}",
        record.submissions
    );
}
