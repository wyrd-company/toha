// ---
// relationships:
//   implements: architecture
// ---
//! Repository access for snapshots.
//!
//! [`Project`] opens the target's repository once through gitoxide and, before
//! any status, checkout, conversion, or merge, removes every configured merge
//! and filter driver from the in-memory configuration so no driver program can
//! start (see [`strip_drivers`]). It changes nothing on disk. Built-in
//! conversions still apply, so a path stored through a filter is compared and
//! written in its stored form.

use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::EntryKind;

use crate::snapshot::record::{RepoPath, SNAPSHOT_REF_PREFIX, Snapshot, SnapshotError, SnapshotId};
use crate::staging::CanonicalTarget;
use std::collections::BTreeSet;

/// A target directory inside a git repository, opened with driver programs
/// disabled in memory.
pub struct Project {
    repo: gix::Repository,
    /// The target directory relative to the repository root.
    rel: RepoPath,
}

/// Whether a target is clean, or the repository-relative paths that are not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cleanliness {
    Clean,
    Dirty { paths: Vec<RepoPath> },
}

impl Project {
    /// Open the repository that contains `target`, or `Ok(None)` when the target
    /// is not inside a git repository. The returned project has every configured
    /// driver removed from its in-memory config.
    pub fn open(target: &CanonicalTarget) -> Result<Option<Project>, ProjectError> {
        let mut repo = match gix::discover(target.as_path()) {
            Ok(repo) => repo,
            Err(gix::discover::Error::Discover(err)) if is_not_a_repository(&err) => {
                return Ok(None);
            }
            Err(err) => return Err(ProjectError::Open(err.to_string())),
        };
        strip_drivers(&mut repo)?;

        let workdir = repo
            .workdir()
            .ok_or(ProjectError::Bare)?
            .to_owned()
            .canonicalize()
            .map_err(|err| ProjectError::Io(err.to_string()))?;
        let rel = match target.as_path().strip_prefix(&workdir) {
            Ok(suffix) if suffix.as_os_str().is_empty() => RepoPath::root(),
            Ok(suffix) => RepoPath::parse(&suffix.to_string_lossy())
                .map_err(|err| ProjectError::Path(err.to_string()))?,
            Err(_) => return Err(ProjectError::Outside(target.to_string())),
        };
        Ok(Some(Project { repo, rel }))
    }

    /// The target directory relative to the repository root.
    pub fn target(&self) -> &RepoPath {
        &self.rel
    }

    /// Whether the target directory has no tracked modification, no staged
    /// change, and no untracked file git does not ignore. Dirt outside the
    /// target does not count.
    pub fn cleanliness(&self) -> Result<Cleanliness, ProjectError> {
        let patterns: Vec<BString> = if self.rel.is_root() {
            Vec::new()
        } else {
            vec![BString::from(format!(":(top){}/", self.rel))]
        };
        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|err| ProjectError::Status(err.to_string()))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(None)
            .into_iter(patterns)
            .map_err(|err| ProjectError::Status(err.to_string()))?;

