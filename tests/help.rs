// ---
// relationships:
//   implements: command-line-interface
// ---
#[allow(dead_code)]
mod support;

fn help(args: &[&str], flag: &str) -> String {
    let isolation = tempfile::tempdir().unwrap();
    let output = support::isolated_command(isolation.path())
        .args(args)
        .arg(flag)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Clap indents an entry by two spaces, or by six for a long-only flag.
fn is_entry(line: &str) -> bool {
    indent(line) == 2 || (indent(line) == 6 && line.trim_start().starts_with("--"))
}

/// Every entry under `Arguments:`, `Options:`, and `Commands:` with its
/// description, which is the text after the entry name on the same line, or
/// the more deeply indented lines that follow it.
fn entries(text: &str) -> Vec<(String, String)> {
    let lines: Vec<_> = text.lines().collect();
    let mut result = Vec::new();
    let mut in_section = false;
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            in_section = matches!(*line, "Arguments:" | "Options:" | "Commands:");
            continue;
        }
        if !in_section || !is_entry(line) {
            continue;
        }
        let entry = line.trim();
        let (name, mut description) = match entry.find("  ") {
            Some(at) => (entry[..at].to_string(), entry[at..].trim().to_string()),
            None => (entry.to_string(), String::new()),
        };
        if description.is_empty() {
            description = lines[index + 1..]
                .iter()
                .take_while(|next| indent(next) > 2 && !is_entry(next))
                .map(|next| next.trim())
                .collect::<Vec<_>>()
                .join(" ");
        }
        result.push((name, description));
    }
    result
}

fn commands(text: &str) -> Vec<String> {
    entries(&text[text.find("Commands:").unwrap_or(text.len())..])
        .into_iter()
        .map(|(name, _)| name)
        .take_while(|name| !name.starts_with('-'))
        .filter(|name| name != "help")
        .collect()
}

#[test]
fn every_command_argument_and_option_has_a_description() {
    let mut pending = vec![Vec::<String>::new()];
    let mut seen = 0;
    let mut missing = Vec::new();
    while let Some(path) = pending.pop() {
        let args: Vec<_> = path.iter().map(String::as_str).collect();
        for flag in ["-h", "--help"] {
            let text = help(&args, flag);
            if text.starts_with("Usage:") {
                missing.push(format!("toha {} {flag}: summary", args.join(" ")));
            }
            for (name, description) in entries(&text) {
                if description.is_empty() {
                    missing.push(format!("toha {} {flag}: {name}", args.join(" ")));
                }
            }
        }
        for command in commands(&help(&args, "-h")) {
            let mut next = path.clone();
            next.push(command);
            pending.push(next);
        }
        seen += 1;
    }
    assert!(
        missing.is_empty(),
        "no description:\n{}",
        missing.join("\n")
    );
    // toha; 7 root commands; templates with 7 subcommands; skills with 2.
    assert_eq!(seen, 1 + 7 + 7 + 2);
}

#[test]
fn apply_help_names_its_values() {
    let text = help(&["apply"], "--help");
    assert!(
        text.contains("Usage: toha apply [OPTIONS] [TEMPLATE] <PATH>"),
        "{text}"
    );
    assert!(text.contains("--answers <FILE>"), "{text}");
    let names: Vec<_> = entries(&text).into_iter().map(|(name, _)| name).collect();
    for name in [
        "[TEMPLATE]",
        "<PATH>",
        "-A, --answers <FILE>",
        "-f, --force",
        "-d, --dry-run",
        "--trust",
    ] {
        assert!(names.iter().any(|n| n == name), "{name}: {names:?}");
    }
}

/// The words made of capitals, digits, and underscores in `text`.
fn names(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect()
}

#[test]
fn top_level_help_points_to_documentation_without_listing_variables() {
    let documentation = include_str!("../docs/user/configuration.md");
    let normalized = documentation.replace("\r\n", "\n");
    let windows = normalized.replace('\n', "\r\n");
    for guide in [&normalized, &windows] {
        let mut lines = guide.lines();
        assert!(
            lines.any(|line| line == "## Environment variables"),
            "configuration guide has no environment variables section"
        );
        let table = lines
            .skip_while(|line| !line.starts_with('|'))
            .take_while(|line| line.starts_with('|'))
            .filter_map(|line| line.split('|').nth(1))
            .collect::<Vec<_>>()
            .join(" ");
        let documented = names(&table);
        for variable in support::ENVIRONMENT {
            assert!(
                documented.contains(variable),
                "{variable} missing from configuration guide table"
            );
        }
    }
    for flag in ["-h", "--help"] {
        let text = help(&[], flag);
        assert!(
            text.contains("https://wyrd.foo/docs/toha/configuration#environment-variables"),
            "{text}"
        );
        let named = names(&text);
        for variable in support::ENVIRONMENT {
            assert!(!named.contains(variable), "{variable}:\n{text}");
        }
    }
}
