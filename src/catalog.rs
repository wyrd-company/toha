// ---
// relationships:
//   implements: template-registry
// ---
//! Installed-template transactions shared by embedded callers and the command line.
use crate::{
    Template, bundled, config,
    registry::{self, Entry, Layer, Registry, RegistryFile, ResolveError},
    resolve::Paths as Dirs,
    source::{self, Address, RefKind},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[non_exhaustive]
#[derive(Debug)]
pub enum Event {
    Added(String),
    SharedShortName {
        name: String,
        formal: String,
    },
    Pinned(String),
    Updated(String),
    ApprovalLapsed(String),
    Removed(String),
    AliasRemoved(String),
    AliasAdded {
        alias: String,
        formal: String,
    },
    AliasShadowsShortName(String),
    Trusted {
        formal: String,
        already: bool,
        surface: Option<crate::HookSurface>,
    },
    Untrusted {
        formal: String,
        already: bool,
    },
}
#[derive(Debug)]
pub struct ListedTemplate {
    pub listed: registry::Listed,
    pub trusted: bool,
}
#[derive(Debug)]
pub struct Listing {
    pub entries: Vec<ListedTemplate>,
    pub bundled: Option<bundled::BundledRow>,
}
#[derive(Debug)]
#[non_exhaustive]
pub enum CatalogError {
    Error(String),
    UserOnly(String),
    AddressRequired {
        name: String,
        folder_exists: bool,
    },
    AliasMultiple {
        alias: String,
        addresses: Vec<String>,
    },
    AliasInUse {
        template: String,
        alias: String,
        removable: bool,
    },
    Surface {
        formal: String,
        error: crate::ReviewError,
    },
    Ambiguous {
        name: String,
        matches: Vec<String>,
    },
}
impl From<ResolveError> for CatalogError {
    fn from(value: ResolveError) -> Self {
        match value {
            ResolveError::NotFound(name) => Self::Error(format!("template not found: {name}")),
            ResolveError::Ambiguous { name, matches } => Self::Ambiguous { name, matches },
        }
    }
}
impl CatalogError {
    fn text(message: impl ToString) -> Self {
        Self::Error(message.to_string())
    }
}
/// Installed templates and the writable user registry, loaded from explicit paths.
#[derive(Debug)]
pub struct Catalog {
    dirs: Dirs,
    config: config::Config,
    system: RegistryFile,
    user: RegistryFile,
    local: RegistryFile,
    registry: Registry,
    pub warnings: Vec<String>,
}
impl Catalog {
    pub fn load(dirs: Dirs, cwd: &Path) -> Result<Self, CatalogError> {
        let config = config::load(&dirs.config_paths(), cwd).map_err(CatalogError::text)?;
        let system = RegistryFile::load(&dirs.system_data.join("templates.yml"), Layer::System)
            .map_err(CatalogError::text)?;
        let user = RegistryFile::load(&dirs.user_data.join("templates.yml"), Layer::User)
            .map_err(CatalogError::text)?;
        let local = if let Some(root) = config.local_templates_paths.first() {
            RegistryFile::load(&root.join("templates.yml"), Layer::Local)
                .map_err(CatalogError::text)?
        } else {
            RegistryFile::default()
        };
        let mut registry = Registry::merge(&system, &user, &local).map_err(CatalogError::text)?;
        registry
            .discover(&config.templates_paths, &dirs.user_data)
            .map_err(CatalogError::text)?;
        let missing = registry
            .apply_local_aliases(&local)
            .map_err(CatalogError::text)?;
        let warnings = missing
            .into_iter()
            .map(|(alias, formal)| {
                format!("local alias {alias} names {formal}, which is not installed")
            })
            .collect();
        Ok(Self {
            dirs,
            config,
            system,
            user,
            local,
            registry,
            warnings,
        })
    }
    fn write(&self, file: &RegistryFile) -> Result<(), CatalogError> {
        file.write_atomic(&self.dirs.user_data.join("templates.yml"))
            .map_err(CatalogError::text)
    }
    fn resolve_user(&self, name: &str) -> Result<(String, Entry), CatalogError> {
        let resolved = self.registry.resolve(name)?;
        self.user
            .templates
            .get(&resolved.formal_name)
            .cloned()
            .map(|entry| (resolved.formal_name, entry))
            .ok_or_else(|| CatalogError::UserOnly(name.into()))
    }
}
fn find_templates(root: &Path, selected: Option<&str>) -> Result<Vec<PathBuf>, CatalogError> {
    let root = root.canonicalize().map_err(CatalogError::text)?;
    let start = if let Some(path) = selected {
        root.join(path)
    } else {
        root.clone()
    };
    let start = start.canonicalize().map_err(CatalogError::text)?;
    if !start.starts_with(&root) {
        return Err(CatalogError::text("template path leaves repository"));
    }
    let mut found = Vec::new();
    fn visit(
        dir: &Path,
        root: &Path,
        seen: &mut std::collections::HashSet<PathBuf>,
        found: &mut Vec<PathBuf>,
    ) -> Result<(), CatalogError> {
        let canonical = dir.canonicalize().map_err(CatalogError::text)?;
        if !canonical.starts_with(root) {
            return Err(CatalogError::text("template path leaves repository"));
        }
        if !seen.insert(canonical.clone()) {
            return Ok(());
        }
        if canonical.join("template.yml").is_file() {
            found.push(canonical);
            return Ok(());
        }
        for child in fs::read_dir(&canonical).map_err(CatalogError::text)? {
            let child = child.map_err(CatalogError::text)?.path();
            if child.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if child.is_dir() {
                visit(&child, root, seen, found)?;
            }
        }
        Ok(())
    }
    visit(
        &start,
        &root,
        &mut std::collections::HashSet::new(),
        &mut found,
    )?;
    found.sort();
    if selected.is_some() && found.len() != 1 {
        return Err(CatalogError::text("#path must select exactly one template"));
    }
    if found.is_empty() {
        return Err(CatalogError::text("no template found"));
    }
    Ok(found)
}
fn clone_sibling(
    address: &Address,
    install: &Path,
) -> Result<(tempfile::TempDir, source::Fetched), CatalogError> {
    let parent = install
        .parent()
        .ok_or_else(|| CatalogError::text("install path has no parent"))?;
    fs::create_dir_all(parent).map_err(CatalogError::text)?;
    let container = tempfile::Builder::new()
        .prefix(".toha-clone-")
        .tempdir_in(parent)
        .map_err(CatalogError::text)?;
    let fetched =
        source::fetch(address, &container.path().join("repo")).map_err(CatalogError::text)?;
    Ok((container, fetched))
}
struct SwappedClone {
    install: PathBuf,
    backup: Option<tempfile::TempDir>,
}
impl SwappedClone {
    fn rollback(self) -> Result<(), CatalogError> {
        fs::remove_dir_all(&self.install).map_err(CatalogError::text)?;
        if let Some(backup) = self.backup {
            fs::rename(backup.path().join("repo"), &self.install).map_err(CatalogError::text)?;
            backup.close().map_err(CatalogError::text)?;
        }
        Ok(())
    }
    fn finish(self) -> Result<(), CatalogError> {
        if let Some(backup) = self.backup {
            backup.close().map_err(CatalogError::text)?;
        }
        Ok(())
    }
}
fn swap_clone(temp: &Path, install: &Path) -> Result<SwappedClone, CatalogError> {
    let backup = if install.exists() {
        let parent = install
            .parent()
            .ok_or_else(|| CatalogError::text("install path has no parent"))?;
        let holder = tempfile::Builder::new()
            .prefix(".toha-backup-")
            .tempdir_in(parent)
            .map_err(CatalogError::text)?;
        fs::rename(install, holder.path().join("repo")).map_err(CatalogError::text)?;
        Some(holder)
    } else {
        None
    };
    if let Err(error) = fs::rename(temp, install) {
        if let Some(holder) = backup.as_ref() {
            fs::rename(holder.path().join("repo"), install).map_err(CatalogError::text)?;
        }
        return Err(CatalogError::text(error));
    }
    Ok(SwappedClone {
        install: install.into(),
        backup,
    })
}
fn install_staged(
    ctx: &Catalog,
    staged: &[(tempfile::TempDir, PathBuf)],
    registry: &RegistryFile,
) -> Result<(), CatalogError> {
    let mut swapped = Vec::new();
    for (clone, install) in staged {
        match swap_clone(&clone.path().join("repo"), install) {
            Ok(swap) => swapped.push(swap),
            Err(error) => {
                for swap in swapped.into_iter().rev() {
                    swap.rollback()?;
                }
                return Err(error);
            }
        }
    }
    if let Err(error) = ctx.write(registry) {
        for swap in swapped.into_iter().rev() {
            swap.rollback()?;
        }
        return Err(error);
    }
    for swap in swapped {
        swap.finish()?;
    }
    Ok(())
}
pub fn add(
    ctx: &Catalog,
    address: &str,
    alias: Option<String>,
    trust: bool,
    cwd: &Path,
) -> Result<Vec<Event>, CatalogError> {
    if alias.as_deref().is_some_and(|value| !valid_alias(value)) {
        return Err(CatalogError::text("invalid alias"));
    }
    let parsed = source::parse(address, &ctx.config.hosts, cwd, &ctx.dirs.home)
        .map_err(CatalogError::text)?;
    let (root, clone, commit) = match &parsed {
        Address::Git { .. } => {
            let install = ctx
                .dirs
                .user_data
                .join("repos")
                .join(source::install_key(&parsed.formal_name(&ctx.config.hosts)));
            let (clone, fetched) = clone_sibling(&parsed, &install)?;
            (
                clone.path().join("repo"),
                Some((clone, install)),
                Some(fetched.commit),
            )
        }
        Address::Folder(folder) => (folder.clone(), None, None),
        Address::Name(name) => {
            return Err(CatalogError::AddressRequired {
                name: name.clone(),
                folder_exists: cwd.join(name).is_dir(),
            });
        }
    };
    let root = root.canonicalize().map_err(CatalogError::text)?;
    let selected = if let Address::Git { path, .. } = &parsed {
        path.as_deref()
    } else {
        None
    };
    let folders = find_templates(&root, selected)?;
    if let Some(name) = alias.as_deref().filter(|_| folders.len() != 1) {
        let addresses: Vec<String> = folders
            .iter()
            .map(|folder| match &parsed {
                Address::Git { .. } => {
                    let base = address.split_once('#').map_or(address, |(base, _)| base);
                    let path = folder.strip_prefix(&root).unwrap_or(folder);
                    format!("{base}#{}", path.to_string_lossy().replace('\\', "/"))
                }
                _ => folder.to_string_lossy().to_string(),
            })
            .collect();
        return Err(CatalogError::AliasMultiple {
            alias: name.into(),
            addresses,
        });
    }
    let mut additions = Vec::new();
    let mut names = Vec::new();
    for folder in folders {
        let loaded = Template::load(&folder).map_err(CatalogError::text)?;
        let name = loaded.name.clone();
        // `--trust` approves the template's current executable surface; the
        // digest authorizes its hooks only while the installed content matches.
        let approval = if trust {
            Some(
                crate::HookSurface::of(&loaded)
                    .map_err(CatalogError::text)?
                    .digest(),
            )
        } else {
            None
        };
        let formal = if matches!(parsed, Address::Folder(_)) {
            folder.to_string_lossy().into_owned()
        } else {
            let base = Address::Git {
                repo: match &parsed {
                    Address::Git { repo, .. } => repo.clone(),
                    _ => unreachable!(),
                },
                reference: match &parsed {
                    Address::Git { reference, .. } => reference.clone(),
                    _ => unreachable!(),
                },
                path: None,
            }
            .formal_name(&ctx.config.hosts);
            let relative = folder.strip_prefix(&root).map_err(CatalogError::text)?;
            if relative.as_os_str().is_empty() {
                base
            } else {
                format!("{base}#{}", relative.to_string_lossy().replace('\\', "/"))
            }
        };
        let path = if let Some((_, install)) = &clone {
            install.join(folder.strip_prefix(&root).map_err(CatalogError::text)?)
        } else {
            folder.clone()
        };
        let entry = Entry {
            name: name.clone(),
            source: match &parsed {
                Address::Git { repo, .. } => source::without_userinfo(repo),
                Address::Folder(folder) => folder.to_string_lossy().into_owned(),
                _ => unreachable!(),
            },
            reference: match &parsed {
                Address::Git { reference, .. } => reference.clone(),
                _ => None,
            },
            commit: commit.clone(),
            path,
            aliases: alias.clone().into_iter().collect(),
            approval,
            denied: false,
        };
        additions.push((formal.clone(), entry));
        names.push((formal, name));
    }
    let mut next = ctx.user.add(additions);
    if let (Some((_, install)), Some(commit)) = (&clone, &commit) {
        let keys: Vec<_> = next
            .templates
            .iter()
            .filter(|(_, entry)| entry.path.starts_with(install))
            .map(|(formal, _)| formal.clone())
            .collect();
        for formal in keys {
            next = next
                .set_commit(&formal, commit.clone())
                .map_err(CatalogError::text)?;
        }
    }
    let mut merged = ctx.registry.clone();
    for (formal, entry) in &next.templates {
        merged.entries.insert(
            formal.clone(),
            registry::Listed {
                formal_name: formal.clone(),
                entry: entry.clone(),
                layer: Layer::User,
                approval: entry.approval.clone(),
            },
        );
    }
    merged.check_aliases().map_err(CatalogError::text)?;
    let mut lines = Vec::new();
    for (formal, name) in &names {
        let matching: Vec<_> = merged
            .entries
            .values()
            .filter(|e| e.entry.name == *name)
            .collect();
        if matching.len() > 1 {
            for entry in matching {
                lines.push(Event::SharedShortName {
                    name: name.clone(),
                    formal: entry.formal_name.clone(),
                });
            }
        }
        lines.push(Event::Added(formal.clone()));
    }
    install_staged(ctx, &clone.into_iter().collect::<Vec<_>>(), &next)?;
    Ok(lines)
}
/// Whether an entry's stored approval still matches its installed executable
/// surface. An entry with no approval, or one whose installed template cannot
/// be loaded or read, is not trusted.
fn effective_trust(listed: &registry::Listed) -> bool {
    let Some(approval) = listed.approval.as_ref() else {
        return false;
    };
    let Ok(template) = Template::load(&listed.entry.path) else {
        return false;
    };
    let Ok(surface) = crate::HookSurface::of(&template) else {
        return false;
    };
    matches!(
        crate::evaluate_trust(Some(approval), &surface.digest()),
        crate::Trust::Trusted
    )
}
pub fn list(ctx: &Catalog, filter: Option<Layer>) -> Result<Listing, CatalogError> {
    let entries: Vec<registry::Listed> = match filter {
        None => ctx.registry.entries.values().cloned().collect(),
        Some(Layer::System) | Some(Layer::User) => {
            let layer = filter.unwrap();
            let file = if layer == Layer::System {
                &ctx.system
            } else {
                &ctx.user
            };
            file.templates
                .iter()
                .map(|(formal, entry)| registry::Listed {
                    formal_name: formal.clone(),
                    entry: entry.clone(),
                    layer,
                    approval: entry.approval.clone(),
                })
                .collect()
        }
        Some(Layer::Local) => {
            let mut result: Vec<_> = ctx
                .local
                .templates
                .keys()
                .filter_map(|formal| ctx.registry.entries.get(formal))
                .cloned()
                .collect();
            result.extend(
                ctx.registry
                    .entries
                    .values()
                    .filter(|entry| {
                        entry.layer == Layer::Discovered
                            && !ctx.local.templates.contains_key(&entry.formal_name)
                            && ctx
                                .config
                                .local_templates_paths
                                .iter()
                                .any(|root| entry.entry.path.starts_with(root))
                    })
                    .cloned(),
            );
            result
        }
        Some(Layer::Discovered) => Vec::new(),
    };
    // D3: the unfiltered listing carries one read-only descriptor for the
    // bundled demo, in both formats, but only when no registry entry occupies the
    // reserved name — that is exactly when the fallback would resolve. It is
    // presentation only: no persisted entry and no resolution source, marked
    // `bundled` in `--json` with no registry layer, source, ref, or path, and
    // resolution never consults it.
    let show_bundled = filter.is_none()
        && matches!(
            ctx.registry.resolve(bundled::RESERVED),
            Err(ResolveError::NotFound(_))
        );
    Ok(Listing {
        entries: entries
            .into_iter()
            .map(|listed| {
                let trusted = effective_trust(&listed);
                ListedTemplate { listed, trusted }
            })
            .collect(),
        bundled: show_bundled.then(bundled::listing_row),
    })
}
pub fn update(ctx: &Catalog, template: Option<String>) -> Result<Vec<Event>, CatalogError> {
    let selected: Vec<_> = if let Some(name) = template {
        vec![ctx.resolve_user(&name)?]
    } else {
        ctx.user
            .templates
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    if selected.is_empty()
        || !selected
            .iter()
            .any(|(_, entry)| entry.source.contains("://") || entry.source.starts_with("git@"))
    {
        return Err(CatalogError::text("no installed git template matches"));
    }
    let mut next = ctx.user.clone();
    let mut lines = Vec::new();
    let mut updated = Vec::new();
    let mut fetched = std::collections::HashSet::new();
    let mut staged = Vec::new();
    for (formal, entry) in selected {
        if !entry.source.contains("://") && !entry.source.starts_with("git@") {
            continue;
        }
        let install = ctx
            .dirs
            .user_data
            .join("repos")
            .join(source::install_key(&formal));
        let address = Address::Git {
            repo: entry.source.clone(),
            reference: entry.reference.clone(),
            path: None,
        };
        if fetched.contains(&install) {
            continue;
        }
        let (clone, result) = clone_sibling(&address, &install)?;
        if matches!(result.reference_kind, RefKind::Tag | RefKind::Commit) {
            lines.push(Event::Pinned(formal.clone()));
            continue;
        }
        let commit = result.commit;
        fetched.insert(install.clone());
        staged.push((clone, install.clone()));
        let keys: Vec<_> = next
            .templates
            .iter()
            .filter(|(_, entry)| entry.path.starts_with(&install))
            .map(|(formal, _)| formal.clone())
            .collect();
        for key in keys {
            next = next
                .set_commit(&key, commit.clone())
                .map_err(CatalogError::text)?;
            lines.push(Event::Updated(key.clone()));
            updated.push(key);
        }
    }
    install_staged(ctx, &staged, &next)?;
    // Effective trust is always the live compare, so the stored approval is
    // never rewritten here. An update that changes a template's executable
    // surface lapses its approval; the next apply lists the change and exits 3.
    for formal in &updated {
        let Some(entry) = next.templates.get(formal) else {
            continue;
        };
        let Some(approval) = entry.approval.as_ref() else {
            continue;
        };
        let listed = registry::Listed {
            formal_name: formal.clone(),
            entry: entry.clone(),
            layer: Layer::User,
            approval: Some(approval.clone()),
        };
        if !effective_trust(&listed) {
            lines.push(Event::ApprovalLapsed(formal.clone()));
        }
    }
    Ok(lines)
}
pub fn remove(ctx: &Catalog, template: &str) -> Result<Vec<Event>, CatalogError> {
    let (formal, entry) = ctx.resolve_user(template)?;
    let next = ctx.user.remove(&formal).map_err(CatalogError::text)?;
    ctx.write(&next)?;
    if entry.source.contains("://") || entry.source.starts_with("git@") {
        let install = ctx
            .dirs
            .user_data
            .join("repos")
            .join(source::install_key(&formal));
        if !next
            .templates
            .values()
            .any(|e| e.path.starts_with(&install))
            && install.exists()
        {
            fs::remove_dir_all(install).map_err(CatalogError::text)?;
        }
    }
    Ok(vec![Event::Removed(formal)])
}
/// Approve the current installed executable surface of an installed template.
/// Reads only the installed folder — never fetches, reclones, or moves the
/// commit — and writes the approval to the user registry alone.
pub fn trust(ctx: &Catalog, name: &str) -> Result<Vec<Event>, CatalogError> {
    let (formal, entry) = ctx.resolve_user(name)?;
    let template = Template::load(&entry.path).map_err(CatalogError::text)?;
    let surface = crate::HookSurface::of(&template).map_err(|error| CatalogError::Surface {
        formal: formal.clone(),
        error,
    })?;
    let live = surface.digest();
    // Idempotency is defined through the one trust rule: a surface that changed
    // since approval is not "already trusted"; the command holds no `==`. A
    // denial (a hand-edited/legacy `trusted: false` beside a matching approval)
    // makes the entry effectively untrusted, so the grant runs to clear it
    // rather than reporting "already trusted".
    if !entry.denied
        && matches!(
            crate::evaluate_trust(entry.approval.as_ref(), &live),
            crate::Trust::Trusted
        )
    {
        return Ok(vec![Event::Trusted {
            formal,
            already: true,
            surface: None,
        }]);
    }
    let next = ctx
        .user
        .set_approval(&formal, Some(live))
        .map_err(CatalogError::text)?;
    ctx.write(&next)?;
    Ok(vec![Event::Trusted {
        formal,
        already: false,
        surface: Some(surface),
    }])
}
/// Revoke approval of an installed template's hooks. Reads nothing on disk, so
/// it cannot fail on the installed surface; it clears the approval and persists
/// a denial so a lower registry layer's approval cannot reappear.
pub fn untrust(ctx: &Catalog, name: &str) -> Result<Vec<Event>, CatalogError> {
    let (formal, entry) = ctx.resolve_user(name)?;
    if entry.approval.is_none() && entry.denied {
        return Ok(vec![Event::Untrusted {
            formal,
            already: true,
        }]);
    }
    let next = ctx
        .user
        .set_approval(&formal, None)
        .map_err(CatalogError::text)?;
    ctx.write(&next)?;
    Ok(vec![Event::Untrusted {
        formal,
        already: false,
    }])
}
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.split('-').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
pub fn remove_alias(ctx: &Catalog, alias: &str) -> Result<Vec<Event>, CatalogError> {
    let next = ctx.user.remove_alias(alias).map_err(CatalogError::text)?;
    ctx.write(&next)?;
    Ok(vec![Event::AliasRemoved(alias.to_owned())])
}
pub fn add_alias(ctx: &Catalog, template: &str, alias: &str) -> Result<Vec<Event>, CatalogError> {
    let template = template.to_owned();
    let alias = alias.to_owned();
    if !valid_alias(&alias) {
        return Err(CatalogError::text("invalid alias"));
    }
    let (formal, _) = ctx.resolve_user(&template)?;
    if ctx.registry.entries.contains_key(&alias)
        || ctx
            .registry
            .entries
            .values()
            .any(|e| e.entry.aliases.contains(&alias))
    {
        let removable = ctx
            .user
            .templates
            .values()
            .any(|entry| entry.aliases.contains(&alias));
        return Err(CatalogError::AliasInUse {
            template: template.clone(),
            alias: alias.clone(),
            removable,
        });
    }
    let next = ctx
        .user
        .add_alias(&formal, &alias)
        .map_err(CatalogError::text)?;
    let warning = ctx
        .registry
        .entries
        .values()
        .any(|e| e.formal_name != formal && e.entry.name == alias);
    // The user layer is the only writable layer.
    ctx.write(&next)?;
    let mut lines = Vec::new();
    if warning {
        lines.push(Event::AliasShadowsShortName(alias.clone()));
    }
    lines.push(Event::AliasAdded { alias, formal });
    Ok(lines)
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error(message) => message.fmt(f),
            Self::UserOnly(name) => write!(
                f,
                "template {name} is not installed in the writable user registry"
            ),
            Self::AddressRequired { name, .. } => {
                write!(f, "a template source address is required for {name}")
            }
            Self::AliasMultiple { alias, addresses } => write!(
                f,
                "alias {alias} identifies multiple templates: {}",
                addresses.join(", ")
            ),
            Self::AliasInUse { alias, .. } => write!(f, "alias {alias} is already in use"),
            Self::Surface { formal, error } => {
                write!(f, "cannot inspect executable surface of {formal}: {error}")
            }
            Self::Ambiguous { name, matches } => {
                write!(f, "template {name} is ambiguous: {}", matches.join(", "))
            }
        }
    }
}
impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Surface { error, .. } => Some(error),
            _ => None,
        }
    }
}
