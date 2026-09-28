// ---
// relationships:
//   implements: command-line-interface
// ---
//! The bundled offline `toha-demo` template, exercised through the packaged CLI.
#[allow(dead_code)]
mod support;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

/// The demo's rendered output for the fixture answers.
const NOTE: &str = "Sample Title\nTopic: Sample Topic\n";

fn command(root: &TempDir, cwd: &Path) -> Command {
    let mut command = support::isolated_command(root.path());
    command.current_dir(cwd);
    command
}

fn run(root: &TempDir, cwd: &Path, args: &[&str], exit: i32) -> Output {
    let output = command(root, cwd).args(args).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "args={args:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn answers(dir: &Path) -> String {
    let path = dir.join("answers.json");
    fs::write(&path, r#"{"title":"Sample Title","topic":"Sample Topic"}"#).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn applies_offline_from_any_directory() {
    // Isolated HOME/XDG, empty cache, no registry, no network. The current
    // directory is an arbitrary scratch folder, not the repository.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = answers(&cwd);
    run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "toha-demo", "./out"],
        0,
    );
    assert_eq!(fs::read_to_string(cwd.join("out/note.txt")).unwrap(), NOTE);
}

#[test]
fn dry_run_previews_without_writing() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = answers(&cwd);
    let output = run(
        &root,
        &cwd,
        &[
            "apply",
            "--dry-run",
            "--answers",
            &answers,
            "toha-demo",
            "./out",
        ],
        0,
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("create note.txt"));
    assert!(!cwd.join("out").exists(), "dry run must not write files");
}

#[test]
fn other_unknown_name_is_still_not_found() {
    // Only the reserved token falls back to the bundled demo; every other
    // unknown name still fails exactly as before.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = answers(&cwd);
    let output = run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "not-a-template", "./out"],
        1,
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("template not found: not-a-template"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!cwd.join("out").exists());
}

#[test]
fn resumes_across_processes() {
    // Stage in one process, continue in a second, apply in a third — each a
    // separate `toha` invocation — offline, with byte-identical output.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join("batch.json"), "{}").unwrap();
    run(
        &root,
        &cwd,
        &["stage", "toha-demo", "./out", "--async", "batch.json"],
        4,
    );
    let answers = answers(&cwd);
    run(&root, &cwd, &["continue", "./out", &answers], 0);
    run(&root, &cwd, &["apply", "./out"], 0);
    assert_eq!(fs::read_to_string(cwd.join("out/note.txt")).unwrap(), NOTE);
}

#[test]
fn apply_naming_the_staged_demo_resumes_it() {
    // `apply toha-demo ./out` while the demo is staged at ./out names its own
    // staged run and resumes it, rather than refusing as a different template.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join("batch.json"), "{}").unwrap();
    run(
        &root,
        &cwd,
        &["stage", "toha-demo", "./out", "--async", "batch.json"],
        4,
    );
    let answers = answers(&cwd);
    run(&root, &cwd, &["continue", "./out", &answers], 0);
    run(&root, &cwd, &["apply", "toha-demo", "./out"], 0);
    assert_eq!(fs::read_to_string(cwd.join("out/note.txt")).unwrap(), NOTE);
}

/// A folder template that occupies the reserved name, so the registry — not the
/// bundled demo — resolves `toha-demo`. Its output is deliberately distinct.
fn install_shadow(root: &TempDir, cwd: &Path, short: &str, alias: Option<&str>) {
    let folder = root.path().join("shadow");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template.yml"),
        format!(
            "name: {short}\ninterview:\n  - {{ id: title, type: text, prompt: Title, required: true }}\n  - {{ id: topic, type: text, prompt: Topic, required: true }}\n"
        ),
    )
    .unwrap();
    fs::write(
        folder.join("template/marker.txt"),
        "installed {{ title }}\n",
    )
    .unwrap();
    let mut args = vec!["templates", "add", folder.to_str().unwrap()];
    if let Some(alias) = alias {
        args.push("--alias");
        args.push(alias);
    }
    run(root, cwd, &args, 0);
}

#[test]
fn installed_short_name_wins_over_bundled() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    install_shadow(&root, &cwd, "toha-demo", None);
    let answers = answers(&cwd);
    run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "toha-demo", "./out"],
        0,
    );
    // The installed template rendered; the bundled demo never ran.
    assert_eq!(
        fs::read_to_string(cwd.join("out/marker.txt")).unwrap(),
        "installed Sample Title\n"
    );
    assert!(!cwd.join("out/note.txt").exists());
}

#[test]
fn installed_alias_wins_over_bundled() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    install_shadow(&root, &cwd, "other", Some("toha-demo"));
    let answers = answers(&cwd);
    run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "toha-demo", "./out"],
        0,
    );
    assert_eq!(
        fs::read_to_string(cwd.join("out/marker.txt")).unwrap(),
        "installed Sample Title\n"
    );
    assert!(!cwd.join("out/note.txt").exists());
}

