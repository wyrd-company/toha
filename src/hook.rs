// ---
// relationships:
//   implements: architecture
// ---
use crate::plan::{PlannedHook, PlannedProgram};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    process::Command,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
}
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("empty hook command")]
    EmptyCommand,
}
pub trait HookRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>;
}
pub struct ProcessRunner;
impl HookRunner for ProcessRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError> {
        let mut command = match &hook.program {
            PlannedProgram::Run(argv) => {
                let Some(program) = argv.first().filter(|v| !v.is_empty()) else {
                    return Err(HookError::EmptyCommand);
                };
                let mut c = Command::new(program);
                c.args(&argv[1..]);
                c
            }
            PlannedProgram::Script { path, args } => {
                let mut c = Command::new(path);
                c.args(args);
                c
            }
        };
        command.current_dir(
            target.join(
                hook.cwd
                    .as_ref()
                    .map(|v| v.as_path())
                    .unwrap_or(Path::new("")),
            ),
        );
        let status = command.status()?;
        Ok(HookOutcome {
            success: status.success(),
            code: status.code(),
        })
    }
}
#[derive(Debug, Default)]
pub struct RecordingRunner {
    calls: RefCell<Vec<(Vec<String>, String)>>,
    failed: Cell<Option<usize>>,
}
impl RecordingRunner {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fail_at(index: usize) -> Self {
        Self {
            failed: Cell::new(Some(index)),
            ..Self::default()
        }
    }
    pub fn calls(&self) -> Vec<(Vec<String>, String)> {
        self.calls.borrow().clone()
    }
}
impl HookRunner for RecordingRunner {
    fn run(&self, hook: &PlannedHook, _target: &Path) -> Result<HookOutcome, HookError> {
        let mut argv = hook.argv();
        if let PlannedProgram::Script { path, .. } = &hook.program {
            argv[0] = path
                .strip_prefix(&hook.template_root)
                .unwrap_or(path)
                .display()
                .to_string()
                .replace('\\', "/");
        }
        let cwd = hook
            .cwd
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let index = self.calls.borrow().len();
        self.calls.borrow_mut().push((argv, cwd));
        let success = self.failed.get() != Some(index);
        Ok(HookOutcome {
            success,
            code: Some(if success { 0 } else { 1 }),
        })
    }
}
