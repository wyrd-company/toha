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

/// Runs `apply <template> <target> --answers {"first":"One"}` with `extra`
/// flags and checks that it emits the second batch.
fn partial(case: &Case, template: &str, extra: &[&str]) -> Output {
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"first":"One"}"#).unwrap();
    let mut args = vec![
        "apply",
        template,
        case.target(),
        "--answers",
        answers.to_str().unwrap(),
    ];
    args.extend(extra);
    let output = case.run(&args);
    assert_code(&output, 4);
    let batch: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        batch["schema"]["properties"].get("second").is_some(),
        "{batch}"
    );
    output
}

#[test]
fn dry_run_with_partial_answers_stages_nothing() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    partial(&case, &template, &["--dry-run"]);
    assert!(!case.staged(), "a dry run stages nothing");
}

#[test]
fn dry_run_with_partial_answers_records_nothing_in_staged_interview() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    partial(&case, &template, &["--dry-run"]);
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

fn answers_path(case: &Case) -> String {
    case.state
        .path()
        .join("answers.json")
        .to_str()
        .unwrap()
        .to_string()
}

#[test]
fn partial_answers_name_the_commands_that_finish_the_interview() {
    for staged in [false, true] {
        let case = Case::new();
        let template = two_batch_template(case.state.path());
        if staged {
            case.stage_incomplete(&template);
        }
        let output = partial(&case, &template, &[]);
        assert_stderr_names(
            &output,
            &[
                format!("toha continue {}", case.target()),
                format!("toha apply {}", case.target()),
            ],
        );
    }
}

#[test]
fn dry_run_with_partial_answers_names_the_command_that_records_them() {
    for staged in [false, true] {
        let case = Case::new();
        let template = two_batch_template(case.state.path());
        if staged {
            case.stage_incomplete(&template);
        }
        let output = partial(&case, &template, &["--dry-run"]);
        let record = format!(
            "toha apply --answers {} {template} {}",
            answers_path(&case),
            case.target()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.lines().any(|line| line.ends_with(&record)),
            "stderr does not name `{record}`:\n{stderr}"
        );
        assert!(
            stderr.contains("records nothing"),
            "stderr does not say the dry run records nothing:\n{stderr}"
        );
    }
}

/// The last stderr line of a partial dry run names this command to record the answers.
fn assert_records_with(output: &Output, record: &str) {
    assert_code(output, 4);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().last().unwrap_or_default();
    assert_eq!(
        last,
        format!("to record these answers: {record}"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn dry_run_hint_is_built_from_parsed_flags() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"first":"One"}"#).unwrap();
    let answers = answers.to_str().unwrap();
    let attached = format!("-dA{answers}");
    let forced = format!(
        "toha apply --answers {answers} --force {template} {}",
        case.target()
    );
    for flags in [
        vec!["-fd", "--answers", answers],
        vec!["-df", "--answers", answers],
    ] {
        let mut args = vec!["apply"];
        args.extend(flags);
        args.extend([template.as_str(), case.target()]);
        assert_records_with(&case.run(&args), &forced);
    }
    let output = case.run(&["apply", &attached, &template, case.target()]);
    assert_records_with(
        &output,
        &format!(
            "toha apply --answers {answers} {template} {}",
            case.target()
        ),
    );
    assert!(!case.staged());
}

#[test]
fn dry_run_hint_keeps_a_target_spelled_like_a_flag() {
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"first":"One"}"#).unwrap();
    let answers = answers.to_str().unwrap();
    let output = support::isolated_command(case.state.path())
        .current_dir(case.target.path())
        .args([
            "apply",
            "--answers",
            answers,
            "--dry-run",
            "--",
            &template,
            "-d",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_records_with(
        &output,
        &format!("toha apply --answers {answers} -- {template} -d"),
    );
}

impl Case {
    fn submissions(&self) -> Vec<serde_json::Map<String, Value>> {
        Store::new(support::staged_dir(self.state.path()))
            .load(&canonical_target(self.target.path()).unwrap())
            .unwrap()
            .expect("staged record")
            .submissions
            .into_iter()
            .map(|submission| submission.into_iter().collect())
            .collect()
    }
}

/// Runs `apply` with `operands` in a terminal and checks that the first batch
/// is recorded before the second prompt and the files are written.
#[cfg(unix)]
fn apply_prompts_in_terminal(operands: &[&str]) {
    use expectrl::{Expect, Session};
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    let operands: Vec<&str> = operands
        .iter()
        .map(|operand| {
            if *operand == "<TEMPLATE>" {
                template.as_str()
            } else {
                operand
            }
        })
        .collect();
    let mut command = support::isolated_command(case.state.path());
    command.arg("apply").args(&operands).arg(case.target());
    let mut session = Session::spawn(command).unwrap();
    session.expect("First?").unwrap();
    session.send_line("One").unwrap();
    session.expect("After One?").unwrap();
    let recorded = case.submissions();
    assert_eq!(
        recorded.len(),
        1,
        "the first batch is recorded before the second prompt: {recorded:?}"
    );
    assert_eq!(recorded[0]["first"], "One");
    session.send_line("Two").unwrap();
    session.expect("result.txt").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert_eq!(
        std::fs::read_to_string(case.target.path().join("result.txt")).unwrap(),
        "One Two\n"
    );
    assert!(!case.staged());
}

#[cfg(unix)]
#[test]
fn apply_prompts_for_incomplete_staged_interview_in_terminal() {
    apply_prompts_in_terminal(&[]);
}

#[cfg(unix)]
#[test]
fn apply_with_template_prompts_for_incomplete_staged_interview_in_terminal() {
    apply_prompts_in_terminal(&["<TEMPLATE>"]);
}

#[cfg(unix)]
#[test]
fn dry_run_in_terminal_records_and_writes_nothing() {
    use expectrl::{Expect, Session};
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    let mut command = support::isolated_command(case.state.path());
    command.args(["apply", "--dry-run"]).arg(case.target());
    let mut session = Session::spawn(command).unwrap();
    session.expect("First?").unwrap();
    session.send_line("One").unwrap();
    session.expect("After One?").unwrap();
    session.send_line("Two").unwrap();
    let rest = session.expect(expectrl::Eof).unwrap();
    assert_eq!(
        std::fs::read_dir(case.target.path()).unwrap().count(),
        0,
        "a dry run writes no file"
    );
    assert!(
        case.submissions().is_empty(),
        "a dry run records no answers: {:?}",
        case.submissions()
    );
    let printed = String::from_utf8_lossy(rest.as_bytes());
    assert!(printed.contains("create result.txt"), "{printed}");
}

#[test]
fn suggested_commands_guard_a_target_spelled_like_a_flag() {
    let case = Case::new();
    let output = support::isolated_command(case.state.path())
        .current_dir(case.target.path())
        .args(["continue", "--", "-d"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            "toha stage -- <TEMPLATE> -d".into(),
            "toha apply -- <TEMPLATE> -d".into(),
        ],
    );
}

#[test]
fn answers_without_template_name_the_commands_that_take_them() {
    let case = Case::new();
    let answers = case.state.path().join("answers.json");
    std::fs::write(&answers, r#"{"name":"Item"}"#).unwrap();
    let answers = answers.to_str().unwrap();
    let args = ["apply", "--answers", answers, case.target()];

    let output = case.run(&args);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[format!(
            "toha apply --answers {answers} <TEMPLATE> {}",
            case.target()
        )],
    );

    case.stage_incomplete(&text_basic());
    let output = case.run(&args);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            format!("toha continue {} {answers}", case.target()),
            format!("toha apply {}", case.target()),
            format!(
                "toha apply --answers {answers} {} {}",
                text_basic(),
                case.target()
            ),
        ],
    );
}

