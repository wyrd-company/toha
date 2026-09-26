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
    hook::ProcessRunner,
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
        #[arg(short = 'd', long)]
        dry_run: bool,
        #[arg(long)]
        trust: bool,
    },
}

enum Outcome {
    Written(Vec<String>),
    Error(String),
    Incomplete(String),
    NeedsTrust(String),
}
impl Outcome {
    fn finish(self) -> ExitCode {
        match self {
            Self::Written(lines) => {
                for line in lines {
                    println!("{line}");
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
            Self::NeedsTrust(message) => {
                eprintln!("{message}");
                ExitCode::from(3)
            }
        }
    }
}
fn plan_lines(plan: &Plan, force: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(message) = &plan.before_apply {
        lines.push(message.clone());
    }
    for file in &plan.files {
        let action = if plan.conflicts.contains(&file.path) {
            if force { "overwrite" } else { "conflict" }
        } else {
            "create"
        };
        lines.push(format!("{action} {}", file.path));
    }
    for hook in &plan.hooks {
        lines.push(format!(
            "hook {:?} cwd {}",
            hook.argv(),
            hook.cwd
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| ".".into())
        ));
    }
    if let Some(message) = &plan.after_apply {
        lines.push(message.clone());
    }
    lines
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

fn run(
    template: Option<String>,
    path: &Path,
    answers: Option<String>,
    force: bool,
    dry_run: bool,
    trust: bool,
) -> Outcome {
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
    if dry_run {
        return Outcome::Written(
            completed
                .messages
                .into_iter()
                .chain(plan_lines(&plan, force))
                .collect(),
        );
    }
    let before = plan.before_apply.clone();
    let preview = plan_lines(&plan, force);
    for message in completed.messages {
        println!("{message}");
    }
    if let Some(message) = &before {
        println!("{message}");
    }
    match plan.apply(
        path,
        ApplyOptions {
            force,
            trusted: trust,
        },
        &ProcessRunner,
    ) {
        Ok(Applied::Written {
            files, after_apply, ..
        }) => {
            let mut lines = Vec::new();
            lines.extend(files.into_iter().map(|p| p.to_string()));
            if let Some(message) = after_apply {
                lines.push(message);
            }
            Outcome::Written(lines)
        }
        Ok(Applied::NeedsTrust(_)) => {
            for line in preview.into_iter().skip(usize::from(before.is_some())) {
                println!("{line}");
            }
            Outcome::NeedsTrust("hooks will not run without --trust".into())
        }
        Err(error) => Outcome::Error(error.to_string()),
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Apply {
            paths,
            answers,
            force,
            dry_run,
            trust,
        } => {
            let (template, path) = match paths.as_slice() {
                [path] => (None, PathBuf::from(path)),
                [template, path] => (Some(template.clone()), PathBuf::from(path)),
                _ => unreachable!("clap enforces one or two positional arguments"),
            };
            run(template, &path, answers, force, dry_run, trust).finish()
        }
    }
}
