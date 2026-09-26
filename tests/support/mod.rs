// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[allow(dead_code)]
pub fn isolated_command(root: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("toha"));
    command
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("TOHA_USER_CONFIG", root.join("config/toha/config.yml"))
        .env("TOHA_CONFIG", root.join("local.yml"));
    command
}

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

pub fn staged_dir(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("home/Library/Application Support/toha/staged")
    } else {
        root.join("toha/staged")
    }
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
