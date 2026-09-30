// ---
// relationships:
//   implements: architecture
// ---
//! The staged update flow through the binary on the agent route: `stage --from`
//! replays the recorded answers and asks only a new question, `continue` answers
//! it, and `apply PATH` merges the completed update from the base.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

use serde_json::Value;

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

/// A folder template with one required question and one rendered file.
fn write_one_question(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
}

/// Grow the template with a second required question and a file that renders it.
fn add_second_question(dir: &Path) {
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: color\n    type: text\n    prompt: Colour\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/color.txt"), "{{ color }}\n").unwrap();
}

fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

fn envelope(dir: &Path, name: &str, formal: &str, answers: Value) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": answers });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    path
}

/// Baseline-apply `address` (name=sample-value) into `target`, commit clean, and return
/// the snapshot id.
fn baseline(iso: &Path, address: &str, formal: &str, target: &Path, trust: bool) -> String {
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
    if trust {
        command.arg("--trust");
    }
    let document = support::first_document(&command.output().unwrap().stdout);
    let id = document["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target, &["add", "."]);
    git(target, &["commit", "--quiet", "-m", "baseline"]);
    id
}

#[cfg(unix)]
#[test]
fn a_person_apply_from_prompts_a_new_question_then_merges() {
    use expectrl::{Expect, Session};
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);
    add_second_question(template_dir.path());

    // `apply ADDRESS TARGET --from ID` at a terminal prompts the new question and
    // merges the update when it is answered.
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot);
    // The batch the replay cannot complete carries both questions, so the person
    // is prompted for `name` (with the recorded value as the default) and the new
    // `color`. Accepting the default keeps the name; the new answer settles it.
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name").unwrap();
    session.send_line("sample-value").unwrap();
    session.expect("Colour").unwrap();
    session.send_line("blue").unwrap();
    session.expect(expectrl::Eof).unwrap();

    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );
}

#[cfg(unix)]
#[test]
fn a_person_stage_prompts_saves_each_batch_then_applies() {
    use expectrl::{Expect, Session};
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);
    add_second_question(template_dir.path());

    // `stage ADDRESS TARGET --from ID` at a terminal (no --async) is the person
    // route: it prompts the unsettled questions and saves the staged update.
    let mut command = support::isolated_command(iso.path());
    command
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot);
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name").unwrap();
    session.send_line("sample-value").unwrap();
    session.expect("Colour").unwrap();
    session.send_line("blue").unwrap();
    session.expect(expectrl::Eof).unwrap();

    // The person staged the update; apply merges it from the base.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let output = apply.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "apply: {output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
}

#[test]
fn stage_trust_environment_is_carried_through_continue_and_apply() {
    // An update whose new version references a captured environment value stages
    // with --trust, capturing the value once. The continue and apply that follow
    // run with a different USER, yet the merged output carries the value captured
    // at stage, proving the grant and its captured decision carry, not a re-read.
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);

    // The new version adds a question and a file that renders the captured user.
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
    std::fs::write(
        template_dir.path().join("template/who.txt"),
        "{{ toha_env_user }}\n",
    )
    .unwrap();

    // Stage with --trust, capturing the environment user as `staged-user`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust")
        .arg("--async")
        .env("USER", "staged-user");
    assert_eq!(
        stage.output().unwrap().status.code(),
        Some(4),
        "stage should ask"
    );

    // Continue and apply under a different USER; neither re-captures.
    let env_color = envelope(
        iso.path(),
        "c.json",
        &formal,
        serde_json::json!({ "color": "blue" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue")
        .arg(target.path())
        .arg(&env_color)
        .env("USER", "continue-user");
    assert_eq!(cont.output().unwrap().status.code(), Some(0));

    let mut apply = support::isolated_command(iso.path());
    apply
        .arg("apply")
        .arg(target.path())
        .env("USER", "apply-user");
    let output = apply.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "apply: {output:?}");

    // The rendered file carries the value captured at stage, not a later USER.
    assert_eq!(
        std::fs::read_to_string(target.path().join("who.txt")).unwrap(),
        "staged-user\n",
        "the captured stage environment must carry through continue and apply"
    );
}

#[test]
fn stage_baseline_then_continue_and_apply_records_a_snapshot() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());

    // Stage a baseline update: there is no recorded answer, so it asks `name`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--async");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "stage: {staged:?}");
    let document = support::first_document(&staged.stdout);
    assert!(
        document["schema"]["properties"]["name"].is_object(),
        "{document}"
    );

    // Answer it and apply: the whole render is merged from empty, saving a snapshot.
    let env = envelope(
        iso.path(),
        "n.json",
        &formal,
        serde_json::json!({ "name": "other-value" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env);
    assert_eq!(cont.output().unwrap().status.code(), Some(0));

    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let document = support::first_document(&apply.output().unwrap().stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello other-value\n"
    );
}

#[test]
fn stage_reanswer_re_asks_a_recorded_answer() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);

    // `--reanswer` offers `name` again even though the base answered it.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--reanswer")
        .arg("--async");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "reanswer stage: {staged:?}");
    let document = support::first_document(&staged.stdout);
    assert_eq!(
        document["schema"]["properties"]["name"]["default"], "sample-value",
        "{document}"
    );

    // Answer with a new value and apply: the greeting changes.
    let env = envelope(
        iso.path(),
        "b.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env);
    assert_eq!(cont.output().unwrap().status.code(), Some(0));
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let document = support::first_document(&apply.output().unwrap().stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello revised-value\n"
    );
}

