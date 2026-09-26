// ---
// relationships:
//   implements: architecture
// ---
mod support;

use std::process::Command;

#[test]
fn every_fixture_through_cli() {
    for fixture in support::fixtures() {
        let expect = support::expectation(&fixture);
        if !expect.cli
            || (!expect.hooks.is_empty()
                && fixture.file_name().unwrap() != "hooks-untrusted"
                && fixture.file_name().unwrap() != "script-cli")
        {
            continue;
        }
        let target = tempfile::tempdir().unwrap();
        support::copy_tree(&fixture.join("existing"), target.path());
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("toha"));
        command
            .arg("apply")
            .arg(fixture.join("template").canonicalize().unwrap())
            .arg(target.path())
            .arg("--answers")
            .arg(fixture.join("answers.json"))
            // The hidden TOHA_NOW seed hook fixes the clock for CLI fixtures.
            .env("TOHA_NOW", expect.now.clone());
        if expect.options.force {
            command.arg("--force");
        }
        if expect.options.trust {
            command.arg("--trust");
        }
        if expect.options.dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(support::expectation(&fixture).exit.into()),
            "{}: {}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error_output = if expect.exit == 4 {
            String::from_utf8_lossy(&output.stdout)
        } else {
            stderr.clone()
        };
        for part in &expect.error_contains {
            assert!(
                error_output.contains(part),
                "{}: {error_output}",
                fixture.display()
            );
        }
        if fixture.join("expected").exists() {
            support::assert_tree(target.path(), &fixture.join("expected"));
            let stdout = String::from_utf8_lossy(&output.stdout);
            for part in &expect.stdout_contains {
                assert!(stdout.contains(part), "{}: {stdout}", fixture.display());
            }
            let messages: Vec<_> = expect.messages.iter().map(|s| s.as_str()).collect();
            let lines: Vec<_> = stdout.lines().collect();
            assert_eq!(
                &lines[..messages.len()],
                messages.as_slice(),
                "{}: {stdout}",
                fixture.display()
            );
        } else {
            assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
        }
    }
}

#[test]
fn commands_without_documents_require_a_terminal() {
    let target = tempfile::tempdir().unwrap();
    let template = std::path::Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    for args in [
        vec![
            "stage".to_string(),
            template.display().to_string(),
            target.path().display().to_string(),
        ],
        vec!["continue".to_string(), target.path().display().to_string()],
        vec![
            "apply".to_string(),
            template.display().to_string(),
            target.path().display().to_string(),
        ],
    ] {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("no terminal: use --async, an answers document, or --answers")
        );
    }
}

#[cfg(unix)]
#[test]
fn terminal_text_and_confirm_write_rendered_file() {
    use expectrl::{Expect, Session};
    let folder = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("template")).unwrap();
    std::fs::write(folder.path().join("template.yml"), "name: sample\nsource: template\ninterview:\n  - id: label\n    type: text\n    prompt: Label?\n    required: true\n  - id: enabled\n    type: confirm\n    prompt: Enabled?\n").unwrap();
    std::fs::write(
        folder.path().join("template/result.txt"),
        "{{ label }} {{ enabled }}",
    )
    .unwrap();
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("toha"));
    command.arg("apply").arg(folder.path()).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Label?").unwrap();
    session.send_line("sample").unwrap();
    session.expect("Enabled?").unwrap();
    session.send_line("y").unwrap();
    session.expect("result.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(target.path().join("result.txt")).unwrap(),
        "sample True"
    );
}

#[cfg(unix)]
#[test]
fn cancel_during_continue_preserves_staged_record() {
    use expectrl::{Expect, Session};
    let fixture = std::path::Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let target = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let stage = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
        .arg("stage")
        .arg(&fixture)
        .arg(target.path())
        .arg("--async")
        .env("XDG_STATE_HOME", state.path())
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4));
    let record = std::fs::read_dir(state.path().join("toha/staged"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let before = std::fs::read(&record).unwrap();
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("toha"));
    command
        .arg("continue")
        .arg(target.path())
        .env("XDG_STATE_HOME", state.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name?").unwrap();
    session.send("\u{1b}").unwrap();
    session.expect("Operation was canceled").unwrap();
    assert_eq!(std::fs::read(record).unwrap(), before);
}

#[test]
fn default_render_failure_exits_one_without_writing() {
    let fixture = std::path::Path::new("tests/fixtures/err-default-render");
    let target = tempfile::tempdir().unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
        .arg("apply")
        .arg(fixture.join("template").canonicalize().unwrap())
        .arg(target.path())
        .arg("--answers")
        .arg(fixture.join("answers.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("second.default"), "{stderr}");
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn trusted_script_runs_in_target() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("template.yml"),
        "name: script-cli\nsource: .\nhooks:\n  - script: write.sh\n",
    )
    .unwrap();
    let script = folder.path().join("write.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf marker > marker.txt\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let answers = folder.path().join("answers.json");
    std::fs::write(&answers, "{}").unwrap();
    let target = tempfile::tempdir().unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
        .args([
            "apply",
            folder.path().to_str().unwrap(),
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
            "--trust",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("marker.txt")).unwrap(),
        "marker"
    );
}
