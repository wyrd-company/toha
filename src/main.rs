mod cli {
    pub mod bundled;
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
    Applied, ApplyOptions, Completed, EndKind, Ended, Interview, Plan, Planned, ReviewDigest, Step,
    Template,
    context::{
        EnvironmentDecision, ExecutionFacts, FixedEnvironment, FixedEnvironmentSource, HostFacts,
        InvocationContext, SelectedTemplate,
    },
    hook::{ProcessRunner, ScriptedRunner},
    protocol::{
        self, AppliedFile, AppliedHook, ApplyReport, Context, DocumentStep, ErrorKind, Headless,
        ResultError, SubmitDocumentError, TrustState,
    },
    staging::{self, CanonicalTarget, StagedRecord, Store},
};

/// The command adapter's live environment source: the process environment and
/// native hostname. Called only under an explicit stage grant or a granted
/// direct/new-apply need; never on a denied, no-need, or missing-trust path.
#[derive(Default)]
struct HostEnvironmentSource;
impl FixedEnvironmentSource for HostEnvironmentSource {
    fn capture(&mut self) -> FixedEnvironment {
        FixedEnvironment::new(
            env_value(if cfg!(windows) { "USERNAME" } else { "USER" }),
            hostname(),
            env_value("EDITOR"),
            env_value("SHELL"),
            env_value("VISUAL"),
        )
    }
}
/// An environment value, or `None` when absent, empty, or non-Unicode.
fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}
#[cfg(unix)]
fn hostname() -> Option<String> {
    rustix::system::uname()
        .nodename()
        .to_str()
        .ok()
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
}
#[cfg(not(unix))]
fn hostname() -> Option<String> {
    None
}
/// Effective administrative authority: effective UID zero on Unix. No username
/// inference, elevation, timeout, or subprocess.
#[cfg(unix)]
fn is_admin() -> bool {
    rustix::process::geteuid().is_root()
}
#[cfg(not(unix))]
fn is_admin() -> bool {
    false
}
/// Builds a fresh current invocation context: admits environment access under
/// `decision`, captures host and execution facts, and assembles the typed
/// context. Only a stage refusal (`RequireStageGrant` with a need) errors here.
fn build_context(
    target: &CanonicalTarget,
    resolved: &ResolvedTemplate,
    template: &Template,
    decision: EnvironmentDecision,
    is_interactive: bool,
) -> Result<InvocationContext, String> {
    let mut source = HostEnvironmentSource;
    let environment = template
        .admit_environment(decision, &mut source)
        .map_err(|e| staging::stage_admission_error(e).to_string())?;
    let selected = SelectedTemplate::new(
        resolved.formal_name.clone(),
        template.name.clone(),
        resolved.aliases.clone(),
        resolved.source.clone(),
    );
    InvocationContext::new(
        target.clone(),
        selected,
        HostFacts::capture(),
        ExecutionFacts::new(is_admin(), is_interactive),
        environment,
    )
    .map_err(|e| e.to_string())
}
/// The effective-trust gate for a direct/new-apply environment decision: an
/// explicit `--trust`, or a current registry approval that matches the live
/// executable surface.
fn apply_environment_grant(trust: bool, resolved: &ResolvedTemplate, template: &Template) -> bool {
    if trust {
        return true;
    }
    match toha::HookSurface::of(template) {
        Ok(surface) => matches!(
            toha::evaluate_trust(resolved.approval.as_ref(), &surface.digest()),
            toha::Trust::Trusted
        ),
        Err(_) => false,
    }
}

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
    /// Without --async this is the person route: it prompts in a terminal and, at
    /// completion, shows the dry-run plan and the apply instructions. With --async
    /// it is the agent route: answer the batch with `continue PATH FILE`, then
    /// `apply PATH`.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    Stage {
        /// Alias, short name, formal name, git address, or folder of the template.
        template: String,
        /// Target directory.
        path: PathBuf,
        /// Emit the first question batch instead of prompting (the agent route).
        ///
        /// Writes the batch to FILE and the instructions to standard output, or the
        /// batch and instructions to standard output when no file is given.
        #[arg(
            short = 'a',
            long = "async",
            num_args = 0..=1,
            value_name = "FILE"
        )]
        r#async: Option<Option<String>>,
        /// Capture trusted environment values (user, hostname, editor, shell,
        /// visual) for this staged interview's templates that reference them.
        #[arg(long)]
        trust: bool,
    },
    /// Continue a staged interview with an answers document or terminal prompts.
    ///
    /// With FILE (the agent route) it answers one batch and writes the next batch
    /// and instructions, or the completion instructions when done. Without FILE
    /// (the person route) it prompts and, at completion, shows the dry-run plan.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    Continue {
        /// Target directory of the staged interview.
        path: PathBuf,
        /// Identity-bearing answers document for the current batch; - reads
        /// standard input. The document names the template it answers:
        /// {"template": <formal>, "answers": {...}}.
        ///
        /// When absent, toha prompts in the terminal for every remaining question,
        /// then shows the dry-run plan.
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
    /// `apply PATH` applies a complete staged interview, or reports the current
    /// batch of an incomplete one without prompting (the agent route).
    /// `apply TEMPLATE PATH` prompts in a terminal (the person route).
    /// `apply TEMPLATE PATH --answers FILE` is the scripted route: one shot, one
    /// JSON result document, never staged; it refuses when an interview is staged.
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
        /// Identity-bearing answers document for the scripted one-shot route; -
        /// reads standard input. The document names the template it answers:
        /// {"template": <formal>, "answers": {...}}.
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
    /// An agent-route result: an optional question-batch document followed by
    /// plain-text instructions, both on standard output. The batch is omitted
    /// when it was written to a file, or when the interview is already complete.
    Agent {
        document: Option<serde_json::Value>,
        instructions: String,
        code: u8,
    },
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
            Self::Agent {
                document,
                instructions,
                code,
            } => {
                if let Some(value) = document {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).expect("JSON value")
                    );
                    println!();
                }
                println!("{instructions}");
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
fn plan_lines(plan: &Plan, force: bool, edits: &[String]) -> Vec<String> {
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
    // Injection previews sit between the whole-file lines and the hook lines, the
    // order in which apply commits and then runs.
    lines.extend_from_slice(edits);
    for planned in &plan.hooks {
        // A deferred hook lists its source form: `run[0]` rendered, later
        // arguments and cwd showing their `{{ id.field }}` placeholders. It runs
        // nothing (dry-run), so no result is read.
        let (argv, cwd) = match planned {
            Planned::Ready(hook) => (hook.argv(), hook.cwd.as_ref().map(ToString::to_string)),
            Planned::AfterHooks(deferred) => deferred
                .hook_preview()
                .expect("a deferred plan hook previews as a hook"),
        };
        lines.push(format!(
            "hook {:?} cwd {}",
            argv,
            cwd.unwrap_or_else(|| ".".into())
        ));
    }
    lines
}

