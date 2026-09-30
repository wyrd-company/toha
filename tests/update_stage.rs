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
    let env_alice = envelope(
        iso.path(),
        "alice.json",
        &formal,
        serde_json::json!({ "name": "Alice" }),
    );
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
            || document["schema"]["properties"]["name"]["default"] == "Alice",
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
    assert_eq!(document["update"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello Alice\n"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}
