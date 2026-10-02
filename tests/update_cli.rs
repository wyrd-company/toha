// ---
// relationships:
//   implements: architecture
// ---
//! The template-update route through the binary: `apply --baseline` records the
//! first snapshot, `apply --from ID` re-renders and merges from it, a re-render
//! that equals the base reports already-current, and an unrelated local edit
//! survives the merge.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

use serde_json::Value;

/// Run `git ARGS...` inside `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A folder template with one text question and one rendered file.
fn write_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
}

/// A fresh git target with one commit.
fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

/// Every tracked worktree file, every git ref, and the git index, captured so
/// a preview can be proved to leave the whole target untouched, not just the
/// one file a test happens to check.
#[derive(Debug, PartialEq)]
struct FullState {
    worktree: Vec<(String, Vec<u8>)>,
    refs: String,
    index: Vec<u8>,
}

fn capture_target_state(target: &Path) -> FullState {
    FullState {
        worktree: tracked_files(target),
        refs: show_refs(target),
        index: std::fs::read(target.join(".git/index")).unwrap(),
    }
}

fn tracked_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().unwrap() == ".git" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                files.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    files
}

fn show_refs(dir: &Path) -> String {
    let output = StdCommand::new("git")
        .args(["show-ref"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// The `path` of every entry in a result document's `merge.changes`, sorted,
/// so a test can assert the plan is complete and accurate rather than merely
/// present.
fn changed_paths(document: &Value) -> Vec<String> {
    let mut paths: Vec<String> = document["merge"]["changes"]
        .as_array()
        .expect("merge.changes array")
        .iter()
        .map(|change| change["path"].as_str().expect("change path").to_owned())
        .collect();
    paths.sort();
    paths
}

/// Write the identity envelope naming `formal` around `answers`.
fn envelope(dir: &Path, name: &str, formal: &str, answers: Value) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": answers });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    path
}

#[test]
fn baseline_records_then_from_updates_and_detects_current() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());

    // 1. `apply --baseline` renders the whole template and records the first
    //    snapshot. The greeting file appears with the answered name.
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let output = command.output().unwrap();
    assert!(output.status.success(), "baseline failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied");
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );

    // Commit the applied file so the next update merges from a clean tree.
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // 2. A re-render with the same answers, from that snapshot, is already
    //    current: nothing changes.
    let mut same = support::isolated_command(iso.path());
    same.arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_first);
    let output = same.output().unwrap();
    assert!(
        output.status.success(),
        "already-current failed: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "already-current", "{document}");

    // 3. An unrelated local edit is committed, then an update with a changed
    //    answer merges: the greeting is rewritten and the unrelated edit, held on
    //    HEAD (the merge's operator side), survives.
    std::fs::write(target.path().join("notes.txt"), "local\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "local notes"]);
    let env_revised = envelope(
        iso.path(),
        "revised.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
    );
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_revised);
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello revised-value\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("notes.txt")).unwrap(),
        "local\n",
        "the unrelated local file must survive the merge"
    );

    // 4. The new snapshot is listed alongside the first.
    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let output = list.output().unwrap();
    assert!(output.status.success());
    let document = support::first_document(&output.stdout);
    assert_eq!(document["snapshots"].as_array().unwrap().len(), 2);
}

#[test]
fn dry_run_from_reports_planned_and_persists_nothing() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // Record the first snapshot, then commit it clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // A dry-run update reports the plan and changes nothing: the greeting still
    // holds the base answer and no second snapshot is recorded.
    let env_revised = envelope(
        iso.path(),
        "revised.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
    );
    let mut dry = support::isolated_command(iso.path());
    dry.arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_revised)
        .arg("--dry-run");
    let output = dry.output().unwrap();
    assert!(output.status.success(), "dry-run failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n",
        "a dry run must not rewrite the target"
    );

    let mut list = support::isolated_command(iso.path());
    list.arg("snapshots")
        .arg("list")
        .arg(target.path())
        .arg("--json");
    let document = support::first_document(&list.output().unwrap().stdout);
    assert_eq!(
        document["snapshots"].as_array().unwrap().len(),
        1,
        "a dry run must not persist a snapshot"
    );
}