/// The dry-run injection lines: `inject`/`update <path> (<owner>)` for a missing
/// or changed region or value, and `<path> (<region>)` for a region drift.
/// Unchanged edits produce no line. Classifies each edit against the target's
/// current bytes.
#[allow(clippy::result_large_err)]
fn injection_report(
    plan: &Plan,
    target: &toha::staging::CanonicalTarget,
) -> Result<Vec<String>, toha::ApplyError> {
    use toha::{EditReport, PlannedEdit};
    let mut lines = Vec::new();
    for edit in &plan.edits {
        let full = target.as_path().join(edit.path().as_path());
        let current = std::fs::read(&full).ok();
        let (report, owner) = match edit {
            PlannedEdit::Region(region) => (
                toha::report_region_edit(current.as_deref(), region)?,
                region.region.to_string(),
            ),
            PlannedEdit::JsonValue(json) => (
                toha::report_json_edit(current.as_deref(), json)?,
                json.json_path.to_string(),
            ),
        };
        let path = edit.path();
        match report {
            EditReport::Inject => lines.push(format!("inject {path} ({owner})")),
            EditReport::Update => lines.push(format!("update {path} ({owner})")),
            EditReport::Drift => lines.push(format!("{path} ({owner})")),
            EditReport::Unchanged => {}
        }
    }
    Ok(lines)
}

