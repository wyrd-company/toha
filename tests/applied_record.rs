// ---
// relationships:
//   verifies: applied-record
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
    support::first_document(&out.stdout)
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

#[test]
fn every_written_record_validates_and_root_and_subpath_applications_share_one_record() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    let address = support::formal_name(source.path());
    let result = document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"first"}),
        &[],
    ));
    assert_eq!(result["record"]["path"], ".toha.json");
    let applied = record(root.path(), ".toha.json");
    let app = &applied["applications"][0];
    assert_eq!(app["target"], ".");
    assert_eq!(app["template"], address);
    assert_eq!(app["source"], address);
    assert!(app["commit"].is_null());
    assert_eq!(app["answers"], json!({"label":"first"}));
    assert_eq!(app["paths"]["result.txt"]["origin"], "toha");
    assert_eq!(
        app["paths"]["result.txt"]["blob"],
        git(root.path(), &["hash-object", "result.txt"])
    );
    assert!(app["paths"].get(".toha.json").is_none());
    let mut preserved = applied.clone();
    preserved["applications"][0]["generated"] = json!("2026-01-02T03:04:05Z[UTC]");
    let original_entry = preserved["applications"][0].clone();
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec(&preserved).unwrap(),
    )
    .unwrap();
    document(&run(
        iso.path(),
        &address,
        &root.path().join("nested"),
        json!({"label":"second"}),
        &[],
    ));
    let applied = record(root.path(), ".toha.json");
    assert_eq!(applied["applications"].as_array().unwrap().len(), 2);
    assert_eq!(applied["applications"][0], original_entry);
    assert_eq!(applied["applications"][1]["target"], "nested");
    assert_eq!(
        applied["applications"][1]["paths"]["result.txt"]["blob"],
        git(root.path(), &["hash-object", "nested/result.txt"])
    );
    document(&run(
        iso.path(),
        &address,
        &root.path().join("nested"),
        json!({"label":"third"}),
        &["--force"],
    ));
    let applied = record(root.path(), ".toha.json");
    assert_eq!(applied["applications"].as_array().unwrap().len(), 2);
    assert_eq!(applied["applications"][0]["answers"]["label"], "first");
    assert_eq!(applied["applications"][1]["answers"]["label"], "third");
    assert!(git(root.path(), &["for-each-ref", "refs/toha/"]).is_empty());
}

#[test]
fn record_override_is_repository_relative_and_written_for_an_unborn_repository() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    git(root.path(), &["init", "-q", "-b", "main"]);
    let address = support::formal_name(source.path());
    let file = iso.path().join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":{"label":"sample"}})).unwrap(),
    )
    .unwrap();
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(&address)
        .arg(root.path().join("nested"))
        .arg("--answers")
        .arg(file)
        .env("TOHA_RECORD", ".github/toha.json")
        .output()
        .unwrap();
    assert_eq!(document(&out)["record"]["path"], ".github/toha.json");
    assert!(!root.path().join(".toha.json").exists());
    assert_eq!(
        record(root.path(), ".github/toha.json")["applications"][0]["target"],
        "nested"
    );
}

#[test]
fn invalid_record_override_is_refused_before_questions_or_writes() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    for path in [
        "../record.json",
        "/record.json",
        "nested/../record.json",
        "C:/record.json",
        "",
    ] {
        let out = support::isolated_command(iso.path())
            .arg("apply")
            .arg(source.path())
            .arg(root.path())
            .env("TOHA_RECORD", path)
            .output()
            .unwrap();
        assert!(!out.status.success(), "accepted {path}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("TOHA_RECORD"),
            "{out:?}"
        );
        assert!(!root.path().join("result.txt").exists());
    }
}

#[cfg(unix)]
#[test]
fn record_override_rejects_symlink_components_before_reading() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    std::os::unix::fs::symlink(other.path(), root.path().join("linked")).unwrap();
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(source.path())
        .arg(root.path())
        .env("TOHA_RECORD", "linked/record.json")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("symbolic link"));
    assert!(!other.path().join("record.json").exists());
}

#[test]
fn computed_skipped_and_hook_values_are_not_answers_and_unused_environment_is_excluded() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(
        source.path(),
        &format!(
            "{QUESTION}  - id: skipped\n    type: text\n    prompt: Skip\n    when: 'false'\n  - id: derived\n    computed: label | upper\n"
        ),
        "{{ derived }} {{ toha_host_os }}\n",
    );
    repository(root.path());
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"credential-name"}),
        &[],
    ));
    let value = record(root.path(), ".toha.json");
    let app = &value["applications"][0];
    assert_eq!(app["answers"], json!({"label":"credential-name"}));
    assert_eq!(app["context"].as_object().unwrap().len(), 1);
    assert!(app["context"]["toha_host_os"].is_string());
}

#[test]
fn a_record_path_is_reserved_for_whole_file_and_injection_plans() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    std::fs::rename(
        source.path().join("template/result.txt"),
        source.path().join("template/.toha.json"),
    )
    .unwrap();
    let address = support::formal_name(source.path());
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("reserved"));
    assert!(!root.path().join(".toha.json").exists());
}

#[test]
fn update_replays_the_record_and_never_creates_refs_or_rewrites_an_already_current_record() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "first {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let source_uri = support::file_url(source.path());
    let first = format!(
        "{}@{}",
        source_uri,
        git(source.path(), &["rev-parse", "HEAD"])
    );
    document(&run(
        iso.path(),
        &first,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let current = document(&run(
        iso.path(),
        &first,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(current["status"], "already-current");
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
    std::fs::write(
        source.path().join("template/result.txt"),
        "second {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    let next = format!(
        "{}@{}",
        source_uri,
        git(source.path(), &["rev-parse", "HEAD"])
    );
    let result = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(result["status"], "applied");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "second sample\n"
    );
    let applied = record(root.path(), ".toha.json");
    assert_eq!(
        applied["applications"][0]["commit"],
        git(source.path(), &["rev-parse", "HEAD"])
    );
    assert!(git(root.path(), &["for-each-ref", "refs/toha/"]).is_empty());
}

#[test]
fn snapshot_ref_repositories_and_unversioned_applications_explicitly_rebaseline() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    git(
        root.path(),
        &["update-ref", "refs/toha/snapshots/legacy", "HEAD"],
    );
    let address = support::formal_name(source.path());
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--update"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--baseline"));
    assert!(!root.path().join(".toha.json").exists());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--baseline"],
    ));
    commit(root.path());
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--baseline"));
}

