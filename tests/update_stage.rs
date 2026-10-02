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
    git(dir, &["config", "core.autocrlf", "false"]);
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
    let text = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
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
        .env("USER", "staged-user")
        .env("USERNAME", "staged-user");
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
        .env("USER", "continue-user")
        .env("USERNAME", "continue-user");
    assert_eq!(cont.output().unwrap().status.code(), Some(0));

    let mut apply = support::isolated_command(iso.path());
    apply
        .arg("apply")
        .arg(target.path())
        .env("USER", "apply-user")
        .env("USERNAME", "apply-user");
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
    let text = String::from_utf8(apply.output().unwrap().stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
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
    let text = String::from_utf8(apply.output().unwrap().stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
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
    let text = String::from_utf8(applied.stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );

    assert!(
        staged_files(iso.path()).is_empty(),
        "successful apply must remove the staged record"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}

#[test]
fn the_final_agent_apply_path_refuses_a_dirty_target() {
    // The resume path re-checks cleanliness so `apply PATH` never merges onto a
    // dirty tree, even though the stage was recorded when the tree was clean.
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

    // Stage and complete an update while the tree is clean.
    add_second_question(template_dir.path());
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4), "stage");
    let env_color = envelope(
        iso.path(),
        "color.json",
        &formal,
        serde_json::json!({ "color": "blue" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env_color);
    assert_eq!(cont.output().unwrap().status.code(), Some(0), "continue");

    // The operator dirties the target after staging but before applying.
    std::fs::write(target.path().join("greeting.txt"), "operator is editing\n").unwrap();

    // `apply PATH` refuses the dirty target and writes nothing.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path());
    let applied = apply.output().unwrap();
    assert_ne!(
        applied.status.code(),
        Some(0),
        "apply must refuse a dirty target: {applied:?}"
    );
    assert!(
        String::from_utf8_lossy(&applied.stderr).contains("uncommitted changes")
            || String::from_utf8_lossy(&applied.stdout).contains("uncommitted changes"),
        "names the dirty target: {}{}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
    // The operator's edit is untouched and no merge output (color.txt) appeared.
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "operator is editing\n"
    );
    assert!(!target.path().join("color.txt").exists(), "nothing written");
}

/// Every tracked worktree file, the staged record, every git ref, and the git
/// index, captured so a dry-run preview can be proved to leave them untouched.
#[derive(Debug, PartialEq)]
struct State {
    worktree: Vec<(String, Vec<u8>)>,
    staged: Vec<(String, Vec<u8>)>,
    refs: String,
    index: Vec<u8>,
}

fn capture_state(iso: &Path, target: &Path) -> State {
    State {
        worktree: tracked_files(target),
        staged: staged_files(iso),
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

fn staged_files(iso: &Path) -> Vec<(String, Vec<u8>)> {
    let dir = support::staged_dir(iso);
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).unwrap();
            (name, bytes)
        })
        .collect();
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
fn changed_paths(text: &str) -> Vec<String> {
    let mut paths: Vec<String> = text
        .lines()
        .filter_map(|line| {
            [
                "Added ",
                "Updated ",
                "Merged ",
                "Deleted ",
                "Conflicted ",
                "Not previewed ",
            ]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
            .map(|path| path.split(" (").next().unwrap().to_owned())
        })
        .collect();
    paths.sort();
    paths
}

/// A completed staged update (`apply --from ID` staged and completed) previews
/// under `apply PATH --dry-run`: the document reports `planned`, the hook does
/// not run, and the target, index, refs, and staged record are byte-identical
/// before and after. The real apply that follows still merges, runs the hook,
/// and consumes the staged interview.
#[test]
fn apply_dry_run_on_a_completed_staged_update_previews_and_preserves_everything() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);

    // A generic, user-owned file the template never names, committed before
    // the update is staged: a real three-way merge, not an empty target, must
    // carry it through both the preview and the real apply.
    std::fs::write(target.path().join("notes.txt"), "user notes\n").unwrap();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "user notes"]);

    // The new version adds a second question and a trusted hook. One hook
    // writes `ran.txt` inside the checkout (so a run shows up in the plan's
    // own `merge.changes`); the other touches an absolute marker entirely
    // outside the target and the throwaway checkout, so its presence proves a
    // hook process actually ran, wherever its cwd was — a target-relative
    // marker alone cannot distinguish "no hook ran" from "a hook ran in the
    // scratch checkout and nothing merged to the target".
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("hook-ran.marker");
    std::fs::write(
        template_dir.path().join("template.yml"),
        format!(
            "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n  - id: color\n    type: text\n    prompt: Colour\n    required: true\nhooks:\n  - run: [ touch, ran.txt ]\n  - run: [ touch, {:?} ]\n",
            marker
        ),
    )
    .unwrap();
    std::fs::write(
        template_dir.path().join("template/color.txt"),
        "{{ color }}\n",
    )
    .unwrap();

    // Stage with --trust so the hook is trusted; the replay asks only `color`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust")
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4), "stage");

    // Answer the new question through the agent continue route, completing the
    // staged update.
    let env_color = envelope(
        iso.path(),
        "color.json",
        &formal,
        serde_json::json!({ "color": "blue" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env_color);
    assert_eq!(cont.output().unwrap().status.code(), Some(0), "continue");

    let before = capture_state(iso.path(), target.path());

    // `apply PATH --dry-run --trust` must preview the merge without writing
    // anything. `--trust` re-asserts the hook trust established at stage, the
    // same as the real apply that follows would need.
    let mut dry = support::isolated_command(iso.path());
    dry.arg("apply")
        .arg(target.path())
        .arg("--dry-run")
        .arg("--trust");
    let dry_output = dry.output().unwrap();
    assert_eq!(dry_output.status.code(), Some(0), "dry-run: {dry_output:?}");
    let text = String::from_utf8(dry_output.stdout.clone()).unwrap();
    assert!(text.contains("Preview only; no changes written."), "{text}");
    // The plan is accurate: the new file the template would render is listed,
    // and no hook-written path leaks into it (the hook never ran).
    assert_eq!(
        changed_paths(&text),
        vec!["color.txt".to_owned()],
        "the preview must list the rendered file and nothing a hook would write: {text}"
    );

    let after_dry_run = capture_state(iso.path(), target.path());
    assert_eq!(
        before, after_dry_run,
        "a dry-run preview must leave the target, index, refs, and staged state untouched"
    );
    assert!(
        !target.path().join("ran.txt").exists(),
        "a dry run must not run hooks"
    );
    assert!(
        !marker.exists(),
        "a dry run must not run hooks, proved by an absolute marker outside the target \
         and the throwaway checkout"
    );
    // The committed user edit is untouched by the preview, named explicitly
    // (the byte-identical worktree check above already covers it structurally).
    assert_eq!(
        std::fs::read_to_string(target.path().join("notes.txt")).unwrap(),
        "user notes\n",
        "a dry-run preview must not touch the committed user file"
    );

    // The real apply that follows is still available: it merges, runs the hook,
    // and consumes the staged interview.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path()).arg("--trust");
    let applied = apply.output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "apply: {applied:?}");
    let text = String::from_utf8(applied.stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("color.txt")).unwrap(),
        "blue\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );
    assert!(
        target.path().join("ran.txt").exists(),
        "the real apply runs the trusted hook"
    );
    assert!(
        marker.exists(),
        "the real apply runs the trusted hook, proved by the absolute marker"
    );
    // The committed user edit survives the real three-way merge too, not just
    // the preview.
    assert_eq!(
        std::fs::read_to_string(target.path().join("notes.txt")).unwrap(),
        "user notes\n",
        "the real apply must preserve the committed user file across the merge"
    );

    assert!(
        staged_files(iso.path()).is_empty(),
        "successful apply must remove the staged record"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}

/// A completed staged baseline (`stage --baseline` staged and completed, merged
/// from an empty base) previews the same way: `apply PATH --dry-run` reports
/// `planned`, writes nothing, and leaves the staged record in place for the
/// real apply that follows.
#[test]
fn apply_dry_run_on_a_completed_staged_baseline_previews_and_preserves_everything() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // A hook-bearing template, plus an absolute marker outside the target and
    // the throwaway checkout, so a hook run is observable regardless of where
    // its cwd was.
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("hook-ran.marker");
    std::fs::write(
        template_dir.path().join("template.yml"),
        format!(
            "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\nhooks:\n  - run: [ touch, ran.txt ]\n  - run: [ touch, {:?} ]\n",
            marker
        ),
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

    // Stage a trusted baseline: there is no recorded answer, so it asks `name`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--baseline")
        .arg("--trust")
        .arg("--async");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "stage: {staged:?}");

    // Answer it, completing the staged baseline.
    let env = envelope(
        iso.path(),
        "n.json",
        &formal,
        serde_json::json!({ "name": "other-value" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env);
    assert_eq!(cont.output().unwrap().status.code(), Some(0), "continue");

    let before = capture_state(iso.path(), target.path());

    // `apply PATH --dry-run --trust` previews the staged baseline merge without
    // writing.
    let mut dry = support::isolated_command(iso.path());
    dry.arg("apply")
        .arg(target.path())
        .arg("--dry-run")
        .arg("--trust");
    let dry_output = dry.output().unwrap();
    assert_eq!(dry_output.status.code(), Some(0), "dry-run: {dry_output:?}");
    let text = String::from_utf8(dry_output.stdout.clone()).unwrap();
    assert!(text.contains("Preview only; no changes written."), "{text}");
    assert_eq!(
        changed_paths(&text),
        vec!["greeting.txt".to_owned()],
        "the preview must list the rendered file and nothing a hook would write: {text}"
    );

    let after_dry_run = capture_state(iso.path(), target.path());
    assert_eq!(
        before, after_dry_run,
        "a dry-run preview of a staged baseline must leave the target, index, refs, \
         and staged state untouched"
    );
    assert!(
        !target.path().join("ran.txt").exists(),
        "a dry run must not run hooks"
    );
    assert!(
        !marker.exists(),
        "a dry run must not run hooks, proved by an absolute marker outside the target \
         and the throwaway checkout"
    );
    assert!(
        !target.path().join("greeting.txt").exists(),
        "a dry run writes nothing"
    );

    // The real apply that follows still works and consumes the staged state.
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(target.path()).arg("--trust");
    let applied = apply.output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "apply: {applied:?}");
    let text = String::from_utf8(applied.stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello other-value\n"
    );
    assert!(
        target.path().join("ran.txt").exists(),
        "the real apply runs the trusted hook"
    );
    assert!(
        marker.exists(),
        "the real apply runs the trusted hook, proved by the absolute marker"
    );

    assert!(
        staged_files(iso.path()).is_empty(),
        "successful apply must remove the staged record"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}

/// A completed staged update whose answers trigger the template's own
/// `flow: dry-run` node previews on a plain `apply PATH` — no CLI `--dry-run`
/// flag needed: `planned`, the target/index/refs/staged state byte-identical
/// before and after, and no hook execution. `abort` still clears the staged
/// record (stop/abort is preserved), and restaging with answers that no
/// longer trigger the flow node still applies for real and runs the hook.
#[test]
fn flow_dry_run_on_a_completed_staged_update_previews_and_preserves_staged_state() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_one_question(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let snapshot = baseline(iso.path(), &address, &formal, target.path(), false);

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

    // Stage with --trust so the hook is trusted; the replay asks only `mode`.
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust")
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4), "stage");

    // Answer `mode: preview`, firing the flow dry-run, completing the staged
    // update.
    let env_preview = envelope(
        iso.path(),
        "preview.json",
        &formal,
        serde_json::json!({ "mode": "preview" }),
    );
    let mut cont = support::isolated_command(iso.path());
    cont.arg("continue").arg(target.path()).arg(&env_preview);
    assert_eq!(cont.output().unwrap().status.code(), Some(0), "continue");

    let before = capture_state(iso.path(), target.path());

    // `apply PATH --trust`, with no CLI `--dry-run` flag, previews because the
    // template's own flow node requested it.
    let mut preview = support::isolated_command(iso.path());
    preview.arg("apply").arg(target.path()).arg("--trust");
    let preview_output = preview.output().unwrap();
    assert_eq!(
        preview_output.status.code(),
        Some(0),
        "preview: {preview_output:?}"
    );
    let text = String::from_utf8(preview_output.stdout.clone()).unwrap();
    assert!(text.contains("Preview only; no changes written."), "{text}");
    // This update renders no new file; the plan must stay empty rather than
    // leak a hook-written path into it.
    assert_eq!(
        changed_paths(&text),
        Vec::<String>::new(),
        "the preview must not include anything a hook would write: {text}"
    );

    let after_preview = capture_state(iso.path(), target.path());
    assert_eq!(
        before, after_preview,
        "a flow dry-run must leave the target, index, refs, and staged state untouched"
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

    // Stop/abort is preserved: aborting the staged interview clears it.
    let mut abort = support::isolated_command(iso.path());
    abort.arg("abort").arg(target.path());
    assert_eq!(abort.output().unwrap().status.code(), Some(0), "abort");

    // Restaging with answers that no longer trigger the flow node still
    // applies for real: the real apply remains available.
    let mut restage = support::isolated_command(iso.path());
    restage
        .arg("stage")
        .arg(&address)
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--trust")
        .arg("--async");
    assert_eq!(restage.output().unwrap().status.code(), Some(4), "restage");
    let env_apply = envelope(
        iso.path(),
        "apply.json",
        &formal,
        serde_json::json!({ "mode": "apply" }),
    );
    let mut cont2 = support::isolated_command(iso.path());
    cont2.arg("continue").arg(target.path()).arg(&env_apply);
    assert_eq!(cont2.output().unwrap().status.code(), Some(0), "continue 2");

    let mut real = support::isolated_command(iso.path());
    real.arg("apply").arg(target.path()).arg("--trust");
    let applied = real.output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "apply: {applied:?}");
    let text = String::from_utf8(applied.stdout.clone()).unwrap();
    assert!(text.contains("Saved snapshot "), "{text}");
    assert!(!text.contains("\"protocol\""), "{text}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hello sample-value\n"
    );
    assert!(
        target.path().join("ran.txt").exists(),
        "the real apply runs the trusted hook"
    );
    assert!(
        marker.exists(),
        "the real apply runs the trusted hook, proved by the absolute marker"
    );

    assert!(
        staged_files(iso.path()).is_empty(),
        "successful apply must remove the staged record"
    );

    // The staged record is consumed; a second apply finds nothing staged.
    let mut again = support::isolated_command(iso.path());
    again.arg("apply").arg(target.path());
    let output = again.output().unwrap();
    assert_ne!(output.status.code(), Some(0), "the record should be gone");
}
