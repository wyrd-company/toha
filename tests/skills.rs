// ---
// relationships:
//   implements: command-line-interface
// ---
#[allow(dead_code)]
mod support;
use std::{fs, path::Path, process::Command};

fn toha(args: &[&str]) -> std::process::Output {
    let isolated = tempfile::tempdir().unwrap();
    support::isolated_command(isolated.path())
        .args(args)
        .output()
        .unwrap()
}

fn assert_file_tree(source: &Path, destination: &Path) {
    let mut names: Vec<_> = fs::read_dir(source)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    let mut exported: Vec<_> = fs::read_dir(destination)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    exported.sort();
    assert_eq!(exported, names);
    for name in names {
        let source = source.join(&name);
        let destination = destination.join(&name);
        if source.is_dir() {
            assert_file_tree(&source, &destination);
        } else {
            assert_eq!(fs::read(&destination).unwrap(), fs::read(&source).unwrap());
        }
    }
}

#[test]
fn list_snapshot_and_frontmatter() {
    let output = toha(&["skills", "list"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, include_bytes!("snapshots/skills-list.json"));
    let list: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<_> = list
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["toha", "toha-templates"]);
    for name in names {
        let file = fs::read_to_string(format!("skills/{name}/SKILL.md")).unwrap();
        let yaml = file
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap()
            .0;
        let metadata: serde_json::Value = serde_norway::from_str(yaml).unwrap();
        assert_eq!(metadata["name"], name);
        assert_eq!(
            metadata["description"],
            list.iter().find(|item| item["name"] == name).unwrap()["description"]
        );
    }
}

#[test]
fn view_and_export_round_trip() {
    let destination = tempfile::tempdir().unwrap();
    for name in ["toha", "toha-templates"] {
        let source = Path::new("skills").join(name);
        let view = toha(&["skills", "view", name]);
        assert!(view.status.success());
        assert_eq!(view.stdout, fs::read(source.join("SKILL.md")).unwrap());
        let reference = if name == "toha" {
            "references/protocol.md"
        } else {
            "references/format.md"
        };
        let path = toha(&["skills", "view", name, "-p", reference]);
        assert!(path.status.success());
        assert_eq!(path.stdout, fs::read(source.join(reference)).unwrap());
        let export = toha(&[
            "skills",
            "view",
            name,
            "-e",
            destination.path().to_str().unwrap(),
        ]);
        assert!(
            export.status.success(),
            "{}",
            String::from_utf8_lossy(&export.stderr)
        );
        assert_file_tree(&source, &destination.path().join(name));
    }
}

