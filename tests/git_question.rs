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
    support::file_url(&dir.join("empty.git"))
}

fn remote_with_history(dir: &Path) -> String {
    let seed = dir.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(seed.join("README.md"), "seed\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "--quiet", "-m", "seed"]);
    git(dir, &["clone", "--quiet", "--bare", "seed", "history.git"]);
    support::file_url(&dir.join("history.git"))
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
    // The new repository has no commit yet, so no snapshot is saved.
    assert_eq!(
        document["record"],
        json!({ "path": ".toha.json" }),
        "{document}"
    );
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
    let missing = support::file_url(scene.work.path()) + "/missing.git";
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
    assert!(
        stdout.contains("saved applied record .toha.json"),
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

/// The files under `root` that hold `secret`, the end of every password in
/// these tests, in any encoding.
fn files_with_secret(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if std::fs::read(&path)
                .unwrap()
                .windows(6)
                .any(|window| window == b"secret")
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn a_git_answer_keeps_no_credentials() {
    // Passwords that gix writes back percent-encoded, decoded or with Rust
    // escapes, and one for a protocol that takes no credentials.
    for (remote, kept) in [
        (
            "https://user:plain-secret@127.0.0.1:9/sample.git",
            "https://127.0.0.1:9/sample.git",
        ),
        (
            "https://user:p@ss-secret@127.0.0.1:9/sample.git",
            "https://127.0.0.1:9/sample.git",
        ),
        (
            "https://user:p%40ss-secret@127.0.0.1:9/sample.git",
            "https://127.0.0.1:9/sample.git",
        ),
        (
            " https://user:caf\u{e9}\u{1}\t-secret@127.0.0.1:9/sample.git ",
            "https://127.0.0.1:9/sample.git",
        ),
        (
            "git://user:p@ss-secret@127.0.0.1:9/sample.git",
            "git://127.0.0.1:9/sample.git",
        ),
        (
            "HTTPS://user:pa?ss-secret@127.0.0.1:9/sample.git",
            "HTTPS://127.0.0.1:9/sample.git",
        ),
        // No credential: a `@` in the query keeps the host.
        (
            "https://127.0.0.1:9?owner=alice@example.invalid/sample.git",
            "https://127.0.0.1:9?owner=alice@example.invalid/sample.git",
        ),
    ] {
        let scene = Scene::new(TEMPLATE, false);
        let (code, document) = scene.apply(json!({ "remote": remote }), &[]);
        assert_eq!(code, Some(0), "{remote:?}: {document}");
        assert!(!document.to_string().contains("secret"), "{document}");
        assert_eq!(document["git"]["remote"], kept, "{document}");
        let target = scene.target();
        assert_eq!(git_stdout(&target, &["remote", "get-url", "origin"]), kept);
        std::fs::remove_file(scene.iso.path().join("answers.json")).unwrap();
        for root in [scene.iso.path(), scene.work.path()] {
            assert_eq!(files_with_secret(root), Vec::<PathBuf>::new(), "{remote:?}");
        }
    }
}

#[test]
fn a_staged_git_answer_and_default_keep_no_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote URL
    default: https://user:default-secret@127.0.0.1:9/sample.git
",
        false,
    );
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    let staged = stage.output().unwrap();
    assert_eq!(staged.status.code(), Some(4), "{staged:?}");
    let questions = support::first_document(&staged.stdout);
    assert_eq!(
        questions["schema"]["properties"]["remote"]["default"], "https://127.0.0.1:9/sample.git",
        "{questions}"
    );
    let answers = tempfile::tempdir().unwrap();
    let file = answers.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "template": support::formal_name(scene.template.path()),
            "answers": { "remote": "https://user:p%40ss-secret@127.0.0.1:9/sample.git" },
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cont = scene.command();
    cont.arg("continue").arg(scene.target()).arg(&file);
    let continued = cont.output().unwrap();
    assert!(continued.status.success(), "{continued:?}");
    // The staged state under the isolated state folder holds no password.
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
    let mut apply = scene.command();
    apply.arg("apply").arg(scene.target());
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    for output in [&staged, &continued, &applied] {
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!shown.contains("secret"), "{shown}");
    }
    assert_eq!(
        git_stdout(&scene.target(), &["remote", "get-url", "origin"]),
        "https://127.0.0.1:9/sample.git"
    );
    for root in [scene.iso.path(), scene.work.path()] {
        assert_eq!(files_with_secret(root), Vec::<PathBuf>::new());
    }
}

#[test]
fn a_continue_naming_another_template_shows_no_credentials() {
    let scene = Scene::new(TEMPLATE, false);
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4));
    let answers = tempfile::tempdir().unwrap();
    let file = answers.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "template": "https://user:p%40ss-identity-secret@host.invalid/r.git",
            "answers": { "with_repo": false },
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cont = scene.command();
    cont.arg("continue").arg(scene.target()).arg(&file);
    let output = cont.output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(shown.contains("https://host.invalid/r.git"), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
}

