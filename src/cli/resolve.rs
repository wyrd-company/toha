// ---
// relationships:
//   implements: architecture
// ---
use crate::{Dirs, cli::bundled};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toha::{
    ReviewDigest,
    config::{self, Config},
    registry::{self, Layer, Registry, RegistryFile},
    source::{self, Address},
};

#[derive(Debug)]
pub struct ResolvedTemplate {
    pub formal_name: String,
    pub commit: String,
    pub folder: PathBuf,
    /// The approved executable-surface digest for a template resolved by name
    /// from a writable registry, or `None` for an address, folder, or local
    /// template, which run hooks only through `--trust`.
    pub approval: Option<ReviewDigest>,
    pub named: bool,
    /// Effective aliases of the selected named registry entry, in registry
    /// order; empty for a folder, direct Git, or bundled selection.
    pub aliases: Vec<String>,
    /// The selected named entry's source; `None` when the selection has no
    /// registry entry.
    pub source: Option<String>,
}

#[derive(Debug)]
pub enum ResolveError {
    Error(String),
    Ambiguous { name: String, matches: Vec<String> },
}
impl ResolveError {
    pub(crate) fn text(error: impl ToString) -> Self {
        Self::Error(error.to_string())
    }
}
impl From<registry::ResolveError> for ResolveError {
    fn from(error: registry::ResolveError) -> Self {
        match error {
            registry::ResolveError::NotFound(name) => {
                Self::Error(format!("template not found: {name}"))
            }
            registry::ResolveError::Ambiguous { name, matches } => {
                Self::Ambiguous { name, matches }
            }
        }
    }
}

pub fn load_context(dirs: &Dirs, cwd: &Path) -> Result<(Config, Registry), ResolveError> {
    let config = config::load(&dirs.config_paths(), cwd).map_err(ResolveError::text)?;
    let system = RegistryFile::load(&dirs.system_data.join("templates.yml"), Layer::System)
        .map_err(ResolveError::text)?;
    let user = RegistryFile::load(&dirs.user_data.join("templates.yml"), Layer::User)
        .map_err(ResolveError::text)?;
    let local = if let Some(root) = config.local_templates_paths.first() {
        RegistryFile::load(&root.join("templates.yml"), Layer::Local).map_err(ResolveError::text)?
    } else {
        RegistryFile::default()
    };
    let mut registry = Registry::merge(&system, &user, &local).map_err(ResolveError::text)?;
    registry
        .discover(&config.templates_paths, &dirs.user_data)
        .map_err(ResolveError::text)?;
    for (alias, formal) in registry
        .apply_local_aliases(&local)
        .map_err(ResolveError::text)?
    {
        eprintln!("warning: local alias {alias} names {formal}, which is not installed");
    }
    Ok((config, registry))
}

fn entry(formal: String, registry: &Registry) -> Option<ResolvedTemplate> {
    let listed = registry.entries.get(&formal)?;
    Some(ResolvedTemplate {
        formal_name: formal,
        commit: listed.entry.commit.clone().unwrap_or_default(),
        folder: listed.entry.path.clone(),
        approval: listed.approval.clone(),
        named: true,
        aliases: listed.entry.aliases.clone(),
        source: Some(listed.entry.source.clone()),
    })
}

fn selected_folder(root: &Path, address: &Address) -> Result<PathBuf, ResolveError> {
    let root = root.canonicalize().map_err(ResolveError::text)?;
    let path = match address {
        Address::Git {
            path: Some(path), ..
        } => root.join(path),
        _ => root.clone(),
    };
    let folder = path.canonicalize().map_err(ResolveError::text)?;
    if !folder.starts_with(root) {
        return Err(ResolveError::text("template path leaves repository"));
    }
    Ok(folder)
}

fn cached(
    address: &Address,
    formal: &str,
    dirs: &Dirs,
    commit: &str,
) -> Result<PathBuf, ResolveError> {
    let root = dirs
        .cache
        .join("sources")
        .join(source::install_key(formal))
        .join(commit);
    if !root.is_dir() {
        let parent = root
            .parent()
            .ok_or_else(|| ResolveError::text("cache path has no parent"))?;
        fs::create_dir_all(parent).map_err(ResolveError::text)?;
        let temporary = tempfile::tempdir_in(parent).map_err(ResolveError::text)?;
        let clone = temporary.path().join("repo");
        let fetched = source::fetch(address, &clone).map_err(ResolveError::text)?;
        if fetched.commit != commit {
            return Err(ResolveError::text(format!(
                "fetched commit {} differs from recorded commit {commit}",
                fetched.commit
            )));
        }
        if !root.exists() {
            fs::rename(clone, &root).map_err(ResolveError::text)?;
        }
    }
    selected_folder(&root, address)
}

