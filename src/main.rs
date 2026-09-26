mod cli {
    pub mod guidance;
    pub mod resolve;
    pub mod templates;
}
// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};
#[path = "cli/skills.rs"]
mod skills;
#[path = "cli/terminal.rs"]
mod terminal;

use clap::{Parser, Subcommand};
use cli::{
    guidance::{self, Progress},
    resolve::{ResolveError, ResolvedTemplate},
};
use toha::{
    AnswerError, Applied, ApplyOptions, Interview, Plan, Seed, Template,
    hook::ProcessRunner,
    protocol::{self, Context, Headless},
    staging::{self, StagedRecord, Store},
};

/// Generate projects and files from templates.
#[derive(Parser)]
#[command(name = "toha", version = env!("TOHA_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
/// Help layout for `apply`, whose operands are one clap argument so that
/// `[TEMPLATE] <PATH>` parses the same on either side of `--`.
const APPLY_HELP: &str = "\
{before-help}{about-with-newline}
{usage-heading} {usage}

Arguments:
  [TEMPLATE]  Template to interview when no interview is staged at the path.
              When given, the command runs `stage` and then applies. When an
              interview for the same template is staged at the path, the
              command applies it as `apply <PATH>` does.
  <PATH>      Target directory.

{all-args}{after-help}";

#[derive(Subcommand)]
enum Command {
    /// Manage installed templates.
    Templates(cli::templates::TemplatesArgs),
    /// Read the agent skills embedded in toha.
    Skills {
        #[command(subcommand)]
        command: skills::Command,
    },
    /// Run the interview for a template and save the answers without writing files.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    Stage {
        /// Alias, short name, formal name, git address, or folder of the template.
        template: String,
        /// Target directory.
        path: PathBuf,
        /// Emit the first question batch instead of prompting.
        ///
        /// Writes to FILE, or to standard output when no file is given.
        #[arg(
            short = 'a',
            long = "async",
            num_args = 0..=1,
            value_name = "FILE"
        )]
        r#async: Option<Option<String>>,
    },
    /// Continue a staged interview with an answers document or terminal prompts.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    Continue {
        /// Target directory of the staged interview.
        path: PathBuf,
        /// Answers document for the current batch; - reads standard input.
        ///
        /// When absent, toha prompts in the terminal for every remaining question, or emits the
        /// complete result document when none remain.
        #[arg(value_name = "FILE")]
        answers: Option<String>,
    },
    /// Delete the staged interview for a target directory.
    Abort {
        /// Target directory of the staged interview.
        path: PathBuf,
    },
    /// Write the files of a staged or new interview and run its hooks.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    #[command(
        override_usage = "toha apply [OPTIONS] [TEMPLATE] <PATH>",
        help_template = APPLY_HELP
    )]
    Apply {
        /// One operand is the target directory; two are the template and the target directory.
        #[arg(num_args = 1..=2, required = true, value_name = "PATH", hide = true)]
        paths: Vec<String>,
        /// Answers document for a new interview, or for the staged interview of the named
        /// template; - reads standard input.
        #[arg(short = 'A', long, value_name = "FILE")]
        answers: Option<String>,
        /// Overwrite existing files.
        #[arg(short, long)]
        force: bool,
        /// Print the files, messages, and hooks apply would produce, and change nothing.
        #[arg(short = 'd', long)]
        dry_run: bool,
        /// Run the hooks of this template for this run.
        #[arg(long)]
        trust: bool,
    },
}