/// Stage `scene` and return what the command wrote.
fn staged_output(scene: &Scene) -> String {
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    let output = stage.output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_rejected_git_default_names_no_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote URL
    default: https://user:p%40ss-default-secret@127.0.0.1:9/r.git
    validate:
      regex: '^allowed$'
",
        false,
    );
    let shown = staged_output(&scene);
    assert!(shown.contains("remote.default"), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_rejected_preset_default_names_no_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote URL
    validate:
      regex: '^allowed$'
",
        false,
    );
    let config = scene.iso.path().join("config/local.yml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        format!(
            "presets:\n  remote_url: https://user:p%40ss-preset-secret@127.0.0.1:9/r.git\n\
             template-defaults:\n  {:?}:\n    remote: {{ preset: remote_url }}\n",
            support::formal_name(scene.template.path())
        ),
    )
    .unwrap();
    let shown = staged_output(&scene);
    // The provenance still names the preset, without its credentials.
    assert!(
        shown.contains(r#"presets.\"remote_url\""#) || shown.contains(r#"presets."remote_url""#),
        "{shown}"
    );
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_git_default_expression_is_never_shown() {
    // An expression can hold a URL in any form, so diagnostics name the field
    // without the expression: when the default is rejected, and when it fails
    // to render.
    for default in [
        r#"'{{ "https://user:p%40ss-jinja-secret@127.0.0.1:9/r.git" }}'"#,
        r#"'{{ "https://user:p%40ss-jinja-secret@127.0.0.1:9/r.git" | dateformat }}'"#,
    ] {
        let scene = Scene::new(
            &format!(
                "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote URL
    default: {default}
    validate:
      regex: '^allowed$'
"
            ),
            false,
        );
        let shown = staged_output(&scene);
        assert!(shown.contains("remote.default"), "{default}: {shown}");
        assert!(!shown.contains("secret"), "{default}: {shown}");
    }
}

#[test]
fn a_wrong_kind_preset_for_a_git_question_is_never_shown() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote URL
",
        false,
    );
    let config = scene.iso.path().join("config/local.yml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        format!(
            "presets:\n  remote_url: [\"https://user:p%40ss-wrong-kind-secret@127.0.0.1:9/r.git\"]\n\
             template-defaults:\n  {:?}:\n    remote: {{ preset: remote_url }}\n",
            support::formal_name(scene.template.path())
        ),
    )
    .unwrap();
    let shown = staged_output(&scene);
    assert!(shown.contains("must be a string"), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_like_seed_is_staged_without_credentials() {
    // A record written before Toha removed a `git` answer's credentials, with
    // the answer as a string, or of the wrong kind after the question changed.
    let url = "https://user:p%40ss-application-secret@127.0.0.1:9/r.git";
    for recorded in [
        json!(url),
        json!([url]),
        json!({ url: null }),
        json!({ "nested": { url: [url] } }),
    ] {
        like_seed_is_staged_without_credentials(recorded);
    }
}

fn like_seed_is_staged_without_credentials(recorded: Value) {
    let scene = Scene::new(TEMPLATE, false);
    let (code, document) = scene.apply(
        json!({ "with_repo": true, "remote": "https://127.0.0.1:9/r.git" }),
        &[],
    );
    assert_eq!(code, Some(0), "{document}");
    let project = scene.target();
    let record_path = project.join(".toha.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(
        record["applications"][0]["answers"]["remote"], "https://127.0.0.1:9/r.git",
        "{record}"
    );
    record["applications"][0]["answers"]["remote"] = recorded.clone();
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "applied"]);
    std::fs::remove_file(scene.iso.path().join("answers.json")).unwrap();
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(project.join("beta"))
        .args(["--like", ".", "--async"]);
    let output = stage.output().unwrap();
    assert_eq!(output.status.code(), Some(4), "{recorded}: {output:?}");
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!shown.contains("secret"), "{shown}");
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_git_question_the_schema_rejects_names_no_credentials() {
    // A schema error writes the question it rejects; a missing `prompt` and
    // a wrong `validate` each reject the whole question.
    for extra in ["", "    prompt: Remote\n    validate: false\n"] {
        for default in [
            "https://user:p%40ss-schema-secret@127.0.0.1:9/r.git",
            r#"'{{ "https://user:p%40ss-schema-secret@127.0.0.1:9/r.git" }}'"#,
            r#"["https://user:p%40ss-schema-secret@127.0.0.1:9/r.git"]"#,
        ] {
            let scene = Scene::new(
                &format!(
                    "name: sample\ninterview:\n  - id: remote\n    type: git\n{extra}    default: {default}\n"
                ),
                false,
            );
            let shown = staged_output(&scene);
            assert!(shown.contains("template.yml/interview/0"), "{shown}");
            assert!(!shown.contains("secret"), "{default}: {shown}");
            // The public loader returns the same problems.
            let error = toha::Template::load(scene.template.path()).unwrap_err();
            let returned = format!("{error} {error:?}");
            assert!(!returned.contains("secret"), "{default}: {returned}");
        }
    }
}

