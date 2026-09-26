// ---
// relationships:
//   implements: command-line-interface
// ---
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use clap::Subcommand;
use include_dir::{Dir, DirEntry, include_dir};
use serde::Deserialize;

use super::Outcome;

static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills");

#[derive(Subcommand)]
pub enum Command {
    /// List the embedded skills as JSON.
    List,
    /// Print or export an embedded skill.
    View {
        name: String,
        /// Print a file inside the skill.
        #[arg(short, long, conflicts_with = "export")]
        path: Option<PathBuf>,
        /// Export the whole skill to this directory.
        #[arg(short, long)]
        export: Option<PathBuf>,
    },
}

#[derive(Deserialize, serde::Serialize)]
struct Metadata {
    name: String,
    description: String,
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

fn export(dir: &Dir<'_>, destination: &Path) -> Result<(), String> {
    let root = destination.join(dir.path());
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    for file in &files {
        let path = destination.join(file.path());
        if path.exists() {
            return Err(format!("file already exists: {}", path.display()));
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

pub fn run(command: Command) -> Outcome {
    match command {
        Command::List => {
            let result: Result<Vec<_>, String> = skill_names()
                .iter()
                .map(|name| {
                    let dir = skill(name)?;
                    let meta = metadata(dir)?;
                    if meta.name != *name {
                        return Err(format!("skill name mismatch: {name}"));
                    }
                    Ok(meta)
                })
                .collect();
            match result {
                Ok(items) => {
                    Outcome::Document(serde_json::to_value(items).expect("skill metadata"), 0)
                }
                Err(error) => Outcome::Error(error),
            }
        }
        Command::View {
            name,
            path,
            export: destination,
        } => {
            let dir = match skill(&name) {
                Ok(dir) => dir,
                Err(error) => return Outcome::Error(error),
            };
            if let Some(destination) = destination {
                return match export(dir, &destination) {
                    Ok(()) => Outcome::Saved(0),
                    Err(error) => Outcome::Error(error),
                };
            }
            let path = match inside(path.as_deref().unwrap_or(Path::new("SKILL.md"))) {
                Ok(path) => path,
                Err(error) => return Outcome::Error(error),
            };
            match dir.get_file(dir.path().join(&path)) {
                Some(file) => match std::io::stdout().write_all(file.contents()) {
                    Ok(()) => Outcome::Saved(0),
                    Err(error) => Outcome::Error(error.to_string()),
                },
                None => Outcome::Error(format!("missing skill file: {}", path.display())),
            }
        }
    }
}
