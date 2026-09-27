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
    ffi::OsString,
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
    guidance::{self, Arg, Invocation, Progress},
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
#[command(
    name = "toha",
    version = env!("TOHA_VERSION"),
    after_long_help = ENVIRONMENT_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
/// The environment variables that `Dirs::resolve`, `seed`, and the terminal
/// driver read.
const ENVIRONMENT_HELP: &str = "\
Environment:
  TOHA_CONFIG       Local configuration file. Default: the file named by the configured
                    local-config-name, .toha.yml unless configured, in the current directory.
  TOHA_USER_CONFIG  User configuration file. Default: $XDG_CONFIG_HOME/toha/config.yml on
                    Linux and macOS, %APPDATA%\\toha\\config.yml on Windows.
  XDG_CONFIG_HOME   Linux and macOS: base of the user configuration file. Default: ~/.config.
  XDG_DATA_HOME     Linux: base of the user registry and installed templates, toha/.
                    Default: ~/.local/share. macOS uses ~/Library/Application Support/toha.
  XDG_CACHE_HOME    Linux: base of git templates fetched by address, toha/.
                    Default: ~/.cache. macOS uses ~/Library/Caches/toha.
  XDG_STATE_HOME    Linux: base of staged interviews, toha/staged/. Default: ~/.local/state.
                    macOS uses ~/Library/Application Support/toha/staged.
  HOME              Home directory for ~ in addresses and the defaults above.
  USERPROFILE       Home directory when HOME is unset.
  APPDATA           Windows: user configuration, registry, and installed templates in
                    %APPDATA%\\toha.
  LOCALAPPDATA      Windows: fetched git templates in %LOCALAPPDATA%\\toha\\cache and staged
                    interviews in %LOCALAPPDATA%\\toha\\staged.
  PROGRAMDATA       Windows: system configuration and registry in %PROGRAMDATA%\\toha.
                    Other platforms use /etc/toha/config.yml and
                    /usr/local/share/toha/templates.yml.
  VISUAL, EDITOR    Editor for multiline answers, VISUAL first. Default: nano, or notepad
                    on Windows. When the editor is not found, answers end with a line of \".\".
  PATH              Directories searched for the editor and for the program of a run hook.
  TOHA_NOW          Instant that now() returns for a new interview, such as
                    2026-01-02T03:04:05+00:00[UTC]. Default: the current time.

  XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, XDG_STATE_HOME, HOME, USERPROFILE,
  APPDATA, LOCALAPPDATA, and PROGRAMDATA count only when they hold an absolute path.
  An empty or relative value counts as unset. toha exits with code 1 when neither HOME
  nor USERPROFILE counts, and on Windows when APPDATA, LOCALAPPDATA, or PROGRAMDATA
  does not count. An empty TOHA_CONFIG or TOHA_USER_CONFIG counts as unset.";

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
/// The platform whose directory conventions `Dirs::from_env` follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    /// Linux and every other platform that is not macOS or Windows.
    Unix,
    MacOs,
    Windows,
}
impl Platform {
    fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}
