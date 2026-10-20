// ---
// relationships:
//   verifies: template-composition
// ---
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use std::{path::Path, process::Command};
use toha::applied::{AppliedRecord, SCHEMA_ID};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn repository(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    commit(root);
}
fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "example"]);
}
fn template(root: &Path, interview: &str, body: &str) {
    std::fs::create_dir_all(root.join("template")).unwrap();
    std::fs::write(
        root.join("template.yml"),
        format!("name: example\ndescription: Example\ninterview:\n{interview}"),
    )
    .unwrap();
    std::fs::write(root.join("template/result.txt"), body).unwrap();
}
fn run(
    iso: &Path,
    address: &str,
    target: &Path,
    answers: Value,
    flags: &[&str],
) -> std::process::Output {
    let file = iso.join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":answers})).unwrap(),
    )
    .unwrap();
    support::isolated_command(iso)
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--answers")
        .arg(file)
        .args(flags)
        .env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]")
        .output()
        .unwrap()
}
fn document(out: &std::process::Output) -> Value {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let result = support::first_document(&out.stdout);
    let schema: Value = serde_norway::from_str(include_str!(
        "../docs/specifications/interview-protocol.schema.yml"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&result), "{result}");
    result
}
fn record(root: &Path, path: &str) -> Value {
    let bytes = std::fs::read(root.join(path)).unwrap();
    AppliedRecord::parse(&bytes).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["$schema"], SCHEMA_ID);
    assert!(bytes.ends_with(b"\n"));
    value
}
const QUESTION: &str = "  - id: label\n    type: text\n    prompt: Label\n    required: true\n";

