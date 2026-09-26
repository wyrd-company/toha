// ---
// relationships:
//   implements: architecture
// ---
//! Resolves the toha version from the git tags of the package root.
//!
//! The version is the tagver version of the repository whose work tree is the
//! package root. A package root that is not such a work tree (a source archive,
//! a crates.io package, or a copy inside another repository) uses the fallback
//! version. `build.rs` and the tests include this file.
use std::path::{Path, PathBuf};

pub struct Resolution {
    pub version: String,
    /// The git files that decide the version.
    pub watched: Vec<PathBuf>,
    /// Why the fallback version replaced the tagver version.
    pub warning: Option<String>,
}

pub fn resolve(root: &Path, fallback: &str) -> Resolution {
    let Some(repository) = repository(root) else {
        return Resolution {
            version: fallback.into(),
            watched: vec![],
            warning: None,
        };
    };
    let watched = watched_paths(&repository);
    match tagver::git::calculate_version(&repository, &tagver::Config::default()) {
        Ok((version, _, _)) => Resolution {
            version: version.to_string(),
            watched,
            warning: None,
        },
        Err(error) => Resolution {
            version: fallback.into(),
            watched,
            warning: Some(format!("tagver: {error}; using the package version")),
        },
    }
}

/// The repository whose work tree is `root`, so that a parent repository does
/// not supply its tags to a copy of the package.
fn repository(root: &Path) -> Option<tagver::Repository> {
    let repository = tagver::Repository::discover(root).ok()?;
    let work_tree = repository.work_dir()?.canonicalize().ok()?;
    (work_tree == root.canonicalize().ok()?).then_some(repository)
}

/// HEAD, the ref HEAD names, the tags, and the packed refs. A ref that has no file yet is watched through its nearest
/// existing directory, so that creating it reruns the script.
fn watched_paths(repository: &tagver::Repository) -> Vec<PathBuf> {
    let git = repository.inner();
    let common = canonical(git.common_dir());
    let mut paths = vec![
        canonical(git.git_dir()).join("HEAD"),
        common.join("packed-refs"),
    ];
    let mut refs = vec![common.join("refs/tags")];
    if let Ok(Some(name)) = git.head_name() {
        refs.push(common.join(name.as_bstr().to_string()));
    }
    let refs_root = common.join("refs");
    for path in refs {
        let existing = path
            .ancestors()
            .take_while(|p| p.starts_with(&refs_root))
            .find(|p| p.exists());
        if let Some(existing) = existing {
            paths.push(existing.to_path_buf());
        }
    }
    paths.retain(|p| p.exists());
    paths
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