#[derive(Clone)]
pub(crate) struct Dirs {
    system_config: PathBuf,
    user_config: PathBuf,
    local_config_override: Option<PathBuf>,
    system_data: PathBuf,
    user_data: PathBuf,
    cache: PathBuf,
    state: PathBuf,
    home: PathBuf,
}
impl Dirs {
    fn resolve() -> Result<Self, String> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .ok_or("home directory unavailable")?;
        let system_root = if cfg!(windows) {
            std::env::var_os("PROGRAMDATA")
                .map(PathBuf::from)
                .ok_or("PROGRAMDATA unavailable")?
                .join("toha")
        } else {
            PathBuf::from("/usr/local/share/toha")
        };
        let system_config = if cfg!(windows) {
            system_root.join("config.yml")
        } else {
            PathBuf::from("/etc/toha/config.yml")
        };
        let app_data = if cfg!(windows) {
            Some(
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .ok_or("APPDATA unavailable")?,
            )
        } else {
            None
        };
        let user_config = if let Some(override_path) = std::env::var_os("TOHA_USER_CONFIG") {
            PathBuf::from(override_path)
        } else if let Some(app_data) = &app_data {
            app_data.join("toha/config.yml")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("toha/config.yml")
        };
        let user_data = if let Some(app_data) = &app_data {
            app_data.join("toha")
        } else if cfg!(target_os = "macos") {
            home.join("Library/Application Support/toha")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("toha")
        };
        let local_app_data = if cfg!(windows) {
            Some(
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .ok_or("LOCALAPPDATA unavailable")?,
            )
        } else {
            None
        };
        let cache = if let Some(base) = &local_app_data {
            base.join("toha/cache")
        } else if cfg!(target_os = "macos") {
            home.join("Library/Caches/toha")
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
                .join("toha")
        };
        let state = if let Some(base) = &local_app_data {
            base.join("toha/staged")
        } else if cfg!(target_os = "macos") {
            home.join("Library/Application Support/toha/staged")
        } else {
            std::env::var_os("XDG_STATE_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/state"))
                .join("toha/staged")
        };
        Ok(Self {
            system_config,
            user_config,
            local_config_override: std::env::var_os("TOHA_CONFIG").map(PathBuf::from),
            system_data: system_root,
            user_data,
            cache,
            state,
            home,
        })
    }
    fn config_paths(&self) -> toha::config::ConfigPaths {
        toha::config::ConfigPaths {
            system_config: self.system_config.clone(),
            user_config: self.user_config.clone(),
            local_config_override: self.local_config_override.clone(),
            system_data: self.system_data.clone(),
            user_data: self.user_data.clone(),
            cache: self.cache.clone(),
            home: self.home.clone(),
        }
    }
}
enum Outcome {
    Written(Vec<String>),
    Error(String),
    Document(serde_json::Value, u8),
    Saved(u8),
    NeedsTrust(String),
    Ambiguous { name: String, matches: Vec<String> },
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
            Self::Ambiguous { name, matches } => {
                eprintln!("ambiguous template name: {name}");
                for formal in matches {
                    eprintln!("toha templates alias {formal} <alias>");
                }
                ExitCode::from(5)
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
    lines
}

fn read_answers(path: &str) -> Result<toha::RawAnswers, String> {
    let source = if path == "-" { "stdin" } else { path };
    let text = if path == "-" {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text).map(|_| text)
    } else {
        fs::read_to_string(path)
    }
    .map_err(|e| format!("{source}: cannot read answers document: {e}"))?;
    let invalid =
        |reason: &dyn std::fmt::Display| format!("{source}: not a JSON answers document: {reason}");
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| invalid(&e))?;
    if !value.is_object() {
        return Err(invalid(&"expected an object keyed by question id"));
    }
    protocol::parse_answers(&text).map_err(|e| invalid(&e))
}
fn seed(template: &Template, config: &toha::config::Config) -> Result<Seed, String> {
    let now = match std::env::var("TOHA_NOW") {
        Ok(value) => value.parse().map_err(|e: jiff::Error| e.to_string())?,
        Err(std::env::VarError::NotPresent) => jiff::Zoned::now(),
        Err(e) => return Err(e.to_string()),
    };
    Ok(Seed {
        now,
        defaults: toha::interview::configured_defaults(template, &config.defaults)
            .map_err(|e| e.to_string())?,
    })
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
fn load_template(resolved: &ResolvedTemplate) -> Result<Template, String> {
    Template::load(&resolved.folder).map_err(|e| e.to_string())
}
fn environment(
    dirs: &Dirs,
) -> Result<(toha::config::Config, toha::registry::Registry, PathBuf), Outcome> {
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli::resolve::load_context(dirs, &cwd).map_err(resolve_error)?;
    Ok((config, registry, cwd))
}
fn resolve_error(error: ResolveError) -> Outcome {
    match error {
        ResolveError::Error(message) => Outcome::Error(message),
        ResolveError::Ambiguous { name, matches } => Outcome::Ambiguous { name, matches },
    }
}
fn record(target: PathBuf, resolved: &ResolvedTemplate, now: &jiff::Zoned) -> StagedRecord {
    StagedRecord {
        target,
        template: resolved.formal_name.clone(),
        commit: resolved.commit.clone(),
        named: resolved.named,
        now: now.to_string(),
        submissions: vec![],
    }
}
/// The configuration, registry, and directories that template names resolve against.
struct Scope<'a> {
    config: &'a toha::config::Config,
    registry: &'a toha::registry::Registry,
    dirs: &'a Dirs,
    cwd: &'a Path,
}
/// Whether the staged interview has questions remaining, for guidance only.
fn progress(saved: &StagedRecord, scope: &Scope) -> Progress {
    let replayed = (|| {
        let resolved = cli::resolve::resume_template(
            &saved.template,
            &saved.commit,
            saved.named,
            scope.config,
            scope.registry,
            scope.dirs,
            scope.cwd,
        )
        .ok()?;
        let template = load_template(&resolved).ok()?;
        let defaults =
            toha::interview::configured_defaults(&template, &scope.config.defaults).ok()?;
        let complete = matches!(
            saved.replay_with_defaults(&template, defaults).ok()?,
            Interview::Complete(_)
        );
        Some(complete)
    })();
    match replayed {
        Some(true) => Progress::Complete,
        Some(false) => Progress::Incomplete,
        None => Progress::Unknown,
    }
}
/// The refusal for `verb <template> <path>` when an interview is staged at the
/// path, or `None` when the staged interview is for the same template.
fn staged_refusal(
    verb: &str,
    arg: &str,
    path: &Path,
    saved: &StagedRecord,
    scope: &Scope,
) -> Option<Outcome> {
    let formal =
        match cli::resolve::formal_name(arg, scope.config, scope.registry, scope.dirs, scope.cwd) {
            Ok(v) => v,
            Err(e) => return Some(resolve_error(e)),
        };
    let progress = || progress(saved, scope);
    if formal != saved.template {
        return Some(Outcome::Error(guidance::other_template(
            verb,
            path,
            &saved.template,
            arg,
            &formal,
            progress(),
        )));
    }
    (verb == "stage")
        .then(|| Outcome::Error(guidance::already_staged(path, &saved.template, progress())))
}
fn stage(template: String, path: PathBuf, output: Option<Option<String>>, dirs: &Dirs) -> Outcome {
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let existing = match store.load(&target) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let (config, registry, cwd) = match environment(dirs) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Some(saved) = existing {
        let scope = Scope {
            config: &config,
            registry: &registry,
            dirs,
            cwd: &cwd,
        };
        return staged_refusal("stage", &template, &path, &saved, &scope)
            .expect("stage refuses every staged target");
    }
    if output.is_none() && !io::stdin().is_terminal() {
        return no_terminal();
    }
    let resolved = match cli::resolve::resolve_template(&template, &config, &registry, dirs, &cwd) {
        Ok(v) => v,
        Err(e) => return resolve_error(e),
    };
    let template = match load_template(&resolved) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let seed = match seed(&template, &config) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let saved = record(target, &resolved, &seed.now);
    let interview = match Interview::start(&template, seed) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    if output.is_none() {
        let mut saved = saved;
        let completed = terminal::drive(interview, &mut terminal::InquireAsk, |submission| {
            saved.submissions.push(submission);
            store.save(&saved).map_err(|e| e.to_string())
        });
        return match completed {
            Ok(_) => {
                // An interview without prompts still needs a staged record.
                if saved.submissions.is_empty() {
                    if let Err(e) = store.save(&saved) {
                        return Outcome::Error(e.to_string());
                    }
                }
                Outcome::Saved(0)
            }
            Err(error) => Outcome::Error(error),
        };
    }
    let output = output.unwrap();
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
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let mut saved = match store.load(&target) {
        Ok(Some(v)) => v,
        Ok(None) => return Outcome::Error(guidance::nothing_staged(&path)),
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let (config, registry, cwd) = match environment(dirs) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let resolved = match cli::resolve::resume_template(
        &saved.template,
        &saved.commit,
        saved.named,
        &config,
        &registry,
        dirs,
        &cwd,
    ) {
        Ok(v) => v,
        Err(e) => return resolve_error(e),
    };
    let template = match load_template(&resolved) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let defaults = match toha::interview::configured_defaults(&template, &config.defaults) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let interview = match saved.replay_with_defaults(&template, defaults) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    if let Interview::Complete(completed) = &interview {
        if answers.is_some() {
            return Outcome::Error(guidance::complete_answers_unused(&path, &saved.template));
        }
        eprintln!("{}", guidance::complete(&path));
        return Outcome::Document(
            protocol::complete_document(&completed.answers, &context(&saved)),
            0,
        );
    }
    if answers.is_none() {
        if !io::stdin().is_terminal() {
            return Outcome::Error(guidance::continue_no_terminal(&path));
        }
        return match terminal::drive(interview, &mut terminal::InquireAsk, |submission| {
            saved.submissions.push(submission);
            store.save(&saved).map_err(|e| e.to_string())
        }) {
            Ok(_) => Outcome::Saved(0),
            Err(error) => Outcome::Error(error),
        };
    }
    let answers = answers.unwrap();
    let raw = match read_answers(&answers) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let Interview::Asking(pending) = interview else {
        unreachable!("a complete interview returned above");
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
        Ok(true) => Outcome::Written(vec![]),
        Ok(false) => {
            eprintln!("{}", guidance::nothing_staged(&path));
            Outcome::Written(vec![])
        }
        Err(e) => Outcome::Error(e.to_string()),
    }
}

fn no_terminal() -> Outcome {
    Outcome::Error("no terminal: use --async, an answers document, or --answers".into())
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
    if template.is_none() && answers.is_some() {
        return Outcome::Error("--answers requires a template".into());
    }
    let (config, registry, cwd) = match environment(dirs) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let (Some(arg), Some(saved)) = (&template, &existing) {
        if let Some(refusal) = staged_refusal(
            "apply",
            arg,
            path,
            saved,
            &Scope {
                config: &config,
                registry: &registry,
                dirs,
                cwd: &cwd,
            },
        ) {
            return refusal;
        }
    }
    // A template named for its own staged interview resumes that interview.
    let template = template.filter(|_| existing.is_none());
    let terminal_run = template.is_some() && answers.is_none();
    let registry_trusted;
    let (template, interview, saved) = match (template, existing) {
        (Some(folder), None) => {
            if answers.is_none() && !io::stdin().is_terminal() {
                return no_terminal();
            }
            let resolved =
                match cli::resolve::resolve_template(&folder, &config, &registry, dirs, &cwd) {
                    Ok(v) => v,
                    Err(e) => return resolve_error(e),
                };
            registry_trusted = resolved.trusted;
            let template = match load_template(&resolved) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let seed = match seed(&template, &config) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let saved = record(target.clone(), &resolved, &seed.now);
            let interview = match Interview::start(&template, seed) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            if let Some(answers_file) = answers {
                let raw = match read_answers(&answers_file) {
                    Ok(v) => v,
                    Err(e) => return Outcome::Error(e),
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
                        if !dry_run {
                            if let Err(e) = store.save(&saved) {
                                return Outcome::Error(e.to_string());
                            }
                        }
                        return Outcome::Document(document, 4);
                    }
                }
            } else {
                let completed =
                    match terminal::drive(interview, &mut terminal::InquireAsk, |_| Ok(())) {
                        Ok(v) => v,
                        Err(e) => return Outcome::Error(e),
                    };
                (template, completed, None)
            }
        }
        (None, Some(mut saved)) => {
            let resolved = match cli::resolve::resume_template(
                &saved.template,
                &saved.commit,
                saved.named,
                &config,
                &registry,
                dirs,
                &cwd,
            ) {
                Ok(v) => v,
                Err(e) => return resolve_error(e),
            };
            registry_trusted = resolved.trusted;
            let template = match load_template(&resolved) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let defaults = match toha::interview::configured_defaults(&template, &config.defaults) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            let interview = match saved.replay_with_defaults(&template, defaults) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            let interview = match (interview, answers) {
                (Interview::Complete(_), Some(_)) => {
                    return Outcome::Error(guidance::complete_answers_unused(
                        path,
                        &saved.template,
                    ));
                }
                (Interview::Asking(pending), Some(answers_file)) => {
                    let raw = match read_answers(&answers_file) {
                        Ok(v) => v,
                        Err(e) => return Outcome::Error(e),
                    };
                    let result =
                        match protocol::answer_headless(&template, Interview::Asking(pending), raw)
                        {
                            Ok(v) => v,
                            Err(e) => return Outcome::Error(e.to_string()),
                        };
                    match result {
                        Headless::Completed {
                            completed,
                            accepted,
                        } => {
                            saved.submissions.extend(accepted);
                            if !dry_run {
                                if let Err(e) = store.save(&saved) {
                                    return Outcome::Error(e.to_string());
                                }
                            }
                            Interview::Complete(completed)
                        }
                        Headless::Pending {
                            pending,
                            rejections,
                            accepted,
                        } => {
                            saved.submissions.extend(accepted);
                            if !dry_run {
                                if let Err(e) = store.save(&saved) {
                                    return Outcome::Error(e.to_string());
                                }
                            }
                            return Outcome::Document(
                                protocol::batch_document(
                                    pending.batch(),
                                    &context(&saved),
                                    Some(&rejections),
                                ),
                                4,
                            );
                        }
                    }
                }
                (interview, None) => interview,
            };
            match interview {
                Interview::Asking(p) => {
                    eprintln!("{}", guidance::incomplete(path));
                    return Outcome::Document(
                        protocol::batch_document(p.batch(), &context(&saved), None),
                        4,
                    );
                }
                Interview::Complete(c) => (template, c, Some(saved)),
            }
        }
        (None, None) => {
            return Outcome::Error(guidance::nothing_staged(path));
        }
        (Some(_), Some(_)) => unreachable!("a named template resumes its staged interview"),
    };
    let completed = interview;
    let plan = match Plan::build(&template, &completed, path) {
        Ok(plan) => plan,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    if dry_run {
        return Outcome::Written(
            (if terminal_run {
                vec![]
            } else {
                completed.messages
            })
            .into_iter()
            .chain(plan_lines(&plan, force))
            .collect(),
        );
    }
    let before = plan.before_apply.clone();
    let preview = plan_lines(&plan, force);
    if !terminal_run {
        for message in completed.messages {
            println!("{message}");
        }
    }
    if let Some(message) = &before {
        println!("{message}");
    }
    match plan.apply(
        path,
        ApplyOptions {
            force,
            trusted: registry_trusted || trust,
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
        Command::Skills { command } => skills::run(command).finish(),
        Command::Stage {
            template,
            path,
            r#async,
        } => stage(template, path, r#async, &dirs).finish(),
        Command::Continue { path, answers } => continue_run(path, answers, &dirs).finish(),
        Command::Abort { path } => abort(path, &dirs).finish(),
        Command::Templates(args) => {
            let cwd = match std::env::current_dir() {
                Ok(cwd) => cwd,
                Err(error) => return Outcome::Error(error.to_string()).finish(),
            };
            match cli::templates::run(args, dirs, &cwd) {
                Ok(lines) => Outcome::Written(lines).finish(),
                Err(cli::templates::CommandError::Error(message)) => {
                    Outcome::Error(message).finish()
                }
                Err(cli::templates::CommandError::Ambiguous { name, matches }) => {
                    Outcome::Ambiguous { name, matches }.finish()
                }
            }
        }
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
                _ => unreachable!("clap requires one or two operands"),
            };
            run(template, &path, answers, force, dry_run, trust, &dirs).finish()
        }
    }
}
