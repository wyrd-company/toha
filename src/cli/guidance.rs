// ---
// relationships:
//   implements: command-line-interface
// ---
//! Messages for requests made at the wrong time for the staged state of a
//! target. Each names the commands that do what the caller meant.
use std::path::Path;

/// Whether a staged interview has questions remaining, when that is known.
#[derive(Clone, Copy)]
pub enum Progress {
    Incomplete,
    Complete,
    Unknown,
}

/// The shell a suggested command is written for.
#[derive(Clone, Copy)]
enum Shell {
    /// A POSIX shell such as sh, bash, or zsh.
    Posix,
    /// cmd.exe or PowerShell.
    Windows,
}
const SHELL: Shell = if cfg!(windows) {
    Shell::Windows
} else {
    Shell::Posix
};

/// A formal name as a command operand. A local folder's formal name on
/// Windows is an extended path (`\\?\D:\...`), which a `<TEMPLATE>` argument
/// reads as a name; its drive path names the same folder.
pub fn plain_formal(formal: &str) -> &str {
    match formal.strip_prefix(r"\\?\") {
        Some(rest)
            if rest.as_bytes().get(0..3).is_some_and(|prefix| {
                prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && prefix[2] == b'\\'
            }) =>
        {
            rest
        }
        _ => formal,
    }
}

/// The form of a formal name that `command` accepts. `stage`, `apply`, and
/// `templates add` read a `<TEMPLATE>` or address argument, so a folder is
/// named by its drive path; `templates remove`, `update`, and `alias` look up
/// the formal name as the registry stores it.
pub fn formal_for<'a>(command: &str, formal: &'a str) -> &'a str {
    match command {
        "templates remove" | "templates update" | "templates alias" => formal,
        _ => plain_formal(formal),
    }
}

/// A path value without trailing separators, so that no suggested path ends
/// in a separator before a closing quote. A root (`/`, `\`, `C:\`, `C:/`)
/// is kept whole.
fn path_value(value: &str, shell: Shell) -> &str {
    let separators: &[char] = match shell {
        Shell::Posix => &['/'],
        Shell::Windows => &['/', '\\'],
    };
    let trimmed = value.trim_end_matches(separators);
    let drive = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    let drive = matches!(shell, Shell::Windows)
        && drive.len() == 2
        && drive.as_bytes()[0].is_ascii_alphabetic()
        && drive.as_bytes()[1] == b':';
    let root = trimmed.is_empty() || drive;
    if root && trimmed.len() < value.len() {
        // Keep one separator: `/`, `\`, `C:\`, `\\?\C:\`.
        &value[..trimmed.len() + 1]
    } else {
        trimmed
    }
}

/// `value` as one shell word for the shell of this platform.
pub fn word(value: &str) -> String {
    word_for(value, SHELL)
}

fn word_for(value: &str, shell: Shell) -> String {
    match shell {
        Shell::Posix => posix_word(value),
        Shell::Windows => windows_word(value),
    }
}

