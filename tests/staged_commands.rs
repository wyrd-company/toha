// ---
// relationships:
//   implements: command-line-interface
// ---
//! Commands given at the wrong time or in the wrong form for the staged state of
//! a target: `apply`/`stage` with a template while an interview is staged,
//! `continue` on a complete interview, the scripted route refusing a staged
//! target, and `continue`/`apply`/`abort` with nothing staged.
#[allow(dead_code)]
mod support;
use serde_json::Value;
use std::{
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
    fn staged(&self) -> bool {
        Store::new(support::staged_dir(self.state.path()))
            .load(&canonical_target(self.target.path()).unwrap())
            .unwrap()
            .is_some()
    }
    /// Stages `template` and returns the question batch (the leading JSON of the
    /// agent route, before its instructions).
    fn stage_incomplete(&self, template: &str) -> Value {
        let output = self.run(&["stage", template, self.target(), "--async"]);
        assert_code(&output, 4);
        support::first_document(&output.stdout)
    }
    /// The identity envelope naming a template, around a bare answers map.
    fn envelope(&self, template: &Path, answers: serde_json::Value) -> PathBuf {
        let formal = support::formal_name(template);
        let document = serde_json::json!({ "template": formal, "answers": answers });
        let path = self.state.path().join("answers.json");
        std::fs::write(&path, document.to_string()).unwrap();
        path
    }
    /// Stages the text-basic template and completes it through `continue PATH
    /// FILE`. A completing agent continue writes instructions only, so there is
    /// nothing to return.
    fn stage_complete(&self) {
        self.stage_incomplete(&text_basic());
        let answers = self.envelope(&fixture_template("text-basic"), serde_json::json!({ "name": "Item" }));
        let output = self.run(&["continue", self.target(), answers.to_str().unwrap()]);
        assert_code(&output, 0);
    }
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
fn apply_with_untrusted_hooks_template_needs_trust() {
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
fn apply_without_template_emits_batch_of_incomplete_staged_interview() {
    // `apply PATH` is the agent route: it reports the current batch with
    // instructions and never prompts.
    let case = Case::new();
    let batch = case.stage_incomplete(&text_basic());
    let output = case.run(&["apply", case.target()]);
    assert_code(&output, 4);
    assert_eq!(support::first_document(&output.stdout), batch);
    // The instructions on standard output name both continue forms and apply.
    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in [
        format!("toha continue {} <ANSWERS>", case.target()),
        format!("toha continue {}", case.target()),
        format!("toha apply {}", case.target()),
    ] {
        assert!(stdout.contains(&command), "stdout misses `{command}`:\n{stdout}");
    }
    assert!(case.staged());
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
fn scripted_route_refuses_a_staged_target_before_reading_the_document() {
    // `apply TEMPLATE PATH --answers FILE` is the scripted route; it refuses a
    // staged target with a `staged` error document naming the commands that
    // finish or discard the interview, and never reads the document.
    let case = Case::new();
    case.stage_incomplete(&text_basic());
    let unreadable = case.state.path().join("does-not-exist.json");
    let output = case.run(&[
        "apply",
        &text_basic(),
        case.target(),
        "--answers",
        unreadable.to_str().unwrap(),
    ]);
    assert_code(&output, 1);
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "staged");
    let commands = document["commands"].as_array().expect("staged commands");
    let commands: Vec<&str> = commands.iter().map(|c| c.as_str().unwrap()).collect();
    for command in [
        format!("toha continue {} <ANSWERS>", case.target()),
        format!("toha apply {}", case.target()),
        format!("toha abort {}", case.target()),
    ] {
        assert!(
            commands.iter().any(|c| *c == command),
            "commands miss `{command}`: {commands:?}"
        );
    }
    assert!(case.staged());
}

#[test]
fn continue_on_complete_interview_shows_the_plan_and_apply_instruction() {
    // `continue PATH` on a complete interview shows the dry-run plan and the
    // apply instructions, writing nothing.
    let case = Case::new();
    case.stage_complete();
    let output = case.run(&["continue", case.target()]);
    assert_code(&output, 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create Item.txt"), "{stdout}");
    assert!(
        stdout.contains(&format!("toha apply {}", case.target())),
        "{stdout}"
    );
    assert_eq!(std::fs::read_dir(case.target.path()).unwrap().count(), 0);
    assert!(case.staged());
}

#[test]
fn continue_file_on_complete_interview_refuses_naming_apply() {
    // `continue PATH FILE` on a complete interview refuses before reading the
    // document, naming `apply PATH --dry-run` and `apply PATH`.
    let case = Case::new();
    case.stage_complete();
    let unreadable = case.state.path().join("does-not-exist.json");
    let output = case.run(&["continue", case.target(), unreadable.to_str().unwrap()]);
    assert_code(&output, 1);
    assert_stderr_names(
        &output,
        &[
            format!("toha apply --dry-run {}", case.target()),
            format!("toha apply {}", case.target()),
        ],
    );
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
        case.run(&["continue", case.target(), "-"]),
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
    store.save(&target, &record).unwrap();
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

/// A two-batch template whose second question renders from the first, so a
/// person route prompts a second batch after the first is answered.
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

/// `apply TEMPLATE PATH` (the person route) prompts each remaining question in a
/// terminal, saving each batch, then applies.
#[cfg(unix)]
#[test]
fn apply_with_template_prompts_for_incomplete_staged_interview_in_terminal() {
    use expectrl::{Expect, Session};
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    let mut command = support::isolated_command(case.state.path());
    command
        .arg("apply")
        .arg(&template)
        .arg(case.target());
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

/// `apply TEMPLATE PATH --dry-run` prompts in a terminal, then records and
/// writes nothing.
#[cfg(unix)]
#[test]
fn apply_with_template_dry_run_in_terminal_records_and_writes_nothing() {
    use expectrl::{Expect, Session};
    let case = Case::new();
    let template = two_batch_template(case.state.path());
    case.stage_incomplete(&template);
    let mut command = support::isolated_command(case.state.path());
    command
        .args(["apply", &template])
        .arg(case.target())
        .arg("--dry-run");
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
