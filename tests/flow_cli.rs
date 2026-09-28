// ---
// relationships:
//   implements: command-line-interface
// ---
//! The driver routes of the flow node against the binary: the `apply` stop
//! notice and exit code, the abort staged-record removal (behaviors 8/9/19),
//! the abort removal I/O-failure surface (behavior 10), the sole
//! `canonical_target` removal key (behavior 11), and the flow dry-run / CLI
//! `--dry-run` union (behavior 13).
#![allow(clippy::zombie_processes)]

#[allow(dead_code)]
mod support;

use indexmap::IndexMap;
use serde_json::Value;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};
use toha::staging::{StagedRecord, Store, canonical_target};

struct Case {
    state: tempfile::TempDir,
    target: tempfile::TempDir,
    template: PathBuf,
    _template_dir: tempfile::TempDir,
}

impl Case {
    /// A case whose template contains `interview` nodes and writes one file.
    fn new(interview: &str) -> Self {
        let template_dir = tempfile::tempdir().unwrap();
        let root = template_dir.path();
        std::fs::create_dir(root.join("template")).unwrap();
        // The source subdir renders by default; `name` may be skipped, so guard
        // the interpolation to keep the render total.
        std::fs::write(
            root.join("template/hello.txt"),
            "greeting {{ name or 'anon' }}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("template.yml"),
            format!("name: sample\ninterview:\n{interview}"),
        )
        .unwrap();
        Self {
            state: tempfile::tempdir().unwrap(),
            target: tempfile::tempdir().unwrap(),
            template: root.to_path_buf(),
            _template_dir: template_dir,
        }
    }