#[test]
fn a_template_that_cannot_be_read_names_no_credentials() {
    // A YAML error can quote the scalar it could not read.
    for tag in ["!!int", "!!float"] {
        let scene = Scene::new(
            &format!(
                "name: sample\ninterview:\n  - id: remote\n    type: git\n    prompt: Remote\n    \
                 default: {tag} https://user:p%40ss-parse-secret@127.0.0.1:9/r.git\n"
            ),
            false,
        );
        let shown = staged_output(&scene);
        assert!(shown.contains("cannot be read as YAML"), "{tag}: {shown}");
        assert!(!shown.contains("secret"), "{tag}: {shown}");
        let error = toha::Template::load(scene.template.path()).unwrap_err();
        let returned = format!("{error} {error:?}");
        assert!(!returned.contains("secret"), "{tag}: {returned}");
    }
}

#[test]
fn a_template_include_or_tag_names_no_credentials() {
    // Resolution names a problem by the keys above it and repeats a tag or an
    // include operand; any of them can hold a URL with credentials.
    let url = "https://user:p%40ss-resolve-secret@127.0.0.1:9/r.git";
    // A verbatim tag loads as its value; the other three are load errors.
    for (default, fails) in [
        (format!("\n      {url:?}: !unknown value"), true),
        (
            format!("\n      outer:\n        {url:?}: !unknown value"),
            true,
        ),
        (format!("!include {url}"), true),
        (format!("!<{url}> value"), false),
    ] {
        let scene = Scene::new(
            &format!(
                "name: sample\ninterview:\n  - id: remote\n    type: git\n    prompt: Remote\n    \
                 default: {default}\n"
            ),
            false,
        );
        let shown = staged_output(&scene);
        assert!(!shown.contains("secret"), "{default}: {shown}");
        let loaded = toha::Template::load(scene.template.path());
        assert_eq!(loaded.is_err(), fails, "{default}");
        if let Err(error) = loaded {
            assert!(shown.contains("template.yml"), "{default}: {shown}");
            let returned = format!("{error} {error:?}");
            assert!(!returned.contains("secret"), "{default}: {returned}");
        }
    }
}

