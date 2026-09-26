// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fmt, fs,
    path::{Path, PathBuf},
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
        for part in value.split('/') {
            match part {
                ".git" => {
                    return Err("target path enters .git".into());
                }
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(format!("target path escapes target: {value}"));
                    }
                }
                value
                    if value.as_bytes().get(1) == Some(&b':')
                        && value.as_bytes()[0].is_ascii_alphabetic() =>
                {
                    return Err(format!("invalid target path: {value}"));
                }
                value => parts.push(value.to_owned()),
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
        let mut parts = self.0.components();
        if let Some(first) = parts.next() {
            first.as_os_str().to_string_lossy().fmt(f)?;
        }
        for part in parts {
            write!(f, "/{}", part.as_os_str().to_string_lossy())?;
        }
        Ok(())
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
    pub hooks: Vec<PlannedHook>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}
#[derive(Debug)]
pub struct PlannedFile {
    pub path: TargetPath,
    pub content: Content,
    pub source: PathBuf,
}
#[derive(Debug)]
pub enum Content {
    Rendered(String),
    Copied(PathBuf),
}
#[derive(Debug, Clone)]
pub struct PlannedHook {
    pub program: PlannedProgram,
    pub cwd: Option<TargetPath>,
    pub template_root: PathBuf,
}
#[derive(Debug, Clone)]
pub enum PlannedProgram {
    Run(Vec<String>),
    Script { path: PathBuf, args: Vec<String> },
}
impl PlannedHook {
    pub fn argv(&self) -> Vec<String> {
        match &self.program {
            PlannedProgram::Run(v) => v.clone(),
            PlannedProgram::Script { path, args } => std::iter::once(path.display().to_string())
                .chain(args.iter().cloned())
                .collect(),
        }
    }
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
    #[error("duplicate target {target}: {first} and {second}")]
    Duplicate {
        target: TargetPath,
        first: PathBuf,
        second: PathBuf,
    },
}
fn rendered(text: &str, ctx: &impl serde::Serialize, source: &Path) -> Result<String, PlanError> {
    Tmpl::compile(text.into())
        .and_then(|t| t.render(ctx))
        .map_err(|e| PlanError::Render {
            path: source.into(),
            message: e.to_string(),
        })
}
fn read_render(path: &Path, ctx: &impl serde::Serialize) -> Result<String, PlanError> {
    let bytes = fs::read(path).map_err(|source| PlanError::Io {
        path: path.into(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| PlanError::Render {
        path: path.into(),
        message: "file is not UTF-8; add it to static".into(),
    })?;
    rendered(&text, ctx, path)
}
fn target_path(path: &Path, ctx: &impl serde::Serialize) -> Result<Option<TargetPath>, PlanError> {
    let mut parts = Vec::new();
    for segment in path.components() {
        let value = rendered(&segment.as_os_str().to_string_lossy(), ctx, path)?;
        if value.is_empty() {
            return Ok(None);
        }
        parts.push(value);
    }
    TargetPath::parse(&parts.join("/"))
        .map(Some)
        .map_err(PlanError::Path)
}
impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Self, PlanError> {
        let ctx = context_from_answers(&completed.answers, &template.data, &completed.now);
        let mut plan = Self {
            files: vec![],
            conflicts: vec![],
            hooks: vec![],
            before_apply: None,
            after_apply: None,
        };
        walk(
            &template.source_dir,
            &template.source_dir,
            target,
            template,
            &ctx,
            &mut plan,
        )?;
        for (i, rule) in template.files.iter().enumerate() {
            let origin = template.root.join(&rule.source);
            if let Some(when) = &rule.when {
                let enabled = when.eval(&ctx).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: format!("files[{i}].when: {e}"),
                })?;
                if !enabled.is_true() {
                    continue;
                }
            }
            let contexts = rule.each.contexts(&ctx).map_err(|e| PlanError::Render {
                path: origin.clone(),
                message: format!("files[{i}].each: {e}"),
            })?;
            for local in contexts {
                let path_text = rule.path.render(&local).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: e.to_string(),
                })?;
                let path = TargetPath::parse(&path_text).map_err(PlanError::Path)?;
                let content = Content::Rendered(read_render(&origin, &local)?);
                plan.add(path, content, origin.clone(), target)?;
            }
        }
        for hook in &completed.hooks {
            plan.hooks.push(plan_hook(hook, template)?);
        }
        for (i, hook) in template.hooks.iter().enumerate() {
            if let Some(when) = &hook.when {
                if !when
                    .eval(&ctx)
                    .map_err(|e| PlanError::Render {
                        path: template.root.clone(),
                        message: format!(
                            "template error in hooks[{i}].when `{}`: {e}",
                            when.source()
                        ),
                    })?
                    .is_true()
                {
                    continue;
                }
            }
            let rendered =
                crate::interview::render_hooks(hook, &ctx).map_err(|f| PlanError::Render {
                    path: template.root.clone(),
                    message: format!(
                        "template error in hooks[{i}].{} `{}`: {}",
                        f.field, f.source, f.message
                    ),
                })?;
            for hook in &rendered {
                plan.hooks.push(plan_hook(hook, template)?);
            }
        }
        for (source, target_message) in [
            (&template.messages.before_apply, &mut plan.before_apply),
            (&template.messages.after_apply, &mut plan.after_apply),
        ] {
            if let Some(source) = source {
                let text = source.render(&ctx).map_err(|e| PlanError::Render {
                    path: template.root.clone(),
                    message: e.to_string(),
                })?;
                if !text.trim().is_empty() {
                    *target_message = Some(text);
                }
            }
        }
        Ok(plan)
    }
    fn add(
        &mut self,
        path: TargetPath,
        content: Content,
        source: PathBuf,
        target: &Path,
    ) -> Result<(), PlanError> {
        if let Some(first) = self.files.iter().find(|f| f.path == path) {
            return Err(PlanError::Duplicate {
                target: path,
                first: first.source.clone(),
                second: source,
            });
        }
        let destination = target.join(path.as_path());
        if has_symlink_component(target, &path).map_err(|source| PlanError::Io {
            path: destination.clone(),
            source,
        })? {
            return Err(PlanError::Path(format!(
                "target path contains symlink: {path}"
            )));
        }
        if destination.exists() {
            self.conflicts.push(path.clone());
        }
        self.files.push(PlannedFile {
            path,
            content,
            source,
        });
        Ok(())
    }
}
fn plan_hook(
    hook: &crate::interview::RenderedHook,
    template: &Template,
) -> Result<PlannedHook, PlanError> {
    use crate::interview::RenderedProgram;
    let program = match &hook.program {
        RenderedProgram::Run(v) => PlannedProgram::Run(v.clone()),
        RenderedProgram::Script { path, args } => PlannedProgram::Script {
            path: template.root.join(path),
            args: args.clone(),
        },
    };
    let cwd = hook
        .cwd
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(TargetPath::parse)
        .transpose()
        .map_err(PlanError::Path)?;
    Ok(PlannedHook {
        program,
        cwd,
        template_root: template.root.clone(),
    })
}
fn walk(
    root: &Path,
    dir: &Path,
    target: &Path,
    template: &Template,
    ctx: &impl serde::Serialize,
    plan: &mut Plan,
) -> Result<(), PlanError> {
    if !dir.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(dir)
        .map_err(|source| PlanError::Io {
            path: dir.into(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| PlanError::Io {
            path: dir.into(),
            source,
        })?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let source = entry.path();
        let relative = source.strip_prefix(root).unwrap();
        if root == template.root && dir == root && entry.file_name() == "template.yml" {
            continue;
        }
        if template.ignore.is_match(relative) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source: std::io::Error| PlanError::Io {
                path: entry.path(),
                source,
            })?;
        if file_type.is_symlink() {
            return Err(PlanError::Path(format!(
                "source symlink not supported: {}",
                source.display()
            )));
        }
        if file_type.is_dir() {
            walk(root, &source, target, template, ctx, plan)?;
            continue;
        }
        let Some(path) = target_path(relative, ctx)? else {
            continue;
        };
        let content = if template.static_files.is_match(relative) {
            Content::Copied(source.clone())
        } else {
            Content::Rendered(read_render(&source, ctx)?)
        };
        plan.add(path, content, source, target)?;
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
            "a/C:/b",
            "//server/share",
            "\\\\server\\share",
        ] {
            assert!(TargetPath::parse(bad).is_err(), "{bad}");
        }
    }
}