/// A POSIX shell leaves these characters unchanged anywhere in a word, and
/// `=` after the first character. Any other word is single-quoted, which a
/// POSIX shell passes through unchanged.
fn posix_word(value: &str) -> String {
    let plain = !value.is_empty()
        && !value.starts_with('=')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Paths keep `\`, `:`, and `~` plain, as cmd.exe and PowerShell do; `~` and
/// `=` only after the first character. Any other word is double-quoted, the
/// form both shells accept, with an embedded `"` written as `""`.
///
/// A word's trailing backslashes are written after the closing quote
/// (`"C:\my dir"\`, `"\\?\C:"\`); the command-line rules of the Universal
/// CRT (as `CommandLineToArgvW`) and PowerShell argument mode both read that
/// as one argument that ends in those backslashes.
///
/// What holds: cmd.exe and PowerShell both pass a double-quoted word with
/// spaces, `&`, `|`, `<`, `>`, `(`, `)`, `;`, `,` and `#` as one argument, and
/// toha reads `""` inside it as `"` through the Universal CRT command-line
/// rules; PowerShell reads `""` as `"` too. A backslash is literal unless it
/// precedes a `"`; trailing backslashes follow the closing quote and Windows
/// paths cannot contain `"`, so no backslash precedes a quote in a suggested
/// path.
///
/// Limits: `%` stays plain, because cmd.exe expands `%NAME%` inside double
/// quotes as well as outside. PowerShell expands `$` and backtick escapes
/// inside double quotes. A word with those characters is quoted but not
/// protected from expansion in every shell.
fn windows_word(value: &str) -> String {
    let plain = !value.is_empty()
        && !value.starts_with(['=', '~'])
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-\\~".contains(c));
    if plain {
        return value.into();
    }
    // A backslash before the closing quote would escape it, so the trailing
    // backslashes follow the closing quote.
    let body = value.trim_end_matches('\\');
    let trailing = &value[body.len()..];
    format!("\"{}\"{trailing}", body.replace('"', "\"\""))
}

fn target(path: &Path) -> String {
    word(path_value(&path.to_string_lossy(), SHELL))
}

/// One operand of a suggested command: its value and how it is written.
pub struct Operand {
    value: String,
    written: String,
}
pub fn value(value: &str) -> Operand {
    Operand {
        value: value.into(),
        written: word(value),
    }
}
fn path_operand(path: &Path) -> Operand {
    file_operand(&path.to_string_lossy())
}
/// A file or folder operand, without trailing separators.
fn file_operand(path: &str) -> Operand {
    value(path_value(path, SHELL))
}
/// A placeholder such as `<TEMPLATE>`, written as is.
pub fn placeholder(name: &str) -> Operand {
    Operand {
        value: name.into(),
        written: name.into(),
    }
}
/// Whether operands must follow `--` to be read as operands.
fn needs_separator(operands: &[Operand]) -> bool {
    operands
        .iter()
        .any(|operand| operand.value.starts_with('-'))
}
/// `toha <command> <options> [--] <operands>`.
pub fn toha(command: &str, options: &[String], operands: &[Operand]) -> String {
    let mut words = vec!["toha".to_string(), command.to_string()];
    words.extend(options.iter().cloned());
    if needs_separator(operands) {
        words.push("--".into());
    }
    words.extend(operands.iter().map(|operand| operand.written.clone()));
    words.join(" ")
}
/// A value in a suggested command: one the caller gave, or a placeholder
/// such as `<TEMPLATE>` for one the caller chooses.
#[derive(Clone, Copy)]
pub enum Arg<'a> {
    Given(&'a str),
    Placeholder(&'static str),
}
impl Arg<'_> {
    fn operand(self) -> Operand {
        match self {
            Self::Given(given) => value(given),
            Self::Placeholder(name) => placeholder(name),
        }
    }
}
fn option_value(flag: &str, value: Arg) -> String {
    match value {
        Arg::Placeholder(name) => format!("{flag} {name}"),
        Arg::Given(value) if value.starts_with('-') && value != "-" => {
            format!("{flag}={}", word(path_value(value, SHELL)))
        }
        Arg::Given(value) => format!("{flag} {}", word(path_value(value, SHELL))),
    }
}

/// The parsed `stage` or `apply` request, from which suggested commands are
/// built.
#[derive(Clone, Copy)]
pub enum Invocation<'a> {
    Stage {
        template: Arg<'a>,
        path: &'a Path,
        output: Option<Option<&'a str>>,
    },
    Apply {
        template: Option<Arg<'a>>,
        path: &'a Path,
        answers: Option<Arg<'a>>,
        force: bool,
        dry_run: bool,
        trust: bool,
    },
}
impl<'a> Invocation<'a> {
    fn verb(&self) -> &'static str {
        match self {
            Self::Stage { .. } => "stage",
            Self::Apply { .. } => "apply",
        }
    }
    fn template(&self) -> Option<&str> {
        match self {
            Self::Stage {
                template: Arg::Given(template),
                ..
            }
            | Self::Apply {
                template: Some(Arg::Given(template)),
                ..
            } => Some(template),
            _ => None,
        }
    }
    fn without_dry_run(mut self) -> Self {
        if let Self::Apply { dry_run, .. } = &mut self {
            *dry_run = false;
        }
        self
    }
    /// The request in canonical form: options first, in long form.
    pub fn command(&self) -> String {
        match *self {
            Self::Stage {
                template,
                path,
                output,
            } => {
                let operands = [template.operand(), path_operand(path)];
                let option = output.map(|file| match file {
                    Some(file) => format!("--async={}", word(file)),
                    None => "--async".to_string(),
                });
                match option {
                    // `--async` takes an optional value, so it follows the
                    // operands unless they need `--`.
                    Some(option) if !needs_separator(&operands) => {
                        format!("{} {option}", toha("stage", &[], &operands))
                    }
                    option => toha("stage", &option.into_iter().collect::<Vec<_>>(), &operands),
                }
            }
            Self::Apply {
                template,
                path,
                answers,
                force,
                dry_run,
                trust,
            } => {
                let mut options = Vec::new();
                if let Some(answers) = answers {
                    options.push(option_value("--answers", answers));
                }
                for (set, flag) in [
                    (force, "--force"),
                    (trust, "--trust"),
                    (dry_run, "--dry-run"),
                ] {
                    if set {
                        options.push(flag.to_string());
                    }
                }
                let operands: Vec<Operand> = template
                    .map(Arg::operand)
                    .into_iter()
                    .chain([path_operand(path)])
                    .collect();
                toha("apply", &options, &operands)
            }
        }
    }
}