#[test]
fn suggested_commands_quote_words_a_shell_would_change() {
    let case = Case::new();
    let mut targets = vec![("~/notes", support::shell_quoted("~/notes"))];
    if cfg!(unix) {
        targets.push((r"foo\bar", support::shell_quoted(r"foo\bar")));
    }
    for (target, quoted) in targets {
        let output = support::isolated_command(case.state.path())
            .current_dir(case.target.path())
            .args(["continue", target])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_code(&output, 1);
        assert_stderr_names(
            &output,
            &[
                format!("toha stage <TEMPLATE> {quoted}"),
                format!("toha apply <TEMPLATE> {quoted}"),
            ],
        );
    }

    let template = two_batch_template(case.state.path());
    std::fs::write(case.target.path().join("#a.json"), r#"{"first":"One"}"#).unwrap();
    let output = support::isolated_command(case.state.path())
        .current_dir(case.target.path())
        .args([
            "apply",
            "--answers",
            "#a.json",
            "--dry-run",
            &template,
            "out",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_records_with(
        &output,
        &format!(
            "toha apply --answers {} {template} out",
            support::shell_quoted("#a.json")
        ),
    );
}

#[test]
fn empty_answers_path_is_named_as_an_empty_word() {
    let case = Case::new();
    let output = case.run(&["apply", case.target(), "--answers", ""]);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[format!(
            "toha apply --answers {} <TEMPLATE> {}",
            support::shell_quoted(""),
            case.target()
        )],
    );
}

/// A staged record that no longer replays (an earlier build recorded a
/// changed answer) names the commands that start the interview over.
#[test]
fn staged_record_that_does_not_replay_names_abort_and_stage() {
    let case = Case::new();
    case.stage_complete();
    let store = Store::new(support::staged_dir(case.state.path()));
    let target = canonical_target(case.target.path()).unwrap();
    let mut record = store.load(&target).unwrap().expect("staged record");
    record.submissions.push(
        [("name".to_string(), Value::from("Changed"))]
            .into_iter()
            .collect(),
    );
    store.save(&record).unwrap();
    for args in [
        vec!["continue", case.target()],
        vec!["apply", case.target()],
    ] {
        let output = case.run(&args);
        assert_code(&output, 1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!(
                "the staged interview at {} cannot be resumed: submission after completed interview",
                case.target()
            )),
            "{args:?}: {stderr}"
        );
        assert_stderr_names(
            &output,
            &[
                format!("toha abort {}", case.target()),
                format!("toha stage {} {}", text_basic(), case.target()),
            ],
        );
    }
}
