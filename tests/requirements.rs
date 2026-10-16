// ---
// relationships:
//   implements: template-format
// ---
//! A template's `requires` programs are confirmed on `PATH` before any route
//! asks a question, writes a file, or runs a hook.
#[allow(dead_code)]
mod support;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use toha::{Requirement, Template, requirement};

const ABSENT: &str = "toha-fixture-absent-program";
const TOOL: &str = "toha-fixture-absent-tool";

fn missing_fixture() -> PathBuf {
    Path::new("tests/fixtures/requires-missing/template")
        .canonicalize()
        .unwrap()
}

fn template(dir: &Path, yaml: &str) -> Result<Template, toha::LoadError> {
    fs::create_dir_all(dir.join("template")).unwrap();
    fs::write(dir.join("template/file.txt"), "body\n").unwrap();
    fs::write(dir.join("template.yml"), yaml).unwrap();
    Template::load(dir)
}

/// Writes an executable named `program` into `dir`, in the form the platform
/// search finds: an `.exe` on Windows, a mode-0755 file elsewhere.
fn install(dir: &Path, program: &str) {
    #[cfg(windows)]
    fs::write(dir.join(format!("{program}.exe")), b"").unwrap();
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(program);
        fs::write(&path, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The inherited `PATH` with `dir` first.
fn path_with(dir: &Path) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(
        std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&inherited)),
    )
    .unwrap()
}

#[test]
fn load_reads_a_name_and_an_object_entry_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        "name: reqs\nrequires:\n  - first\n  - program: second\n    hint: Get second.\n",
    )
    .unwrap();
    assert_eq!(
        template.requires,
        vec![
            Requirement {
                program: "first".into(),
                hint: None
            },
            Requirement {
                program: "second".into(),
                hint: Some("Get second.".into())
            },
        ]
    );
}

#[test]
fn load_without_requires_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(dir.path(), "name: reqs\n").unwrap();
    assert!(template.requires.is_empty());
    requirement::check_in(&template, Some(OsString::new().as_os_str()), dir.path()).unwrap();
}

