// ---
// relationships:
//   implements: architecture
// ---
//! Initializes an apply target as a git repository for a template's `git`
//! question, with an optional `origin` remote.
use crate::staging::CanonicalTarget;
use std::path::Path;

/// The branch `HEAD` names when the configured `init.defaultBranch` is unset or
/// is the outdated `master`.
pub const DEFAULT_BRANCH: &str = "main";
/// The name of the remote a non-empty `git` answer adds.
pub const REMOTE_NAME: &str = "origin";

/// The repository a reached `git` question asks the apply to set up. `remote`
/// is `None` for an empty answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedGit {
    pub remote: Option<String>,
}

/// What the apply did for the `git` question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSetup {
    /// The target was not inside a repository; the apply initialized one.
    Initialized {
        branch: String,
        remote: Option<String>,
    },
    /// The target was already inside a repository; the apply left it as it was.
    Existing,
}

/// The apply's report for the `git` question: the setup, and a warning when the
/// remote could not be inspected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitReport {
    pub setup: GitSetup,
    pub warning: Option<String>,
}

impl PlannedGit {
    /// The dry-run line: `git init`, with the remote when one was given.
    pub fn preview_line(&self) -> String {
        match &self.remote {
            None => "git init".to_owned(),
            Some(url) => format!(
                "git init (remote {REMOTE_NAME} {})",
                crate::source::without_userinfo(url)
            ),
        }
    }
}

impl GitReport {
    /// The human lines for an applied report: the warning first, then what the
    /// apply did.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::from_iter(self.warning.clone());
        lines.push(match &self.setup {
            GitSetup::Initialized {
                branch,
                remote: None,
            } => format!("Initialized git repository on branch {branch}"),
            GitSetup::Initialized {
                branch,
                remote: Some(url),
            } => format!(
                "Initialized git repository on branch {branch} with remote {REMOTE_NAME} {}",
                crate::source::without_userinfo(url)
            ),
            GitSetup::Existing => {
                "Target is already inside a git repository; left it unchanged".to_owned()
            }
        });
        lines
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GitTargetError {
    #[error(
        "the remote {url} already has history; clone it first, then apply the template into the clone"
    )]
    RemoteHasHistory { url: String },
    #[error("could not check whether {path} is inside a git repository: {message}")]
    Discover { path: String, message: String },
    #[error("could not initialize a git repository at {path}: {message}")]
    Init { path: String, message: String },
}

/// What listing a remote's references found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteState {
    /// The remote answered and holds no reference.
    Empty,
    /// The remote answered and holds at least one reference.
    HasHistory,
    /// The remote did not answer; the message says why, without credentials.
    Unreachable(String),
}

/// The decision taken before the apply writes anything.
#[derive(Debug)]
pub enum Preflight {
    /// The target is inside a repository; nothing will be initialized.
    Existing,
    /// The target will be initialized after the files are written.
    Initialize {
        remote: Option<String>,
        warning: Option<String>,
    },
}

/// The branch a new repository's `HEAD` names: the configured
/// `init.defaultBranch`, except that an unset or `master` value becomes `main`.
pub fn default_branch(configured: Option<&str>) -> &str {
    match configured.map(str::trim) {
        None | Some("") | Some("master") => DEFAULT_BRANCH,
        Some(name) => name,
    }
}

/// Decide, before any write, what the apply does for `planned`. A target inside
/// a repository is left as it is. Otherwise a remote with history stops the
/// apply; an unreachable remote is a warning, and the apply continues.
pub fn preflight(
    planned: &PlannedGit,
    target: &CanonicalTarget,
) -> Result<Preflight, GitTargetError> {
    preflight_with(planned, target, &probe)
}

pub(crate) fn preflight_with(
    planned: &PlannedGit,
    target: &CanonicalTarget,
    probe: &dyn Fn(&str) -> RemoteState,
) -> Result<Preflight, GitTargetError> {
    if inside_repository(target.as_path())? {
        return Ok(Preflight::Existing);
    }
    let warning = match planned.remote.as_deref() {
        None => None,
        Some(url) => match probe(&probe_location(url, target.as_path())) {
            RemoteState::Empty => None,
            RemoteState::HasHistory => {
                return Err(GitTargetError::RemoteHasHistory {
                    url: crate::source::without_userinfo(url),
                });
            }
            RemoteState::Unreachable(message) => Some(format!(
                "warning: could not inspect the remote {}; added it without a history check: {message}",
                crate::source::without_userinfo(url)
            )),
        },
    };
    Ok(Preflight::Initialize {
        remote: planned.remote.clone(),
        warning,
    })
}

/// Carry out a [`preflight`] decision after the files are written: initialize
/// the target, or report that it was already inside a repository.
pub fn set_up(preflight: Preflight, target: &CanonicalTarget) -> Result<GitReport, GitTargetError> {
    Ok(match preflight {
        Preflight::Existing => GitReport {
            setup: GitSetup::Existing,
            warning: None,
        },
        Preflight::Initialize { remote, warning } => GitReport {
            setup: GitSetup::Initialized {
                branch: initialize(target.as_path(), remote.as_deref())?,
                remote,
            },
            warning,
        },
    })
}