#[test]
fn a_new_required_question_is_reported_as_questions() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // Record a baseline with the one-question template, then commit clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // The template grows a new required question the base never answered.
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: color\n    type: text\n    prompt: Colour\n    required: true\n",
    )
    .unwrap();

    // The update replays `name` but cannot settle `color`; the script route
    // reports it as a questions document (exit 4), never applies.
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_first);
    let output = update.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(4),
        "expected questions: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "questions", "{document}");
    let properties = &document["schema"]["properties"];
    assert!(
        properties["color"].is_object(),
        "expected color: {document}"
    );
    // The recorded answer becomes the default the route falls back to.
    assert_eq!(properties["name"]["default"], "sample-value", "{document}");
}

#[test]
fn a_configured_preset_answers_a_new_question() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // Record a baseline with the one-question template, then commit clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // The template grows a required question the base never answered, and a file
    // that renders it.
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: color\n    type: text\n    prompt: Colour\n    required: true\n",
    )
    .unwrap();
    std::fs::write(
        template_dir.path().join("template/color.txt"),
        "{{ color }}\n",
    )
    .unwrap();

    // A configured default supplies the new question, so the update completes
    // headlessly instead of asking.
    let config_path = iso.path().join("config/toha/config.yml");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        format!("template-defaults:\n  {formal:?}:\n    color: green\n"),
    )
    .unwrap();

    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_first);
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "green\n",
        "the configured preset must answer the new question"
    );
}

/// Baseline-apply `address` (answering name=sample-value) into a clean committed
/// target and return the recorded snapshot id, leaving the target committed.
fn baseline_snapshot(iso: &Path, address: &str, formal: &str, target: &Path) -> String {
    let env = envelope(
        iso,
        "first.json",
        formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--baseline")
        .arg("--answers")
        .arg(&env);
    let document = support::first_document(&command.output().unwrap().stdout);
    let id = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target, &["add", "."]);
    git(target, &["commit", "--quiet", "-m", "baseline"]);
    id
}

#[test]
fn an_update_refuses_a_dirty_target() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline_snapshot(iso.path(), &address, &formal, target.path());

    // An uncommitted change makes the target dirty; the update refuses.
    std::fs::write(target.path().join("dirty.txt"), "uncommitted\n").unwrap();
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot);
    let output = update.output().unwrap();
    assert!(!output.status.success(), "dirty update should refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("uncommitted"),
        "expected an uncommitted-changes refusal: {stderr}"
    );
}

#[test]
fn an_update_refuses_a_foreign_template_source() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    write_template(other_dir.path()); // a different folder: a different source
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline_snapshot(iso.path(), &address, &formal, target.path());

    // Naming a different template as the update's source is refused (identity 17).
    let other = support::folder_address(&other_dir.path().canonicalize().unwrap());
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(&other)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot);
    let output = update.output().unwrap();
    assert!(!output.status.success(), "foreign source should refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("source"),
        "expected a source refusal: {stderr}"
    );
}

#[test]
fn an_update_refuses_a_snapshot_from_another_target() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    // A repository with a committed subdirectory.
    std::fs::create_dir(repo.path().join("sub")).unwrap();
    std::fs::write(repo.path().join("sub/keep.txt"), "seed\n").unwrap();
    target_repo(repo.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    // Baseline into the subdirectory: the snapshot's target is `sub`.
    let snapshot = baseline_snapshot(iso.path(), &address, &formal, &repo.path().join("sub"));

    // Applying that snapshot at the repository root is a target mismatch.
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(repo.path())
        .arg("--from")
        .arg(&snapshot);
    let output = update.output().unwrap();
    assert!(!output.status.success(), "foreign target should refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("target"),
        "expected a target refusal: {stderr}"
    );
}

