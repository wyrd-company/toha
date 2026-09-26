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
        self.apply_reporting(target, options, runner, &mut |_| {})
    }
    /// `apply`, calling `on_written` with each file path as soon as the file
    /// is written, so that a caller can report the files before any hook runs.
    pub fn apply_reporting(
        self,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        on_written: &mut dyn FnMut(&TargetPath),
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
            on_written(&file.path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hook::RecordingRunner,
        plan::{PlannedFile, PlannedProgram},
    };
    use std::{cell::RefCell, rc::Rc};

    /// A plan that renders `paths` in order from one support file, with
    /// `hooks` hooks.
    fn plan(source: &Path, paths: &[&str], hooks: usize) -> Plan {
        Plan {
            files: paths
                .iter()
                .map(|path| PlannedFile {
                    path: TargetPath::parse(path).unwrap(),
                    content: Content::Rendered(format!("{path}\n")),
                    source: source.to_owned(),
                })
                .collect(),
            conflicts: vec![],
            hooks: (0..hooks)
                .map(|index| PlannedHook {
                    program: PlannedProgram::Run(vec!["tool".into(), index.to_string()]),
                    cwd: None,
                    template_root: source.parent().unwrap().to_owned(),
                })
                .collect(),
            before_apply: None,
            after_apply: None,
        }
    }

    fn support() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("support.txt");
        fs::write(&source, "support\n").unwrap();
        (folder, source)
    }

    /// Records each reported file and each hook run in one event list.
    struct EventRunner(Rc<RefCell<Vec<String>>>);
    impl HookRunner for EventRunner {
        fn run(&self, hook: &PlannedHook, _target: &Path) -> Result<HookOutcome, HookError> {
            self.0.borrow_mut().push(format!("hook {}", hook.argv()[1]));
            Ok(HookOutcome {
                success: true,
                code: Some(0),
            })
        }
    }

    fn apply(
        plan: Plan,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        events: &Rc<RefCell<Vec<String>>>,
    ) -> Result<Applied, ApplyError> {
        plan.apply_reporting(target, options, runner, &mut |path| {
            events.borrow_mut().push(format!("file {path}"))
        })
    }

    const TRUSTED: ApplyOptions = ApplyOptions {
        force: false,
        trusted: true,
    };

    #[test]
    fn reports_each_file_once_in_plan_order_before_any_hook() {
        let (_folder, source) = support();
        let target = tempfile::tempdir().unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let plan = plan(&source, &["b.txt", "a/c.txt", "a.txt"], 2);
        let runner = EventRunner(events.clone());
        apply(plan, target.path(), TRUSTED, &runner, &events).unwrap();
        assert_eq!(
            *events.borrow(),
            [
                "file b.txt",
                "file a/c.txt",
                "file a.txt",
                "hook 0",
                "hook 1"
            ]
        );
    }

    #[test]
    fn reports_a_forced_overwrite() {
        let (_folder, source) = support();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("a.txt"), "existing\n").unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let options = ApplyOptions {
            force: true,
            trusted: true,
        };
        let plan = plan(&source, &["a.txt", "b.txt"], 0);
        apply(
            plan,
            target.path(),
            options,
            &RecordingRunner::new(),
            &events,
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["file a.txt", "file b.txt"]);
        assert_eq!(
            fs::read_to_string(target.path().join("a.txt")).unwrap(),
            "a.txt\n"
        );
    }

    #[test]
    fn reports_nothing_on_a_conflict_or_without_trust() {
        let (_folder, source) = support();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("b.txt"), "existing\n").unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let plan_a = plan(&source, &["a.txt", "b.txt"], 0);
        let error = apply(
            plan_a,
            target.path(),
            TRUSTED,
            &RecordingRunner::new(),
            &events,
        );
        assert!(matches!(error, Err(ApplyError::Conflicts(_))));
        let untrusted = ApplyOptions::default();
        let plan_b = plan(&source, &["c.txt"], 1);
        let applied = apply(
            plan_b,
            target.path(),
            untrusted,
            &RecordingRunner::new(),
            &events,
        );
        assert!(matches!(applied, Ok(Applied::NeedsTrust(_))));
        assert!(events.borrow().is_empty(), "{:?}", events.borrow());
    }

    #[test]
    fn a_failed_write_reports_only_the_files_written() {
        let (_folder, source) = support();
        let target = tempfile::tempdir().unwrap();
        fs::create_dir(target.path().join("dir")).unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let options = ApplyOptions {
            force: true,
            trusted: true,
        };
        let plan = plan(&source, &["a.txt", "dir", "z.txt"], 0);
        let error = apply(
            plan,
            target.path(),
            options,
            &RecordingRunner::new(),
            &events,
        );
        assert!(matches!(error, Err(ApplyError::Io { .. })), "{error:?}");
        assert_eq!(*events.borrow(), ["file a.txt"]);
    }

    #[test]
    fn a_failed_hook_keeps_the_reported_files() {
        let (_folder, source) = support();
        let target = tempfile::tempdir().unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let plan = plan(&source, &["a.txt", "b.txt"], 1);
        let error = apply(
            plan,
            target.path(),
            TRUSTED,
            &RecordingRunner::fail_at(0),
            &events,
        );
        assert!(matches!(error, Err(ApplyError::Hook { .. })));
        assert_eq!(*events.borrow(), ["file a.txt", "file b.txt"]);
    }
}