#[test]
fn an_update_stages_a_recorded_git_answer_without_credentials() {
    let scene = Scene::new(TEMPLATE, false);
    // An update replays the template's recorded revision, so the template is
    // a committed repository.
    let source = scene.template.path();
    git(source, &["init", "--quiet", "-b", "main"]);
    git(source, &["add", "."]);
    git(source, &["commit", "--quiet", "-m", "template"]);
    let address = support::file_url(source);
    let answers = tempfile::tempdir().unwrap();
    let file = answers.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "template": address,
            "answers": { "with_repo": true, "remote": "https://127.0.0.1:9/r.git" },
        }))
        .unwrap(),
    )
    .unwrap();
    let project = scene.target();
    let mut apply = scene.command();
    apply
        .arg("apply")
        .arg(&address)
        .arg(&project)
        .arg("--answers")
        .arg(&file);
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    // A record written before Toha removed a `git` answer's credentials.
    let record_path = project.join(".toha.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["applications"][0]["answers"]["remote"] =
        json!("https://user:p%40ss-update-secret@127.0.0.1:9/r.git");
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "applied"]);
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(&address)
        .arg(&project)
        .args(["--update", "--async"]);
    let output = stage.output().unwrap();
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
    // The staged submissions and the pinned base hold no password.
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
    // An update staged before the fix held the password in its submissions
    // and its pinned base; resuming it keeps and saves neither.
    let path = staged_record(&scene);
    let mut staged: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    if let Some(submission) = staged["submissions"].get_mut(0) {
        submission["remote"] = json!("https://user:p%40ss-update-secret@127.0.0.1:9/r.git");
    }
    // Toha 0.4.1 pinned the application as recorded, naming no `git` answer.
    let mut base: Value = serde_json::from_str(staged["base"].as_str().unwrap()).unwrap();
    base["answers"]["remote"] = json!("https://user:p%40ss-update-secret@127.0.0.1:9/r.git");
    base.as_object_mut().unwrap().remove("git_answers");
    staged["base"] = json!(base.to_string());
    std::fs::write(&path, serde_json::to_vec(&staged).unwrap()).unwrap();
    // The staged update resumes against the record that still holds it.
    let mut resume = scene.command();
    resume.arg("apply").arg(&project);
    let resumed = resume.output().unwrap();
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(resumed.status.success(), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_staged_update_refuses_a_changed_answer_that_differs_only_in_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
  - id: label
    type: text
    prompt: Label
",
        false,
    );
    let source = scene.template.path();
    std::fs::write(source.join("template/value.txt"), "{{ label }}\n").unwrap();
    git(source, &["init", "--quiet", "-b", "main"]);
    git(source, &["add", "."]);
    git(source, &["commit", "--quiet", "-m", "template"]);
    let address = support::file_url(source);
    let answers = tempfile::tempdir().unwrap();
    let file = answers.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "template": address,
            "answers": {
                "remote": "https://127.0.0.1:9/r.git",
                "label": "https://alice:old@host.invalid/r",
            },
        }))
        .unwrap(),
    )
    .unwrap();
    let project = scene.target();
    let mut apply = scene.command();
    apply
        .arg("apply")
        .arg(&address)
        .arg(&project)
        .arg("--answers")
        .arg(&file);
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "applied"]);
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(&address)
        .arg(&project)
        .args(["--update", "--async"]);
    let staged = stage.output().unwrap();
    assert!(staged.status.success(), "{staged:?}");
    // A `text` answer's user information is text, not a credential: changing
    // only it changes the application the update was staged from.
    let record_path = project.join(".toha.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["applications"][0]["answers"]["label"] = json!("https://bob:new@host.invalid/r");
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(&project, &["commit", "--quiet", "-am", "changed label"]);
    let mut resume = scene.command();
    resume.arg("apply").arg(&project);
    let resumed = resume.output().unwrap();
    let shown = String::from_utf8_lossy(&resumed.stderr);
    assert!(!resumed.status.success(), "{resumed:?}");
    assert!(shown.contains("changed"), "{shown}");
    let record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(
        record["applications"][0]["answers"]["label"],
        "https://bob:new@host.invalid/r"
    );
}

