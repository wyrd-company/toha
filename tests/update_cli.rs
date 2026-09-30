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
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let output = command.output().unwrap();
    assert!(output.status.success(), "baseline failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "applied");
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Alice\n"
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
        .arg(&env_alice);
    let output = same.output().unwrap();
    assert!(
        output.status.success(),
        "already-current failed: {output:?}"
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "already-current", "{document}");

    // 3. An unrelated local edit is committed, then an update with a changed
    //    answer merges: the greeting is rewritten and the unrelated edit, held on
    //    HEAD (the merge's operator side), survives.
    std::fs::write(target.path().join("notes.txt"), "local\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "local notes"]);
    let env_bob = envelope(
        iso.path(),
        "bob.json",
        &formal,
        serde_json::json!({ "name": "Bob" }),
    );
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_bob);
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Bob\n"
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );

    // Record the first snapshot, then commit it clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // A dry-run update reports the plan and changes nothing: the greeting still
    // holds the base answer and no second snapshot is recorded.
    let env_bob = envelope(
        iso.path(),
        "bob.json",
        &formal,
        serde_json::json!({ "name": "Bob" }),
    );
    let mut dry = support::isolated_command(iso.path());
    dry.arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_bob)
        .arg("--dry-run");
    let output = dry.output().unwrap();
    assert!(output.status.success(), "dry-run failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "planned", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Alice\n",
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );

    // Record a baseline with the one-question template, then commit clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
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
        .arg(&env_alice);
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
    assert_eq!(properties["name"]["default"], "Alice", "{document}");
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );

    // Record a baseline with the one-question template, then commit clean.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
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
        .arg(&env_alice);
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["update"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "green\n",
        "the configured preset must answer the new question"
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );

    // Baseline at a fixed instant. The stamp records that date.
    let mut baseline = support::isolated_command(iso.path());
    baseline
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice)
        .env("TOHA_NOW", "2020-06-15T12:00:00+00:00[UTC]");
    let document = support::first_document(&baseline.output().unwrap().stdout);
    let snapshot = document["snapshot"].as_str().unwrap().to_owned();
    assert_eq!(
        std::fs::read_to_string(target.path().join("stamp.txt")).unwrap(),
        "Alice 2020-06-15\n"
    );
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // Update the answer at a different wall clock. The date bytes must not move:
    // the update renders from the carried frozen instant, not the runtime clock.
    let env_bob = envelope(
        iso.path(),
        "bob.json",
        &formal,
        serde_json::json!({ "name": "Bob" }),
    );
    let mut update = support::isolated_command(iso.path());
    update
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_bob)
        .env("TOHA_NOW", "2026-09-30T00:00:00+00:00[UTC]");
    let output = update.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("stamp.txt")).unwrap(),
        "Bob 2020-06-15\n",
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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );

    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--answers")
        .arg(&env_alice);
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "an untrusted hooked update must refuse"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--trust"), "unexpected: {stderr}");
    assert!(
        !target.path().join("greeting.txt").exists(),
        "an untrusted update must write nothing"
    );
}