fn fetch_new(
    address: &Address,
    config: &Config,
    dirs: &Dirs,
) -> Result<ResolvedTemplate, ResolveError> {
    let formal_name = address.formal_name(&config.hosts);
    let parent = dirs
        .cache
        .join("sources")
        .join(source::install_key(&formal_name));
    if let Address::Git {
        reference: Some(commit),
        ..
    } = address
        && commit.len() == 40
        && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        let root = parent.join(commit);
        if root.is_dir() {
            return Ok(ResolvedTemplate {
                formal_name,
                commit: commit.clone(),
                folder: selected_folder(&root, address)?,
                approval: None,
                named: false,
                aliases: Vec::new(),
                source: None,
            });
        }
    }
    fs::create_dir_all(&parent).map_err(ResolveError::text)?;
    let temporary = tempfile::tempdir_in(&parent).map_err(ResolveError::text)?;
    let clone = temporary.path().join("repo");
    let fetched = source::fetch(address, &clone).map_err(ResolveError::text)?;
    let root = parent.join(&fetched.commit);
    if !root.exists() {
        fs::rename(clone, &root).map_err(ResolveError::text)?;
    }
    Ok(ResolvedTemplate {
        formal_name,
        commit: fetched.commit,
        folder: selected_folder(&root, address)?,
        approval: None,
        named: false,
        aliases: Vec::new(),
        source: None,
    })
}