#[test]
fn stage_refuses_before_any_batch_when_env_needs_trust() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // A template that references a captured environment value.
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(
        template_dir.path().join("template/greeting.txt"),
        "Hello {{ name }} from {{ toha_env_user }}\n",
    )
    .unwrap();
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), true);

    // Without --trust the stage refuses before asking anything.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--async");
    let output = stage.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "env stage should refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("trust"),
        "expected a trust refusal: {stderr}"
    );
    // Nothing was staged.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let applied = apply.output().unwrap();
    let stderr = String::from_utf8_lossy(&applied.stderr);
    assert!(
        stderr.contains("git") || stderr.contains("staged") || stderr.contains("nothing"),
        "no update staged: {stderr}"
    );
}

#[test]
fn a_continue_with_a_foreign_identity_leaves_the_staged_update_unchanged() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);
    add_second_question(template_dir.path());

    // Stage the update (asks `color`).
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4));

    // A continue naming a foreign template is rejected; the record is unchanged.
    let wrong = envelope(
        iso.path(),
        "wrong.json",
        "not-the-template",
        serde_json::json!({ "color": "blue" }),
    );
    let mut bad = support::isolated_command(iso.path());
    bad.arg("continue").arg(target.path()).arg(&wrong);
    let output = bad.output().unwrap();
    assert_ne!(
        output.status.code(),
        Some(0),
        "foreign identity should refuse"
    );
    assert!(!target.path().join("color.txt").exists(), "nothing applied");

    // The staged update still answers the right document afterward.
    let right = envelope(
        iso.path(),
        "right.json",
        &formal,
        serde_json::json!({ "color": "blue" }),
    );
    let mut good = support::isolated_command(iso.path());
    good.arg("continue").arg(target.path()).arg(&right);
    assert_eq!(
        good.output().unwrap().status.code(),
        Some(0),
        "the record survived"
    );
}

#[test]
fn agent_stages_continues_and_applies_a_staged_update() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());

    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());

    // Baseline the first version and commit it clean.
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

    // The template grows a required question the base never answered.
    add_second_question(template_dir.path());

    // Stage the update: it replays `name` and asks only `color`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--async");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "stage: {staged:?}");
    let document = support::first_document(&staged.stdout);
    assert_eq!(document["status"], "questions", "{document}");
    assert!(
        document["schema"]["properties"]["color"].is_object(),
        "the staged batch asks for colour: {document}"
    );
    // The recorded answer is the default, not re-asked.
    assert!(
        document["schema"]["properties"]["name"].is_null()
            || document["schema"]["properties"]["name"]["default"] == "sample-value",
        "{document}"
    );

    // Answer the new question through the agent continue route.
    let env_color = envelope(
        iso.path(),
        "color.json",
        &formal,
        serde_json::json!({ "color": "blue" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env_color);
    let continued = cont.output().unwrap();
    assert_eq!(continued.status.code(), Some(0), "continue: {continued:?}");

    // Apply the completed staged update: it merges from the base.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let applied = apply.output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "apply: {applied:?}");
    let document = support::first_document(&applied.stdout);
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}
