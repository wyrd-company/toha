// ---
// relationships:
//   implements: architecture
// ---
use crate::{
    hook::{HookError, HookOutcome, HookResults, HookRunner, decode},
    inject::{
        EditResolution, JsonEditError, PlannedEdit, RegionError, RegionKey, resolve_json_edit,
        resolve_region_edit,
    },
    plan::{Content, Plan, PlanError, Planned, PlannedHook, TargetPath, has_symlink_component},
    staging::CanonicalTarget,
    template::Id,
};
#[cfg(test)]
use std::path::Path;
use std::{fmt, fs, io::Write, path::PathBuf};
#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyOptions {
    pub force: bool,
    pub trusted: bool,
}
/// One hook that ran to completion during a successful apply: its id when the
/// hook declares one, its exit code, and whether it succeeded (an
/// `allow-failure` hook can succeed the apply with a nonzero code). Captured
/// streams never appear here, so no hook output bytes leave the apply.
#[derive(Debug, Clone)]
pub struct HookReport {
    pub id: Option<Id>,
    pub exit_code: Option<i32>,
    pub success: bool,
}
#[derive(Debug)]
pub enum Applied {
    Written {
        files: Vec<TargetPath>,
        hooks_run: usize,
        hooks: Vec<HookReport>,
        after_apply: Option<String>,
    },
    NeedsTrust(Plan),
}
/// One indented target path per line, each after a line break.
fn conflict_lines(paths: &[TargetPath]) -> String {
    paths.iter().map(|path| format!("\n  {path}")).collect()
}
/// One indented `path (region)` per line for the region-drift error.
fn drift_lines(regions: &[(TargetPath, RegionKey)]) -> String {
    regions
        .iter()
        .map(|(path, region)| format!("\n  {path} ({region})"))
        .collect()
}

