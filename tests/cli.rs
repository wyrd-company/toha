// ---
// relationships:
//   implements: architecture
// ---
mod support;

use std::process::Command;

#[test]
fn every_fixture_through_cli() {
    for fixture in support::fixtures() {
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
            .arg("apply")
            .arg(fixture.join("template").canonicalize().unwrap())
            .arg(target.path())
            .arg("--answers")
            .arg(fixture.join("answers.json"))
            // The hidden TOHA_NOW seed hook fixes the clock for CLI fixtures.
            .env("TOHA_NOW", support::expectation(&fixture).now.clone())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(support::expectation(&fixture).exit.into()),
            "{}: {}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let expect = support::expectation(&fixture);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for part in &expect.error_contains {
            assert!(stderr.contains(part), "{}: {stderr}", fixture.display());
        }
        if expect.exit == 0 {
            support::assert_tree(target.path(), &fixture.join("expected"));
            let stdout = String::from_utf8_lossy(&output.stdout);
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
