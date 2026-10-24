use crate::{Dirs, cli::guidance};
use clap::{Args, Subcommand};
use std::path::Path;
use toha::catalog::{self, Catalog, CatalogError, Event};
use toha::registry::Layer;

#[derive(Args)]
pub struct TemplatesArgs {
    #[command(subcommand)]
    command: TemplatesCommand,
}
#[derive(Subcommand)]
enum TemplatesCommand {
    /// Install every template in a repository, or the one template at #<path>.
    Add {
        /// Git address or local folder.
        address: String,
        /// Alias for the template; fails when the address holds more than one template.
        #[arg(short, long)]
        alias: Option<String>,
        /// Approve the added templates' current hooks in the user registry.
        #[arg(long)]
        trust: bool,
    },
    /// List available templates with formal name, short name, aliases, and trust.
    List {
        /// Only templates from the local layer.
        #[arg(short, long, conflicts_with_all=["system", "user"])]
        local: bool,
        /// Only templates from the system layer.
        #[arg(short, long, conflicts_with = "user")]
        system: bool,
        /// Only templates from the user layer.
        #[arg(short, long)]
        user: bool,
        /// Print the list as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Move installed git templates that follow a branch to its newest commit.
    Update {
        /// Template to update; all installed git templates when absent.
        template: Option<String>,
    },
    /// Uninstall a template and delete its entry from the user registry.
    Remove {
        /// Template to uninstall.
        template: String,
    },
    /// Add or remove an alias in the user registry.
    Alias {
        /// Formal or short name of the template.
        #[arg(requires = "alias", required_unless_present = "remove")]
        template: Option<String>,
        /// Alias to add.
        #[arg(requires = "template")]
        alias: Option<String>,
        /// Alias to remove.
        #[arg(short, long, conflicts_with_all=["template", "alias"], value_name = "ALIAS")]
        remove: Option<String>,
    },
    /// Approve the installed hooks of a template so its apply runs them.
    Trust {
        /// Formal, short, or alias name of an installed template.
        template: String,
    },
    /// Revoke approval of a template's hooks.
    Untrust {
        /// Formal, short, or alias name of an installed template.
        template: String,
    },
}

#[derive(Debug)]
pub enum CommandError {
    Error(String),
    Ambiguous { name: String, matches: Vec<String> },
}
impl From<CatalogError> for CommandError {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::Error(message) => Self::Error(message),
            CatalogError::Ambiguous { name, matches } => Self::Ambiguous { name, matches },
            CatalogError::UserOnly(name) => Self::Error(guidance::not_in_user_registry(&name)),
            CatalogError::AddressRequired {
                name,
                folder_exists,
            } => Self::Error(guidance::add_needs_address(&name, folder_exists)),
            CatalogError::AliasMultiple { alias, addresses } => {
                Self::Error(guidance::alias_needs_one_template(
                    &alias,
                    &addresses
                        .iter()
                        .map(|address| guidance::formal_for("templates add", address).to_string())
                        .collect::<Vec<_>>(),
                ))
            }
            CatalogError::AliasInUse {
                template,
                alias,
                removable,
            } => Self::Error(guidance::alias_in_use(&template, &alias, removable)),
            CatalogError::Surface { formal, error } => {
                Self::Error(guidance::surface_unreadable(&formal, &error))
            }
        }
    }
}
fn events(events: Vec<Event>) -> Vec<String> {
    events
        .into_iter()
        .map(|event| match event {
            Event::Added(formal) => format!("added {formal}"),
            Event::SharedShortName { name, formal } => {
                format!("shared short name {name}: toha templates alias {formal} <alias>")
            }
            Event::Pinned(formal) => format!("pinned {formal}"),
            Event::Updated(formal) => format!("updated {formal}"),
            Event::ApprovalLapsed(formal) => format!("{formal} (hooks changed; approval lapsed)"),
            Event::Removed(formal) => format!("removed {formal}"),
            Event::AliasRemoved(alias) => format!("removed alias {alias}"),
            Event::AliasAdded { alias, formal } => format!("added alias {alias} for {formal}"),
            Event::AliasShadowsShortName(alias) => {
                format!("warning: alias {alias} is another template's short name")
            }
            Event::Trusted {
                formal,
                already,
                surface,
            } => {
                if let Some(surface) = surface {
                    eprintln!("{}", guidance::approved_surface(&surface));
                }
                format!("{}trusted {formal}", if already { "already " } else { "" })
            }
            Event::Untrusted { formal, already } => format!(
                "{}untrusted {formal}",
                if already { "already " } else { "" }
            ),
        })
        .collect()
}
fn list(ctx: &Catalog, filter: Option<Layer>, json: bool) -> Result<Vec<String>, CommandError> {
    let listing = catalog::list(ctx, filter)?;
    if json {
        let mut values: Vec<_> = listing.entries.iter().map(|row| { let e=&row.listed; serde_json::json!({"formal_name":e.formal_name,"name":e.entry.name,"aliases":e.entry.aliases,"trusted":row.trusted,"layer":e.layer,"source":e.entry.source,"ref":e.entry.reference,"commit":e.entry.commit,"path":e.entry.path}) }).collect();
        if let Some(row) = listing.bundled {
            values.push(serde_json::json!({"formal_name":row.formal,"name":row.short,"aliases":[],"trusted":row.trusted,"commit":toha::bundled::commit(),"bundled":true}));
        }
        Ok(vec![
            serde_json::to_string(&values).map_err(|e| CommandError::Error(e.to_string()))?,
        ])
    } else {
        let mut lines = vec!["FORMAL NAME\tSHORT NAME\tALIASES\tTRUSTED".into()];
        lines.extend(listing.entries.iter().map(|row| {
            let e = &row.listed;
            format!(
                "{}\t{}\t{}\t{}",
                e.formal_name,
                e.entry.name,
                e.entry.aliases.join(","),
                row.trusted
            )
        }));
        if let Some(row) = listing.bundled {
            lines.push(format!("{}\t{}\t\t{}", row.formal, row.short, row.trusted));
        }
        Ok(lines)
    }
}
/// The same `templates` command with a formal name in place of an ambiguous
/// template name.
pub fn retry(args: &TemplatesArgs) -> impl Fn(&str) -> String + use<> {
    let (command, alias) = match &args.command {
        TemplatesCommand::Update { .. } => ("templates update", None),
        TemplatesCommand::Remove { .. } => ("templates remove", None),
        TemplatesCommand::Alias { alias, .. } => ("templates alias", alias.clone()),
        TemplatesCommand::Trust { .. } => ("templates trust", None),
        TemplatesCommand::Untrust { .. } => ("templates untrust", None),
        TemplatesCommand::Add { .. } | TemplatesCommand::List { .. } => ("templates list", None),
    };
    move |formal| {
        let mut operands = vec![guidance::value(guidance::formal_for(command, formal))];
        operands.extend(alias.as_deref().map(guidance::value));
        guidance::toha(command, &[], &operands)
    }
}
pub fn run(args: TemplatesArgs, dirs: Dirs, cwd: &Path) -> Result<Vec<String>, CommandError> {
    let ctx = Catalog::load(dirs, cwd)?;
    for warning in &ctx.warnings {
        eprintln!("warning: {warning}");
    }
    match args.command {
        TemplatesCommand::Add {
            address,
            alias,
            trust,
        } => catalog::add(&ctx, &address, alias, trust, cwd)
            .map(events)
            .map_err(Into::into),
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
        TemplatesCommand::Update { template } => catalog::update(&ctx, template)
            .map(events)
            .map_err(Into::into),
        TemplatesCommand::Remove { template } => catalog::remove(&ctx, &template)
            .map(events)
            .map_err(Into::into),
        TemplatesCommand::Alias {
            template,
            alias: name,
            remove,
        } => catalog::alias(&ctx, template, name, remove)
            .map(events)
            .map_err(Into::into),
        TemplatesCommand::Trust { template } => catalog::trust(&ctx, &template)
            .map(events)
            .map_err(Into::into),
        TemplatesCommand::Untrust { template } => catalog::untrust(&ctx, &template)
            .map(events)
            .map_err(Into::into),
    }
}