#[test]
fn malformed_record_is_refused_without_writes_and_retired_commands_are_absent() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    std::fs::write(root.path().join(".toha.json"), "<<<<<<< branch\n").unwrap();
    let address = support::formal_name(source.path());
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    );
    assert!(!out.status.success());
    assert!(!root.path().join("result.txt").exists());
    for command in ["snapshots", "init"] {
        let out = support::isolated_command(iso.path())
            .arg(command)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
    }
    let out = support::isolated_command(iso.path())
        .args(["apply", "--from", "legacy"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn git_answers_strip_url_user_information_from_the_record() {
    use indexmap::IndexMap;
    use toha::{Interview, RawAnswer, Seed};
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    template(
        source.path(),
        "  - id: remote\n    type: git\n    prompt: Repository\n",
        "example\n",
    );
    let template = toha::Template::load(source.path()).unwrap();
    let target = toha::staging::canonical_target(target.path()).unwrap();
    let seed = Seed {
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        defaults: IndexMap::new(),
        context: toha::context::InvocationContext::for_target(target.clone()),
    };
    let pending = match Interview::start(&template, seed).unwrap() {
        Interview::Asking(p) => p,
        _ => panic!("git question"),
    };
    let answers = [(
        toha::Id::parse("remote").unwrap(),
        RawAnswer(json!(
            "https://example-user:example-credential@example.invalid/project"
        )),
    )]
    .into_iter()
    .collect();
    let completed = match pending.answer(answers).unwrap() {
        Interview::Complete(c) => c,
        _ => panic!("completed"),
    };
    let plan = toha::Plan::build(&template, &completed, &target).unwrap();
    let values = serde_json::to_string(&plan.record.unwrap().answers).unwrap();
    assert!(!values.contains("example-user"));
    assert!(!values.contains("example-credential"));
    assert!(values.contains("https://example.invalid/project"));
}

#[test]
fn preexisting_dirty_paths_are_not_classified_as_hook_output() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    std::fs::write(root.path().join("operator.txt"), "uncommitted\n").unwrap();
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"example"}),
        &[],
    ));
    let applied = record(root.path(), ".toha.json");
    assert!(
        applied["applications"][0]["paths"]
            .get("operator.txt")
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn hook_paths_record_the_final_blob_and_hook_results_are_not_answers() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    let spec = source.path().join("template.yml");
    let mut text = std::fs::read_to_string(&spec).unwrap();
    text.push_str("hooks:\n  - id: generate\n    run: [sh, '-c', 'printf hook > created.txt; printf changed > result.txt; printf result']\n    capture: [stdout]\nmessages:\n  after-apply: '{{ generate.stdout }}'\n");
    std::fs::write(spec, text).unwrap();
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"example"}),
        &["--trust"],
    ));
    let applied = record(root.path(), ".toha.json");
    let app = &applied["applications"][0];
    assert_eq!(app["paths"]["created.txt"]["origin"], "hook");
    assert_eq!(
        app["paths"]["created.txt"]["blob"],
        git(root.path(), &["hash-object", "created.txt"])
    );
    assert_eq!(
        app["paths"]["result.txt"]["blob"],
        git(root.path(), &["hash-object", "result.txt"])
    );
    assert!(app["answers"].get("generate").is_none());
}

#[test]
fn context_references_include_partial_bodies_conditions_and_unused_values_are_absent() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{% include 'partial.txt' %}\n");
    repository(root.path());
    std::fs::write(
        source.path().join("partial.txt"),
        "{% if toha_is_admin %}admin{% else %}user{% endif %} {{ toha_host_arch }}",
    )
    .unwrap();
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"example"}),
        &[],
    ));
    let applied = record(root.path(), ".toha.json");
    let context = &applied["applications"][0]["context"];
    assert_eq!(context.as_object().unwrap().len(), 2);
    assert!(context["toha_is_admin"].is_boolean());
    assert!(context["toha_host_arch"].is_string());
    assert!(context.get("toha_env_user").is_none());
}

#[test]
fn a_changed_staged_application_is_refused_and_names_stage_update() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"example"}),
        &[],
    ));
    commit(root.path());
    let output = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--reanswer", "--async"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let mut value = record(root.path(), ".toha.json");
    value["applications"][0]["answers"]["label"] = json!("changed");
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let output = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("stage TEMPLATE PATH --update"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "example\n"
    );
}

#[test]
fn like_stage_uses_its_copied_seed_after_the_source_entry_is_removed() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"first"}),
        &[],
    ));
    commit(root.path());
    let target = root.path().join("nested");
    let output = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(&target)
        .args(["--like", ".", "--async"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec(&json!({"$schema":SCHEMA_ID,"applications":[]})).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let file = iso.path().join("continue.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":{}})).unwrap(),
    )
    .unwrap();
    let output = support::isolated_command(iso.path())
        .arg("continue")
        .arg(&target)
        .arg(&file)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = support::isolated_command(iso.path())
        .arg("apply")
        .arg(&target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(target.join("result.txt")).unwrap(),
        "first\n"
    );
}

