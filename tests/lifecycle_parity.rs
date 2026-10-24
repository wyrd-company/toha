// ---
// relationships:
//   verifies: architecture
// ---
//! Same semantic requests through public sessions and the command adapter.
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use toha::{
    Id, RawAnswer, RawAnswers, Template,
    context::{
        EnvironmentDecision, ExecutionFacts, FixedEnvironment, FixedEnvironmentSource, HostFacts,
    },
    operations::{Preparation, Session, SessionState},
    resolve::Paths,
    staging::{Store, canonical_target},
};
struct Environment;
impl FixedEnvironmentSource for Environment {
    fn capture(&mut self) -> FixedEnvironment {
        FixedEnvironment::new(None, None, None, None, None)
    }
}
fn raw(id: &str, value: Value) -> RawAnswers {
    [(Id::parse(id).unwrap(), RawAnswer(value))]
        .into_iter()
        .collect()
}
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Example User")
        .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
        .env("GIT_COMMITTER_NAME", "Example User")
        .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
struct Fixture {
    root: tempfile::TempDir,
    target: PathBuf,
    source: PathBuf,
    paths: Paths,
}
impl Fixture {
    fn new(body: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let source = root.path().join("source");
        std::fs::create_dir_all(source.join("template")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("template.yml"), body).unwrap();
        std::fs::write(source.join("template/result.txt"), "{{ label }}\n").unwrap();
        git(&target, &["init", "-q"]);
        std::fs::write(target.join("README.md"), "operator\n").unwrap();
        git(&target, &["add", "."]);
        git(&target, &["commit", "-qm", "fixture"]);
        let base = root.path();
        let paths = Paths {
            system_config: "/etc/toha/config.yml".into(),
            user_config: base.join("config/toha/config.yml"),
            local_config_override: Some(base.join("config/local.yml")),
            system_data: "/usr/local/share/toha".into(),
            user_data: base.join("data/toha"),
            cache: base.join("cache/toha"),
            state: base.join("toha/staged"),
            home: base.join("home"),
        };
        Self {
            root,
            target,
            source,
            paths,
        }
    }
    fn command(&self) -> std::process::Command {
        let mut cmd = support::isolated_command(self.root.path());
        cmd.current_dir(self.root.path())
            .env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]");
        cmd
    }
    fn envelope(&self, name: &str, answers: Value) -> PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(
            &path,
            json!({"template": self.source.to_str().unwrap(), "answers": answers}).to_string(),
        )
        .unwrap();
        path
    }
    fn session(&self) -> Session {
        let target = canonical_target(&self.target).unwrap();
        let (config, registry, _) =
            toha::resolve::load_context(&self.paths, self.root.path()).unwrap();
        let resolved = toha::resolve::resolve_template(
            self.source.to_str().unwrap(),
            &config,
            &registry,
            &self.paths,
            self.root.path(),
        )
        .unwrap();
        let preparation = Preparation::new(
            Template::load(&resolved.folder).unwrap(),
            resolved,
            target,
            &[],
            &RawAnswers::new(),
        )
        .unwrap();
        let context = preparation
            .context(
                EnvironmentDecision::RequireStageGrant,
                HostFacts::capture(),
                ExecutionFacts::new(false, false),
                &mut Environment,
            )
            .unwrap();
        preparation
            .start(
                context,
                "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                config,
                Default::default(),
                &RawAnswers::new(),
                None,
            )
            .unwrap()
    }
    fn resume(&self, record: toha::staging::StagedRecord) -> Session {
        let (config, registry, _) =
            toha::resolve::load_context(&self.paths, self.root.path()).unwrap();
        Session::resume(
            record,
            canonical_target(&self.target).unwrap(),
            &self.paths,
            config,
            &registry,
            self.root.path(),
        )
        .unwrap()
    }
}
#[test]
#[cfg(target_os = "linux")]
fn fresh_library_and_cli_agree_across_reconstruction_refusal_and_apply() {
    let f = Fixture::new(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - {id: approval, type: confirm, prompt: Approve, required: true, when: \"label == 'example'\"}\n",
    );
    let mut session = f.session();
    let SessionState::Asking { document, .. } = session.state().unwrap() else {
        panic!("initial batch missing")
    };
    assert!(
        document.get("errors").is_none(),
        "shared_fresh_initial_batch_omits_empty_errors"
    );
    let target = session.target.clone();
    let store = Store::new(f.paths.state.clone());
    session.persist(&store).unwrap();
    let initial = session.record.clone();
    let expected = serde_json::to_value(&initial).unwrap();
    store.remove(&target).unwrap();
    let staged = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(4), "{staged:?}");
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        expected,
        "shared_fresh_initial_record"
    );
    let invalid = session.answer(raw("label", json!(42))).unwrap();
    let SessionState::Asking { rejections, .. } = invalid else {
        panic!("invalid accepted")
    };
    assert_eq!(rejections[0].id.as_str(), "label");
    let invalid_file = f.envelope("invalid.json", json!({"label":42}));
    let rejected = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(invalid_file)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(4));
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        expected,
        "shared_fresh_rejection_unchanged"
    );
    assert!(matches!(
        session.answer(raw("label", json!("example"))).unwrap(),
        SessionState::Asking { .. }
    ));
    let first = f.envelope("first.json", json!({"label":"example"}));
    let continued = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(first)
        .output()
        .unwrap();
    assert_eq!(continued.status.code(), Some(4), "{continued:?}");
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&session.record).unwrap(),
        "shared_fresh_accepted_record"
    );
    session = f.resume(session.record);
    let SessionState::Completed(done) = session.answer(raw("approval", json!(true))).unwrap()
    else {
        panic!("reconstructed answer incomplete")
    };
    let second = f.envelope("second.json", json!({"approval":true}));
    let completed = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(second)
        .output()
        .unwrap();
    assert_eq!(completed.status.code(), Some(0), "{completed:?}");
    let saved = store.load(&target).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(&session.record).unwrap(),
        "shared_fresh_reconstructed_record"
    );
    let refused = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg("missing-answers.json")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    let refusal = String::from_utf8_lossy(&refused.stderr);
    assert!(
        refusal.contains("complete") && !refusal.contains("No such file"),
        "shared_finished_input_nonread: {refusal}"
    );
    assert!(matches!(
        f.resume(saved.clone()).state().unwrap(),
        SessionState::Completed(_)
    ));
    session
        .apply(
            &done,
            toha::ApplyOptions {
                force: false,
                trusted: true,
            },
            false,
            &toha::hook::ProcessRunner,
            Some(&store),
        )
        .unwrap();
    let library_record = std::fs::read(f.target.join(".toha.json")).unwrap();
    let library_file = std::fs::read(f.target.join("result.txt")).unwrap();
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_fresh_apply_consumes_stage"
    );
    git(&f.target, &["reset", "--hard", "HEAD"]);
    git(&f.target, &["clean", "-fd"]);
    store.save(&target, &saved).unwrap();
    let applied = f.command().arg("apply").arg(&f.target).output().unwrap();
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read(f.target.join("result.txt")).unwrap(),
        library_file,
        "shared_fresh_applied_bytes"
    );
    assert_eq!(
        std::fs::read(f.target.join(".toha.json")).unwrap(),
        library_record,
        "shared_fresh_applied_record"
    );
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_fresh_cli_consumes_stage"
    );
}
#[test]
#[cfg(target_os = "linux")]
fn accepted_abort_has_same_cleanup_and_no_saved_submission() {
    let f = Fixture::new(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - flow: abort\n    when: label == 'example'\n",
    );
    let mut session = f.session();
    let store = Store::new(f.paths.state.clone());
    session.persist(&store).unwrap();
    let before = session.record.clone();
    let state = session.answer(raw("label", json!("example"))).unwrap();
    assert!(matches!(state, SessionState::Ended(ended) if ended.kind() == toha::EndKind::Abort));
    assert_eq!(
        session.record.submissions.len(),
        before.submissions.len(),
        "shared_abort_has_no_submission"
    );
    session.persist(&store).unwrap();
    assert!(
        store.load(&session.target).unwrap().is_none(),
        "shared_abort_removes_stage"
    );
    store.save(&session.target, &before).unwrap();
    let answers = f.envelope("abort.json", json!({"label":"example"}));
    let output = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        store.load(&session.target).unwrap().is_none(),
        "shared_cli_abort_removes_stage"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn update_library_and_cli_agree_on_replay_trust_retry_and_merge() {
    use toha::updates::{self, PrepareMode, UpdateBase, UpdateSession};
    let f = Fixture::new(
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n",
    );
    let part_question = "  - {id: features, type: multiselect, parts: true, prompt: Features, options: [core, optional], default: [core, optional]}\n";
    let part_files = "files:\n  - each: \"['one'] as item\"\n    source: optional.txt\n    path: optional.txt\n    when: \"'optional' in features\"\n";
    std::fs::write(f.source.join("template.yml"), format!("name: sample\ninterview:\n  - {{id: label, type: text, prompt: Label, required: true}}\n{part_question}{part_files}")).unwrap();
    std::fs::write(f.source.join("optional.txt"), "optional\n").unwrap();
    let address = support::versioned_template(&f.source);
    let answers = f.root.path().join("baseline.json");
    std::fs::write(
        &answers,
        json!({"template": address, "answers":{"label":"example", "features":["core", "optional"]}}).to_string(),
    )
    .unwrap();
    let baseline = f
        .command()
        .arg("apply")
        .arg(&address)
        .arg(&f.target)
        .arg("--baseline")
        .arg("--answers")
        .arg(answers)
        .output()
        .unwrap();
    assert!(baseline.status.success(), "{baseline:?}");
    git(&f.target, &["add", "."]);
    git(&f.target, &["commit", "-qm", "baseline"]);
    std::fs::write(f.source.join("template.yml"), format!("name: sample\ninterview:\n  - {{id: label, type: text, prompt: Label, required: true}}\n  - {{id: color, type: text, prompt: Color, required: true}}\n{part_question}  - flow: abort\n    when: color == 'cancel'\n{part_files}hooks:\n  - run: [git, --version]\n")).unwrap();
    std::fs::write(f.source.join("template/color.txt"), "{{ color }}\n").unwrap();
    let address = support::refresh_template(&address);
    let target = canonical_target(&f.target).unwrap();
    let store = Store::new(f.paths.state.clone());
    let (config, registry, _) = toha::resolve::load_context(&f.paths, f.root.path()).unwrap();
    let mut source = Environment;
    let mut environment = updates::Environment {
        config: &config,
        registry: &registry,
        cwd: f.root.path(),
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        host: HostFacts::capture(),
        is_admin: false,
        source: &mut source,
    };
    let prep = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target,
        &f.paths,
        false,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap();
    let mut session = UpdateSession::start(
        prep,
        target.clone(),
        UpdateBase::Update,
        false,
        toha::parts::Selection {
            with: vec![],
            without: vec!["optional".into()],
            base: None,
        },
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    let initial = session.record.clone();
    let staged = f
        .command()
        .arg("stage")
        .arg(&address)
        .arg(&f.target)
        .arg("--update")
        .arg("--async")
        .arg("--without")
        .arg("optional")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(4), "{staged:?}");
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&initial).unwrap(),
        "shared_update_initial_record"
    );
    store.save(&target, &initial).unwrap();
    let abort_answers = f.root.path().join("abort-update.json");
    std::fs::write(
        &abort_answers,
        json!({"template":address,"answers":{"color":"cancel"}}).to_string(),
    )
    .unwrap();
    let abort = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(abort_answers)
        .output()
        .unwrap();
    assert_eq!(abort.status.code(), Some(1));
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_cli_update_abort_consumes_stage"
    );
    store.save(&target, &initial).unwrap();
    let mut abort_environment = updates::Environment {
        config: &config,
        registry: &registry,
        cwd: f.root.path(),
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        host: HostFacts::capture(),
        is_admin: false,
        source: &mut Environment,
    };
    let mut aborted = UpdateSession::resume(
        initial.clone(),
        target.clone(),
        &f.paths,
        false,
        &mut abort_environment,
    )
    .unwrap();
    assert!(
        matches!(aborted.answer(raw("color", json!("cancel"))).unwrap(), SessionState::Ended(ended) if ended.kind() == toha::EndKind::Abort)
    );
    assert_eq!(
        aborted.record.submissions, initial.submissions,
        "shared_update_abort_has_no_submission"
    );
    aborted.persist(&store).unwrap();
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_update_abort_consumes_stage"
    );
    store.save(&target, &initial).unwrap();
    let SessionState::Completed(_) = session.answer(raw("color", json!("blue"))).unwrap() else {
        panic!("update answer incomplete")
    };
    session.persist(&store).unwrap();
    let completed_record = session.record.clone();
    store.save(&target, &initial).unwrap();
    let answers = f.root.path().join("color.json");
    std::fs::write(
        &answers,
        json!({"template": address,"answers":{"color":"blue"}}).to_string(),
    )
    .unwrap();
    let continued = f
        .command()
        .arg("continue")
        .arg(&f.target)
        .arg(answers)
        .output()
        .unwrap();
    assert_eq!(continued.status.code(), Some(0), "{continued:?}");
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&completed_record).unwrap(),
        "shared_update_accepted_record"
    );
    let before = std::fs::read(f.target.join(".toha.json")).unwrap();
    let refusal = UpdateSession::resume(
        completed_record.clone(),
        target.clone(),
        &f.paths,
        false,
        &mut environment,
    )
    .unwrap();
    let SessionState::Completed(done) = refusal.state().unwrap() else {
        panic!("resume incomplete")
    };
    let result = refusal
        .apply(&done, false, &toha::hook::ProcessRunner, Some(&store))
        .unwrap();
    assert!(
        matches!(result.0, toha::applied::Merged::NeedsTrust),
        "shared_library_update_needs_trust"
    );
    assert!(
        store.load(&target).unwrap().is_some(),
        "shared_library_update_needs_trust_retains_stage"
    );
    assert_eq!(std::fs::read(f.target.join(".toha.json")).unwrap(), before);
    let refusal = f.command().arg("apply").arg(&f.target).output().unwrap();
    assert_eq!(refusal.status.code(), Some(0), "{refusal:?}");
    assert!(String::from_utf8_lossy(&refusal.stdout).contains("Hooks need trust"));
    assert_eq!(
        serde_json::to_value(store.load(&target).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&completed_record).unwrap(),
        "shared_cli_update_needs_trust_retains_stage"
    );
    assert_eq!(std::fs::read(f.target.join(".toha.json")).unwrap(), before);
    let admitted = UpdateSession::resume(
        completed_record.clone(),
        target.clone(),
        &f.paths,
        true,
        &mut environment,
    )
    .unwrap();
    let SessionState::Completed(done) = admitted.state().unwrap() else {
        panic!("retry incomplete")
    };
    assert!(matches!(
        admitted
            .apply(&done, false, &toha::hook::ProcessRunner, Some(&store))
            .unwrap()
            .0,
        toha::applied::Merged::Written { .. }
    ));
    let library_record = std::fs::read(f.target.join(".toha.json")).unwrap();
    let retracted: Value = serde_json::from_slice(&library_record).unwrap();
    assert_eq!(
        retracted["applications"][0]["parts"],
        json!(["core"]),
        "shared_update_retracted_parts"
    );
    assert!(
        retracted["applications"][0]["paths"]
            .get("optional.txt")
            .is_none(),
        "shared_update_retracted_ownership"
    );
    assert!(
        !f.target.join("optional.txt").exists(),
        "shared_update_retracted_file"
    );
    let library_file = std::fs::read(f.target.join("color.txt")).unwrap();
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_library_update_consumes_stage"
    );
    git(&f.target, &["reset", "--hard", "HEAD"]);
    git(&f.target, &["clean", "-fd"]);
    store.save(&target, &completed_record).unwrap();
    let applied = f
        .command()
        .arg("apply")
        .arg(&f.target)
        .arg("--trust")
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read(f.target.join(".toha.json")).unwrap(),
        library_record,
        "shared_update_applied_record"
    );
    assert_eq!(
        std::fs::read(f.target.join("color.txt")).unwrap(),
        library_file,
        "shared_update_applied_bytes"
    );
    assert!(
        store.load(&target).unwrap().is_none(),
        "shared_cli_update_consumes_stage"
    );
    // The old completed stage is stale after the accepted update. Its pin must
    // refuse before a newly broken config, clock, or answers path is acquired.
    git(&f.target, &["add", "."]);
    git(&f.target, &["commit", "-qm", "accepted update"]);
    let current = f
        .command()
        .arg("stage")
        .arg(&address)
        .arg(&f.target)
        .arg("--update")
        .arg("--trust")
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(current.status.code(), Some(0), "{current:?}");
    let preview = f
        .command()
        .arg("apply")
        .arg(&f.target)
        .arg("--dry-run")
        .arg("--trust")
        .output()
        .unwrap();
    assert_eq!(preview.status.code(), Some(0), "{preview:?}");
    assert!(
        String::from_utf8_lossy(&preview.stdout).contains("Already current"),
        "cli_already_current_preview_disposition: {preview:?}"
    );
    assert!(
        store.load(&target).unwrap().is_some(),
        "cli_already_current_preview_retains_stage"
    );
    let current = f
        .command()
        .arg("apply")
        .arg(&f.target)
        .arg("--trust")
        .output()
        .unwrap();
    assert_eq!(current.status.code(), Some(0), "{current:?}");
    assert!(
        store.load(&target).unwrap().is_none(),
        "cli_already_current_real_consumes_stage"
    );
    store.save(&target, &completed_record).unwrap();
    assert!(
        matches!(
            updates::admit_resume(&completed_record, &UpdateBase::Update, &target),
            Err(updates::UpdateError::StaleApplication)
        ),
        "shared_library_stale_before_host_inputs"
    );
    std::fs::create_dir_all(f.paths.user_config.parent().unwrap()).unwrap();
    std::fs::write(&f.paths.user_config, "invalid: [").unwrap();
    let stale = f
        .command()
        .env("TOHA_NOW", "invalid-clock")
        .arg("continue")
        .arg(&f.target)
        .arg("missing-answers.json")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&stale.stderr);
    assert_eq!(stale.status.code(), Some(1));
    assert!(
        text.contains("applied record entry changed")
            && !text.contains("invalid-clock")
            && !text.contains("config.yml"),
        "shared_cli_stale_before_host_inputs: {text}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn warning_and_environment_refusal_are_visible_to_library_and_cli() {
    use toha::updates::{self, PrepareMode, UpdateBase, UpdateError};
    let f = Fixture::new("name: sample\ninterview:\n  - {id: label, type: text, prompt: Label}\n");
    std::fs::write(
        f.source.join("template/result.txt"),
        "{{ toha_env_user }}\n",
    )
    .unwrap();
    std::fs::create_dir_all(f.paths.user_config.parent().unwrap()).unwrap();
    std::fs::write(
        &f.paths.user_config,
        format!(
            "template-defaults:\n  {:?}:\n    removed_question: unused\n",
            f.source.to_str().unwrap()
        ),
    )
    .unwrap();
    let (config, registry, _) = toha::resolve::load_context(&f.paths, f.root.path()).unwrap();
    let target = canonical_target(&f.target).unwrap();
    let mut source = Environment;
    let mut environment = updates::Environment {
        config: &config,
        registry: &registry,
        cwd: f.root.path(),
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        host: HostFacts::capture(),
        is_admin: false,
        source: &mut source,
    };
    let failure = updates::prepare(
        &UpdateBase::Baseline,
        f.source.to_str().unwrap(),
        &target,
        &f.paths,
        false,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap_err();
    assert!(
        std::error::Error::source(&failure).is_some(),
        "update_refusal_source_chain"
    );
    let UpdateError::WithWarnings { warnings, source } = failure else {
        panic!("warning lost on refusal")
    };
    assert_eq!(warnings.len(), 1, "shared_library_warning_on_refusal");
    assert!(warnings[0].contains("removed_question"));
    assert!(source.to_string().contains("trust"));
    let refused = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--baseline")
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let text = String::from_utf8_lossy(&refused.stderr);
    assert!(
        text.contains("warning:") && text.contains("removed_question") && text.contains("trust"),
        "shared_cli_warning_on_refusal: {text}"
    );
    assert!(
        Store::new(f.paths.state.clone())
            .load(&target)
            .unwrap()
            .is_none()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn fresh_admission_checks_requirements_and_documents_before_environment() {
    let f = Fixture::new(
        "name: sample\nrequires: [fixture-absent-program]\ninterview:\n  - {id: label, type: text, prompt: Label}\n",
    );
    std::fs::write(
        f.source.join("template/result.txt"),
        "{{ toha_env_user }}\n",
    )
    .unwrap();
    let (config, registry, _) = toha::resolve::load_context(&f.paths, f.root.path()).unwrap();
    let resolved = toha::resolve::resolve_template(
        f.source.to_str().unwrap(),
        &config,
        &registry,
        &f.paths,
        f.root.path(),
    )
    .unwrap();
    let missing = Preparation::new(
        Template::load(&resolved.folder).unwrap(),
        resolved,
        canonical_target(&f.target).unwrap(),
        &[f.root.path().join("missing.json")],
        &RawAnswers::new(),
    )
    .unwrap_err();
    assert!(
        matches!(missing, toha::operations::OperationError::Requirements(_)),
        "shared_library_requirements_before_document_and_environment"
    );
    let refused = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--async")
        .arg("--input")
        .arg(f.root.path().join("missing.json"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&refused.stderr);
    assert!(
        text.contains("fixture-absent-program") && !text.contains("requires stage trust"),
        "shared_cli_requirements_before_document_and_environment: {text}"
    );
    std::fs::write(
        f.source.join("template.yml"),
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label}\n",
    )
    .unwrap();
    let resolved = toha::resolve::resolve_template(
        f.source.to_str().unwrap(),
        &config,
        &registry,
        &f.paths,
        f.root.path(),
    )
    .unwrap();
    let document = Preparation::new(
        Template::load(&resolved.folder).unwrap(),
        resolved,
        canonical_target(&f.target).unwrap(),
        &[f.root.path().join("missing.json")],
        &RawAnswers::new(),
    );
    assert!(
        matches!(document, Err(toha::operations::OperationError::Document(_))),
        "shared_library_document_before_environment"
    );
    let refused = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--async")
        .arg("--input")
        .arg(f.root.path().join("missing.json"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&refused.stderr);
    assert!(
        text.contains("missing.json") && !text.contains("requires stage trust"),
        "shared_cli_document_before_environment: {text}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn initial_stop_creates_no_saved_progress_for_either_driver() {
    let f = Fixture::new("name: sample\ninterview:\n  - flow: stop\n");
    let session = f.session();
    let store = Store::new(f.paths.state.clone());
    assert!(matches!(session.state().unwrap(), SessionState::Ended(_)));
    session.persist(&store).unwrap();
    assert!(
        store.load(&session.target).unwrap().is_none(),
        "shared_initial_stop_has_no_stage"
    );
    let stopped = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    assert!(
        store.load(&session.target).unwrap().is_none(),
        "shared_cli_initial_stop_has_no_stage"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn cli_warning_survives_environment_refusal() {
    let f = Fixture::new("name: sample\ninterview:\n  - {id: label, type: text, prompt: Label}\n");
    std::fs::write(
        f.source.join("template/result.txt"),
        "{{ toha_env_user }}\n",
    )
    .unwrap();
    std::fs::create_dir_all(f.paths.user_config.parent().unwrap()).unwrap();
    std::fs::write(
        &f.paths.user_config,
        format!(
            "template-defaults:\n  {:?}:\n    removed_question: unused\n",
            f.source.to_str().unwrap()
        ),
    )
    .unwrap();
    let output = f
        .command()
        .arg("stage")
        .arg(&f.source)
        .arg(&f.target)
        .arg("--baseline")
        .arg("--async")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text.contains("removed_question") && text.contains("warning:") && text.contains("trust"),
        "cli_warning_survives_refusal: {text}"
    );
}
