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
        std::fs::write(&path, "{}").unwrap();
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

#[test]
fn dry_run_of_untrusted_hooks_names_the_command_that_runs_them() {
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
    assert_dry_run(&output);
    assert_eq!(
        trust_hint(&output),
        [
            "apply",
            "--answers",
            &answers,
            "--trust",
            &template,
            &target
        ]
    );
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
fn dry_run_of_an_installed_untrusted_template_also_names_registry_trust() {
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
    assert_dry_run(&output);
    assert_eq!(
        trust_hint(&output),
        [
            "apply",
            "--answers",
            &answers,
            "--trust",
            "hooks-untrusted",
            &target
        ]
    );
    assert_eq!(
        support::suggested(&String::from_utf8_lossy(&output.stderr), "for every run"),
        ["templates", "add", "--trust", &template]
    );
}

#[test]
fn dry_run_of_trusted_hooks_names_no_trust_command() {
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
    assert_dry_run(&for_this_run);
    assert_no_trust_hint(&for_this_run);
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
    assert_dry_run(&by_registry);
    assert_no_trust_hint(&by_registry);
}

#[test]
fn dry_run_without_hooks_names_no_trust_command() {
    let folder = tempfile::tempdir().unwrap();
    template(folder.path(), "");
    let target = tempfile::tempdir().unwrap();
    let output = apply(folder.path(), target.path(), &["--dry-run"]);
    assert_eq!(output.status.code(), Some(0));
    assert_no_trust_hint(&output);
}
