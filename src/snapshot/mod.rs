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
mod record;

pub use record::{
    CommitId, FrozenNow, Origin, PathOwnership, ProjectPoint, Revision, Snapshot, SnapshotDoc,
    SnapshotError, SnapshotId, Timestamp,
};