    fn command(&self) -> Command {
        let mut command = support::isolated_command(self.state.path());
        command.env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]");
        command
    }

    fn target(&self) -> &str {
        self.target.path().to_str().unwrap()
    }

    fn template(&self) -> &str {
        self.template.to_str().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Runs with `input` on standard input (for an `--answers -` document).
    fn send(&self, args: &[&str], input: &str) -> Output {
        let mut child = self
            .command()
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

    fn wrote_file(&self) -> bool {
        self.target.path().join("hello.txt").exists()
    }

    /// Writes a staged record for this case's template and target directly,
    /// so `apply <target>` resumes it without a preceding save.
    fn seed_record(&self, submissions: Vec<IndexMap<String, Value>>) {
        let store = Store::new(support::staged_dir(self.state.path()));
        let target = canonical_target(self.target.path()).unwrap();
        let record = StagedRecord::new(
            &target,
            self.template().to_string(),
            String::new(),
            false,
            "2026-01-02T03:04:05+00:00[UTC]".into(),
            submissions,
        );
        store.save(&target, &record).unwrap();
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const STOP_GATE: &str = "  - { id: proceed, type: confirm, prompt: Ready? }\n  - flow: stop\n    when: \"not proceed\"\n    label: declined at gate\n  - { id: name, type: text, prompt: Name? }\n";

const ABORT_GATE: &str = "  - { id: cancel, type: confirm, prompt: Cancel? }\n  - flow: abort\n    when: cancel\n  - { id: name, type: text, prompt: Name? }\n";

#[test]
fn apply_answers_stop_emits_the_ended_document_and_writes_nothing() {
    // P1/D1: a headless `apply --answers` reports a stop with the additive
    // `ended` document on stdout (so an agent tells it from a completion or an
    // abort) and the stderr notice, exit 0, nothing written.
    let case = Case::new(STOP_GATE);
    let output = case.send(
        &["apply", case.template(), case.target(), "--answers", "-"],
        r#"{"proceed": false}"#,
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not the ended document: {e}\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(document["status"], "ended");
    assert_eq!(document["kind"], "stop");
    assert_eq!(document["label"], "declined at gate");
    assert!(
        stderr(&output).contains("stopped: declined at gate"),
        "{}",
        stderr(&output)
    );
    assert!(!case.wrote_file(), "stop wrote a file");
    assert!(!case.staged(), "a fresh apply stages nothing");
}

#[test]
fn apply_answers_abort_emits_the_ended_document_with_kind_abort() {
    // The abort kind is distinguishable on the wire from a stop.
    let case = Case::new(ABORT_GATE);
    let output = case.send(
        &["apply", case.template(), case.target(), "--answers", "-"],
        r#"{"cancel": true}"#,
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["status"], "ended");
    assert_eq!(document["kind"], "abort");
    assert!(!case.wrote_file());
}

#[test]
fn apply_proceeds_and_writes_when_the_stop_is_not_triggered() {
    let case = Case::new(STOP_GATE);
    let output = case.send(
        &["apply", case.template(), case.target(), "--answers", "-"],
        r#"{"proceed": true, "name": "Ada"}"#,
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(case.wrote_file(), "proceed did not write");
    assert_eq!(
        std::fs::read_to_string(case.target.path().join("hello.txt")).unwrap(),
        "greeting Ada\n"
    );
}

#[test]
fn abort_via_continue_removes_the_staged_record() {
    // Behaviors 8 + 19: stage, then continue with the abort answer. The record
    // is removed, and a later continue finds nothing staged.
    let case = Case::new(ABORT_GATE);
    let staged = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_eq!(code(&staged), 4, "{}", stderr(&staged));
    assert!(case.staged(), "stage did not persist a record");

    let ended = case.send(&["continue", case.target(), "-"], r#"{"cancel": true}"#);
    assert_eq!(code(&ended), 0, "{}", stderr(&ended));
    let document: Value = serde_json::from_slice(&ended.stdout).unwrap();
    assert_eq!(document["status"], "ended");
    assert_eq!(document["kind"], "abort");
    assert!(!case.staged(), "abort did not remove the staged record");

    let after = case.run(&["continue", case.target()]);
    assert_ne!(code(&after), 0);
    assert!(
        stderr(&after).contains("no interview is staged"),
        "{}",
        stderr(&after)
    );
}

#[test]
fn abort_on_a_fresh_apply_is_a_no_op_removal_without_error() {
    // Behavior 9: a fresh apply has no staged record; the abort removal returns
    // false and does not error.
    let case = Case::new(ABORT_GATE);
    let output = case.send(
        &["apply", case.template(), case.target(), "--answers", "-"],
        r#"{"cancel": true}"#,
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).contains("aborted"), "{}", stderr(&output));
    assert!(!case.wrote_file());
    assert!(!case.staged());
}

#[cfg(unix)]
#[test]
fn abort_removal_io_failure_surfaces_nonzero() {
    // Behavior 10: when `Store::remove` cannot delete the record, the failure
    // surfaces and exits nonzero. A seeded record replays straight to the
    // abort, so `apply` reaches the removal with no preceding write; a
    // read-only staged directory then makes the unlink fail.
    use std::os::unix::fs::PermissionsExt;
    let case = Case::new(ABORT_GATE);
    case.seed_record(vec![IndexMap::from([("cancel".into(), Value::Bool(true))])]);
    assert!(case.staged());
    let dir = support::staged_dir(case.state.path());
    let original = std::fs::metadata(&dir).unwrap().permissions();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    // `apply <target>` resumes the staged interview and reaches the abort's
    // removal directly.
    let output = case.run(&["apply", case.target()]);
    // Restore before asserting so the tempdir can be cleaned up.
    std::fs::set_permissions(&dir, original).unwrap();
    assert_ne!(
        code(&output),
        0,
        "abort removal I/O failure did not surface: {}",
        stderr(&output)
    );
}

#[test]
fn abort_removes_the_record_under_the_canonical_key() {
    // Behavior 11: the removal uses the sole `canonical_target` identity, so an
    // abort reached through a non-canonical spelling of the target (a trailing
    // `/.`) still removes the record stored under the canonical key.
    let case = Case::new(ABORT_GATE);
    let staged = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_eq!(code(&staged), 4, "{}", stderr(&staged));
    let noisy = format!("{}/.", case.target());
    let ended = case.send(&["continue", &noisy, "-"], r#"{"cancel": true}"#);
    assert_eq!(code(&ended), 0, "{}", stderr(&ended));
    assert!(
        !case.staged(),
        "abort via a non-canonical path did not remove the canonical record"
    );
}

#[test]
fn a_rejecting_and_aborting_continue_keeps_the_staged_record() {
    // Behavior 21 at the driver: the abort is triggered by a *valid* answer
    // (`cancel`), so the tentative probe genuinely fires it, while a *separate*
    // answer (`code`) in the same document is invalid. The rejection wins: exit
    // 4 with the `code` error, and the staged record is never removed — the
    // removal runs only after a committed end, not from the probe.
    let interview = "  - { id: cancel, type: confirm, prompt: Cancel? }\n  - { id: code, type: text, prompt: Code?, validate: { min: 2 } }\n  - flow: abort\n    when: cancel\n";
    let case = Case::new(interview);
    let staged = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_eq!(code(&staged), 4, "{}", stderr(&staged));
    assert!(case.staged());

    let rejected = case.send(
        &["continue", case.target(), "-"],
        r#"{"cancel": true, "code": "x"}"#,
    );
    assert_eq!(code(&rejected), 4, "{}", stderr(&rejected));
    assert!(
        case.staged(),
        "a rejected document removed the staged record"
    );
}

#[test]
fn answers_after_a_flow_stop_are_refused() {
    // P2/behavior 23: a stop keeps the staged record. A later `continue` with
    // answers is a submission after a terminal interview, so it is refused
    // (exit 1) and the record is kept.
    let case = Case::new(STOP_GATE);
    let staged = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_eq!(code(&staged), 4, "{}", stderr(&staged));
    let stopped = case.send(&["continue", case.target(), "-"], r#"{"proceed": false}"#);
    assert_eq!(code(&stopped), 0, "{}", stderr(&stopped));
    let document: Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(document["status"], "ended");
    assert!(case.staged(), "a stop must keep the staged record");

    let refused = case.send(&["continue", case.target(), "-"], r#"{"name": "late"}"#);
    assert_ne!(code(&refused), 0, "answers after a stop were not refused");
    assert!(
        stderr(&refused).contains("not used"),
        "{}",
        stderr(&refused)
    );
    assert!(case.staged(), "the refusal must keep the record");
}

#[test]
fn apply_answers_after_a_flow_stop_is_refused_but_no_answers_shows_the_notice() {
    // R2/behavior 23: the terminal-interview refusal holds on *every* answers
    // route, not just `continue`. `apply TEMPLATE PATH --answers` on a staged
    // interview a flow node already ended is refused (exit 1) and keeps the
    // record; `apply TEMPLATE PATH` without answers keeps the notice (exit 0).
    let case = Case::new(STOP_GATE);
    let staged = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_eq!(code(&staged), 4, "{}", stderr(&staged));
    let stopped = case.send(&["continue", case.target(), "-"], r#"{"proceed": false}"#);
    assert_eq!(code(&stopped), 0, "{}", stderr(&stopped));
    assert!(case.staged(), "a stop must keep the staged record");

    // The named-template answers route must refuse, not exit 0 discarding them.
    let refused = case.send(
        &["apply", case.template(), case.target(), "--answers", "-"],
        r#"{"name": "late"}"#,
    );
    assert_eq!(
        code(&refused),
        1,
        "apply --answers on an ended record was not refused: {}",
        stderr(&refused)
    );
    assert!(
        stderr(&refused).contains("not used"),
        "{}",
        stderr(&refused)
    );
    assert!(refused.stdout.is_empty(), "a refusal emits no document");
    assert!(case.staged(), "the refusal must keep the record");

    // The no-answers route is unchanged: the notice, exit 0.
    let notice = case.run(&["apply", case.template(), case.target()]);
    assert_eq!(code(&notice), 0, "{}", stderr(&notice));
    assert!(
        stderr(&notice).contains("stopped: declined at gate"),
        "{}",
        stderr(&notice)
    );
}

#[test]
fn stage_on_an_ended_record_names_starting_over_not_apply() {
    // P3: guidance for a stopped staged record must not offer `apply` (which
    // would write nothing), but name starting over.
    let case = Case::new(STOP_GATE);
    case.run(&["stage", case.template(), case.target(), "--async"]);
    let stopped = case.send(&["continue", case.target(), "-"], r#"{"proceed": false}"#);
    assert_eq!(code(&stopped), 0, "{}", stderr(&stopped));
    assert!(case.staged());

    let refused = case.run(&["stage", case.template(), case.target(), "--async"]);
    assert_ne!(code(&refused), 0);
    let message = stderr(&refused);
    assert!(
        message.contains("ended by a flow node"),
        "guidance did not report the end: {message}"
    );
    assert!(
        !message.contains("toha apply"),
        "guidance must not offer apply for an ended interview: {message}"
    );
}

#[test]
fn flow_dry_run_and_cli_dry_run_compose_without_writing() {
    // Behavior 13: a flow dry-run alone, and combined with `--dry-run`, both
    // show the plan and write nothing, exit 0, neither applying twice.
    let interview = "  - { id: name, type: text, prompt: Name? }\n  - flow: dry-run\n";
    for extra in [Vec::new(), vec!["--dry-run"]] {
        let case = Case::new(interview);
        let mut args = vec!["apply", case.template(), case.target(), "--answers", "-"];
        args.extend(extra.iter().copied());
        let output = case.send(&args, r#"{"name": "Ada"}"#);
        assert_eq!(code(&output), 0, "{}", stderr(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("create hello.txt"),
            "plan not shown: {stdout}"
        );
        assert!(!case.wrote_file(), "dry-run wrote a file");
    }
}
