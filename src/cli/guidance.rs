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

/// `value` as one shell word.
pub fn word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-~\\#".contains(c));
    if plain {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

fn target(path: &Path) -> String {
    word(&path.to_string_lossy())
}

/// The command line of this process, as the caller can run it again.
pub fn this_command() -> String {
    std::iter::once("toha".to_string())
        .chain(std::env::args().skip(1).map(|arg| word(&arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn finish(path: &Path, progress: Progress) -> String {
    let p = target(path);
    match progress {
        Progress::Complete => format!("to finish the staged interview: toha apply {p}"),
        Progress::Incomplete => {
            format!("to finish the staged interview: toha continue {p}, then toha apply {p}")
        }
        Progress::Unknown => format!(
            "to finish the staged interview: toha continue {p} if questions remain, then toha apply {p}"
        ),
    }
}

fn remaining(path: &Path) -> String {
    let p = target(path);
    format!(
        "to answer them: toha continue {p} in a terminal, or toha continue {p} <ANSWERS> with an answers document (- reads standard input)"
    )
}

/// `stage` or `apply` named a template other than the staged one.
pub fn other_template(
    verb: &str,
    path: &Path,
    staged: &str,
    arg: &str,
    formal: &str,
    progress: Progress,
) -> String {
    let named = if arg == formal {
        arg.to_string()
    } else {
        format!("{arg} ({formal})")
    };
    let p = target(path);
    [
        format!("an interview for {staged} is staged at {p}; {named} is a different template"),
        format!(
            "to {verb} {} instead: toha abort {p}, then {}",
            word(arg),
            this_command()
        ),
        finish(path, progress),
    ]
    .join("\n")
}

/// `stage` named the template whose interview is already staged.
pub fn already_staged(path: &Path, staged: &str, progress: Progress) -> String {
    let p = target(path);
    let mut lines = match progress {
        Progress::Complete => vec![
            format!("an interview for {staged} is already staged at {p} and is complete"),
            format!("to write its files: toha apply {p}"),
        ],
        Progress::Incomplete => vec![
            format!(
                "an interview for {staged} is already staged at {p} and has questions remaining"
            ),
            remaining(path),
            format!("then write its files: toha apply {p}"),
        ],
        Progress::Unknown => vec![
            format!("an interview for {staged} is already staged at {p}"),
            finish(path, progress),
        ],
    };
    lines.push(format!(
        "to start it again: toha abort {p}, then {}",
        this_command()
    ));
    lines.join("\n")
}

/// `apply` found the staged interview incomplete.
pub fn incomplete(path: &Path) -> String {
    let p = target(path);
    [
        format!("the interview at {p} has questions remaining"),
        remaining(path),
        format!("then write its files: toha apply {p}"),
    ]
    .join("\n")
}

/// The command line of this process without `--dry-run`.
fn this_command_without_dry_run() -> String {
    std::iter::once("toha".to_string())
        .chain(
            std::env::args()
                .skip(1)
                .filter(|arg| arg != "--dry-run" && arg != "-d")
                .map(|arg| word(&arg)),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// `apply --answers --dry-run` left questions remaining.
pub fn dry_run_incomplete(path: &Path) -> String {
    [
        format!(
            "questions remain for the interview at {}; a dry run records nothing",
            target(path)
        ),
        format!(
            "to preview the files: add answers for the batch to the answers document, then {}",
            this_command()
        ),
        format!(
            "to record these answers: {}",
            this_command_without_dry_run()
        ),
    ]
    .join("\n")
}

/// `continue` found the staged interview complete.
pub fn complete(path: &Path) -> String {
    format!(
        "the interview at {} is complete; to write its files: toha apply {}",
        target(path),
        target(path)
    )
}

/// An answers document was given for a complete staged interview.
pub fn complete_answers_unused(path: &Path, staged: &str) -> String {
    let p = target(path);
    [
        format!("the interview at {p} is complete, so the answers document is not used"),
        format!("to write its files: toha apply {p}"),
        format!(
            "to answer it again: toha abort {p}, then toha stage {} {p}",
            word(staged)
        ),
    ]
    .join("\n")
}

/// `continue` without an answers document has no terminal to prompt in.
pub fn continue_no_terminal(path: &Path) -> String {
    let p = target(path);
    [
        format!("no terminal to prompt in for the interview at {p}"),
        format!(
            "to answer the current batch: toha continue {p} <ANSWERS> with an answers document (- reads standard input)"
        ),
    ]
    .join("\n")
}

/// `continue`, `apply`, or `abort` found no staged interview.
pub fn nothing_staged(path: &Path) -> String {
    let p = target(path);
    [
        format!("no interview is staged at {p}"),
        format!("to start one: toha stage <TEMPLATE> {p}"),
        format!("to start one and write its files: toha apply <TEMPLATE> {p}"),
    ]
    .join("\n")
}