fn abort(path: &Path) -> String {
    toha("abort", &[], &[path_operand(path)])
}
fn apply_staged(path: &Path) -> String {
    toha("apply", &[], &[path_operand(path)])
}
fn continue_prompting(path: &Path) -> String {
    toha("continue", &[], &[path_operand(path)])
}
fn continue_answers(path: &Path) -> String {
    toha(
        "continue",
        &[],
        &[path_operand(path), placeholder("<ANSWERS>")],
    )
}

fn finish(path: &Path, progress: Progress) -> String {
    let apply = apply_staged(path);
    let resume = continue_prompting(path);
    match progress {
        Progress::Complete => format!("to finish the staged interview: {apply}"),
        Progress::Incomplete => {
            format!("to finish the staged interview: {resume}, then {apply}")
        }
        Progress::Unknown => {
            format!("to finish the staged interview: {resume} if questions remain, then {apply}")
        }
    }
}

fn remaining(path: &Path) -> String {
    format!(
        "to answer them: {} in a terminal, or {} with an answers document (- reads standard input)",
        continue_prompting(path),
        continue_answers(path)
    )
}

/// `stage` or `apply` named a template other than the staged one.
pub fn other_template(
    invocation: &Invocation,
    path: &Path,
    staged: &str,
    formal: &str,
    progress: Progress,
) -> String {
    let arg = invocation.template().unwrap_or(formal);
    let named = if arg == formal {
        arg.to_string()
    } else {
        format!("{arg} ({formal})")
    };
    [
        format!(
            "an interview for {staged} is staged at {}; {named} is a different template",
            target(path)
        ),
        format!(
            "to {} {} instead: {}, then {}",
            invocation.verb(),
            word(arg),
            abort(path),
            invocation.command()
        ),
        finish(path, progress),
    ]
    .join("\n")
}

/// `stage` named the template whose interview is already staged.
pub fn already_staged(
    invocation: &Invocation,
    path: &Path,
    staged: &str,
    progress: Progress,
) -> String {
    let p = target(path);
    let mut lines = match progress {
        Progress::Complete => vec![
            format!("an interview for {staged} is already staged at {p} and is complete"),
            format!("to write its files: {}", apply_staged(path)),
        ],
        Progress::Incomplete => vec![
            format!(
                "an interview for {staged} is already staged at {p} and has questions remaining"
            ),
            remaining(path),
            format!("then write its files: {}", apply_staged(path)),
        ],
        Progress::Unknown => vec![
            format!("an interview for {staged} is already staged at {p}"),
            finish(path, progress),
        ],
    };
    lines.push(format!(
        "to start it again: {}, then {}",
        abort(path),
        invocation.command()
    ));
    lines.join("\n")
}

/// `apply` found the staged interview incomplete.
pub fn incomplete(path: &Path) -> String {
    [
        format!("the interview at {} has questions remaining", target(path)),
        remaining(path),
        format!("then write its files: {}", apply_staged(path)),
    ]
    .join("\n")
}

/// `apply --answers --dry-run` left questions remaining.
pub fn dry_run_incomplete(invocation: &Invocation, path: &Path) -> String {
    [
        format!(
            "questions remain for the interview at {}; a dry run records nothing",
            target(path)
        ),
        format!(
            "to preview the files: add answers for the batch to the answers document, then {}",
            invocation.command()
        ),
        format!(
            "to record these answers: {}",
            invocation.without_dry_run().command()
        ),
    ]
    .join("\n")
}

/// `continue` found the staged interview complete.
pub fn complete(path: &Path) -> String {
    format!(
        "the interview at {} is complete; to write its files: {}",
        target(path),
        apply_staged(path)
    )
}

