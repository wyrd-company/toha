// ---
// relationships:
//   implements: content-injection
//   verifies: project-updates
// ---
//! Update refusal and explicit force at the CLI boundary.
#[allow(dead_code)]
mod support;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    output.stdout
}

struct Fixture {
    _root: tempfile::TempDir,
    iso: std::path::PathBuf,
    template: std::path::PathBuf,
    target: std::path::PathBuf,
    answers: std::path::PathBuf,
    address: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let iso = root.path().join("isolated");
        let template = root.path().join("template");
        let target = root.path().join("project");
        fs::create_dir_all(template.join("template")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(template.join("template/.keep"), "").unwrap();
        fs::write(template.join("template.yml"), Self::manifest("alpha = 1")).unwrap();
        git(&target, &["init", "--quiet"]);
        fs::write(
            target.join("config.toml"),
            "[application]\n\n\n[other]\nkeep = 1\n",
        )
        .unwrap();
        git(&target, &["add", "-A"]);
        git(&target, &["commit", "--quiet", "-m", "seed"]);
        let address = support::versioned_template(&template);
        fs::create_dir_all(&iso).unwrap();
        let answers = iso.join("answers.json");
        fs::write(
            &answers,
            serde_json::to_vec(&serde_json::json!({"template": address, "answers": {}})).unwrap(),
        )
        .unwrap();
        let fixture = Self {
            _root: root,
            iso,
            template,
            target,
            answers,
            address,
        };
        let output = fixture.apply(&[]);
        assert!(output.status.success(), "initial apply: {output:?}");
        fixture.commit("apply");
        fixture
    }
    fn manifest(body: &str) -> String {
        format!(
            "name: cfg\nignore: [.keep]\ninject:\n  - into: config.toml\n    region: features\n    content: |\n      {body}\n    anchor: {{ after: \"[application]\", occurrence: only }}\n"
        )
    }
    fn commit(&self, message: &str) {
        git(&self.target, &["add", "-A"]);
        git(&self.target, &["commit", "--quiet", "-m", message]);
    }
    fn drift(&self) {
        let path = self.target.join("config.toml");
        let content = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            content
                .replace("alpha = 1", "alpha = 7")
                .replace("keep = 1", "keep = 2"),
        )
        .unwrap();
        self.commit("operator edits");
    }
    fn unrelated_template_change(&self) {
        fs::write(
            self.template.join("template/new.txt"),
            "new template file\n",
        )
        .unwrap();
    }
    fn apply(&self, flags: &[&str]) -> Output {
        support::isolated_command(&self.iso)
            .arg("apply")
            .arg(support::refresh_template(&self.address))
            .arg(&self.target)
            .arg("--answers")
            .arg(&self.answers)
            .args(flags)
            .output()
            .unwrap()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct State {
    files: Vec<(String, Vec<u8>, u32)>,
    index: Vec<u8>,
    staged: Vec<u8>,
    refs: Vec<u8>,
}
fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().unwrap().permissions().mode()
    }
    #[cfg(not(unix))]
    {
        u32::from(path.metadata().unwrap().permissions().readonly())
    }
}
fn state(target: &Path) -> State {
    let mut files = Vec::new();
    for name in ["config.toml", ".toha.json"] {
        let path = target.join(name);
        files.push((name.into(), fs::read(&path).unwrap(), mode(&path)));
    }
    assert!(
        !target.join("new.txt").exists(),
        "planned new file must be absent"
    );
    assert!(
        !target.join(".git/index.lock").exists(),
        "no stale index lock"
    );
    State {
        files,
        index: fs::read(target.join(".git/index")).unwrap(),
        staged: git(target, &["diff", "--cached", "--binary"]),
        refs: git(target, &["show-ref", "--head"]),
    }
}

