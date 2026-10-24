// ---
// relationships:
//   implements: command-line-interface
// ---
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use include_dir::{Dir, DirEntry, include_dir};
use serde::Deserialize;

static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills");

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Metadata {
    pub name: String,
    pub description: String,
}

fn metadata(dir: &Dir<'_>) -> Result<Metadata, String> {
    let file = dir
        .get_file(dir.path().join("SKILL.md"))
        .ok_or("embedded SKILL.md is missing")?;
    let body = file
        .contents_utf8()
        .ok_or("embedded SKILL.md is not UTF-8")?;
    let rest = body
        .strip_prefix("---\n")
        .ok_or("embedded SKILL.md has no frontmatter")?;
    let (yaml, _) = rest
        .split_once("\n---\n")
        .ok_or("embedded SKILL.md has no frontmatter end")?;
    serde_norway::from_str(yaml).map_err(|error| error.to_string())
}

fn skill_names() -> Vec<String> {
    let mut names: Vec<_> = SKILLS
        .dirs()
        .map(|dir| {
            dir.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn skill(name: &str) -> Result<&'static Dir<'static>, String> {
    SKILLS
        .get_dir(name)
        .filter(|dir| dir.path().components().count() == 1)
        .ok_or_else(|| {
            format!(
                "unknown skill {name}; available: {}",
                skill_names().join(", ")
            )
        })
}

fn inside(path: &Path) -> Result<PathBuf, String> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => return Err(format!("path outside skill: {}", path.display())),
        }
    }
    if result.as_os_str().is_empty() {
        return Err(format!("missing skill file: {}", path.display()));
    }
    Ok(result)
}

fn collect_files<'a>(dir: &'a Dir<'a>, files: &mut Vec<&'a include_dir::File<'a>>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => collect_files(child, files),
            DirEntry::File(file) => files.push(file),
        }
    }
}

fn export_directory(dir: &Dir<'_>, destination: &Path) -> Result<(), String> {
    let root = destination.join(dir.path());
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    for file in &files {
        let path = destination.join(file.path());
        let mut ancestors: Vec<_> = path.ancestors().skip(1).collect();
        ancestors.reverse();
        for ancestor in ancestors {
            match fs::metadata(ancestor) {
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(format!("file blocks export: {}", ancestor.display()));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if fs::symlink_metadata(ancestor).is_ok() {
                        return Err(format!("path blocks export: {}", ancestor.display()));
                    }
                }
                Err(error) => return Err(format!("{}: {error}", ancestor.display())),
            }
        }
        match fs::symlink_metadata(&path) {
            Ok(_) => return Err(format!("file already exists: {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    for file in files {
        let path = destination.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        output
            .write_all(file.contents())
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

/// Read embedded agent guidance without terminal output or configuration.
pub fn read(name: &str, path: &Path) -> Result<&'static [u8], String> {
    let dir = skill(name)?;
    let path = inside(path)?;
    dir.get_file(dir.path().join(&path))
        .map(|file| file.contents())
        .ok_or_else(|| format!("missing skill file: {}", path.display()))
}
/// List the embedded guidance metadata in stable name order.
pub fn list() -> Result<Vec<Metadata>, String> {
    skill_names()
        .iter()
        .map(|name| {
            let meta = metadata(skill(name)?)?;
            if meta.name != *name {
                return Err(format!("skill name mismatch: {name}"));
            }
            Ok(meta)
        })
        .collect()
}
/// Export one embedded skill with existing containment and create-new semantics.
pub fn export(name: &str, destination: &Path) -> Result<(), String> {
    export_directory(skill(name)?, destination)
}