#[test]
fn a_formatted_git_answer_keeps_no_credentials() {
    // `format` turns a scp-like answer, which holds no password, into an
    // HTTPS URL whose user information would carry one.
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
    format: \"'https://' ~ value\"
  - id: label
    type: text
    prompt: 'Label for {{ remote }}'
",
        false,
    );
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4));
    let answers = tempfile::tempdir().unwrap();
    let file = answers.path().join("answers.json");
    let submit = |values: Value| {
        std::fs::write(
            &file,
            serde_json::to_vec(&json!({
                "template": support::formal_name(scene.template.path()),
                "answers": values,
            }))
            .unwrap(),
        )
        .unwrap();
        let mut cont = scene.command();
        cont.arg("continue").arg(scene.target()).arg(&file);
        cont.output().unwrap()
    };
    let first = submit(json!({ "remote": "user:p%40ss-format-secret@127.0.0.1:9/r.git" }));
    assert_eq!(first.status.code(), Some(4), "{first:?}");
    let questions = support::first_document(&first.stdout);
    assert!(!questions.to_string().contains("secret"), "{questions}");
    assert!(
        questions
            .to_string()
            .contains("Label for https://127.0.0.1:9/r.git"),
        "{questions}"
    );
    let second = submit(json!({ "label": "sample" }));
    assert!(second.status.success(), "{second:?}");
    let mut apply = scene.command();
    apply.arg("apply").arg(scene.target());
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    for output in [&first, &second, &applied] {
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!shown.contains("secret"), "{shown}");
    }
    // The answer as given stays in the record: as a scp-like address, its
    // `@` is in the path and holds no password. Only `format` built a URL
    // whose user information is one, and the remote saves it without.
    assert_eq!(
        git_stdout(&scene.target(), &["remote", "get-url", "origin"]),
        "https://127.0.0.1:9/r.git"
    );
    let config = std::fs::read_to_string(scene.target().join(".git/config")).unwrap();
    assert!(!config.contains("secret"), "{config}");
}

#[test]
fn a_formatted_git_default_keeps_no_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
    default: default-format-secret
    format: \"'https://user:' ~ value ~ '@127.0.0.1:9/r.git'\"
",
        false,
    );
    let (code, document) = scene.apply(json!({}), &[]);
    assert_eq!(code, Some(0), "{document}");
    assert!(!document.to_string().contains("secret"), "{document}");
    // The record keeps the default as the template gives it; the remote
    // keeps no credentials the format built from it.
    assert_eq!(
        git_stdout(&scene.target(), &["remote", "get-url", "origin"]),
        "https://127.0.0.1:9/r.git"
    );
    let config = std::fs::read_to_string(scene.target().join(".git/config")).unwrap();
    assert!(!config.contains("secret"), "{config}");
}

