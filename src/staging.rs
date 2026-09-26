// ---
// relationships:
//   implements: architecture
// ---
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
    pub target: PathBuf,
    pub template: String,
    pub commit: String,
    #[serde(default)]
    pub named: bool,
    pub now: String,
    pub submissions: Vec<IndexMap<String, Value>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StagingError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Replay(String),
}

pub fn canonical_target(path: &Path) -> Result<PathBuf, StagingError> {
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
    Ok(ancestor.canonicalize()?.join(
        normalized
            .strip_prefix(ancestor)
            .expect("ancestor is a prefix"),
    ))
}

pub struct Store {
    dir: PathBuf,
}
impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    pub fn path_for(&self, target: &Path) -> PathBuf {
        let digest = Sha256::digest(target.to_string_lossy().as_bytes());
        self.dir.join(format!("{digest:x}.json"))
    }
    pub fn load(&self, target: &Path) -> Result<Option<StagedRecord>, StagingError> {
        match fs::read(self.path_for(target)) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, record: &StagedRecord) -> Result<(), StagingError> {
        fs::create_dir_all(&self.dir)?;
        let target = self.path_for(&record.target);
        let bytes = serde_json::to_vec_pretty(record)?;
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
            fs::rename(&temp, target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(Into::into)
    }
    pub fn remove(&self, target: &Path) -> Result<bool, StagingError> {
        match fs::remove_file(self.path_for(target)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

impl StagedRecord {
    pub fn replay<'a>(&self, template: &'a Template) -> Result<Interview<'a>, StagingError> {
        self.replay_with_defaults(template, IndexMap::new())
    }
    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: IndexMap<crate::Id, RawAnswer>,
    ) -> Result<Interview<'a>, StagingError> {
        let now = self
            .now
            .parse()
            .map_err(|e: jiff::Error| StagingError::Replay(e.to_string()))?;
        let mut interview = Interview::start(template, Seed { now, defaults })
            .map_err(|e| StagingError::Replay(e.to_string()))?;
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