#[test]
fn template_source_is_recorded_as_a_render_input_for_named_and_direct_selections() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(
        source.path(),
        QUESTION,
        "{{ toha_template_source | tojson }} {{ label }}\n",
    );
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    let output = support::isolated_command(iso.path())
        .args(["templates", "add", &address, "--alias", "example"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let file = iso.path().join("named.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":{"label":"sample"}})).unwrap(),
    )
    .unwrap();
    let output = support::isolated_command(iso.path())
        .args(["apply", "example"])
        .arg(root.path())
        .arg("--answers")
        .arg(&file)
        .output()
        .unwrap();
    document(&output);
    let named = record(root.path(), ".toha.json");
    assert_eq!(
        named["applications"][0]["context"]["toha_template_source"],
        address
    );
    commit(root.path());
    let next = root.path().join("direct");
    document(&run(
        iso.path(),
        &address,
        &next,
        json!({"label":"sample"}),
        &[],
    ));
    let value = record(root.path(), ".toha.json");
    assert_eq!(
        value["applications"][1]["context"]["toha_template_source"],
        Value::Null
    );
}

#[test]
fn recorded_environment_replays_as_the_base_while_the_candidate_uses_current_values() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ toha_env_user }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    let file = iso.path().join("environment.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":{"label":"sample"}})).unwrap(),
    )
    .unwrap();
    let invoke = |value: &str, update: bool| {
        let mut command = support::isolated_command(iso.path());
        command
            .arg("apply")
            .arg(&address)
            .arg(root.path())
            .arg("--answers")
            .arg(&file)
            .arg("--trust")
            .env("USER", value)
            .env("USERNAME", value);
        if update {
            command.arg("--update");
        }
        command.output().unwrap()
    };
    document(&invoke("first-value", false));
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["context"],
        json!({"toha_env_user":"first-value"})
    );
    commit(root.path());
    std::fs::write(root.path().join("result.txt"), "operator-value\n").unwrap();
    commit(root.path());
    let old_blob = git(root.path(), &["rev-parse", "HEAD~1:result.txt"]);
    std::fs::remove_file(
        root.path()
            .join(".git/objects")
            .join(&old_blob[..2])
            .join(&old_blob[2..]),
    )
    .unwrap();
    let outcome = document(&invoke("second-value", true));
    assert_eq!(outcome["merge"]["conflicted"], json!(["result.txt"]));
    assert_eq!(git(root.path(), &["show", ":1:result.txt"]), "first-value");
    assert_eq!(
        git(root.path(), &["show", ":2:result.txt"]),
        "operator-value"
    );
    assert_eq!(git(root.path(), &["show", ":3:result.txt"]), "second-value");
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["context"],
        json!({"toha_env_user":"second-value"})
    );
}

#[test]
fn applied_record_blobs_use_the_repository_object_format() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    git(
        root.path(),
        &["init", "-q", "-b", "main", "--object-format=sha256"],
    );
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    let blob = record(root.path(), ".toha.json")["applications"][0]["paths"]["result.txt"]["blob"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(blob.len(), 64);
    assert_eq!(blob, git(root.path(), &["hash-object", "result.txt"]));
}

#[cfg(unix)]
#[test]
fn recorded_mode_preserves_an_operator_chmod_during_a_content_update() {
    use std::os::unix::fs::PermissionsExt;
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "old {{ label }}\n");
    let manifest = source.path().join("template.yml");
    let text = std::fs::read_to_string(&manifest).unwrap()
        + "hooks:\n  - id: mode\n    run: [sh, '-c', 'chmod -x result.txt']\n";
    std::fs::write(&manifest, text).unwrap();
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--trust"],
    ));
    commit(root.path());
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["paths"]["result.txt"]["mode"],
        "100644"
    );
    std::fs::set_permissions(
        root.path().join("result.txt"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    commit(root.path());
    std::fs::write(
        source.path().join("template/result.txt"),
        "new {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let output = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update", "--trust"],
    ));
    assert_eq!(output["status"], "applied");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "new sample\n"
    );
    assert_ne!(
        std::fs::metadata(root.path().join("result.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert!(git(root.path(), &["ls-files", "-u"]).is_empty());
}

#[cfg(unix)]
#[test]
fn hook_symlink_base_uses_its_recorded_mode_when_the_operator_changes_its_kind() {
    hook_symlink_update(Some("file"));
}

#[cfg(unix)]
#[test]
fn hook_symlink_retarget_stays_a_link_in_the_worktree_and_index() {
    hook_symlink_update(None);
}

#[cfg(unix)]
#[test]
fn conflicting_hook_symlink_targets_preserve_the_operator_link() {
    hook_symlink_update(Some("link"));
}

#[cfg(unix)]
fn hook_symlink_update(operator: Option<&str>) {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    let manifest = source.path().join("template.yml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str(
        "hooks:\n  - id: link\n    run: [sh, '-c', 'rm -f link.txt; ln -s first link.txt']\n",
    );
    std::fs::write(&manifest, &text).unwrap();
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--trust"],
    ));
    commit(root.path());
    let app = record(root.path(), ".toha.json");
    assert_eq!(
        app["applications"][0]["paths"]["link.txt"]["mode"],
        "120000"
    );
    let link = root.path().join("link.txt");
    assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("first"));
    if let Some(kind) = operator {
        std::fs::remove_file(&link).unwrap();
        if kind == "file" {
            std::fs::write(&link, "operator\n").unwrap();
        } else {
            std::os::unix::fs::symlink("operator", &link).unwrap();
        }
        commit(root.path());
    }
    let operator_blob = git(root.path(), &["rev-parse", "HEAD:link.txt"]);
    std::fs::write(&manifest, text.replace("ln -s first", "ln -s second")).unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let output = run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update", "--trust"],
    );
    let output = document(&output);
    assert_eq!(output["status"], "applied");
    let recorded = record(root.path(), ".toha.json");
    let owned = &recorded["applications"][0]["paths"]["link.txt"];
    assert_eq!(owned["mode"], "120000");
    assert_eq!(
        git(
            root.path(),
            &["cat-file", "blob", owned["blob"].as_str().unwrap()]
        ),
        "second"
    );
    if let Some(kind) = operator {
        assert_eq!(output["merge"]["conflicted"], json!(["link.txt"]));
        assert!(
            output["merge"]["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["path"] == "link.txt"
                    && c["action"] == "conflicted"
                    && c["conflict"] == "content")
        );
        let stages = git(root.path(), &["ls-files", "-u", "link.txt"]);
        let rows: Vec<_> = stages.lines().collect();
        assert_eq!(rows.len(), 3, "{stages}");
        let blobs = [
            app["applications"][0]["paths"]["link.txt"]["blob"]
                .as_str()
                .unwrap(),
            operator_blob.as_str(),
            owned["blob"].as_str().unwrap(),
        ];
        for (i, mode) in [
            "120000",
            if kind == "file" { "100644" } else { "120000" },
            "120000",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                rows[i],
                format!("{mode} {} {}\tlink.txt", blobs[i], i + 1),
                "{stages}"
            );
        }
        assert_eq!(git(root.path(), &["show", ":1:link.txt"]), "first");
        assert_eq!(git(root.path(), &["show", ":2:link.txt"]), "operator");
        assert_eq!(git(root.path(), &["show", ":3:link.txt"]), "second");
        if kind == "file" {
            assert!(std::fs::symlink_metadata(&link).unwrap().is_file());
            assert_eq!(std::fs::read(&link).unwrap(), b"operator\n");
        } else {
            assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("operator"));
        }
    } else {
        assert_eq!(output["merge"]["conflicted"], json!([]));
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("second"));
        assert_eq!(
            git(root.path(), &["ls-files", "--stage", "link.txt"]),
            format!("120000 {} 0\tlink.txt", owned["blob"].as_str().unwrap())
        );
        assert_eq!(
            git(root.path(), &["status", "--porcelain", "link.txt"]),
            "M  link.txt".trim()
        );
        assert!(git(root.path(), &["diff", "--", "link.txt"]).is_empty());
    }
}