#[test]
fn from_an_unknown_snapshot_is_an_error() {
    let iso = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    target_repo(target.path());

    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg("00000000000000000000000000");
    let output = command.output().unwrap();
    assert!(!output.status.success(), "unknown --from should refuse");
}

#[test]
fn a_rendered_date_is_byte_stable_across_an_update() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // The render carries both the answer and a date drawn from the frozen
    // instant, so the date bytes prove the instant is carried, not re-read.
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: dated\ndescription: A dated greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(
        template_dir.path().join("template/stamp.txt"),
        "{{ name }} {{ now() | dateformat('%Y-%m-%d') }}\n",
    )
    .unwrap();
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    // Baseline at a fixed instant. The stamp records that date.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first)
        .env("TOHA_NOW", "2020-06-15T12:00:00+00:00[UTC]");
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(target.path().join("stamp.txt")).unwrap(),
        "sample-value 2020-06-15\n"
    );
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // Update the answer at a different wall clock. The date bytes must not move:
    // the update renders from the carried frozen instant, not the runtime clock.
    let env_revised = envelope(
        iso.path(),
        "revised.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
    );
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_revised)
        .env("TOHA_NOW", "2026-09-30T00:00:00+00:00[UTC]");
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("stamp.txt")).unwrap(),
        "revised-value 2020-06-15\n",
        "the date bytes must be carried from the base snapshot, not re-read"
    );
}

#[test]
fn an_update_with_untrusted_hooks_refuses_and_changes_nothing() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // A template whose render carries a hook. Without --trust the update never
    // runs it and never writes.
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: hooked\ndescription: A greeting with a hook\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\nhooks:\n  - run: [ tool, call ]\n",
    )
    .unwrap();
    std::fs::write(
        template_dir.path().join("template/greeting.txt"),
        "Hello {{ name }}\n",
    )
    .unwrap();
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let output = command.output().unwrap();
    assert!(output.status.success(), "planned is exit 0: {output:?}");
    let document = support::first_document(&output.stdout);
    // An untrusted update previews the plan and writes nothing: `planned` with
    // `trusted: false`, no snapshot, and nothing applied.
    assert_eq!(document["status"], "planned", "{document}");
    assert_eq!(document["trusted"], false, "{document}");
    assert!(document["snapshot"].is_null(), "{document}");
    assert!(
        !target.path().join("greeting.txt").exists(),
        "an untrusted update must write nothing"
    );
}

/// A scripted update (`apply --from ID --answers FILE`, no CLI `--dry-run`)
/// whose answers trigger the template's own `flow: dry-run` node previews the
/// merge: `planned`, no hook, and the target, index, and refs untouched. A
/// second update whose answers no longer trigger the flow still applies for
/// real and runs the hook.
#[test]
fn flow_dry_run_on_a_scripted_update_previews_without_writing() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // The new version adds a `mode` question, a flow dry-run node gated on it,
    // and a trusted hook. An absolute marker outside the target and the
    // throwaway checkout makes a hook run observable regardless of its cwd.
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("hook-ran.marker");
    std::fs::write(
        template_dir.path().join("template.yml"),
        format!(
            "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: mode\n    type: text\n    prompt: Mode\n    required: true\n  - flow: dry-run\n    when: \"mode == 'preview'\"\nhooks:\n  - run: [ touch, ran.txt ]\n  - run: [ touch, {:?} ]\n",
            marker
        ),
    )
    .unwrap();

    let before = capture_target_state(target.path());

    // `mode: preview` fires the flow dry-run without a CLI `--dry-run` flag.
    let env_preview = envelope(
        iso.path(),
        "preview.json",
        &formal,
        serde_json::json!({ "name": "revised-value", "mode": "preview" }),
    );
    let mut preview = support::isolated_command(iso.path());
    preview
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_preview)
        .arg("--trust");
    let output = preview.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "planned", "{document}");
    // The plan is accurate: the rendered change the new answers would make is
    // listed, proving the preview reflects the real render rather than an
    // incomplete candidate.
    assert_eq!(
        changed_paths(&document),
        vec!["greeting.txt".to_owned()],
        "the preview must list the rendered file and nothing a hook would write: {document}"
    );

    let after = capture_target_state(target.path());
    assert_eq!(
        before, after,
        "a flow dry-run must leave the whole target, index, and refs untouched"
    );
    assert!(
        !target.path().join("ran.txt").exists(),
        "a flow dry-run must not run hooks"
    );
    assert!(
        !marker.exists(),
        "a flow dry-run must not run hooks, proved by an absolute marker outside the \
         target and the throwaway checkout"
    );

    // `mode: apply` no longer fires the flow node: the real apply is still
    // available and merges for real, running the hook.
    let env_apply = envelope(
        iso.path(),
        "apply.json",
        &formal,
        serde_json::json!({ "name": "revised-value", "mode": "apply" }),
    );
    let mut real = support::isolated_command(iso.path());
    real.arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_apply)
        .arg("--trust");
    let output = real.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello revised-value\n"
    );
    assert!(
        target.path().join("ran.txt").exists(),
        "the real apply runs the trusted hook"
    );
    assert!(
        marker.exists(),
        "the real apply runs the trusted hook, proved by the absolute marker"
    );
}