/// An answers document was given for a complete staged interview.
pub fn complete_answers_unused(path: &Path, staged: &str) -> String {
    let restage = Invocation::Stage {
        template: Arg::Given(formal_for("stage", staged)),
        path,
        output: None,
    };
    [
        format!(
            "the interview at {} is complete, so the answers document is not used",
            target(path)
        ),
        format!("to write its files: {}", apply_staged(path)),
        format!(
            "to answer it again: {}, then {}",
            abort(path),
            restage.command()
        ),
    ]
    .join("\n")
}

/// A staged record at `path` that no longer replays against its template, and
/// the commands that start the interview over.
pub fn replay_failed(path: &Path, staged: &str, error: &str) -> String {
    let restage = Invocation::Stage {
        template: Arg::Given(formal_for("stage", staged)),
        path,
        output: None,
    };
    [
        format!(
            "the staged interview at {} cannot be resumed: {error}",
            target(path)
        ),
        format!("to start over: {}, then {}", abort(path), restage.command()),
    ]
    .join("\n")
}

/// The rejections of an answers document for the interview at `path`, each
/// rejection of a different answer for an answered question followed by the
/// commands that change it.
pub fn explain_rejections(
    rejections: &toha::Rejections,
    path: &Path,
    staged: &str,
) -> toha::Rejections {
    let restage = Invocation::Stage {
        template: Arg::Given(formal_for("stage", staged)),
        path,
        output: Some(None),
    };
    rejections
        .iter()
        .cloned()
        .map(|mut rejection| {
            if rejection.kind == toha::RejectionKind::Answered {
                rejection.message = format!(
                    "{}; to change it: {}, then {}",
                    rejection.message,
                    abort(path),
                    restage.command()
                );
            }
            rejection
        })
        .collect()
}

/// `continue` without an answers document has no terminal to prompt in.
pub fn continue_no_terminal(path: &Path) -> String {
    [
        format!(
            "no terminal to prompt in for the interview at {}",
            target(path)
        ),
        format!(
            "to answer the current batch: {} with an answers document (- reads standard input)",
            continue_answers(path)
        ),
    ]
    .join("\n")
}

/// `continue`, `apply`, or `abort` found no staged interview.
pub fn nothing_staged(path: &Path) -> String {
    let template = placeholder("<TEMPLATE>");
    [
        format!("no interview is staged at {}", target(path)),
        format!(
            "to start one: {}",
            toha(
                "stage",
                &[],
                &[placeholder("<TEMPLATE>"), path_operand(path)]
            )
        ),
        format!(
            "to start one and write its files: {}",
            toha("apply", &[], &[template, path_operand(path)])
        ),
    ]
    .join("\n")
}

/// `stage` or `apply` with a template has no terminal to prompt in.
pub fn no_terminal(template: &str, path: &Path) -> String {
    let stage = Invocation::Stage {
        template: Arg::Given(template),
        path,
        output: Some(None),
    };
    let apply = Invocation::Apply {
        template: Some(Arg::Given(template)),
        path,
        answers: Some(Arg::Placeholder("<FILE>")),
        force: false,
        dry_run: false,
        trust: false,
    };
    [
        "no terminal to prompt in".to_string(),
        format!(
            "to stage the interview and emit its question batch: {}",
            stage.command()
        ),
        format!(
            "to answer from an answers document and write the files: {}",
            apply.command()
        ),
    ]
    .join("\n")
}

/// `apply <path> --answers <file>` names no template.
pub fn answers_without_template(
    path: &Path,
    answers: &str,
    staged: Option<(&str, Progress)>,
) -> String {
    let apply_new = |template| Invocation::Apply {
        template: Some(template),
        path,
        answers: Some(Arg::Given(answers)),
        force: false,
        dry_run: false,
        trust: false,
    };
    let mut lines = vec!["--answers requires a template".to_string()];
    match staged {
        Some((_, Progress::Complete)) => lines.push(format!(
            "the staged interview at {} is complete; to write its files: {}",
            target(path),
            apply_staged(path)
        )),
        Some((staged, _)) => {
            lines.push(format!(
                "to answer the staged interview: {}, then {}",
                toha(
                    "continue",
                    &[],
                    &[path_operand(path), file_operand(answers)]
                ),
                apply_staged(path)
            ));
            lines.push(format!(
                "to answer it and write its files in one command: {}",
                apply_new(Arg::Given(formal_for("apply", staged))).command()
            ));
        }
        None => lines.push(format!(
            "to answer a new interview and write its files: {}",
            apply_new(Arg::Placeholder("<TEMPLATE>")).command()
        )),
    }
    lines.join("\n")
}

