// ---
// relationships:
//   implements: template-format
// ---
//! End-to-end content-injection behavior through the real `toha apply` binary.
//! Resolver-level cases live in the `inject` unit tests; these prove the whole
//! plan-and-apply path, the CLI report vocabulary, and the failure contract.

#[allow(dead_code)]
mod support;

use std::{fs, path::Path, process::Output};

/// Writes a template folder with the given `template.yml` and an empty source
/// subdirectory (injection-only templates generate no whole files).
fn make_template(root: &Path, yaml: &str) -> std::path::PathBuf {
    let folder = root.join("tmpl");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template.yml"), yaml).unwrap();
    folder
}

/// Runs `toha apply --answers <empty> <template> <target> [extra]` in isolation.
/// Injection templates have no interview, so an empty answers document drives
/// the headless route without a terminal.
fn apply(template: &Path, target: &Path, extra: &[&str]) -> Output {
    let isolation = tempfile::tempdir().unwrap();
    let answers = isolation.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let mut command = support::isolated_command(isolation.path());
    command
        .arg("apply")
        .arg("--answers")
        .arg(&answers)
        .arg(template)
        .arg(target)
        .args(extra);
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const REGION_TEMPLATE: &str = r#"name: sample
inject:
  - into: "config/app.toml"
    region: features
    content: |
      analytics_enabled = true
      analytics_sample_rate = 0.1
    anchor: { after: "[application]", occurrence: only }
"#;

fn region_target(root: &Path) -> std::path::PathBuf {
    let target = root.join("out");
    fs::create_dir_all(target.join("config")).unwrap();
    fs::write(
        target.join("config/app.toml"),
        "[application]\nname = \"sample-service\"\n",
    )
    .unwrap();
    target
}

#[test]
fn region_injects_once_and_second_apply_is_a_byte_no_op() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(root.path(), REGION_TEMPLATE);
    let target = region_target(root.path());
    let file = target.join("config/app.toml");

    let first = apply(&template, &target, &[]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr(&first));
    let after_first = fs::read(&file).unwrap();
    // The region body is placed once, after the anchor, with visible markers.
    let text = String::from_utf8(after_first.clone()).unwrap();
    assert!(text.contains("# >>> toha:region features >>>"));
    assert!(text.contains("analytics_enabled = true"));
    assert!(text.contains("# <<< toha:end features sha256:"));
    assert!(text.starts_with("[application]\n"));
    assert!(text.ends_with("name = \"sample-service\"\n"));
    assert!(stdout(&first).contains("config/app.toml"));

    // A second apply changes nothing: the file is byte-identical and no target
    // is reported.
    let second = apply(&template, &target, &[]);
    assert_eq!(second.status.code(), Some(0), "{}", stderr(&second));
    assert_eq!(
        fs::read(&file).unwrap(),
        after_first,
        "second apply changed bytes"
    );
    assert!(
        !stdout(&second).contains("config/app.toml"),
        "second apply reported a write: {}",
        stdout(&second)
    );
}

#[test]
fn region_drift_refuses_without_force_and_force_replaces_only_the_region() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(root.path(), REGION_TEMPLATE);
    let target = region_target(root.path());
    let file = target.join("config/app.toml");
    apply(&template, &target, &[]);

    // The operator edits inside the owned span.
    let drifted = fs::read_to_string(&file)
        .unwrap()
        .replace("analytics_enabled = true", "analytics_enabled = false");
    fs::write(&file, &drifted).unwrap();

    let refused = apply(&template, &target, &[]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("config/app.toml (features)"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        drifted,
        "drift wrote the file"
    );

    // --force replaces only the region and restores the owned body.
    let forced = apply(&template, &target, &["--force"]);
    assert_eq!(forced.status.code(), Some(0), "{}", stderr(&forced));
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("analytics_enabled = true"));
    assert!(!text.contains("analytics_enabled = false"));
    assert!(text.ends_with("name = \"sample-service\"\n"));
}

const STRUCT_TEMPLATE: &str = r#"name: sample
inject:
  - into: "package.json"
    struct:
      path: "scripts.build"
      value: "tsc"