#[test]
fn committed_region_drift_update_refuses_atomically_after_unrelated_template_change() {
    let fixture = Fixture::new();
    fixture.drift();
    fixture.unrelated_template_change();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.target.join("config.toml"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fixture.commit("executable mode");
    }
    let before = state(&fixture.target);
    // Plain apply reaches the existing region-drift guard; no whole-file collision.
    let plain = fixture.apply(&[]);
    assert_eq!(plain.status.code(), Some(1), "{plain:?}");
    assert!(
        String::from_utf8_lossy(&plain.stdout).contains("region drift"),
        "{plain:?}"
    );
    assert_eq!(
        state(&fixture.target),
        before,
        "plain refusal writes nothing"
    );
    let update = fixture.apply(&["--update"]);
    assert_eq!(
        update.status.code(),
        Some(1),
        "unforced update must refuse committed region drift: {update:?}"
    );
    assert!(
        String::from_utf8_lossy(&update.stdout).contains("region drift"),
        "{update:?}"
    );
    assert_eq!(
        state(&fixture.target),
        before,
        "update refusal writes nothing"
    );
}

fn conflict(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["status"], "error", "{document}");
    assert_eq!(document["kind"], "conflict", "{document}");
    assert!(
        document["message"]
            .as_str()
            .unwrap()
            .contains("region drift"),
        "{document}"
    );
}

#[test]
fn one_shot_force_preview_is_read_only_and_force_replaces_only_the_owned_block() {
    let fixture = Fixture::new();
    fixture.drift();
    fixture.unrelated_template_change();
    let before = state(&fixture.target);
    conflict(&fixture.apply(&["--update", "--dry-run"]));
    assert_eq!(state(&fixture.target), before);
    let preview = fixture.apply(&["--update", "--force", "--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(
        support::first_document(&preview.stdout)["status"],
        "planned"
    );
    assert_eq!(
        state(&fixture.target),
        before,
        "forced preview must write nothing"
    );
    conflict(&fixture.apply(&["--update"]));
    assert_eq!(
        state(&fixture.target),
        before,
        "preview must not grant later force"
    );
    let applied = fixture.apply(&["--update", "--force"]);
    assert!(applied.status.success(), "{applied:?}");
    let content = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        content.contains("alpha = 1") && !content.contains("alpha = 7"),
        "{content}"
    );
    assert!(
        content.contains("keep = 2"),
        "outside edit must survive: {content}"
    );
    assert!(fixture.target.join("new.txt").exists());
    assert_ne!(
        fs::read(fixture.target.join(".toha.json")).unwrap(),
        before.files[1].1
    );
}

fn staged_files(iso: &Path) -> Vec<(String, Vec<u8>)> {
    let dir = support::staged_dir(iso);
    if !dir.exists() {
        return vec![];
    }
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn staged_update_rebuild_refuses_without_force_and_retains_a_usable_stage() {
    let fixture = Fixture::new();
    fixture.drift();
    fixture.unrelated_template_change();
    let stage = support::isolated_command(&fixture.iso)
        .arg("stage")
        .arg(support::refresh_template(&fixture.address))
        .arg(&fixture.target)
        .args(["--update", "--async", "--trust"])
        .output()
        .unwrap();
    assert!(stage.status.success(), "{stage:?}");
    let before = state(&fixture.target);
    let saved = staged_files(&fixture.iso);
    assert_eq!(saved.len(), 1, "actual persisted stage");
    // Each child process reloads the stage. Trust and previous progress grant no force.
    for flags in [&[][..], &["--dry-run"][..]] {
        let output = support::isolated_command(&fixture.iso)
            .arg("apply")
            .arg(&fixture.target)
            .args(flags)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("region drift"),
            "{output:?}"
        );
        assert_eq!(state(&fixture.target), before);
        assert_eq!(staged_files(&fixture.iso), saved, "refusal retains stage");
    }
    let preview = support::isolated_command(&fixture.iso)
        .arg("apply")
        .arg(&fixture.target)
        .args(["--force", "--dry-run"])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(state(&fixture.target), before);
    assert_eq!(
        staged_files(&fixture.iso),
        saved,
        "preview retains stage without force authorization"
    );
    let refused = support::isolated_command(&fixture.iso)
        .arg("apply")
        .arg(&fixture.target)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let applied = support::isolated_command(&fixture.iso)
        .arg("apply")
        .arg(&fixture.target)
        .arg("--force")
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
    assert!(
        staged_files(&fixture.iso).is_empty(),
        "successful apply consumes stage"
    );
    let content = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        content.contains("alpha = 1")
            && content.contains("keep = 2")
            && !content.contains("alpha = 7"),
        "{content}"
    );
}