#[test]
fn an_application_cannot_claim_the_record_as_an_owned_path() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    let mut value = record(root.path(), ".toha.json");
    let owned = value["applications"][0]["paths"]["result.txt"].clone();
    value["applications"][0]["paths"][".toha.json"] = owned;
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let output = run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"changed"}),
        &["--force"],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("cannot own itself"),
        "{output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "sample\n"
    );
}

#[test]
fn a_mismatched_render_uses_the_recorded_object_as_its_merge_base() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "old {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    let retained = git(root.path(), &["hash-object", "seed.txt"]);
    let mut value = record(root.path(), ".toha.json");
    value["applications"][0]["paths"]["result.txt"]["blob"] = json!(retained);
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    std::fs::write(root.path().join("result.txt"), "operator\n").unwrap();
    commit(root.path());
    std::fs::write(
        source.path().join("template/result.txt"),
        "candidate {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let output = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(output["merge"]["conflicted"], json!(["result.txt"]));
    assert_eq!(git(root.path(), &["rev-parse", ":1:result.txt"]), retained);
}

#[test]
fn unavailable_recorded_objects_conflict_for_changed_additions_and_removals() {
    for remove in [false, true] {
        let iso = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        template(source.path(), QUESTION, "old {{ label }}\n");
        repository(source.path());
        repository(root.path());
        let address = support::file_url(source.path());
        document(&run(
            iso.path(),
            &address,
            root.path(),
            json!({"label":"sample"}),
            &[],
        ));
        commit(root.path());
        let mut value = record(root.path(), ".toha.json");
        value["applications"][0]["paths"]["result.txt"]["blob"] = json!("f".repeat(40));
        std::fs::write(
            root.path().join(".toha.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        std::fs::write(root.path().join("result.txt"), "operator\n").unwrap();
        commit(root.path());
        if remove {
            std::fs::remove_file(source.path().join("template/result.txt")).unwrap();
            std::fs::write(source.path().join("template/.keep"), "").unwrap();
        } else {
            std::fs::write(
                source.path().join("template/result.txt"),
                "candidate {{ label }}\n",
            )
            .unwrap();
        }
        commit(source.path());
        let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
        let output = document(&run(
            iso.path(),
            &next,
            root.path(),
            json!({}),
            &["--update"],
        ));
        assert_eq!(output["merge"]["conflicted"], json!(["result.txt"]));
        assert_eq!(
            std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
            "operator\n"
        );
        let stages = git(root.path(), &["ls-files", "-u", "result.txt"]);
        assert!(stages.contains(" 2\tresult.txt"), "{stages}");
        assert_eq!(stages.contains(" 3\tresult.txt"), !remove, "{stages}");
        assert!(!stages.contains(" 1\tresult.txt"), "{stages}");
    }
}

#[test]
fn a_hook_path_never_reconstructs_its_missing_object_from_rendered_bytes() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "old {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    let mut value = record(root.path(), ".toha.json");
    let old = value["applications"][0]["paths"]["result.txt"]["blob"]
        .as_str()
        .unwrap()
        .to_owned();
    value["applications"][0]["paths"]["result.txt"]["origin"] = json!("hook");
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    std::fs::write(root.path().join("result.txt"), "operator\n").unwrap();
    commit(root.path());
    std::fs::remove_file(
        root.path()
            .join(".git/objects")
            .join(&old[..2])
            .join(&old[2..]),
    )
    .unwrap();
    std::fs::write(
        source.path().join("template/result.txt"),
        "candidate {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let output = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(output["merge"]["conflicted"], json!(["result.txt"]));
    let stages = git(root.path(), &["ls-files", "-u", "result.txt"]);
    assert!(!stages.contains(" 1\tresult.txt"), "{stages}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "operator\n"
    );
}

#[cfg(unix)]
#[test]
fn a_hook_chmod_without_a_content_change_is_captured_and_merged() {
    use std::os::unix::fs::PermissionsExt;
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    let manifest = source.path().join("template.yml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("hooks:\n  - id: generate\n    run: [sh, '-c', 'printf same > hook.txt']\n");
    std::fs::write(&manifest, &text).unwrap();
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--trust"],
    ));
    commit(root.path());
    std::fs::write(
        &manifest,
        text.replace(
            "printf same > hook.txt",
            "printf same > hook.txt; chmod +x hook.txt",
        ),
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update", "--trust"],
    ));
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["paths"]["hook.txt"]["mode"],
        "100755"
    );
    assert_ne!(
        std::fs::metadata(root.path().join("hook.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn a_missing_referenced_context_value_requires_rebaseline_before_writes() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ toha_host_os }} {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    let mut value = record(root.path(), ".toha.json");
    value["applications"][0]["context"] = json!({});
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    commit(root.path());
    let before = std::fs::read(root.path().join("result.txt")).unwrap();
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert!(!out.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("referenced render context") && text.contains("--baseline"),
        "{text}"
    );
    assert_eq!(
        std::fs::read(root.path().join("result.txt")).unwrap(),
        before
    );
}

#[test]
fn a_dirty_record_outside_the_target_is_refused_and_names_its_path() {
    for staged in [false, true] {
        let iso = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        template(source.path(), QUESTION, "{{ label }}\n");
        repository(source.path());
        repository(root.path());
        let address = support::file_url(source.path());
        let target = root.path().join("nested");
        document(&run(
            iso.path(),
            &address,
            &target,
            json!({"label":"sample"}),
            &[],
        ));
        commit(root.path());
        let mut value = record(root.path(), ".toha.json");
        value["applications"][0]["answers"]["label"] = json!("edited");
        std::fs::write(
            root.path().join(".toha.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        if staged {
            git(root.path(), &["add", ".toha.json"]);
        }
        let out = run(iso.path(), &address, &target, json!({}), &["--update"]);
        assert!(!out.status.success());
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains(".toha.json") && text.contains("HEAD"),
            "{text}"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("result.txt")).unwrap(),
            "sample\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn capture_excludes_gitlinks_ignored_hook_files_and_paths_outside_the_target() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let sandbox = tempfile::tempdir().unwrap();
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    repository(&root);
    let vendor = root.join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();
    repository(&vendor);
    std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    commit(&root);
    template(source.path(), QUESTION, "{{ label }}\n");
    std::fs::create_dir_all(source.path().join("template/vendor")).unwrap();
    std::fs::write(
        source.path().join("template/vendor/seed.txt"),
        "generated\n",
    )
    .unwrap();
    let manifest = source.path().join("template.yml");
    let text = std::fs::read_to_string(&manifest).unwrap()
        + "hooks:\n  - id: generate\n    run: [sh, '-c', 'printf changed > vendor/seed.txt; printf ignored > ignored.txt; printf outside > ../outside.txt; printf owned > owned.txt']\n";
    std::fs::write(manifest, text).unwrap();
    let address = support::formal_name(source.path());
    document(&run(
        iso.path(),
        &address,
        &root,
        json!({"label":"sample"}),
        &["--trust", "--force"],
    ));
    let value = record(&root, ".toha.json");
    let paths = value["applications"][0]["paths"].as_object().unwrap();
    assert_eq!(
        paths.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["owned.txt", "result.txt"]
    );
    assert!(sandbox.path().join("outside.txt").exists());
    assert_eq!(
        git(&root, &["ls-files", "--stage", "vendor"])
            .split_whitespace()
            .next(),
        Some("160000")
    );
}

#[cfg(unix)]
#[test]
fn a_record_write_failure_rolls_back_merged_files_and_preserves_the_index() {
    use std::os::unix::fs::PermissionsExt;
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "old {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let target = root.path().join("nested");
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        &target,
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    let old_record = std::fs::read(root.path().join(".toha.json")).unwrap();
    let old_index = std::fs::read(root.path().join(".git/index")).unwrap();
    std::fs::write(
        source.path().join("template/result.txt"),
        "new {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let permissions = std::fs::metadata(root.path()).unwrap().permissions();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = run(iso.path(), &next, &target, json!({}), &["--update"]);
    std::fs::set_permissions(root.path(), permissions).unwrap();
    assert!(
        !out.status.success(),
        "record-write fault must be reached: {out:?}"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains(".toha.json"), "{text}");
    assert_eq!(
        std::fs::read_to_string(target.join("result.txt")).unwrap(),
        "old sample\n"
    );
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        old_record
    );
    assert_eq!(
        std::fs::read(root.path().join(".git/index")).unwrap(),
        old_index
    );
}

const DOCUMENT_URI: &str = "https://example.invalid/schemas/catalog/1";
fn document_template(root: &Path) {
    template(
        root,
        &format!(
            "  - id: catalog\n    type: document\n    prompt: Catalog\n    schema: {DOCUMENT_URI}\n    input: catalog.json\n  - id: label\n    type: text\n    prompt: '{{{{ catalog.title }}}} label'\n    required: true\n"
        ),
        "{{ catalog.title }} {{ label }}\n",
    );
    let manifest = root.join("template.yml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("interview:\n", "schemas: schemas\ninterview:\n");
    std::fs::write(manifest, text).unwrap();
    std::fs::create_dir_all(root.join("schemas")).unwrap();
    std::fs::write(root.join("schemas/catalog.json"),serde_json::to_vec(&json!({"$id":DOCUMENT_URI,"type":"object","required":["title"],"properties":{"title":{"type":"string"}}})).unwrap()).unwrap();
}
fn catalog(title: &str) -> Value {
    json!({"$schema":DOCUMENT_URI,"title":title,"extra":[1,{"note":"preserved"}]})
}
fn write_json(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn update_uses_current_documents_while_the_base_renders_only_the_recorded_document() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    write_json(&root.path().join("catalog.json"), catalog("First"));
    commit(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    let initial = record(root.path(), ".toha.json");
    assert_eq!(
        initial["applications"][0]["answers"]["catalog"],
        catalog("First")
    );
    commit(root.path());
    let blob = initial["applications"][0]["paths"]["result.txt"]["blob"]
        .as_str()
        .unwrap();
    std::fs::write(root.path().join("result.txt"), "operator\n").unwrap();
    write_json(&root.path().join("catalog.json"), catalog("Second"));
    commit(root.path());
    std::fs::remove_file(
        root.path()
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    let output = document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(output["merge"]["conflicted"], json!(["result.txt"]));
    assert_eq!(git(root.path(), &["show", ":1:result.txt"]), "First sample");
    assert_eq!(
        git(root.path(), &["show", ":3:result.txt"]),
        "Second sample"
    );
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["answers"]["catalog"],
        catalog("Second")
    );
}

#[test]
fn an_update_never_replays_an_old_document_when_the_current_input_is_missing() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    write_json(&root.path().join("catalog.json"), catalog("First"));
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    std::fs::remove_file(root.path().join("catalog.json")).unwrap();
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert_eq!(support::first_document(&out.stdout)["status"], "questions");
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
    let input = iso.path().join("catalog.json");
    write_json(&input, catalog("Input"));
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"catalog":catalog("Answer")}),
        &["--update", "--input", input.to_str().unwrap()],
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "Input sample\n"
    );
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["answers"]["catalog"],
        catalog("Input")
    );
}

