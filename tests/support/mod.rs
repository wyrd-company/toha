// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Expect {
    pub exit: u8,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub hooks: Vec<HookCall>,
    pub fail_hook: Option<usize>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
    #[serde(default = "yes")]
    pub cli: bool,
    #[serde(default)]
    pub stdout_contains: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub error_contains: Vec<String>,
    #[serde(default)]
    pub messages: Vec<String>,
    #[serde(default = "fixed_now")]
    pub now: String,
}

fn yes() -> bool {
    true
}
#[derive(Debug, Default, Deserialize)]
pub struct Options {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub trust: bool,
    #[serde(default)]
    pub dry_run: bool,
}
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct HookCall {
    pub argv: Vec<String>,
    pub cwd: String,
}
pub fn copy_tree(source: &Path, target: &Path) {
    if !source.exists() {
        return;
    }
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let dest = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&dest).unwrap();
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
fn fixed_now() -> String {
    "2026-01-02T03:04:05+00:00[UTC]".into()
}

pub fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|entry| entry.unwrap().path().canonicalize().unwrap())
        .filter(|path| path.is_dir())
        // `update-*` fixtures drive the data-driven update harness
        // (tests/update_fixtures.rs), not the standard single-apply runner.
        .filter(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("update-"))
        })
        .collect();
    paths.sort();
    paths
}

pub fn isolated_command(root: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("toha"));
    isolate(&mut command, root);
    command
}

pub fn isolate(command: &mut Command, root: &Path) {
    for (key, path) in [
        ("HOME", root.join("home")),
        ("USERPROFILE", root.join("home")),
        ("APPDATA", root.join("data")),
        ("LOCALAPPDATA", root.to_path_buf()),
        ("PROGRAMDATA", root.join("system")),
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_CACHE_HOME", root.join("cache")),
        ("XDG_STATE_HOME", root.to_path_buf()),
        ("TOHA_USER_CONFIG", root.join("config/toha/config.yml")),
    ] {
        command.env(key, path);
    }
    command.env("TOHA_CONFIG", root.join("config/local.yml"));
}

/// `value` quoted as toha quotes a word in a suggested command: single quotes
/// for a POSIX shell, double quotes on Windows.
pub fn shell_quoted(value: &str) -> String {
    if cfg!(windows) {
        format!("\"{value}\"")
    } else {
        format!("'{value}'")
    }
}

/// The words of a suggested `toha` command line after the shell of this
/// platform splits it: single quotes and `'\''` for a POSIX shell, double
/// quotes and `""` on Windows.
pub fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' if !cfg!(windows) => {
                started = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            '\\' if !cfg!(windows) => {
                started = true;
                current.extend(chars.next());
            }
            '"' if cfg!(windows) => {
                started = true;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        if chars.peek() == Some(&'"') {
                            chars.next();
                            current.push('"');
                        } else {
                            break;
                        }
                    } else {
                        current.push(c);
                    }
                }
            }
            c => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// The suggested command in the first stderr line that contains `marker`,
/// split into the arguments after `toha`.
pub fn suggested(stderr: &str, marker: &str) -> Vec<String> {
    let line = stderr
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("no line with `{marker}`:\n{stderr}"));
    let command = &line[line.find("toha ").expect("a toha command")..];
    let mut words = shell_words(command);
    assert_eq!(words.remove(0), "toha");
    words
}

pub fn staged_dir(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("home/Library/Application Support/toha/staged")
    } else {
        root.join("toha/staged")
    }
}

pub fn user_data_dir(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("home/Library/Application Support/toha")
    } else {
        root.join("data/toha")
    }
}

pub fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("file:///{path}")
    } else {
        format!("file://{path}")
    }
}

pub fn folder_address(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    if let Some(drive_path) = value.strip_prefix(r"\\?\") {
        assert!(
            drive_path
                .as_bytes()
                .get(0..3)
                .is_some_and(|prefix| prefix[0].is_ascii_alphabetic()
                    && prefix[1] == b':'
                    && prefix[2] == b'\\'),
            "expected a drive-letter folder path: {value}"
        );
        return drive_path.to_owned();
    }
    value.into_owned()
}

#[test]
fn every_platform_directory_input_is_isolated() {
    let mut command = Command::new("toha");
    let root = Path::new("isolated");
    isolate(&mut command, root);
    let actual: std::collections::HashMap<_, _> = command.get_envs().collect();
    for key in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "TOHA_USER_CONFIG",
        "TOHA_CONFIG",
    ] {
        let value = actual
            .get(std::ffi::OsStr::new(key))
            .and_then(|value| *value)
            .unwrap_or_else(|| panic!("{key} is not isolated"));
        assert!(Path::new(value).starts_with(root), "{key}: {value:?}");
    }
}

pub fn expectation(fixture: &Path) -> Expect {
    serde_norway::from_str(&fs::read_to_string(fixture.join("expect.yml")).unwrap()).unwrap()
}

pub fn assert_tree(actual: &Path, expected: &Path, fixture: &Path) {
    fn files(root: &Path) -> Vec<PathBuf> {
        fn walk(root: &Path, dir: &Path, result: &mut Vec<PathBuf>) {
            let mut entries: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap())
                .collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &entry.path(), result);
                } else {
                    result.push(entry.path().strip_prefix(root).unwrap().to_owned());
                }
            }
        }
        let mut result = Vec::new();
        walk(root, root, &mut result);
        result
    }
    let actual_files = files(actual);
    let expected_files = files(expected);
    let missing: Vec<_> = expected_files
        .iter()
        .filter(|path| !actual_files.contains(path))
        .collect();
    let extra: Vec<_> = actual_files
        .iter()
        .filter(|path| !expected_files.contains(path))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{}: missing: {missing:?}; extra: {extra:?}",
        fixture.display()
    );
    for path in expected_files {
        let actual_bytes = fs::read(actual.join(&path)).unwrap();
        let expected_bytes = fs::read(expected.join(&path)).unwrap();
        assert_eq!(
            actual_bytes,
            expected_bytes,
            "{}: different file: {}",
            fixture.display(),
            path.display()
        );
    }
}

