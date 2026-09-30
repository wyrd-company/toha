// ---
// relationships:
//   implements: architecture
// ---
//! Dedicated update behaviors: operator files a snapshot does not name survive an
//! update (4), a managed region merges the operator's surrounding lines with the
//! template's new body (10), and a JSON value the template changes is updated
//! while an unrelated key is untouched (11).

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command as StdCommand;

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

fn target_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    std::fs::write(dir.join("README.md"), "seed\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "seed"]);
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", message]);
}

fn envelope(
    dir: &Path,
    name: &str,
    formal: &str,
    answers: serde_json::Value,
) -> std::path::PathBuf {
    let document = serde_json::json!({ "template": formal, "answers": answers });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    path
}

/// Baseline-apply `address` and commit, returning the snapshot id.
fn baseline(iso: &Path, address: &str, env: &Path, target: &Path) -> String {
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--baseline")
        .arg("--answers")
        .arg(env);
    let document = support::first_document(&command.output().unwrap().stdout);
    let id = document["snapshot"]["id"].as_str().unwrap().to_owned();
    commit(target, "baseline");
    id
}

/// Plain-apply `address` into a clean git target (records a snapshot) and commit,
/// returning the snapshot id. Used for inject templates, which modify an existing
/// file rather than merging from an empty base.
fn plain_apply(iso: &Path, address: &str, env: &Path, target: &Path) -> String {
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--answers")
        .arg(env);
    let output = command.output().unwrap();
    assert!(output.status.success(), "apply failed: {output:?}");
    let document = support::first_document(&output.stdout);
    let id = document["snapshot"]["id"].as_str().unwrap().to_owned();
    commit(target, "apply");
    id
}

fn update_from(
    iso: &Path,
    address: &str,
    snapshot: &str,
    env: Option<&Path>,
    target: &Path,
) -> serde_json::Value {
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--from")
        .arg(snapshot);
    if let Some(env) = env {
        command.arg("--answers").arg(env);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "update failed: {output:?}");
    support::first_document(&output.stdout)
}

#[test]
fn b4_operator_files_a_snapshot_does_not_name_are_untouched_by_an_update() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: greeter\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
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
    let env_a = envelope(
        iso.path(),
        "a.json",
        &formal,
        serde_json::json!({ "name": "sample-value" }),
    );
    let snapshot = baseline(iso.path(), &address, &env_a, target.path());

    // An operator file the snapshot never named, at the repository root.
    std::fs::write(target.path().join("operator-only.txt"), "operator bytes\n").unwrap();
    commit(target.path(), "operator file");

    // The update changes the template's own file; the operator file is untouched.
    std::fs::write(
        template_dir.path().join("template/greeting.txt"),
        "Hi {{ name }}\n",
    )
    .unwrap();
    let env_b = envelope(
        iso.path(),
        "b.json",
        &formal,
        serde_json::json!({ "name": "revised-value" }),
    );
    let document = update_from(iso.path(), &address, &snapshot, Some(&env_b), target.path());
    assert_eq!(document["status"], "applied", "{document}");
    assert_eq!(
        std::fs::read_to_string(target.path().join("greeting.txt")).unwrap(),
        "Hi revised-value\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("operator-only.txt")).unwrap(),
        "operator bytes\n",
        "an operator file the snapshot did not name must be untouched"
    );
}

#[test]
fn b10_a_managed_region_keeps_operator_lines_and_takes_the_new_body() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // Version A injects a region into config.toml after [application].
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: cfg\ninject:\n  - into: config.toml\n    region: features\n    content: |\n      alpha = 1\n    anchor: { after: \"[application]\", occurrence: only }\n",
    )
    .unwrap();
    // config.toml pre-exists with the anchor and a separate operator section far
    // from where the region injects, so an operator edit there is a separate hunk.
    target_repo(target.path());
    std::fs::write(
        target.path().join("config.toml"),
        "[application]\n\n\n[other]\nkeep = 1\n",
    )
    .unwrap();
    commit(target.path(), "config");
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(iso.path(), "a.json", &formal, serde_json::json!({}));
    let snapshot = plain_apply(iso.path(), &address, &env, target.path());

    // The operator edits a line in the far [other] section.
    let applied = std::fs::read_to_string(target.path().join("config.toml")).unwrap();
    assert!(applied.contains("alpha = 1"), "region applied: {applied}");
    std::fs::write(
        target.path().join("config.toml"),
        applied.replace("keep = 1", "keep = 2"),
    )
    .unwrap();
    commit(target.path(), "operator edit");

    // Version B changes the region body.
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: cfg\ninject:\n  - into: config.toml\n    region: features\n    content: |\n      alpha = 2\n      beta = 3\n    anchor: { after: \"[application]\", occurrence: only }\n",
    )
    .unwrap();
    let document = update_from(iso.path(), &address, &snapshot, None, target.path());
    assert_eq!(document["status"], "applied", "{document}");

    let merged = std::fs::read_to_string(target.path().join("config.toml")).unwrap();
    assert!(
        !merged.contains("<<<<<<<"),
        "the region and the far operator edit merge cleanly: {merged}"
    );
    assert!(
        merged.contains("alpha = 2") && merged.contains("beta = 3"),
        "new body: {merged}"
    );
    assert!(merged.contains("keep = 2"), "operator edit kept: {merged}");
    assert!(
        !merged.contains("alpha = 1\n"),
        "old body replaced: {merged}"
    );
}

#[test]
fn b11_a_json_value_the_template_changes_is_updated_and_unrelated_keys_stay() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    // Version A sets a JSON value the template owns.
    std::fs::create_dir_all(template_dir.path().join("template")).unwrap();
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: pkg\ninject:\n  - into: package.json\n    struct:\n      path: \"scripts.build\"\n      value: \"build-a\"\n",
    )
    .unwrap();
    target_repo(target.path());
    std::fs::write(
        target.path().join("package.json"),
        "{\n  \"name\": \"operator\",\n  \"scripts\": {}\n}\n",
    )
    .unwrap();
    commit(target.path(), "package");
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let env = envelope(iso.path(), "a.json", &formal, serde_json::json!({}));
    let snapshot = plain_apply(iso.path(), &address, &env, target.path());

    // The operator adds an unrelated key.
    let applied: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(target.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(
        applied["scripts"]["build"], "build-a",
        "template value applied"
    );
    let mut edited = applied.clone();
    edited["author"] = serde_json::json!("operator");
    std::fs::write(
        target.path().join("package.json"),
        serde_json::to_string_pretty(&edited).unwrap() + "\n",
    )
    .unwrap();
    commit(target.path(), "operator key");

    // Version B changes the template's value.
    std::fs::write(
        template_dir.path().join("template.yml"),
        "name: pkg\ninject:\n  - into: package.json\n    struct:\n      path: \"scripts.build\"\n      value: \"build-b\"\n",
    )
    .unwrap();
    let document = update_from(iso.path(), &address, &snapshot, None, target.path());
    assert_eq!(document["status"], "applied", "{document}");

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(target.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(
        merged["scripts"]["build"], "build-b",
        "template value updated: {merged}"
    );
    assert_eq!(
        merged["author"], "operator",
        "unrelated operator key kept: {merged}"
    );
    assert_eq!(merged["name"], "operator", "unrelated key kept: {merged}");
}
