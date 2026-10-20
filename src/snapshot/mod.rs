// ---
// relationships:
//   implements: applied-record
// ---
//! Reader and management operations for legacy snapshot refs. Apply and update
//! persist applied records and do not create or migrate legacy snapshots.
mod project;
mod record;
pub use project::{
    Cleanliness, FetchSetting, LikelyBase, LikelyBaseBy, Listed, Project, ProjectError, Removed,
};
pub use record::{
    CommitId, ContentDigest, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision,
    Snapshot, SnapshotDoc, SnapshotError, SnapshotId, Timestamp, source_identity,
};
