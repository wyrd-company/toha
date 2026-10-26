fn runtime_now() -> Result<jiff::Zoned, String> {
    match std::env::var("TOHA_NOW") {
        Ok(value) => value.parse().map_err(|e: jiff::Error| e.to_string()),
        Err(std::env::VarError::NotPresent) => Ok(jiff::Zoned::now()),
        Err(e) => Err(e.to_string()),
    }
}
mod cli {
    pub mod guidance;
    pub mod like;
    pub mod resolve;
    pub mod templates;
}
mod parts_output;
mod update;
mod update_output;
mod update_stage;
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
    Applied, ApplyOptions, Completed, Interview, Plan, Planned, Template,
    context::{
        EnvironmentDecision, ExecutionFacts, FixedEnvironment, FixedEnvironmentSource, HostFacts,
        InvocationContext,
    },
    git_target::GitReport,
    hook::{ProcessRunner, ScriptedRunner},
    protocol::{
        self, AppliedFile, AppliedHook, ApplyReport, Context, ErrorKind, ResultError,
        SubmitDocumentError, TrustState,
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
    toha::operations::invocation_context(
        target,
        resolved,
        template,
        decision,
        HostFacts::capture(),
        ExecutionFacts::new(is_admin(), is_interactive),
        &mut HostEnvironmentSource,
    )
    .map_err(|e| e.to_string())
}
/// The effective-trust gate for a direct/new-apply environment decision: an
/// explicit `--trust`, or a current registry approval that matches the live
/// executable surface.
fn apply_environment_grant(trust: bool, resolved: &ResolvedTemplate, template: &Template) -> bool {
    toha::operations::effective_trust(trust, resolved, template)
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
  TOHA_RECORD       Applied record path relative to the Git repository root. Default: .toha.json.
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
        /// Read a JSON contract document, routed by its exact $schema URI. Repeatable.
        #[arg(long, value_name = "FILE")]
        input: Vec<PathBuf>,
        /// Add an optional part. Repeatable.
        #[arg(long = "with", value_name = "PART")]
        with: Vec<String>,
        /// Remove an optional part. Repeatable.
        #[arg(long = "without", value_name = "PART")]
        without: Vec<String>,
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
        /// Update from the applied record for this source and target.
        #[arg(long, conflicts_with = "baseline")]
        update: bool,
        /// Stage an update from an empty base.
        #[arg(long)]
        baseline: bool,
        /// Re-ask every recorded answer instead of replaying it.
        #[arg(long, requires = "update")]
        reanswer: bool,
        /// Seed answer defaults from an application of the same source at TARGET.
        /// Omit the value to choose an application in a terminal.
        #[arg(long, value_name = "SELECTOR", num_args = 0..=1, conflicts_with_all = ["update", "baseline"])]
        like: Option<Option<String>>,
    },
    /// Continue a staged interview with an answers document or terminal prompts.
    ///
    /// With FILE (the agent route) it answers one batch and writes the next batch
    /// and instructions, or the completion instructions when done. Without FILE
    /// (the person route) it prompts and, at completion, shows the dry-run plan.
    ///
    /// Multiline input uses an editor, or lines ending with . when no editor is available.
    Continue {
        /// Submit a JSON contract document, routed by its exact $schema URI.
        #[arg(long, value_name = "FILE", conflicts_with = "answers")]
        input: Vec<PathBuf>,
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
        /// Read a JSON contract document, routed by its exact $schema URI. Repeatable.
        #[arg(long, value_name = "FILE")]
        input: Vec<PathBuf>,
        /// Add an optional part. Repeatable.
        #[arg(long = "with", value_name = "PART")]
        with: Vec<String>,
        /// Remove an optional part. Repeatable.
        #[arg(long = "without", value_name = "PART")]
        without: Vec<String>,
        /// One operand is the target directory; two are the template and the target directory.
        #[arg(num_args = 1..=2, required = true, value_name = "PATH", hide = true)]
        paths: Vec<String>,
        /// Identity-bearing answers document for the scripted one-shot route; -
        /// reads standard input. The document names the template it answers:
        /// {"template": <formal>, "answers": {...}}.
        #[arg(short = 'A', long, value_name = "FILE")]
        answers: Option<String>,
        /// Overwrite existing files or replace drifted managed regions.
        #[arg(short, long)]
        force: bool,
        /// Print the files, messages, and hooks apply would produce, and change nothing.
        #[arg(short = 'd', long)]
        dry_run: bool,
        /// Run the hooks of this template for this run.
        #[arg(long)]
        trust: bool,
        /// Update from the applied record for this source and target.
        #[arg(long, conflicts_with = "baseline")]
        update: bool,
        /// Update from an empty base: merge the whole render into the target,
        /// three-way against nothing, so unrelated local edits survive.
        #[arg(long)]
        baseline: bool,
        /// Re-ask every recorded answer instead of replaying it.
        #[arg(long, requires = "update")]
        reanswer: bool,
        /// Seed answer defaults from an application of the same source at TARGET.
        /// Omit the value to choose an application in a terminal.
        #[arg(long, value_name = "SELECTOR", num_args = 0..=1, conflicts_with_all = ["update", "baseline"])]
        like: Option<Option<String>>,
    },
}

use toha::resolve::Paths as Dirs;
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
trait RuntimeDirs: Sized {
    fn resolve() -> Result<Self, String>;
    fn from_env(platform: Platform, var: impl Fn(&str) -> Option<OsString>)
    -> Result<Self, String>;
}
impl RuntimeDirs for Dirs {
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
}

enum Outcome {
    Written(Vec<String>),
    Error(String),
    Usage(String),
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
    /// Write the outcome to standard output/error and return the exit code. A
    /// broken pipe or other output error at this reporting boundary is surfaced
    /// as a failure exit code — it never panics, so a report failure after a
    /// merge has committed leaves the committed result, index, worktree, and ref
    /// intact (this boundary holds no repository handle and mutates nothing).
    fn finish(self) -> ExitCode {
        let mut out = std::io::stdout().lock();
        let mut err = std::io::stderr().lock();
        match self.render(&mut out, &mut err) {
            Ok(code) => code,
            // The committed work already stands; report the output failure.
            Err(_) => ExitCode::from(1),
        }
    }

