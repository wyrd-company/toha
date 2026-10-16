// ---
// relationships:
//   implements: template-format
// ---
//! The programs a template declares in `requires`, confirmed on `PATH` before
//! an interview starts so that a caller does not answer questions for an apply
//! whose hooks cannot run.
use crate::template::Template;
use std::{
    ffi::OsStr,
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
        std::env::var_os("PATH").as_deref(),
        &std::env::current_dir().unwrap_or_default(),
    )
}

/// The executable that `program` names on the process `PATH`, resolved the
/// same way as [`check`]. A hook starts this path, so the pre-interview check
/// and the hook agree on which program runs, whatever the hook's working
/// directory.
pub fn locate(program: &str) -> Option<PathBuf> {
    which::which_in(
        program,
        std::env::var_os("PATH"),
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
