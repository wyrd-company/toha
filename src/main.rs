// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use toha::{
    AnswerError, Applied, ApplyOptions, Id, Interview, Plan, RawAnswer, RawAnswers, Seed, Template,
};

#[derive(Parser)]
#[command(name = "toha")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Apply {
        #[arg(num_args = 1..=2)]
        paths: Vec<String>,
        #[arg(short = 'A', long)]
        answers: Option<String>,
        #[arg(short, long)]
        force: bool,
    },
}

enum Outcome {
    Written(Vec<toha::TargetPath>, Vec<String>),
    Error(String),
    Incomplete(String),
}
impl Outcome {
    fn finish(self) -> ExitCode {
        match self {
            Self::Written(paths, messages) => {
                for message in messages {
                    println!("{message}");
                }
                for path in paths {
                    println!("{path}");
                }
                ExitCode::SUCCESS
            }
            Self::Error(message) => {
                eprintln!("{message}");
                ExitCode::from(1)
            }
            Self::Incomplete(message) => {
                eprintln!("{message}");
                ExitCode::from(4)
            }
        }
    }
}

fn local_folder(value: &str) -> Result<PathBuf, String> {
    if !(value.starts_with('.') || value.starts_with('/') || value.starts_with('~')) {
        return Err("only local folder templates are supported yet".into());
    }
    if let Some(rest) = value.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            return std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(rest.trim_start_matches('/')))
                .ok_or_else(|| "home directory is unavailable".into());
        }
    }
    Ok(PathBuf::from(value))
}

fn read_answers(path: &str) -> Result<RawAnswers, String> {
    let text = if path == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        text
    } else {
        fs::read_to_string(path).map_err(|error| error.to_string())?
    };
    let values: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    values
        .into_iter()
        .map(|(key, value)| Id::parse(&key).map(|id| (id, RawAnswer(value))))
        .collect()
}

fn seed() -> Seed {
    let now = std::env::var("TOHA_NOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(jiff::Zoned::now);
    Seed {
        now,
        defaults: indexmap::IndexMap::new(),
    }
}

fn run(template: Option<String>, path: &Path, answers: Option<String>, force: bool) -> Outcome {
    let Some(template) = template else {
        return Outcome::Error("template folder is required".into());
    };
    let folder = match local_folder(&template) {
        Ok(folder) => folder,
        Err(error) => return Outcome::Error(error),
    };
    let Some(answers) = answers else {
        return Outcome::Error("interactive prompts are not supported yet".into());
    };
    let template = match Template::load(&folder) {
        Ok(template) => template,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let mut raw = match read_answers(&answers) {
        Ok(raw) => raw,
        Err(error) => return Outcome::Error(error),
    };
    let mut interview = match Interview::start(&template, seed()) {
        Ok(interview) => interview,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let completed = loop {
        match interview {
            Interview::Complete(completed) => break completed,
            Interview::Asking(pending) => {
                interview = match pending.answer(std::mem::take(&mut raw)) {
                    Ok(interview) => interview,
                    Err(AnswerError::Rejected { rejections, .. }) => {
                        return Outcome::Incomplete(
                            rejections
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                    Err(AnswerError::Eval(error)) => return Outcome::Error(error.to_string()),
                };
            }
        }
    };
    let plan = match Plan::build(&template, &completed, path) {
        Ok(plan) => plan,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    match plan.apply(path, ApplyOptions { force }) {
        Ok(Applied::Written(paths)) => Outcome::Written(paths, completed.messages),
        Err(error) => Outcome::Error(error.to_string()),
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Apply {
            paths,
            answers,
            force,
        } => {
            let (template, path) = match paths.as_slice() {
                [path] => (None, PathBuf::from(path)),
                [template, path] => (Some(template.clone()), PathBuf::from(path)),
                _ => unreachable!("clap enforces one or two positional arguments"),
            };
            run(template, &path, answers, force).finish()
        }
    }
}
