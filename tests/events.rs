// ---
// relationships:
//   verifies: template-format
// ---
#[allow(dead_code)]
mod support;
use std::{collections::BTreeMap, path::Path};
use toha::{
    Template,
    applied::{Application, AppliedRecord, FrozenNow, RepoPath, Revision},
    hook::{HookError, HookOutcome, HookRunner},
};

fn application(target: &str) -> Application {
    Application::new(
        "example".into(),
        Revision::Unversioned,
        RepoPath::parse(target).unwrap(),
        FrozenNow::parse("2026-01-01T00:00:00Z[UTC]").unwrap(),
        BTreeMap::from([("label".into(), serde_json::json!("sample"))]),
        BTreeMap::new(),
        vec![],
        vec![],
    )
}
fn fixture() -> Template {
    Template::load(Path::new("tests/fixtures/events/template")).unwrap()
}
#[test]
fn prepares_only_matching_handlers_in_the_recorded_target_context() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("component")).unwrap();
    let template = fixture();
    let handlers =
        toha::events::prepare(&template, &application("component"), "refresh", root.path())
            .unwrap();
    assert_eq!(handlers.len(), 1);
    assert_eq!(
        handlers[0].target,
        root.path().join("component").canonicalize().unwrap()
    );
    assert_eq!(handlers[0].hook.argv(), ["tool", "changed", "sample"]);
    assert!(
        toha::events::prepare(&template, &application("."), "unknown", root.path())
            .unwrap()
            .is_empty()
    );
}
struct Fail(std::cell::Cell<usize>);
impl HookRunner for Fail {
    fn run(&self, _: &toha::PlannedHook, _: &Path) -> Result<HookOutcome, HookError> {
        self.0.set(self.0.get() + 1);
        Ok(HookOutcome {
            success: false,
            code: Some(2),
            stdout: None,
            stderr: None,
        })
    }
}
#[test]
fn dispatch_stops_at_the_first_failure() {
    let root = tempfile::tempdir().unwrap();
    let template = fixture();
    let mut handlers =
        toha::events::prepare(&template, &application("."), "refresh", root.path()).unwrap();
    handlers.extend(
        toha::events::prepare(&template, &application("."), "refresh", root.path()).unwrap(),
    );
    let runner = Fail(std::cell::Cell::new(0));
    assert!(
        toha::events::dispatch(&handlers, &runner)
            .unwrap_err()
            .contains("Some(2)")
    );
    assert_eq!(runner.0.get(), 1);
}
#[test]
fn event_declarations_and_script_bytes_are_reviewed() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("template")).unwrap();
    std::fs::write(root.path().join("handler"), "first").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("handler"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let manifest = "name: example\nevents:\n  refresh:\n    - script: handler\n";
    std::fs::write(root.path().join("template.yml"), manifest).unwrap();
    let digest = || {
        toha::HookSurface::of(&Template::load(root.path()).unwrap())
            .unwrap()
            .digest()
    };
    let first = digest();
    std::fs::write(root.path().join("handler"), "second").unwrap();
    let second = digest();
    assert_ne!(first, second);
    std::fs::write(
        root.path().join("template.yml"),
        manifest.replace("refresh", "changed"),
    )
    .unwrap();
    assert_ne!(second, digest());
}
#[test]
fn cli_discovers_record_from_child_and_preflights_trust_before_running() {
    let iso = tempfile::tempdir().unwrap();
    let repo = iso.path().join("repo");
    let template = iso.path().join("source");
    std::fs::create_dir_all(repo.join("child")).unwrap();
    std::fs::create_dir_all(template.join("template")).unwrap();
    std::fs::write(
        template.join("template.yml"),
        "name: example\nevents:\n  empty: []\n  skipped:\n    - run: [definitely-missing-event-tool]\n      when: 'false'\n  refresh:\n    - run: [definitely-missing-event-tool]\n",
    )
    .unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    let mut record = AppliedRecord::default();
    let mut app = application(".");
    // A folder source is read from its recorded formal identity.
    app = Application::new(
        template.to_str().unwrap().into(),
        Revision::Unversioned,
        app.target().clone(),
        app.generated().clone(),
        app.answers.clone(),
        app.context.clone(),
        vec![],
        vec![],
    );
    record.replace(app);
    std::fs::write(repo.join(".toha.json"), record.to_json_bytes().unwrap()).unwrap();
    let run = |flags: &[&str]| {
        support::isolated_command(iso.path())
            .arg("event")
            .arg("refresh")
            .arg(repo.join("child"))
            .args(flags)
            .output()
            .unwrap()
    };
    for event in ["empty", "skipped", "unknown"] {
        let result = support::isolated_command(iso.path())
            .args(["event", event])
            .arg(repo.join("child"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("0 handler(s) executed"));
    }
    let dry = run(&["--dry-run"]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(String::from_utf8_lossy(&dry.stdout).contains("1 handler(s) planned"));
    let refused = run(&[]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--trust"));
    let trusted = run(&["--trust"]);
    assert!(!trusted.status.success());
    assert!(!String::from_utf8_lossy(&trusted.stderr).contains("require trust"));
}
#[cfg(unix)]
#[test]
fn refuses_symlink_escape_from_application_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("component")).unwrap();
    assert!(
        toha::events::prepare(
            &fixture(),
            &application("component"),
            "refresh",
            root.path()
        )
        .unwrap_err()
        .contains("escapes")
    );
}

#[cfg(unix)]
#[test]
fn child_event_updates_parent_file_with_recorded_answers() {
    use std::os::unix::fs::PermissionsExt;
    let iso = tempfile::tempdir().unwrap();
    let repo = iso.path().join("repo");
    let source = iso.path().join("source");
    std::fs::create_dir_all(repo.join("child")).unwrap();
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::write(source.join("template.yml"), "name: example\ninterview:\n  - id: label\n    type: text\n    prompt: Label\nevents:\n  refresh:\n    - script: refresh.sh\n      args: ['{{ label }}']\n").unwrap();
    std::fs::write(
        source.join("refresh.sh"),
        "#!/bin/sh\nprintf '%s' \"$1\" > index.txt\n",
    )
    .unwrap();
    std::fs::set_permissions(
        source.join("refresh.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    let mut record = AppliedRecord::default();
    for target in [".", "child"] {
        let app = application(target);
        record.replace(Application::new(
            source.to_str().unwrap().into(),
            Revision::Unversioned,
            app.target().clone(),
            app.generated().clone(),
            app.answers.clone(),
            app.context.clone(),
            vec![],
            vec![],
        ));
    }
    std::fs::write(repo.join(".toha.json"), record.to_json_bytes().unwrap()).unwrap();
    let run = |flags: &[&str]| {
        support::isolated_command(iso.path())
            .args(["event", "refresh"])
            .arg(repo.join("child"))
            .args(flags)
            .output()
            .unwrap()
    };
    assert!(run(&["--dry-run"]).status.success());
    assert!(!repo.join("index.txt").exists());
    let mut invalid_record = record.clone();
    let app = application("missing");
    invalid_record.replace(Application::new(
        source.to_str().unwrap().into(),
        Revision::Unversioned,
        app.target().clone(),
        app.generated().clone(),
        app.answers.clone(),
        app.context.clone(),
        vec![],
        vec![],
    ));
    std::fs::write(
        repo.join(".toha.json"),
        invalid_record.to_json_bytes().unwrap(),
    )
    .unwrap();
    assert!(!run(&["--trust"]).status.success());
    assert!(
        !repo.join("index.txt").exists(),
        "a later preparation error must prevent earlier scripts"
    );
    assert!(!repo.join("child/index.txt").exists());
    std::fs::write(repo.join(".toha.json"), record.to_json_bytes().unwrap()).unwrap();
    let result = run(&["--trust"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("index.txt")).unwrap(),
        "sample"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("child/index.txt")).unwrap(),
        "sample"
    );
    assert_eq!(
        std::fs::read(repo.join(".toha.json")).unwrap(),
        record.to_json_bytes().unwrap()
    );
}

#[test]
fn installed_approval_is_used_and_revocation_or_changed_surface_refuses_dispatch() {
    let iso = tempfile::tempdir().unwrap();
    let repo = iso.path().join("repo");
    let source = iso.path().join("source");
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        source.join("template.yml"),
        "name: example\nevents:\n  refresh:\n    - run: [git, init, -q, marker]\n",
    )
    .unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    let mut record = AppliedRecord::default();
    let app = application(".");
    record.replace(Application::new(
        source.to_str().unwrap().into(),
        Revision::Unversioned,
        app.target().clone(),
        app.generated().clone(),
        app.answers.clone(),
        app.context.clone(),
        vec![],
        vec![],
    ));
    std::fs::write(repo.join(".toha.json"), record.to_json_bytes().unwrap()).unwrap();
    let run = |args: &[&str]| {
        support::isolated_command(iso.path())
            .args(args)
            .output()
            .unwrap()
    };
    let ok = |out: std::process::Output| {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        )
    };
    ok(run(&[
        "templates",
        "add",
        source.to_str().unwrap(),
        "--alias",
        "sample",
    ]));
    ok(run(&["templates", "trust", "sample"]));
    ok(run(&["event", "refresh", repo.to_str().unwrap()]));
    assert!(repo.join("marker/.git").exists());
    std::fs::remove_dir_all(repo.join("marker")).unwrap();
    ok(run(&["templates", "untrust", "sample"]));
    let refused = run(&["event", "refresh", repo.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("require trust"));
    assert!(!repo.join("marker").exists());
    ok(run(&[
        "event",
        "refresh",
        repo.to_str().unwrap(),
        "--trust",
    ]));
    std::fs::remove_dir_all(repo.join("marker")).unwrap();
    ok(run(&["templates", "trust", "sample"]));
    std::fs::write(
        source.join("template.yml"),
        "name: example\nevents:\n  refresh:\n    - run: [git, init, -q, changed]\n",
    )
    .unwrap();
    let refused = run(&["event", "refresh", repo.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("require trust"));
    assert!(!repo.join("changed").exists());
}

#[cfg(unix)]
#[test]
fn apply_captures_event_only_context_for_each_application_target() {
    let iso = tempfile::tempdir().unwrap();
    let repo = iso.path().join("repo");
    let source = iso.path().join("source");
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::create_dir_all(repo.join("child")).unwrap();
    std::fs::write(source.join("template.yml"), "name: example\nevents:\n  refresh:\n    - run: [sh, '-c', 'printf \"%s\\n\" \"$@\" > observed.txt', event, '{{ toha_target_name }}', '{{ toha_template_name }}', '{{ toha_template_formal_name }}', '{{ toha_host_os }}']\n").unwrap();
    std::fs::write(source.join("template/keep.txt"), "kept\n").unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "seed",
    ]);
    let answers = iso.path().join("answers.json");
    std::fs::write(
        &answers,
        serde_json::json!({"template": source, "answers": {}}).to_string(),
    )
    .unwrap();
    for target in [repo.clone(), repo.join("child")] {
        let out = support::isolated_command(iso.path())
            .arg("apply")
            .arg(&source)
            .arg(&target)
            .arg("--baseline")
            .arg("--answers")
            .arg(&answers)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "apply",
        ]);
    }
    let record = AppliedRecord::parse(&std::fs::read(repo.join(".toha.json")).unwrap()).unwrap();
    for app in record.applications() {
        assert!(
            app.context.contains_key("toha_target_name"),
            "{:?}",
            app.context
        );
        assert!(app.context.contains_key("toha_host_os"));
    }
    let out = support::isolated_command(iso.path())
        .args(["event", "refresh"])
        .arg(repo.join("child"))
        .arg("--trust")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (path, name) in [(&repo, "repo"), (&repo.join("child"), "child")] {
        let lines = std::fs::read_to_string(path.join("observed.txt")).unwrap();
        assert_eq!(
            lines.lines().collect::<Vec<_>>(),
            [
                name,
                "example",
                source.to_str().unwrap(),
                std::env::consts::OS
            ]
        );
    }
}
