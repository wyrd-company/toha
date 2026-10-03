// ---
// relationships:
//   implements:
//     - interview-protocol
//     - command-line-interface
//   references:
//     - template-format
// ---
//! End-to-end CLI proofs that strengthen two Jinja-context guards (task 1099,
//! consolidating the accepted P2 ordering-witness and P3 deterministic
//! ambient-read dispositions from task 1060 PR review).
//!
//! These drive the real `toha` binary through the isolated harness, so the
//! "before progress" ordering and the recorded-snapshot-over-ambient behavior
//! are witnessed by observable effects (process exit, emitted document, staged
//! record file, rendered file bytes) rather than by an internal spy — no
//! production seam is introduced. The child's environment is fully controlled,
//! so the ambient-read kill fires regardless of the outer process's `$EDITOR`.

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

/// The isolated binary with a deterministic child environment: `EDITOR` is set
/// to a sentinel so any ambient read surfaces it, and `TOHA_NOW` is fixed.
fn toha(state: &Path) -> Command {
    let mut command = support::isolated_command(state);
    command.env("EDITOR", SENTINEL);
    command.env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]");
    command
}

const SENTINEL: &str = "SENTINEL-EDITOR-VALUE";

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

/// Whether the isolated state directory holds any staged record.
fn staged_records_exist(state: &Path) -> bool {
    let dir = support::staged_dir(state);
    fs::read_dir(&dir)
        .map(|mut entries| entries.any(|e| e.is_ok()))
        .unwrap_or(false)
}

/// P2: a no-trust stage of a template that references a fixed environment value
/// refuses before any progress. The observable conjuncts of design.md
/// "Behaviors to prove" #3 are all witnessed: no staged record is written
/// (store), no batch/complete document is emitted (interview start, render, and
/// submission), and the process refuses naming stage trust. A mutation that
/// admitted after starting the interview or after writing the record would make
/// a document or a staged file appear and fail this test. `EDITOR` is set in the
/// child, so a refusal that nonetheless read the environment would leak the
/// sentinel — it must not.
#[test]
fn stage_environment_reference_without_trust_writes_nothing_before_refusing() {
    let state = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    write(&template_dir.path().join("template.yml"), "name: sample\n");
    // The reference lives in a source-tree body, an analyzed render surface.
    write(
        &template_dir.path().join("template/note.txt"),
        "editor is {{ toha_env_editor }}\n",
    );
    let target = tempfile::tempdir().unwrap();

    let output = toha(state.path())
        .args([
            "stage",
            support::folder_address(template_dir.path()).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stage must refuse; stderr: {stderr}",
    );
    assert!(
        stderr.contains("stage trust"),
        "refusal names stage trust: {stderr}",
    );
    // No interview started, rendered, or submitted: nothing on stdout.
    assert!(
        output.stdout.is_empty(),
        "no document emitted before refusal: {}",
        String::from_utf8_lossy(&output.stdout),
    );
    // No store write occurred before admission.
    assert!(
        !staged_records_exist(state.path()),
        "no staged record written before refusal",
    );
    // The environment was not read: the sentinel never surfaces.
    assert!(
        !stderr.contains(SENTINEL),
        "environment must not be read: {stderr}"
    );
}

/// P3: a stage recorded with no environment grant keeps its unavailable
/// snapshot through a later folder edit that adds a reference. The child's
/// `EDITOR` is a known sentinel, so the kill is deterministic: correct code
/// renders the recorded (absent) value and the sentinel never appears, while a
/// replay that read the ambient environment would emit the sentinel and fail
/// this test regardless of the outer process environment.
#[test]
fn mutable_folder_no_grant_ignores_ambient_editor_on_apply() {
    let state = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    write(&template_dir.path().join("template.yml"), "name: sample\n");
    // No environment reference at stage time, so a no-trust async stage records
    // an unavailable snapshot and completes.
    let body = template_dir.path().join("template/note.txt");
    write(&body, "editor line\n");
    let target = tempfile::tempdir().unwrap();

    let staged = toha(state.path())
        .args([
            "stage",
            support::folder_address(template_dir.path()).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(
        staged.status.code(),
        Some(0),
        "no-question stage completes; stderr: {}",
        String::from_utf8_lossy(&staged.stderr),
    );
    assert!(
        staged_records_exist(state.path()),
        "a staged record is written for the completed async stage",
    );

    // Mutate the folder: a later-added reference to the fixed environment value.
    write(&body, "editor is {{ toha_env_editor }}\n");

    // Apply the staged interview. Replay restores the recorded (unavailable)
    // snapshot; it performs no ambient read even though EDITOR is set.
    let applied = toha(state.path())
        .args(["apply", target.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        applied.status.code(),
        Some(0),
        "apply of the completed staged interview succeeds; stderr: {}",
        String::from_utf8_lossy(&applied.stderr),
    );

    let rendered = fs::read_to_string(target.path().join("note.txt")).unwrap();
    assert!(
        !rendered.contains(SENTINEL),
        "the recorded unavailable snapshot is used, not the ambient EDITOR: {rendered:?}",
    );
    assert_eq!(
        rendered, "editor is \n",
        "the added reference renders the recorded absent value",
    );
}
