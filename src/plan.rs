// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    interview::Completed,
    jinja::{Tmpl, context_from_answers},
    template::Template,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPath(PathBuf);
impl TargetPath {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || value.starts_with('/')
            || value.starts_with('\\')
            || value.contains('\\')
            || value.as_bytes().get(1) == Some(&b':')
        {
            return Err(format!("invalid target path: {value}"));
        }
        let mut parts = Vec::new();
        for part in Path::new(value).components() {
            match part {
                Component::Normal(value) if value == ".git" => {
                    return Err(format!("target path enters .git: {value:?}"));
                }
                Component::Normal(value) => parts.push(value.to_owned()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err(format!("target path escapes target: {value}"));
                    }
                }
                _ => return Err(format!("invalid target path: {value}")),
            }
        }
        if parts.is_empty() {
            return Err("empty target path".into());
        }
        Ok(Self(parts.iter().collect()))
    }
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
impl fmt::Display for TargetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

pub(crate) fn has_symlink_component(
    target: &Path,
    path: &TargetPath,
) -> Result<bool, std::io::Error> {
    let mut current = target.to_owned();
    for component in path.as_path().components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[derive(Debug)]
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub conflicts: Vec<TargetPath>,
}
#[derive(Debug)]
pub struct PlannedFile {
    pub path: TargetPath,
    pub content: Content,
}
#[derive(Debug)]
pub enum Content {
    Rendered(String),
}
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{0}")]
    Path(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Render { path: PathBuf, message: String },
}

impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Self, PlanError> {
        let mut plan = Self {
            files: Vec::new(),
            conflicts: Vec::new(),
        };
        walk(
            &template.source_dir,
            &template.source_dir,
            target,
            completed,
            template,
            &mut plan,
        )?;
        Ok(plan)
    }
}

fn walk(
    source_root: &Path,
    dir: &Path,
    target: &Path,
    completed: &Completed,
    template: &Template,
    plan: &mut Plan,
) -> Result<(), PlanError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|source| PlanError::Io {
            path: dir.to_owned(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| PlanError::Io {
            path: dir.to_owned(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        if source_root == dir && entry.file_name() == "template.yml" {
            continue;
        }
        let relative = source_path
            .strip_prefix(source_root)
            .expect("walk stays under source root");
        let mut rendered = PathBuf::new();
        let mut skip = false;
        for segment in relative.components() {
            let text = segment.as_os_str().to_string_lossy();
            let segment_template =
                Tmpl::compile(text.into_owned()).map_err(|error| PlanError::Render {
                    path: source_path.clone(),
                    message: error.to_string(),
                })?;
            let value = segment_template
                .render(context_from_answers(
                    &completed.answers,
                    &template.data,
                    &completed.now,
                ))
                .map_err(|error| PlanError::Render {
                    path: source_path.clone(),
                    message: error.to_string(),
                })?;
            if value.is_empty() {
                skip = true;
                break;
            }
            rendered.push(value);
        }
        if skip {
            continue;
        }
        let path = TargetPath::parse(&rendered.to_string_lossy()).map_err(PlanError::Path)?;
        let file_type = entry.file_type().map_err(|source| PlanError::Io {
            path: source_path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(PlanError::Path(format!(
                "source symlink not supported: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            walk(source_root, &source_path, target, completed, template, plan)?;
            continue;
        }
        let source = fs::read_to_string(&source_path).map_err(|source| PlanError::Io {
            path: source_path.clone(),
            source,
        })?;
        let content = Tmpl::compile(source)
            .and_then(|tmpl| {
                tmpl.render(context_from_answers(
                    &completed.answers,
                    &template.data,
                    &completed.now,
                ))
            })
            .map_err(|error| PlanError::Render {
                path: source_path.clone(),
                message: error.to_string(),
            })?;
        let destination = target.join(path.as_path());
        if has_symlink_component(target, &path).map_err(|source| PlanError::Io {
            path: destination.clone(),
            source,
        })? {
            return Err(PlanError::Path(format!(
                "target path contains symlink: {path}"
            )));
        }
        if destination.exists() || plan.files.iter().any(|file| file.path == path) {
            plan.conflicts.push(path.clone());
        }
        plan.files.push(PlannedFile {
            path,
            content: Content::Rendered(content),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TargetPath;
    #[test]
    fn target_path() {
        assert_eq!(TargetPath::parse("a/./b/../c").unwrap().to_string(), "a/c");
        for bad in [
            "",
            ".",
            "../a",
            "a/../../b",
            "/a",
            "a/.git/b",
            "a\\b",
            "C:/a",
        ] {
            assert!(TargetPath::parse(bad).is_err(), "{bad}");
        }
    }
}
