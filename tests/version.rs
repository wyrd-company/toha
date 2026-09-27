// ---
// relationships:
//   implements: architecture
// ---
#[allow(dead_code)]
mod support;

use regex::Regex;

// The SemVer 2.0.0 grammar, from semver.org.
const SEMVER: &str = r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$";

fn reported_version(flag: &str) -> String {
    let isolation = tempfile::tempdir().unwrap();
    let output = support::isolated_command(isolation.path())
        .arg(flag)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{flag}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout
        .strip_suffix('\n')
        .unwrap_or_else(|| panic!("{flag}: one line expected: {stdout:?}"));
    let version = line
        .strip_prefix("toha ")
        .unwrap_or_else(|| panic!("{flag}: `toha <version>` expected: {line:?}"));
    assert!(
        Regex::new(SEMVER).unwrap().is_match(version),
        "{flag}: SemVer expected: {version:?}"
    );
    version.to_string()
}

#[test]
fn version_flag_prints_build_version() {
    let version = reported_version("--version");
    assert_eq!(
        Some(version.as_str()),
        option_env!("TOHA_VERSION"),
        "--version must print the build-time version"
    );
}

#[test]
fn short_version_flag_matches_long_flag() {
    assert_eq!(reported_version("-V"), reported_version("--version"));
}