#[test]
fn staged_update_inputs_remain_session_answers_until_apply() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    write_json(&root.path().join("catalog.json"), catalog("First"));
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    std::fs::remove_file(root.path().join("catalog.json")).unwrap();
    commit(root.path());
    let input = iso.path().join("input.json");
    write_json(&input, catalog("Session"));
    let out = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--reanswer", "--async", "--input"])
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    write_json(&input, catalog("Replacement"));
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .arg("--input")
        .arg(&input)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already answered"),
        "{out:?}"
    );
    let answers = iso.path().join("continued.json");
    write_json(
        &answers,
        json!({"template":address,"answers":{"label":"continued"}}),
    );
    let out = support::isolated_command(iso.path())
        .arg("continue")
        .arg(root.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "Session continued\n"
    );
    assert_eq!(
        record(root.path(), ".toha.json")["applications"][0]["answers"]["catalog"],
        catalog("Session")
    );
}

#[test]
fn a_document_answer_submitted_during_update_continue_survives_a_later_default() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    write_json(&root.path().join("catalog.json"), catalog("First"));
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    std::fs::remove_file(root.path().join("catalog.json")).unwrap();
    commit(root.path());
    let out = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--reanswer", "--async"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let answers = iso.path().join("continued.json");
    write_json(
        &answers,
        json!({"template":address,"answers":{"catalog":catalog("Session")}}),
    );
    let out = support::isolated_command(iso.path())
        .arg("continue")
        .arg(root.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    write_json(&root.path().join("catalog.json"), catalog("Later default"));
    commit(root.path());
    write_json(
        &answers,
        json!({"template":address,"answers":{"label":"continued"}}),
    );
    let out = support::isolated_command(iso.path())
        .arg("continue")
        .arg(root.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "Session continued\n"
    );
}