    /// Render the outcome to the given writers, propagating any output error
    /// instead of panicking. Separated from [`finish`] so the reporting boundary
    /// is exercisable with a failing writer.
    fn render(
        self,
        out: &mut dyn std::io::Write,
        err: &mut dyn std::io::Write,
    ) -> std::io::Result<ExitCode> {
        match self {
            Self::Written(lines) => {
                for line in lines {
                    writeln!(out, "{line}")?;
                }
                Ok(ExitCode::SUCCESS)
            }
            Self::Error(message) => {
                writeln!(err, "{message}")?;
                Ok(ExitCode::from(1))
            }
            Self::Usage(message) => {
                writeln!(err, "{message}")?;
                Ok(ExitCode::from(2))
            }
            Self::Saved(code) => Ok(ExitCode::from(code)),
            Self::Document(value, code) => {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&value).expect("JSON value")
                )?;
                Ok(ExitCode::from(code))
            }
            Self::Agent {
                document,
                instructions,
                code,
            } => {
                if let Some(value) = document {
                    writeln!(
                        out,
                        "{}",
                        serde_json::to_string_pretty(&value).expect("JSON value")
                    )?;
                    writeln!(out)?;
                }
                writeln!(out, "{instructions}")?;
                Ok(ExitCode::from(code))
            }
            Self::Ambiguous {
                name,
                matches,
                retry,
            } => {
                writeln!(err, "{}", guidance::ambiguous(&name, &matches, &retry))?;
                Ok(ExitCode::from(5))
            }
            Self::NeedsTrust(message) => {
                writeln!(err, "{message}")?;
                Ok(ExitCode::from(3))
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
    // The repository is initialized after the files and before the hooks.
    lines.extend(
        plan.git
            .as_ref()
            .map(toha::git_target::PlannedGit::preview_line),
    );
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

fn document_input_error(error: toha::document::PrepareError, context: Option<&Context>) -> Outcome {
    let usage = error.is_usage();
    let message = error.to_string();
    if let Some(context) = context {
        let error = ResultError {
            kind: ErrorKind::Input,
            message,
            commands: vec![],
        };
        Outcome::Document(
            protocol::error_document(&error, Some(context)),
            if usage { 2 } else { 1 },
        )
    } else if usage {
        Outcome::Usage(message)
    } else {
        Outcome::Error(message)
    }
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
) -> Result<jiff::Zoned, String> {
    let now = runtime_now()?;
    report_config_warnings(
        &toha::operations::configuration_warnings(formal_name, template, config)
            .map_err(|e| e.to_string())?,
    );
    Ok(now)
}
/// The parsed `--like` request from clap's `Option<Option<String>>`: absent, a
/// bare flag (picker on the person route; usage error elsewhere), or an explicit
/// selector.
enum LikeFlag {
    Absent,
    Bare,
    Select(cli::like::LikeSelector),
}
impl LikeFlag {
    fn parse(like: Option<Option<String>>) -> Self {
        match like {
            None => LikeFlag::Absent,
            Some(None) => LikeFlag::Bare,
            Some(Some(value)) => LikeFlag::Select(cli::like::LikeSelector::Reference(value)),
        }
    }
    fn is_present(&self) -> bool {
        !matches!(self, LikeFlag::Absent)
    }
}
/// Why a `--like` request could not be resolved: a usage error (bare selector on
/// a non-interactive route, exit 2) or a record error (exit 1).
struct LikeReject {
    message: String,
    /// `true` for a usage error (exit 2); `false` for a record error (exit 1).
    usage: bool,
}
impl LikeReject {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            usage: true,
        }
    }
}
impl From<cli::like::LikeError> for LikeReject {
    fn from(error: cli::like::LikeError) -> Self {
        Self {
            message: match error {
                cli::like::LikeError::NoProject => {
                    "the target is not inside a git repository; --like needs a recorded application"
                        .into()
                }
                cli::like::LikeError::Missing { name, target } => format!(
                    "no application of {name} at target {target}; --like selects a target in the applied record"
                ),
                other => other.to_string(),
            },
            usage: false,
        }
    }
}
/// A repository-open or read fault while resolving `--like` is a record error
/// (exit 1), not a usage error.
fn project_reject(error: toha::applied::ProjectError) -> LikeReject {
    LikeReject {
        message: error.to_string(),
        usage: false,
    }
}
/// Resolve a `--like` request against the target repository into a seed, or
/// `None` when there is nothing to seed from and the request does not require a
/// application (flag absent, or a person-route picker with no candidates or a
/// "none" choice). `interactive` selects the person route, where a bare `--like`
/// opens a picker; on every other route a bare `--like` is a usage error.
fn resolve_like_seed(
    flag: LikeFlag,
    interactive: bool,
    source: &str,
    target: &CanonicalTarget,
) -> Result<Option<cli::like::LikeSeed>, LikeReject> {
    let selector = match flag {
        LikeFlag::Absent => return Ok(None),
        LikeFlag::Bare if !interactive => {
            return Err(LikeReject::usage(
                "--like needs a selector here: name an application target",
            ));
        }
        LikeFlag::Bare => None, // the person picker
        LikeFlag::Select(selector) => Some(selector),
    };
    let project = toha::applied::Project::open(target).map_err(project_reject)?;
    match selector {
        Some(cli::like::LikeSelector::Reference(reference)) => {
            let application = cli::like::find_required(project.as_ref(), source, &reference)?;
            Ok(Some(cli::like::seed(&application)))
        }
        Some(_) => unreachable!("CLI constructs reference selectors"),
        // Bare, interactive: the person picker. No project or no candidate means
        // there is nothing to offer, so proceed with a normal apply.
        None => {
            let Some(project) = project.as_ref() else {
                return Ok(None);
            };
            let candidates = cli::like::candidates(project, source).map_err(project_reject)?;
            if candidates.is_empty() {
                return Ok(None);
            }
            match pick_application(&candidates).map_err(LikeReject::usage)? {
                Some(index) => Ok(Some(cli::like::seed(&candidates[index]))),
                None => Ok(None),
            }
        }
    }
}
/// The person-route picker: list the source's applications in target order and let
/// the person choose one to seed from, or "none". Returns the chosen index, or
/// `None` for "none". Prompts only here; the pure selection functions never do.
fn pick_application(candidates: &[toha::applied::Application]) -> Result<Option<usize>, String> {
    const NONE_LABEL: &str = "none (ask every question)";
    let mut options: Vec<String> = candidates
        .iter()
        .map(|application| format!("{} ({})", application.target(), application.source()))
        .collect();
    options.push(NONE_LABEL.to_owned());
    let chosen = inquire::Select::new("Seed defaults from which application?", options.clone())
        .prompt()
        .map_err(|error| error.to_string())?;
    Ok(options
        .iter()
        .position(|option| option == &chosen)
        .and_then(|index| (index < candidates.len()).then_some(index)))
}
fn setup(path: &Path, dirs: &Dirs) -> Result<(staging::CanonicalTarget, Store), String> {
    let target = staging::canonical_target(path).map_err(|e| e.to_string())?;
    if let Some(project) = toha::applied::Project::open(&target).map_err(|e| e.to_string())? {
        project.read_record().map_err(|e| e.to_string())?;
    } else {
        toha::applied::validate_record_path(target.as_path()).map_err(|e| e.to_string())?;
    }
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
/// Loads the template an interview is about to drive and confirms its
/// `requires` programs are on `PATH`, so a caller answers no question for an
/// apply whose hooks cannot run.
fn environment(
    dirs: &Dirs,
) -> Result<(toha::config::Config, toha::registry::Registry, PathBuf), Outcome> {
    let cwd = std::env::current_dir().map_err(|e| Outcome::Error(e.to_string()))?;
    let (config, registry) = cli::resolve::load_context(dirs, &cwd).map_err(resolve_error)?;
    Ok((config, registry, cwd))
}
fn resolve_error(error: ResolveError) -> Outcome {
    match error {
        error @ (ResolveError::Error(_)
        | ResolveError::NotFound(_)
        | ResolveError::StaleRevision { .. }) => Outcome::Error(error.to_string()),
        ResolveError::Ambiguous { name, matches } => Outcome::Ambiguous {
            name,
            matches,
            retry: vec![],
        },
        other => Outcome::Error(other.to_string()),
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
    let session = match toha::operations::Session::resume(
        saved.clone(),
        target.clone(),
        scope.dirs,
        scope.config.clone(),
        scope.registry,
        scope.cwd,
    ) {
        Ok(v) => v,
        Err(_) => return Progress::Unknown,
    };
    if let Ok(warnings) = session.warnings() {
        report_config_warnings(&warnings);
    }
    match session.state() {
        Ok(toha::operations::SessionState::Completed(_)) => Progress::Complete,
        Ok(toha::operations::SessionState::Ended(_)) => Progress::Ended,
        Ok(toha::operations::SessionState::Asking { .. }) => Progress::Incomplete,
        _ => Progress::Unknown,
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
#[allow(clippy::too_many_arguments)]
fn stage(
    input: &[PathBuf],
    template: String,
    path: PathBuf,
    output: Option<Option<String>>,
    trust: bool,
    like: Option<Option<String>>,
    parts: toha::parts::Selection,
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
    let interactive = output.is_none();
    if interactive && !io::stdin().is_terminal() {
        return Outcome::Error(guidance::no_terminal(&template, &path));
    }
    let resolved = match cli::resolve::resolve_template(&template, &config, &registry, dirs, &cwd) {
        Ok(v) => v,
        Err(e) => return resolve_error(e),
    };
    let template = match Template::load(&resolved.folder) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let preparation = match toha::operations::Preparation::new(
        template,
        resolved,
        target.clone(),
        input,
        &Default::default(),
    ) {
        Ok(v) => v,
        Err(e) => return operation_error(e, None),
    };
    let now = match resolution(
        &preparation.resolved.formal_name,
        &preparation.template,
        &config,
    ) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let decision = if trust {
        EnvironmentDecision::CarryStageGrant
    } else {
        EnvironmentDecision::RequireStageGrant
    };
    let invocation = match preparation.context(
        decision,
        HostFacts::capture(),
        ExecutionFacts::new(is_admin(), interactive),
        &mut HostEnvironmentSource,
    ) {
        Ok(v) => v,
        Err(e) => return operation_error(e, None),
    };
    let seed = match resolve_like_seed(
        LikeFlag::parse(like),
        interactive,
        &toha::applied::source_identity(&preparation.resolved.formal_name),
        &target,
    ) {
        Ok(v) => v,
        Err(reject) => {
            return if interactive {
                Outcome::Error(reject.message)
            } else {
                like_reject_document(reject, None)
            };
        }
    };
    if interactive && let Some(seed) = &seed {
        println!("seed defaults from application {}", seed.label);
    }
    let mut session =
        match preparation.start(invocation, now, config, parts, &Default::default(), seed) {
            Ok(v) => v,
            Err(e) => return operation_error(e, None),
        };
    match session.seed_warnings() {
        Ok(warnings) => report_config_warnings(&warnings),
        Err(e) => return operation_error(e, None),
    }
    let installed = trustable(&session.resolved, &registry);
    if interactive {
        return match prompt_fresh(&mut session, Some(&store)) {
            Ok(terminal::Session::Completed(completed)) => completion_preview(
                &session.template,
                &completed,
                &target,
                &session.resolved,
                installed.as_deref(),
                &path,
                false,
            ),
            Ok(terminal::Session::Ended(ended)) => {
                eprintln!("{}", guidance::flow_ended(&ended));
                Outcome::Written(vec![])
            }
            Err(e) => Outcome::Error(e),
        };
    }
    let state = match session.state() {
        Ok(v) => v,
        Err(e) => return operation_error(e, None),
    };
    if let Err(e) = session.persist(&store) {
        return operation_error(e, None);
    }
    match state {
        toha::operations::SessionState::Asking { document, .. } => {
            let document = with_seed(document, session.record.seed.as_deref());
            let instructions = instructions(
                &AgentOutcome::Batch {
                    template: &session.record.template,
                },
                &path,
            );
            match output.flatten() {
                Some(file) => match fs::write(
                    file,
                    serde_json::to_vec_pretty(&document).expect("JSON value"),
                ) {
                    Ok(()) => Outcome::Agent {
                        document: None,
                        instructions,
                        code: 4,
                    },
                    Err(e) => Outcome::Error(e.to_string()),
                },
                None => Outcome::Agent {
                    document: Some(document),
                    instructions,
                    code: 4,
                },
            }
        }
        toha::operations::SessionState::Completed(_) => Outcome::Agent {
            document: None,
            instructions: instructions(&AgentOutcome::Complete, &path),
            code: 0,
        },
        toha::operations::SessionState::Ended(ended) => {
            eprintln!("{}", guidance::flow_ended(&ended));
            let doc = protocol::ended_document(&ended, &context(&target, &session.record));
            match output.flatten() {
                Some(file) => match fs::write(file, serde_json::to_vec_pretty(&doc).unwrap()) {
                    Ok(()) => Outcome::Saved(0),
                    Err(e) => Outcome::Error(e.to_string()),
                },
                None => Outcome::Document(doc, 0),
            }
        }
        _ => unreachable!("known session state"),
    }
}

fn seed_diagnostic(error: &toha::operations::SeedError) -> String {
    use toha::operations::SeedError;
    let message = match error {
        SeedError::MissingCopy => "staged --like has no copied application",
        SeedError::WrongSource => "copied --like application has another source",
        SeedError::WrongTarget => "copied --like application has another target",
        SeedError::Record(_) => return error.to_string(),
        _ => return format!("{error}; abort and stage again"),
    };
    format!("{message}; abort and stage again")
}
fn replay_diagnostic(error: &toha::operations::OperationError) -> String {
    match error {
        toha::operations::OperationError::Seed(error) => seed_diagnostic(error),
        other => other.to_string(),
    }
}
fn operation_error(error: toha::operations::OperationError, ctx: Option<&Context>) -> Outcome {
    match error {
        toha::operations::OperationError::Document(error) => document_input_error(error, ctx),
        toha::operations::OperationError::Parts(error) => Outcome::Usage(error),
        toha::operations::OperationError::Resolve(error) => resolve_error(error),
        toha::operations::OperationError::Seed(error) => Outcome::Error(seed_diagnostic(&error)),
        other => Outcome::Error(other.to_string()),
    }
}

/// Presentation driver: the library owns advancement, acceptance and stage persistence.
fn prompt_fresh(
    session: &mut toha::operations::Session,
    store: Option<&Store>,
) -> Result<terminal::Session, String> {
    let mut reached = 0;
    loop {
        let state = session.state().map_err(|e| e.to_string())?;
        match state {
            toha::operations::SessionState::Completed(done) => {
                for message in done.messages.iter().skip(reached) {
                    println!("{message}");
                }
                if let Some(store) = store {
                    session.persist(store).map_err(|e| e.to_string())?;
                }
                return Ok(terminal::Session::Completed(done));
            }
            toha::operations::SessionState::Ended(ended) => {
                for message in ended.messages.iter().skip(reached) {
                    println!("{message}");
                }
                if let Some(store) = store {
                    session.persist(store).map_err(|e| e.to_string())?;
                }
                return Ok(terminal::Session::Ended(ended));
            }
            toha::operations::SessionState::Asking { .. } => {}
            _ => unreachable!("known session state"),
        }
        let submission = match session.replay().map_err(|e| e.to_string())? {
            Interview::Asking(pending) => {
                reached = pending.messages_reached();
                terminal::prompt_batch(&pending, &mut terminal::InquireAsk)?
            }
            _ => unreachable!("asking session"),
        };
        let state = session.answer(submission).map_err(|e| e.to_string())?;
        if let Some(store) = store {
            session.persist(store).map_err(|e| e.to_string())?;
        }
        if let toha::operations::SessionState::Ended(ended) = state {
            for message in ended.messages.iter().skip(reached) {
                println!("{message}");
            }
            return Ok(terminal::Session::Ended(ended));
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
fn continue_run(path: PathBuf, answers: Option<String>, input: &[PathBuf], dirs: &Dirs) -> Outcome {
    use toha::operations::SessionState;
    let (target, store) = match setup(&path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let saved = match store.load(&target) {
        Ok(Some(v)) => v,
        Ok(None) => return Outcome::Error(guidance::nothing_staged(&path)),
        Err(e) => return Outcome::Error(e.to_string()),
    };
    if update_stage::is_staged_update(&saved) {
        return update_stage::continue_staged(saved, &path, answers, input, dirs);
    }
    let (config, registry, cwd) = match environment(dirs) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut session = match toha::operations::Session::resume(
        saved,
        target.clone(),
        dirs,
        config,
        &registry,
        &cwd,
    ) {
        Ok(v) => v,
        Err(e) => return operation_error(e, None),
    };
    if let Ok(warnings) = session.warnings() {
        report_config_warnings(&warnings);
    }
    let state = match session.state() {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Error(guidance::replay_failed(
                &path,
                &session.record.template,
                &replay_diagnostic(&e),
            ));
        }
    };
    if answers.is_some() {
        match state {
            SessionState::Completed(_) => {
                return Outcome::Error(guidance::complete_answers_unused(&path));
            }
            SessionState::Ended(ended) => {
                return Outcome::Error(guidance::ended_answers_unused(
                    &path,
                    &session.record.template,
                    ended.kind(),
                ));
            }
            _ => {}
        }
    }
    let incoming = match answers.as_deref().map(read_answers_text).transpose() {
        Ok(Some(text)) => match protocol::document_answers(&session.record.template, &text) {
            Ok(v) => v,
            Err(e) => {
                return Outcome::Error(
                    document_error(SubmitDocumentError::Document(e), &session.record.template)
                        .message,
                );
            }
        },
        Ok(None) => Default::default(),
        Err(e) => return Outcome::Error(e),
    };
    if let Err(e) = session.admit_documents(input, &incoming) {
        return operation_error(e, None);
    }
    let installed = trustable(&session.resolved, &registry);
    if answers.is_none() && input.is_empty() {
        match session.state() {
            Ok(SessionState::Asking { .. }) => {
                if !io::stdin().is_terminal() {
                    return Outcome::Error(guidance::continue_no_terminal(&path));
                }
                return match prompt_fresh(&mut session, Some(&store)) {
                    Ok(terminal::Session::Completed(done)) => completion_preview(
                        &session.template,
                        &done,
                        &target,
                        &session.resolved,
                        installed.as_deref(),
                        &path,
                        false,
                    ),
                    Ok(terminal::Session::Ended(ended)) => {
                        eprintln!("{}", guidance::flow_ended(&ended));
                        Outcome::Written(vec![])
                    }
                    Err(e) => Outcome::Error(e),
                };
            }
            Ok(SessionState::Completed(done)) => {
                return completion_preview(
                    &session.template,
                    &done,
                    &target,
                    &session.resolved,
                    installed.as_deref(),
                    &path,
                    true,
                );
            }
            Err(e) => return operation_error(e, None),
            _ => {}
        }
    }
    let state = match if answers.is_some() && input.is_empty() {
        session.answer(incoming)
    } else {
        session.state()
    } {
        Ok(v) => v,
        Err(e) => return operation_error(e, None),
    };
    let rejected =
        matches!(&state, SessionState::Asking { rejections, .. } if !rejections.is_empty());
    if !rejected && let Err(e) = session.persist(&store) {
        return operation_error(e, None);
    }
    match state {
        SessionState::Asking {
            mut document,
            rejections,
        } => {
            if !rejections.is_empty() {
                let explained =
                    guidance::explain_rejections(&rejections, &path, &session.record.template);
                if let Ok(Interview::Asking(pending)) = session.replay() {
                    document = protocol::batch_document(
                        pending.batch(),
                        &context(&target, &session.record),
                        Some(&explained),
                    );
                }
            }
            Outcome::Agent {
                document: Some(if input.is_empty() {
                    with_seed(document, session.record.seed.as_deref())
                } else {
                    document
                }),
                instructions: instructions(
                    &AgentOutcome::Batch {
                        template: &session.record.template,
                    },
                    &path,
                ),
                code: 4,
            }
        }
        SessionState::Completed(_) => Outcome::Agent {
            document: None,
            instructions: instructions(&AgentOutcome::Complete, &path),
            code: 0,
        },
        SessionState::Ended(ended) => {
            eprintln!("{}", guidance::flow_ended(&ended));
            Outcome::Document(
                protocol::ended_document(&ended, &context(&target, &session.record)),
                0,
            )
        }
        _ => unreachable!("known session state"),
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
/// The result document for a `--like` rejection on a document route: a usage
/// error (exit 2) for a bare selector where one is required, or a record error
/// (exit 1) for an unknown, ambiguous, wrong-source, or absent selector.
fn like_reject_document(reject: LikeReject, context: Option<&Context>) -> Outcome {
    let (kind, code) = if reject.usage {
        (ErrorKind::Input, 2)
    } else {
        (ErrorKind::Record, 1)
    };
    Outcome::Document(
        protocol::error_document(
            &ResultError {
                kind,
                message: reject.message,
                commands: vec![],
            },
            context,
        ),
        code,
    )
}
fn with_application(
    mut document: serde_json::Value,
    metadata: &serde_json::Value,
) -> serde_json::Value {
    document["application"] = metadata.clone();
    document
}
/// Add the optional `seed: { from }` member to a result document when `--like`
/// seeded the interview; leaves the document unchanged otherwise.
fn with_seed(mut document: serde_json::Value, seed_from: Option<&str>) -> serde_json::Value {
    if let (Some(from), Some(object)) = (seed_from, document.as_object_mut()) {
        object.insert("seed".to_owned(), serde_json::json!({ "from": from }));
    }
    document
}
/// The scripted `error` document of kind `requirement` for a template whose
/// `requires` programs are not on `PATH`.
fn requirement_error(error: &toha::MissingRequirements) -> Outcome {
    scripted_error(ErrorKind::Requirement, error.to_string(), vec![], None)
}
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
use toha::operations::classify_paths;
/// Maps an apply failure to a scripted `error` document by kind.
fn scripted_apply_error(error: toha::ApplyError, context: &Context) -> Outcome {
    use toha::ApplyError as E;
    let kind = match &error {
        E::Conflicts(_) | E::Drift(_) | E::Symlink(_) => ErrorKind::Conflict,
        E::Hook { .. } | E::HookIo { .. } | E::HookOutput { .. } => ErrorKind::Hook,
        E::Io { .. } | E::Deferred(_) | E::Region(_) | E::Json(_) => ErrorKind::Render,
        E::Git(_) | E::Record(_) => ErrorKind::Git,
    };
    scripted_error(kind, error.to_string(), vec![], Some(context))
}
/// The scripted route: `apply TEMPLATE PATH --answers FILE`. One shot. It never
/// stages, refuses when an interview is staged (before reading the document),
/// and writes exactly one JSON result document for every outcome.
#[allow(clippy::too_many_arguments)]
fn scripted(
    input: &[PathBuf],
    template_arg: String,
    path: &Path,
    answers_file: Option<String>,
    force: bool,
    dry_run: bool,
    trust: bool,
    like: Option<Option<String>>,
    parts: toha::parts::Selection,
    dirs: &Dirs,
) -> Outcome {
    let answers_file = answers_file.as_deref().unwrap_or("");
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
        Err(e) => return scripted_resolve_error(e, path, answers_file, force, dry_run, trust),
    };
    // 3. Resolve the command template, load it, produce the configured
    //    Resolution, and start the interview.
    let resolved =
        match cli::resolve::resolve_template(&template_arg, &config, &registry, dirs, &cwd) {
            Ok(v) => v,
            Err(e) => return scripted_resolve_error(e, path, answers_file, force, dry_run, trust),
        };
    let template = match Template::load(&resolved.folder) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Source, e.to_string(), vec![], None),
    };
    if let Err(e) = toha::requirement::check(&template) {
        return requirement_error(&e);
    }
    let now = match resolution(&resolved.formal_name, &template, &config) {
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
    // The scripted route is non-interactive: a bare `--like` is a usage error.
    let like_seed = match resolve_like_seed(
        LikeFlag::parse(like),
        false,
        &toha::applied::source_identity(&resolved.formal_name),
        &target,
    ) {
        Ok(v) => v,
        Err(reject) => return like_reject_document(reject, Some(&ctx)),
    };
    let seed_from = like_seed.as_ref().map(|seed| seed.id.to_string());
    // 4. Read the document once as UTF-8.
    let text = match if answers_file.is_empty() {
        Ok(serde_json::json!({"template": resolved.formal_name, "answers": {}}).to_string())
    } else {
        read_answers_text(answers_file)
    } {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Input, e, vec![], Some(&ctx)),
    };
    let supplied = match protocol::document_answers(&resolved.formal_name, &text) {
        Ok(v) => v,
        Err(e) => {
            let error = document_error(SubmitDocumentError::Document(e), &resolved.formal_name);
            return Outcome::Document(protocol::error_document(&error, Some(&ctx)), 1);
        }
    };
    if let Err(error) = parts.admit(&template, &supplied) {
        return Outcome::Usage(error);
    }
    let mut session = match toha::operations::Session::start(
        template,
        resolved,
        target.clone(),
        new_context,
        now,
        config,
        parts,
        input,
        &supplied,
        like_seed,
    ) {
        Ok(v) => v,
        Err(e) => return operation_error(e, Some(&ctx)),
    };
    match session.seed_warnings() {
        Ok(warnings) => report_config_warnings(&warnings),
        Err(e) => return scripted_error(ErrorKind::Render, e.to_string(), vec![], Some(&ctx)),
    }
    let state = match session.answer_all(supplied) {
        Ok(v) => v,
        Err(e) => return scripted_error(ErrorKind::Render, e.to_string(), vec![], Some(&ctx)),
    };
    match state {
        toha::operations::SessionState::Asking { document, .. } => {
            Outcome::Document(with_seed(document, seed_from.as_deref()), 4)
        }
        toha::operations::SessionState::Ended(ended) => {
            Outcome::Document(protocol::ended_document(&ended, &ctx), 0)
        }
        toha::operations::SessionState::Completed(completed) => scripted_completed(
            &session,
            completed,
            &ctx,
            force,
            dry_run,
            trust,
            seed_from.as_deref(),
        ),
        _ => unreachable!("known session state"),
    }
}
/// The scripted `Completed` tail: build the plan, then report `planned` (dry run
/// or untrusted hooks) or apply and report `applied`, writing the applied record for
/// the apply when the project allows it.
#[allow(clippy::too_many_arguments)]
fn scripted_completed(
    session: &toha::operations::Session,
    completed: Completed,
    ctx: &Context,
    force: bool,
    dry_run: bool,
    trust: bool,
    seed_from: Option<&str>,
) -> Outcome {
    let template = &session.template;
    let resolved = &session.resolved;
    let target = &session.target;
    let dry_run = toha::updates::effective_dry_run(dry_run, &completed);
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
    let plan = match session.plan(&completed) {
        Ok(plan) => plan,
        Err(toha::operations::OperationError::Record(error)) => {
            return Outcome::Error(error.to_string());
        }
        Err(error) => {
            return scripted_error(ErrorKind::Render, error.to_string(), vec![], Some(ctx));
        }
    };
    let metadata = parts_output::metadata(
        None,
        &plan.record.as_ref().expect("plan record").revision,
        &[],
        &template.selected_parts(&completed),
    );
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
            with_application(
                protocol::planned_document(&classified, &plan, &messages, ctx, trust_state),
                &metadata,
            ),
            code,
        );
    }
    if has_hooks && !trusted {
        return Outcome::Document(
            with_application(
                protocol::planned_document(
                    &classified,
                    &plan,
                    &messages,
                    ctx,
                    TrustState::Untrusted,
                ),
                &metadata,
            ),
            3,
        );
    }
    let actions: std::collections::HashMap<String, String> = classified
        .iter()
        .map(|entry| (entry.path.clone(), entry.action.clone()))
        .collect();
    match session.apply_plan(
        plan,
        ApplyOptions {
            force,
            trusted: true,
        },
        // The scripted route keeps standard output a single JSON document: an
        // uncaptured hook's stdout is forwarded to standard error.
        &ScriptedRunner,
        None,
        &mut |_| {},
    ) {
        Ok(Applied::Written {
            files,
            hooks,
            after_apply,
            git,
            record,
            ..
        }) => {
            if let Some(warning) = git.as_ref().and_then(|g| g.warning.clone()) {
                messages.push(warning);
            }
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
                git: git.as_ref().map(|g| g.setup.clone()),
            };
            let mut document = with_application(
                with_seed(protocol::applied_document(&report, ctx), seed_from),
                &metadata,
            );
            document["record"] = record_document(&record);
            Outcome::Document(document, 0)
        }
        // Trust was granted above, so an untrusted plan cannot occur here.
        Ok(Applied::NeedsTrust(_)) => scripted_error(
            ErrorKind::Hook,
            "hooks are not trusted".into(),
            vec![],
            Some(ctx),
        ),
        Err(toha::operations::OperationError::Apply(error)) => scripted_apply_error(*error, ctx),
        Err(error) => operation_error(error, Some(ctx)),
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
        error @ (ResolveError::Error(_)
        | ResolveError::NotFound(_)
        | ResolveError::StaleRevision { .. }) => {
            scripted_error(ErrorKind::Source, error.to_string(), vec![], None)
        }
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
        other => scripted_error(ErrorKind::Source, other.to_string(), vec![], None),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    input: &[PathBuf],
    template: Option<String>,
    path: &Path,
    answers: Option<String>,
    force: bool,
    dry_run: bool,
    trust: bool,
    like: Option<Option<String>>,
    parts: toha::parts::Selection,
    dirs: &Dirs,
) -> Outcome {
    // Scripted route: apply TEMPLATE PATH --answers FILE. One shot, one result
    // document, never staged.
    if let Some(template) = template.clone()
        && (answers.is_some() || (!input.is_empty() && !io::stdin().is_terminal()))
    {
        return scripted(
            input,
            template,
            path,
            answers.clone(),
            force,
            dry_run,
            trust,
            like,
            parts,
            dirs,
        );
    }
    // `--like` seeds a fresh application; resume re-reads any pinned seed.
    let like = LikeFlag::parse(like);
    let (target, store) = match setup(path, dirs) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e),
    };
    let existing = match store.load(&target) {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    if parts.is_present() && (existing.is_some() || template.is_none()) {
        return Outcome::Usage("part flags belong to a new interview; use stage TEMPLATE PATH --update to change parts".into());
    }
    // `apply PATH` on a staged update resumes it through the update replay
    // adapter and merges from the base.
    if answers.is_none()
        && let Some(saved) = &existing
        && update_stage::is_staged_update(saved)
    {
        return update_stage::apply_staged(
            saved.clone(),
            path,
            toha::updates::UpdateOptions { force, dry_run },
            trust,
            input,
            dirs,
        );
    }
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
    if let (Some(arg), Some(saved)) = (&template, &existing)
        && let Some(refusal) = staged_refusal(&invocation, arg, path, saved, &scope, &target)
    {
        return refusal;
    }
    // A template named for its own staged interview resumes that interview.
    let template = template.filter(|_| existing.is_none());
    // `--like` seeds a new application; it has nothing to attach to on a resume
    // (`apply PATH`, or `apply TEMPLATE PATH` naming a staged interview), whose
    // seed is already pinned in the staged record and re-read below.
    if like.is_present() && template.is_none() {
        return Outcome::Error(
            "--like applies to a new application (apply TEMPLATE PATH); a resumed interview \
             already pinned its seed when it was staged"
                .to_owned(),
        );
    }
    let mut terminal_run = false;
    // The stored approval digest of the template resolved by name, compared to
    // the live executable surface once the template is loaded.
    // The installed template whose registry trust would run the hooks.
    let mut session = match (template, existing) {
        (Some(folder), None) => {
            let resolved =
                match cli::resolve::resolve_template(&folder, &config, &registry, dirs, &cwd) {
                    Ok(v) => v,
                    Err(e) => return resolve_error(e),
                };
            let template = match Template::load(&resolved.folder) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.to_string()),
            };
            if let Err(e) = toha::requirement::check(&template) {
                return Outcome::Error(e.to_string());
            }
            if let Err(e) = parts.admit(&template, &Default::default()) {
                return Outcome::Usage(e);
            }
            if !io::stdin().is_terminal() {
                return Outcome::Error(guidance::no_terminal(&folder, path));
            }
            let preparation = match toha::operations::Preparation::new(
                template,
                resolved,
                target.clone(),
                input,
                &Default::default(),
            ) {
                Ok(v) => v,
                Err(e) => return operation_error(e, None),
            };
            let now = match resolution(
                &preparation.resolved.formal_name,
                &preparation.template,
                &config,
            ) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e),
            };
            let decision = EnvironmentDecision::grant_if_needed(apply_environment_grant(
                trust,
                &preparation.resolved,
                &preparation.template,
            ));
            let context = match preparation.context(
                decision,
                HostFacts::capture(),
                ExecutionFacts::new(is_admin(), true),
                &mut HostEnvironmentSource,
            ) {
                Ok(v) => v,
                Err(e) => return operation_error(e, None),
            };
            let seed = match resolve_like_seed(
                like,
                true,
                &toha::applied::source_identity(&preparation.resolved.formal_name),
                &target,
            ) {
                Ok(v) => v,
                Err(e) => return Outcome::Error(e.message),
            };
            if let Some(seed) = &seed {
                println!("seed defaults from application {}", seed.label);
            }
            let session =
                match preparation.start(context, now, config, parts, &Default::default(), seed) {
                    Ok(v) => v,
                    Err(e) => return operation_error(e, None),
                };
            match session.seed_warnings() {
                Ok(warnings) => report_config_warnings(&warnings),
                Err(e) => return operation_error(e, None),
            }
            session
        }
        (None, Some(saved)) => {
            let mut session = match toha::operations::Session::resume(
                saved,
                target.clone(),
                dirs,
                config,
                &registry,
                &cwd,
            ) {
                Ok(v) => v,
                Err(e) => return operation_error(e, None),
            };
            if let Ok(warnings) = session.warnings() {
                report_config_warnings(&warnings);
            }
            if let Err(e) = session.admit_documents(input, &Default::default()) {
                return operation_error(e, None);
            }
            session
        }
        (None, None) => return Outcome::Error(guidance::nothing_staged(path)),
        _ => unreachable!("a named template resumes its staged interview"),
    };
    let approval = session.resolved.approval.clone();
    let installed = trustable(&session.resolved, &registry);
    let completed = match session.state() {
        Ok(toha::operations::SessionState::Completed(done)) => done,
        Ok(toha::operations::SessionState::Ended(ended)) => {
            if let Err(e) = session.persist(&store) {
                return operation_error(e, None);
            }
            eprintln!("{}", guidance::flow_ended(&ended));
            return Outcome::Written(vec![]);
        }
        Ok(toha::operations::SessionState::Asking { document, .. }) if !named => {
            return Outcome::Agent {
                document: Some(with_seed(document, session.record.seed.as_deref())),
                instructions: instructions(
                    &AgentOutcome::ApplyIncomplete {
                        template: &session.record.template,
                    },
                    path,
                ),
                code: 4,
            };
        }
        Ok(toha::operations::SessionState::Asking { .. }) => {
            if !io::stdin().is_terminal() {
                return Outcome::Error(guidance::continue_no_terminal(path));
            }
            match prompt_fresh(&mut session, if dry_run { None } else { Some(&store) }) {
                Ok(terminal::Session::Completed(done)) => {
                    terminal_run = true;
                    done
                }
                Ok(terminal::Session::Ended(ended)) => {
                    eprintln!("{}", guidance::flow_ended(&ended));
                    return Outcome::Written(vec![]);
                }
                Err(e) => return Outcome::Error(e),
            }
        }
        Err(e) => {
            return Outcome::Error(guidance::replay_failed(
                path,
                &session.record.template,
                &replay_diagnostic(&e),
            ));
        }
        _ => unreachable!("known session state"),
    };
    let template = &session.template;
    // A flow `dry-run` composes with the CLI `--dry-run` by union: either
    // suppresses the apply.
    let dry_run = toha::updates::effective_dry_run(dry_run, &completed);
    // Trust is the live executable surface matching the stored approval; the
    // one flag `--trust` overrides for a single run. This is the only place a
    // template becomes trusted from stored state.
    let live_surface = match toha::HookSurface::of(template) {
        Ok(surface) => surface.digest(),
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let registry_trusted = matches!(
        toha::evaluate_trust(approval.as_ref(), &live_surface),
        toha::Trust::Trusted
    );
    // An approval that no longer matches marks the hooks changed since approval.
    let changed_since_approval = approval.is_some() && !registry_trusted;
    let plan = session.plan(&completed);
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => return Outcome::Error(error.to_string()),
    };
    let metadata = parts_output::metadata(
        None,
        &plan.record.as_ref().expect("plan record").revision,
        &[],
        &template.selected_parts(&completed),
    );
    if dry_run {
        let edits = match injection_report(&plan, &target) {
            Ok(edits) => edits,
            Err(error) => return Outcome::Error(error.to_string()),
        };
        let lines = if terminal_run {
            vec![]
        } else {
            completed.messages.clone()
        }
        .into_iter()
        .chain(parts_output::lines(&metadata))
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
        for message in &completed.messages {
            println!("{message}");
        }
    }
    if let Some(message) = &before {
        println!("{message}");
    }
    // Each file line is printed as the file is written, so that the lines
    // precede the output of every hook.
    match session.apply_plan(
        plan,
        ApplyOptions {
            force,
            trusted: registry_trusted || trust,
        },
        &ProcessRunner,
        Some(&store),
        &mut |file| println!("{file}"),
    ) {
        Ok(Applied::Written {
            after_apply,
            git,
            record,
            ..
        }) => {
            let mut lines = parts_output::lines(&metadata);
            lines.extend(git.as_ref().map(GitReport::lines).unwrap_or_default());
            lines.extend(after_apply);
            if let toha::applied::RecordOutcome::Written(path) = record {
                lines.push(format!("saved applied record {path}"));
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
        Err(toha::operations::OperationError::Apply(error))
            if matches!(
                *error,
                toha::ApplyError::Conflicts(_) | toha::ApplyError::Drift(_)
            ) =>
        {
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
    let record_target = match &command {
        Command::Stage { path, .. } | Command::Continue { path, .. } => Some(path.clone()),
        Command::Apply { paths, .. } => paths.last().map(PathBuf::from),
        _ => None,
    };
    if let Some(path) = record_target {
        let checked = staging::canonical_target(&path)
            .map_err(|e| e.to_string())
            .and_then(|target| {
                match toha::applied::Project::open(&target).map_err(|e| e.to_string())? {
                    Some(project) => project.record_relative().map_err(|e| e.to_string()),
                    None => toha::applied::validate_record_path(target.as_path())
                        .map_err(|e| e.to_string()),
                }
            });
        if let Err(error) = checked {
            return Outcome::Usage(error).finish();
        }
    }
    let dirs = match Dirs::resolve() {
        Ok(v) => v,
        Err(e) => return Outcome::Error(e).finish(),
    };
    match command {
        Command::Skills { .. } => unreachable!("the home-free skills command returned above"),
        Command::Stage {
            input,
            with,
            without,
            template,
            path,
            r#async,
            trust,
            update,
            baseline,
            reanswer,
            like,
        } => {
            let parts = toha::parts::Selection {
                with,
                without,
                base: None,
            };
            // A staged update (`stage --update`/`--baseline`) drives the update
            // replay adapter; a plain stage starts a fresh interview.
            if update || baseline {
                let base = if update {
                    update::UpdateBase::Update
                } else {
                    update::UpdateBase::Baseline
                };
                return update_stage::stage(
                    base,
                    Some(template),
                    &path,
                    r#async,
                    reanswer,
                    trust,
                    &input,
                    parts,
                    &dirs,
                )
                .finish();
            }
            stage(
                &input,
                template,
                path.clone(),
                r#async.clone(),
                trust,
                like,
                parts,
                &dirs,
            )
            .retry(|formal| {
                Invocation::Stage {
                    template: Arg::Given(guidance::formal_for("stage", formal)),
                    path: &path,
                    output: r#async.as_ref().map(|file| file.as_deref()),
                }
                .command()
            })
            .finish()
        }
        Command::Continue {
            path,
            answers,
            input,
        } => continue_run(path, answers, &input, &dirs).finish(),
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
            input,
            with,
            without,
            paths,
            answers,
            force,
            dry_run,
            trust,
            update,
            baseline,
            reanswer,
            like,
        } => {
            let parts = toha::parts::Selection {
                with,
                without,
                base: None,
            };
            let (template, path) = match paths.as_slice() {
                [path] => (None, PathBuf::from(path)),
                [template, path] => (Some(template.clone()), PathBuf::from(path)),
                _ => unreachable!("clap requires one or two operands"),
            };
            if update || baseline {
                let base = if update {
                    update::UpdateBase::Update
                } else {
                    update::UpdateBase::Baseline
                };
                return update::run_update(
                    base, template, &path, answers, reanswer, force, dry_run, trust, &input, parts,
                    &dirs,
                )
                .finish();
            }
            run(
                &input,
                template,
                &path,
                answers.clone(),
                force,
                dry_run,
                trust,
                like,
                parts,
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
    use super::{Dirs, Platform, RuntimeDirs};

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
            .expect_err("no home directory");
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

#[cfg(test)]
mod report_tests {
    use super::Outcome;
    use std::io::Write;
    use std::path::Path;
    use std::process::Command;

    /// A writer that fails every write, standing in for a broken pipe at the
    /// reporting output boundary.
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            ))
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@x.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@x.invalid")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn a_report_write_failure_surfaces_an_error_and_never_panics() {
        // The reporting boundary renders the result document; a broken pipe (or
        // any output error) there must surface as an error, never a panic and
        // never a silent swallow. This is the one real kill on this seam
        // (mutation M26: reintroducing the panicking `println!` makes render stop
        // honouring the injected writer, so the result is no longer an error).
        // That the committed repository cannot be touched is proved structurally
        // by render's signature — render(self, out, err) takes an `Outcome` of
        // owned strings/JSON and holds no repository handle — and, against a real
        // committed update, by
        // `a_real_committed_update_survives_a_report_write_failure`.
        let outcome = Outcome::Document(
            serde_json::json!({
                "protocol": 1,
                "status": "applied",
                "record": { "path": ".toha.json" },
                "merge": { "changes": [], "conflicted": [] }
            }),
            0,
        );
        let mut err = Vec::new();
        let result = outcome.render(&mut FailingWriter, &mut err);
        assert!(
            result.is_err(),
            "a broken pipe at the report boundary surfaces as an error, not a panic"
        );
    }

    #[test]
    fn a_real_committed_update_survives_a_report_write_failure() {
        // The actual-state proof for finding 3: drive a genuine committed update
        // through the same library merge the CLI uses, then render its real
        // result document through the same reporting boundary with a failing
        // writer. The exact worktree bytes, the raw on-disk index, and the FULL
        // snapshot ref-map — captured from the real update before the report —
        // must be byte-identical after the report fails.
        use indexmap::IndexMap;
        use toha::applied::{Base, MergeOptions, Merged, Project, RecordInputs, Revision};
        use toha::interview::{Interview, Seed};
        use toha::template::{Id, Template};
        use toha::{RawAnswer, RawAnswers};

        // A template with one required question and one rendered file.
        let tdir = tempfile::tempdir().unwrap();
        std::fs::create_dir(tdir.path().join("template")).unwrap();
        std::fs::write(
            tdir.path().join("template.yml"),
            "name: greeter\ndescription: d\ninterview:\n  - id: name\n    type: text\n    prompt: Name\n    required: true\n",
        )
        .unwrap();
        std::fs::write(
            tdir.path().join("template/greeting.txt"),
            "Hello {{ name }}\n",
        )
        .unwrap();
        let template = Template::load(tdir.path()).unwrap();

        // A committed git target.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("seed.txt"), b"seed\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "seed"]);

        // Complete the interview with the one answer.
        let target = toha::staging::canonical_target(root).unwrap();
        let seed = Seed {
            now: "2026-03-14T09:26:53+00:00[UTC]".parse().unwrap(),
            defaults: IndexMap::new(),
            context: toha::context::InvocationContext::for_target(target.clone()),
        };
        let mut raw = RawAnswers::new();
        raw.insert(
            Id::parse("name").unwrap(),
            RawAnswer(serde_json::json!("sample-value")),
        );
        let completed = match Interview::start(&template, seed).unwrap() {
            Interview::Asking(pending) => match pending.answer(raw).unwrap() {
                Interview::Complete(completed) => completed,
                other => panic!("the one required question was answered: {other:?}"),
            },
            other => panic!("the template asks its required question: {other:?}"),
        };

        // A real committed baseline update: merge_apply writes the worktree, the
        // index, and the snapshot ref, exactly as the CLI update route does.
        let mut submission = IndexMap::new();
        submission.insert(
            Id::parse("name").unwrap(),
            serde_json::json!("sample-value"),
        );
        let project = Project::open(&target).unwrap().unwrap();
        let merged = toha::applied::merge_apply(
            &project,
            Base::Empty,
            &template,
            &completed,
            RecordInputs {
                template: "forge:demo/greeter@stable".to_owned(),
                revision: Revision::Unversioned,
            },
            MergeOptions {
                force: false,
                trusted: true,
                dry_run: false,
            },
            &toha::hook::ProcessRunner,
        )
        .unwrap();
        let Merged::Written {
            record, changes, ..
        } = merged
        else {
            panic!("a baseline merge writes: {merged:?}");
        };
        assert!(
            !changes.is_empty(),
            "the baseline merge added the rendered file"
        );

        // The real result document, built as the update route builds it.
        let outcome = Outcome::Document(
            serde_json::json!({
                "protocol": 1,
                "status": "applied",
                "record": { "path": record },
                "merge": { "changes": [], "conflicted": [] }
            }),
            0,
        );

        // The exact committed state, captured from the real update before report.
        let refmap = |root: &std::path::Path| {
            git(
                root,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/toha/snapshots/",
                ],
            )
        };
        let file_before = std::fs::read(root.join("greeting.txt")).unwrap();
        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        let refmap_before = refmap(root);
        assert!(refmap_before.is_empty(), "an update creates no Toha ref");
        let record_before = std::fs::read(root.join(".toha.json")).unwrap();

        // Reporting to a failing writer surfaces the error (no panic, no swallow).
        let mut err = Vec::new();
        let result = outcome.render(&mut FailingWriter, &mut err);
        assert!(
            result.is_err(),
            "a broken pipe at the report boundary surfaces as an error"
        );

        assert_eq!(
            std::fs::read(root.join(".toha.json")).unwrap(),
            record_before
        );
        // The real committed result is byte-identical after the failed report.
        assert_eq!(
            std::fs::read(root.join("greeting.txt")).unwrap(),
            file_before,
            "the committed worktree is preserved"
        );
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before,
            "the raw on-disk index is preserved"
        );
        assert_eq!(
            refmap(root),
            refmap_before,
            "the full snapshot ref-map is preserved"
        );
    }

    #[test]
    fn a_successful_report_renders_the_document_and_exit_code() {
        // The refactor preserves the success path: the document is written to the
        // out writer and the exit code is the document's.
        let outcome = Outcome::Document(serde_json::json!({ "status": "applied" }), 0);
        let mut out = Vec::new();
        let mut err = Vec::new();
        // ExitCode is not comparable; a successful render returning Ok is the check.
        outcome.render(&mut out, &mut err).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("\"status\": \"applied\""),
            "document written: {text}"
        );
        assert!(err.is_empty(), "nothing on stderr for a success");
    }
}

