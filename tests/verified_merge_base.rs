// ---
// relationships:
//   verifies: template-composition
// ---
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use std::{path::Path, process::Command};
use toha::applied::{AppliedRecord, SCHEMA_ID};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn repository(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    commit(root);
}
fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "example"]);
}
fn template(root: &Path, interview: &str, body: &str) {
    std::fs::create_dir_all(root.join("template")).unwrap();
    std::fs::write(
        root.join("template.yml"),
        format!("name: example\ndescription: Example\ninterview:\n{interview}"),
    )
    .unwrap();
    std::fs::write(root.join("template/result.txt"), body).unwrap();
}
fn run(
    iso: &Path,
    address: &str,
    target: &Path,
    answers: Value,
    flags: &[&str],
) -> std::process::Output {
    let file = iso.join("answers.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"template":address,"answers":answers})).unwrap(),
    )
    .unwrap();
    support::isolated_command(iso)
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--answers")
        .arg(file)
        .args(flags)
        .env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]")
        .output()
        .unwrap()
}
fn document(out: &std::process::Output) -> Value {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    support::first_document(&out.stdout)
}
fn record(root: &Path, path: &str) -> Value {
    let bytes = std::fs::read(root.join(path)).unwrap();
    AppliedRecord::parse(&bytes).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["$schema"], SCHEMA_ID);
    assert!(bytes.ends_with(b"\n"));
    value
}
const QUESTION: &str = "  - id: label\n    type: text\n    prompt: Label\n    required: true\n";

const GREEN: &str = "color = green\n";
const BLUE: &str = "color = blue\n";
const RED: &str = "color = red\n";
fn reference_merge(base: &str, ours: &str, theirs: &str, conflicts: bool) {
    let dir = tempfile::tempdir().unwrap();
    for (name, data) in [("base", base), ("ours", ours), ("theirs", theirs)] {
        std::fs::write(dir.path().join(name), data).unwrap();
    }
    let out = Command::new("git")
        .current_dir(dir.path())
        .args(["merge-file", "--diff3", "-p", "ours", "base", "theirs"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(i32::from(conflicts)));
    if conflicts {
        assert!(String::from_utf8_lossy(&out.stdout).contains(BLUE));
        assert!(String::from_utf8_lossy(&out.stdout).contains(RED));
    } else {
        assert_eq!(String::from_utf8(out.stdout).unwrap(), theirs);
    }
}
#[test]
fn control_old_host_false_base() {
    reference_merge(BLUE, BLUE, RED, false);
}
#[test]
fn control_actual_applied_base() {
    reference_merge(GREEN, BLUE, RED, true);
}
// Exercise the production restoration and merge, with a false re-render equal
// to ours. Keep exact stage assertions: a conflict alone cannot prove which
// ancestor was selected.
fn production_merge(origin: &str, rendered: &str, object: bool, identical: bool) {
    let iso = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    template(source.path(), QUESTION, "color = {{ label }}\n");
    repository(source.path());
    repository(root.path());
    let address = support::file_url(source.path());
    document(&run(
        iso.path(),
        &address,
        root.path(),
        json!({"label":"green"}),
        &[],
    ));
    commit(root.path());
    let mut value = record(root.path(), ".toha.json");
    let blob = value["applications"][0]["paths"]["result.txt"]["blob"]
        .as_str()
        .unwrap()
        .to_owned();
    value["applications"][0]["answers"]["label"] = json!(rendered);
    value["applications"][0]["paths"]["result.txt"]["origin"] = json!(origin);
    std::fs::write(
        root.path().join(".toha.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    std::fs::write(root.path().join("result.txt"), BLUE).unwrap();
    commit(root.path());
    if !object {
        std::fs::remove_file(
            root.path()
                .join(".git/objects")
                .join(&blob[..2])
                .join(&blob[2..]),
        )
        .unwrap();
    }
    template(source.path(), QUESTION, if identical { BLUE } else { RED });
    commit(source.path());
    let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
    let result = document(&run(
        iso.path(),
        &next,
        root.path(),
        json!({}),
        &["--update"],
    ));
    assert_eq!(
        result["merge"]["conflicted"],
        if identical {
            json!([])
        } else {
            json!(["result.txt"])
        }
    );
    let has_base = (origin == "toha" && rendered == "green") || object;
    reference_merge(
        if has_base { GREEN } else { "" },
        BLUE,
        if identical { BLUE } else { RED },
        !identical,
    );
    let stages = git(root.path(), &["ls-files", "-u", "result.txt"]);
    if !identical {
        assert_eq!(stages.contains(" 1\tresult.txt"), has_base, "{stages}");
        assert_eq!(
            result["merge"]["changes"][0]["conflict"],
            if has_base { "content" } else { "add-add" }
        );
        assert_eq!(git(root.path(), &["show", ":2:result.txt"]), BLUE.trim());
        assert_eq!(git(root.path(), &["show", ":3:result.txt"]), RED.trim());
        if has_base {
            assert_eq!(git(root.path(), &["show", ":1:result.txt"]), GREEN.trim());
        } else {
            assert_eq!(
                std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
                BLUE
            );
        }
    } else {
        assert_eq!(
            std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
            BLUE
        );
        assert!(stages.is_empty());
    }
}
#[test]
fn recorded_context_rerender() {
    production_merge("toha", "green", false, false);
}
#[test]
fn host_mismatch_recorded_object() {
    production_merge("toha", "blue", true, false);
}
#[test]
fn host_mismatch_object_missing() {
    production_merge("toha", "blue", false, false);
}
#[test]
fn hook_changed_planned_object_missing() {
    production_merge("toha", "blue", false, false);
}
#[test]
fn hook_owned_object_present() {
    production_merge("hook", "blue", true, false);
}
#[test]
fn hook_owned_object_missing() {
    production_merge("hook", "blue", false, false);
}
#[test]
fn hook_render_matches_but_no_object() {
    production_merge("hook", "green", false, false);
}
#[test]
fn missing_base_identical_candidate() {
    production_merge("toha", "blue", false, true);
}
#[test]
fn pin_updates_merge_disjoint_template_changes_and_same_line_edits_conflict() {
    for conflict in [false, true] {
        let iso = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        template(
            source.path(),
            QUESTION,
            "uses: example/action@first\n\n\nname: first\n",
        );
        repository(source.path());
        repository(root.path());
        let address = support::file_url(source.path());
        document(&run(
            iso.path(),
            &address,
            root.path(),
            json!({"label":"sample"}),
            &[],
        ));
        commit(root.path());
        std::fs::write(
            root.path().join("result.txt"),
            "uses: example/action@operator\n\n\nname: first\n",
        )
        .unwrap();
        commit(root.path());
        template(
            source.path(),
            QUESTION,
            if conflict {
                "uses: example/action@template\n\n\nname: first\n"
            } else {
                "uses: example/action@first\n\n\nname: second\n"
            },
        );
        commit(source.path());
        let next = format!("{address}@{}", git(source.path(), &["rev-parse", "HEAD"]));
        let result = document(&run(
            iso.path(),
            &next,
            root.path(),
            json!({}),
            &["--update"],
        ));
        assert_eq!(
            result["merge"]["conflicted"],
            if conflict {
                json!(["result.txt"])
            } else {
                json!([])
            }
        );
        if !conflict {
            assert_eq!(
                std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
                "uses: example/action@operator\n\n\nname: second\n"
            );
        }
    }
}
