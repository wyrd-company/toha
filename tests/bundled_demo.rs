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

/// Writes the bare answer map `bare`, wraps it in the identity envelope naming
/// the formal template `formal`, and returns the envelope path. The scripted
/// route (`apply TEMPLATE PATH --answers FILE`, `continue PATH FILE`) requires
/// this `{"template", "answers"}` envelope, not a bare map.
fn envelope(dir: &Path, formal: &str, bare: &str) -> String {
    let bare_path = dir.join("bare.json");
    fs::write(&bare_path, bare).unwrap();
    support::envelope_file(dir, formal, &bare_path)
        .to_string_lossy()
        .into_owned()
}

/// The demo's fixture answers wrapped in the envelope naming `formal`.
fn demo_answers(dir: &Path, formal: &str) -> String {
    envelope(
        dir,
        formal,
        r#"{"title":"Sample Title","topic":"Sample Topic"}"#,
    )
}

#[test]
fn applies_offline_from_any_directory() {
    // Isolated HOME/XDG, empty cache, no registry, no network. The current
    // directory is an arbitrary scratch folder, not the repository.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = demo_answers(&cwd, "toha-demo");
    run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "toha-demo", "./out"],
        0,
    );
    assert_eq!(fs::read_to_string(cwd.join("out/note.txt")).unwrap(), NOTE);
}

#[test]
fn bundled_identity_selects_its_configured_defaults() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let config = root.path().join("config/toha/config.yml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        config,
        "presets: { sample_title: Configured Title }\ntemplate-defaults:\n  toha-demo:\n    title: { preset: sample_title }\n    topic: Configured Topic\n",
    )
    .unwrap();
    let answers = envelope(&cwd, "toha-demo", "{}");
    run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "toha-demo", "./out"],
        0,
    );
    assert_eq!(
        fs::read_to_string(cwd.join("out/note.txt")).unwrap(),
        "Configured Title\nTopic: Configured Topic\n"
    );
}

#[test]
fn dry_run_previews_without_writing() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = demo_answers(&cwd, "toha-demo");
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
    // A dry run is a single `planned` document; its reconstructed plan text
    // carries the create line the former plain-text output printed.
    let document = support::first_document(&output.stdout);
    assert!(
        support::plan_text_from_document(&document).contains("create note.txt"),
        "{document}"
    );
    assert!(!cwd.join("out").exists(), "dry run must not write files");
}