fn record_document(record: &toha::applied::RecordOutcome) -> serde_json::Value {
    match record {
        toha::applied::RecordOutcome::Written(path) => serde_json::json!({"path":path}),
        toha::applied::RecordOutcome::NotGit => serde_json::json!({"skipped":"not-git"}),
        toha::applied::RecordOutcome::Internal => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod seed_diagnostic_tests {
    #[test]
    fn seed_refusals_keep_cli_guidance() {
        use toha::operations::{OperationError, SeedError};
        for (error, expected) in [
            (
                SeedError::MissingCopy,
                "staged --like has no copied application; abort and stage again",
            ),
            (
                SeedError::WrongSource,
                "copied --like application has another source; abort and stage again",
            ),
            (
                SeedError::WrongTarget,
                "copied --like application has another target; abort and stage again",
            ),
        ] {
            assert_eq!(
                super::replay_diagnostic(&OperationError::Seed(error)),
                expected,
                "cli_seed_replay_text"
            );
        }
        match super::operation_error(OperationError::Seed(SeedError::MissingCopy), None) {
            super::Outcome::Error(message) => assert_eq!(
                message, "staged --like has no copied application; abort and stage again",
                "cli_seed_operation_text"
            ),
            _ => panic!("seed error changed category"),
        }
    }
}
