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
    // The scripted route requires the identity-bearing envelope naming the
    // formal template: the folder's canonical path.
    std::fs::write(
        dir.join("answers.json"),
        support::envelope_text(
            &support::formal_name(dir),
            &serde_json::json!({ "title": "Sample" }),
        ),
    )
    .unwrap();
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
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The dry run is a `planned` document. Its interview messages, including the
    // before-apply message, lead `messages`; the after-apply message is withheld
    // until files are written.
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    let messages = support::document_messages(&document);
    assert_eq!(messages, ["Node Sample", "Before Sample"], "{document}");
    assert!(!messages.iter().any(|message| message == "After Sample"));
    // The plan lists the single created file.
    assert_eq!(
        support::plan_text_from_document(&document),
        "create note.txt",
        "{document}"
    );
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[test]
fn untrusted_hooks_dry_run_does_not_show_after_apply() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "hooks:\n  - run: [ tool, call ]\n");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &[]);
    // Untrusted hooks turn a plain scripted apply into a planned dry run: exit 3,
    // a `planned` document with trusted:false that shows the before-apply message
    // and the create plan but withholds the after-apply message.
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    assert_eq!(document["trusted"], false, "{document}");
    let messages = support::document_messages(&document);
    assert!(
        messages.iter().any(|message| message == "Before Sample"),
        "{document}"
    );
    assert!(
        !messages.iter().any(|message| message == "After Sample"),
        "{document}"
    );
    assert!(
        support::plan_text_from_document(&document).contains("create note.txt"),
        "{document}"
    );
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[test]
fn apply_shows_after_apply_after_writing() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // Files were written, so the applied document ends with the after-apply
    // message.
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    let messages = support::document_messages(&document);
    assert_eq!(
        messages.last().map(String::as_str),
        Some("After Sample"),
        "{document}"
    );
}

/// One isolated configuration, registry, and state root shared by the calls
/// of a scenario, with the target directory for its runs.
struct Scenario {
    root: tempfile::TempDir,
    target: tempfile::TempDir,
}
impl Scenario {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            target: tempfile::tempdir().unwrap(),
        }
    }
    fn target(&self) -> String {
        self.target.path().to_str().unwrap().to_string()
    }
    fn run(&self, args: &[&str]) -> Output {
        support::isolated_command(self.root.path())
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }
    fn answers(&self) -> String {
        let path = self.root.path().join("answers.json");
        // The scripted route requires the identity envelope; every scenario
        // drives the `hooks-untrusted` fixture, so its formal names it.
        std::fs::write(
            &path,
            support::envelope_text(&hooks_template(), &serde_json::json!({})),
        )
        .unwrap();
        path.to_str().unwrap().to_string()
    }
}

fn hooks_template() -> String {
    support::folder_address(
        &Path::new("tests/fixtures/hooks-untrusted/template")
            .canonicalize()
            .unwrap(),
    )
}

fn assert_dry_run(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hook "),
        "the plan lists the hook: {stdout}"
    );
}

fn trust_hint(output: &Output) -> Vec<String> {
    support::suggested(&String::from_utf8_lossy(&output.stderr), "hooks need trust")
}

fn assert_no_trust_hint(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("trust"), "{stderr}");
}

/// The scripted route reports hook trust structurally in its `planned`
/// document: `trusted` and the exit code, with no trust command on standard
/// error. Untrusted is exit 3; trusted is exit 0. Either way the plan lists the
/// hook.
fn assert_scripted_plan(output: &Output, trusted: bool) {
    let expected_exit = if trusted { 0 } else { 3 };
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    assert_eq!(document["trusted"], trusted, "{document}");
    assert!(
        support::plan_text_from_document(&document).contains("hook "),
        "{document}"
    );
    // The scripted route names no trust command on standard error.
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dry_run_of_untrusted_hooks_reports_them_as_untrusted() {
    let scenario = Scenario::new();
    let template = hooks_template();
    let answers = scenario.answers();
    let target = scenario.target();
    let output = scenario.run(&[
        "apply",
        &template,
        &target,
        "--answers",
        &answers,
        "--dry-run",
    ]);
    // The scripted route reports the untrusted hooks structurally (exit 3,
    // trusted:false, hooks listed) rather than naming a trust command on
    // standard error.
    assert_scripted_plan(&output, false);
    assert!(
        std::fs::read_dir(scenario.target.path())
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn dry_run_of_a_staged_interview_with_untrusted_hooks_names_the_command_that_runs_them() {
    let scenario = Scenario::new();
    let template = hooks_template();
    let target = scenario.target();
    let staged = scenario.run(&["stage", &template, &target, "--async"]);
    assert_eq!(
        staged.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let by_path = scenario.run(&["apply", &target, "--dry-run"]);
    assert_dry_run(&by_path);
    assert_eq!(trust_hint(&by_path), ["apply", "--trust", &target]);
    let by_template = scenario.run(&["apply", &template, &target, "--dry-run"]);
    assert_dry_run(&by_template);
    assert_eq!(
        trust_hint(&by_template),
        ["apply", "--trust", &template, &target]
    );
}

#[test]
fn dry_run_of_an_installed_untrusted_template_reports_it_as_untrusted() {
    let scenario = Scenario::new();
    let template = hooks_template();
    let added = scenario.run(&["templates", "add", &template]);
    assert_eq!(added.status.code(), Some(0));
    let answers = scenario.answers();
    let target = scenario.target();
    let output = scenario.run(&[
        "apply",
        "hooks-untrusted",
        &target,
        "--answers",
        &answers,
        "--dry-run",
    ]);
    // Resolved through the registry by short name, the scripted route still
    // reports untrusted hooks structurally (exit 3, trusted:false). The
    // registry-trust command the text routes name on standard error is not part
    // of the scripted document.
    assert_scripted_plan(&output, false);
}

#[test]
fn dry_run_of_trusted_hooks_reports_them_as_trusted() {
    let scenario = Scenario::new();
    let template = hooks_template();
    let answers = scenario.answers();
    let target = scenario.target();
    let for_this_run = scenario.run(&[
        "apply",
        &template,
        &target,
        "--answers",
        &answers,
        "--trust",
        "--dry-run",
    ]);
    // `--trust` makes the scripted plan trusted: exit 0, trusted:true, no trust
    // command on standard error.
    assert_scripted_plan(&for_this_run, true);
    let added = scenario.run(&["templates", "add", "--trust", &template]);
    assert_eq!(added.status.code(), Some(0));
    let by_registry = scenario.run(&[
        "apply",
        "hooks-untrusted",
        &target,
        "--answers",
        &answers,
        "--dry-run",
    ]);
    // A current registry approval trusts the hooks the same way.
    assert_scripted_plan(&by_registry, true);
}

#[test]
fn dry_run_without_hooks_names_no_trust_command() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &["--dry-run"]);
    assert_eq!(output.status.code(), Some(0));
    // No hooks means nothing to trust: the scripted route names no trust command.
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    assert_no_trust_hint(&output);
}