/// A project applied from a committed template `v1` with `answers`, then
/// its recorded `remote` answer set to `recorded` and committed, as Toha
/// 0.4.1 could record it. Returns the template address and the project.
fn applied_from_revision(scene: &Scene, answers: Value, recorded: &str) -> (String, PathBuf) {
    let source = scene.template.path();
    git(source, &["init", "--quiet", "-b", "main"]);
    git(source, &["add", "."]);
    git(source, &["commit", "--quiet", "-m", "v1"]);
    let address = support::file_url(source);
    let file = scene.iso.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({ "template": address, "answers": answers })).unwrap(),
    )
    .unwrap();
    // The project is a repository already, so `.toha.json` is written
    // whether or not the template asks a `git` question.
    let project = scene.target();
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "--quiet", "-b", "main"]);
    git(
        &project,
        &["commit", "--quiet", "--allow-empty", "-m", "project"],
    );
    let mut apply = scene.command();
    apply
        .arg("apply")
        .arg(&address)
        .arg(&project)
        .arg("--answers")
        .arg(&file);
    let applied = apply.output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    std::fs::remove_file(&file).unwrap();
    let record_path = project.join(".toha.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["applications"][0]["answers"]["remote"] = json!(recorded);
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "applied"]);
    (address, project)
}

/// Commit `yml` as the next revision of `scene`'s template.
fn next_revision(scene: &Scene, yml: &str) {
    let source = scene.template.path();
    std::fs::write(source.join("template.yml"), yml).unwrap();
    git(source, &["commit", "--quiet", "-am", "v2"]);
}

#[test]
fn a_recorded_git_answer_keeps_no_credentials_when_the_template_drops_or_retypes_it() {
    let v1 = "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
";
    let secret = "https://user:p%40ss-migration-secret@127.0.0.1:9/r.git";
    for v2 in [
        // The question is gone.
        "name: sample
interview:
  - id: label
    type: text
    prompt: Label
",
        // The question is a text question.
        "name: sample
interview:
  - id: remote
    type: text
    prompt: Remote
",
    ] {
        let scene = Scene::new(v1, false);
        let (address, project) = applied_from_revision(
            &scene,
            json!({ "remote": "https://127.0.0.1:9/r.git" }),
            secret,
        );
        next_revision(&scene, v2);
        let mut stage = scene.command();
        stage
            .arg("stage")
            .arg(&address)
            .arg(&project)
            .args(["--update", "--async"]);
        let output = stage.output().unwrap();
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{v2}: {shown}");
        assert!(!shown.contains("secret"), "{shown}");
        assert_eq!(
            files_with_secret(scene.iso.path()),
            Vec::<PathBuf>::new(),
            "{v2}"
        );
        // A staged update from Toha 0.4.1 pinned the application as recorded;
        // resuming it keeps and saves no credentials either.
        let path = staged_record(&scene);
        let mut staged: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut base: Value = serde_json::from_str(staged["base"].as_str().unwrap()).unwrap();
        base["answers"]["remote"] = json!(secret);
        base.as_object_mut().unwrap().remove("git_answers");
        staged["base"] = json!(base.to_string());
        std::fs::write(&path, serde_json::to_vec(&staged).unwrap()).unwrap();
        let mut resume = scene.command();
        resume.arg("apply").arg(&project);
        let resumed = resume.output().unwrap();
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&resumed.stdout),
            String::from_utf8_lossy(&resumed.stderr)
        );
        assert!(resumed.status.success(), "{v2}: {shown}");
        assert!(!shown.contains("secret"), "{shown}");
        assert_eq!(
            files_with_secret(scene.iso.path()),
            Vec::<PathBuf>::new(),
            "{v2}"
        );
    }
}

#[test]
fn a_like_seed_keeps_no_credentials_of_a_dropped_git_question() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
",
        false,
    );
    let (address, project) = applied_from_revision(
        &scene,
        json!({ "remote": "https://127.0.0.1:9/r.git" }),
        "https://user:p%40ss-like-migration-secret@127.0.0.1:9/r.git",
    );
    next_revision(
        &scene,
        "name: sample
interview:
  - id: label
    type: text
    prompt: Label
",
    );
    let mut stage = scene.command();
    stage
        .current_dir(&project)
        .arg("stage")
        .arg(&address)
        .arg(project.join("beta"))
        .args(["--like", ".", "--async"]);
    let output = stage.output().unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!shown.contains("secret"), "{shown}");
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_staged_update_refuses_a_changed_text_answer_the_template_now_asks_as_git() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: text
    prompt: Remote
