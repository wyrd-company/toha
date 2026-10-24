// ---
// relationships:
//   implements: command-line-interface
// ---
use super::Outcome;
use clap::Subcommand;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Subcommand)]
pub enum Command {
    /// List the embedded skills as JSON objects with name and description.
    List,
    /// Print the SKILL.md of a skill, or another file of it.
    View {
        /// Name of the skill.
        name: String,
        /// Path of a file inside the skill.
        #[arg(short, long, conflicts_with = "export")]
        path: Option<PathBuf>,
        /// Write the whole skill directory to <PATH>/<NAME>/.
        #[arg(short, long, value_name = "PATH")]
        export: Option<PathBuf>,
    },
}

pub fn run(command: Command) -> Outcome {
    match command {
        Command::List => match toha::assets::list() {
            Ok(items) => Outcome::Document(serde_json::to_value(items).expect("skill metadata"), 0),
            Err(e) => Outcome::Error(e),
        },
        Command::View { name, path, export } => {
            if let Some(destination) = export {
                return match toha::assets::export(&name, &destination) {
                    Ok(()) => Outcome::Saved(0),
                    Err(e) => Outcome::Error(e),
                };
            }
            match toha::assets::read(&name, path.as_deref().unwrap_or(Path::new("SKILL.md"))) {
                Ok(bytes) => match std::io::stdout().write_all(bytes) {
                    Ok(()) => Outcome::Saved(0),
                    Err(e) => Outcome::Error(e.to_string()),
                },
                Err(e) => Outcome::Error(e),
            }
        }
    }
}
