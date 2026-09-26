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
        for part in &expect.error_contains {
            assert!(stderr.contains(part), "{}: {stderr}", fixture.display());
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