",
        false,
    );
    let (address, project) = applied_from_revision(
        &scene,
        json!({ "remote": "https://alice:old@host.invalid/r" }),
        "https://alice:old@host.invalid/r",
    );
    next_revision(
        &scene,
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
",
    );
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(&address)
        .arg(&project)
        .args(["--update", "--async"]);
    let staged = stage.output().unwrap();
    assert!(staged.status.success(), "{staged:?}");
    // The recorded answer was text, so its user information is text, and a
    // change to it changes the application the update was staged from.
    let record_path = project.join(".toha.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["applications"][0]["answers"]["remote"] = json!("https://bob:new@host.invalid/r");
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    git(&project, &["commit", "--quiet", "-am", "changed remote"]);
    let mut resume = scene.command();
    resume.arg("apply").arg(&project);
    let resumed = resume.output().unwrap();
    let shown = String::from_utf8_lossy(&resumed.stderr);
    assert!(!resumed.status.success(), "{resumed:?}");
    assert!(shown.contains("changed"), "{shown}");
    let record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(
        record["applications"][0]["answers"]["remote"],
        "https://bob:new@host.invalid/r"
    );
}

/// The one staged record in the `staged` folder under the isolated root of
/// `scene`, wherever the platform keeps state.
fn staged_record(scene: &Scene) -> PathBuf {
    let mut records = Vec::new();
    let mut folders = vec![scene.iso.path().to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if folder.file_name().is_some_and(|name| name == "staged") {
                records.push(path);
            }
        }
    }
    assert_eq!(records.len(), 1, "{records:?}");
    records.remove(0)
}

#[test]
fn a_stage_from_before_the_fix_resumes_without_credentials() {
    let scene = Scene::new(
        "name: sample
interview:
  - id: remote
    type: git
    prompt: Remote
  - id: name
    type: text
    prompt: 'Name for {{ remote }}'
",
        false,
    );
    let mut stage = scene.command();
    stage
        .arg("stage")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--async");
    assert_eq!(stage.output().unwrap().status.code(), Some(4));
    let answers = tempfile::tempdir().unwrap();
    let submit = |values: Value| {
        let file = answers.path().join("answers.json");
        std::fs::write(
            &file,
            serde_json::to_vec(&json!({
                "template": support::formal_name(scene.template.path()),
                "answers": values,
            }))
            .unwrap(),
        )
        .unwrap();
        let mut cont = scene.command();
        cont.arg("continue").arg(scene.target()).arg(&file);
        cont.output().unwrap()
    };
    let first = submit(json!({ "remote": "https://127.0.0.1:9/r.git" }));
    assert_eq!(first.status.code(), Some(4), "{first:?}");
    // The record as Toha 0.4.1 saved it, with the answer's credentials.
    let path = staged_record(&scene);
    let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["submissions"][0]["remote"] =
        json!("https://user:p%40ss-legacy-stage-secret@127.0.0.1:9/r.git");
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let second = submit(json!({ "name": "sample" }));
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(second.status.success(), "{shown}");
    assert!(!shown.contains("secret"), "{shown}");
    assert_eq!(files_with_secret(scene.iso.path()), Vec::<PathBuf>::new());
}