const PARTS: &str = "  - id: targets\n    type: multiselect\n    parts: true\n    prompt: Parts\n    options: [core, extra]\n    default: [core]\n";
fn parts_template(root: &Path) {
    template(
        root,
        PARTS,
        "name: example\njobs:\n  core: first\n{% if 'extra' in targets %}  extra: enabled\n{% endif %}",
    );
    let mut config = std::fs::read_to_string(root.join("template.yml")).unwrap();
    config.push_str("files:\n  - each: \"['optional'] as item\"\n    source: optional.txt\n    path: optional.txt\n    when: \"'extra' in targets\"\ninject:\n  - into: settings.toml\n    region: optional\n    content: 'optional = true'\n    anchor: {after: '[settings]', occurrence: only}\n    when: \"'extra' in targets\"\n");
    std::fs::write(root.join("template.yml"), config).unwrap();
    std::fs::write(root.join("optional.txt"), "optional\n").unwrap();
}
#[test]
fn parts_add_update_remove_shared_workflow_and_retract_and_release_edits() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    parts_template(source.path());
    repository(source.path());
    repository(root.path());
    std::fs::write(
        root.path().join("settings.toml"),
        "[settings]\noperator = true\n",
    )
    .unwrap();
    commit(root.path());
    let address = support::file_url(source.path());
    let initial = document(&run(iso.path(), &address, root.path(), json!({}), &[]));
    assert_eq!(initial["application"]["parts_added"], json!(["core"]));
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["parts"],
        json!(["core"])
    );
    assert!(
        record(root.path(), ".toha.json")["applications"][0]["answers"]
            .get("targets")
            .is_none()
    );
    commit(root.path());
    let added = document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--update", "--with", "extra"],
    ));
    assert_eq!(added["application"]["parts_added"], json!(["extra"]));
    assert!(root.path().join("optional.txt").exists());
    assert!(
        std::fs::read_to_string(root.path().join("result.txt"))
            .unwrap()
            .contains("extra: enabled")
    );
    commit(root.path());
    let prior = git(source.path(), &["rev-parse", "HEAD"]);
    std::fs::write(source.path().join("template/result.txt"), "name: example\njobs:\n  core: second\n{% if 'extra' in targets %}  extra: enabled\n{% endif %}").unwrap();
    commit(source.path());
    let resolved = git(source.path(), &["rev-parse", "HEAD"]);
    let next = format!("{address}@{resolved}");
    let updated = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(
        updated["application"]["prior_revision"],
        json!({"commit":prior})
    );
    assert_eq!(
        updated["application"]["resolved_revision"],
        json!({"commit":resolved})
    );
    assert!(
        std::fs::read_to_string(root.path().join("result.txt"))
            .unwrap()
            .contains("extra: enabled")
    );
    commit(root.path());
    let removed = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update", "--without", "extra"],
    ));
    assert_eq!(removed["application"]["parts_removed"], json!(["extra"]));
    assert!(!root.path().join("optional.txt").exists());
    assert!(
        !std::fs::read_to_string(root.path().join("result.txt"))
            .unwrap()
            .contains("extra: enabled")
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("settings.toml")).unwrap(),
        "[settings]\noperator = true\n"
    );
    commit(root.path());
    document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    commit(root.path());
    document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("settings.toml")).unwrap(),
        "[settings]\noperator = true\n"
    );
}
#[test]
fn removing_a_part_conflicts_with_an_edited_whole_file() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    parts_template(source.path());
    repository(source.path());
    repository(root.path());
    std::fs::write(root.path().join("settings.toml"), "[settings]\n").unwrap();
    commit(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--with", "extra"],
    ));
    commit(root.path());
    std::fs::write(root.path().join("optional.txt"), "operator\n").unwrap();
    commit(root.path());
    let result = document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--update", "--without", "extra"],
    ));
    assert_eq!(result["merge"]["conflicted"], json!(["optional.txt"]));
    assert_eq!(
        result["merge"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == "optional.txt")
            .unwrap()["conflict"],
        "modify-delete"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("optional.txt")).unwrap(),
        "operator\n"
    );
}
#[test]
fn part_answers_and_flags_render_in_option_order_and_flags_override_reanswer() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets|join(',') }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"targets":["extra", "core"]}),
        &[],
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "core,extra\n"
    );
    commit(root.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--update", "--reanswer", "--without", "core"],
    ));
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["parts"],
        json!(["extra"])
    );
}
#[test]
fn part_flags_reject_usage_before_questions_and_validate_the_settled_answer() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets }}\n");
    let address = support::formal_name(source.path());
    for (answers, flags) in [
        (json!({}), vec!["--with", "missing"]),
        (json!({}), vec!["--with", "core", "--without", "core"]),
        (json!({"targets":[]}), vec!["--with", "extra"]),
    ] {
        let out = run(iso.path(), &address, root.path(), answers, &flags);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(!root.path().join("result.txt").exists());
    }
    template(
        source.path(),
        &(PARTS.to_owned() + "    required: true\n    validate: {min: 1, max: 1}\n"),
        "{{ targets }}\n",
    );
    for flags in [vec!["--without", "core"], vec!["--with", "extra"]] {
        let out = run(iso.path(), &address, root.path(), json!({}), &flags);
        assert_eq!(out.status.code(), Some(4), "{out:?}");
        assert!(!root.path().join("result.txt").exists());
    }
    template(source.path(), QUESTION, "{{ label }}\n");
    assert_eq!(
        run(
            iso.path(),
            &address,
            root.path(),
            json!({}),
            &["--with", "core"]
        )
        .status
        .code(),
        Some(2)
    );
}
#[test]
fn invalid_part_declarations_are_load_errors() {
    let source = tempfile::tempdir().unwrap();
    for interview in [
        PARTS
            .replace("multiselect", "select")
            .replace("default: [core]", "default: core"),
        PARTS.replace("[core, extra]", "\"['core', 'extra']\""),
        PARTS.replace("[core, extra]", "[core, core]"),
        PARTS.replace("[core, extra]", "[Upper]"),
        PARTS.to_owned() + "    when: 'true'\n",
        format!(
            "{PARTS}{}",
            PARTS.replace("id: targets", "id: other_targets")
        ),
        format!("  - flow: dry-run\n{PARTS}"),
        format!(
            "  - group: optional\n    nodes:\n{}",
            PARTS
                .lines()
                .map(|l| format!("    {l}\n"))
                .collect::<String>()
        ),
    ] {
        template(source.path(), &interview, "sample\n");
        assert!(toha::Template::load(source.path()).is_err(), "{interview}");
    }
}
#[test]
fn staged_part_flags_survive_new_questions_continue_and_apply() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets|join(',') }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(iso.path(), &address, root.path(), json!({}), &[]));
    commit(root.path());
    template(
        source.path(),
        &(PARTS.to_owned() + QUESTION),
        "{{ targets|join(',') }} {{ label }}\n",
    );
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let stage = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&next)
        .arg(root.path())
        .args(["--update", "--with", "extra", "--async"])
        .output()
        .unwrap();
    assert!(
        [Some(0), Some(4)].contains(&stage.status.code()),
        "{stage:?}"
    );
    let file = iso.path().join("continue.json");
    std::fs::write(
        &file,
        json!({"template":next,"answers":{"label":"sample"}}).to_string(),
    )
    .unwrap();
    let continued = support::isolated_command(iso.path())
        .arg("continue")
        .arg(root.path())
        .arg(&file)
        .output()
        .unwrap();
    assert!(continued.status.success(), "{continued:?}");
    let applied = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "core,extra sample\n"
    );
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["parts"],
        json!(["core", "extra"])
    );
    assert!(String::from_utf8_lossy(&applied.stdout).contains("Parts added: extra"));
}
#[test]
fn fresh_stage_part_flags_and_like_parts_survive_resume() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets|join(',') }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--with", "extra", "--without", "core"],
    ));
    commit(root.path());
    let target = root.path().join("child");
    let stage = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .args(["--like", ".", "--with", "core", "--async"])
        .output()
        .unwrap();
    assert!(
        [Some(0), Some(4)].contains(&stage.status.code()),
        "{stage:?}"
    );
    let applied = support::isolated_command(iso.path())
        .arg("apply")
        .arg(&target)
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(target.join("result.txt")).unwrap(),
        "core,extra\n"
    );
}
#[test]
fn unversioned_folder_update_refuses_and_names_baseline() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets }}\n");
    repository(root.path());
    let address = support::formal_name(source.path());
    document(&run(iso.path(), &address, root.path(), json!({}), &[]));
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--baseline"));
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
}

