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
    AnswerError, Applied, ApplyOptions, Interview, Plan, Seed, Template,
    hook::ProcessRunner,
    protocol::{self, Context, Headless},
    staging::{self, StagedRecord, Store},
};

#[derive(Parser)]
#[command(name = "toha")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Stage {
        template: String,
        path: PathBuf,
        #[arg(short = 'a', long = "async", num_args = 0..=1)]
        r#async: Option<Option<String>>,
    },
    Continue {
        path: PathBuf,
        answers: Option<String>,
    },
    Abort {
        path: PathBuf,
    },
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
    Document(serde_json::Value, u8),
    Saved(u8),
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
            Self::Saved(code) => ExitCode::from(code),
            Self::Document(value, code) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("JSON value")
                );
                ExitCode::from(code)
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

fn read_answers(path: &str) -> Result<toha::RawAnswers, String> {
    let text = if path == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        text
    } else {
        fs::read_to_string(path).map_err(|e| e.to_string())?
    };
    protocol::parse_answers(&text).map_err(|e| e.to_string())
}
fn seed() -> Result<Seed, String> {
    let now = match std::env::var("TOHA_NOW") {
        Ok(value) => value.parse().map_err(|e: jiff::Error| e.to_string())?,
        Err(std::env::VarError::NotPresent) => jiff::Zoned::now(),
        Err(e) => return Err(e.to_string()),
    };
    Ok(Seed {
        now,
        defaults: indexmap::IndexMap::new(),
    })
}
struct Dirs {
    state: PathBuf,
}
impl Dirs {
    fn resolve() -> Result<Self, String> {
        #[cfg(windows)]
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is unavailable")?;
        #[cfg(target_os = "macos")]
        let base = std::env::var_os("HOME")
            .map(|v| PathBuf::from(v).join("Library/Application Support"))
            .ok_or("HOME is unavailable")?;
        #[cfg(all(unix, not(target_os = "macos")))]
        let base = match std::env::var_os("XDG_STATE_HOME") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => std::env::var_os("HOME")
                .map(|v| PathBuf::from(v).join(".local/state"))
                .ok_or("HOME is unavailable")?,
        };
        Ok(Self {
            state: base.join("toha/staged"),
        })
    }
}
fn setup(path: &Path, dirs: &Dirs) -> Result<(PathBuf, Store), String> {
    let target = staging::canonical_target(path).map_err(|e| e.to_string())?;
    Ok((target, Store::new(dirs.state.clone())))
}
fn context(record: &StagedRecord) -> Context {
    Context {
        target: record.target.to_string_lossy().into_owned(),
        template: record.template.clone(),
        commit: record.commit.clone(),
    }
}
fn load_template(folder: &str) -> Result<(Template, String), String> {
    let path = local_folder(folder)?;
    let formal = path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    Ok((Template::load(&path).map_err(|e| e.to_string())?, formal))
}
fn record(target: PathBuf, template: String, now: &jiff::Zoned) -> StagedRecord {
    StagedRecord {
        target,
        template,
        commit: String::new(),
        now: now.to_string(),
        submissions: vec![],
    }
}
fn stage(template: String, path: PathBuf, output: Option<Option<String>>, dirs: &Dirs) -> Outcome {
    let Some(output) = output else {
        return Outcome::Error("interactive prompts are not supported yet".into());
    };
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    match store.load(&target) {
        Ok(Some(_)) => {
            return Outcome::Error("interview already staged; use continue or abort".into());
        }
        Ok(None) => {}
        Err(e) => return Outcome::Error(e.to_string()),
    }
    let (template, formal) = match load_template(&template) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let seed = match seed() {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let saved = record(target, formal, &seed.now);
    let interview = match Interview::start(&template, seed) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let (document, code) = match interview {
        Interview::Asking(pending) => (
            protocol::batch_document(pending.batch(), &context(&saved), None),
            4,
        ),
        Interview::Complete(completed) => (
            protocol::complete_document(&completed.answers, &context(&saved)),
            0,
        ),
    };
    if let Err(e) = store.save(&saved) {
        return Outcome::Error(e.to_string());
    }
    if let Some(file) = output {
        if let Err(e) = fs::write(
            file,
            serde_json::to_vec_pretty(&document).expect("JSON value"),
        ) {
            return Outcome::Error(e.to_string());
        }
        Outcome::Saved(code)
    } else {
        Outcome::Document(document, code)
    }
}
fn continue_run(path: PathBuf, answers: Option<String>, dirs: &Dirs) -> Outcome {
    let Some(answers) = answers else {
        return Outcome::Error("interactive prompts are not supported yet".into());
    };
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let mut saved = match store.load(&target) {
        Ok(Some(v)) => v,
        Ok(None) => return Outcome::Error("no staged interview".into()),
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let (template, _) = match load_template(&saved.template) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let interview = match saved.replay(&template) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let raw = match read_answers(&answers) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let Interview::Asking(pending) = interview else {
        return Outcome::Error("interview already complete".into());
    };
    let submission = raw
        .iter()
        .map(|(id, value)| (id.to_string(), value.0.clone()))
        .collect();
    match pending.answer(raw) {
        Ok(next) => {
            saved.submissions.push(submission);
            if let Err(e) = store.save(&saved) {
                return Outcome::Error(e.to_string());
            }
            match next {
                Interview::Asking(p) => Outcome::Document(
                    protocol::batch_document(p.batch(), &context(&saved), None),
                    4,
                ),
                Interview::Complete(c) => {
                    Outcome::Document(protocol::complete_document(&c.answers, &context(&saved)), 0)
                }
            }
        }
        Err(AnswerError::Rejected {
            pending,
            rejections,
        }) => Outcome::Document(
            protocol::batch_document(pending.batch(), &context(&saved), Some(&rejections)),
            4,
        ),
        Err(AnswerError::Eval(e)) => Outcome::Error(e.to_string()),
    }
}
fn abort(path: PathBuf, dirs: &Dirs) -> Outcome {
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    match store.remove(&target) {
        Ok(_) => Outcome::Written(vec![]),
        Err(e) => Outcome::Error(e.to_string()),
    }
}

fn run(
    template: Option<String>,
    path: &Path,
    answers: Option<String>,
    force: bool,
    dry_run: bool,
    trust: bool,
    dirs: &Dirs,
) -> Outcome {
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let existing = match store.load(&target) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    if template.is_some() && existing.is_some() {
        return Outcome::Error("interview already staged; use continue or abort".into());
    }
    if template.is_none() && answers.is_some() {
        return Outcome::Error("--answers requires a template".into());
    }
    let (template, interview, saved) = match (template, existing) {
        (Some(folder), None) => {
            let Some(answers_file) = answers else {
                return Outcome::Error("interactive prompts are not supported yet".into());
            };
            let (template, formal) = match load_template(&folder) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let seed = match seed() {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let saved = record(target.clone(), formal, &seed.now);
            let raw = match read_answers(&answers_file) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let interview = match Interview::start(&template, seed) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            let result = match protocol::answer_headless(&template, interview, raw) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            match result {
                Headless::Completed { completed, .. } => (template, completed, None),
                Headless::Pending {
                    pending,
                    rejections,
                    accepted,
                } => {
                    let mut saved = saved;
                    saved.submissions = accepted;
                    let document = protocol::batch_document(
                        pending.batch(),
                        &context(&saved),
                        Some(&rejections),
                    );
                    if let Err(e) = store.save(&saved) {
                        return Outcome::Error(e.to_string());
                    }
                    return Outcome::Document(document, 4);
                }
            }
        }
        (None, Some(saved)) => {
            let (template, _) = match load_template(&saved.template) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let interview = match saved.replay(&template) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            match interview {
                Interview::Asking(p) => {
                    eprintln!("interview is incomplete; use continue or abort");
                    return Outcome::Document(
                        protocol::batch_document(p.batch(), &context(&saved), None),
                        4,
                    );
                }
                Interview::Complete(c) => (template, c, Some(saved)),
            }
        }
        (None, None) => {
            return Outcome::Error("no staged interview; template folder is required".into());
        }
        (Some(_), Some(_)) => unreachable!(),
    };
    let completed = interview;
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
            if saved.is_some() {
                if let Err(e) = store.remove(&target) {
                    return Outcome::Error(e.to_string());
                }
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
    let dirs = match Dirs::resolve() {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e).finish(),
    };
    match Cli::parse().command {
        Command::Stage {
            template,
            path,
            r#async,
        } => stage(template, path, r#async, &dirs).finish(),
        Command::Continue { path, answers } => continue_run(path, answers, &dirs).finish(),
        Command::Abort { path } => abort(path, &dirs).finish(),
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
            run(template, &path, answers, force, dry_run, trust, &dirs).finish()
        }
    }
}