#[test]
fn update_and_baseline_input_routing_errors_keep_exit_two_and_do_not_write() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    let input = iso.path().join("bad.json");
    write_json(&input, json!({"title":"Missing schema"}));
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({}),
        &["--baseline", "--input", input.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(support::first_document(&out.stdout)["kind"], "input");
    assert!(!root.path().join("result.txt").exists());
    assert!(!root.path().join(".toha.json").exists());

    let good = iso.path().join("good.json");
    write_json(&good, catalog("Example"));
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--input", good.to_str().unwrap()],
    ));
    commit(root.path());
    let before_record = std::fs::read(root.path().join(".toha.json")).unwrap();
    let before_index = std::fs::read(root.path().join(".git/index")).unwrap();
    let before_file = std::fs::read(root.path().join("result.txt")).unwrap();
    let unchanged = || {
        assert_eq!(
            std::fs::read(root.path().join(".toha.json")).unwrap(),
            before_record
        );
        assert_eq!(
            std::fs::read(root.path().join(".git/index")).unwrap(),
            before_index
        );
        assert_eq!(
            std::fs::read(root.path().join("result.txt")).unwrap(),
            before_file
        );
    };
    for mode in ["--update", "--baseline"] {
        let out = run(
            iso.path(),
            &address,
            root.path(),
            json!({}),
            &[mode, "--input", input.to_str().unwrap()],
        );
        assert_eq!(out.status.code(), Some(2), "{mode}: {out:?}");
        assert_eq!(support::first_document(&out.stdout)["kind"], "input");
        unchanged();
        let out = support::isolated_command(iso.path())
            .arg("apply")
            .arg(&address)
            .arg(root.path())
            .args([mode, "--input"])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{mode}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("$schema"),
            "{out:?}"
        );
        unchanged();
        let out = support::isolated_command(iso.path())
            .arg("stage")
            .arg(&address)
            .arg(root.path())
            .args([mode, "--async", "--input"])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{mode}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("$schema"),
            "{out:?}"
        );
        unchanged();
    }
    let out = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--reanswer", "--async"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let stage = std::fs::read_dir(support::staged_dir(iso.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let before_stage = std::fs::read(&stage).unwrap();
    let answers = iso.path().join("continued.json");
    write_json(
        &answers,
        json!({"template":address,"answers":{"label":"continued"}}),
    );
    for (route, with_answers) in [("apply", false), ("continue", true), ("continue", false)] {
        let mut command = support::isolated_command(iso.path());
        command.arg(route).arg(root.path());
        if with_answers {
            command.arg(&answers);
        }
        let out = command.arg("--input").arg(&input).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{route}: {out:?}");
        let diagnostic = if with_answers {
            "cannot be used"
        } else {
            "$schema"
        };
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(diagnostic),
            "{out:?}"
        );
        unchanged();
        assert_eq!(std::fs::read(&stage).unwrap(), before_stage);
    }
}

