// ---
// relationships:
//   implements: command-line-interface
// ---
#[allow(dead_code)]
mod support;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn command(root: &TempDir) -> Command {
    let mut command = support::isolated_command(root.path());
    command.current_dir(root.path());
    command
}
fn run(root: &TempDir, args: &[&str], exit: i32) -> Output {
    let output = command(root).args(args).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "args={args:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn repository(root: &TempDir) -> (String, String) {
    let repo = root.path().join("owner/repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Sample"]);
    git(&repo, &["config", "user.email", "sample@example.invalid"]);
    for (folder, name, content) in [("one", "same", "old"), ("two", "same", "second")] {
        fs::create_dir_all(repo.join(folder).join("template")).unwrap();
        fs::write(repo.join(folder).join("template.yml"), format!("name: {name}\ninterview:\n  - {{ id: label, type: text, prompt: Label?, default: Template }}\n")).unwrap();
        fs::write(
            repo.join(folder).join("template/result.txt"),
            format!("{content} {{{{ label }}}}"),
        )
        .unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    (format!("file://{}", repo.display()), commit)
}
fn write_config(root: &TempDir, content: &str) {
    let path = root.path().join("config/toha/config.yml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn document(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn alias_short_ambiguous_and_address_forms() {
    let root = TempDir::new().unwrap();
    let (url, commit) = repository(&root);
    write_config(
        &root,
        &format!("hosts:\n  local: file://{}\n", root.path().display()),
    );
    let first = "local:owner/repo#one";
    run(
        &root,
        &[
            "templates",
            "add",
            &format!("{url}#one"),
            "--alias",
            "chosen",
        ],
        0,
    );
    let target = root.path().join("target");
    let batch = document(&run(
        &root,
        &["stage", "chosen", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(batch["context"]["template"], first);
    assert_eq!(batch["context"]["commit"], commit);
    run(&root, &["abort", target.to_str().unwrap()], 0);
    let short = document(&run(
        &root,
        &["stage", "same", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(short["context"]["template"], first);
    run(&root, &["abort", target.to_str().unwrap()], 0);
    let second = "local:owner/repo#two";
    run(&root, &["templates", "add", &format!("{url}#two")], 0);
    let ambiguous = run(
        &root,
        &["stage", "same", target.to_str().unwrap(), "--async"],
        5,
    );
    let error = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(
        error.contains(first) && error.contains(second) && error.contains("templates alias"),
        "{error}"
    );
    let answers = root.path().join("answers.json");
    fs::write(&answers, r#"{"label":"Headless"}"#).unwrap();
    let explicit = format!("{url}@main#one");
    run(
        &root,
        &[
            "apply",
            &explicit,
            root.path().join("explicit").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("explicit/result.txt")).unwrap(),
        "old Headless"
    );
    let shorthand = "local:owner/repo@main#one";
    run(
        &root,
        &[
            "apply",
            shorthand,
            root.path().join("shorthand").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("shorthand/result.txt")).unwrap(),
        "old Headless"
    );
    let missing = run(
        &root,
        &["stage", "absent", target.to_str().unwrap(), "--async"],
        1,
    );
    assert!(String::from_utf8_lossy(&missing.stderr).contains("template not found"));
    let missing_path = run(
        &root,
        &[
            "stage",
            &format!("{url}#absent"),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(!missing_path.stderr.is_empty());
    let missing_repo = run(
        &root,
        &[
            "stage",
            &format!("file://{}/absent", root.path().display()),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(!missing_repo.stderr.is_empty());
    let escaped = run(
        &root,
        &[
            "stage",
            &format!("{url}#{}", root.path().join("owner/repo/one").display()),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(String::from_utf8_lossy(&escaped.stderr).contains("leaves repository"));
}

#[test]
fn cached_commit_survives_source_disappearance() {
    let root = TempDir::new().unwrap();
    let (url, commit) = repository(&root);
    let address = format!("{url}@{commit}#one");
    let answers = root.path().join("answers.json");
    fs::write(&answers, r#"{"label":"Cached"}"#).unwrap();
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("first").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    fs::rename(
        root.path().join("owner/repo"),
        root.path().join("owner/removed"),
    )
    .unwrap();
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("second").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("second/result.txt")).unwrap(),
        "old Cached"
    );
}

#[test]
fn configured_default_is_visible_and_answers_win() {
    let root = TempDir::new().unwrap();
    let (url, _) = repository(&root);
    write_config(&root, "defaults:\n  label: Configured\n");
    let address = format!("{url}#one");
    let target = root.path().join("staged");
    let batch = document(&run(
        &root,
        &["stage", &address, target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(
        batch["schema"]["properties"]["label"]["default"],
        "Configured"
    );
    write_config(&root, "defaults:\n  label: Resumed\n");
    let empty = root.path().join("empty.json");
    fs::write(&empty, "{}").unwrap();
    let resumed = document(&run(
        &root,
        &[
            "continue",
            target.to_str().unwrap(),
            empty.to_str().unwrap(),
        ],
        0,
    ));
    assert_eq!(resumed["answers"]["label"], "Resumed");
    write_config(&root, "defaults:\n  label: Configured\n");
    let answers = root.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("defaulted").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("defaulted/result.txt")).unwrap(),
        "old Configured"
    );
    fs::write(&answers, r#"{"label":"Override"}"#).unwrap();
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("overridden").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("overridden/result.txt")).unwrap(),
        "old Override"
    );
    write_config(&root, "defaults:\n  label: [wrong]\n");
    let wrong = run(
        &root,
        &[
            "stage",
            &address,
            root.path().join("wrong").to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("label.configured default"));
}

#[test]
fn resume_uses_recorded_commit_after_registry_update() {
    let root = TempDir::new().unwrap();
    let (url, old) = repository(&root);
    let address = format!("{url}#one");
    run(
        &root,
        &["templates", "add", &address, "--alias", "chosen"],
        0,
    );
    let target = root.path().join("target");
    let staged = document(&run(
        &root,
        &["stage", "chosen", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(staged["context"]["commit"], old);
    let repo = root.path().join("owner/repo");
    fs::write(repo.join("one/template/result.txt"), "new {{ label }}").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "next"]);
    run(&root, &["templates", "update", "chosen"], 0);
    let answers = root.path().join("answers.json");
    fs::write(&answers, r#"{"label":"Resumed"}"#).unwrap();
    let continued = document(&run(
        &root,
        &[
            "continue",
            target.to_str().unwrap(),
            answers.to_str().unwrap(),
        ],
        0,
    ));
    assert_eq!(continued["context"]["commit"], old);
    run(&root, &["apply", target.to_str().unwrap()], 0);
    assert_eq!(
        fs::read_to_string(target.join("result.txt")).unwrap(),
        "old Resumed"
    );
}

#[cfg(unix)]
#[test]
fn registry_trust_runs_hooks_but_discovery_and_local_registry_do_not() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("hook-template");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: hooked\nhooks:\n  - run: [/bin/sh, -c, 'printf ok > hook.txt']\n",
    )
    .unwrap();
    fs::write(folder.join("template/file.txt"), "file").unwrap();
    let answers = root.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let direct = run(
        &root,
        &[
            "apply",
            folder.to_str().unwrap(),
            root.path().join("direct").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        3,
    );
    assert!(String::from_utf8_lossy(&direct.stderr).contains("--trust"));
    let staged = root.path().join("staged-trust");
    run(
        &root,
        &[
            "stage",
            folder.to_str().unwrap(),
            staged.to_str().unwrap(),
            "--async",
        ],
        0,
    );
    run(
        &root,
        &["templates", "add", folder.to_str().unwrap(), "--trust"],
        0,
    );
    run(
        &root,
        &[
            "apply",
            "hooked",
            root.path().join("trusted").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("trusted/hook.txt")).unwrap(),
        "ok"
    );
    run(&root, &["apply", staged.to_str().unwrap()], 0);
    assert_eq!(fs::read_to_string(staged.join("hook.txt")).unwrap(), "ok");
    let revoked = root.path().join("revoked-trust");
    run(
        &root,
        &["stage", "hooked", revoked.to_str().unwrap(), "--async"],
        0,
    );
    run(&root, &["templates", "remove", "hooked"], 0);
    run(&root, &["apply", revoked.to_str().unwrap()], 3);
    let discovered = root.path().join(".templates/discovered");
    fs::create_dir_all(discovered.join("template")).unwrap();
    fs::write(
        discovered.join("template.yml"),
        "name: discovered\nhooks:\n  - run: [/bin/sh, -c, 'printf ok > hook.txt']\n",
    )
    .unwrap();
    fs::write(discovered.join("template/file.txt"), "file").unwrap();
    fs::write(
        root.path().join(".templates/templates.yml"),
        format!(
            "templates:\n  {}:\n    aliases: [local-name]\n",
            discovered.display()
        ),
    )
    .unwrap();
    run(
        &root,
        &[
            "apply",
            "local-name",
            root.path().join("untrusted").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ],
        3,
    );
    run(
        &root,
        &[
            "apply",
            "local-name",
            root.path().join("allowed").to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
            "--trust",
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("allowed/hook.txt")).unwrap(),
        "ok"
    );
}