/// One injected target resolved in memory before the commit loop: its final
/// image when it changed, and the index of its whole-file rule when one supplies
/// the starting image and its executable mode.
struct EditedTarget {
    path: TargetPath,
    /// The bytes to write, or `None` when every edit left the target unchanged.
    image: Option<Vec<u8>>,
    whole_file: Option<usize>,
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
    /// A deferred hook or after-apply message failed to render, or its deferred
    /// `cwd` was invalid, in the apply loop.
    #[error("{0}")]
    Deferred(PlanError),
    /// An operator edited an owned region; `--force` would replace it. Writes
    /// nothing. Each entry is a `path (region)` pair.
    #[error("region drift; rerun with --force to overwrite:{}", drift_lines(.0))]
    Drift(Vec<(TargetPath, RegionKey)>),
    /// A region edit could not be resolved against the target.
    #[error("{0}")]
    Region(#[from] RegionError),
    /// A JSON-family edit could not be resolved against the target.
    #[error("{0}")]
    Json(#[from] JsonEditError),
    /// A captured stream could not be decoded or parsed. Carries no bytes: only
    /// the hook index, its id, and the fault's position.
    #[error("hook {index} `{id}` output: {fault}")]
    HookOutput {
        index: usize,
        id: Id,
        fault: OutputFault,
    },
}
/// A fault in a hook's captured output. Every variant is byte-free: a message
/// carries only a position, never the stream's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFault {
    /// The captured stream was not valid UTF-8, first invalid at `valid_up_to`.
    NotUtf8 { valid_up_to: usize },
    /// `parse: json` on an exit-0 hook whose stdout was empty.
    Empty,
    /// `parse: json` on an exit-0 hook whose stdout was not valid JSON.
    NotJson { line: usize, column: usize },
}
impl fmt::Display for OutputFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 { valid_up_to } => {
                write!(
                    f,
                    "captured stream is not valid UTF-8 (valid up to byte {valid_up_to})"
                )
            }
            Self::Empty => write!(f, "parse: json but the captured stdout was empty"),
            Self::NotJson { line, column } => {
                write!(
                    f,
                    "parse: json but the captured stdout was not JSON at line {line} column {column}"
                )
            }
        }
    }
}
impl Plan {
    // `ApplyError::Hook` intentionally carries the planned hook and its outcome
    // for diagnostics; the crate accepts the resulting large `Err`, as elsewhere.
    #[allow(clippy::result_large_err)]
    pub fn apply(
        self,
        target: &CanonicalTarget,
        options: ApplyOptions,
        runner: &dyn HookRunner,
    ) -> Result<Applied, ApplyError> {
        self.apply_reporting(target, options, runner, &mut |_| {})
    }
    /// `apply`, calling `on_written` with each file path as soon as the file
    /// is written, so that a caller can report the files before any hook runs.
    #[allow(clippy::result_large_err)]
    pub fn apply_reporting(
        self,
        target: &CanonicalTarget,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        on_written: &mut dyn FnMut(&TargetPath),
    ) -> Result<Applied, ApplyError> {
        let mut conflicts = self.conflicts.clone();
        for file in &self.files {
            if target.as_path().join(file.path.as_path()).exists()
                && !conflicts.contains(&file.path)
            {
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
            if has_symlink_component(target.as_path(), &file.path).map_err(|source| {
                ApplyError::Io {
                    path: target.as_path().join(file.path.as_path()),
                    source,
                }
            })? {
                return Err(ApplyError::Symlink(file.path.clone()));
            }
        }
        // Every injected target's path is protected before it is read or written.
        for edit in &self.edits {
            if has_symlink_component(target.as_path(), edit.path()).map_err(|source| {
                ApplyError::Io {
                    path: target.as_path().join(edit.path().as_path()),
                    source,
                }
            })? {
                return Err(ApplyError::Symlink(edit.path().clone()));
            }
        }
        // A ready hook's cwd is known now, so it is symlink-checked before any
        // file is written, exactly as before. A deferred hook's cwd is not known
        // until its result-reading fields render, so it is checked in the loop.
        for planned in &self.hooks {
            if let Planned::Ready(hook) = planned
                && let Some(cwd) = &hook.cwd
                && has_symlink_component(target.as_path(), cwd).map_err(|source| {
                    ApplyError::Io {
                        path: target.as_path().join(cwd.as_path()),
                        source,
                    }
                })?
            {
                return Err(ApplyError::Symlink(cwd.clone()));
            }
        }
        // Resolve every injected target fully in memory before the first write, so
        // an unforced region drift, a parse or path error, or a missing target
        // leaves every file on disk unchanged.
        let resolved = self.resolve_edits(target, options.force)?;
        let mut written = Vec::new();
        // Whole-file targets that carry no edit write exactly as before. A target
        // with edits is committed once below, from its folded image.
        for file in &self.files {
            if resolved.iter().any(|e| e.path == file.path) {
                continue;
            }
            let path = target.as_path().join(file.path.as_path());
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
        // Commit each changed injected target once, atomically.
        for edited in &resolved {
            if let Some(bytes) = &edited.image {
                self.commit_edited(target, edited, bytes)?;
                on_written(&edited.path);
                written.push(edited.path.clone());
            }
        }
        // The only site where hook results exist. Each declared producer is
        // seeded not-run so a reader of a skipped producer sees a bound `none`;
        // real results overwrite the seed as producers run.
        let mut results = HookResults::default();
        for spec in &self.result_seed {
            results.seed(spec);
        }
        let mut hooks_run = 0usize;
        let mut hook_reports = Vec::new();
        for (index, planned) in self.hooks.iter().enumerate() {
            // Materialize the hook: a ready one as-is; a deferred one by rendering
            // its result-reading fields, which yields nothing when its `when` is
            // false (it does not run; its result stays the seeded not-run).
            let hook = match planned {
                Planned::Ready(hook) => hook.clone(),
                Planned::AfterHooks(deferred) => {
                    let mut rendered = deferred
                        .render_hooks(&results)
                        .map_err(ApplyError::Deferred)?;
                    let Some(hook) = rendered.pop() else {
                        continue;
                    };
                    // A deferred cwd is only now known.
                    if let Some(cwd) = &hook.cwd
                        && has_symlink_component(target.as_path(), cwd).map_err(|source| {
                            ApplyError::Io {
                                path: target.as_path().join(cwd.as_path()),
                                source,
                            }
                        })?
                    {
                        return Err(ApplyError::Symlink(cwd.clone()));
                    }
                    hook
                }
            };
            let outcome =
                runner
                    .run(&hook, target.as_path())
                    .map_err(|source| ApplyError::HookIo {
                        index,
                        hook: hook.clone(),
                        source,
                    })?;
            hooks_run += 1;
            // Failure is checked before any decode or parse: an untolerated
            // nonzero exit or a signal stops the apply and nothing is decoded, so
            // no captured bytes reach an error. `allow-failure` tolerates a
            // nonzero exit code only; a signal (no code) stays fatal.
            let tolerated = hook.allow_failure && outcome.code.is_some();
            if !outcome.success && !tolerated {
                return Err(ApplyError::Hook {
                    index,
                    hook,
                    outcome,
                });
            }
            hook_reports.push(HookReport {
                id: hook.id.clone(),
                exit_code: outcome.code,
                success: outcome.success,
            });
            if hook.id.is_some() {
                record_result(&mut results, &hook, &outcome, index)?;
            }
        }
        let after_apply = match self.after_apply {
            None => None,
            Some(Planned::Ready(text)) => Some(text),
            Some(Planned::AfterHooks(deferred)) => deferred
                .render_message(&results)
                .map_err(ApplyError::Deferred)?,
        };
        Ok(Applied::Written {
            files: written,
            hooks_run,
            hooks: hook_reports,
            after_apply,
        })
    }

    /// Resolves every injected target in memory. Each target starts from its
    /// whole-file image when a `files:` rule supplies one, otherwise from the
    /// current bytes read once; its edits fold in source order. Returns one entry
    /// per target, or an error that leaves every file unchanged.
    #[allow(clippy::result_large_err)]
    fn resolve_edits(
        &self,
        target: &CanonicalTarget,
        force: bool,
    ) -> Result<Vec<EditedTarget>, ApplyError> {
        let mut resolved: Vec<EditedTarget> = Vec::new();
        let mut drift: Vec<(TargetPath, RegionKey)> = Vec::new();
        for edit in &self.edits {
            let path = edit.path();
            if resolved.iter().any(|e| &e.path == path) {
                continue;
            }
            let whole_file = self.files.iter().position(|f| &f.path == path);
            let mut buffer = match whole_file {
                Some(index) => Some(self.file_image(index)?),
                None => read_current(target, path)?,
            };
            // A whole-file rule always writes; a pure injection writes only when
            // an edit changes the target.
            let mut changed = whole_file.is_some();
            for edit in self.edits.iter().filter(|e| e.path() == path) {
                let resolution = match edit {
                    PlannedEdit::Region(region) => resolve_region_edit(buffer.as_deref(), region)?,
                    PlannedEdit::JsonValue(json) => resolve_json_edit(buffer.as_deref(), json)?,
                };
                match resolution {
                    EditResolution::Unchanged => {}
                    EditResolution::Write(bytes) => {
                        buffer = Some(bytes);
                        changed = true;
                    }
                    EditResolution::Drift { forced } => {
                        if force {
                            buffer = Some(forced);
                            changed = true;
                        } else if let PlannedEdit::Region(region) = edit {
                            drift.push((path.clone(), region.region.clone()));
                        }
                    }
                }
            }
            resolved.push(EditedTarget {
                path: path.clone(),
                image: changed.then(|| buffer.unwrap_or_default()),
                whole_file,
            });
        }
        if !drift.is_empty() {
            return Err(ApplyError::Drift(drift));
        }
        Ok(resolved)
    }

    /// The starting bytes a whole-file rule supplies for an injected target.
    #[allow(clippy::result_large_err)]
    fn file_image(&self, index: usize) -> Result<Vec<u8>, ApplyError> {
        match &self.files[index].content {
            Content::Rendered(text) => Ok(text.clone().into_bytes()),
            Content::Copied(source) => fs::read(source).map_err(|error| ApplyError::Io {
                path: source.clone(),
                source: error,
            }),
        }
    }

    /// Atomically replaces one injected target with its resolved image, writing a
    /// sibling temporary file and renaming it into place. Preserves the existing
    /// mode, adding the whole-file source's executable bit when one applies.
    #[allow(clippy::result_large_err)]
    fn commit_edited(
        &self,
        target: &CanonicalTarget,
        edited: &EditedTarget,
        bytes: &[u8],
    ) -> Result<(), ApplyError> {
        let path = target.as_path().join(edited.path.as_path());
        let parent = path
            .parent()
            .expect("a joined target path has a parent")
            .to_owned();
        fs::create_dir_all(&parent).map_err(|source| ApplyError::Io {
            path: parent.clone(),
            source,
        })?;
        let io = |source| ApplyError::Io {
            path: path.clone(),
            source,
        };
        let mut temp = tempfile::NamedTempFile::new_in(&parent).map_err(io)?;
        temp.write_all(bytes).map_err(io)?;
        temp.flush().map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = fs::metadata(&path)
                .map(|meta| meta.permissions().mode() & 0o7777)
                .unwrap_or(0o644);
            if let Some(index) = edited.whole_file {
                let exec = fs::metadata(&self.files[index].source)
                    .map_err(|source| ApplyError::Io {
                        path: self.files[index].source.clone(),
                        source,
                    })?
                    .permissions()
                    .mode()
                    & 0o111;
                mode |= exec;
            }
            let mut permissions = temp.as_file().metadata().map_err(io)?.permissions();
            permissions.set_mode(mode);
            temp.as_file().set_permissions(permissions).map_err(io)?;
        }
        temp.persist(&path).map_err(|error| ApplyError::Io {
            path: path.clone(),
            source: error.error,
        })?;
        Ok(())
    }
}

/// Reads a target's current bytes, or `None` when it does not yet exist.
#[allow(clippy::result_large_err)]
fn read_current(
    target: &CanonicalTarget,
    path: &TargetPath,
) -> Result<Option<Vec<u8>>, ApplyError> {
    let full = target.as_path().join(path.as_path());
    match fs::read(&full) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ApplyError::Io { path: full, source }),
    }
}