/// The person-route update (`apply --from ID`, interactive, no `--answers`)
/// previews the same way when the prompted answers trigger the template's own
/// `flow: dry-run` node: `planned`, the rendered change the new answer would
/// make is listed, no hook runs, and the target stays untouched.
#[cfg(unix)]
#[test]
fn flow_dry_run_on_a_person_update_previews_without_writing() {
    use expectrl::{Expect, Session};

    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env_first = envelope(
        iso.path(),
        "first.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );

    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // An absolute marker outside the target and the throwaway checkout makes a
    // hook run observable regardless of its cwd.
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("hook-ran.marker");
    std::fs::write(
        template_dir.path().join("template.yml"),
        format!(
            "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: mode\n    type: text\n    prompt: Mode\n    required: true\n  - flow: dry-run\n    when: \"mode == 'preview'\"\nhooks:\n  - run: [ touch, ran.txt ]\n  - run: [ touch, {:?} ]\n",
            marker
        ),
    )
    .unwrap();

    let before = capture_target_state(target.path());

    // The person route prompts `name` (defaulting to the recorded answer) and
    // the new `mode`; answering `mode` with `preview` fires the flow dry-run.
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust");
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name").unwrap();
    session.send_line("revised-value").unwrap();
    session.expect("Mode").unwrap();
    session.send_line("preview").unwrap();
    session.expect("Template: ").unwrap();
    session.expect("Merged greeting.txt").unwrap();
    session.expect("Preview only; no changes written.").unwrap();
    session.expect(expectrl::Eof).unwrap();
    let after = capture_target_state(target.path());
    assert_eq!(
        before, after,
        "a flow dry-run must leave the whole target, index, and refs untouched"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n",
        "a flow dry-run must not rewrite the target"
    );
    assert!(
        !target.path().join("ran.txt").exists(),
        "a flow dry-run must not run hooks"
    );
    assert!(
        !marker.exists(),
        "a flow dry-run must not run hooks, proved by an absolute marker outside the \
         target and the throwaway checkout"
    );

    // Answers that no longer trigger the flow node still apply for real
    // through the person route: the real apply remains available.
    let mut real = support::isolated_command(iso.path());
    real.arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust");
    let mut session = Session::spawn(real).unwrap();
    session.expect("Name").unwrap();
    session.send_line("revised-value").unwrap();
    session.expect("Mode").unwrap();
    session.send_line("apply").unwrap();
    session.expect("Merged greeting.txt").unwrap();
    session.expect("Saved snapshot ").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello revised-value\n"
    );
    assert!(
        target.path().join("ran.txt").exists(),
        "the real apply runs the trusted hook"
    );
    assert!(
        marker.exists(),
        "the real apply runs the trusted hook, proved by the absolute marker"
    );
}
