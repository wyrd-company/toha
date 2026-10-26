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
        Self::with_marker("#")
    }
    fn with_marker(marker: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let iso = root.path().join("isolated");
        let template = root.path().join("template");
        let target = root.path().join("project");
        fs::create_dir_all(template.join("template")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(template.join("template/.keep"), "").unwrap();
        fs::write(
            template.join("template.yml"),
            format!("{}    marker: '{marker}'\n", Self::manifest("alpha = 1")),
        )
        .unwrap();
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
    staged_drift_retry(false);
}

#[test]
fn staged_region_removal_refusal_retains_a_usable_stage_for_forced_retry() {
    staged_drift_retry(true);
}

fn staged_drift_retry(removal: bool) {
    let fixture = Fixture::new();
    fixture.drift();
    if removal {
        transition_plan(&fixture, false);
    } else {
        fixture.unrelated_template_change();
    }
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
        (content.contains("alpha = 1") == !removal)
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
    let path = fixture.target.join("config.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("keep = 1", "keep = 2"),
    )
    .unwrap();
    fixture.commit("outside edit");
    // An undrifted prior region and matching outside bytes permit takeover.
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

fn transition_plan(fixture: &Fixture, takeover: bool) {
    if takeover {
        fs::write(
            fixture.template.join("template/config.toml"),
            "[application]\n\n\n[other]\nkeep = 2\n",
        )
        .unwrap();
    } else {
        fs::write(
            fixture.template.join("template.yml"),
            "name: cfg\nignore: [.keep]\n",
        )
        .unwrap();
    }
    fixture.unrelated_template_change();
}

fn assert_previous_region_refusal(takeover: bool) {
    let fixture = Fixture::new();
    fixture.drift();
    transition_plan(&fixture, takeover);
    let before = state(&fixture.target);
    let output = fixture.apply(&["--update"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "previous-region drift must refuse before {}: {output:?}",
        if takeover {
            "whole-file takeover"
        } else {
            "retraction"
        }
    );
    conflict(&output);
    assert_eq!(
        state(&fixture.target),
        before,
        "previous-region refusal must be atomic"
    );
    conflict(&fixture.apply(&["--update", "--dry-run"]));
    assert_eq!(state(&fixture.target), before);
    let preview = fixture.apply(&["--update", "--force", "--dry-run"]);
    assert!(
        preview.status.success(),
        "transition forced preview: {preview:?}"
    );
    assert_eq!(
        state(&fixture.target),
        before,
        "forced preview keeps all project state"
    );
    conflict(&fixture.apply(&["--update"]));
    let applied = fixture.apply(&["--update", "--force"]);
    assert!(
        applied.status.success(),
        "transition forced apply: {applied:?}"
    );
    let content = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        content.contains("keep = 2"),
        "outside edit survives: {content}"
    );
    assert!(
        !content.contains("alpha = 7"),
        "authorized region transition: {content}"
    );
    assert_eq!(
        content.contains("alpha = 1"),
        takeover,
        "region replaced or retracted: {content}"
    );
    assert!(fixture.target.join("new.txt").exists());
}

#[test]
fn previous_region_removal_drift_refuses_before_retraction() {
    assert_previous_region_refusal(false);
}

#[test]
fn previous_region_takeover_drift_refuses_before_whole_file_replacement() {
    assert_previous_region_refusal(true);
}

#[test]
fn whole_file_takeover_refuses_different_outside_bytes_even_with_region_force() {
    for drifted in [false, true] {
        let fixture = Fixture::new();
        if drifted {
            fixture.drift();
        } else {
            let path = fixture.target.join("config.toml");
            fs::write(
                &path,
                fs::read_to_string(&path)
                    .unwrap()
                    .replace("keep = 1", "keep = 2"),
            )
            .unwrap();
            fixture.commit("outside edit");
        }
        transition_plan(&fixture, true);
        fs::write(
            fixture.template.join("template/config.toml"),
            "[application]\n\n\n[other]\nkeep = 1\n",
        )
        .unwrap();
        let before = state(&fixture.target);
        for flags in [
            &["--update"][..],
            &["--update", "--force"][..],
            &["--update", "--force", "--dry-run"][..],
        ] {
            let output = fixture.apply(flags);
            assert_eq!(
                output.status.code(),
                Some(1),
                "takeover must refuse outside loss: {output:?}"
            );
            assert_eq!(
                support::first_document(&output.stdout)["kind"],
                "conflict",
                "{output:?}"
            );
            assert_eq!(
                state(&fixture.target),
                before,
                "outside conflict keeps operator bytes and state"
            );
        }
    }
}

#[test]
fn removed_custom_marker_region_uses_its_recorded_revision_style() {
    let fixture = Fixture::with_marker("# managed");
    fixture.drift();
    transition_plan(&fixture, false);
    let before = state(&fixture.target);
    conflict(&fixture.apply(&["--update"]));
    assert_eq!(state(&fixture.target), before);
    let applied = fixture.apply(&["--update", "--force"]);
    assert!(applied.status.success(), "{applied:?}");
    let text = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        !text.contains("alpha") && !text.contains("toha:"),
        "custom block must retract: {text}"
    );
    assert!(text.contains("keep = 2"), "{text}");
}

#[test]
fn removal_of_one_of_several_regions_protects_the_removed_region() {
    let fixture = Fixture::new();
    let manifest = format!(
        "{}  - into: config.toml\n    region: extra\n    content: |\n      beta = 1\n",
        Fixture::manifest("alpha = 1")
    );
    fs::write(fixture.template.join("template.yml"), &manifest).unwrap();
    let applied = fixture.apply(&[]);
    assert!(applied.status.success(), "{applied:?}");
    fixture.commit("two regions");
    let path = fixture.target.join("config.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("beta = 1", "beta = 7")
            .replace("keep = 1", "keep = 2"),
    )
    .unwrap();
    fixture.commit("edit removed region");
    fs::write(
        fixture.template.join("template.yml"),
        Fixture::manifest("alpha = 1"),
    )
    .unwrap();
    fixture.unrelated_template_change();
    let before = state(&fixture.target);
    let refused = fixture.apply(&["--update"]);
    conflict(&refused);
    assert!(String::from_utf8_lossy(&refused.stdout).contains("extra"));
    assert_eq!(state(&fixture.target), before);
    let applied = fixture.apply(&["--update", "--force"]);
    assert!(applied.status.success(), "{applied:?}");
    let content = fs::read_to_string(path).unwrap();
    assert!(
        content.contains("alpha = 1") && content.contains("keep = 2") && !content.contains("beta"),
        "{content}"
    );
}

#[test]
fn undrifted_region_retraction_preserves_outside_edits() {
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
    transition_plan(&fixture, false);
    let before = state(&fixture.target);
    let preview = fixture.apply(&["--update", "--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(state(&fixture.target), before);
    let output = fixture.apply(&["--update"]);
    assert!(output.status.success(), "{output:?}");
    let text = fs::read_to_string(path).unwrap();
    assert!(
        text.contains("keep = 2") && !text.contains("alpha"),
        "{text}"
    );
}

#[test]
fn removed_region_malformed_markers_still_refuse_with_force() {
    let fixture = Fixture::new();
    let path = fixture.target.join("config.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("# <<< toha:end", "# broken toha:end"),
    )
    .unwrap();
    fixture.commit("malformed prior markers");
    transition_plan(&fixture, false);
    let before = state(&fixture.target);
    let output = fixture.apply(&["--update", "--force"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let document = support::first_document(&output.stdout);
    assert_eq!(document["kind"], "render", "{document}");
    assert!(
        document["message"].as_str().unwrap().contains("malformed"),
        "{document}"
    );
    assert_eq!(state(&fixture.target), before);
}

#[test]
fn json_edit_to_whole_takeover_keeps_unowned_values_and_converges_owned_values() {
    let fixture = Fixture::new();
    let path = fixture.target.join("settings.json");
    let original = b"{\"owned\":1,\"keep\":2}\n";
    fs::write(&path, original).unwrap();
    fixture.commit("operator settings");
    fs::write(
        fixture.template.join("template.yml"),
        format!(
            "{}  - into: settings.json\n    struct: {{path: owned, value: 1}}\n",
            Fixture::manifest("alpha = 1")
        ),
    )
    .unwrap();
    let output = fixture.apply(&[]);
    assert!(output.status.success(), "{output:?}");
    fixture.commit("own a JSON value");
    fs::write(
        fixture.template.join("template/settings.json"),
        "{\"owned\":3,\"keep\":1}\n",
    )
    .unwrap();
    fixture.unrelated_template_change();
    let before = state(&fixture.target);
    for flags in [&["--update"][..], &["--update", "--force"][..]] {
        let output = fixture.apply(flags);
        assert_eq!(
            output.status.code(),
            Some(1),
            "JSON takeover outside conflict: {output:?}"
        );
        assert_eq!(support::first_document(&output.stdout)["kind"], "conflict");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(state(&fixture.target), before);
    }
    fs::write(
        fixture.template.join("template/settings.json"),
        "{\"owned\":3,\"keep\":2}\n",
    )
    .unwrap();
    // The old JSON injection still converges its declared value on this takeover.
    let output = fixture.apply(&["--update"]);
    assert!(
        output.status.success(),
        "matching JSON outside bytes admit: {output:?}"
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["keep"], 2);
    assert_eq!(value["owned"], 1);
}

#[test]
fn whole_file_takeover_without_new_injections_keeps_the_owned_region_boundary() {
    let fixture = Fixture::new();
    fixture.drift();
    transition_plan(&fixture, false);
    fs::write(
        fixture.template.join("template/config.toml"),
        "[application]\n\n\n[other]\nkeep = 1\n",
    )
    .unwrap();
    let before = state(&fixture.target);
    conflict(&fixture.apply(&["--update"]));
    let outside = fixture.apply(&["--update", "--force"]);
    assert_eq!(
        outside.status.code(),
        Some(1),
        "plain takeover must protect outside bytes: {outside:?}"
    );
    assert_eq!(state(&fixture.target), before);
    fs::write(
        fixture.template.join("template/config.toml"),
        "[application]\n\n\n[other]\nkeep = 2\n",
    )
    .unwrap();
    let preview = fixture.apply(&["--update", "--force", "--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(state(&fixture.target), before);
    let applied = fixture.apply(&["--update", "--force"]);
    assert!(applied.status.success(), "{applied:?}");
    let content = fs::read_to_string(fixture.target.join("config.toml")).unwrap();
    assert!(
        !content.contains("alpha") && content.contains("keep = 2"),
        "{content}"
    );
}

#[test]
fn json_takeover_compares_outside_array_indices_numerically() {
    let fixture = Fixture::new();
    let path = fixture.target.join("settings.json");
    let original = "{\"items\":[0,1,2,3,4,5,6,7,8,9,10,11]}\n";
    fs::write(&path, original).unwrap();
    fixture.commit("operator array");
    let old = format!(
        "{}  - into: settings.json\n    struct: {{path: 'items[2]', value: 2}}\n  - into: settings.json\n    struct: {{path: 'items[10]', value: 10}}\n",
        Fixture::manifest("alpha = 1")
    );
    fs::write(fixture.template.join("template.yml"), &old).unwrap();
    assert!(fixture.apply(&[]).status.success());
    fixture.commit("own two array values");
    fs::write(
        fixture.template.join("template/settings.json"),
        "{\"items\":[0,1,2,3,4,5,6,7,8,9,10,111]}\n",
    )
    .unwrap();
    fixture.unrelated_template_change();
    let before = state(&fixture.target);
    let output = fixture.apply(&["--update", "--force"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "numeric outside index must remain protected: {output:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(state(&fixture.target), before);
    fs::write(fixture.template.join("template/settings.json"), original).unwrap();
    fs::write(
        fixture.template.join("template.yml"),
        old.replace("value: 2}", "value: 20}")
            .replace("value: 10}", "value: 100}"),
    )
    .unwrap();
    let output = fixture.apply(&["--update"]);
    assert!(
        output.status.success(),
        "matching outside array image admits: {output:?}"
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["items"][2], 20);
    assert_eq!(value["items"][10], 100);
    assert_eq!(value["items"][11], 11);
}