/// `apply` did not run hooks of an untrusted template.
pub fn needs_trust(invocation: &Invocation, installed: Option<&str>) -> String {
    trust_lines(
        "hooks will not run without --trust\nto run them this time: ",
        invocation,
        installed,
    )
}

/// `apply --dry-run` planned hooks of an untrusted template, which the same
/// command without `--dry-run` refuses to run.
pub fn dry_run_needs_trust(invocation: &Invocation, installed: Option<&str>) -> String {
    trust_lines(
        "hooks need trust; to run them: ",
        &invocation.without_dry_run(),
        installed,
    )
}

/// `lead` followed by the command with `--trust`, and the `templates add
/// --trust` command for an installed template.
fn trust_lines(lead: &str, invocation: &Invocation, installed: Option<&str>) -> String {
    let mut trusted = *invocation;
    if let Invocation::Apply { trust, .. } = &mut trusted {
        *trust = true;
    }
    let mut lines = vec![format!("{lead}{}", trusted.command())];
    if let Some(formal) = installed {
        lines.push(format!(
            "to trust {formal} for every run: {}",
            toha(
                "templates add",
                &["--trust".into()],
                &[value(formal_for("templates add", formal))]
            )
        ));
    }
    lines.join("\n")
}

/// `apply` found target files that exist.
pub fn conflicts(invocation: &Invocation) -> String {
    let mut forced = *invocation;
    if let Invocation::Apply { force, .. } = &mut forced {
        *force = true;
    }
    format!("to overwrite them: {}", forced.command())
}

/// `templates add` was given a name, which is not an address.
pub fn add_needs_address(name: &str, folder_exists: bool) -> String {
    let mut lines = vec![
        "add requires a git address or folder".to_string(),
        format!(
            "to see installed templates: {}",
            toha("templates list", &[], &[])
        ),
    ];
    if folder_exists {
        lines.push(format!(
            "to add the folder {name}: {}",
            toha("templates add", &[], &[value(&format!("./{name}"))])
        ));
    }
    lines.push(format!(
        "to add from a git address or folder: {}",
        toha("templates add", &[], &[placeholder("<ADDRESS>")])
    ));
    lines.join("\n")
}

