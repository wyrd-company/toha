// ---
// relationships:
//   implements: architecture
// ---
use crate::Dirs;
use std::{
    fs,
    path::{Path, PathBuf},
};
use toha::{
    config::{self, Config},
    registry::{self, Layer, Registry, RegistryFile},
    source::{self, Address},
};

#[derive(Debug)]
pub struct ResolvedTemplate {
    pub formal_name: String,
    pub commit: String,
    pub folder: PathBuf,
    pub trusted: bool,
}

#[derive(Debug)]
pub enum ResolveError {
    Error(String),
    Ambiguous { name: String, matches: Vec<String> },
}
impl ResolveError {
    fn text(error: impl ToString) -> Self {
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
        trusted: listed.trusted,
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
        trusted: false,
    })
}

pub fn resolve_template(
    arg: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<ResolvedTemplate, ResolveError> {
    let address = source::parse(arg, &config.hosts, cwd, &dirs.home).map_err(ResolveError::text)?;
    match &address {
        Address::Folder(folder) => Ok(ResolvedTemplate {
            formal_name: folder.to_string_lossy().into_owned(),
            commit: String::new(),
            folder: folder.clone(),
            trusted: entry(address.formal_name(&config.hosts), registry).is_some_and(|v| v.trusted),
        }),
        Address::Git { .. } => {
            let formal = address.formal_name(&config.hosts);
            entry(formal, registry).map_or_else(|| fetch_new(&address, config, dirs), Ok)
        }
        Address::Name(name) => {
            let resolved = registry.resolve(name)?;
            Ok(entry(resolved.formal_name, registry).expect("resolved registry entry"))
        }
    }
}

pub fn resume_template(
    formal: &str,
    commit: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<ResolvedTemplate, ResolveError> {
    if commit.is_empty() {
        let address =
            source::parse(formal, &config.hosts, cwd, &dirs.home).map_err(ResolveError::text)?;
        let Address::Folder(folder) = address else {
            return Err(ResolveError::text(
                "staged folder template has invalid formal name",
            ));
        };
        return Ok(ResolvedTemplate {
            formal_name: formal.into(),
            commit: String::new(),
            folder,
            trusted: entry(formal.into(), registry).is_some_and(|v| v.trusted),
        });
    }
    if let Some(found) = entry(formal.into(), registry) {
        if found.commit == commit {
            return Ok(found);
        }
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
        trusted: entry(formal.into(), registry).is_some_and(|v| v.trusted),
    })
}
