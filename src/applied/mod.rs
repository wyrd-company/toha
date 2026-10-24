// ---
// relationships:
//   implements: architecture
// ---
//! Committed applied records and three-way template updates.
//!
//! The record validates at its JSON boundary. Whole-file and hook ownership
//! retains object ids and modes; base reconstruction verifies rendered bytes
//! against those ids before the merge engine uses them.
pub(crate) mod capture;
mod merge;
pub(crate) mod project;
mod record;
mod replay;

pub use capture::CaptureInputs;
pub use merge::{
    Action, Base, Change, ConflictKind, MergeError, MergeOptions, Merged, RecordInputs,
    UpdateReport, merge_apply, merge_apply_reported,
};
pub use project::{Cleanliness, Project, ProjectError};
pub use record::{
    Application, AppliedRecord, CommitId, ContentDigest, FrozenNow, Origin, PathOwnership,
    RecordError, RepoPath, Revision, SCHEMA, SCHEMA_ID, source_identity,
};
pub use replay::{UpdateDrive, drive_update, drive_update_resume};

/// Whether an apply wrote its repository's record.
#[derive(Debug)]
pub enum RecordOutcome {
    Written(String),
    NotGit,
    Internal,
}

pub fn validate_record_path(root: &std::path::Path) -> Result<String, ProjectError> {
    project::record_relative(root)
}

pub(crate) use replay::covered_answers;