pub fn resolve_template(
    arg: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<ResolvedTemplate, ResolveError> {
    // Windows verbatim formal names already address installed registry entries.
    // Keep that named route while allowing unregistered absolute folder paths.
    if cfg!(windows) && arg.starts_with(r"\\?\") {
        match registry.resolve(arg) {
            Ok(resolved) => {
                return Ok(entry(resolved.formal_name, registry).expect("resolved registry entry"));
            }
            Err(registry::ResolveError::NotFound(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let address = source::parse(arg, &config.hosts, cwd, &dirs.home).map_err(ResolveError::text)?;
    match &address {
        Address::Folder(folder) => Ok(ResolvedTemplate {
            formal_name: folder.to_string_lossy().into_owned(),
            commit: String::new(),
            folder: folder.clone(),
            approval: None,
            named: false,
            aliases: Vec::new(),
            source: None,
        }),
        Address::Git { .. } => {
            let formal = address.formal_name(&config.hosts);
            if let Some(mut found) = entry(formal, registry) {
                found.approval = None;
                found.named = false;
                found.aliases = Vec::new();
                found.source = None;
                Ok(found)
            } else {
                fetch_new(&address, config, dirs)
            }
        }
        Address::Name(name) => match registry.resolve(name) {
            Ok(resolved) => {
                Ok(entry(resolved.formal_name, registry).expect("resolved registry entry"))
            }
            // The reserved demo answers only when nothing else does; NotFound(other)
            // and every Ambiguous propagate exactly as today.
            Err(registry::ResolveError::NotFound(n)) if n == bundled::RESERVED => {
                bundled::resolve(dirs)
            }
            Err(e) => Err(e.into()),
        },
    }
}

/// The formal name a `<TEMPLATE>` argument resolves to, without fetching it.
pub fn formal_name(
    arg: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<String, ResolveError> {
    let address = source::parse(arg, &config.hosts, cwd, &dirs.home).map_err(ResolveError::text)?;
    Ok(match &address {
        Address::Folder(folder) => folder.to_string_lossy().into_owned(),
        Address::Git { .. } => address.formal_name(&config.hosts),
        // A reserved NotFound keeps the name, so `apply toha-demo` finds its own
        // staged run.
        Address::Name(name) => match registry.resolve(name) {
            Ok(resolved) => resolved.formal_name,
            Err(registry::ResolveError::NotFound(n)) if n == bundled::RESERVED => n,
            Err(e) => return Err(e.into()),
        },
    })
}

pub fn resume_template(
    formal: &str,
    commit: &str,
    named: bool,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<ResolvedTemplate, ResolveError> {
    if commit.is_empty() {
        let path = Path::new(formal);
        if !path.is_absolute() {
            return Err(ResolveError::text(
                "staged folder template has invalid formal name",
            ));
        }
        let folder = path.canonicalize().map_err(ResolveError::text)?;
        return Ok(ResolvedTemplate {
            formal_name: formal.into(),
            commit: String::new(),
            folder,
            approval: if named {
                entry(formal.into(), registry).and_then(|v| v.approval)
            } else {
                None
            },
            named,
            aliases: Vec::new(),
            source: None,
        });
    }
    if let Some(found) = entry(formal.into(), registry)
        && found.commit == commit
    {
        return Ok(ResolvedTemplate {
            approval: if named { found.approval.clone() } else { None },
            named,
            ..found
        });
    }
    if let Some(result) = bundled::resume(formal, commit, dirs) {
        return result;
    }
    let mut address =
        source::parse(formal, &config.hosts, cwd, &dirs.home).map_err(ResolveError::text)?;
    let Address::Git { reference, .. } = &mut address else {
        return Err(ResolveError::text(
            "staged git template has invalid formal name",
        ));
    };
    *reference = Some(commit.into());
    let folder = cached(&address, formal, dirs, commit)?;
    Ok(ResolvedTemplate {
        formal_name: formal.into(),
        commit: commit.into(),
        folder,
        approval: if named {
            entry(formal.into(), registry).and_then(|v| v.approval)
        } else {
            None
        },
        named,
        aliases: Vec::new(),
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn fetched_cache_commit_must_match_record() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: Vec<&str>| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("HOME", root.path().join("home"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        };
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Sample"],
            vec!["config", "user.email", "sample@example.invalid"],
        ] {
            assert!(git(args));
        }
        fs::write(repo.join("file.txt"), "content").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-m", "initial"]] {
            assert!(git(args));
        }
        let dirs = Dirs {
            system_config: root.path().join("system-config"),
            user_config: root.path().join("user-config"),
            local_config_override: None,
            system_data: root.path().join("system-data"),
            user_data: root.path().join("user-data"),
            cache: root.path().join("cache"),
            state: root.path().join("state"),
            home: root.path().join("home"),
        };
        let address = Address::Git {
            repo: format!("file://{}", repo.display()),
            reference: None,
            path: None,
        };
        let recorded = "0000000000000000000000000000000000000000";
        let overrides = [
            ("HOME", root.path().join("home")),
            ("XDG_CONFIG_HOME", root.path().join("config")),
            ("XDG_DATA_HOME", root.path().join("data")),
            ("XDG_CACHE_HOME", root.path().join("cache")),
            ("XDG_STATE_HOME", root.path().join("state")),
            (
                "TOHA_USER_CONFIG",
                root.path().join("config/toha/config.yml"),
            ),
            ("TOHA_CONFIG", root.path().join("local.yml")),
        ];
        let prior: Vec<_> = overrides
            .iter()
            .map(|(key, _)| std::env::var_os(key))
            .collect();
        unsafe {
            for (key, value) in &overrides {
                std::env::set_var(key, value);
            }
        }
        let error = cached(&address, "sample", &dirs, recorded).unwrap_err();
        unsafe {
            for ((key, _), value) in overrides.iter().zip(prior) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        assert!(
            matches!(error, ResolveError::Error(message) if message.contains("differs from recorded commit"))
        );
    }
}

/// Interpret the selected template revision without treating content as Git.
pub fn snapshot_revision(
    formal: &str,
    commit: &str,
) -> Result<toha::snapshot::Revision, toha::snapshot::SnapshotError> {
    use toha::snapshot::{CommitId, ContentDigest, Revision};
    if commit.is_empty() {
        Ok(Revision::Unversioned)
    } else if formal == bundled::RESERVED {
        ContentDigest::parse(commit).map(Revision::Content)
    } else {
        CommitId::parse(commit).map(Revision::Commit)
    }
}

/// Resume a content snapshot only through a content-aware source resolver.
pub fn resume_snapshot_template(
    formal: &str,
    revision: &toha::snapshot::Revision,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<ResolvedTemplate, ResolveError> {
    use toha::snapshot::Revision;
    match revision {
        Revision::Content(digest) => {
            bundled::resume(formal, digest.as_str(), dirs).unwrap_or_else(|| {
                Err(ResolveError::text(
                    "no content resolver for this template source",
                ))
            })
        }
        Revision::Commit(commit) => {
            resume_template(formal, commit.as_str(), false, config, registry, dirs, cwd)
        }
        Revision::Unversioned => resume_template(formal, "", false, config, registry, dirs, cwd),
    }
}

#[cfg(test)]
mod content_revision_tests {
    use super::*;
    use toha::snapshot::{ContentDigest, Revision};

    #[test]
    fn content_revision_never_falls_through_to_git_resolution() {
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            system_config: root.path().join("system-config"),
            user_config: root.path().join("user-config"),
            local_config_override: None,
            system_data: root.path().join("system-data"),
            user_data: root.path().join("user-data"),
            cache: root.path().join("cache"),
            state: root.path().join("state"),
            home: root.path().join("home"),
        };
        let config = config::load(&dirs.config_paths(), root.path()).unwrap();
        let revision = Revision::Content(ContentDigest::parse(&"ab".repeat(32)).unwrap());
        let result = resume_snapshot_template(
            "gh:example/template",
            &revision,
            &config,
            &Registry::default(),
            &dirs,
            root.path(),
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("content revision reached Git"),
        };
        assert!(
            matches!(&error, ResolveError::Error(message) if message.contains("no content resolver")),
            "{error:?}"
        );
        assert!(
            !dirs.cache.exists(),
            "no Git fetch or cache materialization"
        );
        let result = resume_snapshot_template(
            bundled::RESERVED,
            &revision,
            &config,
            &Registry::default(),
            &dirs,
            root.path(),
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("stale content accepted"),
        };
        assert!(
            matches!(&error, ResolveError::Error(message) if message.contains("changed since")),
            "{error:?}"
        );
    }
}
