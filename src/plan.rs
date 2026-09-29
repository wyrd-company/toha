// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::{
    fault::TemplateFault,
    interview::Completed,
    jinja::{Tmpl, context_from_answers},
    staging::CanonicalTarget,
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
    #[error("plan target does not match the completed interview's context target")]
    ContextTarget,
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
impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &CanonicalTarget,
    ) -> Result<Self, PlanError> {
        // The plan renders under the completed interview's context. A current
        // context must carry the target the plan is applied to; a legacy context
        // carries the same target restored during replay.
        if completed.context().target() != target {
            return Err(PlanError::ContextTarget);
        }
        let mut ctx = context_from_answers(&completed.answers, &template.data, &completed.now);
        completed.context().project(&mut ctx);
        let mut plan = Self {
            files: vec![],
            conflicts: vec![],
            hooks: vec![],
            before_apply: None,
            after_apply: None,
        };
        // Render the retained source-tree program; it is not reopened here.
        for entry in &template.render_program {
            let mut parts = Vec::new();
            let mut skip = false;
            for segment in &entry.segments {
                let value = segment.render(&ctx).map_err(|e| PlanError::Render {
                    path: entry.source.clone(),
                    message: TemplateFault {
                        field: "path".into(),
                        expression: Some(segment.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                if value.is_empty() {
                    skip = true;
                    break;
                }
                parts.push(value);
            }
            if skip {
                continue;
            }
            let path = TargetPath::parse(&parts.join("/")).map_err(PlanError::Path)?;
            let content = match &entry.body {
                crate::template::SourceBody::Static => Content::Copied(entry.source.clone()),
                crate::template::SourceBody::Rendered(body) => {
                    Content::Rendered(body.render(&ctx).map_err(|e| PlanError::Render {
                        path: entry.source.clone(),
                        message: e.to_string(),
                    })?)
                }
                crate::template::SourceBody::NonUtf8 => {
                    return Err(PlanError::Render {
                        path: entry.source.clone(),
                        message: "file is not UTF-8; add it to static".into(),
                    });
                }
            };
            plan.add(path, content, entry.source.clone(), target.as_path())?;
        }
        for (i, rule) in template.files.iter().enumerate() {
            let origin = template.root.join(&rule.source);
            if let Some(when) = &rule.when {
                let enabled = when.eval(&ctx).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: TemplateFault {
                        field: format!("files[{i}].when"),
                        expression: Some(when.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                if !enabled.is_true() {
                    continue;
                }
            }
            let contexts = rule.each.contexts(&ctx).map_err(|e| PlanError::Render {
                path: origin.clone(),
                message: TemplateFault {
                    field: format!("files[{i}].each"),
                    expression: Some(rule.each.expr.source().into()),
                    message: e,
                }
                .to_string(),
            })?;
            for local in contexts {
                let path_text = rule.path.render(&local).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: TemplateFault {
                        field: format!("files[{i}].path"),
                        expression: Some(rule.path.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                let path = TargetPath::parse(&path_text).map_err(PlanError::Path)?;
                let rendered = match &rule.body {
                    Some(body) => body.render(&local).map_err(|e| PlanError::Render {
                        path: origin.clone(),
                        message: e.to_string(),
                    })?,
                    // A non-UTF-8 rule body was tolerated at load; surface it now.
                    None => read_render(&origin, &local)?,
                };
                let content = Content::Rendered(rendered);
                plan.add(path, content, origin.clone(), target.as_path())?;
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