/// Reads the answers document once as UTF-8 and prefixes a read failure with the
/// source name. It does not inspect the JSON; the protocol boundary parses and
/// verifies it.
fn read_answers_text(path: &str) -> Result<String, String> {
    let source = if path == "-" { "stdin" } else { path };
    if path == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map(|_| text)
            .map_err(|e| format!("{source}: cannot read answers document: {e}"))
    } else {
        fs::read_to_string(path).map_err(|e| format!("{source}: cannot read answers document: {e}"))
    }
}
fn report_config_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}
fn resolution(
    formal_name: &str,
    template: &Template,
    config: &toha::config::Config,
) -> Result<(toha::interview::Resolution, jiff::Zoned), String> {
    let now = match std::env::var("TOHA_NOW") {
        Ok(value) => value.parse().map_err(|e: jiff::Error| e.to_string())?,
        Err(std::env::VarError::NotPresent) => jiff::Zoned::now(),
        Err(e) => return Err(e.to_string()),
    };
    let resolution = toha::interview::configured_defaults(
        formal_name,
        template,
        &config.presets,
        &config.template_defaults,
    )
    .map_err(|e| e.to_string())?;
    report_config_warnings(resolution.warnings());
    Ok((resolution, now))
}
fn setup(path: &Path, dirs: &Dirs) -> Result<(staging::CanonicalTarget, Store), String> {
    let target = staging::canonical_target(path).map_err(|e| e.to_string())?;
    Ok((target, Store::new(dirs.state.clone())))
}
fn context(target: &staging::CanonicalTarget, record: &StagedRecord) -> Context {
    Context::new(target, record)
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
/// The configuration, registry, and directories that template names resolve against.
struct Scope<'a> {
    config: &'a toha::config::Config,
    registry: &'a toha::registry::Registry,
    dirs: &'a Dirs,
    cwd: &'a Path,
}
/// Whether the staged interview has questions remaining, for guidance only.
fn progress(saved: &StagedRecord, scope: &Scope, target: &CanonicalTarget) -> Progress {
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
        let resolution = toha::interview::configured_defaults(
            &saved.template,
            &template,
            &scope.config.presets,
            &scope.config.template_defaults,
        )
        .ok()?;
        report_config_warnings(resolution.warnings());
        // A flow stop/abort is terminal but distinct from complete: nothing can
        // be applied, so its guidance names starting over, not `apply`.
        Some(
            match saved
                .replay_with_resolution(&template, resolution, target)
                .ok()?
            {
                Interview::Complete(_) => Progress::Complete,
                Interview::Ended(_) => Progress::Ended,
                Interview::Asking(_) => Progress::Incomplete,
            },
        )
    })();
    replayed.unwrap_or(Progress::Unknown)
}
/// The refusal for `stage` or `apply` with a template when an interview is
/// staged at the path, or `None` when `apply` names the staged template.
fn staged_refusal(
    invocation: &Invocation,
    arg: &str,
    path: &Path,
    saved: &StagedRecord,
    scope: &Scope,
    target: &CanonicalTarget,
) -> Option<Outcome> {
    let formal =
        match cli::resolve::formal_name(arg, scope.config, scope.registry, scope.dirs, scope.cwd) {
            Ok(v) => v,
            Err(e) => return Some(resolve_error(e)),
        };
    let progress = || progress(saved, scope, target);
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
fn stage(
    template: String,
    path: PathBuf,
    output: Option<Option<String>>,
    trust: bool,
    dirs: &Dirs,
) -> Outcome {
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
        return staged_refusal(&invocation, &template, &path, &saved, &scope, &target)
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
    let (resolution, now) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    // Stage uses only its explicit flag: with it, capture the fixed five once
    // (even without a reference); without it, refuse a need before any progress.
    let decision = if trust {
        EnvironmentDecision::CarryStageGrant
    } else {
        EnvironmentDecision::RequireStageGrant
    };
    // The originating driver can prompt a human only on the terminal path.
    let interactive = output.is_none();
    let invocation = match build_context(&target, &resolved, &template, decision, interactive) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let saved = StagedRecord::new_with_context(
        invocation.clone(),
        resolved.commit.clone(),
        resolved.named,
        now.to_string(),
        vec![],
    );
    let interview = match resolution.start_with_context(&template, now, invocation) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let installed = trustable(&resolved, &registry);
    if output.is_none() {
        // Person route: prompt, saving each batch, then show the dry-run plan and
        // apply instructions at completion.
        let mut saved = saved;
        let completed = terminal::drive(interview, &mut terminal::InquireAsk, |submission| {
            saved.submissions.push(submission);
            store.save(&target, &saved).map_err(|e| e.to_string())
        });
        return match completed {
            Ok(terminal::Session::Completed(completed)) => {
                // An interview without prompts still needs a staged record.
                if saved.submissions.is_empty() {
                    if let Err(e) = store.save(&target, &saved) {
                        return Outcome::Error(e.to_string());
                    }
                }
                completion_preview(
                    &template,
                    &completed,
                    &target,
                    &resolved,
                    installed.as_deref(),
                    &path,
                    false,
                )
            }
            Ok(terminal::Session::Ended(ended)) => ended_outcome(&ended, &target, &store),
            Err(error) => Outcome::Error(error),
        };
    }
    // Agent route: emit the first question batch and the answer instructions.
    let output = output.unwrap();
    let formal = resolved.formal_name.clone();
    match interview {
        Interview::Asking(pending) => {
            let document =
                protocol::batch_document(pending.batch(), &context(&target, &saved), None);
            if let Err(e) = store.save(&target, &saved) {
                return Outcome::Error(e.to_string());
            }
            let instructions = instructions(&AgentOutcome::Batch { template: &formal }, &path);
            match output {
                Some(file) => {
                    if let Err(e) = fs::write(
                        file,
                        serde_json::to_vec_pretty(&document).expect("JSON value"),
                    ) {
                        return Outcome::Error(e.to_string());
                    }
                    Outcome::Agent {
                        document: None,
                        instructions,
                        code: 4,
                    }
                }
                None => Outcome::Agent {
                    document: Some(document),
                    instructions,
                    code: 4,
                },
            }
        }
        // A staged interview with no questions is complete; save it and point to
        // apply. There is no batch to write to a file.
        Interview::Complete(_) => {
            if let Err(e) = store.save(&target, &saved) {
                return Outcome::Error(e.to_string());
            }
            Outcome::Agent {
                document: None,
                instructions: instructions(&AgentOutcome::Complete, &path),
                code: 0,
            }
        }
        // A stop/abort at the first batch ends the interview: nothing is staged,
        // an abort removes any record, and the `ended` document reports it.
        Interview::Ended(ended) => {
            if let Err(e) = ended_removed(&ended, &target, &store) {
                return Outcome::Error(e);
            }
            eprintln!("{}", guidance::flow_ended(&ended));
            let document = protocol::ended_document(&ended, &context(&target, &saved));
            match output {
                Some(file) => match fs::write(
                    file,
                    serde_json::to_vec_pretty(&document).expect("JSON value"),
                ) {
                    Ok(()) => Outcome::Saved(0),
                    Err(e) => Outcome::Error(e.to_string()),
                },
                None => Outcome::Document(document, 0),
            }
        }
    }
}
/// The completion preview a person route shows at the end of `stage` or
/// `continue PATH`: the dry-run plan and the apply instructions, writing nothing
/// to the target. `show_messages` includes the interview's messages when they
/// were not already shown by the prompting driver.
fn completion_preview(
    template: &Template,
    completed: &Completed,
    target: &CanonicalTarget,
    resolved: &ResolvedTemplate,
    installed: Option<&str>,
    path: &Path,
    show_messages: bool,
) -> Outcome {
    let live_surface = match toha::HookSurface::of(template) {
        Ok(surface) => surface.digest(),
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let registry_trusted = matches!(
        toha::evaluate_trust(resolved.approval.as_ref(), &live_surface),
        toha::Trust::Trusted
    );
    let changed_since_approval = resolved.approval.is_some() && !registry_trusted;
    let plan = match Plan::build(template, completed, target) {
        Ok(plan) => plan,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let edits = match injection_report(&plan, target) {
        Ok(edits) => edits,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let mut lines: Vec<String> = if show_messages {
        completed.messages.clone()
    } else {
        Vec::new()
    };
    lines.extend(plan_lines(&plan, false, &edits));
    let invocation = Invocation::Apply {
        template: Some(Arg::Given(guidance::formal_for(
            "apply",
            &resolved.formal_name,
        ))),
        path,
        answers: None,
        force: false,
        dry_run: true,
        trust: false,
    };
    if plan.hooks.is_empty() || registry_trusted {
        lines.push(guidance::complete(path));
        Outcome::Written(lines)
    } else {
        for line in &lines {
            println!("{line}");
        }
        eprintln!(
            "{}",
            guidance::dry_run_needs_trust(&invocation, installed, changed_since_approval)
        );
        Outcome::Saved(0)
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
    let resolution = match toha::interview::configured_defaults(
        &resolved.formal_name,
        &template,
        &config.presets,
        &config.template_defaults,
    ) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    report_config_warnings(resolution.warnings());
    let installed = trustable(&resolved, &registry);
    let interview = match saved.replay_with_resolution(&template, resolution, &target) {
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
        // `continue PATH FILE` on a complete interview refuses before reading the
        // document. `continue PATH` shows the completion preview.
        if answers.is_some() {
            return Outcome::Error(guidance::complete_answers_unused(&path));
        }
        return completion_preview(
            &template,
            completed,
            &target,
            &resolved,
            installed.as_deref(),
            &path,
            true,
        );
    }
    if let Interview::Ended(ended) = &interview {
        // A prior submission ended the staged interview. `continue PATH FILE`
        // refuses before reading the document. Without a document, report the end.
        if answers.is_some() {
            return Outcome::Error(guidance::ended_answers_unused(
                &path,
                &saved.template,
                ended.kind(),
            ));
        }
        if let Err(e) = ended_removed(ended, &target, &store) {
            return Outcome::Error(e);
        }
        eprintln!("{}", guidance::flow_ended(ended));
        return Outcome::Document(
            protocol::ended_document(ended, &context(&target, &saved)),
            0,
        );
    }
    let Interview::Asking(pending) = interview else {
        unreachable!("a complete or ended interview returned above");
    };
    // `continue PATH` (person): prompt the remaining questions, saving each
    // batch, then show the completion preview.
    let Some(answers_file) = answers else {
        if !io::stdin().is_terminal() {
            return Outcome::Error(guidance::continue_no_terminal(&path));
        }
        return match terminal::drive(
            Interview::Asking(pending),
            &mut terminal::InquireAsk,
            |submission| {
                saved.submissions.push(submission);
                store.save(&target, &saved).map_err(|e| e.to_string())
            },
        ) {
            Ok(terminal::Session::Completed(completed)) => completion_preview(
                &template,
                &completed,
                &target,
                &resolved,
                installed.as_deref(),
                &path,
                false,
            ),
            Ok(terminal::Session::Ended(ended)) => ended_outcome(&ended, &target, &store),
            Err(error) => Outcome::Error(error),
        };
    };
    // `continue PATH FILE` (agent): submit one document as a single step.
    let text = match read_answers_text(&answers_file) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let formal = saved.template.clone();
    match protocol::answer_document_once(&formal, pending, &text) {
        Ok(DocumentStep::Accepted {
            interview,
            submission,
        }) => {
            // An accepted abort removes the record and saves no submission; every
            // other accepted step saves the one submission.
            if let Interview::Ended(ended) = &interview {
                if ended.kind() == EndKind::Abort {
                    if let Err(e) = store.remove(&target) {
                        return Outcome::Error(e.to_string());
                    }
                    eprintln!("{}", guidance::flow_ended(ended));
                    return Outcome::Document(
                        protocol::ended_document(ended, &context(&target, &saved)),
                        0,
                    );
                }
            }
            saved.submissions.push(submission);
            if let Err(e) = store.save(&target, &saved) {
                return Outcome::Error(e.to_string());
            }
            match interview {
                Interview::Asking(next) => Outcome::Agent {
                    document: Some(protocol::batch_document(
                        next.batch(),
                        &context(&target, &saved),
                        None,
                    )),
                    instructions: instructions(
                        &AgentOutcome::Batch {
                            template: &saved.template,
                        },
                        &path,
                    ),
                    code: 4,
                },
                Interview::Complete(_) => Outcome::Agent {
                    document: None,
                    instructions: instructions(&AgentOutcome::Complete, &path),
                    code: 0,
                },
                // A flow `stop` retains the saved record; the notice names the
                // abort command.
                Interview::Ended(ended) => {
                    eprintln!("{}", guidance::flow_ended(&ended));
                    Outcome::Document(
                        protocol::ended_document(&ended, &context(&target, &saved)),
                        0,
                    )
                }
            }
        }
        Ok(DocumentStep::Rejected {
            pending,
            rejections,
        }) => {
            // Nothing is saved for a rejected document.
            let errors = guidance::explain_rejections(&rejections, &path, &saved.template);
            Outcome::Agent {
                document: Some(protocol::batch_document(
                    pending.batch(),
                    &context(&target, &saved),
                    Some(&errors),
                )),
                instructions: instructions(
                    &AgentOutcome::Batch {
                        template: &saved.template,
                    },
                    &path,
                ),
                code: 4,
            }
        }
        Err(e) => Outcome::Error(document_error(e, &formal).message),
    }
}
/// The outcome of a flow `stop`/`abort` at any driver. Both write nothing and
/// exit 0; `Abort` additionally removes the target's staged record via the
/// existing `Store::remove` (a no-op when none exists; a removal I/O failure
/// surfaces as an error and exits nonzero). The engine reached this only after
/// a committed `Interview::Ended`, so a rejected document never removes a
/// record.
fn ended_removed(ended: &Ended, target: &CanonicalTarget, store: &Store) -> Result<(), String> {
    if ended.kind() == EndKind::Abort {
        store.remove(target).map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// The terminal/direct outcome of a flow end: a stderr notice and exit 0, with
/// the staged record removed on an abort.
fn ended_outcome(ended: &Ended, target: &CanonicalTarget, store: &Store) -> Outcome {
    if let Err(e) = ended_removed(ended, target, store) {
        return Outcome::Error(e);
    }
    eprintln!("{}", guidance::flow_ended(ended));
    Outcome::Written(vec![])
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

/// What an agent-route step produced, for the plain-text instructions that
/// follow the batch JSON on standard output.
enum AgentOutcome<'a> {
    /// A question batch is available; answer it with `continue PATH FILE`. Used
    /// by `stage --async` and `continue PATH FILE` while questions remain.
    Batch { template: &'a str },
    /// `apply PATH` found the staged interview incomplete: answer with
    /// `continue PATH FILE` or, in a terminal, `continue PATH`.
    ApplyIncomplete { template: &'a str },
    /// `continue PATH FILE` completed the interview: preview or write with
    /// `apply PATH`.
    Complete,
}
/// The instructions for an agent-route step, built from the parsed command
/// values and the outcome. It parses no JSON of its own.
fn instructions(outcome: &AgentOutcome, path: &Path) -> String {
    match outcome {
        AgentOutcome::Batch { template } => guidance::answer_batch(path, template),
        AgentOutcome::ApplyIncomplete { template } => {
            guidance::apply_incomplete_agent(path, template)
        }
        AgentOutcome::Complete => guidance::continue_complete(path),
    }
}
/// One scripted `error` result document with its tabled exit code.
fn scripted_error(
    kind: ErrorKind,
    message: String,
    commands: Vec<String>,
    context: Option<&Context>,
) -> Outcome {
    let code = if kind == ErrorKind::Ambiguous { 5 } else { 1 };
    Outcome::Document(
        protocol::error_document(
            &ResultError {
                kind,
                message,
                commands,
            },
            context,
        ),
        code,
    )
}
/// Maps a document or evaluation failure to a scripted `error` document. A
/// malformed envelope is `document`; a missing, malformed, or mismatched
/// identity is `identity`; an engine evaluation failure is `render`.
fn document_error(error: SubmitDocumentError, expected: &str) -> ResultError {
    use protocol::AnswersDocumentError as E;
    let (kind, message) = match error {
        SubmitDocumentError::Evaluation(error) => (ErrorKind::Render, error.to_string()),
        SubmitDocumentError::Document(error) => match &error {
            E::Json { .. } | E::Shape { .. } => (ErrorKind::Document, error.to_string()),
            E::MissingIdentity => (
                ErrorKind::Identity,
                format!(
                    "answers document has no template identity; wrap the answers as {{\"template\": {}, \"answers\": {{ ... }}}}",
                    serde_json::to_string(expected).expect("template string")
                ),
            ),
            E::MalformedIdentity => (
                ErrorKind::Identity,
                format!(
                    "answers document template must be a non-empty string equal to {}",
                    serde_json::to_string(expected).expect("template string")
                ),
            ),
            E::TemplateMismatch { .. } => (ErrorKind::Identity, error.to_string()),
        },
    };
    ResultError {
        kind,
        message,
        commands: vec![],
    }
}
/// Classifies each planned write against the target's current bytes before any
/// write: `create`/`overwrite`/`conflict` for a whole-file rule, `inject`/
/// `update` for an edit (by whether the region or value is present), carrying the
/// region key or JSON path an edit governs. A whole-file rule that a path also
/// edits keeps its create/overwrite action; the edit folds into the generated
/// image and is not classified against the not-yet-written target. An unchanged
/// edit is omitted, as it writes nothing.
#[allow(clippy::result_large_err)]
fn classify_paths(
    plan: &Plan,
    target: &CanonicalTarget,
    force: bool,
) -> Result<Vec<protocol::PlannedPath>, toha::ApplyError> {
    use toha::{EditReport, PlannedEdit};
    let mut paths = Vec::new();
    for file in &plan.files {
        let action = if plan.conflicts.contains(&file.path) {
            if force { "overwrite" } else { "conflict" }
        } else {
            "create"
        };
        paths.push(protocol::PlannedPath {
            path: file.path.to_string(),
            action: action.to_string(),
            owner: None,
        });
    }
    for edit in &plan.edits {
        if plan.files.iter().any(|file| file.path == *edit.path()) {
            continue;
        }
        let full = target.as_path().join(edit.path().as_path());
        let current = std::fs::read(&full).ok();
        let (report, owner) = match edit {
            PlannedEdit::Region(region) => (
                toha::report_region_edit(current.as_deref(), region)?,
                region.region.to_string(),
            ),
            PlannedEdit::JsonValue(json) => (
                toha::report_json_edit(current.as_deref(), json)?,
                json.json_path.to_string(),
            ),
        };
        let action = match report {
            EditReport::Inject => "inject",
            EditReport::Update | EditReport::Drift => "update",
            EditReport::Unchanged => continue,
        };
        paths.push(protocol::PlannedPath {
            path: edit.path().to_string(),
            action: action.to_string(),
            owner: Some(owner),
        });
    }
    Ok(paths)
}
/// Maps an apply failure to a scripted `error` document by kind.
fn scripted_apply_error(error: toha::ApplyError, context: &Context) -> Outcome {
    use toha::ApplyError as E;
    let kind = match &error {
        E::Conflicts(_) | E::Drift(_) | E::Symlink(_) => ErrorKind::Conflict,
        E::Hook { .. } | E::HookIo { .. } | E::HookOutput { .. } => ErrorKind::Hook,
        E::Io { .. } | E::Deferred(_) | E::Region(_) | E::Json(_) => ErrorKind::Render,
    };
    scripted_error(kind, error.to_string(), vec![], Some(context))
}
/// The scripted route: `apply TEMPLATE PATH --answers FILE`. One shot. It never
/// stages, refuses when an interview is staged (before reading the document),
/// and writes exactly one JSON result document for every outcome.
fn scripted(
    template_arg: String,
    path: &Path,
    answers_file: String,
    force: bool,
    dry_run: bool,
    trust: bool,
    dirs: &Dirs,
) -> Outcome {
    // 1. Construct one canonical target and check for a staged record.
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Input, e, vec![], None),
    };
    let existing = match store.load(&target) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Staged, e.to_string(), vec![], None),
    };
    // 2. Refuse when a record exists; the document is not read.
    if existing.is_some() {
        return scripted_error(
            ErrorKind::Staged,
            format!(
                "an interview is staged at {target}; the scripted route does not use staged state"
            ),
            guidance::staged_commands(path),
            None,
        );
    }
    let cwd = match std::env::current_dir() {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Input, e.to_string(), vec![], None),
    };
    let (config, registry) = match cli::resolve::load_context(dirs, &cwd) {
        Ok(v) => v,
        Err(e) => return scripted_resolve_error(e, path, &answers_file, force, dry_run, trust),
    };
    // 3. Resolve the command template, load it, produce the configured
    //    Resolution, and start the interview.
    let resolved =
        match cli::resolve::resolve_template(&template_arg, &config, &registry, dirs, &cwd) {
            Ok(v) => v,
            Err(e) => return scripted_resolve_error(e, path, &answers_file, force, dry_run, trust),
        };
    let template = match load_template(&resolved) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Source, e, vec![], None),
    };
    let (resolution, now) = match resolution(&resolved.formal_name, &template, &config) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Source, e, vec![], None),
    };
    let decision =
        EnvironmentDecision::grant_if_needed(apply_environment_grant(trust, &resolved, &template));
    let new_context = match build_context(&target, &resolved, &template, decision, false) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Source, e, vec![], None),
    };
    // An in-memory record carries the context for the result documents; the
    // scripted route never saves or removes it.
    let saved = StagedRecord::new_with_context(
        new_context.clone(),
        resolved.commit.clone(),
        resolved.named,
        now.to_string(),
        vec![],
    );
    let ctx = context(&target, &saved);
    let interview = match resolution.start_with_context(&template, now, new_context) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Render, e.to_string(), vec![], Some(&ctx)),
    };
    // 4. Read the document once as UTF-8.
    let text = match read_answers_text(&answers_file) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Input, e, vec![], Some(&ctx)),
    };
    // 5-8. Parse, verify identity, and drive the headless walk.
    let headless = match protocol::answer_document_headless(
        &resolved.formal_name,
        &template,
        interview,
        &text,
    ) {
        Ok(v) => v,
        Err(e) => {
            let error = document_error(e, &resolved.formal_name);
            let code = if error.kind == ErrorKind::Ambiguous {
                5
            } else {
                1
            };
            return Outcome::Document(protocol::error_document(&error, Some(&ctx)), code);
        }
    };
    match headless {
        Headless::Pending {
            pending,
            rejections,
            ..
        } => {
            // The headless walk reaches each unanswered required question and
            // rejects it with `is required`; the scripted `questions` document
            // carries those rejections. Agent routes take one step and leave an
            // unanswered question pending without such an error.
            Outcome::Document(
                protocol::batch_document(pending.batch(), &ctx, Some(&rejections)),
                4,
            )
        }
        Headless::Ended { ended, .. } => {
            // The scripted route never staged, so an abort removes nothing.
            Outcome::Document(protocol::ended_document(&ended, &ctx), 0)
        }
        Headless::Completed { completed, .. } => scripted_completed(
            &template, completed, &target, &resolved, &ctx, force, dry_run, trust,
        ),
    }
}
/// The scripted `Completed` tail: build the plan, then report `planned` (dry run
/// or untrusted hooks) or apply and report `applied`.
#[allow(clippy::too_many_arguments)]
fn scripted_completed(
    template: &Template,
    completed: Completed,
    target: &CanonicalTarget,
    resolved: &ResolvedTemplate,
    ctx: &Context,
    force: bool,
    dry_run: bool,
    trust: bool,
) -> Outcome {
    let dry_run = dry_run || matches!(completed.step(), Step::Plan { apply: false });
    let live_surface = match toha::HookSurface::of(template) {
        Ok(surface) => surface.digest(),
        Err(error) => {
            return scripted_error(ErrorKind::Render, error.to_string(), vec![], Some(ctx));
        }
    };
    let trusted = trust
        || matches!(
            toha::evaluate_trust(resolved.approval.as_ref(), &live_surface),
            toha::Trust::Trusted
        );
    let plan = match Plan::build(template, &completed, target) {
        Ok(plan) => plan,
        Err(error) => {
            return scripted_error(ErrorKind::Render, error.to_string(), vec![], Some(ctx));
        }
    };
    let mut messages = completed.messages.clone();
    if let Some(before) = &plan.before_apply {
        messages.push(before.clone());
    }
    let has_hooks = !plan.hooks.is_empty();
    let trust_state = if trusted {
        TrustState::Trusted
    } else {
        TrustState::Untrusted
    };
    // The planned paths, classified against the target before any write. Shared
    // by the planned and applied documents.
    let classified = match classify_paths(&plan, target, force) {
        Ok(v) => v,
        Err(e) => return scripted_apply_error(e, ctx),
    };
    if dry_run {
        let code = if has_hooks && !trusted { 3 } else { 0 };
        return Outcome::Document(
            protocol::planned_document(&classified, &plan, &messages, ctx, trust_state),
            code,
        );
    }
    if has_hooks && !trusted {
        return Outcome::Document(
            protocol::planned_document(&classified, &plan, &messages, ctx, TrustState::Untrusted),
            3,
        );
    }
    let actions: std::collections::HashMap<String, String> = classified
        .iter()
        .map(|entry| (entry.path.clone(), entry.action.clone()))
        .collect();
    match plan.apply_reporting(
        target,
        ApplyOptions {
            force,
            trusted: true,
        },
        // The scripted route keeps standard output a single JSON document: an
        // uncaptured hook's stdout is forwarded to standard error.
        &ScriptedRunner,
        &mut |_| {},
    ) {
        Ok(Applied::Written {
            files,
            hooks,
            after_apply,
            ..
        }) => {
            if let Some(after) = after_apply {
                messages.push(after);
            }
            let report = ApplyReport {
                files: files
                    .iter()
                    .map(|path| AppliedFile {
                        path: path.to_string(),
                        action: actions
                            .get(&path.to_string())
                            .cloned()
                            .unwrap_or_else(|| "create".to_string()),
                    })
                    .collect(),
                messages,
                hooks: hooks
                    .iter()
                    .map(|hook| AppliedHook {
                        id: hook.id.as_ref().map(ToString::to_string),
                        exit_code: hook.exit_code,
                        success: hook.success,
                    })
                    .collect(),
            };
            Outcome::Document(protocol::applied_document(&report, ctx), 0)
        }
        // Trust was granted above, so an untrusted plan cannot occur here.
        Ok(Applied::NeedsTrust(_)) => scripted_error(
            ErrorKind::Hook,
            "hooks are not trusted".into(),
            vec![],
            Some(ctx),
        ),
        Err(error) => scripted_apply_error(error, ctx),
    }
}
/// Maps a resolve failure on the scripted route to a `source` or `ambiguous`
/// error document. The ambiguous document lists the scripted command with each
/// match's formal name.
fn scripted_resolve_error(
    error: ResolveError,
    path: &Path,
    answers_file: &str,
    force: bool,
    dry_run: bool,
    trust: bool,
) -> Outcome {
    match error {
        ResolveError::Error(message) => scripted_error(ErrorKind::Source, message, vec![], None),
        ResolveError::Ambiguous { name, matches } => {
            let commands = matches
                .iter()
                .map(|formal| {
                    Invocation::Apply {
                        template: Some(Arg::Given(guidance::formal_for("apply", formal))),
                        path,
                        answers: Some(Arg::Given(answers_file)),
                        force,
                        dry_run,
                        trust,
                    }
                    .command()
                })
                .collect();
            scripted_error(
                ErrorKind::Ambiguous,
                format!("ambiguous template name: {name}"),
                commands,
                None,
            )
        }
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
    // Scripted route: apply TEMPLATE PATH --answers FILE. One shot, one result
    // document, never staged.
    if let (Some(template), Some(answers_file)) = (template.clone(), answers.clone()) {
        return scripted(template, path, answers_file, force, dry_run, trust, dirs);
    }
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
    // apply PATH --answers FILE with no template: the scripted route requires a
    // template. There is no target-only answers route.
    if answers.is_some() {
        let staged = existing
            .as_ref()
            .map(|saved| (saved.template.as_str(), progress(saved, &scope, &target)));
        return Outcome::Error(guidance::answers_without_template(
            path,
            answers.as_deref().unwrap_or_default(),
            staged,
        ));
    }
    // A template argument selects the person route (`apply TEMPLATE PATH`), which
    // prompts; its absence selects the agent route (`apply PATH`), which never
    // prompts.
    let named = template.is_some();
    let requested_template = template.clone();
    let invocation = Invocation::Apply {
        template: requested_template.as_deref().map(Arg::Given),
        path,
        answers: None,
        force,
        dry_run,
        trust,
    };
    if let (Some(arg), Some(saved)) = (&template, &existing) {
        if let Some(refusal) = staged_refusal(&invocation, arg, path, saved, &scope, &target) {
            return refusal;
        }
    }
    // A template named for its own staged interview resumes that interview.
    let template = template.filter(|_| existing.is_none());
    let mut terminal_run = false;
    // The stored approval digest of the template resolved by name, compared to
    // the live executable surface once the template is loaded.
    let approval: Option<ReviewDigest>;
    // The installed template whose registry trust would run the hooks.
    let installed: Option<String>;
    let (template, interview, saved) = match (template, existing) {
        // apply TEMPLATE PATH with nothing staged: the person route starts the
        // interview, prompts, saves at each batch boundary, and applies at
        // completion.
        (Some(folder), None) => {
            if !io::stdin().is_terminal() {
                return Outcome::Error(guidance::no_terminal(&folder, path));
            }
            let resolved =
                match cli::resolve::resolve_template(&folder, &config, &registry, dirs, &cwd) {
                    Ok(v) => v,
                    Err(e) => return resolve_error(e),
                };
            approval = resolved.approval.clone();
            installed = trustable(&resolved, &registry);
            let template = match load_template(&resolved) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let (resolution, now) = match resolution(&resolved.formal_name, &template, &config) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            // Direct/new-apply grants environment access on explicit `--trust`
            // or a current matching reviewed approval; otherwise it denies.
            let decision = EnvironmentDecision::grant_if_needed(apply_environment_grant(
                trust, &resolved, &template,
            ));
            let new_context = match build_context(&target, &resolved, &template, decision, true) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let mut saved = StagedRecord::new_with_context(
                new_context.clone(),
                resolved.commit.clone(),
                resolved.named,
                now.to_string(),
                vec![],
            );
            let interview = match resolution.start_with_context(&template, now, new_context) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            // Save each accepted batch so an interrupt leaves a resumable record;
            // a dry run records nothing.
            match terminal::drive(interview, &mut terminal::InquireAsk, |submission| {
                if dry_run {
                    return Ok(());
                }
                saved.submissions.push(submission);
                store.save(&target, &saved).map_err(|e| e.to_string())
            }) {
                Ok(terminal::Session::Completed(completed)) => {
                    terminal_run = true;
                    (template, completed, Some(saved))
                }
                Ok(terminal::Session::Ended(ended)) => {
                    return ended_outcome(&ended, &target, &store);
                }
                Err(e) => return Outcome::Error(e),
            }
        }
        // apply PATH (agent) or apply TEMPLATE PATH naming the staged template
        // (person) resumes the staged interview. The agent route never prompts;
        // the person route prompts the remaining questions.
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
            approval = resolved.approval.clone();
            installed = trustable(&resolved, &registry);
            let template = match load_template(&resolved) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let resolution = match toha::interview::configured_defaults(
                &resolved.formal_name,
                &template,
                &config.presets,
                &config.template_defaults,
            ) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            report_config_warnings(resolution.warnings());
            let interview = match saved.replay_with_resolution(&template, resolution, &target) {
                Ok(v) => v,
                Err(e) => {
                    return Outcome::Error(guidance::replay_failed(
                        path,
                        &saved.template,
                        &e.to_string(),
                    ));
                }
            };
            match interview {
                Interview::Ended(ended) => {
                    return ended_outcome(&ended, &target, &store);
                }
                Interview::Complete(completed) => (template, completed, Some(saved)),
                // apply TEMPLATE PATH resume: the person route prompts the rest,
                // saving each batch, then applies.
                Interview::Asking(pending) if named => {
                    if !io::stdin().is_terminal() {
                        return Outcome::Error(guidance::continue_no_terminal(path));
                    }
                    match terminal::drive(
                        Interview::Asking(pending),
                        &mut terminal::InquireAsk,
                        |submission| {
                            if dry_run {
                                return Ok(());
                            }
                            saved.submissions.push(submission);
                            store.save(&target, &saved).map_err(|e| e.to_string())
                        },
                    ) {
                        Ok(terminal::Session::Completed(c)) => {
                            terminal_run = true;
                            (template, c, Some(saved))
                        }
                        Ok(terminal::Session::Ended(ended)) => {
                            return ended_outcome(&ended, &target, &store);
                        }
                        Err(e) => return Outcome::Error(e),
                    }
                }
                // apply PATH: the agent route reports the current batch with
                // instructions and never prompts. Nothing new is saved.
                Interview::Asking(pending) => {
                    return Outcome::Agent {
                        document: Some(protocol::batch_document(
                            pending.batch(),
                            &context(&target, &saved),
                            None,
                        )),
                        instructions: instructions(
                            &AgentOutcome::ApplyIncomplete {
                                template: &saved.template,
                            },
                            path,
                        ),
                        code: 4,
                    };
                }
            }
        }
        (None, None) => {
            return Outcome::Error(guidance::nothing_staged(path));
        }
        (Some(_), Some(_)) => unreachable!("a named template resumes its staged interview"),
    };
    let completed = interview;
    // A flow `dry-run` composes with the CLI `--dry-run` by union: either
    // suppresses the apply.
    let dry_run = dry_run || matches!(completed.step(), Step::Plan { apply: false });
    // Trust is the live executable surface matching the stored approval; the
    // one flag `--trust` overrides for a single run. This is the only place a
    // template becomes trusted from stored state.
    let live_surface = match toha::HookSurface::of(&template) {
        Ok(surface) => surface.digest(),
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let registry_trusted = matches!(
        toha::evaluate_trust(approval.as_ref(), &live_surface),
        toha::Trust::Trusted
    );
    // An approval that no longer matches marks the hooks changed since approval.
    let changed_since_approval = approval.is_some() && !registry_trusted;
    let plan = match Plan::build(&template, &completed, &target) {
        Ok(plan) => plan,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    if dry_run {
        let edits = match injection_report(&plan, &target) {
            Ok(edits) => edits,
            Err(error) => return Outcome::Error(error.to_string()),
        };
        let lines = if terminal_run {
            vec![]
        } else {
            completed.messages
        }
        .into_iter()
        .chain(plan_lines(&plan, force, &edits));
        if plan.hooks.is_empty() || registry_trusted || trust {
            return Outcome::Written(lines.collect());
        }
        for line in lines {
            println!("{line}");
        }
        // Written after the plan, so that a terminal shows it last.
        eprintln!(
            "{}",
            guidance::dry_run_needs_trust(
                &invocation,
                installed.as_deref(),
                changed_since_approval
            )
        );
        return Outcome::Saved(0);
    }
    let before = plan.before_apply.clone();
    let preview = plan_lines(&plan, force, &[]);
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
        &target,
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
            Outcome::NeedsTrust(guidance::needs_trust(
                &invocation,
                installed.as_deref(),
                changed_since_approval,
            ))
        }
        Err(error @ (toha::ApplyError::Conflicts(_) | toha::ApplyError::Drift(_))) => {
            Outcome::Error(format!("{error}\n{}", guidance::conflicts(&invocation)))
        }
        Err(error) => Outcome::Error(error.to_string()),
    }
}

fn main() -> ExitCode {
    // Parse first so that clap handles --version, --help, and usage errors
    // without a home directory; only the commands below resolve the folders.
    let command = Cli::parse().command;
    // Skills reads neither configuration nor state, so it resolves no directory
    // and runs without a home directory, like --version and --help.
    if let Command::Skills { command } = command {
        return skills::run(command).finish();
    }
    let dirs = match Dirs::resolve() {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e).finish(),
    };
    match command {
        Command::Skills { .. } => unreachable!("the home-free skills command returned above"),
        Command::Stage {
            template,
            path,
            r#async,
            trust,
        } => stage(template, path.clone(), r#async.clone(), trust, &dirs)
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
