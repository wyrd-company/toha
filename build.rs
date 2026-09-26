// ---
// relationships:
//   implements: architecture
// ---
//! Sets `TOHA_VERSION` from the git tags of the toha repository.
//!
//! The version is the tagver version of the repository whose work tree is the
//! package root. A package root that is not such a work tree (a source archive,
//! a crates.io package, or a copy inside another repository) uses the package
//! version.
use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let version = match repository(&root) {
        Some(repository) => {
            for path in watched_paths(&repository) {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            match tagver::git::calculate_version(&repository, &tagver::Config::default()) {
                Ok((version, _, _)) => version.to_string(),
                Err(error) => {
                    println!("cargo:warning=tagver: {error}; using the package version");
                    package_version()
                }
            }
        }
        None => package_version(),
    };
    println!("cargo:rustc-env=TOHA_VERSION={version}");
}

fn package_version() -> String {
    env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION")
}

/// The repository whose work tree is `root`, so that a parent repository does
/// not supply its tags to a copy of the package.
fn repository(root: &Path) -> Option<tagver::Repository> {
    let repository = tagver::Repository::discover(root).ok()?;
    let work_tree = repository.work_dir()?.canonicalize().ok()?;
    (work_tree == root.canonicalize().ok()?).then_some(repository)
}

/// The git files that decide the version: HEAD, the ref HEAD names, the tags,
/// and the packed refs. A ref that has no file yet is watched through its
/// nearest existing directory, so that creating it reruns the script.
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