impl Dirs {
    fn resolve() -> Result<Self, String> {
        Self::from_env(Platform::host(), |key| std::env::var_os(key))
    }
    /// The directories of `platform`, with `var` as the environment.
    ///
    /// A directory variable counts only when it holds an absolute path. An
    /// empty or relative value counts as unset, as the XDG Base Directory
    /// Specification requires for its variables, so that no toha directory
    /// depends on the current directory. An empty `TOHA_USER_CONFIG` or
    /// `TOHA_CONFIG` counts as unset.
    fn from_env(
        platform: Platform,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let dir = |key: &str| {
            var(key)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        // A file variable may be relative to the current directory.
        let file = |key: &str| {
            var(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let windows = platform == Platform::Windows;
        let macos = platform == Platform::MacOs;
        let home = dir("HOME").or_else(|| dir("USERPROFILE")).ok_or(
            "home directory unavailable: set HOME (or USERPROFILE on Windows) to an absolute path",
        )?;
        let system_root = if windows {
            dir("PROGRAMDATA")
                .ok_or("PROGRAMDATA unavailable")?
                .join("toha")
        } else {
            PathBuf::from("/usr/local/share/toha")
        };
        let system_config = if windows {
            system_root.join("config.yml")
        } else {
            PathBuf::from("/etc/toha/config.yml")
        };
        let app_data = if windows {
            Some(dir("APPDATA").ok_or("APPDATA unavailable")?)
        } else {
            None
        };
        let user_config = if let Some(override_path) = file("TOHA_USER_CONFIG") {
            override_path
        } else if let Some(app_data) = &app_data {
            app_data.join("toha/config.yml")
        } else {
            dir("XDG_CONFIG_HOME")
                .unwrap_or_else(|| home.join(".config"))
                .join("toha/config.yml")
        };
        let user_data = if let Some(app_data) = &app_data {
            app_data.join("toha")
        } else if macos {
            home.join("Library/Application Support/toha")
        } else {
            dir("XDG_DATA_HOME")
                .unwrap_or_else(|| home.join(".local/share"))
                .join("toha")
        };
        let local_app_data = if windows {
            Some(dir("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?)
        } else {
            None
        };
        let cache = if let Some(base) = &local_app_data {
            base.join("toha/cache")
        } else if macos {
            home.join("Library/Caches/toha")
        } else {
            dir("XDG_CACHE_HOME")
                .unwrap_or_else(|| home.join(".cache"))
                .join("toha")
        };
        let state = if let Some(base) = &local_app_data {
            base.join("toha/staged")
        } else if macos {
            home.join("Library/Application Support/toha/staged")
        } else {
            dir("XDG_STATE_HOME")
                .unwrap_or_else(|| home.join(".local/state"))
                .join("toha/staged")
        };
        Ok(Self {
            system_config,
            user_config,
            local_config_override: file("TOHA_CONFIG"),
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
    Ambiguous {
        name: String,
        matches: Vec<String>,
        /// The same command with each match's formal name, in match order.
        retry: Vec<String>,
    },
}
impl Outcome {
    /// Names `command` with each formal name when the template name is ambiguous.
    fn retry(self, command: impl Fn(&str) -> String) -> Self {
        match self {
            Self::Ambiguous { name, matches, .. } => Self::Ambiguous {
                retry: matches.iter().map(|formal| command(formal)).collect(),
                name,
                matches,
            },
            other => other,
        }
    }
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
            Self::Ambiguous {
                name,
                matches,
                retry,
            } => {
                eprintln!("{}", guidance::ambiguous(&name, &matches, &retry));
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
        commit: Some(record.commit.clone()).filter(|commit| !commit.is_empty()),
    }
}
/// The formal name of a template resolved by name from the user or system
/// registry, whose recorded trust would run its hooks.
fn trustable(resolved: &ResolvedTemplate, registry: &toha::registry::Registry) -> Option<String> {
    let listed = registry.entries.get(&resolved.formal_name)?;
    (resolved.named && listed.layer != toha::registry::Layer::Local)
        .then(|| resolved.formal_name.clone())
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
        ResolveError::Ambiguous { name, matches } => Outcome::Ambiguous {
            name,
            matches,
            retry: vec![],
        },
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
/// The refusal for `stage` or `apply` with a template when an interview is
/// staged at the path, or `None` when `apply` names the staged template.
fn staged_refusal(
    invocation: &Invocation,
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
            invocation,
            path,
            &saved.template,
            &formal,
            progress(),
        )));
    }
    matches!(invocation, Invocation::Stage { .. }).then(|| {
        Outcome::Error(guidance::already_staged(
            invocation,
            path,
            &saved.template,
            progress(),
        ))
    })
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
        let invocation = Invocation::Stage {
            template: Arg::Given(&template),
            path: &path,
            output: output.as_ref().map(|file| file.as_deref()),
        };
        return staged_refusal(&invocation, &template, &path, &saved, &scope)
            .expect("stage refuses every staged target");
    }
    if output.is_none() && !io::stdin().is_terminal() {
        return Outcome::Error(guidance::no_terminal(&template, &path));
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
        Interview::Complete(completed) => {
            (protocol::complete_document(&completed, &context(&saved)), 0)
        }
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
        Err(e) => {
            return Outcome::Error(guidance::replay_failed(
                &path,
                &saved.template,
                &e.to_string(),
            ));
        }
    };
    if let Interview::Complete(completed) = &interview {
        if answers.is_some() {
            return Outcome::Error(guidance::complete_answers_unused(&path, &saved.template));
        }
        eprintln!("{}", guidance::complete(&path));
        return Outcome::Document(protocol::complete_document(completed, &context(&saved)), 0);
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
                    Outcome::Document(protocol::complete_document(&c, &context(&saved)), 0)
                }
            }
        }
        Err(AnswerError::Rejected {
            pending,
            rejections,
        }) => Outcome::Document(
            protocol::batch_document(
                pending.batch(),
                &context(&saved),
                Some(&guidance::explain_rejections(
                    &rejections,
                    &path,
                    &saved.template,
                )),
            ),
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
    let (config, registry, cwd) = match environment(dirs) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let scope = Scope {
        config: &config,
        registry: &registry,
        dirs,
        cwd: &cwd,
    };
    if template.is_none() && answers.is_some() {
        let staged = existing
            .as_ref()
            .map(|saved| (saved.template.as_str(), progress(saved, &scope)));
        return Outcome::Error(guidance::answers_without_template(
            path,
            answers.as_deref().unwrap_or_default(),
            staged,
        ));
    }
    let (requested_template, requested_answers) = (template.clone(), answers.clone());
    let invocation = Invocation::Apply {
        template: requested_template.as_deref().map(Arg::Given),
        path,
        answers: requested_answers.as_deref().map(Arg::Given),
        force,
        dry_run,
        trust,
    };
    if let (Some(arg), Some(saved)) = (&template, &existing) {
        if let Some(refusal) = staged_refusal(&invocation, arg, path, saved, &scope) {
            return refusal;
        }
    }
    // A template named for its own staged interview resumes that interview.
    let template = template.filter(|_| existing.is_none());
    let mut terminal_run = template.is_some() && answers.is_none();
    let registry_trusted;
    // The installed template whose registry trust would run the hooks.
    let installed: Option<String>;
    let (template, interview, saved) = match (template, existing) {
        (Some(folder), None) => {
            if answers.is_none() && !io::stdin().is_terminal() {
                return Outcome::Error(guidance::no_terminal(&folder, path));
            }
            let resolved =
                match cli::resolve::resolve_template(&folder, &config, &registry, dirs, &cwd) {
                    Ok(v) => v,
                    Err(e) => return resolve_error(e),
                };
            registry_trusted = resolved.trusted;
            installed = trustable(&resolved, &registry);
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
                        if dry_run {
                            eprintln!("{}", guidance::dry_run_incomplete(&invocation, path));
                        } else {
                            if let Err(e) = store.save(&saved) {
                                return Outcome::Error(e.to_string());
                            }
                            eprintln!("{}", guidance::incomplete(path));
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
            installed = trustable(&resolved, &registry);
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
                Err(e) => {
                    return Outcome::Error(guidance::replay_failed(
                        path,
                        &saved.template,
                        &e.to_string(),
                    ));
                }
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
                            if dry_run {
                                eprintln!("{}", guidance::dry_run_incomplete(&invocation, path));
                            } else {
                                if let Err(e) = store.save(&saved) {
                                    return Outcome::Error(e.to_string());
                                }
                                eprintln!("{}", guidance::incomplete(path));
                            }
                            return Outcome::Document(
                                protocol::batch_document(
                                    pending.batch(),
                                    &context(&saved),
                                    Some(&guidance::explain_rejections(
                                        &rejections,
                                        path,
                                        &saved.template,
                                    )),
                                ),
                                4,
                            );
                        }
                    }
                }
                (interview, None) => interview,
            };
            match interview {
                Interview::Asking(p) if io::stdin().is_terminal() && io::stdout().is_terminal() => {
                    let completed = terminal::drive(
                        Interview::Asking(p),
                        &mut terminal::InquireAsk,
                        |submission| {
                            if dry_run {
                                return Ok(());
                            }
                            saved.submissions.push(submission);
                            store.save(&saved).map_err(|e| e.to_string())
                        },
                    );
                    match completed {
                        Ok(c) => {
                            terminal_run = true;
                            (template, c, Some(saved))
                        }
                        Err(e) => return Outcome::Error(e),
                    }
                }
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
        let lines = if terminal_run {
            vec![]
        } else {
            completed.messages
        }
        .into_iter()
        .chain(plan_lines(&plan, force));
        if plan.hooks.is_empty() || registry_trusted || trust {
            return Outcome::Written(lines.collect());
        }
        for line in lines {
            println!("{line}");
        }
        // Written after the plan, so that a terminal shows it last.
        eprintln!(
            "{}",
            guidance::dry_run_needs_trust(&invocation, installed.as_deref())
        );
        return Outcome::Saved(0);
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
    // Each file line is printed as the file is written, so that the lines
    // precede the output of every hook.
    match plan.apply_reporting(
        path,
        ApplyOptions {
            force,
            trusted: registry_trusted || trust,
        },
        &ProcessRunner,
        &mut |file| println!("{file}"),
    ) {
        Ok(Applied::Written { after_apply, .. }) => {
            let lines = Vec::from_iter(after_apply);
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
            Outcome::NeedsTrust(guidance::needs_trust(&invocation, installed.as_deref()))
        }
        Err(error @ toha::ApplyError::Conflicts(_)) => {
            Outcome::Error(format!("{error}\n{}", guidance::conflicts(&invocation)))
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
        } => stage(template, path.clone(), r#async.clone(), &dirs)
            .retry(|formal| {
                Invocation::Stage {
                    template: Arg::Given(guidance::formal_for("stage", formal)),
                    path: &path,
                    output: r#async.as_ref().map(|file| file.as_deref()),
                }
                .command()
            })
            .finish(),
        Command::Continue { path, answers } => continue_run(path, answers, &dirs).finish(),
        Command::Abort { path } => abort(path, &dirs).finish(),
        Command::Templates(args) => {
            let cwd = match std::env::current_dir() {
                Ok(cwd) => cwd,
                Err(error) => return Outcome::Error(error.to_string()).finish(),
            };
            let retry = cli::templates::retry(&args);
            match cli::templates::run(args, dirs, &cwd) {
                Ok(lines) => Outcome::Written(lines).finish(),
                Err(cli::templates::CommandError::Error(message)) => {
                    Outcome::Error(message).finish()
                }
                Err(cli::templates::CommandError::Ambiguous { name, matches }) => {
                    Outcome::Ambiguous {
                        name,
                        matches,
                        retry: vec![],
                    }
                    .retry(retry)
                    .finish()
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
            run(
                template,
                &path,
                answers.clone(),
                force,
                dry_run,
                trust,
                &dirs,
            )
            .retry(|formal| {
                Invocation::Apply {
                    template: Some(Arg::Given(guidance::formal_for("apply", formal))),
                    path: &path,
                    answers: answers.as_deref().map(Arg::Given),
                    force,
                    dry_run,
                    trust,
                }
                .command()
            })
            .finish()
        }
    }
}

#[cfg(test)]
mod dirs_tests {
    use super::{Dirs, Platform};

    const HOME_UNAVAILABLE: &str =
        "home directory unavailable: set HOME (or USERPROFILE on Windows) to an absolute path";
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};

    /// An absolute path on the host, so that `Path::is_absolute` accepts it
    /// on every platform.
    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join("toha-dirs").join(name)
    }

    /// The directories of `platform` in an environment that holds `vars` and
    /// an absolute HOME, APPDATA, LOCALAPPDATA, and PROGRAMDATA unless
    /// `vars` names them.
    fn dirs(platform: Platform, vars: &[(&str, &str)]) -> Result<Dirs, String> {
        dirs_without(platform, vars, &[])
    }

    /// `dirs` with the variables in `unset` removed.
    fn dirs_without(
        platform: Platform,
        vars: &[(&str, &str)],
        unset: &[&str],
    ) -> Result<Dirs, String> {
        let mut env: HashMap<String, OsString> = [
            ("HOME", absolute("home")),
            ("APPDATA", absolute("roaming")),
            ("LOCALAPPDATA", absolute("local")),
            ("PROGRAMDATA", absolute("program")),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.into_os_string()))
        .collect();
        for (key, value) in vars {
            env.insert((*key).to_owned(), OsString::from(value));
        }
        for key in unset {
            env.remove(*key);
        }
        Dirs::from_env(platform, |key| env.get(key).cloned())
    }

    fn text(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn xdg_config_home_is_used_only_when_absolute() {
        let default = absolute("home").join(".config/toha/config.yml");
        for platform in [Platform::Unix, Platform::MacOs] {
            for value in ["", "relative"] {
                let actual = dirs(platform, &[("XDG_CONFIG_HOME", value)]).unwrap();
                assert_eq!(actual.user_config, default, "{platform:?} {value:?}");
            }
            let base = text(&absolute("config"));
            let actual = dirs(platform, &[("XDG_CONFIG_HOME", &base)]).unwrap();
            assert_eq!(
                actual.user_config,
                absolute("config").join("toha/config.yml")
            );
        }
    }

    #[test]
    fn xdg_data_home_is_used_only_when_absolute() {
        let default = absolute("home").join(".local/share/toha");
        for value in ["", "relative"] {
            let actual = dirs(Platform::Unix, &[("XDG_DATA_HOME", value)]).unwrap();
            assert_eq!(actual.user_data, default, "{value:?}");
        }
        let base = text(&absolute("data"));
        let actual = dirs(Platform::Unix, &[("XDG_DATA_HOME", &base)]).unwrap();
        assert_eq!(actual.user_data, absolute("data").join("toha"));
    }

    #[test]
    fn xdg_cache_home_is_used_only_when_absolute() {
        let default = absolute("home").join(".cache/toha");
        for value in ["", "relative"] {
            let actual = dirs(Platform::Unix, &[("XDG_CACHE_HOME", value)]).unwrap();
            assert_eq!(actual.cache, default, "{value:?}");
        }
        let base = text(&absolute("cache"));
        let actual = dirs(Platform::Unix, &[("XDG_CACHE_HOME", &base)]).unwrap();
        assert_eq!(actual.cache, absolute("cache").join("toha"));
    }

    #[test]
    fn xdg_state_home_is_used_only_when_absolute() {
        let default = absolute("home").join(".local/state/toha/staged");
        for value in ["", "relative"] {
            let actual = dirs(Platform::Unix, &[("XDG_STATE_HOME", value)]).unwrap();
            assert_eq!(actual.state, default, "{value:?}");
        }
        let base = text(&absolute("state"));
        let actual = dirs(Platform::Unix, &[("XDG_STATE_HOME", &base)]).unwrap();
        assert_eq!(actual.state, absolute("state").join("toha/staged"));
    }

    /// Absolute values for every XDG variable.
    fn every_xdg() -> Vec<(&'static str, String)> {
        [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ]
        .into_iter()
        .map(|key| (key, text(&absolute(&key.to_lowercase()))))
        .collect()
    }

    #[test]
    fn macos_reads_only_xdg_config_home() {
        let xdg = every_xdg();
        let vars: Vec<_> = xdg.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let actual = dirs(Platform::MacOs, &vars).unwrap();
        let home = absolute("home");
        assert_eq!(
            actual.user_config,
            absolute("xdg_config_home").join("toha/config.yml")
        );
        assert_eq!(
            actual.user_data,
            home.join("Library/Application Support/toha")
        );
        assert_eq!(actual.cache, home.join("Library/Caches/toha"));
        assert_eq!(
            actual.state,
            home.join("Library/Application Support/toha/staged")
        );
    }

    #[test]
    fn windows_reads_no_xdg_variable() {
        let xdg = every_xdg();
        let vars: Vec<_> = xdg.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let actual = dirs(Platform::Windows, &vars).unwrap();
        assert_eq!(
            actual.user_config,
            absolute("roaming").join("toha/config.yml")
        );
        assert_eq!(actual.user_data, absolute("roaming").join("toha"));
        assert_eq!(actual.cache, absolute("local").join("toha/cache"));
        assert_eq!(actual.state, absolute("local").join("toha/staged"));
    }

    #[test]
    fn toha_user_config_is_used_unless_empty() {
        let actual = dirs(Platform::Unix, &[("TOHA_USER_CONFIG", "")]).unwrap();
        assert_eq!(
            actual.user_config,
            absolute("home").join(".config/toha/config.yml")
        );
        let actual = dirs(Platform::Unix, &[("TOHA_USER_CONFIG", "relative.yml")]).unwrap();
        assert_eq!(actual.user_config, PathBuf::from("relative.yml"));
        let file = text(&absolute("user.yml"));
        let actual = dirs(Platform::Unix, &[("TOHA_USER_CONFIG", &file)]).unwrap();
        assert_eq!(actual.user_config, absolute("user.yml"));
    }

    #[test]
    fn toha_config_is_used_unless_empty() {
        let actual = dirs(Platform::Unix, &[("TOHA_CONFIG", "")]).unwrap();
        assert_eq!(actual.local_config_override, None);
        let actual = dirs(Platform::Unix, &[("TOHA_CONFIG", "relative.yml")]).unwrap();
        assert_eq!(
            actual.local_config_override,
            Some(PathBuf::from("relative.yml"))
        );
        let file = text(&absolute("local.yml"));
        let actual = dirs(Platform::Unix, &[("TOHA_CONFIG", &file)]).unwrap();
        assert_eq!(actual.local_config_override, Some(absolute("local.yml")));
    }

    #[test]
    fn missing_home_names_the_variables() {
        let error = dirs_without(Platform::Unix, &[], &["HOME", "USERPROFILE"])
            .err()
            .expect("no home directory");
        assert_eq!(error, HOME_UNAVAILABLE);
    }

    #[test]
    fn home_is_used_only_when_absolute() {
        let profile = text(&absolute("profile"));
        for value in ["", "relative"] {
            let actual = dirs(
                Platform::Unix,
                &[("HOME", value), ("USERPROFILE", &profile)],
            )
            .unwrap();
            assert_eq!(actual.home, absolute("profile"), "{value:?}");
        }
        let home = text(&absolute("other-home"));
        let actual = dirs(
            Platform::Unix,
            &[("HOME", &home), ("USERPROFILE", &profile)],
        )
        .unwrap();
        assert_eq!(actual.home, absolute("other-home"));
    }

    #[test]
    fn userprofile_is_used_only_when_absolute() {
        for value in ["", "relative"] {
            let error = dirs_without(Platform::Windows, &[("USERPROFILE", value)], &["HOME"])
                .err()
                .unwrap_or_else(|| panic!("USERPROFILE {value:?} was used"));
            assert_eq!(error, HOME_UNAVAILABLE);
        }
        let profile = text(&absolute("profile"));
        let actual =
            dirs_without(Platform::Windows, &[("USERPROFILE", &profile)], &["HOME"]).unwrap();
        assert_eq!(actual.home, absolute("profile"));
    }

    #[test]
    fn appdata_is_used_only_when_absolute() {
        for value in ["", "relative"] {
            let error = dirs(Platform::Windows, &[("APPDATA", value)])
                .err()
                .unwrap_or_else(|| panic!("APPDATA {value:?} was used"));
            assert_eq!(error, "APPDATA unavailable");
        }
        let actual = dirs(Platform::Windows, &[]).unwrap();
        assert_eq!(
            actual.user_config,
            absolute("roaming").join("toha/config.yml")
        );
        assert_eq!(actual.user_data, absolute("roaming").join("toha"));
    }

    #[test]
    fn localappdata_is_used_only_when_absolute() {
        for value in ["", "relative"] {
            let error = dirs(Platform::Windows, &[("LOCALAPPDATA", value)])
                .err()
                .unwrap_or_else(|| panic!("LOCALAPPDATA {value:?} was used"));
            assert_eq!(error, "LOCALAPPDATA unavailable");
        }
        let actual = dirs(Platform::Windows, &[]).unwrap();
        assert_eq!(actual.cache, absolute("local").join("toha/cache"));
        assert_eq!(actual.state, absolute("local").join("toha/staged"));
    }

    #[test]
    fn programdata_is_used_only_when_absolute() {
        for value in ["", "relative"] {
            let error = dirs(Platform::Windows, &[("PROGRAMDATA", value)])
                .err()
                .unwrap_or_else(|| panic!("PROGRAMDATA {value:?} was used"));
            assert_eq!(error, "PROGRAMDATA unavailable");
        }
        let actual = dirs(Platform::Windows, &[]).unwrap();
        assert_eq!(
            actual.system_config,
            absolute("program").join("toha/config.yml")
        );
        assert_eq!(actual.system_data, absolute("program").join("toha"));
    }
}