/// `templates add --alias` found more than one template at the address.
pub fn alias_needs_one_template(alias: &str, addresses: &[String]) -> String {
    let options = [format!("--alias {}", word(alias))];
    std::iter::once("--alias requires exactly one template".to_string())
        .chain(addresses.iter().map(|address| {
            format!(
                "to add {address} as {alias}: {}",
                toha("templates add", &options, &[value(address)])
            )
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `templates alias` was given an alias that names another template.
pub fn alias_in_use(template: &str, alias: &str, removable: bool) -> String {
    let mut lines = vec!["alias is in use".to_string()];
    if removable {
        lines.push(format!(
            "to move it to {template}: {}, then {}",
            toha(
                "templates alias",
                &[format!("--remove {}", word(alias))],
                &[]
            ),
            toha("templates alias", &[], &[value(template), value(alias)])
        ));
    }
    lines.push(format!(
        "to choose another alias: {}",
        toha(
            "templates alias",
            &[],
            &[value(template), placeholder("<ALIAS>")]
        )
    ));
    lines.join("\n")
}

/// A `templates` command that changes the user registry named a template from
/// another layer.
pub fn not_in_user_registry(name: &str) -> String {
    [
        format!("not installed in user registry: {name}"),
        format!(
            "to see the layer of each template: {}",
            toha("templates list", &["--json".into()], &[])
        ),
    ]
    .join("\n")
}

/// A template name matched more than one installed template.
pub fn ambiguous(name: &str, matches: &[String], retry: &[String]) -> String {
    let mut lines = vec![format!("ambiguous template name: {name}")];
    for (index, formal) in matches.iter().enumerate() {
        if let Some(command) = retry.get(index) {
            lines.push(format!("to use {formal}: {command}"));
        }
        lines.push(format!(
            "to give {formal} an alias: {}",
            toha(
                "templates alias",
                &[],
                &[
                    value(formal_for("templates alias", formal)),
                    placeholder("<ALIAS>")
                ]
            )
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{Shell, formal_for, path_value, plain_formal, word_for};

    #[test]
    fn each_command_gets_the_formal_name_form_it_accepts() {
        let extended = r"\\?\D:\a\template";
        for command in ["stage", "apply", "templates add"] {
            assert_eq!(formal_for(command, extended), r"D:\a\template", "{command}");
        }
        for command in ["templates remove", "templates update", "templates alias"] {
            assert_eq!(formal_for(command, extended), extended, "{command}");
        }
        assert_eq!(
            word_for(formal_for("templates remove", extended), Shell::Windows),
            r#""\\?\D:\a\template""#
        );
    }

    #[test]
    fn path_values_end_without_a_separator_except_a_root() {
        assert_eq!(path_value(r"C:\my dir\", Shell::Windows), r"C:\my dir");
        assert_eq!(path_value("C:/my dir/", Shell::Windows), "C:/my dir");
        assert_eq!(path_value(r"C:\", Shell::Windows), r"C:\");
        assert_eq!(path_value("C:/", Shell::Windows), "C:/");
        assert_eq!(path_value(r"\", Shell::Windows), r"\");
        assert_eq!(path_value("notes/", Shell::Posix), "notes");
        assert_eq!(path_value("/", Shell::Posix), "/");
        assert_eq!(path_value(r"a\", Shell::Posix), r"a\");
        assert_eq!(
            word_for(path_value(r"C:\my dir\", Shell::Windows), Shell::Windows),
            r#""C:\my dir""#
        );
        assert_eq!(
            word_for(path_value(r"C:\", Shell::Windows), Shell::Windows),
            r"C:\"
        );
        assert_eq!(path_value("", Shell::Windows), "");
        assert_eq!(path_value("", Shell::Posix), "");
        assert_eq!(path_value(r"\\?\C:\", Shell::Windows), r"\\?\C:\");
        assert_eq!(path_value(r"\\?\C:\dir\", Shell::Windows), r"\\?\C:\dir");
        assert_eq!(path_value(r"\\?\C:\\", Shell::Windows), r"\\?\C:\");
    }

    #[test]
    fn windows_quoted_words_keep_trailing_backslashes_outside_the_quotes() {
        assert_eq!(word_for(r"\\?\C:\", Shell::Windows), r#""\\?\C:"\"#);
        assert_eq!(word_for(r"C:\my dir\", Shell::Windows), r#""C:\my dir"\"#);
        assert_eq!(word_for(r"C:\my dir\\", Shell::Windows), r#""C:\my dir"\\"#);
        assert_eq!(word_for(r"C:\my dir", Shell::Windows), r#""C:\my dir""#);
        assert_eq!(word_for("", Shell::Windows), r#""""#);
    }

    #[test]
    fn extended_drive_paths_are_named_by_their_drive_path() {
        assert_eq!(plain_formal(r"\\?\D:\a\template"), r"D:\a\template");
        for kept in [
            r"\\?\UNC\server\share",
            r"\\server\share",
            "/srv/template",
            "gh:org/repo#one",
        ] {
            assert_eq!(plain_formal(kept), kept);
        }
    }

    const SPECIAL: [&str; 19] = [
        "", "#x", "$x", "`x`", "a*", "a?", "[a]", "a!", "a&b", "a;b", "a|b", "<a>", "(a)", "{a}",
        "a'b", "a\"b", "=x", "a b", "~/x",
    ];

    #[test]
    fn posix_words_a_shell_would_change_are_single_quoted() {
        for plain in [
            "notes",
            "a/b.json",
            "x=y",
            "gh:org/repo@main",
            "-",
            "a,b%c+d",
        ] {
            assert_eq!(word_for(plain, Shell::Posix), plain);
        }
        for special in SPECIAL.into_iter().chain([r"a\b", "a~b"]) {
            let quoted = word_for(special, Shell::Posix);
            assert!(
                quoted.starts_with('\'') && quoted.ends_with('\''),
                "{special}: {quoted}"
            );
        }
        assert_eq!(word_for("a'b", Shell::Posix), r"'a'\''b'");
    }

    #[test]
    fn windows_words_keep_paths_plain_and_double_quote_the_rest() {
        for plain in [
            r"C:\work",
            r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmpA1",
            r"D:\a\toha\tests\fixtures\text-basic\template",
            "file:///C:/Users/x/remote",
            "notes",
            "a/b.json",
            "x=y",
            "-",
        ] {
            assert_eq!(word_for(plain, Shell::Windows), plain);
        }
        for special in SPECIAL {
            let quoted = word_for(special, Shell::Windows);
            assert!(
                quoted.starts_with('"') && quoted.ends_with('"'),
                "{special}: {quoted}"
            );
        }
        assert_eq!(word_for(r"C:\my dir", Shell::Windows), r#""C:\my dir""#);
        assert_eq!(word_for(r#"a"b"#, Shell::Windows), r#""a""b""#);
    }
}