#[test]
fn unavailable_recorded_commit_refuses_before_questions_and_names_baseline() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(iso.path(), &address, root.path(), json!({}), &[]));
    commit(root.path());
    let mut value = record(root.path(), ".toha.json");
    let missing = "f".repeat(40);
    value["applications"][0]["commit"] = json!(missing);
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("--baseline") && error.contains(&address) && error.contains(&missing),
        "{out:?}"
    );
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
}
#[test]
fn content_digest_updates_use_the_bundled_resolver_and_reject_different_content() {
    let iso = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    document(&run(
        iso.path(),
        "toha-demo",
        root.path(),
        json!({"title":"Sample Title","topic":"Sample Topic"}),
        &[],
    ));
    commit(root.path());
    let result = document(&run(
        iso.path(),
        "toha-demo",
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(result["status"], "already-current");
    assert!(result["application"]["prior_revision"]["content_digest"].is_string());
    let mut value = record(root.path(), ".toha.json");
    value["applications"][0]["content_digest"] = json!("f".repeat(64));
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let out = run(
        iso.path(),
        "toha-demo",
        root.path(),
        json!({}),
        &["--update"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--baseline"));
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
}
#[test]
fn a_removed_part_option_is_rejected_on_replay_without_writes() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--with", "extra"],
    ));
    commit(root.path());
    template(
        source.path(),
        &PARTS.replace("[core, extra]", "[core]"),
        "{{ targets }}\n",
    );
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let out = run(iso.path(), &next, root.path(), json!({}), &["--update"]);
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
}

#[test]
fn part_flags_settle_dynamic_defaults_after_earlier_answers() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(
        source.path(),
        &(QUESTION.to_owned()
            + &PARTS.replace(
                "default: [core]",
                "default: \"['core'] if label == 'first' else ['extra']\"",
            )),
        "{{ targets|join(',') }}\n",
    );
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--with", "core"],
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "core,extra\n"
    );
}
#[test]
fn applying_a_staged_update_refuses_new_part_flags() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), PARTS, "{{ targets }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(iso.path(), &address, root.path(), json!({}), &[]));
    commit(root.path());
    let stage = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--async"])
        .output()
        .unwrap();
    assert!(stage.status.success());
    let applied = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .args(["--with", "extra"])
        .output()
        .unwrap();
    assert_eq!(applied.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&applied.stderr).contains("stage TEMPLATE PATH --update"));
}

#[test]
fn a_parts_format_cannot_hide_an_invalid_settled_selection() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(
        source.path(),
        &(PARTS.to_owned() + "    format: \"['missing']\"\n"),
        "{{ targets }}\n",
    );
    let address = support::formal_name(source.path());
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--with", "extra"],
    );
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert!(!root.path().join("result.txt").exists());
}