"#;

fn json_target(root: &Path, contents: &str) -> std::path::PathBuf {
    let target = root.join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), contents).unwrap();
    target
}

#[test]
fn struct_json_inserts_typed_value_and_converges_without_force() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(root.path(), STRUCT_TEMPLATE);
    let target = json_target(
        root.path(),
        "{\n  \"name\": \"sample-app\",\n  \"scripts\": {\n    \"start\": \"node index.js\"\n  }\n}",
    );
    let file = target.join("package.json");

    let first = apply(&template, &target, &[]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr(&first));
    let after_first = fs::read(&file).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&after_first).unwrap();
    assert_eq!(value["scripts"]["build"], serde_json::json!("tsc"));
    assert_eq!(
        value["scripts"]["start"],
        serde_json::json!("node index.js")
    );

    // A second apply is a byte no-op.
    let second = apply(&template, &target, &[]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(fs::read(&file).unwrap(), after_first);
    assert!(!stdout(&second).contains("package.json"));

    // An operator change to the owned value converges without --force.
    let edited = fs::read_to_string(&file)
        .unwrap()
        .replace("\"tsc\"", "\"webpack\"");
    fs::write(&file, edited).unwrap();
    let converge = apply(&template, &target, &[]);
    assert_eq!(converge.status.code(), Some(0), "{}", stderr(&converge));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(value["scripts"]["build"], serde_json::json!("tsc"));
}

#[test]
fn dry_run_reports_inject_then_update_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(root.path(), STRUCT_TEMPLATE);
    let target = json_target(root.path(), "{\n  \"name\": \"sample-app\"\n}");
    let file = target.join("package.json");
    let before = fs::read(&file).unwrap();

    let dry = apply(&template, &target, &["--dry-run"]);
    assert_eq!(dry.status.code(), Some(0), "{}", stderr(&dry));
    assert!(
        stdout(&dry).contains("inject package.json (scripts.build)"),
        "{}",
        stdout(&dry)
    );
    assert_eq!(fs::read(&file).unwrap(), before, "dry-run wrote the file");

    // After the value exists, a changed desired value reports as an update.
    apply(&template, &target, &[]);
    let changed = make_template(root.path(), &STRUCT_TEMPLATE.replace("tsc", "rollup"));
    let dry = apply(&changed, &target, &["--dry-run"]);
    assert!(
        stdout(&dry).contains("update package.json (scripts.build)"),
        "{}",
        stdout(&dry)
    );
}

#[test]
fn marker_rule_targeting_json_is_rejected_with_guidance() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"data.json\"\n    region: block\n    content: \"x\"\n",
    );
    let target = json_target(root.path(), "{}");
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("struct:") && stderr(&output).to_lowercase().contains("json"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn jsonc_injection_preserves_comments() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"tsconfig.jsonc\"\n    struct:\n      path: \"compilerOptions.strict\"\n      value: true\n",
    );
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("tsconfig.jsonc"),
        "{\n  // project config\n  \"compilerOptions\": {\n    \"target\": \"es2022\"\n  }\n}",
    )
    .unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = fs::read_to_string(target.join("tsconfig.jsonc")).unwrap();
    assert!(text.contains("// project config"), "{text}");
    assert!(text.contains("\"strict\": true"), "{text}");
    assert!(text.contains("\"target\": \"es2022\""), "{text}");
}

#[test]
fn whole_file_generation_plus_edit_commits_one_final_image() {
    let root = tempfile::tempdir().unwrap();
    // The template generates config.json, then injects a value into it.
    let folder = root.path().join("tmpl");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template/config.json"),
        "{\n  \"generated\": true\n}\n",
    )
    .unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: sample\ninject:\n  - into: \"config.json\"\n    struct:\n      path: \"version\"\n      value: \"1.0.0\"\n",
    )
    .unwrap();
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    let output = apply(&folder, &target, &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(target.join("config.json")).unwrap()).unwrap();
    // Both the whole-file content and the injected value are present.
    assert_eq!(value["generated"], serde_json::json!(true));
    assert_eq!(value["version"], serde_json::json!("1.0.0"));
}

