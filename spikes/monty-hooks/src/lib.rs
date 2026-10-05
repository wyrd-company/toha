//! Spike host for running Toha hooks as Python scripts inside Monty.
//!
//! The target directory is the only mount. The only way out of the sandbox
//! other than the mount is the host function `run(argv)`, which starts a
//! process only when the caller trusts the template.

use std::{
    cell::Cell,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use monty::{MontyRun, RunProgress};
use monty_fs::{MountCallOutcome, MountMode, MountTable};
use monty_types::{
    CompileOptions, ExcType, ExtFunctionResult, MontyException, MontyObject, NameLookupResult,
    PrintWriter, ResourceLimits, ResourceTracker, normalize_virtual_path,
};

/// The virtual path the target directory is mounted at.
pub const TARGET: &str = "/target";

/// Options for one hook run.
#[derive(Clone, Copy, Debug)]
pub struct HookOptions<'a> {
    /// Interview answers, bound to the sandbox global `answers`.
    pub answers: &'a [(&'a str, &'a str)],
    /// Whether the template is trusted; gates the `run` host function.
    pub trusted: bool,
    /// Whether the host refuses any filesystem path with a `.git` component.
    pub deny_git: bool,
    /// Mount mode for the target directory.
    pub mode: Mode,
    /// Execution-time budget; `None` runs unbounded.
    pub max_duration: Option<Duration>,
    /// Allocator budget in bytes; `None` runs unbounded.
    pub max_memory: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    ReadWrite,
    ReadOnly,
}

impl Default for HookOptions<'_> {
    fn default() -> Self {
        Self {
            answers: &[],
            trusted: false,
            deny_git: false,
            mode: Mode::ReadWrite,
            max_duration: Some(Duration::from_secs(5)),
            max_memory: Some(64 * 1024 * 1024),
        }
    }
}

/// The observable result of one hook run.
#[derive(Debug)]
pub struct HookRun {
    /// The final expression value, or the exception that ended the run.
    pub result: Result<MontyObject, MontyException>,
    /// Text the hook printed.
    pub printed: String,
    /// Processes started through the `run` host function.
    pub spawned: Vec<Vec<String>>,
    /// Filesystem calls that reached no mount and fell through to the host.
    pub unmounted_os_calls: Vec<String>,
    /// Wall time for compile + run.
    pub elapsed: Duration,
}

impl HookRun {
    /// The exception type name, when the run failed.
    pub fn error_type(&self) -> Option<String> {
        self.result.as_ref().err().map(|e| format!("{:?}", e.exc_type()))
    }
}

fn permission_error(message: impl Into<String>) -> MontyException {
    MontyException::new(ExcType::PermissionError, Some(message.into()))
}

fn has_git_component(path: &str) -> bool {
    normalize_virtual_path(path).split('/').any(|part| part == ".git")
}

/// Runs `code` as a hook with `target` mounted at [`TARGET`].
pub fn run_hook(code: &str, target: &Path, options: HookOptions<'_>) -> HookRun {
    let started = Instant::now();
    let mut printed = String::new();
    let spawned = Cell::new(Vec::new());
    let mut unmounted = Vec::new();

    let result = (|| {
        let mut mounts = MountTable::new();
        let mode = match options.mode {
            Mode::ReadWrite => MountMode::ReadWrite,
            Mode::ReadOnly => MountMode::ReadOnly,
        };
        mounts
            .mount(TARGET, target, mode, None)
            .map_err(|e| MontyException::new(ExcType::RuntimeError, Some(e.to_string())))?;

        let mut runner = MontyRun::new(code.to_owned(), "hook.py", vec!["run".to_owned(), "answers".to_owned()], CompileOptions::default())?;
        // Relative paths resolve against the target, as a process hook's cwd does.
        runner.set_cwd(TARGET);
        let limits = ResourceLimits {
            max_feed_duration: options.max_duration,
            max_memory: options.max_memory,
            ..ResourceLimits::default()
        };
        let mut progress = runner.start(
            vec![
                MontyObject::function("run", None),
                MontyObject::dict(
                    options
                        .answers
                        .iter()
                        .map(|(k, v)| (MontyObject::string(*k), MontyObject::string(*v))),
                ),
            ],
            ResourceTracker::new(limits),
            PrintWriter::CollectString(&mut printed, Some(1 << 20)),
        )?;
        loop {
            progress = match progress {
                RunProgress::Complete(value) => return Ok(value),
                RunProgress::FunctionCall(call) => {
                    let print = PrintWriter::Disabled;
                    if call.function_name != "run" {
                        let message = format!("no host function {}", call.function_name);
                        call.abort(permission_error(message), print)?
                    } else if !options.trusted {
                        // Uncatchable: the hook cannot swallow the refusal and carry on.
                        call.abort(permission_error("run() requires a trusted template"), print)?
                    } else {
                        let argv: Option<Vec<String>> = call.args.arg(0).and_then(|list| {
                            list.items()?.iter().map(|item| item.as_str().map(str::to_owned)).collect()
                        });
                        match argv {
                            Some(argv) if !argv.is_empty() => {
                                let mut log = spawned.take();
                                log.push(argv.clone());
                                spawned.set(log);
                                let output = Command::new(&argv[0]).args(&argv[1..]).current_dir(target).output();
                                let result: ExtFunctionResult = match output {
                                    Ok(out) => MontyObject::dict([
                                        (
                                            MontyObject::string("exit_code"),
                                            out.status.code().map_or_else(MontyObject::none, |c| MontyObject::int(c.into())),
                                        ),
                                        (
                                            MontyObject::string("stdout"),
                                            MontyObject::string(String::from_utf8_lossy(&out.stdout)),
                                        ),
                                        (
                                            MontyObject::string("stderr"),
                                            MontyObject::string(String::from_utf8_lossy(&out.stderr)),
                                        ),
                                    ])
                                    .into(),
                                    Err(e) => MontyException::new(ExcType::OSError, Some(e.to_string())).into(),
                                };
                                call.resume(result, print)?
                            }
                            _ => call.resume(
                                MontyException::new(ExcType::TypeError, Some("run() takes a list of str".into())),
                                print,
                            )?,
                        }
                    }
                }
                RunProgress::OsCall(call) => {
                    let fc = &call.function_call;
                    let git = options.deny_git
                        && (fc.fs_primary_path().is_some_and(has_git_component)
                            || fc.rename_destination().is_some_and(has_git_component));
                    if git {
                        call.resume(permission_error("hooks cannot reach .git"), PrintWriter::Disabled)?
                    } else {
                        let unmounted = &mut unmounted;
                        call.resume_with(PrintWriter::Disabled, |fc| match mounts.handle_os_call(fc) {
                            MountCallOutcome::Handled(Ok(value)) => value.into(),
                            MountCallOutcome::Handled(Err(e)) => e.into_exception().into(),
                            MountCallOutcome::NotHandled(fc) => {
                                unmounted.push(format!("{fc:?}"));
                                fc.on_no_handler().into()
                            }
                        })?
                    }
                }
                RunProgress::NameLookup(lookup) => lookup.resume(NameLookupResult::Undefined, PrintWriter::Disabled)?,
                RunProgress::ResolveFutures(futures) => {
                    futures.abort(permission_error("no host futures"), PrintWriter::Disabled)?
                }
            };
        }
    })();

    HookRun {
        result,
        printed,
        spawned: spawned.take(),
        unmounted_os_calls: unmounted,
        elapsed: started.elapsed(),
    }
}
