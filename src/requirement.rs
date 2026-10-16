// ---
// relationships:
//   implements: template-format
// ---
//! The programs a template declares in `requires`, confirmed on `PATH` before
//! an interview starts so that a caller does not answer questions for an apply
//! whose hooks cannot run.
use crate::template::Template;
use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

/// One program a template requires: a bare executable name and an optional
/// hint that tells the caller how to install it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub program: String,
    pub hint: Option<String>,
}

/// The requirements of one template that are not on the search path, in
/// declaration order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct MissingRequirements {
    pub template: String,
    pub missing: Vec<Requirement>,
}
impl fmt::Display for MissingRequirements {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "template `{}` requires programs that are not on PATH:",
            self.template
        )?;
        for requirement in &self.missing {
            match &requirement.hint {
                Some(hint) => write!(f, "\n  {}: {hint}", requirement.program)?,
                None => write!(f, "\n  {}", requirement.program)?,
            }
        }
        Ok(())
    }
}

/// Confirms every requirement of `template` is on the process `PATH`. A
/// relative `PATH` entry resolves against the process working directory, as
/// it does when a hook starts the program; see [`locate`].
pub fn check(template: &Template) -> Result<(), MissingRequirements> {
    check_in(
        template,
        search_path().as_deref(),
        &std::env::current_dir().unwrap_or_default(),
    )
}

/// The process `PATH` with each relative entry, including an empty one, made
/// absolute against the process working directory. Every hook receives this
/// value as its `PATH`, so a hook, a script it runs, and their children search
/// the directories [`check`] searched, whatever the hook's working directory.
/// A relative entry is dropped when it has no absolute `PATH` form: the working
/// directory is unavailable, or its absolute form holds the platform separator
/// (`:` on Unix). The check and every hook then search the same remaining
/// entries, so the check refuses before the interview rather than a hook
/// failing after files are written.
///
/// `None` means no directory remains, or `PATH` is unset. The check then finds
/// no program, and a hook inherits the environment unchanged. An empty value
/// is never returned, because an empty `PATH` names the working directory.
pub fn search_path() -> Option<OsString> {
    let path = std::env::var_os("PATH")?;
    let cwd = std::env::current_dir().ok();
    let entries: Vec<PathBuf> = std::env::split_paths(&path)
        .filter_map(|entry| {
            if entry.is_absolute() {
                return Some(entry);
            }
            let absolute = cwd.as_ref()?.join(entry);
            std::env::join_paths([&absolute])
                .is_ok()
                .then_some(absolute)
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    std::env::join_paths(entries).ok()
}

/// The executable that `program` names on [`search_path`], resolved the same
/// way as [`check`], with the platform's executable extensions on Windows.
pub fn locate(program: &str) -> Option<PathBuf> {
    which::which_in(
        program,
        search_path(),
        std::env::current_dir().unwrap_or_default(),
    )
    .ok()
}

/// Confirms every requirement of `template` is on the search path `paths`,
/// in the platform `PATH` form; `cwd` resolves a relative path entry.
pub fn check_in(
    template: &Template,
    paths: Option<&OsStr>,
    cwd: &Path,
) -> Result<(), MissingRequirements> {
    let missing: Vec<Requirement> = template
        .requirements()
        .into_iter()
        .filter(|requirement| which::which_in(&requirement.program, paths, cwd).is_err())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(MissingRequirements {
            template: template.name.clone(),
            missing,
        })
    }
}