#[cfg(unix)]
#[test]
fn a_reconstructed_whole_file_includes_injections_and_never_runs_old_hooks() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let body = "{\n  \"rendered\": \"old {{ label }}\",\n  \"injected\": \"placeholder\"\n}\n";
    template(source.path(), QUESTION, body);
    std::fs::rename(
        source.path().join("template/result.txt"),
        source.path().join("template/result.json"),
    )
    .unwrap();
    let counter = iso.path().join("counter.txt");
    let manifest = source.path().join("template.yml");
    let mut configuration: Value =
        serde_norway::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    configuration["inject"] =
        json!([{"into":"result.json","struct":{"path":"injected","value":"settled"}}]);
    configuration["hooks"] = json!([{"id":"count","run":["sh","-c",format!("printf run >> {}; printf 1",counter.display())],"capture":["stdout"],"parse":"json"}]);
    configuration["messages"] = json!({"after-apply":"{{ 1 / count }}"});
    let text = serde_norway::to_string(&configuration).unwrap();
    std::fs::write(manifest, text).unwrap();
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--trust"],
    ));
    let initial = std::fs::read_to_string(root.path().join("result.json")).unwrap();
    let app = record(root.path(), ".toha.json");
    let blob = app["applications"][0]["paths"]["result.json"]["blob"]
        .as_str()
        .unwrap();
    commit(root.path());
    std::fs::write(
        root.path().join("result.json"),
        initial.replace("old sample", "operator"),
    )
    .unwrap();
    commit(root.path());
    std::fs::remove_file(
        root.path()
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    std::fs::write(
        source.path().join("template/result.json"),
        body.replace("old", "new"),
    )
    .unwrap();
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let output = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update", "--trust"],
    ));
    assert_eq!(output["merge"]["conflicted"], json!(["result.json"]));
    assert_eq!(
        git(root.path(), &["show", ":1:result.json"]),
        initial.trim()
    );
    assert_eq!(std::fs::read_to_string(counter).unwrap(), "runrun");
}

#[test]
fn base_reconstruction_never_repeats_the_recorded_git_setup() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(
        source.path(),
        "  - id: remote\n    type: git\n    prompt: Repository\n",
        "example\n",
    );
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"remote":address}),
        &[],
    ));
    commit(root.path());
    assert_eq!(
        document(&run(
            iso.path(),
            &address,
            root.path(),
            json!({}),
            &["--update"]
        ))["status"],
        "already-current"
    );
}

#[test]
fn copied_like_seeds_keep_the_selected_source_and_target_identity() {
    for mutation in [0, 1, 2] {
        let iso = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        template(source.path(), QUESTION, "{{ label }}\n");
        repository(root.path());
        let address = support::formal_name(source.path());
        document(&run(
            iso.path(),
            &address,
            root.path(),
            json!({"label":"sample"}),
            &[],
        ));
        commit(root.path());
        let target = root.path().join("nested");
        let out = support::isolated_command(iso.path())
            .arg("stage")
            .arg(&address)
            .arg(&target)
            .args(["--like", ".", "--async"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(4), "{out:?}");
        let state = if cfg!(target_os = "macos") {
            iso.path()
                .join("home/Library/Application Support/toha/staged")
        } else {
            iso.path().join("toha/staged")
        };
        let store = toha::staging::Store::new(state);
        let canonical = toha::staging::canonical_target(&target).unwrap();
        let mut saved = store.load(&canonical).unwrap().unwrap();
        match mutation {
            0 => {
                let value = saved.seed_entry.as_mut().unwrap();
                value["source"] = json!("forge:example/other");
                value["template"] = json!("forge:example/other");
            }
            1 => saved.seed_entry.as_mut().unwrap()["target"] = json!("other"),
            _ => saved.seed_entry = None,
        }
        store.save(&canonical, &saved).unwrap();
        let answers = iso.path().join("continue.json");
        write_json(&answers, json!({"template":address,"answers":{}}));
        let out = support::isolated_command(iso.path())
            .arg("continue")
            .arg(&target)
            .arg(answers)
            .output()
            .unwrap();
        assert!(!out.status.success(), "mutation {mutation}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("abort and stage again"),
            "{out:?}"
        );
        assert!(!target.join("result.txt").exists());
    }
}

#[test]
fn update_continue_admits_a_new_default_only_for_an_unanswered_document() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    document_template(source.path());
    repository(source.path());
    repository(root.path());
    write_json(&root.path().join("catalog.json"), catalog("First"));
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    commit(root.path());
    std::fs::remove_file(root.path().join("catalog.json")).unwrap();
    commit(root.path());
    let out = support::isolated_command(iso.path())
        .arg("stage")
        .arg(&address)
        .arg(root.path())
        .args(["--update", "--reanswer", "--async"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    write_json(&root.path().join("catalog.json"), catalog("Current"));
    commit(root.path());
    let answers = iso.path().join("continued.json");
    write_json(
        &answers,
        json!({"template":address,"answers":{"label":"continued"}}),
    );
    let out = support::isolated_command(iso.path())
        .arg("continue")
        .arg(root.path())
        .arg(&answers)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let out = support::isolated_command(iso.path())
        .arg("apply")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "Current continued\n"
    );
}

#[test]
fn a_root_checkout_rename_reconstructs_the_base_with_its_recorded_target_name() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let sandbox = tempfile::tempdir().unwrap();
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    template(source.path(), QUESTION, "{{ toha_target_name }}\n");
    repository(source.path());
    repository(&root);
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        &root,
        json!({"label":"sample"}),
        &[],
    ));
    let value = record(&root, ".toha.json");
    assert_eq!(
        value["applications"][0]["context"]["toha_target_name"],
        "project"
    );
    let blob = value["applications"][0]["paths"]["result.txt"]["blob"]
        .as_str()
        .unwrap();
    commit(&root);
    std::fs::write(root.join("result.txt"), "operator\n").unwrap();
    commit(&root);
    let renamed = sandbox.path().join("worktree");
    std::fs::rename(root, &renamed).unwrap();
    std::fs::remove_file(
        renamed
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    let result = document(&run(
        iso.path(),
        &address,
        &renamed,
        json!({}),
        &["--update"],
    ));
    assert_eq!(result["merge"]["conflicted"], json!(["result.txt"]));
    assert_eq!(git(&renamed, &["show", ":1:result.txt"]), "project");
    assert_eq!(git(&renamed, &["show", ":3:result.txt"]), "worktree");
    assert_eq!(
        record(&renamed, ".toha.json")["applications"][0]["context"]["toha_target_name"],
        "worktree"
    );
}