/// Every environment variable toha reads, as its documentation names it.
pub const ENVIRONMENT: &[&str] = &[
    "TOHA_CONFIG",
    "TOHA_USER_CONFIG",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "VISUAL",
    "EDITOR",
    "PATH",
    "TOHA_NOW",
];

/// Builds `RawAnswers` from a bare JSON answers map, for in-memory engine and
/// staged-replay tests that drive `Pending::answer` / `answer_headless`
/// directly. External-document tests use the `{"template", "answers"}` envelope
/// through `protocol::answer_document_once` / `answer_document_headless`; this
/// helper is the in-memory path that the removed public `parse_answers` served.
#[allow(dead_code)]
pub fn raw_answers(json: &str) -> Result<toha::RawAnswers, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    value
        .as_object()
        .ok_or_else(|| "answers document must be a JSON object".to_string())?
        .iter()
        .map(|(key, value)| toha::Id::parse(key).map(|id| (id, toha::RawAnswer(value.clone()))))
        .collect()
}

/// Projects a completed interview to a comparable value `{answers, messages,
/// disposition?}`, for engine and staged-replay equivalence tests. The product
/// no longer emits a `complete` document (a scripted completion is `applied` or
/// `planned`, and an agent completion is instructions only), so this projection
/// replaces the removed `protocol::complete_document` as a test comparison
/// medium only. It carries no context, which is identical on both sides of a
/// replay-equivalence comparison.
#[allow(dead_code)]
pub fn completed_projection(completed: &toha::Completed) -> serde_json::Value {
    let answers: serde_json::Map<String, serde_json::Value> = completed
        .answers
        .iter()
        .map(|(id, answer)| (id.to_string(), answer.to_json()))
        .collect();
    let mut document = serde_json::json!({
        "answers": answers,
        "messages": completed.last_messages,
    });
    if completed.disposition() == toha::Disposition::DryRun {
        document["disposition"] = serde_json::json!("dry-run");
    }
    document
}

/// The formal template identity the scripted route establishes for a local
/// folder: its canonical absolute path, the same value the route publishes as
/// `context.template`. Envelope answers documents must copy it exactly.
#[allow(dead_code)]
pub fn formal_name(template_folder: &Path) -> String {
    template_folder
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Wraps a bare answers map in the `{"template", "answers"}` envelope naming
/// `formal`, writes it to `dir`, and returns the path, for `apply --answers`
/// and `continue PATH FILE`.
#[allow(dead_code)]
pub fn envelope_file(dir: &Path, formal: &str, answers: &Path) -> PathBuf {
    let inner: serde_json::Value = serde_json::from_slice(&fs::read(answers).unwrap()).unwrap();
    let document = serde_json::json!({ "template": formal, "answers": inner });
    let path = dir.join("envelope-answers.json");
    fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    path
}

/// The envelope text naming `formal` around a bare answers value, for stdin.
#[allow(dead_code)]
pub fn envelope_text(formal: &str, answers: &serde_json::Value) -> String {
    serde_json::json!({ "template": formal, "answers": answers }).to_string()
}

/// Parses the leading JSON value from a route's standard output, ignoring any
/// plain-text instructions an agent route appends after the batch document.
#[allow(dead_code)]
pub fn first_document(stdout: &[u8]) -> serde_json::Value {
    serde_json::Deserializer::from_slice(stdout)
        .into_iter::<serde_json::Value>()
        .next()
        .expect("a JSON document on standard output")
        .expect("a valid JSON document on standard output")
}

/// The `messages` array of a result document as strings.
#[allow(dead_code)]
pub fn document_messages(document: &serde_json::Value) -> Vec<String> {
    document["messages"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Reconstructs the plan-line text (`<action> <path>`, `hook [..] cwd <cwd>`)
/// from an `applied` or `planned` result document, matching the dry-run text the
/// former plain-text output produced, so plan-line fixture expectations hold
/// against the JSON document.
#[allow(dead_code)]
pub fn plan_text_from_document(document: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(files) = document["files"].as_array() {
        for file in files {
            let action = file["action"].as_str().unwrap_or_default();
            let path = file["path"].as_str().unwrap_or_default();
            match file["owner"].as_str() {
                Some(owner) => lines.push(format!("{action} {path} ({owner})")),
                None => lines.push(format!("{action} {path}")),
            }
        }
    }
    if let Some(hooks) = document["hooks"].as_array() {
        for hook in hooks {
            let command: Vec<String> = hook["command"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default();
            lines.push(format!(
                "hook {command:?} cwd {}",
                hook["cwd"].as_str().unwrap_or(".")
            ));
        }
    }
    lines.join("\n")
}

/// The decoded diagnostic text of a result document: an `error` document's
/// `message`, plus every per-question error string of a `questions` document.
/// Fixture `error_contains` parts match this rather than the JSON-escaped
/// standard output.
#[allow(dead_code)]
pub fn diagnostic_text(document: &serde_json::Value) -> String {
    let mut text = String::new();
    if let Some(message) = document["message"].as_str() {
        text.push_str(message);
    }
    if let Some(errors) = document["errors"].as_object() {
        for (id, value) in errors {
            text.push('\n');
            text.push_str(id);
            if let Some(items) = value.as_array() {
                for item in items {
                    if let Some(sentence) = item.as_str() {
                        text.push('\n');
                        text.push_str(sentence);
                    }
                }
            }
        }
    }
    text
}
