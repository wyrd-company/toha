// ---
// relationships:
//   implements: architecture
// ---
mod support;

use std::process::Command;

#[test]
fn successful_fixtures_through_cli() {
    for fixture in support::fixtures() {
        if !fixture.join("expected").exists() {
            continue;
        }
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(assert_cmd::cargo::cargo_bin!("toha"))
            .arg("apply")
            .arg(fixture.join("template").canonicalize().unwrap())
            .arg(target.path())
            .arg("--answers")
            .arg(fixture.join("answers.json"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(support::expectation(&fixture).exit.into()),
            "{}: {}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        support::assert_tree(target.path(), &fixture.join("expected"));
    }
}