#[cfg(unix)]
#[test]
fn record_files_use_ordinary_permissions_after_apply_and_update() {
    use std::os::unix::fs::PermissionsExt;
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    let answers = iso.path().join("answers.json");
    write_json(
        &answers,
        json!({"template":address,"answers":{"label":"sample"}}),
    );
    let invoke = |update: bool| {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "umask 022; exec \"$@\"",
                "sh",
                env!("CARGO_BIN_EXE_toha"),
                "apply",
            ])
            .arg(&address)
            .arg(root.path())
            .arg("--answers")
            .arg(&answers);
        if update {
            command.arg("--update");
        }
        support::isolate(&mut command, iso.path());
        document(&command.output().unwrap());
        assert_eq!(
            std::fs::metadata(root.path().join(".toha.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    };
    invoke(false);
    commit(root.path());
    std::fs::set_permissions(
        root.path().join(".toha.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    std::fs::write(
        source.path().join("template/result.txt"),
        "new {{ label }}\n",
    )
    .unwrap();
    commit(source.path());
    invoke(true);
}

#[cfg(unix)]
#[test]
fn a_default_record_symlink_names_the_path_without_an_unset_override() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(root.path());
    std::fs::write(root.path().join("record.json"), "{}").unwrap();
    std::os::unix::fs::symlink("record.json", root.path().join(".toha.json")).unwrap();
    commit(root.path());
    let out = run(
        iso.path(),
        &support::formal_name(source.path()),
        root.path(),
        json!({"label":"sample"}),
        &[],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let diagnostic = String::from_utf8_lossy(&out.stderr);
    assert!(diagnostic.contains(".toha.json"), "{out:?}");
    assert!(diagnostic.contains("symbolic link"), "{out:?}");
    assert!(!diagnostic.contains("TOHA_RECORD"), "{out:?}");
    assert!(!root.path().join("result.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("record.json")).unwrap(),
        "{}"
    );
}

#[cfg(unix)]
#[test]
fn a_pure_base_apply_failure_names_rebaseline_and_preserves_the_project() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "{{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    std::fs::remove_file(root.path().join("result.txt")).unwrap();
    std::fs::create_dir(root.path().join("result.txt")).unwrap();
    std::fs::write(root.path().join("result.txt/operator.txt"), "operator\n").unwrap();
    commit(root.path());
    let before = std::fs::read(root.path().join(".toha.json")).unwrap();
    let index = std::fs::read(root.path().join(".git/index")).unwrap();
    let out = run(iso.path(), &address, root.path(), json!({}), &["--update"]);
    assert!(!out.status.success(), "{out:?}");
    let diagnostic = String::from_utf8_lossy(&out.stderr);
    assert!(diagnostic.contains("cannot render merge base"), "{out:?}");
    assert!(diagnostic.contains("--baseline"), "{out:?}");
    assert_eq!(
        std::fs::read(root.path().join(".toha.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(root.path().join(".git/index")).unwrap(),
        index
    );
    assert!(root.path().join("result.txt").is_dir());
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt/operator.txt")).unwrap(),
        "operator\n"
    );
}

#[test]
fn autocrlf_update_merges_in_repository_form() {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let before = "one\ntwo\nthree\nfour\nfive\n";
    template(source.path(), QUESTION, before);
    repository(source.path());
    repository(root.path());
    git(root.path(), &["config", "core.autocrlf", "true"]);
    let source_uri = support::file_url(source.path());
    let address = format!(
        "{}@{}",
        source_uri,
        git(source.path(), &["rev-parse", "HEAD"])
    );
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &[],
    ));
    // Simulate an existing CRLF blob committed before normalization was enabled.
    // The repository's effective setting remains autocrlf=true for UpdateSession.
    std::fs::write(root.path().join("result.txt"), before.replace('\n', "\r\n")).unwrap();
    git(root.path(), &["-c", "core.autocrlf=false", "add", "."]);
    git(
        root.path(),
        &["commit", "-q", "-m", "existing CRLF content"],
    );
    std::fs::write(
        source.path().join("template/result.txt"),
        "one\ntwo\nthree\nfour\nFIVE\n",
    )
    .unwrap();
    commit(source.path());
    let address = format!(
        "{}@{}",
        source_uri,
        git(source.path(), &["rev-parse", "HEAD"])
    );
    let out = run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"sample"}),
        &["--update"],
    );
    let value = document(&out);
    assert_eq!(
        std::fs::read(root.path().join("result.txt")).unwrap(),
        b"one\r\ntwo\r\nthree\r\nfour\r\nFIVE\r\n",
        "{value}"
    );
    assert!(
        git(root.path(), &["ls-files", "-u"]).is_empty(),
        "unedited text must not conflict"
    );
}
