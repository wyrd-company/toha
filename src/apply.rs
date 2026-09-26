// ---
// relationships:
//   implements: architecture
// ---
use crate::{
    hook::{HookError, HookOutcome, HookRunner},
    plan::{Content, Plan, PlannedHook, TargetPath, has_symlink_component},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyOptions {
    pub force: bool,
    pub trusted: bool,
}
#[derive(Debug)]
pub enum Applied {
    Written {
        files: Vec<TargetPath>,
        hooks_run: usize,
        after_apply: Option<String>,
    },
    NeedsTrust(Plan),
}
/// One indented target path per line, each after a line break.
fn conflict_lines(paths: &[TargetPath]) -> String {
    paths.iter().map(|path| format!("\n  {path}")).collect()
}
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("conflicting files:{}", conflict_lines(.0))]
    Conflicts(Vec<TargetPath>),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("target path contains symlink: {0}")]
    Symlink(TargetPath),
    #[error("hook {index} failed: {hook:?}, outcome: {outcome:?}")]
    Hook {
        index: usize,
        hook: PlannedHook,
        outcome: HookOutcome,
    },
    #[error("hook {index} failed: {hook:?}: {source}")]
    HookIo {
        index: usize,
        hook: PlannedHook,
        source: HookError,
    },
}
impl Plan {
    pub fn apply(
        self,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
    ) -> Result<Applied, ApplyError> {
        let mut conflicts = self.conflicts.clone();
        for file in &self.files {
            if target.join(file.path.as_path()).exists() && !conflicts.contains(&file.path) {
                conflicts.push(file.path.clone());
            }
        }
        if !options.force && !conflicts.is_empty() {
            return Err(ApplyError::Conflicts(conflicts));
        }
        if !options.trusted && !self.hooks.is_empty() {
            return Ok(Applied::NeedsTrust(self));
        }
        for file in &self.files {
            if has_symlink_component(target, &file.path).map_err(|source| ApplyError::Io {
                path: target.join(file.path.as_path()),
                source,
            })? {
                return Err(ApplyError::Symlink(file.path.clone()));
            }
        }
        for hook in &self.hooks {
            if let Some(cwd) = &hook.cwd {
                if has_symlink_component(target, cwd).map_err(|source| ApplyError::Io {
                    path: target.join(cwd.as_path()),
                    source,
                })? {
                    return Err(ApplyError::Symlink(cwd.clone()));
                }
            }
        }
        let mut written = Vec::new();
        for file in &self.files {
            let path = target.join(file.path.as_path());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| ApplyError::Io {
                    path: parent.into(),
                    source,
                })?;
            }
            match &file.content {
                Content::Rendered(text) => {
                    fs::write(&path, text).map_err(|source| ApplyError::Io {
                        path: path.clone(),
                        source,
                    })?
                }
                Content::Copied(source) => {
                    fs::copy(source, &path).map_err(|source| ApplyError::Io {
                        path: path.clone(),
                        source,
                    })?;
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&file.source)
                    .map_err(|source| ApplyError::Io {
                        path: file.source.clone(),
                        source,
                    })?
                    .permissions()
                    .mode()
                    & 0o111;
                if mode != 0 {
                    let mut permissions = fs::metadata(&path)
                        .map_err(|source| ApplyError::Io {
                            path: path.clone(),
                            source,
                        })?
                        .permissions();
                    permissions.set_mode(permissions.mode() | mode);
                    fs::set_permissions(&path, permissions).map_err(|source| ApplyError::Io {
                        path: path.clone(),
                        source,
                    })?;
                }
            }
            written.push(file.path.clone());
        }
        for (index, hook) in self.hooks.iter().enumerate() {
            let outcome = runner
                .run(hook, target)
                .map_err(|source| ApplyError::HookIo {
                    index,
                    hook: hook.clone(),
                    source,
                })?;
            if !outcome.success {
                return Err(ApplyError::Hook {
                    index,
                    hook: hook.clone(),
                    outcome,
                });
            }
        }
        Ok(Applied::Written {
            files: written,
            hooks_run: self.hooks.len(),
            after_apply: self.after_apply,
        })
    }
}