/// Strict-UTF-8 decodes one captured stream, mapping a non-UTF-8 stream to a
/// byte-free [`OutputFault::NotUtf8`] naming the hook and the fault position.
#[allow(clippy::result_large_err)]
fn decode_stream(
    bytes: &Option<Vec<u8>>,
    index: usize,
    id: &Id,
) -> Result<Option<String>, ApplyError> {
    match bytes {
        None => Ok(None),
        Some(bytes) => {
            decode(bytes.clone())
                .map(Some)
                .map_err(|valid_up_to| ApplyError::HookOutput {
                    index,
                    id: id.clone(),
                    fault: OutputFault::NotUtf8 { valid_up_to },
                })
        }
    }
}

/// Records one producer's result into the per-apply table. Runs only after the
/// failure gate, so decode and parse never see an untolerated failure. Strict
/// UTF-8 decode precedes JSON parse; on a tolerated nonzero exit an unparseable
/// stdout is lenient (the value is `none`), while on exit 0 it is fatal.
#[allow(clippy::result_large_err)]
fn record_result(
    results: &mut HookResults,
    hook: &PlannedHook,
    outcome: &HookOutcome,
    index: usize,
) -> Result<(), ApplyError> {
    let id = hook.id.clone().expect("recorded hook has an id");
    let stdout = decode_stream(&outcome.stdout, index, &id)?;
    let stderr = decode_stream(&outcome.stderr, index, &id)?;
    if !hook.parse_json {
        results.record_text(&id, outcome.code, stdout, stderr);
        return Ok(());
    }
    let spec = crate::hook::ResultSpec {
        id: id.clone(),
        json: true,
        status_id: hook.status_id.clone(),
    };
    // `parse: json` requires `capture: [stdout]`, so stdout is always captured.
    let raw = stdout.clone().unwrap_or_default();
    match parse_json(&raw) {
        Ok(value) => results.record_json(&spec, outcome.code, stdout, stderr, Some(value), true),
        Err(fault) => {
            if outcome.success {
                // Exit 0: empty or malformed stdout is fatal, byte-free.
                return Err(ApplyError::HookOutput { index, id, fault });
            }
            // Tolerated nonzero: lenient. `<id>` is none, `parsed` is false, and
            // the raw text remains in `status_id.stdout`.
            results.record_json(&spec, outcome.code, stdout, stderr, None, false);
        }
    }
    Ok(())
}

