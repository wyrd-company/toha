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
    assert_eq!(count, 2);
}
