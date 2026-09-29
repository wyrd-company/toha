// ---
// relationships:
//   implements: architecture
// ---
use crate::plan::{PlannedHook, PlannedProgram};
use crate::template::Id;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Read,
    path::Path,
    process::{Command, Stdio},
};
/// The outcome of running one hook. `stdout`/`stderr` hold the raw captured
/// bytes of a stream the hook declared in `capture`, and are `None` for an
/// uncaptured stream. The bytes are raw so that `apply` can strict-UTF-8 decode
/// them only after the tolerance-aware failure gate; the runner never decodes,
/// because it does not know whether a nonzero exit is tolerated.
#[derive(Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}
impl std::fmt::Debug for HookOutcome {
    /// Prints only `success` and `code`; captured bytes never appear in any
    /// `Debug` or `Display`, so an `ApplyError::Hook` message carries no output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookOutcome")
            .field("success", &self.success)
            .field("code", &self.code)
            .finish()
    }
}
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("empty hook command")]
    EmptyCommand,
}
/// Strict UTF-8 decode of a captured stream, with every trailing `\r`/`\n`
/// removed (shell `$(…)` semantics). On invalid UTF-8 it reports the byte
/// offset of the first invalid sequence and no bytes.
pub(crate) fn decode(bytes: Vec<u8>) -> Result<String, usize> {
    let text = String::from_utf8(bytes).map_err(|e| e.utf8_error().valid_up_to())?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}
pub trait HookRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>;
}
pub struct ProcessRunner;
impl ProcessRunner {
    fn command(hook: &PlannedHook, target: &Path) -> Result<Command, HookError> {
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
        Ok(command)
    }
}
impl HookRunner for ProcessRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError> {
        let mut command = Self::command(hook, target)?;
        let capture = hook.capture;
        match (capture.stdout, capture.stderr) {
            // No capture: today's path exactly — inherited stdio, `status()`.
            (false, false) => {
                let status = command.status()?;
                Ok(HookOutcome {
                    success: status.success(),
                    code: status.code(),
                    stdout: None,
                    stderr: None,
                })
            }
            // Both captured: pipe both and read them concurrently.
            (true, true) => {
                let output = command
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()?;
                Ok(HookOutcome {
                    success: output.status.success(),
                    code: output.status.code(),
                    stdout: Some(output.stdout),
                    stderr: Some(output.stderr),
                })
            }
            // One stream captured, the other inherited: only one pipe exists, so
            // reading it fully before `wait` cannot deadlock.
            (stdout_captured, _) => {
                command.stdout(if stdout_captured {
                    Stdio::piped()
                } else {
                    Stdio::inherit()
                });
                command.stderr(if stdout_captured {
                    Stdio::inherit()
                } else {
                    Stdio::piped()
                });
                let mut child = command.spawn()?;
                let mut buffer = Vec::new();
                if stdout_captured {
                    child
                        .stdout
                        .take()
                        .expect("piped stdout")
                        .read_to_end(&mut buffer)?;
                } else {
                    child
                        .stderr
                        .take()
                        .expect("piped stderr")
                        .read_to_end(&mut buffer)?;
                }
                let status = child.wait()?;
                let (stdout, stderr) = if stdout_captured {
                    (Some(buffer), None)
                } else {
                    (None, Some(buffer))
                };
                Ok(HookOutcome {
                    success: status.success(),
                    code: status.code(),
                    stdout,
                    stderr,
                })
            }
        }
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
            stdout: None,
            stderr: None,
        })
    }
}

/// The declared shape of one hook result producer, used to seed a not-run value
/// and to record a run. A producer with `json` reads its parsed stdout directly
/// at `id` and, when declared, its execution metadata at `status_id`.
#[derive(Debug, Clone)]
pub(crate) struct ResultSpec {
    pub id: Id,
    pub json: bool,
    pub status_id: Option<Id>,
}

/// The per-apply table of hook results, projected into a deferred surface's
/// render context. It is `pub(crate)` and dropped when `apply` returns; results
/// never reach `Plan`, `Applied`, `ApplyError`, or `StagedRecord`.
#[derive(Debug, Default)]
pub(crate) struct HookResults {
    /// The Jinja bindings each declared producer contributes, in declaration
    /// order. A text producer contributes `id`; a JSON producer contributes `id`
    /// and, when declared, `status_id`.
    bindings: BTreeMap<String, Value>,
}
impl HookResults {
    /// The three-field metadata object shared by text results and JSON
    /// `status_id`s, plus `parsed` for JSON.
    fn meta(exit_code: Option<i32>, stdout: &Option<String>, stderr: &Option<String>) -> Value {
        json!({
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        })
    }
    /// Binds a declared producer as not-run: text `id` is the three-`none`
    /// object; JSON `id` is `none` and its `status_id` is the metadata object
    /// with `parsed: false`.
    pub(crate) fn seed(&mut self, spec: &ResultSpec) {
        if spec.json {
            self.bindings
                .insert(spec.id.as_str().to_owned(), Value::Null);
            if let Some(status) = &spec.status_id {
                let mut meta = Self::meta(None, &None, &None);
                meta["parsed"] = Value::Bool(false);
                self.bindings.insert(status.as_str().to_owned(), meta);
            }
        } else {
            self.bindings
                .insert(spec.id.as_str().to_owned(), Self::meta(None, &None, &None));
        }
    }
    /// Records a text-mode producer's run: `id` is `{exit_code, stdout, stderr}`.
    pub(crate) fn record_text(
        &mut self,
        id: &Id,
        exit_code: Option<i32>,
        stdout: Option<String>,
        stderr: Option<String>,
    ) {
        self.bindings.insert(
            id.as_str().to_owned(),
            Self::meta(exit_code, &stdout, &stderr),
        );
    }
    /// Records a JSON-mode producer's run: `id` is the parsed value (or `none`
    /// when the stdout did not parse); `status_id`, when declared, is
    /// `{exit_code, stdout, stderr, parsed}`.
    pub(crate) fn record_json(
        &mut self,
        spec: &ResultSpec,
        exit_code: Option<i32>,
        stdout: Option<String>,
        stderr: Option<String>,
        value: Option<Value>,
        parsed: bool,
    ) {
        self.bindings
            .insert(spec.id.as_str().to_owned(), value.unwrap_or(Value::Null));
        if let Some(status) = &spec.status_id {
            let mut meta = Self::meta(exit_code, &stdout, &stderr);
            meta["parsed"] = Value::Bool(parsed);
            self.bindings.insert(status.as_str().to_owned(), meta);
        }
    }
    /// Projects every recorded binding into a render context.
    pub(crate) fn project(&self, ctx: &mut BTreeMap<String, Value>) {
        for (name, value) in &self.bindings {
            ctx.insert(name.clone(), value.clone());
        }
    }
}
