// ---
// relationships:
//   implements: architecture
// ---
//! Git-based project snapshots and template updates.
//!
//! A snapshot is the stored output of one `apply`, kept as a parentless git
//! commit under `refs/toha/snapshots/<id>`. The module reads a snapshot at one
//! boundary and refuses a malformed or forged one before any render (see
//! [`record`]). Later slices add the repository access, capture, merge, and
//! replay seams named in the design.
mod capture;
mod merge;
mod project;
mod record;
mod replay;

pub use capture::CaptureInputs;
pub use merge::{
    Action, Base, Change, ConflictKind, MergeError, MergeOptions, Merged, SnapshotInputs,
    merge_apply,
};
pub use project::{
    Cleanliness, FetchSetting, LikelyBase, LikelyBaseBy, Listed, Project, ProjectError, Removed,
    SkipReason, SnapshotOutcome,
};
pub use record::{
    CommitId, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision, Snapshot,
    SnapshotDoc, SnapshotError, SnapshotId, Timestamp,
};
pub use replay::{UpdateDrive, drive_update, drive_update_resume};