#[test]
fn stale_staged_commit_asks_to_stage_again() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join("batch.json"), "{}").unwrap();
    run(
        &root,
        &cwd,
        &["stage", "toha-demo", "./out", "--async", "batch.json"],
        4,
    );
    // Rewrite the staged commit to a build that no longer matches.
    let staged = support::staged_dir(root.path());
    let record = fs::read_dir(&staged)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("a staged record");
    let text = fs::read_to_string(&record).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["commit"] = serde_json::Value::String("0".repeat(64));
    fs::write(&record, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let output = run(&root, &cwd, &["apply", "./out"], 1);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("stage toha-demo again"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!cwd.join("out/note.txt").exists());
}

/// The bundled demo's text row: its formal name is `toha-demo`, which no
/// persisted registry entry can hold (a folder entry's formal name is its path).
/// The header line is excluded.
fn text_bundled_rows(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .skip(1)
        .filter(|line| line.split('\t').next() == Some("toha-demo"))
        .collect()
}

/// Every text row that names `toha-demo` as its formal or short name — the
/// bundled row or a registry entry that claims the name — to prove the name is
/// never duplicated.
fn text_rows_naming_demo(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .skip(1)
        .filter(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            cols.first() == Some(&"toha-demo") || cols.get(1) == Some(&"toha-demo")
        })
        .collect()
}

/// The `--json` array objects that describe the bundled demo.
fn json_demo_rows(stdout: &str) -> Vec<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(stdout).unwrap();
    value
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e.get("bundled") == Some(&serde_json::Value::Bool(true)))
        .cloned()
        .collect()
}

#[test]
fn templates_list_text_shows_one_bundled_row() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    // Unclaimed: exactly one read-only bundled row appears in the text listing.
    let output = run(&root, &cwd, &["templates", "list"], 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(text_bundled_rows(&stdout), vec!["toha-demo\tdemo\t\tfalse"]);

    // Claimed by an installed template: the bundled row is suppressed, leaving
    // exactly the registry's own row, so no duplicate misrepresents resolution.
    install_shadow(&root, &cwd, "toha-demo", None);
    let output = run(&root, &cwd, &["templates", "list"], 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(text_bundled_rows(&stdout).is_empty());
    assert_eq!(
        text_rows_naming_demo(&stdout).len(),
        1,
        "exactly one row names toha-demo:\n{stdout}"
    );
}

#[test]
fn templates_list_json_shows_one_truthful_bundled_row() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    // Unclaimed: exactly one bundled descriptor appears in --json, marked as the
    // presentation-only row with no persisted registry layer, source, or path.
    let output = run(&root, &cwd, &["templates", "list", "--json"], 0);
    let rows = json_demo_rows(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["formal_name"], "toha-demo");
    assert_eq!(row["name"], "demo");
    assert_eq!(row["aliases"], serde_json::json!([]));
    assert_eq!(row["trusted"], false);
    assert_eq!(row["bundled"], true);
    assert_eq!(row["commit"].as_str().unwrap().len(), 64);
    // No fabricated registry metadata.
    for fabricated in ["layer", "source", "ref", "path"] {
        assert!(
            row.get(fabricated).is_none(),
            "bundled row must not fabricate {fabricated}: {row}"
        );
    }

    // Claimed by an installed template: suppressed from --json too.
    install_shadow(&root, &cwd, "toha-demo", None);
    let output = run(&root, &cwd, &["templates", "list", "--json"], 0);
    assert!(json_demo_rows(&String::from_utf8_lossy(&output.stdout)).is_empty());
}

#[test]
fn templates_list_text_and_json_agree_on_the_bundled_row() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    // Both formats expose the bundled demo, and they agree on its shared fields.
    let text = run(&root, &cwd, &["templates", "list"], 0);
    let text_stdout = String::from_utf8_lossy(&text.stdout);
    let text_rows = text_bundled_rows(&text_stdout);
    let json = run(&root, &cwd, &["templates", "list", "--json"], 0);
    let json_rows = json_demo_rows(&String::from_utf8_lossy(&json.stdout));
    assert_eq!(text_rows.len(), 1);
    assert_eq!(json_rows.len(), 1);
    let json_row = &json_rows[0];
    let expected = format!(
        "{}\t{}\t\t{}",
        json_row["formal_name"].as_str().unwrap(),
        json_row["name"].as_str().unwrap(),
        json_row["trusted"].as_bool().unwrap()
    );
    assert_eq!(text_rows[0], expected);

    // Suppression is consistent across both formats.
    install_shadow(&root, &cwd, "toha-demo", None);
    let text = run(&root, &cwd, &["templates", "list"], 0);
    let json = run(&root, &cwd, &["templates", "list", "--json"], 0);
    assert!(text_bundled_rows(&String::from_utf8_lossy(&text.stdout)).is_empty());
    assert!(json_demo_rows(&String::from_utf8_lossy(&json.stdout)).is_empty());
}
