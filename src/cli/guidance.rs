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
/// form both shells accept, with `"` and the backslashes before it escaped by
/// the rules Windows programs use to split a command line.
///
/// Limits: inside double quotes cmd.exe still expands `%NAME%`, and PowerShell
/// still expands `$` and backtick escapes. A word with those characters is
/// quoted but not protected from expansion in every shell.
fn windows_word(value: &str) -> String {
    let plain = !value.is_empty()
        && !value.starts_with(['=', '~'])
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-\\~".contains(c));
    if plain {
        return value.into();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in value.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            c => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

fn target(path: &Path) -> String {
    word(&path.to_string_lossy())
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
    value(&path.to_string_lossy())
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
            format!("{flag}={}", word(value))
        }
        Arg::Given(value) => format!("{flag} {}", word(value)),
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
        template: Arg::Given(plain_formal(staged)),
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
                toha("continue", &[], &[path_operand(path), value(answers)]),
                apply_staged(path)
            ));
            lines.push(format!(
                "to answer it and write its files in one command: {}",
                apply_new(Arg::Given(plain_formal(staged))).command()
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
    let mut trusted = *invocation;
    if let Invocation::Apply { trust, .. } = &mut trusted {
        *trust = true;
    }
    let mut lines = vec![
        "hooks will not run without --trust".to_string(),
        format!("to run them this time: {}", trusted.command()),
    ];
    if let Some(formal) = installed {
        lines.push(format!(
            "to trust {formal} for every run: {}",
            toha(
                "templates add",
                &["--trust".into()],
                &[value(plain_formal(formal))]
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
                &[value(plain_formal(formal)), placeholder("<ALIAS>")]
            )
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{Shell, plain_formal, word_for};

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
        assert_eq!(word_for(r"C:\my dir\", Shell::Windows), r#""C:\my dir\\""#);
        assert_eq!(word_for(r#"a"b"#, Shell::Windows), r#""a\"b""#);
        assert_eq!(word_for(r#"a\"b"#, Shell::Windows), r#""a\\\"b""#);
    }
}
