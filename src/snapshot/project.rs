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

use crate::snapshot::record::RepoPath;
use crate::staging::CanonicalTarget;

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
}

#[cfg(test)]
mod tests;