#[test]
fn an_error_in_a_later_edit_leaves_every_target_unchanged() {
    let root = tempfile::tempdir().unwrap();
    // First edit is valid; the second targets an out-of-range array index.
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"a.json\"\n    struct: { path: \"ok\", value: 1 }\n  - into: \"b.json\"\n    struct: { path: \"list[9]\", value: 2 }\n",
    );
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.json"), "{}").unwrap();
    fs::write(target.join("b.json"), "{\n  \"list\": [0]\n}").unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    // Neither target was written: a.json is still empty, b.json is unchanged.
    assert_eq!(fs::read_to_string(target.join("a.json")).unwrap(), "{}");
    assert_eq!(
        fs::read_to_string(target.join("b.json")).unwrap(),
        "{\n  \"list\": [0]\n}"
    );
}

#[test]
fn struct_rule_targeting_a_non_json_file_is_a_planning_error() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"notes.txt\"\n    struct: { path: \"a\", value: 1 }\n",
    );
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("notes.txt"), "hi\n").unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("not a .json"),
        "{}",
        stderr(&output)
    );
    // Nothing was written.
    assert_eq!(
        fs::read_to_string(target.join("notes.txt")).unwrap(),
        "hi\n"
    );
}

#[test]
fn overlapping_struct_paths_on_one_target_are_a_planning_error() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"c.json\"\n    struct: { path: \"a\", value: 1 }\n  - into: \"c.json\"\n    struct: { path: \"a.b\", value: 2 }\n",
    );
    let target = json_target(root.path(), "{}");
    fs::rename(target.join("package.json"), target.join("c.json")).unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("overlap"), "{}", stderr(&output));
}

#[test]
fn duplicate_region_key_on_one_target_is_a_planning_error() {
    let root = tempfile::tempdir().unwrap();
    let template = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"a.conf\"\n    region: dup\n    content: \"x\"\n  - into: \"a.conf\"\n    region: dup\n    content: \"y\"\n",
    );
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.conf"), "base\n").unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("duplicate region"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn an_injected_target_through_a_symlink_is_refused() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let template = make_template(root.path(), STRUCT_TEMPLATE);
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    // `package.json` is a symlink pointing outside the target.
    let outside = root.path().join("outside.json");
    fs::write(&outside, "{}").unwrap();
    symlink(&outside, target.join("package.json")).unwrap();
    let output = apply(&template, &target, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        stderr(&output).to_lowercase().contains("symlink"),
        "{}",
        stderr(&output)
    );
    // The file the symlink points at was not written through.
    assert_eq!(fs::read_to_string(&outside).unwrap(), "{}");
}

#[test]
fn region_body_from_a_source_support_file() {
    let root = tempfile::tempdir().unwrap();
    // The region body is a rendered support file rather than inline content.
    let folder = root.path().join("tmpl");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("snippet.txt"), "port = {{ port }}\n").unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: sample\ndata: { port: 8080 }\ninject:\n  - into: \"app.conf\"\n    region: net\n    source: snippet.txt\n",
    )
    .unwrap();
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("app.conf"), "existing = 1\n").unwrap();
    let output = apply(&folder, &target, &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = fs::read_to_string(target.join("app.conf")).unwrap();
    assert!(text.contains("# >>> toha:region net >>>"), "{text}");
    assert!(text.contains("port = 8080"), "{text}");
    assert!(text.starts_with("existing = 1\n"), "{text}");
}

#[test]
fn missing_target_without_create_is_an_error_and_create_makes_it() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("out");
    fs::create_dir_all(&target).unwrap();

    let no_create = make_template(root.path(), STRUCT_TEMPLATE);
    let output = apply(&no_create, &target, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!target.join("package.json").exists());

    let create = make_template(
        root.path(),
        "name: sample\ninject:\n  - into: \"package.json\"\n    create: true\n    struct: { path: \"scripts.build\", value: \"tsc\" }\n",
    );
    let output = apply(&create, &target, &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(target.join("package.json")).unwrap()).unwrap();
    assert_eq!(value["scripts"]["build"], serde_json::json!("tsc"));
}
