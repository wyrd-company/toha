// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Expect {
    pub exit: u8,
    #[serde(default)]
    #[allow(dead_code)]
    pub error_contains: Vec<String>,
    #[serde(default)]
    pub messages: Vec<String>,
    #[serde(default = "fixed_now")]
    pub now: String,
}

fn fixed_now() -> String {
    "2026-01-02T03:04:05+00:00[UTC]".into()
}

pub fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    paths
}

pub fn expectation(fixture: &Path) -> Expect {
    serde_norway::from_str(&fs::read_to_string(fixture.join("expect.yml")).unwrap()).unwrap()
}

pub fn assert_tree(actual: &Path, expected: &Path) {
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
        "missing: {missing:?}; extra: {extra:?}"
    );
    for path in expected_files {
        let actual_bytes = fs::read(actual.join(&path)).unwrap();
        let expected_bytes = fs::read(expected.join(&path)).unwrap();
        assert_eq!(
            actual_bytes,
            expected_bytes,
            "different file: {}",
            path.display()
        );
    }
}
