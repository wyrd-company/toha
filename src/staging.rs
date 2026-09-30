// ---
// relationships:
//   implements: architecture
// ---
use crate::context::{
    EnvironmentAdmissionError, InvocationContext, InvocationContextWire, RenderOrigin,
};
use crate::interview::Resolution;
use crate::{AnswerError, Interview, RawAnswer, RawAnswers, Seed, Template};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedRecord {
    target: PathBuf,
    pub template: String,
    pub commit: String,
    #[serde(default)]
    pub named: bool,
    pub now: String,
    pub submissions: Vec<IndexMap<String, Value>>,
    /// The base snapshot id an update stages from, absent for a plain apply. When
    /// present, the record resumes through the update replay adapter and applies
    /// by merging from the base, not a fresh apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Whether the staged update re-asks every recorded answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reanswer: bool,
    /// The versioned invocation context. A record written by [`StagedRecord::new`]
    /// carries the legacy projection; [`StagedRecord::new_with_context`] carries
    /// the current context. A pre-context record on disk has no field and
    /// deserializes as legacy.
    #[serde(default, skip_serializing_if = "InvocationContextWire::is_legacy")]
    context: InvocationContextWire,
}

#[derive(Debug, thiserror::Error)]
pub enum StagingError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Replay(String),
    /// A stage need was analyzed but stage trust was not granted. The stage
    /// adapter maps only this admission fault; it carries the authored location
    /// and no captured value.
    #[error("environment access at {reference} requires stage trust")]
    EnvironmentTrustRequired { reference: RenderOrigin },
    /// Any other environment-admission fault, retaining its typed source.
    #[error("{source}")]
    EnvironmentAdmission {
        #[source]
        source: EnvironmentAdmissionError,
    },
}

/// Maps an environment-admission fault to the stage command's typed error. Only
/// a trust refusal becomes [`StagingError::EnvironmentTrustRequired`]; every
/// other fault keeps its typed source under [`StagingError::EnvironmentAdmission`].
pub fn stage_admission_error(error: EnvironmentAdmissionError) -> StagingError {
    match error {
        EnvironmentAdmissionError::TrustRequired { reference } => {
            StagingError::EnvironmentTrustRequired { reference }
        }
    }
}

/// A target identity produced only by [`canonical_target`].
///
/// Raw paths cannot enter identity-sensitive consumers:
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::staging::{StagedRecord, Store};
/// let raw = PathBuf::from("output");
/// let store: Store = unimplemented!();
/// let record: StagedRecord = unimplemented!();
/// store.save(&raw, &record);
/// ```
///
/// The abort action's removal key is a `CanonicalTarget`, never a raw path:
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::staging::Store;
/// let raw = PathBuf::from("output");
/// let store: Store = unimplemented!();
/// store.remove(&raw);
/// ```
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::staging::StagedRecord;
/// let raw = PathBuf::from("output");
/// StagedRecord::new(
///     &raw,
///     "sample".into(),
///     String::new(),
///     false,
///     "2026-01-02T03:04:05+00:00[UTC]".into(),
///     vec![],
/// );
/// ```
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::staging::StagedRecord;
/// let raw = PathBuf::from("output");
/// let _ = StagedRecord {
///     target: raw,
///     template: "sample".into(),
///     commit: String::new(),
///     named: false,
///     now: "2026-01-02T03:04:05+00:00[UTC]".into(),
///     submissions: vec![],
/// };
/// ```
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::{Completed, Plan, Template};
/// let raw = PathBuf::from("output");
/// let template: Template = unimplemented!();
/// let completed: Completed = unimplemented!();
/// Plan::build(&template, &completed, &raw);
/// ```
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::protocol::Context;
/// use toha::staging::StagedRecord;
/// let raw = PathBuf::from("output");
/// let record: StagedRecord = unimplemented!();
/// Context::new(&raw, &record);
/// ```
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use toha::{ApplyOptions, Plan};
/// use toha::hook::RecordingRunner;
/// let raw = PathBuf::from("output");
/// let plan: Plan = unimplemented!();
/// plan.apply(&raw, ApplyOptions::default(), &RecordingRunner::default());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalTarget(PathBuf);

impl CanonicalTarget {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for CanonicalTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component)
            }
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
        }
    }
    let mut ancestor = normalized.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| StagingError::Replay("target has no existing ancestor".into()))?;
    }
    let suffix = normalized
        .strip_prefix(ancestor)
        .expect("ancestor is a prefix");
    let mut canonical = ancestor.canonicalize()?;
    if !suffix.as_os_str().is_empty() {
        canonical.push(suffix);
    }
    Ok(CanonicalTarget(canonical))
}