#[test]
fn a_relative_remote_with_history_is_checked_from_the_target() {
    let scene = Scene::new(TEMPLATE, false);
    // `history.git` sits beside the target, so `../history.git` names it from
    // the target; Toha runs from another directory.
    remote_with_history(scene.work.path());
    let elsewhere = tempfile::tempdir().unwrap();
    let document = json!({
        "template": support::formal_name(scene.template.path()),
        "answers": { "remote": "../history.git" },
    });
    let env = scene.iso.path().join("answers.json");
    std::fs::write(&env, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut command = scene.command();
    command
        .current_dir(elsewhere.path())
        .arg("apply")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--answers")
        .arg(&env);
    let output = command.output().unwrap();
    let document = support::first_document(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{document}");
    assert_eq!(document["kind"], "git", "{document}");
    assert!(!scene.target().exists(), "the target was written");
}

#[cfg(unix)]
#[test]
fn a_parent_step_after_a_symlink_in_a_remote_follows_the_symlink() {
    let scene = Scene::new(TEMPLATE, false);
    let work = scene.work.path();
    // `work/history.git` is empty; `work/alias` points into `other/sub`, so
    // `../alias/../history.git` from the target names `other/history.git`,
    // which has history.
    git(work, &["init", "--quiet", "--bare", "history.git"]);
    let other = work.join("other");
    std::fs::create_dir_all(other.join("sub")).unwrap();
    remote_with_history(&other);
    std::os::unix::fs::symlink(other.join("sub"), work.join("alias")).unwrap();
    let (code, document) = scene.apply(json!({ "remote": "../alias/../history.git" }), &[]);
    assert_eq!(code, Some(1), "{document}");
    assert_eq!(document["kind"], "git", "{document}");
    assert!(!scene.target().exists(), "the target was written");
}

#[cfg(windows)]
#[test]
fn a_canonical_windows_file_url_with_history_stops_the_apply() {
    let scene = Scene::new(TEMPLATE, false);
    remote_with_history(scene.work.path());
    // A canonical Windows path keeps its verbatim `\\?\` prefix in the URL.
    let canonical = scene
        .work
        .path()
        .join("history.git")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    assert!(canonical.starts_with("//?/"), "{canonical}");
    // `file://` and the `//?/` path give the URL form `file:////?/C:/...`.
    let (code, document) = scene.apply(json!({ "remote": format!("file://{canonical}") }), &[]);
    assert_eq!(code, Some(1), "{document}");
    assert_eq!(document["kind"], "git", "{document}");
    assert!(!scene.target().exists(), "the target was written");
}

#[test]
fn a_git_question_requires_git_before_the_interview() {
    let scene = Scene::new(TEMPLATE, false);
    let empty = tempfile::tempdir().unwrap();
    let document = json!({
        "template": support::formal_name(scene.template.path()),
        "answers": { "remote": null },
    });
    let env = scene.iso.path().join("answers.json");
    std::fs::write(&env, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut command = scene.command();
    command
        .env("PATH", empty.path())
        .arg("apply")
        .arg(scene.address())
        .arg(scene.target())
        .arg("--answers")
        .arg(&env);
    let output = command.output().unwrap();
    let document = support::first_document(&output.stdout);
    assert_valid(&document);
    assert_eq!(output.status.code(), Some(1), "{document}");
    assert_eq!(document["kind"], "requirement", "{document}");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("\n  git: "), "{message}");
    assert!(!scene.target().exists(), "the target was written");
}

#[test]
fn the_implicit_git_requirement_yields_to_a_declared_one() {
    let scene = Scene::new(
        "name: sample
requires:
  - program: git
    hint: declared
interview:
  - id: remote
    type: git
    prompt: Remote URL
",
        false,
    );
    let template = toha::Template::load(scene.template.path()).unwrap();
    let requirements = template.requirements();
    assert_eq!(requirements.len(), 1, "{requirements:?}");
    assert_eq!(requirements[0].hint.as_deref(), Some("declared"));
}

#[test]
fn a_template_without_a_git_question_does_not_require_git() {
    let scene = Scene::new("name: sample\n", false);
    let template = toha::Template::load(scene.template.path()).unwrap();
    assert!(template.requirements().is_empty());
}