#[test]
fn undrifted_changed_and_unchanged_regions_update_with_outside_edits() {
    for changed in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.target.join("config.toml");
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("keep = 1", "keep = 2"),
        )
        .unwrap();
        fixture.commit("outside edit");
        fixture.unrelated_template_change();
        if changed {
            fs::write(
                fixture.template.join("template.yml"),
                Fixture::manifest("alpha = 2"),
            )
            .unwrap();
        }
        let output = fixture.apply(&["--update"]);
        assert!(output.status.success(), "{output:?}");
        let content = fs::read_to_string(path).unwrap();
        assert!(
            content.contains(if changed { "alpha = 2" } else { "alpha = 1" }),
            "{content}"
        );
        assert!(content.contains("keep = 2"), "{content}");
        assert!(fixture.target.join("new.txt").exists());
    }
}

#[test]
fn mixed_whole_file_region_checks_the_planned_image_instead_of_head() {
    let fixture = Fixture::new();
    fixture.drift();
    // Whole-file ownership supplies an undrifted image before the region folds over it.
    fs::write(
        fixture.template.join("template/config.toml"),
        "[application]\n\n\n[other]\nkeep = 2\n",
    )
    .unwrap();
    fixture.unrelated_template_change();
    let output = fixture.apply(&["--update"]);
    assert!(
        output.status.success(),
        "mixed effective image must admit: {output:?}"
    );
    assert_eq!(
        support::first_document(&output.stdout)["merge"]["conflicted"],
        serde_json::json!([])
    );
    let content = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        content.contains("alpha = 1") && content.contains("keep = 2"),
        "{content}"
    );

    let fixture = Fixture::new();
    let planned = fs::read_to_string(fixture.target.join("config.toml"))
        .unwrap()
        .replace("alpha = 1", "alpha = 7");
    fs::write(fixture.template.join("template/config.toml"), planned).unwrap();
    fixture.unrelated_template_change();
    let before = state(&fixture.target);
    conflict(&fixture.apply(&["--update"]));
    assert_eq!(
        state(&fixture.target),
        before,
        "drift in the planned whole-file image must refuse even with undrifted HEAD"
    );
    let preview = fixture.apply(&["--update", "--force", "--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(state(&fixture.target), before);
}

#[test]
fn force_does_not_suppress_malformed_markers_missing_anchors_or_invalid_paths() {
    for fault in ["marker", "anchor", "path"] {
        let fixture = Fixture::new();
        let path = fixture.target.join("config.toml");
        match fault {
            "marker" => fs::write(
                &path,
                fs::read_to_string(&path)
                    .unwrap()
                    .replace("# <<< toha:end", "# broken toha:end"),
            )
            .unwrap(),
            "anchor" => fs::write(&path, "no anchor here\n").unwrap(),
            "path" => fs::write(
                fixture.template.join("template.yml"),
                Fixture::manifest("alpha = 1").replace("into: config.toml", "into: ../escape.toml"),
            )
            .unwrap(),
            _ => unreachable!(),
        }
        if fault != "path" {
            fixture.commit("invalid target");
        }
        fixture.unrelated_template_change();
        let before = state(&fixture.target);
        let output = fixture.apply(&["--update", "--force"]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "force must not suppress {fault}: {output:?}"
        );
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            diagnostic.contains(match fault {
                "marker" => "marker",
                "anchor" => "anchor",
                _ => "path",
            }),
            "{fault}: {diagnostic}"
        );
        assert_eq!(
            state(&fixture.target),
            before,
            "{fault} error writes nothing"
        );
    }
}
