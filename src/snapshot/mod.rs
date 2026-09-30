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
// TEMPORARY: the merge transaction core is tested but its public consumer
// `merge_apply` (checkout + capture-with-base orchestration) lands in the next
// commit, which removes this allow. Recorded on task 1029.
#[allow(dead_code)]
mod merge;
mod project;
mod record;

pub use capture::CaptureInputs;
pub use merge::{Action, Base, Change, ConflictKind, MergeError, MergeOptions, Merged};
pub use project::{
    Cleanliness, FetchSetting, LikelyBase, LikelyBaseBy, Listed, Project, ProjectError, Removed,
};
pub use record::{
    CommitId, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision, Snapshot,
    SnapshotDoc, SnapshotError, SnapshotId, Timestamp,
};
