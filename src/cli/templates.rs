use crate::Dirs;
use clap::{Args, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toha::{
    Template, config,
    registry::{self, Entry, Layer, Registry, RegistryFile, ResolveError},
    source::{self, Address, RefKind},
};

#[derive(Args)]
pub struct TemplatesArgs {
    #[command(subcommand)]
    command: TemplatesCommand,
}
#[derive(Subcommand)]
enum TemplatesCommand {
    Add {
        address: String,
        #[arg(short, long)]
        alias: Option<String>,
        #[arg(long)]
        trust: bool,
    },
    List {
        #[arg(short, long, conflicts_with_all=["system", "user"])]
        local: bool,
        #[arg(short, long, conflicts_with = "user")]
        system: bool,
        #[arg(short, long)]
        user: bool,
        #[arg(long)]
        json: bool,
    },
    Update {
        template: Option<String>,
    },
    Remove {
        template: String,
    },
    Alias {
        #[arg(requires = "alias", required_unless_present = "remove")]
        template: Option<String>,
        #[arg(requires = "template")]
        alias: Option<String>,
        #[arg(short, long, conflicts_with_all=["template", "alias"])]
        remove: Option<String>,
    },
}
#[derive(Debug)]
pub enum CommandError {
    Error(String),
    Ambiguous { name: String, matches: Vec<String> },
}
impl From<ResolveError> for CommandError {
    fn from(value: ResolveError) -> Self {
        match value {
            ResolveError::NotFound(name) => Self::Error(format!("template not found: {name}")),
            ResolveError::Ambiguous { name, matches } => Self::Ambiguous { name, matches },
        }
    }
}
impl CommandError {
    fn text(message: impl ToString) -> Self {
        Self::Error(message.to_string())
    }
}
struct Context {
    dirs: Dirs,
    config: config::Config,
    system: RegistryFile,
    user: RegistryFile,
    local: RegistryFile,
    registry: Registry,
}
impl Context {
    fn load(dirs: Dirs, cwd: &Path) -> Result<Self, CommandError> {
        let config = config::load(&dirs.config_paths(), cwd).map_err(CommandError::text)?;
        let system = RegistryFile::load(&dirs.system_data.join("templates.yml"), Layer::System)
            .map_err(CommandError::text)?;
        let user = RegistryFile::load(&dirs.user_data.join("templates.yml"), Layer::User)
            .map_err(CommandError::text)?;
        let local = if let Some(root) = config.local_templates_paths.first() {
            RegistryFile::load(&root.join("templates.yml"), Layer::Local)
                .map_err(CommandError::text)?
        } else {
            RegistryFile::default()
        };
        let mut registry = Registry::merge(&system, &user, &local).map_err(CommandError::text)?;
        registry
            .discover(&config.templates_paths, &dirs.user_data)
            .map_err(CommandError::text)?;
        registry
            .apply_local_aliases(&local)
            .map_err(CommandError::text)?;
        Ok(Self {
            dirs,
            config,
            system,
            user,
            local,
            registry,
        })
    }
    fn write(&self, file: &RegistryFile) -> Result<(), CommandError> {
        file.write_atomic(&self.dirs.user_data.join("templates.yml"))
            .map_err(CommandError::text)
    }
    fn resolve_user(&self, name: &str) -> Result<(String, Entry), CommandError> {
        let resolved = self.registry.resolve(name)?;
        self.user
            .templates
            .get(&resolved.formal_name)
            .cloned()
            .map(|entry| (resolved.formal_name, entry))
            .ok_or_else(|| CommandError::text(format!("not installed in user registry: {name}")))
    }
}
fn find_templates(root: &Path, selected: Option<&str>) -> Result<Vec<PathBuf>, CommandError> {
    let start = if let Some(path) = selected {
        root.join(path)
    } else {
        root.to_path_buf()
    };
    let start = start.canonicalize().map_err(CommandError::text)?;
    if !start.starts_with(root) {
        return Err(CommandError::text("template path leaves repository"));
    }
    let mut found = Vec::new();
    fn visit(
        dir: &Path,
        root: &Path,
        seen: &mut std::collections::HashSet<PathBuf>,
        found: &mut Vec<PathBuf>,
    ) -> Result<(), CommandError> {
        let canonical = dir.canonicalize().map_err(CommandError::text)?;
        if !canonical.starts_with(root) {
            return Err(CommandError::text("template path leaves repository"));
        }
        if !seen.insert(canonical.clone()) {
            return Ok(());
        }
        if canonical.join("template.yml").is_file() {
            found.push(canonical);
            return Ok(());
        }
        for child in fs::read_dir(&canonical).map_err(CommandError::text)? {
            let child = child.map_err(CommandError::text)?.path();
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
        root,
        &mut std::collections::HashSet::new(),
        &mut found,
    )?;
    found.sort();
    if selected.is_some() && found.len() != 1 {
        return Err(CommandError::text("#path must select exactly one template"));
    }
    if found.is_empty() {
        return Err(CommandError::text("no template found"));
    }
    Ok(found)
}
fn clone_sibling(
    address: &Address,
    install: &Path,
) -> Result<(tempfile::TempDir, source::Fetched), CommandError> {
    let parent = install
        .parent()
        .ok_or_else(|| CommandError::text("install path has no parent"))?;
    fs::create_dir_all(parent).map_err(CommandError::text)?;
    let container = tempfile::Builder::new()
        .prefix(".toha-clone-")
        .tempdir_in(parent)
        .map_err(CommandError::text)?;
    let fetched =
        source::fetch(address, &container.path().join("repo")).map_err(CommandError::text)?;
    Ok((container, fetched))
}
struct SwappedClone {
    install: PathBuf,
    old: PathBuf,
    had_old: bool,
}
impl SwappedClone {
    fn rollback(self) -> Result<(), CommandError> {
        fs::remove_dir_all(&self.install).map_err(CommandError::text)?;
        if self.had_old {
            fs::rename(&self.old, &self.install).map_err(CommandError::text)?;
        }
        Ok(())
    }
    fn finish(self) -> Result<(), CommandError> {
        if self.had_old {
            fs::remove_dir_all(self.old).map_err(CommandError::text)?;
        }
        Ok(())
    }
}
fn swap_clone(temp: &Path, install: &Path) -> Result<SwappedClone, CommandError> {
    let old = install.with_extension("toha-old");
    if old.exists() {
        return Err(CommandError::text(format!(
            "old clone backup exists: {}",
            old.display()
        )));
    }
    let had_old = install.exists();
    if had_old {
        fs::rename(install, &old).map_err(CommandError::text)?;
    }
    if let Err(error) = fs::rename(temp, install) {
        if had_old {
            fs::rename(&old, install).map_err(CommandError::text)?;
        }
        return Err(CommandError::text(error));
    }
    Ok(SwappedClone {
        install: install.into(),
        old,
        had_old,
    })
}
fn install_staged(
    ctx: &Context,
    staged: &[(tempfile::TempDir, PathBuf)],
    registry: &RegistryFile,
) -> Result<(), CommandError> {
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
fn add(
    ctx: &Context,
    address: &str,
    alias: Option<String>,
    trust: bool,
    cwd: &Path,
) -> Result<Vec<String>, CommandError> {
    if alias.as_deref().is_some_and(|value| !valid_alias(value)) {
        return Err(CommandError::text("invalid alias"));
    }
    let parsed = source::parse(address, &ctx.config.hosts, cwd, &ctx.dirs.home)
        .map_err(CommandError::text)?;
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
        Address::Name(_) => return Err(CommandError::text("add requires a git address or folder")),
    };
    let selected = if let Address::Git { path, .. } = &parsed {
        path.as_deref()
    } else {
        None
    };
    let folders = find_templates(&root, selected)?;
    if alias.is_some() && folders.len() != 1 {
        return Err(CommandError::text("--alias requires exactly one template"));
    }
    let mut additions = Vec::new();
    let mut names = Vec::new();
    for folder in folders {
        let loaded = Template::load(&folder).map_err(CommandError::text)?;
        let name = loaded.name.clone();
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
            let relative = folder.strip_prefix(&root).map_err(CommandError::text)?;
            if relative.as_os_str().is_empty() {
                base
            } else {
                format!("{base}#{}", relative.to_string_lossy().replace('\\', "/"))
            }
        };
        let path = if let Some((_, install)) = &clone {
            install.join(folder.strip_prefix(&root).map_err(CommandError::text)?)
        } else {
            folder.clone()
        };
        let mut entry = Entry {
            name: name.clone(),
            source: match &parsed {
                Address::Git { repo, .. } => repo.clone(),
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
            trusted: trust,
        };
        if let Some(old) = ctx.user.templates.get(&formal) {
            for old_alias in &old.aliases {
                if !entry.aliases.contains(old_alias) {
                    entry.aliases.push(old_alias.clone());
                }
            }
            entry.trusted |= old.trusted;
        }
        additions.push((formal.clone(), entry));
        names.push((formal, name));
    }
    let mut next = ctx.user.clone();
    for (formal, entry) in additions {
        next.templates.insert(formal, entry);
    }
    if let (Some((_, install)), Some(commit)) = (&clone, &commit) {
        for entry in next
            .templates
            .values_mut()
            .filter(|entry| entry.path.starts_with(install))
        {
            entry.commit = Some(commit.clone());
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
                trusted: entry.trusted,
            },
        );
    }
    merged.check_aliases().map_err(CommandError::text)?;
    let mut lines = Vec::new();
    for (formal, name) in &names {
        let matching: Vec<_> = merged
            .entries
            .values()
            .filter(|e| e.entry.name == *name)
            .collect();
        if matching.len() > 1 {
            for entry in matching {
                lines.push(format!(
                    "shared short name {name}: toha templates alias {} <alias>",
                    entry.formal_name
                ));
            }
        }
        lines.push(format!("added {formal}"));
    }
    install_staged(ctx, &clone.into_iter().collect::<Vec<_>>(), &next)?;
    Ok(lines)
}
fn list(ctx: &Context, filter: Option<Layer>, json: bool) -> Result<Vec<String>, CommandError> {
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
                    trusted: entry.trusted,
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
    if json {
        let values: Vec<_> = entries.iter().map(|e| serde_json::json!({ "formal_name": e.formal_name, "name": e.entry.name, "aliases": e.entry.aliases, "trusted": e.trusted, "layer": e.layer, "source": e.entry.source, "ref": e.entry.reference, "commit": e.entry.commit, "path": e.entry.path })).collect();
        return Ok(vec![
            serde_json::to_string(&values).map_err(CommandError::text)?,
        ]);
    }
    let mut lines = vec!["FORMAL NAME\tSHORT NAME\tALIASES\tTRUSTED".into()];
    lines.extend(entries.iter().map(|e| {
        format!(
            "{}\t{}\t{}\t{}",
            e.formal_name,
            e.entry.name,
            e.entry.aliases.join(","),
            e.trusted
        )
    }));
    Ok(lines)
}
fn update(ctx: &Context, template: Option<String>) -> Result<Vec<String>, CommandError> {
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
        return Err(CommandError::text("no installed git template matches"));
    }
    let mut next = ctx.user.clone();
    let mut lines = Vec::new();
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
            lines.push(format!("pinned {formal}"));
            continue;
        }
        let commit = result.commit;
        fetched.insert(install.clone());
        staged.push((clone, install.clone()));
        for (key, value) in next
            .templates
            .iter_mut()
            .filter(|(_, v)| v.path.starts_with(&install))
        {
            value.commit = Some(commit.clone());
            lines.push(format!("updated {key}"));
        }
    }
    install_staged(ctx, &staged, &next)?;
    Ok(lines)
}
fn remove(ctx: &Context, template: &str) -> Result<Vec<String>, CommandError> {
    let (formal, entry) = ctx.resolve_user(template)?;
    let next = ctx.user.remove(&formal).map_err(CommandError::text)?;
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
            fs::remove_dir_all(install).map_err(CommandError::text)?;
        }
    }
    Ok(vec![format!("removed {formal}")])
}
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.split('-').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
fn alias(
    ctx: &Context,
    template: Option<String>,
    name: Option<String>,
    remove: Option<String>,
) -> Result<Vec<String>, CommandError> {
    if let Some(alias) = remove {
        if template.is_some() || name.is_some() {
            return Err(CommandError::text(
                "-r cannot be combined with a template or alias",
            ));
        }
        let next = ctx.user.remove_alias(&alias).map_err(CommandError::text)?;
        ctx.write(&next)?;
        return Ok(vec![format!("removed alias {alias}")]);
    }
    let template = template.ok_or_else(|| CommandError::text("template and alias are required"))?;
    let alias = name.ok_or_else(|| CommandError::text("template and alias are required"))?;
    if !valid_alias(&alias) {
        return Err(CommandError::text("invalid alias"));
    }
    let (formal, _) = ctx.resolve_user(&template)?;
    if ctx.registry.entries.contains_key(&alias)
        || ctx
            .registry
            .entries
            .values()
            .any(|e| e.entry.aliases.contains(&alias))
    {
        return Err(CommandError::text("alias is in use"));
    }
    let next = ctx
        .user
        .add_alias(&formal, &alias)
        .map_err(CommandError::text)?;
    let warning = ctx
        .registry
        .entries
        .values()
        .any(|e| e.formal_name != formal && e.entry.name == alias);
    // The user layer is the only writable layer.
    ctx.write(&next)?;
    let mut lines = Vec::new();
    if warning {
        lines.push(format!(
            "warning: alias {alias} is another template's short name"
        ));
    }
    lines.push(format!("added alias {alias} for {formal}"));
    Ok(lines)
}
pub fn run(args: TemplatesArgs, dirs: Dirs, cwd: &Path) -> Result<Vec<String>, CommandError> {
    let ctx = Context::load(dirs, cwd)?;
    match args.command {
        TemplatesCommand::Add {
            address,
            alias,
            trust,
        } => add(&ctx, &address, alias, trust, cwd),
        TemplatesCommand::List {
            local,
            system,
            user,
            json,
        } => list(
            &ctx,
            if local {
                Some(Layer::Local)
            } else if system {
                Some(Layer::System)
            } else if user {
                Some(Layer::User)
            } else {
                None
            },
            json,
        ),
        TemplatesCommand::Update { template } => update(&ctx, template),
        TemplatesCommand::Remove { template } => remove(&ctx, &template),
        TemplatesCommand::Alias {
            template,
            alias: name,
            remove,
        } => alias(&ctx, template, name, remove),
    }
}
