// ---
// relationships:
//   implements: architecture
// ---
//! A reached `git` question initializes the apply target as a git repository,
//! names `main` instead of an unset or `master` default branch, adds a non-empty
//! answer as `origin`, and stops before any write when that remote already has
//! history.

#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Output};

use serde_json::{Value, json};

const TEMPLATE: &str = "name: sample
interview:
  - id: with_repo
    type: confirm
    prompt: Create a repository?
    default: true
  - id: remote
    type: git
    prompt: Remote URL
    when: with_repo
";

fn git(dir: &Path, args: &[&str]) -> Output {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
    output
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(git(dir, args).stdout)
        .unwrap()
        .trim()
        .to_owned()
}

fn file_url(path: &Path) -> String {
    let text = path
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// One isolated run: a template, an empty target parent outside any repository,
/// and a home directory whose git config the test controls.
struct Scene {
    iso: tempfile::TempDir,
    template: tempfile::TempDir,
    work: tempfile::TempDir,
}

impl Scene {
    fn new(template_yml: &str, hook: bool) -> Self {
        let scene = Scene {
            iso: tempfile::tempdir().unwrap(),
            template: tempfile::tempdir().unwrap(),
            work: tempfile::tempdir().unwrap(),
        };
        let root = scene.template.path();
        std::fs::create_dir_all(root.join("template")).unwrap();
        let mut yml = template_yml.to_owned();
        if hook {
            // The hook fails unless the repository already exists.
            yml.push_str("hooks:\n  - run: [git, rev-parse, --git-dir]\n");
        }
        std::fs::write(root.join("template.yml"), yml).unwrap();
        std::fs::write(root.join("template/README.md"), "sample\n").unwrap();
        std::fs::create_dir_all(scene.iso.path().join("home")).unwrap();
        scene
    }

    fn global_default_branch(&self, branch: &str) {
        std::fs::write(
            self.iso.path().join("home/.gitconfig"),
            format!("[init]\n\tdefaultBranch = {branch}\n"),
        )
        .unwrap();
    }

    fn target(&self) -> PathBuf {
        self.work.path().join("project")
    }

    fn command(&self) -> StdCommand {
        let mut command = support::isolated_command(self.iso.path());
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn address(&self) -> String {
        support::folder_address(&self.template.path().canonicalize().unwrap())
    }

    /// Run a scripted `apply` with `answers` and return its exit code and
    /// document.
    fn apply(&self, answers: Value, extra: &[&str]) -> (Option<i32>, Value) {
        let document = json!({
            "template": support::formal_name(self.template.path()),
            "answers": answers,
        });
        let env = self.iso.path().join("answers.json");
        std::fs::write(&env, serde_json::to_vec(&document).unwrap()).unwrap();
        let mut command = self.command();
        command
            .arg("apply")
            .arg(self.address())
            .arg(self.target())
            .arg("--answers")
            .arg(&env)
            .args(extra);
        let output = command.output().unwrap();
        let document = support::first_document(&output.stdout);
        assert_valid(&document);
        (output.status.code(), document)
    }
}

fn assert_valid(document: &Value) {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(toha::protocol::protocol_schema())
        .expect("protocol schema builds");
    if let Err(error) = validator.validate(document) {
        panic!("document does not match the protocol schema: {error}\n{document}");
    }
}

fn empty_remote(dir: &Path) -> String {
    git(dir, &["init", "--quiet", "--bare", "empty.git"]);
    file_url(&dir.join("empty.git"))
}

fn remote_with_history(dir: &Path) -> String {
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(seed.join("README.md"), "seed\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "--quiet", "-m", "seed"]);
    git(dir, &["clone", "--quiet", "--bare", "seed", "history.git"]);
    file_url(&dir.join("history.git"))
}

#[test]
fn a_git_answer_initializes_the_target_with_origin_on_main() {
    let scene = Scene::new(TEMPLATE, true);
    let remote = empty_remote(scene.work.path());
    let (code, document) = scene.apply(json!({ "remote": remote }), &["--trust"]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(
        document["git"],
        json!({ "setup": "initialized", "branch": "main", "remote": remote }),
    );
    let target = scene.target();
    assert_eq!(
        git_stdout(&target, &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(
        git_stdout(&target, &["config", "--get", "remote.origin.url"]),
        remote
    );
    assert_eq!(
        git_stdout(&target, &["config", "--get", "remote.origin.fetch"]),
        "+refs/heads/*:refs/remotes/origin/*"
    );
    // The hook ran inside the new repository.
    assert_eq!(document["hooks"][0]["success"], true, "{document}");
    assert!(target.join("README.md").exists());
}

#[test]
fn an_outdated_master_default_becomes_main() {
    let scene = Scene::new(TEMPLATE, false);
    scene.global_default_branch("master");
    let (code, document) = scene.apply(json!({ "remote": null }), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(document["git"]["branch"], "main", "{document}");
    assert_eq!(
        git_stdout(&scene.target(), &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
}

#[test]
fn another_configured_default_branch_is_kept() {
    let scene = Scene::new(TEMPLATE, false);
    scene.global_default_branch("trunk");
    let (code, document) = scene.apply(json!({}), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(
        document["git"],
        json!({ "setup": "initialized", "branch": "trunk", "remote": null }),
    );
    assert_eq!(
        git_stdout(&scene.target(), &["symbolic-ref", "HEAD"]),
        "refs/heads/trunk"
    );
}

#[test]
fn an_empty_answer_initializes_without_a_remote() {
    let scene = Scene::new(TEMPLATE, false);
    let (code, document) = scene.apply(json!({ "remote": "  " }), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(document["git"]["remote"], Value::Null, "{document}");
    let remotes = git_stdout(&scene.target(), &["remote"]);
    assert_eq!(remotes, "");
}

#[test]
fn a_remote_with_history_stops_before_any_write() {
    let scene = Scene::new(TEMPLATE, false);
    let remote = remote_with_history(scene.work.path());
    let (code, document) = scene.apply(json!({ "remote": remote }), &[]);
    assert_eq!(code, Some(1), "{document}");
    assert_eq!(document["status"], "error");
    assert_eq!(document["kind"], "git");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("clone it first"), "{message}");
    assert!(message.contains(&remote), "{message}");
    assert!(!scene.target().exists(), "the target was written");
}

#[test]
fn an_unreachable_remote_warns_and_is_still_added() {
    let scene = Scene::new(TEMPLATE, false);
    let missing = file_url(scene.work.path()) + "/missing.git";
    let (code, document) = scene.apply(json!({ "remote": missing }), &[]);
    assert_eq!(code, Some(0), "{document}");
    let messages = support::document_messages(&document);
    assert!(
        messages
            .iter()
            .any(|m| m.starts_with("warning: could not inspect the remote")),
        "{document}"
    );
    assert_eq!(
        git_stdout(&scene.target(), &["config", "--get", "remote.origin.url"]),
        missing
    );
}

#[test]
fn a_skipped_git_question_initializes_nothing() {
    let scene = Scene::new(TEMPLATE, false);
    let (code, document) = scene.apply(json!({ "with_repo": false }), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert!(document.get("git").is_none(), "{document}");
    assert!(scene.target().join("README.md").exists());
    assert!(!scene.target().join(".git").exists());
}

#[test]
fn a_target_inside_a_repository_is_left_unchanged() {
    let scene = Scene::new(TEMPLATE, false);
    git(scene.work.path(), &["init", "--quiet"]);
    let (code, document) = scene.apply(
        json!({ "remote": "https://example.invalid/sample.git" }),
        &[],
    );
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(document["git"], json!({ "setup": "existing" }));
    assert!(!scene.target().join(".git").exists());
    let remotes = git_stdout(scene.work.path(), &["remote"]);
    assert_eq!(remotes, "");
}

#[test]
fn a_dry_run_plans_the_repository_and_writes_nothing() {
    let scene = Scene::new(TEMPLATE, false);
    let (code, document) = scene.apply(
        json!({ "remote": "https://user:secret@example.invalid/sample.git" }),
        &["--dry-run"],
    );
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(document["status"], "planned");
    assert_eq!(
        document["git"],
        json!({ "remote": "https://example.invalid/sample.git" })
    );
    assert!(!scene.target().exists());
}

#[test]
fn a_git_question_is_a_text_prompt_in_the_question_batch() {
    let scene = Scene::new(
        "name: sample\ninterview:\n  - id: remote\n    type: git\n    prompt: Remote URL\n",
        false,
    );
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    let output = stage.output().unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let document = support::first_document(&output.stdout);
    let property = &document["schema"]["properties"]["remote"];
    assert_eq!(property["x-toha-type"], "git", "{document}");
    assert_eq!(property["type"], json!(["string", "null"]), "{document}");
}

#[test]
fn the_agent_apply_route_reports_the_repository() {
    let scene = Scene::new(TEMPLATE, false);
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4));
    let env = scene.iso.path().join("answers.json");
    std::fs::write(
        &env,
        serde_json::to_vec(&json!({
            "template": support::formal_name(scene.template.path()),
            "answers": { "with_repo": true, "remote": null },
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cont = scene.command();
    cont.arg("continue").arg(scene.target()).arg(&env);
    let continued = cont.output().unwrap();
    assert!(continued.status.success(), "{continued:?}");
    let mut apply = scene.command();
    apply.arg("apply").arg(scene.target());
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    let stdout = String::from_utf8_lossy(&applied.stdout);
    assert!(
        stdout.contains("Initialized git repository on branch main"),
        "{stdout}"
    );
}

#[test]
fn a_template_with_two_git_questions_does_not_load() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: first
    type: git
    prompt: First
  - id: second
    type: git
    prompt: Second
",
        false,
    );
    let (code, document) = scene.apply(json!({}), &[]);
    assert_eq!(code, Some(1), "{document}");
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("only one git question allowed"),
        "{message}"
    );
}

#[test]
fn an_update_never_sets_up_a_repository() {
    let scene = Scene::new(TEMPLATE, false);
    // A remote with history would stop a plain apply; an update never checks it.
    let remote = remote_with_history(scene.work.path());
    let target = scene.target();
    std::fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "--quiet", "--initial-branch=main"]);
    git(&target, &["config", "core.autocrlf", "false"]);
    std::fs::write(target.join("notes.txt"), "seed\n").unwrap();
    git(&target, &["add", "."]);
    git(&target, &["commit", "--quiet", "-m", "seed"]);
    let (code, document) = scene.apply(json!({ "remote": remote }), &["--baseline"]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(document["status"], "applied", "{document}");
    assert!(document.get("git").is_none(), "{document}");
    assert_eq!(git_stdout(&target, &["remote"]), "");
}

#[test]
fn the_applied_document_never_shows_remote_credentials() {
    let scene = Scene::new(TEMPLATE, false);
    // Nothing listens on the discard port, so the remote is unreachable.
    let remote = "https://user:secret@127.0.0.1:9/sample.git";
    let (code, document) = scene.apply(json!({ "remote": remote }), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert_eq!(
        document["git"]["remote"], "https://127.0.0.1:9/sample.git",
        "{document}"
    );
    let text = document.to_string();
    assert!(!text.contains("secret"), "{text}");
    assert!(!text.contains("user:"), "{text}");
}