        let mut paths = Vec::new();
        for item in iter {
            let item = item.map_err(|err| ProjectError::Status(err.to_string()))?;
            let rela = match &item {
                gix::status::Item::TreeIndex(change) => {
                    change.location().to_str_lossy().into_owned()
                }
                gix::status::Item::IndexWorktree(change) => {
                    change.rela_path().to_str_lossy().into_owned()
                }
            };
            paths.push(RepoPath::parse(&rela).map_err(|err| ProjectError::Path(err.to_string()))?);
        }
        paths.sort();
        paths.dedup();
        Ok(if paths.is_empty() {
            Cleanliness::Clean
        } else {
            Cleanliness::Dirty { paths }
        })
    }

    /// Every snapshot whose `target` is this project's target, newest first, plus
    /// every snapshot ref that cannot be read, listed as invalid with its reason.
    pub fn snapshots(&self) -> Result<Vec<Listed>, ProjectError> {
        let mut listed: Vec<Listed> = self
            .all_snapshots()?
            .into_iter()
            .filter(|entry| match entry {
                Listed::Valid(snapshot) => snapshot.target() == &self.rel,
                Listed::Invalid { .. } => true,
            })
            .collect();
        // Newest first: ULIDs sort by creation time, invalids after valids.
        listed.sort_by(|a, b| match (a, b) {
            (Listed::Valid(x), Listed::Valid(y)) => y.id().cmp(x.id()),
            (Listed::Valid(_), Listed::Invalid { .. }) => std::cmp::Ordering::Less,
            (Listed::Invalid { .. }, Listed::Valid(_)) => std::cmp::Ordering::Greater,
            (Listed::Invalid { r#ref: x, .. }, Listed::Invalid { r#ref: y, .. }) => x.cmp(y),
        });
        Ok(listed)
    }

    /// Resolve a snapshot from any unique prefix of at least six characters,
    /// across every valid snapshot in the repository regardless of its target so
    /// that a snapshot for another target can still be named and refused later.
    pub fn find(&self, prefix: &str) -> Result<Snapshot, SnapshotError> {
        let normalised = SnapshotId::parse_prefix(prefix)?;
        let mut matches: Vec<Snapshot> = self
            .all_snapshots()
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .into_iter()
            .filter_map(|entry| match entry {
                Listed::Valid(snapshot) if snapshot.id().has_prefix(&normalised) => Some(snapshot),
                _ => None,
            })
            .collect();
        match matches.len() {
            0 => Err(SnapshotError::Unknown(prefix.to_owned())),
            1 => Ok(matches.pop().expect("one match")),
            _ => Err(SnapshotError::Ambiguous {
                prefix: prefix.to_owned(),
                matches: matches.iter().map(|s| s.id().to_string()).collect(),
            }),
        }
    }

    /// Read every ref under the snapshot namespace, valid or not.
    fn all_snapshots(&self) -> Result<Vec<Listed>, ProjectError> {
        let platform = self
            .repo
            .references()
            .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
        let iter = platform
            .prefixed(SNAPSHOT_REF_PREFIX)
            .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
        let mut out = Vec::new();
        for reference in iter {
            let mut reference = reference
                .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
            let full = reference.name().as_bstr().to_str_lossy().into_owned();
            let id_segment = full.strip_prefix(SNAPSHOT_REF_PREFIX).unwrap_or(&full);
            let id = match id_segment.parse::<SnapshotId>() {
                Ok(id) => id,
                Err(err) => {
                    out.push(Listed::Invalid {
                        r#ref: full,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            match self.read_snapshot(&mut reference, id) {
                Ok(snapshot) => out.push(Listed::Valid(snapshot)),
                Err(err) => out.push(Listed::Invalid {
                    r#ref: full,
                    reason: err.to_string(),
                }),
            }
        }
        Ok(out)
    }

    /// Read and validate one snapshot ref, applying the git-level checks the
    /// document boundary cannot: the commit is parentless, its tree holds exactly
    /// `snapshot.json` (a regular blob) and, unless empty, `files/` (a tree), and
    /// every entry under `files/` is a regular file, an executable, or a symlink
    /// with no gitlink and nothing outside `files/`.
    fn read_snapshot(
        &self,
        reference: &mut gix::Reference<'_>,
        id: SnapshotId,
    ) -> Result<Snapshot, SnapshotError> {
        let commit = reference
            .peel_to_commit()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        if commit.parent_ids().next().is_some() {
            return Err(SnapshotError::HasParent);
        }
        let tree = commit
            .tree()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;

        let mut snapshot_json = None;
        let mut files_tree = None;
        for entry in tree.iter() {
            let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
            let name = entry.filename().to_str_lossy().into_owned();
            match name.as_str() {
                "snapshot.json" => {
                    if entry.mode().kind() != EntryKind::Blob {
                        return Err(SnapshotError::SnapshotJsonNotBlob);
                    }
                    snapshot_json = Some(entry.oid().to_owned());
                }
                "files" => {
                    if entry.mode().kind() != EntryKind::Tree {
                        return Err(SnapshotError::FilesNotTree);
                    }
                    files_tree = Some(entry.oid().to_owned());
                }
                _ => return Err(SnapshotError::ExtraTreeEntry(name)),
            }
        }

        let json_oid = snapshot_json.ok_or(SnapshotError::MissingSnapshotJson)?;
        let mut files = BTreeSet::new();
        if let Some(tree_oid) = files_tree {
            self.collect_files(tree_oid, "", &mut files)?;
        }
        let bytes = self
            .repo
            .find_object(json_oid)
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .data
            .clone();
        Snapshot::validate(id, &bytes, &files)
    }

    /// Collect the leaf paths under a `files/` tree, refusing a gitlink.
    fn collect_files(
        &self,
        tree_oid: gix::ObjectId,
        prefix: &str,
        files: &mut BTreeSet<String>,
    ) -> Result<(), SnapshotError> {
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        for entry in tree.iter() {
            let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
            let name = entry.filename().to_str_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match entry.mode().kind() {
                EntryKind::Tree => self.collect_files(entry.oid().to_owned(), &path, files)?,
                EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                    files.insert(path);
                }
                EntryKind::Commit => return Err(SnapshotError::BadFileEntry(path)),
            }
        }
        Ok(())
    }
}

/// One entry from [`Project::snapshots`]: a valid snapshot, or a ref that could
/// not be read together with the reason.
///
/// A valid snapshot carries its whole validated document, so it is much larger
/// than the invalid variant; the size gap is intended, as the common list holds
/// valid entries.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Listed {
    Valid(Snapshot),
    Invalid { r#ref: String, reason: String },
}

/// Remove every named `filter.<name>` and `merge.<name>` driver subsection from
/// the repository's in-memory configuration.
///
/// Removing the whole named subsection (not merely the driver value) is what
/// makes gitoxide fall back to its built-in behaviour: a leftover empty command
/// would make a tree merge try to spawn it and fail. Top-level `merge.*`/
/// `filter.*` keys and every built-in conversion stay in place, and nothing on
/// disk changes.
pub(crate) fn strip_drivers(repo: &mut gix::Repository) -> Result<(), ProjectError> {
    let mut config = repo.config_snapshot_mut();
    for section in ["filter", "merge"] {
        let ids: Vec<_> = match config.sections_and_ids_by_name(section) {
            Some(sections) => sections
                .filter(|(section, _)| section.header().subsection_name().is_some())
                .map(|(_, id)| id)
                .collect(),
            None => Vec::new(),
        };
        for id in ids {
            config.remove_section_by_id(id);
        }
    }
    config
        .commit()
        .map_err(|err| ProjectError::Config(err.to_string()))?;
    Ok(())
}

/// Whether a discovery error means there simply is no repository, as opposed to
/// a real fault opening one that exists.
fn is_not_a_repository(err: &gix::discover::upwards::Error) -> bool {
    use gix::discover::upwards::Error;
    matches!(
        err,
        Error::NoGitRepository { .. }
            | Error::NoGitRepositoryWithinFs { .. }
            | Error::NoGitRepositoryWithinCeiling { .. }
    )
}

/// A failure to open or inspect the repository. Every variant names a path or a
/// diagnostic and never an answer value or a file's content.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("cannot open the repository: {0}")]
    Open(String),
    #[error("the repository has no working tree")]
    Bare,
    #[error("the target is outside the repository working tree: {0}")]
    Outside(String),
    #[error("cannot read repository status: {0}")]
    Status(String),
    #[error("cannot adjust in-memory git configuration: {0}")]
    Config(String),
    #[error("invalid repository path: {0}")]
    Path(String),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
}

#[cfg(test)]
mod tests;