#[test]
fn other_unknown_name_is_still_not_found() {
    // Only the reserved token falls back to the bundled demo; every other
    // unknown name still fails exactly as before.
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let answers = demo_answers(&cwd, "not-a-template");
    let output = run(
        &root,
        &cwd,
        &["apply", "--answers", &answers, "not-a-template", "./out"],
        1,
    );
    // Resolution fails before the envelope identity is read: the fault is an
    // `error` document whose decoded message names the unknown template.
    let document = support::first_document(&output.stdout);
    assert!(
        support::diagnostic_text(&document).contains("template not found: not-a-template"),
        "{document}"
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
    let answers = demo_answers(&cwd, "toha-demo");
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
    let answers = demo_answers(&cwd, "toha-demo");
    run(&root, &cwd, &["continue", "./out", &answers], 0);
    run(&root, &cwd, &["apply", "toha-demo", "./out"], 0);
    assert_eq!(fs::read_to_string(cwd.join("out/note.txt")).unwrap(), NOTE);
}

/// A folder template that occupies the reserved name, so the registry — not the
/// bundled demo — resolves `toha-demo`. Its output is deliberately distinct.
fn install_shadow(
    root: &TempDir,
    cwd: &Path,
    short: &str,
    alias: Option<&str>,
) -> std::path::PathBuf {
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
    folder
}

#[test]
fn installed_short_name_wins_over_bundled() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("scratch");
    fs::create_dir_all(&cwd).unwrap();
    let folder = install_shadow(&root, &cwd, "toha-demo", None);
    // `toha-demo` resolves to the installed folder, so its formal identity is
    // that folder's canonical path, not the reserved short name.
    let answers = demo_answers(&cwd, &support::formal_name(&folder));
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
    let folder = install_shadow(&root, &cwd, "other", Some("toha-demo"));
    // The alias resolves to the installed folder; the envelope names its formal
    // path identity.
    let answers = demo_answers(&cwd, &support::formal_name(&folder));
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

fn git_fixture(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn bundled_snapshots_preserve_content_identity_and_update_offline() {
    for route in ["scripted", "staged", "baseline"] {
        let root = TempDir::new().unwrap();
        let cwd = root.path().join("project");
        fs::create_dir_all(&cwd).unwrap();
        git_fixture(&cwd, &["init", "-q"]);
        git_fixture(&cwd, &["config", "user.name", "Fixture Author"]);
        git_fixture(&cwd, &["config", "user.email", "fixture@example.test"]);
        git_fixture(&cwd, &["config", "core.autocrlf", "false"]);
        git_fixture(&cwd, &["commit", "--allow-empty", "-qm", "Initial project"]);
        let answers = demo_answers(root.path(), "toha-demo");
        if route == "staged" {
            let batch = root.path().join("batch.json");
            run(
                &root,
                &cwd,
                &[
                    "stage",
                    "toha-demo",
                    ".",
                    "--async",
                    batch.to_str().unwrap(),
                ],
                4,
            );
            run(&root, &cwd, &["continue", ".", &answers], 0);
            let output = run(&root, &cwd, &["apply", "."], 0);
            assert!(String::from_utf8_lossy(&output.stdout).contains("saved snapshot"));
        } else {
            let mut args = vec!["apply", "toha-demo", ".", "--answers", &answers];
            if route == "baseline" {
                args.push("--baseline");
            }
            let output = run(&root, &cwd, &args, 0);
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(value["snapshot"]["id"].is_string(), "{value}");
        }
        let output = run(&root, &cwd, &["snapshots", "list", ".", "--json"], 0);
        let listing: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let row = &listing["snapshots"][0];
        let id = row["id"].as_str().unwrap();
        assert!(row["commit"].is_null());
        let digest = row["content_digest"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        let record = git_fixture(
            &cwd,
            &["show", &format!("refs/toha/snapshots/{id}:snapshot.json")],
        );
        let record: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert!(record["commit"].is_null());
        assert_eq!(record["content_digest"], digest);
        let schema: serde_json::Value = serde_norway::from_str(
            &fs::read_to_string("docs/specifications/snapshot.schema.yml").unwrap(),
        )
        .unwrap();
        for (definition, value) in [("record", &record), ("list", &listing)] {
            let mut schema = schema.clone();
            schema["$ref"] = serde_json::json!(format!("#/$defs/{definition}"));
            let validator = jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .build(&schema)
                .unwrap();
            assert!(validator.is_valid(value), "{definition}: {value}");
        }

        fs::write(
            cwd.join("note.txt"),
            format!("{NOTE}\nKeep this operator note.\n"),
        )
        .unwrap();
        git_fixture(&cwd, &["add", "note.txt"]);
        git_fixture(&cwd, &["commit", "-qm", "Keep operator note"]);
        let override_answers = envelope(
            root.path(),
            "toha-demo",
            r#"{"title":"Updated Title","topic":"Sample Topic"}"#,
        );
        if route == "staged" {
            let batch = root.path().join("update-batch.json");
            run(
                &root,
                &cwd,
                &[
                    "stage",
                    "toha-demo",
                    ".",
                    "--from",
                    id,
                    "--reanswer",
                    "--async",
                    batch.to_str().unwrap(),
                ],
                4,
            );
            run(&root, &cwd, &["continue", ".", &override_answers], 0);
            let before = fs::read(cwd.join("note.txt")).unwrap();
            run(&root, &cwd, &["apply", ".", "--dry-run"], 0);
            assert_eq!(fs::read(cwd.join("note.txt")).unwrap(), before);
            let output = run(&root, &cwd, &["apply", "."], 0);
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("Saved snapshot "), "{text}");
            assert!(text.contains("Merged note.txt"), "{text}");
            assert_eq!(
                fs::read_to_string(cwd.join("note.txt")).unwrap(),
                "Updated Title\nTopic: Sample Topic\n\nKeep this operator note.\n"
            );
            continue;
        }
        let before = fs::read(cwd.join("note.txt")).unwrap();
        let refs = git_fixture(&cwd, &["for-each-ref", "refs/toha/snapshots"]);
        // No TEMPLATE argument: resolve the stored content revision offline.
        let output = run(
            &root,
            &cwd,
            &[
                "apply",
                ".",
                "--from",
                id,
                "--answers",
                &override_answers,
                "--dry-run",
            ],
            0,
        );
        let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(preview["merge"]["changes"][0]["action"], "merged");
        assert_eq!(fs::read(cwd.join("note.txt")).unwrap(), before);
        assert_eq!(
            git_fixture(&cwd, &["for-each-ref", "refs/toha/snapshots"]),
            refs
        );
        let output = run(
            &root,
            &cwd,
            &["apply", ".", "--from", id, "--answers", &override_answers],
            0,
        );
        let applied: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(applied["snapshot"]["id"].is_string());
        assert_eq!(
            fs::read_to_string(cwd.join("note.txt")).unwrap(),
            "Updated Title\nTopic: Sample Topic\n\nKeep this operator note.\n"
        );
    }
}

#[test]
#[cfg(unix)]
fn person_bundled_apply_reports_saved_snapshot() {
    use expectrl::{Expect, Session};
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    git_fixture(&cwd, &["init", "-q"]);
    git_fixture(&cwd, &["config", "user.name", "Fixture Author"]);
    git_fixture(&cwd, &["config", "user.email", "fixture@example.test"]);
    git_fixture(&cwd, &["config", "core.autocrlf", "false"]);
    git_fixture(&cwd, &["commit", "--allow-empty", "-qm", "Initial project"]);
    let mut command = command(&root, &cwd);
    command.args(["apply", "toha-demo", "."]);
    let mut session = Session::spawn(command).unwrap();
    session.expect("Note title").unwrap();
    session.send_line("Sample Title").unwrap();
    session.expect("Topic").unwrap();
    session.send_line("Sample Topic").unwrap();
    session.expect("saved snapshot").unwrap();
    session.expect(expectrl::Eof).unwrap();
    let output = run(&root, &cwd, &["snapshots", "list", ".", "--json"], 0);
    let listing: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(listing["snapshots"].as_array().unwrap().len(), 1);
    assert!(listing["snapshots"][0]["commit"].is_null());
    assert_eq!(
        listing["snapshots"][0]["content_digest"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
#[cfg(unix)]
fn person_update_completion_is_prose_and_preserves_operator_edits() {
    use expectrl::{Expect, Session};
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    git_fixture(&cwd, &["init", "-q"]);
    git_fixture(&cwd, &["config", "user.name", "Fixture Author"]);
    git_fixture(&cwd, &["config", "user.email", "fixture@example.test"]);
    git_fixture(&cwd, &["config", "core.autocrlf", "false"]);
    git_fixture(&cwd, &["commit", "--allow-empty", "-qm", "Initial project"]);
    let answers = demo_answers(root.path(), "toha-demo");
    let output = run(
        &root,
        &cwd,
        &["apply", "toha-demo", ".", "--answers", &answers],
        0,
    );
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = doc["snapshot"]["id"].as_str().unwrap();
    fs::write(
        cwd.join("note.txt"),
        format!("{NOTE}\nKeep this operator note.\n"),
    )
    .unwrap();
    git_fixture(&cwd, &["add", "note.txt"]);
    git_fixture(&cwd, &["commit", "-qm", "Keep operator note"]);
    for preview in [true, false] {
        let mut command = command(&root, &cwd);
        command.args(["apply", "toha-demo", ".", "--from", id, "--reanswer"]);
        if preview {
            command.arg("--dry-run");
        }
        let mut session = Session::spawn(command).unwrap();
        session.expect("Note title").unwrap();
        session.send_line("Updated Title").unwrap();
        session.expect("Topic").unwrap();
        session.send_line("Sample Topic").unwrap();
        session.expect("Template: toha-demo").unwrap();
        session
            .expect(format!("Target: {}", cwd.canonicalize().unwrap().display()))
            .unwrap();
        session.expect("Merged note.txt").unwrap();
        if preview {
            session.expect("Preview only; no changes written.").unwrap();
        } else {
            session.expect("Saved snapshot ").unwrap();
        }
        session.expect(expectrl::Eof).unwrap();
        assert_eq!(
            fs::read_to_string(cwd.join("note.txt")).unwrap(),
            if preview {
                format!("{NOTE}\nKeep this operator note.\n")
            } else {
                "Updated Title\nTopic: Sample Topic\n\nKeep this operator note.\n".into()
            }
        );
    }
}

#[test]
#[cfg(unix)]
fn staged_update_reports_messages_and_allowed_hook_failure() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    git_fixture(&cwd, &["init", "-q"]);
    git_fixture(&cwd, &["config", "user.name", "Fixture Author"]);
    git_fixture(&cwd, &["config", "user.email", "fixture@example.test"]);
    git_fixture(&cwd, &["config", "core.autocrlf", "false"]);
    git_fixture(&cwd, &["commit", "--allow-empty", "-qm", "Initial project"]);
    let template = root.path().join("sample-template");
    fs::create_dir_all(template.join("template")).unwrap();
    fs::write(template.join("template/note.txt"), "Sample note\n").unwrap();
    fs::write(template.join("template.yml"), "name: sample-template\ninterview:\n  - message: Interview message\nmessages:\n  before-apply: Before message\n  after-apply: 'After message {{ setup.exit_code }}'\nhooks:\n  - id: setup\n    run: [sh, -c, 'exit 7']\n    allow-failure: true\n").unwrap();
    run(
        &root,
        &cwd,
        &[
            "stage",
            template.to_str().unwrap(),
            ".",
            "--baseline",
            "--trust",
            "--async",
        ],
        0,
    );
    let output = run(&root, &cwd, &["apply", ".", "--trust"], 0);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("Template: "), "{text}");
    for expected in [
        "Added note.txt",
        "Interview message",
        "Before message",
        "After message",
        "Hook setup failed (exit 7); failure was allowed.",
        "Saved snapshot ",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains("\"protocol\""), "{text}");
    for message in ["Interview message", "Before message", "After message"] {
        assert_eq!(text.matches(message).count(), 1, "{text}");
    }
    git_fixture(&cwd, &["add", "."]);
    git_fixture(&cwd, &["commit", "-qm", "Apply staged baseline"]);
    let manifest = template.join("template.yml");
    let source = fs::read_to_string(&manifest)
        .unwrap()
        .replace("exit 7", "printf hook-output; exit 7");
    fs::write(&manifest, source).unwrap();
    let formal = template.canonicalize().unwrap();
    let answers = envelope(root.path(), formal.to_str().unwrap(), "{}");
    let output = run(
        &root,
        &cwd,
        &[
            "apply",
            template.to_str().unwrap(),
            "scripted",
            "--baseline",
            "--trust",
            "--answers",
            &answers,
        ],
        0,
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["status"], "applied");
    assert!(String::from_utf8_lossy(&output.stderr).contains("hook-output"));
}