#[test]
fn view_errors_and_export_does_not_overwrite() {
    for args in [
        vec!["skills", "view", "unknown"],
        vec!["skills", "view", "toha", "-p", "references/absent.md"],
    ] {
        let output = toha(&args);
        assert_eq!(output.status.code(), Some(1));
    }
    let unknown = toha(&["skills", "view", "unknown"]);
    let error = String::from_utf8(unknown.stderr).unwrap();
    assert!(error.contains("toha, toha-templates"));
    let escaped = toha(&["skills", "view", "toha", "-p", "../toha-templates/SKILL.md"]);
    assert_eq!(escaped.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&escaped.stderr).contains("outside skill"));
    let destination = tempfile::tempdir().unwrap();
    fs::create_dir(destination.path().join("toha")).unwrap();
    let existing = destination.path().join("toha/SKILL.md");
    fs::write(&existing, "keep").unwrap();
    let output = toha(&[
        "skills",
        "view",
        "toha",
        "-e",
        destination.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("toha/SKILL.md"));
    assert_eq!(fs::read_to_string(existing).unwrap(), "keep");
    assert!(!destination.path().join("toha/references").exists());
    let later_collision = tempfile::tempdir().unwrap();
    fs::create_dir_all(later_collision.path().join("toha/references")).unwrap();
    fs::write(
        later_collision.path().join("toha/references/protocol.md"),
        "keep",
    )
    .unwrap();
    let output = toha(&[
        "skills",
        "view",
        "toha",
        "-e",
        later_collision.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!later_collision.path().join("toha/SKILL.md").exists());
    let ancestor_collision = tempfile::tempdir().unwrap();
    fs::create_dir(ancestor_collision.path().join("toha")).unwrap();
    let blocker = ancestor_collision.path().join("toha/references");
    fs::write(&blocker, "keep").unwrap();
    let output = toha(&[
        "skills",
        "view",
        "toha",
        "-e",
        ancestor_collision.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("file blocks export: {}\n", blocker.display())
    );
    assert_eq!(fs::read_to_string(blocker).unwrap(), "keep");
    assert!(!ancestor_collision.path().join("toha/SKILL.md").exists());
    assert_eq!(
        toha(&["skills", "view", "toha", "-p", "SKILL.md", "-e", "."])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn documented_protocol_exchange() {
    let folder = tempfile::tempdir().unwrap();
    let template = folder.path().join("template");
    let target = folder.path().join("target");
    fs::create_dir(&template).unwrap();
    fs::create_dir(&target).unwrap();
    for file in ["template.yml", "template/{{ slug }}.txt"] {
        let source = Path::new("docs/examples/basic").join(file);
        assert_documented_file(&source);
        let destination = template.join(file);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
    let doc = fs::read_to_string("skills/toha/references/protocol.md").unwrap();
    let mut lines = doc.lines();
    let mut count = 0;
    while let Some(line) = lines.next() {
        if line != "```sh" {
            continue;
        }
        let mut script = String::new();
        for line in lines.by_ref() {
            if line == "```" {
                break;
            }
            script.push_str(line);
            script.push('\n');
        }
        if !script.starts_with("# test\n") {
            continue;
        }
        let expected = loop {
            match lines.next() {
                Some("```text") => {
                    break lines
                        .by_ref()
                        .take_while(|line| *line != "```")
                        .collect::<Vec<_>>()
                        .join("\n")
                        + "\n";
                }
                Some(_) => {}
                None => panic!("missing documented output"),
            }
        };
        let mut command = Command::new("sh");
        support::isolate(&mut command, folder.path());
        let output = command
            .arg("-eu")
            .arg("-c")
            .arg(&script)
            .env("TOHA_BIN", assert_cmd::cargo::cargo_bin!("toha"))
            .env("TOHA_TEMPLATE", &template)
            .env("TOHA_TARGET", &target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .replace("\r\n", "\n"),
            expected
        );
        count += 1;
    }
    assert_eq!(count, 1);
}

/// The protocol exchange shows each file of the template it runs, so that
/// a reader with only the binary sees the questions it answers.
fn assert_documented_file(source: &Path) {
    let example = fs::read_to_string(source).unwrap();
    let header = "# ---\n# relationships:\n#   exemplifies: template-format\n# ---\n";
    let example = example.strip_prefix(header).unwrap_or(&example);
    let doc = fs::read_to_string("skills/toha/references/protocol.md").unwrap();
    let blocks: Vec<_> = doc
        .split("\n```")
        .skip(1)
        .step_by(2)
        .filter_map(|block| block.split_once('\n'))
        .map(|(_, body)| format!("{body}\n"))
        .collect();
    assert!(
        blocks.iter().any(|block| block == example),
        "protocol.md does not show {}",
        source.display()
    );
}

/// Every file of every embedded skill, as `toha skills view --export` writes it.
fn exported_skills() -> (tempfile::TempDir, Vec<(String, std::path::PathBuf, String)>) {
    let export = tempfile::tempdir().unwrap();
    let destination = export.path();
    let mut files = Vec::new();
    for name in ["toha", "toha-templates"] {
        let output = toha(&["skills", "view", name, "-e", destination.to_str().unwrap()]);
        assert!(output.status.success());
        let mut pending = vec![destination.join(name)];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let text = fs::read_to_string(&path).unwrap();
                    files.push((name.to_string(), path, text));
                }
            }
        }
    }
    (export, files)
}

/// A reader with only the binary can read what a skill names: no repository
/// documents, and only files of the same skill, each with the command that
/// prints it.
#[test]
fn embedded_skills_name_only_what_the_binary_shows() {
    let (_export, files) = exported_skills();
    for (name, path, text) in files {
        let root = path
            .ancestors()
            .find(|dir| dir.file_name().is_some_and(|file| file == name.as_str()))
            .unwrap()
            .to_path_buf();
        let file = path.strip_prefix(&root).unwrap().display().to_string();
        assert!(!text.contains("docs/"), "{name}/{file} names a docs/ path");
        assert!(
            !text.to_lowercase().contains("specification"),
            "{name}/{file} points at a specification"
        );
        for link in text.split("](").skip(1) {
            let target = &link[..link.find(')').unwrap()];
            if target.contains("://") || target.starts_with('#') {
                continue;
            }
            let linked = path.parent().unwrap().join(target);
            assert!(linked.is_file(), "{name}/{file} links to {target}");
            let within = linked
                .canonicalize()
                .unwrap()
                .strip_prefix(root.canonicalize().unwrap())
                .unwrap()
                .display()
                .to_string()
                .replace('\\', "/");
            let command = format!("toha skills view {name} --path {within}");
            assert!(
                text.contains(&command),
                "{name}/{file} links to {target} without `{command}`"
            );
        }
    }
}

#[test]
fn toha_skill_names_every_environment_variable() {
    let (_export, files) = exported_skills();
    let text: String = files
        .into_iter()
        .filter(|(name, _, _)| name == "toha")
        .map(|(_, _, text)| text)
        .collect();
    let named: Vec<_> = text
        .split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .collect();
    for variable in support::ENVIRONMENT {
        assert!(named.contains(variable), "{variable}");
    }
}
