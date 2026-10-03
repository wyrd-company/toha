// ---
// relationships:
//   implements: architecture
// ---
//! The update route's result documents — `applied`, `planned`, and
//! `already-current` with their `merge` and `snapshot` members — validate
//! against the published interview-protocol schema, and a shape or gate
//! mutation of an emitted document is rejected. This exercises actual emitted
//! documents, not a hand-written example.

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

fn write_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: greeter\ndescription: A greeting\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("template/greeting.txt"), "Hello {{ name }}\n").unwrap();
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

fn validator() -> jsonschema::Validator {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(toha::protocol::protocol_schema())
        .expect("the protocol schema compiles")
}

#[test]
fn update_result_documents_validate_and_mutations_are_rejected() {
    let iso = tempfile::tempdir().unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_template(template_dir.path());
    target_repo(target.path());
    let formal = support::formal_name(template_dir.path());
    let address = support::folder_address(&template_dir.path().canonicalize().unwrap());
    let schema = validator();

    // 1. A baseline apply emits an `applied` document with snapshot and merge.
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
    let applied = support::first_document(&baseline.output().unwrap().stdout);
    assert_eq!(applied["status"], "applied", "{applied}");
    assert!(schema.is_valid(&applied), "applied invalid: {applied}");
    assert!(applied["merge"]["changes"].is_array(), "{applied}");
    let snapshot = applied["snapshot"]["id"].as_str().unwrap().to_owned();
    git(target.path(), &["add", "."]);
    git(target.path(), &["commit", "--quiet", "-m", "baseline"]);

    // Negative: an unknown merge action is rejected by the schema.
    let mut bad_action = applied.clone();
    bad_action["merge"]["changes"] =
        serde_json::json!([{ "path": "greeting.txt", "action": "bogus" }]);
    assert!(
        !schema.is_valid(&bad_action),
        "an unknown merge action must be rejected"
    );
    // Negative: an unexpected member is rejected (additionalProperties: false).
    let mut extra = applied.clone();
    extra["unexpected"] = serde_json::json!(true);
    assert!(!schema.is_valid(&extra), "an extra member must be rejected");
    // Observation 7: the governing contract requires an `applied` document to
    // report its snapshot decision, so the schema requires `snapshot`. Prove that
    // existing requirement with a negative: dropping `snapshot` is rejected.
    let mut no_snapshot = applied.clone();
    no_snapshot.as_object_mut().unwrap().remove("snapshot");
    assert!(
        !schema.is_valid(&no_snapshot),
        "an applied document without its snapshot decision must be rejected"
    );
    // Observation 7: the governing contract carries `merge` only for updates, so
    // the schema keeps it optional. Dropping `merge` stays valid — proving the
    // schema does not require a member the contract does not (no new policy).
    let mut no_merge = applied.clone();
    no_merge.as_object_mut().unwrap().remove("merge");
    assert!(
        schema.is_valid(&no_merge),
        "an applied document without merge stays valid: merge is optional"
    );

    // 2. A re-render with the same answers emits `already-current`.
    let mut current = support::isolated_command(iso.path());
    current
        .arg("apply")
        .arg(target.path())
        .arg("--from")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&env_first);
    let document = support::first_document(&current.output().unwrap().stdout);
    assert_eq!(document["status"], "already-current", "{document}");
    assert!(
        schema.is_valid(&document),
        "already-current invalid: {document}"
    );
    // Negative: a wrong status constant is rejected.
    let mut bad_status = document.clone();
    bad_status["status"] = serde_json::json!("applied-ish");
    assert!(
        !schema.is_valid(&bad_status),
        "a bad status must be rejected"
    );

    // 3. A dry-run update emits a `planned` document with merge and trusted.
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
    let planned = support::first_document(&dry.output().unwrap().stdout);
    assert_eq!(planned["status"], "planned", "{planned}");
    assert_eq!(planned["trusted"], true, "{planned}");
    assert!(schema.is_valid(&planned), "planned invalid: {planned}");
    // Negative: dropping the required `trusted` gate is rejected.
    let mut no_trust = planned.clone();
    no_trust.as_object_mut().unwrap().remove("trusted");
    assert!(
        !schema.is_valid(&no_trust),
        "a planned document without the trusted gate must be rejected"
    );
}