pub struct Store {
    dir: PathBuf,
}
impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    pub fn path_for(&self, target: &CanonicalTarget) -> PathBuf {
        let digest = Sha256::digest(target.as_path().to_string_lossy().as_bytes());
        self.dir.join(format!("{digest:x}.json"))
    }
    fn legacy_path_for(&self, target: &CanonicalTarget) -> Option<PathBuf> {
        let path = target.as_path();
        path.parent()?;
        let mut legacy = path.as_os_str().to_os_string();
        legacy.push(std::path::MAIN_SEPARATOR_STR);
        let digest = Sha256::digest(Path::new(&legacy).to_string_lossy().as_bytes());
        Some(self.dir.join(format!("{digest:x}.json")))
    }
    pub fn load(&self, target: &CanonicalTarget) -> Result<Option<StagedRecord>, StagingError> {
        let read = |path: PathBuf| -> Result<Option<StagedRecord>, StagingError> {
            match fs::read(path) {
                Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        };
        if let Some(record) = read(self.path_for(target))? {
            if canonical_target(&record.target)? != *target {
                return Err(StagingError::Replay(
                    "staged record target does not match its storage key".into(),
                ));
            }
            return Ok(Some(record));
        }
        let Some(path) = self.legacy_path_for(target) else {
            return Ok(None);
        };
        let Some(record) = read(path)? else {
            return Ok(None);
        };
        if canonical_target(&record.target)? != *target {
            return Err(StagingError::Replay(
                "staged record target does not match its storage key".into(),
            ));
        }
        Ok(Some(record))
    }
    pub fn save(
        &self,
        target: &CanonicalTarget,
        record: &StagedRecord,
    ) -> Result<(), StagingError> {
        if canonical_target(&record.target)? != *target {
            return Err(StagingError::Replay(
                "staged record target does not match the active target".into(),
            ));
        }
        fs::create_dir_all(&self.dir)?;
        let canonical_path = self.path_for(target);
        let mut stored = record.clone();
        stored.target = target.as_path().to_owned();
        let bytes = serde_json::to_vec_pretty(&stored)?;
        let (temp, mut file) = loop {
            let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let temp = self
                .dir
                .join(format!(".toha-{}-{n}.tmp", std::process::id()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
            {
                Ok(file) => break (temp, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &canonical_path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(StagingError::Io)?;
        if let Some(legacy) = self
            .legacy_path_for(target)
            .filter(|p| p != &canonical_path)
        {
            match fs::remove_file(legacy) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    pub fn remove(&self, target: &CanonicalTarget) -> Result<bool, StagingError> {
        let canonical = self.path_for(target);
        let legacy = self
            .legacy_path_for(target)
            .filter(|path| path != &canonical);
        let remove = |path: PathBuf| -> Result<bool, StagingError> {
            match fs::remove_file(path) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e.into()),
            }
        };
        let canonical_removed = remove(canonical)?;
        let legacy_removed = match legacy {
            Some(path) => remove(path)?,
            None => false,
        };
        Ok(canonical_removed || legacy_removed)
    }
}

impl StagedRecord {
    /// Creates a legacy-projection record. Retained for existing crate callers;
    /// current invocations use [`Self::new_with_context`].
    pub fn new(
        target: &CanonicalTarget,
        template: String,
        commit: String,
        named: bool,
        now: String,
        submissions: Vec<IndexMap<String, Value>>,
    ) -> Self {
        Self {
            target: target.as_path().to_owned(),
            template,
            commit,
            named,
            now,
            submissions,
            base: None,
            reanswer: false,
            context: InvocationContextWire::Legacy,
        }
    }

    /// Creates a current record from a completed invocation context. The stored
    /// target and formal name are derived from the context; the versioned wire
    /// carries identity, host, execution, and the environment snapshot.
    pub fn new_with_context(
        context: InvocationContext,
        commit: String,
        named: bool,
        now: String,
        submissions: Vec<IndexMap<String, Value>>,
    ) -> Self {
        let (target, formal_name, wire) = context.staged_parts();
        Self {
            target: target.as_path().to_owned(),
            template: formal_name.unwrap_or_default(),
            commit,
            named,
            now,
            submissions,
            base: None,
            reanswer: false,
            context: wire,
        }
    }

    /// Restores the invocation context from the versioned wire, validating the
    /// persisted target text against the producer-created carrier. A pre-context
    /// record restores a legacy context.
    pub(crate) fn invocation_context(
        &self,
        target: &CanonicalTarget,
    ) -> Result<InvocationContext, StagingError> {
        if canonical_target(&self.target)? != *target {
            return Err(StagingError::Replay(
                "staged record target does not match its storage key".into(),
            ));
        }
        Ok(InvocationContext::from_wire(
            target.clone(),
            &self.template,
            self.context.clone(),
        ))
    }

    pub fn replay<'a>(
        &self,
        template: &'a Template,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError> {
        self.replay_with_defaults(template, IndexMap::new(), target)
    }
    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: IndexMap<crate::Id, RawAnswer>,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError> {
        let now = self
            .now
            .parse()
            .map_err(|e: jiff::Error| StagingError::Replay(e.to_string()))?;
        let context = self.invocation_context(target)?;
        let interview = Interview::start(
            template,
            Seed {
                now,
                defaults,
                context,
            },
        )
        .map_err(|e| StagingError::Replay(e.to_string()))?;
        self.replay_from(interview)
    }

    pub fn replay_with_resolution<'a>(
        &self,
        template: &'a Template,
        resolution: Resolution,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError> {
        let now = self
            .now
            .parse()
            .map_err(|e: jiff::Error| StagingError::Replay(e.to_string()))?;
        let context = self.invocation_context(target)?;
        let interview = resolution
            .start_with_context(template, now, context)
            .map_err(|e| StagingError::Replay(e.to_string()))?;
        self.replay_from(interview)
    }

    fn replay_from<'a>(&self, mut interview: Interview<'a>) -> Result<Interview<'a>, StagingError> {
        for submission in &self.submissions {
            let Interview::Asking(pending) = interview else {
                return Err(StagingError::Replay(
                    "submission after completed interview".into(),
                ));
            };
            let raw: RawAnswers = submission
                .iter()
                .map(|(key, value)| crate::Id::parse(key).map(|id| (id, RawAnswer(value.clone()))))
                .collect::<Result<_, _>>()
                .map_err(StagingError::Replay)?;
            interview = pending.answer(raw).map_err(|e| match e {
                AnswerError::Rejected { rejections, .. } => StagingError::Replay(
                    rejections
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                AnswerError::Eval(e) => StagingError::Replay(e.to_string()),
            })?;
        }
        Ok(interview)
    }
}