#[test]
fn load_rejects_a_program_that_is_not_a_bare_name() {
    for program in ["bin/tool", "bin\\\\tool", "/usr/bin/tool", ".", "..", ""] {
        for entry in [
            format!("\"{program}\""),
            format!("{{ program: \"{program}\" }}"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let error = template(dir.path(), &format!("name: reqs\nrequires:\n  - {entry}\n"))
                .expect_err(&entry)
                .to_string();
            assert!(error.contains("requires"), "{entry}: {error}");
        }
    }
}

#[test]
fn load_rejects_an_empty_hint() {
    let dir = tempfile::tempdir().unwrap();
    let error = template(
        dir.path(),
        "name: reqs\nrequires:\n  - { program: tool, hint: \"\" }\n",
    )
    .expect_err("empty hint")
    .to_string();
    assert!(error.contains("requires"), "{error}");
}

#[test]
fn check_finds_a_program_on_the_search_path() {
    let bin = tempfile::tempdir().unwrap();
    install(bin.path(), ABSENT);
    install(bin.path(), TOOL);
    let template = Template::load(&missing_fixture()).unwrap();
    let search = std::env::join_paths([bin.path()]).unwrap();
    requirement::check_in(&template, Some(&search), bin.path()).unwrap();
}

#[test]
fn check_names_each_absent_program_in_order_with_its_hint() {
    let bin = tempfile::tempdir().unwrap();
    let template = Template::load(&missing_fixture()).unwrap();
    let search = std::env::join_paths([bin.path()]).unwrap();
    let error = requirement::check_in(&template, Some(&search), bin.path()).unwrap_err();
    assert_eq!(error.template, "requires-missing");
    assert_eq!(
        error.missing,
        vec![
            Requirement {
                program: ABSENT.into(),
                hint: None
            },
            Requirement {
                program: TOOL.into(),
                hint: Some("Install the absent tool.".into())
            },
        ]
    );
    assert_eq!(
        error.to_string(),
        format!(
            "template `requires-missing` requires programs that are not on PATH:\n  {ABSENT}\n  {TOOL}: Install the absent tool."
        )
    );
}

#[test]
fn check_names_only_the_absent_program() {
    let bin = tempfile::tempdir().unwrap();
    install(bin.path(), ABSENT);
    let template = Template::load(&missing_fixture()).unwrap();
    let search = std::env::join_paths([bin.path()]).unwrap();
    let error = requirement::check_in(&template, Some(&search), bin.path()).unwrap_err();
    let names: Vec<_> = error.missing.iter().map(|r| r.program.as_str()).collect();
    assert_eq!(names, [TOOL]);
}

#[cfg(unix)]
#[test]
fn check_does_not_accept_a_file_that_is_not_executable() {
    let bin = tempfile::tempdir().unwrap();
    install(bin.path(), TOOL);
    fs::write(bin.path().join(ABSENT), b"#!/bin/sh\n").unwrap();
    let template = Template::load(&missing_fixture()).unwrap();
    let search = std::env::join_paths([bin.path()]).unwrap();
    let error = requirement::check_in(&template, Some(&search), bin.path()).unwrap_err();
    let names: Vec<_> = error.missing.iter().map(|r| r.program.as_str()).collect();
    assert_eq!(names, [ABSENT]);
}

struct Case {
    state: tempfile::TempDir,
    target: tempfile::TempDir,
    bin: tempfile::TempDir,
}
impl Case {
    fn new() -> Self {
        Self {
            state: tempfile::tempdir().unwrap(),
            target: tempfile::tempdir().unwrap(),
            bin: tempfile::tempdir().unwrap(),
        }
    }
    /// Puts both required programs on the case `PATH`.
    fn provide(&self) {
        install(self.bin.path(), ABSENT);
        install(self.bin.path(), TOOL);
    }
    fn target(&self) -> &str {
        self.target.path().to_str().unwrap()
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = support::isolated_command(self.state.path());
        command
            .args(args)
            .env("PATH", path_with(self.bin.path()))
            .stdin(Stdio::null());
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn answers(&self) -> String {
        let formal = support::formal_name(&missing_fixture());
        let document = serde_json::json!({ "template": formal, "answers": { "title": "Example" } });
        let path = self.state.path().join("answers.json");
        fs::write(&path, document.to_string()).unwrap();
        path.to_str().unwrap().to_owned()
    }
    fn assert_target_empty(&self) {
        assert!(
            fs::read_dir(self.target.path()).unwrap().next().is_none(),
            "the target holds files"
        );
    }
    fn assert_nothing_staged(&self) {
        let staged = support::staged_dir(self.state.path());
        assert!(
            !staged.exists() || fs::read_dir(&staged).unwrap().next().is_none(),
            "an interview is staged"
        );
    }
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

fn assert_requirement_error(output: &Output) {
    assert_code(output, 1);
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "requirement");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains(ABSENT), "{message}");
    assert!(
        message.contains(&format!("{TOOL}: Install the absent tool.")),
        "{message}"
    );
}

#[test]
fn scripted_apply_reports_a_requirement_error_and_writes_nothing() {
    let case = Case::new();
    let template = support::folder_address(&missing_fixture());
    let answers = case.answers();
    let output = case.run(&["apply", &template, case.target(), "--answers", &answers]);
    assert_requirement_error(&output);
    case.assert_target_empty();
}

#[test]
fn scripted_dry_run_reports_a_requirement_error() {
    let case = Case::new();
    let template = support::folder_address(&missing_fixture());
    let answers = case.answers();
    let output = case.run(&[
        "apply",
        &template,
        case.target(),
        "--answers",
        &answers,
        "--dry-run",
    ]);
    assert_requirement_error(&output);
}

#[test]
fn scripted_apply_runs_when_every_program_is_present() {
    let case = Case::new();
    case.provide();
    let template = support::folder_address(&missing_fixture());
    let answers = case.answers();
    let output = case.run(&["apply", &template, case.target(), "--answers", &answers]);
    assert_code(&output, 0);
    assert_eq!(
        fs::read_to_string(case.target.path().join("file.txt")).unwrap(),
        "Example\n"
    );
}

#[test]
fn agent_stage_refuses_before_the_first_batch() {
    let case = Case::new();
    let template = support::folder_address(&missing_fixture());
    let output = case.run(&["stage", &template, case.target(), "--async"]);
    assert_code(&output, 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(ABSENT), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Title?"));
    case.assert_nothing_staged();
    case.assert_target_empty();
}

#[test]
fn continue_refuses_when_a_program_left_path_after_stage() {
    let case = Case::new();
    case.provide();
    let template = support::folder_address(&missing_fixture());
    let staged = case.run(&["stage", &template, case.target(), "--async"]);
    assert_code(&staged, 4);
    fs::remove_dir_all(case.bin.path()).unwrap();
    let answers = case.state.path().join("batch.json");
    fs::write(&answers, r#"{"title": "Example"}"#).unwrap();
    let output = case.run(&["continue", case.target(), answers.to_str().unwrap()]);
    assert_code(&output, 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(ABSENT), "{stderr}");
    case.assert_target_empty();
}

#[cfg(unix)]
#[test]
fn person_apply_refuses_before_the_first_prompt() {
    use expectrl::{Expect, Session};
    let case = Case::new();
    let template = support::folder_address(&missing_fixture());
    let mut command = support::isolated_command(case.state.path());
    command
        .args(["apply", &template, case.target()])
        .env("PATH", path_with(case.bin.path()));
    let mut session = Session::spawn(command).unwrap();
    let seen = session.expect(expectrl::Eof).unwrap();
    let text = String::from_utf8_lossy(seen.as_bytes()).into_owned();
    assert!(text.contains(ABSENT), "{text}");
    assert!(!text.contains("Title?"), "{text}");
    case.assert_nothing_staged();
    case.assert_target_empty();
}

#[test]
fn staged_apply_refuses_when_a_program_left_path_after_stage() {
    let case = Case::new();
    case.provide();
    let template = support::folder_address(&missing_fixture());
    let staged = case.run(&["stage", &template, case.target(), "--async"]);
    assert_code(&staged, 4);
    fs::remove_dir_all(case.bin.path()).unwrap();
    let output = case.run(&["apply", case.target()]);
    assert_code(&output, 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(ABSENT), "{stderr}");
    case.assert_target_empty();
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=Example",
            "-c",
            "user.email=example@example.invalid",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A git target whose only commit holds one seed file.
fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

#[test]
fn baseline_refuses_while_a_program_is_absent() {
    let case = Case::new();
    target_repo(case.target.path());
    let template = support::folder_address(&missing_fixture());
    let answers = case.answers();
    let output = case.run(&[
        "apply",
        &template,
        case.target(),
        "--baseline",
        "--answers",
        &answers,
    ]);
    assert_code(&output, 1);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(ABSENT), "{text}");
    assert!(!case.target.path().join("file.txt").exists());
}

/// Baseline-applies the fixture into a git target with every program present,
/// commits it, and returns the snapshot id.
fn baselined(case: &Case) -> String {
    case.provide();
    target_repo(case.target.path());
    let template = support::folder_address(&missing_fixture());
    let answers = case.answers();
    let baseline = case.run(&[
        "apply",
        &template,
        case.target(),
        "--baseline",
        "--answers",
        &answers,
    ]);
    assert_code(&baseline, 0);
    let document = support::first_document(&baseline.stdout);
    let id = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(case.target.path(), &["add", "."]);
    git(case.target.path(), &["commit", "--quiet", "-m", "baseline"]);
    id
}

#[test]
fn staged_update_continue_refuses_when_a_program_left_path() {
    let case = Case::new();
    let id = baselined(&case);
    let template = support::folder_address(&missing_fixture());
    let staged = case.run(&[
        "stage",
        &template,
        case.target(),
        "--from",
        &id,
        "--reanswer",
        "--async",
    ]);
    assert_code(&staged, 4);
    fs::remove_dir_all(case.bin.path()).unwrap();
    let batch = case.state.path().join("batch.json");
    fs::write(&batch, r#"{"title": "Other"}"#).unwrap();
    let output = case.run(&["continue", case.target(), batch.to_str().unwrap()]);
    assert_code(&output, 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(ABSENT), "{stderr}");
    assert_eq!(
        fs::read_to_string(case.target.path().join("file.txt")).unwrap(),
        "Example\n"
    );
}

#[test]
fn update_refuses_when_a_program_left_path_after_the_baseline() {
    let case = Case::new();
    let id = baselined(&case);
    let answers = case.answers();
    fs::remove_dir_all(case.bin.path()).unwrap();
    let output = case.run(&["apply", case.target(), "--from", &id, "--answers", &answers]);
    assert_code(&output, 1);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(ABSENT), "{text}");
}

#[test]
fn a_hint_and_a_program_are_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let template = template(
        dir.path(),
        "name: reqs\nrequires:\n  - \"{{ undefined_name }}\"\n  - { program: tool, hint: \"Run {{ undefined_name }}.\" }\n",
    )
    .unwrap();
    assert_eq!(template.requires[0].program, "{{ undefined_name }}");
    assert_eq!(
        template.requires[1].hint.as_deref(),
        Some("Run {{ undefined_name }}.")
    );
}