/// Parses a decoded stdout as one JSON value, mapping the failure to a byte-free
/// fault. An empty stdout is [`OutputFault::Empty`]; any other parse failure is
/// [`OutputFault::NotJson`] with only a position.
fn parse_json(text: &str) -> Result<serde_json::Value, OutputFault> {
    if text.is_empty() {
        return Err(OutputFault::Empty);
    }
    serde_json::from_str(text).map_err(|e| OutputFault::NotJson {
        line: e.line(),
        column: e.column(),
    })
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
            edits: vec![],
            conflicts: vec![],
            hooks: (0..hooks)
                .map(|index| {
                    Planned::Ready(PlannedHook {
                        program: PlannedProgram::Run(vec!["tool".into(), index.to_string()]),
                        cwd: None,
                        template_root: source.parent().unwrap().to_owned(),
                        id: None,
                        capture: Default::default(),
                        allow_failure: false,
                        parse_json: false,
                        status_id: None,
                    })
                })
                .collect(),
            before_apply: None,
            after_apply: None,
            result_seed: vec![],
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
                stdout: None,
                stderr: None,
            })
        }
    }

    #[allow(clippy::result_large_err)]
    fn apply(
        plan: Plan,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        events: &Rc<RefCell<Vec<String>>>,
    ) -> Result<Applied, ApplyError> {
        let target = crate::staging::canonical_target(target).unwrap();
        plan.apply_reporting(&target, options, runner, &mut |path| {
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