/// The location to probe for the remote `url` saved in the repository at
/// `target`. Git resolves a relative local path from the repository's working
/// tree, not from Toha's working directory, so the probe resolves it the same
/// way. Every other URL is probed as written.
fn probe_location(url: &str, target: &Path) -> String {
    let Ok(parsed) = gix::url::parse(gix::bstr::BStr::new(url)) else {
        return url.to_owned();
    };
    if parsed.scheme != gix::url::Scheme::File {
        return url.to_owned();
    }
    let path = gix::path::from_bstr(&parsed.path);
    if !path.is_relative() || path.has_root() {
        // Git cannot parse a verbatim `\\?\` drive path; probe its plain form.
        return match without_verbatim(&path) {
            Some(plain) => plain.to_string_lossy().into_owned(),
            None => url.to_owned(),
        };
    }
    // The target may not exist yet, so the leading `..` and `.` steps are
    // resolved lexically; the target is canonical, so no symlink sits above
    // it. The rest is kept as written, so the file system resolves any `..`
    // after a symlink the way git will.
    let mut resolved = without_verbatim(target).unwrap_or_else(|| target.to_path_buf());
    let mut parts = path.components().peekable();
    while let Some(part) = parts.peek() {
        match part {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            _ => break,
        }
        parts.next();
    }
    resolved.extend(parts);
    resolved.to_string_lossy().into_owned()
}

/// `path` without a Windows verbatim drive prefix (`\\?\C:\...` becomes
/// `C:\...`), or `None` when it has none. A canonical Windows path carries the
/// prefix, and git URL parsing rejects it.
fn without_verbatim(path: &Path) -> Option<std::path::PathBuf> {
    let rest = path.to_str()?.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then(|| std::path::PathBuf::from(rest))
}

/// Whether `path`, or its nearest existing ancestor, is inside a git repository.
fn inside_repository(path: &Path) -> Result<bool, GitTargetError> {
    let start = path
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(path)
        .to_path_buf();
    match gix::discover(&start) {
        Ok(_) => Ok(true),
        Err(gix::discover::Error::Discover(
            gix::discover::upwards::Error::NoGitRepository { .. }
            | gix::discover::upwards::Error::NoGitRepositoryWithinCeiling { .. }
            | gix::discover::upwards::Error::NoGitRepositoryWithinFs { .. },
        )) => Ok(false),
        Err(err) => Err(GitTargetError::Discover {
            path: path.display().to_string(),
            message: err.to_string(),
        }),
    }
}

/// List the references of the repository at `url`.
pub fn probe(url: &str) -> RemoteState {
    match list_refs(url) {
        Ok(0) => RemoteState::Empty,
        Ok(_) => RemoteState::HasHistory,
        Err(message) => RemoteState::Unreachable(crate::source::redacted(&message, url)),
    }
}

fn list_refs(url: &str) -> Result<usize, String> {
    // A remote needs a repository to belong to; a throwaway bare one holds it.
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let repo = gix::init_bare(scratch.path()).map_err(|e| e.to_string())?;
    let remote = repo
        .remote_at(url)
        .map_err(|e| e.to_string())?
        .with_refspecs(["+refs/*:refs/probe/*"], gix::remote::Direction::Fetch)
        .map_err(|e| e.to_string())?;
    let mut connection = remote
        .connect(gix::remote::Direction::Fetch)
        .map_err(|e| e.to_string())?;
    let mut roots = None;
    crate::source::trust_native_roots(&mut connection, &mut roots).map_err(|e| e.to_string())?;
    let (map, _) = connection
        .ref_map(
            gix::progress::Discard,
            gix::remote::ref_map::Options {
                prefix_from_spec_as_filter_on_remote: false,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    // An empty repository still advertises its unborn `HEAD`; only a reference
    // that names an object is history.
    Ok(map
        .remote_refs
        .iter()
        .filter(|r| !matches!(r, gix::protocol::handshake::Ref::Unborn { .. }))
        .count())
}

/// Initialize `target` as a repository whose `HEAD` names [`default_branch`],
/// and add `remote` as `origin` when given. Returns the branch.
pub fn initialize(target: &Path, remote: Option<&str>) -> Result<String, GitTargetError> {
    let fail = |message: String| GitTargetError::Init {
        path: target.display().to_string(),
        message,
    };
    let repo = gix::init(target).map_err(|e| fail(e.to_string()))?;
    let configured = repo
        .config_snapshot()
        .string("init.defaultBranch")
        .map(|v| v.to_string());
    let branch = default_branch(configured.as_deref()).to_owned();
    set_head(&repo, &branch).map_err(fail)?;
    if let Some(url) = remote {
        add_remote(&repo, url).map_err(|message| fail(crate::source::redacted(&message, url)))?;
    }
    Ok(branch)
}

fn set_head(repo: &gix::Repository, branch: &str) -> Result<(), String> {
    use gix::refs::{
        FullName, Target,
        transaction::{Change, LogChange, PreviousValue, RefEdit},
    };
    let referent = FullName::try_from(format!("refs/heads/{branch}"))
        .map_err(|e| format!("invalid branch name {branch:?}: {e}"))?;
    let head = FullName::try_from("HEAD").map_err(|e| e.to_string())?;
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange::default(),
            expected: PreviousValue::Any,
            new: Target::Symbolic(referent),
        },
        name: head,
        deref: false,
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Write `remote "origin"` to the local config as `git remote add` does.
fn add_remote(repo: &gix::Repository, url: &str) -> Result<(), String> {
    // Parse first, so an invalid URL is named before the config changes.
    repo.remote_at(url).map_err(|e| e.to_string())?;
    let path = repo.common_dir().join("config");
    let mut file =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
            .map_err(|e| e.to_string())?;
    {
        let mut section = file
            .section_mut_or_create_new("remote", Some(REMOTE_NAME.into()))
            .map_err(|e| e.to_string())?;
        section.push("url", url).map_err(|e| e.to_string())?;
        section
            .push(
                "fetch",
                format!("+refs/heads/*:refs/remotes/{REMOTE_NAME}/*").as_str(),
            )
            .map_err(|e| e.to_string())?;
    }
    let mut out = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    file.write_to(&mut out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
