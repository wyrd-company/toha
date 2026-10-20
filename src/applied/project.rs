// ---
// relationships:
//   implements: architecture
// ---
//! Repository access for applications.
//!
//! [`Project`] opens the target's repository once through gitoxide and, before
//! any status, checkout, conversion, or merge, removes every configured merge
//! and filter driver from the in-memory configuration so no driver program can
//! start (see [`strip_drivers`]). It changes nothing on disk. Built-in
//! conversions still apply, so a path stored through a filter is compared and
//! written in its stored form.

use gix::bstr::{BString, ByteSlice};

use crate::applied::record::{Application, AppliedRecord, RecordError, RepoPath};
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
    ///
    /// Discovery starts from the nearest ancestor of `target` that exists on
    /// disk, so a target subdirectory an `apply` has not created yet still
    /// resolves its repository (an approved clean-git generate into a fresh
    /// subpath, and its repository-wide application reader, both open the project
    /// before the subpath exists). The target-relative identity, path validation,
    /// and repository-containment bound are unchanged: `rel` is still computed
    /// from `target` itself against the canonical working tree, and a target
    /// outside the working tree is still refused. For a target that already
    /// exists, the nearest existing ancestor is the target, so discovery is
    /// byte-for-byte as before.
    pub fn open(target: &CanonicalTarget) -> Result<Option<Project>, ProjectError> {
        let discover_from = nearest_existing_ancestor(target.as_path());
        let mut repo = match gix::discover(&discover_from) {
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
            Ok(suffix) => {
                // Git records slash-separated components on every host. Convert
                // the OS path at this boundary before validating the git path.
                let git_path = suffix
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                RepoPath::parse(&git_path).map_err(|err| ProjectError::Path(err.to_string()))?
            }
            Err(_) => return Err(ProjectError::Outside(target.to_string())),
        };
        Ok(Some(Project { repo, rel }))
    }

    /// The target directory relative to the repository root.
    pub fn target(&self) -> &RepoPath {
        &self.rel
    }

    /// The opened repository, with drivers already stripped. Crate-internal seams
    /// (capture, merge) read and write git objects through it.
    pub(crate) fn repo(&self) -> &gix::Repository {
        &self.repo
    }

    /// Whether the target directory has no tracked modification, no staged
    /// change, and no untracked file git does not ignore. Dirt outside the
    /// target does not count.
    pub fn cleanliness(&self) -> Result<Cleanliness, ProjectError> {
        let patterns: Vec<BString> = if self.rel.is_root() {
            Vec::new()
        } else {
            vec![
                BString::from(format!(":(top){}/", self.rel)),
                BString::from(format!(":(top){}", self.record_relative()?)),
            ]
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

    /// Resolve and validate the reserved record path before interview or planning.
    pub fn record_relative(&self) -> Result<String, ProjectError> {
        record_relative(self.repo.workdir().ok_or(ProjectError::Bare)?)
    }
    pub fn record_path(&self) -> Result<std::path::PathBuf, ProjectError> {
        Ok(self
            .repo
            .workdir()
            .ok_or(ProjectError::Bare)?
            .join(self.record_relative()?))
    }
    pub fn read_record(&self) -> Result<AppliedRecord, ProjectError> {
        let path = self.record_path()?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let record = AppliedRecord::parse(&bytes)
                    .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
                let reserved = self.record_relative()?;
                for app in record.applications() {
                    for owned in app.paths() {
                        let full = if app.target().is_root() {
                            owned.path().to_string()
                        } else {
                            format!("{}/{}", app.target(), owned.path())
                        };
                        if full == reserved {
                            return Err(ProjectError::Io(format!(
                                "{}: the applied record cannot own itself",
                                path.display()
                            )));
                        }
                    }
                }
                Ok(record)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppliedRecord::default()),
            Err(e) => Err(ProjectError::Io(format!("{}: {e}", path.display()))),
        }
    }
    pub fn applications(&self) -> Result<Vec<Application>, ProjectError> {
        Ok(self.read_record()?.applications().to_vec())
    }
    pub fn restore_base(
        &self,
        application: &mut Application,
        plan: crate::plan::Plan,
    ) -> Result<(), ProjectError> {
        super::capture::restore_base(self, application, plan).map_err(ProjectError::Record)
    }
    pub fn application(&self, source: &str) -> Result<Application, ProjectError> {
        self.applications()?
            .into_iter()
            .find(|a| a.target() == &self.rel && a.source() == source)
            .ok_or_else(|| ProjectError::Record(RecordError::Unknown(source.to_owned())))
    }
    pub fn record_bytes_with(&self, application: Application) -> Result<Vec<u8>, ProjectError> {
        let mut record = self.read_record()?;
        record.replace(application);
        record.to_json_bytes().map_err(|e| {
            ProjectError::Io(format!(
                "{}: {e}",
                self.record_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".toha.json".into())
            ))
        })
    }
    pub fn write_application(&self, application: Application) -> Result<(), ProjectError> {
        let bytes = self.record_bytes_with(application)?;
        let path = self.record_path()?;
        let parent = path.parent().expect("record has a parent");
        std::fs::create_dir_all(parent)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
        use std::io::Write;
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o666));
        }
        let mut temp = builder
            .tempfile_in(parent)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
        temp.write_all(&bytes)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
        temp.persist(&path)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
        Ok(())
    }
    pub fn ensure_record_clean(&self) -> Result<(), ProjectError> {
        let relative = self.record_relative()?;
        let mut changes = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|e| ProjectError::Status(e.to_string()))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(None)
            .into_iter(vec![BString::from(format!(":(top){relative}"))])
            .map_err(|e| ProjectError::Status(e.to_string()))?;
        if let Some(change) = changes.next() {
            change.map_err(|e| ProjectError::Status(e.to_string()))?;
            return Err(ProjectError::Io(format!(
                "{} differs from HEAD; commit or discard it before apply --update",
                self.record_path()?.display()
            )));
        }
        Ok(())
    }
}

/// The environment override always uses repository-relative slash-separated paths.
/// Reject symlinks before reads as well as writes.
pub fn record_relative(root: &std::path::Path) -> Result<String, ProjectError> {
    let value = match std::env::var("TOHA_RECORD") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => ".toha.json".to_owned(),
        Err(_) => return Err(ProjectError::Path("TOHA_RECORD is not Unicode".into())),
    };
    if value.split('/').any(|p| p == "..") {
        return Err(ProjectError::Path("TOHA_RECORD must not contain ..".into()));
    }
    let path = crate::plan::TargetPath::parse(&value)
        .map_err(|e| ProjectError::Path(format!("TOHA_RECORD: {e}")))?;
    if crate::plan::has_symlink_component(root, &path)
        .map_err(|e| ProjectError::Io(e.to_string()))?
    {
        return Err(ProjectError::Path(format!(
            "{} must not pass through a symbolic link",
            root.join(path.as_path()).display()
        )));
    }
    Ok(path.to_string())
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

/// The nearest ancestor of `path` (including `path` itself) that exists on disk.
/// A `CanonicalTarget` always has an existing ancestor (its canonicalization
/// walked up to one), so this terminates at or above the working tree, and
/// discovery walks up to the repository from there.
fn nearest_existing_ancestor(path: &std::path::Path) -> std::path::PathBuf {
    let mut current = path;
    loop {
        if current.exists() {
            return current.to_owned();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return current.to_owned(),
        }
    }
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
    #[error("no remote named {0}")]
    RemoteMissing(String),
    #[error(transparent)]
    Record(#[from] RecordError),
}
