// ---
// relationships:
//   implements: command-line-interface
// ---
//! `--version`, `--help`, and the `skills` commands resolve no directory, so
//! they work when neither `HOME` nor `USERPROFILE` is set. Every other command
//! still resolves the folders and fails with the existing clear error.
use std::process::{Command, Output};

/// A `toha` invocation with `HOME` and `USERPROFILE` removed from the
/// environment, so the home directory is unavailable.
fn homeless(args: &[&str]) -> Output {
    Command::new(assert_cmd::cargo::cargo_bin("toha"))
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .args(args)
        .output()
        .unwrap()
}

/// The subcommand names listed under `Commands:` in help `text`, without the
/// built-in `help` command.
fn subcommands(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in text.lines() {
        if !line.starts_with(' ') {
            in_commands = line == "Commands:";
            continue;
        }
        if in_commands && line.starts_with("  ") && !line.trim_start().starts_with('-') {
            let name = line.split_whitespace().next().unwrap();
            if name != "help" {
                names.push(name.to_string());
            }
        }
    }
    names
}

#[test]
fn version_flags_work_without_a_home_directory() {
    for flag in ["--version", "-V"] {
        let output = homeless(&[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.starts_with("toha "), "{flag}: {stdout:?}");
    }
}

#[test]
fn help_flags_work_without_a_home_directory() {
    // Walk the whole command tree from the homeless top-level help, so every
    // command's -h and --help is exercised with no home directory.
    let mut pending = vec![Vec::<String>::new()];
    let mut seen = 0;
    while let Some(path) = pending.pop() {
        let args: Vec<&str> = path.iter().map(String::as_str).collect();
        let mut help_text = String::new();
        for flag in ["-h", "--help"] {
            let mut with_flag = args.clone();
            with_flag.push(flag);
            let output = homeless(&with_flag);
            assert_eq!(
                output.status.code(),
                Some(0),
                "toha {} {flag}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            if flag == "-h" {
                help_text = String::from_utf8(output.stdout).unwrap();
            }
        }
        for command in subcommands(&help_text) {
            let mut next = path.clone();
            next.push(command);
            pending.push(next);
        }
        seen += 1;
    }
    // toha; 8 commands; templates with 7 subcommands; skills with 2; snapshots
    // with 2.
    assert_eq!(seen, 1 + 8 + 7 + 2 + 2);
}

#[test]
fn skills_list_works_without_a_home_directory() {
    let output = homeless(&["skills", "list"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<_> = list
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["toha", "toha-templates"]);
}

#[test]
fn skills_view_works_without_a_home_directory() {
    let output = homeless(&["skills", "view", "toha"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        std::fs::read("skills/toha/SKILL.md").unwrap()
    );
}

#[test]
fn a_command_that_reads_folders_still_fails_without_a_home_directory() {
    let output = homeless(&["abort", "/tmp/toha-homeless"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "home directory unavailable: set HOME (or USERPROFILE on Windows) to an absolute path\n"
    );
}
