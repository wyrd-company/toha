// ---
// relationships:
//   verifies:
//     - template-composition
//     - architecture
// ---
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};
use toha::{
    Applied, ApplyOptions, Id, RawAnswer, RawAnswers, Template,
    context::{
        EnvironmentDecision, ExecutionFacts, FixedEnvironment, FixedEnvironmentSource, HostFacts,
    },
    hook::{HookError, HookOutcome, HookRunner},
    operations::{ApplicationResult, Session, SessionState},
    resolve::{Paths, resolve_template},
    staging::{StagedRecord, Store, canonical_target},
    updates::{self, PrepareMode, UpdateBase, UpdateSession},
};
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
}
fn write(root: &Path, path: &str, body: impl AsRef<[u8]>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}
fn paths(root: &Path) -> Paths {
    Paths {
        system_config: root.join("system/config.yml"),
        user_config: root.join("user/config.yml"),
        local_config_override: None,
        system_data: root.join("system/data"),
        user_data: root.join("user/data"),
        cache: root.join("cache"),
        state: root.join("state"),
        home: root.join("home"),
    }
}
struct NoEnvironment;
impl FixedEnvironmentSource for NoEnvironment {
    fn capture(&mut self) -> FixedEnvironment {
        FixedEnvironment::new(None, None, None, None, None)
    }
}
struct Runner {
    calls: RefCell<Vec<Vec<String>>>,
}
struct FailingRunner;
impl HookRunner for FailingRunner {
    fn run(&self, _: &toha::PlannedHook, _: &Path) -> Result<HookOutcome, HookError> {
        Err(HookError::Io(std::io::Error::other(
            "fixture runner failure",
        )))
    }
}
impl HookRunner for Runner {
    fn run(&self, hook: &toha::PlannedHook, target: &Path) -> Result<HookOutcome, HookError> {
        assert!(target.is_absolute());
        assert_eq!(hook.argv(), vec!["git", "--version"]);
        assert!(hook.cwd.is_none());
        self.calls.borrow_mut().push(hook.argv());
        Ok(HookOutcome {
            success: true,
            code: Some(0),
            stdout: None,
            stderr: None,
        })
    }
}
fn raw(key: &str, value: Value) -> RawAnswers {
    [(Id::parse(key).unwrap(), RawAnswer(value))]
        .into_iter()
        .collect()
}
fn done(state: SessionState) -> toha::Completed {
    match state {
        SessionState::Completed(done) => done,
        _ => panic!("interview did not complete"),
    }
}
fn template(root: &Path, second: bool) {
    let extra = if second {
        "  - {id: new_answer, type: text, prompt: New answer, required: true}\n  - flow: abort\n    when: new_answer == 'cancel'\n"
    } else {
        ""
    };
    write(
        root,
        "template.yml",
        format!(
            "name: sample\nschemas: schemas\ninterview:\n  - {{id: label, type: text, prompt: Label, required: true}}\n  - {{id: confirmation, type: confirm, prompt: Confirm, required: true, when: \"label == 'sample'\"}}\n  - {{id: features, type: multiselect, parts: true, prompt: Features, options: [core, optional], default: [core]}}\n  - {{id: catalog, type: document, prompt: Catalog, schema: 'https://schemas.example.test/catalog/1', input: catalog.json}}\n{extra}files:\n  - each: \"['one'] as item\"\n    source: optional.txt\n    path: optional.txt\n    when: \"'optional' in features\"\nhooks:\n  - run: [git, --version]\n"
        ),
    );
    write(root,"schemas/catalog.json",json!({"$id":"https://schemas.example.test/catalog/1","type":"object","required":["title"],"properties":{"title":{"type":"string"}}}).to_string());
    write(root, "optional.txt", "optional\n");
    write(
        root,
        "template/result.txt",
        format!(
            "label: {{{{ label }}}}\nfirst: {}\nsecond: stable\nthird: stable\nfourth: stable\nfifth: {}\ndocument: {{{{ catalog.title }}}}\nos: {{{{ toha_host_os }}}}\n",
            if second { "template" } else { "original" },
            if second { "new" } else { "original" }
        ),
    );
}
fn update_environment<'a>(
    config: &'a toha::config::Config,
    registry: &'a toha::registry::Registry,
    cwd: &'a Path,
    source: &'a mut NoEnvironment,
) -> updates::Environment<'a, NoEnvironment> {
    updates::Environment {
        config,
        registry,
        cwd,
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        host: HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
        is_admin: false,
        source,
    }
}
/// Set when the credential resolver is asked. Every remote in this consumer is
/// local, and only HTTPS remotes consult the resolver.
static RESOLVER_ASKED: AtomicBool = AtomicBool::new(false);

fn main() {
    let resolver = std::sync::Arc::new(|_: &toha::source::CredentialRequest| {
        RESOLVER_ASKED.store(true, Ordering::Relaxed);
        Ok(None)
    });
    toha::source::with_credential_resolver(resolver, run);
    assert!(!RESOLVER_ASKED.load(Ordering::Relaxed));
    println!(
        "external consumer passed: replay, Send, document refresh, runner, preview, update, parts, pins and credentials"
    );
}

fn run() {
    update_null_replay_contract();
    update_start_replay_contract();
    update_start_end_contract();
    fn assert_send<T: Send>() {}
    assert_send::<Template>();
    assert_send::<Session>();
    assert_send::<StagedRecord>();
    assert_send::<SessionState>();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    let target = root.join("output");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    template(&source, false);
    commit(&source);
    let v1 = git(&source, &["rev-parse", "HEAD"]);
    git(&target, &["init", "-q", "-b", "main"]);
    write(
        &target,
        "catalog.json",
        json!({"$schema":"https://schemas.example.test/catalog/1","title":"first"}).to_string(),
    );
    commit(&target);
    let paths = paths(&root);
    let (config, registry, warnings) = toha::resolve::load_context(&paths, &root).unwrap();
    assert!(warnings.is_empty());
    let address = format!("file://{}", source.display());
    let resolved = resolve_template(&address, &config, &registry, &paths, &root).unwrap();
    assert_eq!(resolved.commit, v1);
    assert!(
        matches!(
            resolve_template("not-installed", &config, &registry, &paths, &root),
            Err(toha::resolve::ResolveError::NotFound(_))
        ),
        "missing source lost its type"
    );
    let loaded = Template::load(&resolved.folder).unwrap();
    let target_id = canonical_target(&target).unwrap();
    let mut env_source = NoEnvironment;
    let context = toha::operations::invocation_context(
        &target_id,
        &resolved,
        &loaded,
        EnvironmentDecision::GrantIfNeeded,
        HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
        ExecutionFacts::new(false, false),
        &mut env_source,
    )
    .unwrap();
    let invalid_doc = root.join("invalid-document.json");
    write(
        &root,
        "invalid-document.json",
        json!({"$schema":"https://schemas.example.test/catalog/1","title":42}).to_string(),
    );
    let bad = Session::start(
        Template::load(&resolved.folder).unwrap(),
        resolved.clone(),
        target_id.clone(),
        context.clone(),
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        config.clone(),
        Default::default(),
        &[invalid_doc],
        &RawAnswers::new(),
        None,
    );
    assert!(
        matches!(bad, Err(toha::operations::OperationError::Document(_))),
        "invalid document accepted"
    );
    assert!(
        !target.join("result.txt").exists(),
        "invalid document wrote target"
    );
    let wrong_target = canonical_target(&root.join("wrong-target")).unwrap();
    assert!(
        matches!(
            Session::start(
                Template::load(&resolved.folder).unwrap(),
                resolved.clone(),
                wrong_target,
                context.clone(),
                "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                config.clone(),
                Default::default(),
                &[],
                &RawAnswers::new(),
                None
            ),
            Err(toha::operations::OperationError::Input(_))
        ),
        "fresh context mismatch admitted"
    );
    let mut other_resolved = resolved.clone();
    other_resolved.formal_name = "different-template".into();
    let other_context = toha::operations::invocation_context(
        &target_id,
        &other_resolved,
        &loaded,
        EnvironmentDecision::GrantIfNeeded,
        HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
        ExecutionFacts::new(false, false),
        &mut env_source,
    )
    .unwrap();
    assert!(
        matches!(
            Session::start(
                Template::load(&resolved.folder).unwrap(),
                resolved.clone(),
                target_id.clone(),
                other_context,
                "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                config.clone(),
                Default::default(),
                &[],
                &RawAnswers::new(),
                None
            ),
            Err(toha::operations::OperationError::Input(_))
        ),
        "fresh formal mismatch admitted"
    );
    let mut session = Session::start(
        loaded,
        resolved,
        target_id.clone(),
        context,
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        config.clone(),
        toha::parts::Selection {
            with: vec!["optional".into()],
            without: vec![],
            base: None,
        },
        &[],
        &RawAnswers::new(),
        None,
    )
    .unwrap();
    match session.state().unwrap() {
        SessionState::Asking { document, .. } => assert!(
            document.get("errors").is_none(),
            "consumer_fresh_initial_batch_omits_empty_errors"
        ),
        _ => panic!("initial question missing"),
    }
    let invalid_parts = Session::start(
        Template::load(&session.resolved.folder).unwrap(),
        session.resolved.clone(),
        target_id.clone(),
        session.record.invocation_context(&target_id).unwrap(),
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        config.clone(),
        toha::parts::Selection {
            with: vec!["missing".into()],
            without: vec![],
            base: None,
        },
        &[],
        &RawAnswers::new(),
        None,
    );
    assert!(
        matches!(
            invalid_parts,
            Err(toha::operations::OperationError::Parts(_))
        ),
        "consumer_parts_admission_has_domain_error"
    );
    let record_before = serde_json::to_vec(&session.record).unwrap();
    let rejected = session.answer(raw("label", json!(42))).unwrap();
    match rejected {
        SessionState::Asking { rejections, .. } => assert_eq!(rejections[0].id.as_str(), "label"),
        _ => panic!("invalid answer accepted"),
    };
    assert_eq!(
        serde_json::to_vec(&session.record).unwrap(),
        record_before,
        "consumer_rejected_fresh_record_unchanged"
    );
    let accepted = session.answer(raw("label", json!("sample"))).unwrap();
    assert!(
        matches!(accepted, SessionState::Asking { .. }),
        "first_request_requires_followup"
    );
    let wire = serde_json::to_vec(&session.record).unwrap();
    drop(session);
    let mut session = Session::resume(
        serde_json::from_slice(&wire).unwrap(),
        target_id.clone(),
        &paths,
        config.clone(),
        &registry,
        &root,
    )
    .unwrap();
    let mut bad_seed = session.record.clone();
    bad_seed.seed = Some("missing-copy".into());
    assert!(
        matches!(
            toha::operations::pinned_seed(&bad_seed, &address),
            Err(toha::operations::SeedError::MissingCopy)
        ),
        "missing seed copy accepted"
    );
    let SessionState::Completed(completed) =
        session.answer(raw("confirmation", json!(true))).unwrap()
    else {
        panic!("consumer_reconstructed_answer_completes");
    };
    assert_eq!(
        session
            .record
            .submissions
            .iter()
            .filter(|batch| batch.contains_key("label"))
            .count(),
        1,
        "reconstructed_label_recorded_once"
    );
    assert_eq!(
        session
            .record
            .submissions
            .iter()
            .filter(|batch| batch.contains_key("confirmation"))
            .count(),
        1,
        "reconstructed_followup_recorded_once"
    );
    let other_target = canonical_target(&root.join("other-output")).unwrap();
    assert!(
        matches!(
            toha::operations::build_plan(
                &session.template,
                &completed,
                &other_target,
                &session.resolved.formal_name,
                &session.resolved.commit
            ),
            Err(toha::operations::OperationError::Input(_))
        ),
        "completion target mismatch was accepted"
    );
    let store = Store::new(paths.state.clone());
    store.save(&target_id, &session.record).unwrap();
    let runner = Runner {
        calls: RefCell::new(vec![]),
    };
    match session
        .apply(
            &completed,
            ApplyOptions {
                force: false,
                trusted: false,
            },
            false,
            &runner,
            Some(&store),
        )
        .unwrap()
    {
        ApplicationResult::Applied(Applied::NeedsTrust(_)) => {}
        _ => panic!("untrusted hook was admitted"),
    };
    assert!(
        store.load(&target_id).unwrap().is_some(),
        "consumer_fresh_trust_retains_stage"
    );
    assert!(!target.join("result.txt").exists());
    assert!(matches!(
        session.apply(
            &completed,
            ApplyOptions {
                force: false,
                trusted: true
            },
            false,
            &FailingRunner,
            Some(&store)
        ),
        Err(toha::operations::OperationError::Apply(_))
    ));
    assert!(
        store.load(&target_id).unwrap().is_some(),
        "consumer_fresh_failure_retains_stage"
    );
    assert!(!target.join(".toha.json").exists());
    match session
        .apply(
            &completed,
            ApplyOptions {
                force: true,
                trusted: true,
            },
            false,
            &runner,
            Some(&store),
        )
        .unwrap()
    {
        ApplicationResult::Applied(Applied::Written { .. }) => {}
        _ => panic!("not applied"),
    };
    assert!(
        store.load(&target_id).unwrap().is_none(),
        "consumer_fresh_apply_consumes_stage"
    );
    assert_eq!(runner.calls.borrow().len(), 1);
    assert!(target.join("optional.txt").is_file());
    assert!(target.join(".toha.json").is_file());
    let applied: Value =
        serde_json::from_slice(&std::fs::read(target.join(".toha.json")).unwrap()).unwrap();
    let application = &applied["applications"][0];
    assert_eq!(
        application["commit"], v1,
        "generation revision not recorded"
    );
    assert_eq!(application["answers"]["label"], "sample");
    assert_eq!(application["answers"]["catalog"]["title"], "first");
    assert_eq!(application["parts"], json!(["core", "optional"]));
    assert_eq!(application["context"]["toha_host_os"], "linux");
    assert!(application["paths"]["result.txt"]["blob"].is_string());
    assert!(application["paths"]["optional.txt"]["blob"].is_string());
    let mut seed_record = session.record.clone();
    seed_record.submissions.clear();
    seed_record.seed = Some(".".into());
    let mut seed_entry = application.clone();
    seed_entry["answers"]["label"] = json!(42);
    seed_record.seed_entry = Some(seed_entry);
    let seeded = Session::resume(
        seed_record,
        target_id.clone(),
        &paths,
        config.clone(),
        &registry,
        &root,
    )
    .unwrap();
    let seed_warnings = seeded.seed_warnings().unwrap();
    assert!(
        seed_warnings
            .iter()
            .any(|warning| warning.contains("label") && warning.contains("does not match")),
        "consumer_wrong_kind_seed_warning"
    );
    let generated = application["generated"].clone();
    commit(&target);
    let clean_record = std::fs::read(target.join(".toha.json")).unwrap();
    let mut dirty_record = clean_record.clone();
    dirty_record.push(b'\n');
    std::fs::write(target.join(".toha.json"), &dirty_record).unwrap();
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    assert!(
        matches!(
            updates::prepare(
                &UpdateBase::Update,
                &address,
                &target_id,
                &paths,
                true,
                PrepareMode::Apply,
                false,
                &mut environment,
            ),
            Err(updates::UpdateError::Project(_))
        ),
        "dirty record admitted"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        dirty_record,
        "dirty record refusal wrote record"
    );
    std::fs::write(target.join(".toha.json"), &clean_record).unwrap();
    let old = std::fs::read_to_string(target.join("result.txt")).unwrap();
    write(
        &target,
        "result.txt",
        old.replace("second: stable", "second: operator"),
    );
    write(
        &target,
        "catalog.json",
        json!({"$schema":"https://schemas.example.test/catalog/1","title":"fresh"}).to_string(),
    );
    let before_record = std::fs::read(target.join(".toha.json")).unwrap();
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    assert!(
        matches!(
            updates::prepare(
                &UpdateBase::Update,
                &address,
                &target_id,
                &paths,
                true,
                PrepareMode::Apply,
                false,
                &mut environment,
            ),
            Err(updates::UpdateError::DirtyTarget)
        ),
        "dirty target admitted"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        before_record,
        "dirty refusal changed record"
    );
    commit(&target);
    template(&source, true);
    commit(&source);
    let v2 = git(&source, &["rev-parse", "HEAD"]);
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let prepared = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap();
    assert_eq!(prepared.resolved.commit, v2);
    assert!(
        matches!(
            UpdateSession::start(
                prepared,
                canonical_target(&root.join("different-output")).unwrap(),
                UpdateBase::Update,
                false,
                Default::default(),
                &[],
                &RawAnswers::new()
            ),
            Err(updates::SessionError::Operation(
                toha::operations::OperationError::Input(_)
            ))
        ),
        "prepared update target mismatch admitted"
    );
    let prepared = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap();
    let mut upgrade = UpdateSession::start(
        prepared,
        target_id.clone(),
        UpdateBase::Update,
        false,
        Default::default(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    match upgrade.state().unwrap() {
        SessionState::Asking { document, .. } => {
            assert!(
                document.get("errors").is_none(),
                "consumer_update_initial_batch_omits_empty_errors"
            );
            let properties = document["schema"]["properties"].as_object().unwrap();
            assert_eq!(
                properties.len(),
                1,
                "consumer_update_projects_only_new_question"
            );
            assert!(properties.contains_key("new_answer"));
        }
        _ => panic!("new question not asked"),
    }
    let before_invalid = serde_json::to_vec(&upgrade.record).unwrap();
    match upgrade.answer(raw("new_answer", json!(42))).unwrap() {
        SessionState::Asking {
            document,
            rejections,
        } => {
            assert_eq!(
                document["schema"]["properties"].as_object().unwrap().len(),
                1,
                "rejection exposed covered answers"
            );
            assert_eq!(rejections[0].id.as_str(), "new_answer");
        }
        _ => panic!("invalid update answer accepted"),
    }
    assert_eq!(
        serde_json::to_vec(&upgrade.record).unwrap(),
        before_invalid,
        "rejected update changed record"
    );
    let mut abort_environment = update_environment(&config, &registry, &root, &mut env_source);
    let mut aborted = UpdateSession::resume(
        upgrade.record.clone(),
        target_id.clone(),
        &paths,
        true,
        &mut abort_environment,
    )
    .unwrap();
    let original_submissions = aborted.record.submissions.clone();
    store.save(&target_id, &aborted.record).unwrap();
    assert!(
        matches!(aborted.answer(raw("new_answer", json!("cancel"))).unwrap(), SessionState::Ended(ended) if ended.kind() == toha::EndKind::Abort),
        "consumer_update_abort_rebuild_restores_documents"
    );
    assert_eq!(
        aborted.record.submissions, original_submissions,
        "consumer_update_abort_has_no_submission"
    );
    aborted.persist(&store).unwrap();
    assert!(
        store.load(&target_id).unwrap().is_none(),
        "consumer_update_abort_consumes_stage"
    );
    let _ = done(upgrade.answer(raw("new_answer", json!("new"))).unwrap());
    let wire = serde_json::to_vec(&upgrade.record).unwrap();
    drop(upgrade);
    // A changed default after admission must not replace this update session's document.
    write(
        &target,
        "catalog.json",
        json!({"$schema":"https://schemas.example.test/catalog/1","title":"later"}).to_string(),
    );
    commit(&target);
    let request_record: StagedRecord = serde_json::from_slice(&wire).unwrap();
    let returned = request_in_thread(
        request_record.clone(),
        target_id.clone(),
        paths.clone(),
        config.clone(),
        registry.clone(),
        root.clone(),
    )
    .join()
    .unwrap()
    .unwrap();
    let SessionState::Completed(returned) = returned else {
        panic!("worker_restores_completed_document_state");
    };
    assert_eq!(
        returned.now.to_string(),
        request_record.now,
        "worker replaced generated time"
    );
    let reserialized = StagedRecord::new_with_context(
        returned.context().unwrap().clone(),
        request_record.commit.clone(),
        request_record.named,
        returned.now.to_string(),
        vec![],
    );
    assert_eq!(
        serde_json::to_value(&reserialized).unwrap()["context"],
        serde_json::to_value(&request_record).unwrap()["context"],
        "worker replaced pinned context"
    );
    assert_eq!(
        returned.accepted_raw()[&Id::parse("catalog").unwrap()].0["title"],
        "fresh",
        "worker replaced admitted document"
    );
    let mut stale = request_record;
    let mut application: Value = serde_json::from_str(stale.base.as_deref().unwrap()).unwrap();
    application["answers"]["label"] = json!("different");
    stale.base = Some(application.to_string());
    let stale_result = request_in_thread(
        stale,
        target_id.clone(),
        paths.clone(),
        config.clone(),
        registry.clone(),
        root.clone(),
    )
    .join()
    .unwrap();
    assert!(
        matches!(
            stale_result,
            Err(updates::SessionError::Update(
                updates::UpdateError::StaleApplication
            ))
        ),
        "stale application was not returned across the worker boundary"
    );
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let upgrade = UpdateSession::resume(
        serde_json::from_slice(&wire).unwrap(),
        target_id.clone(),
        &paths,
        true,
        &mut environment,
    )
    .unwrap();
    let completed = done(upgrade.state().unwrap());
    store.save(&target_id, &upgrade.record).unwrap();
    let before_record = std::fs::read(target.join(".toha.json")).unwrap();
    let before_file = std::fs::read(target.join("result.txt")).unwrap();
    for mutation in 0..3 {
        let mut environment = update_environment(&config, &registry, &root, &mut env_source);
        let mut prepared = updates::prepare(
            &UpdateBase::Update,
            &address,
            &target_id,
            &paths,
            true,
            PrepareMode::Apply,
            false,
            &mut environment,
        )
        .unwrap();
        let mut mismatched = upgrade.record.clone();
        match mutation {
            0 => {
                prepared.context = toha::context::InvocationContext::for_target(
                    canonical_target(&root.join("other-request")).unwrap(),
                )
            }
            1 => mismatched.template = "another-template".into(),
            _ => mismatched.commit = "another-revision".into(),
        }
        assert!(
            matches!(
                UpdateSession::from_prepared(prepared, mismatched, target_id.clone()),
                Err(updates::SessionError::Operation(
                    toha::operations::OperationError::Input(_)
                ))
            ),
            "prepared_record_identity_conjunct_{mutation}"
        );
    }
    let mut untrusted_environment = update_environment(&config, &registry, &root, &mut env_source);
    let untrusted = UpdateSession::resume(
        upgrade.record.clone(),
        target_id.clone(),
        &paths,
        false,
        &mut untrusted_environment,
    )
    .unwrap();
    let untrusted_completed = done(untrusted.state().unwrap());
    assert!(
        matches!(
            untrusted
                .apply(&untrusted_completed, false, &runner, Some(&store))
                .unwrap()
                .0,
            toha::applied::Merged::NeedsTrust
        ),
        "consumer_update_requires_trust"
    );
    assert!(
        store.load(&target_id).unwrap().is_some(),
        "consumer_update_needs_trust_retains_stage"
    );
    let mut check_environment = update_environment(&config, &registry, &root, &mut env_source);
    let check_prepared = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Apply,
        false,
        &mut check_environment,
    )
    .unwrap();
    assert!(
        matches!(
            check_prepared.finish(
                &canonical_target(&root.join("different-output")).unwrap(),
                &completed,
                true,
                &runner
            ),
            Err(toha::applied::MergeError::WrongTarget)
        ),
        "prepared finish target mismatch admitted"
    );
    let preview = upgrade
        .apply(&completed, true, &runner, Some(&store))
        .unwrap();
    assert!(
        matches!(preview.0, toha::applied::Merged::Planned { .. }),
        "consumer_update_preview_disposition"
    );
    assert_eq!(
        std::fs::read(target.join("result.txt")).unwrap(),
        before_file
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        before_record
    );
    assert!(
        store.load(&target_id).unwrap().is_some(),
        "consumer_update_preview_retains_stage"
    );
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let failed = UpdateSession::resume(
        serde_json::from_slice(&wire).unwrap(),
        target_id.clone(),
        &paths,
        true,
        &mut environment,
    )
    .unwrap();
    let completed = done(failed.state().unwrap());
    assert!(
        matches!(
            failed.apply(&completed, false, &FailingRunner, Some(&store)),
            Err(updates::SessionError::Merge(
                toha::applied::MergeError::Apply(_)
            ))
        ),
        "update hook failure lost its type"
    );
    assert_eq!(
        std::fs::read(target.join("result.txt")).unwrap(),
        before_file,
        "failed update wrote target"
    );
    assert_eq!(
        std::fs::read(target.join(".toha.json")).unwrap(),
        before_record,
        "failed update wrote record"
    );
    assert!(
        store.load(&target_id).unwrap().is_some(),
        "failed update removed staged state"
    );
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let upgrade = UpdateSession::resume(
        serde_json::from_slice(&wire).unwrap(),
        target_id.clone(),
        &paths,
        true,
        &mut environment,
    )
    .unwrap();
    let completed = done(upgrade.state().unwrap());
    upgrade
        .apply(&completed, false, &runner, Some(&store))
        .unwrap();
    let updated = std::fs::read_to_string(target.join("result.txt")).unwrap();
    assert!(updated.contains("second: operator"));
    assert!(updated.contains("fifth: new"));
    assert!(updated.contains("document: fresh"));
    assert!(
        updated.contains("os: linux"),
        "worker host replaced context"
    );
    let applied: Value =
        serde_json::from_slice(&std::fs::read(target.join(".toha.json")).unwrap()).unwrap();
    assert_eq!(
        applied["applications"][0]["commit"], v2,
        "update revision not recorded"
    );
    assert_eq!(
        applied["applications"][0]["generated"], generated,
        "update replaced generated instant"
    );
    assert_eq!(
        applied["applications"][0]["answers"]["catalog"]["title"],
        "fresh"
    );
    assert_eq!(
        runner.calls.borrow().len(),
        2,
        "update bypassed the caller's hook runner"
    );
    assert!(
        store.load(&target_id).unwrap().is_none(),
        "consumer_update_apply_consumes_stage"
    );
    commit(&target);
    {
        let mut environment = update_environment(&config, &registry, &root, &mut env_source);
        let prepared = updates::prepare(
            &UpdateBase::Update,
            &address,
            &target_id,
            &paths,
            true,
            PrepareMode::Stage,
            false,
            &mut environment,
        )
        .unwrap();
        let current = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Update,
            false,
            Default::default(),
            &[],
            &raw(
                "catalog",
                json!({"$schema":"https://schemas.example.test/catalog/1", "title":"fresh"}),
            ),
        )
        .unwrap();
        let current_record = current.record.clone();
        let completed = done(current.state().unwrap());
        store.save(&target_id, &current_record).unwrap();
        let current_runner = Runner {
            calls: RefCell::new(vec![]),
        };
        assert!(
            matches!(
                current
                    .apply(&completed, true, &current_runner, Some(&store))
                    .unwrap()
                    .0,
                toha::applied::Merged::AlreadyCurrent
            ),
            "consumer_already_current_preview_disposition"
        );
        assert!(
            store.load(&target_id).unwrap().is_some(),
            "consumer_already_current_preview_retains_stage"
        );
        let mut environment = update_environment(&config, &registry, &root, &mut env_source);
        let current = UpdateSession::resume(
            current_record,
            target_id.clone(),
            &paths,
            true,
            &mut environment,
        )
        .unwrap();
        let completed = done(current.state().unwrap());
        assert!(matches!(
            current
                .apply(&completed, false, &current_runner, Some(&store))
                .unwrap()
                .0,
            toha::applied::Merged::AlreadyCurrent
        ));
        assert!(
            store.load(&target_id).unwrap().is_none(),
            "consumer_already_current_real_consumes_stage"
        );
    }
    {
        let mut environment = update_environment(&config, &registry, &root, &mut env_source);
        let prepared = updates::prepare(
            &UpdateBase::Update,
            &address,
            &target_id,
            &paths,
            true,
            PrepareMode::Stage,
            false,
            &mut environment,
        )
        .unwrap();
        let reanswer = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Update,
            true,
            Default::default(),
            &[],
            &RawAnswers::new(),
        )
        .unwrap();
        match reanswer.state().unwrap() {
            SessionState::Asking { document, .. } => assert!(
                document["schema"]["properties"]["label"].is_object(),
                "reanswer hid a recorded question"
            ),
            _ => panic!("reanswer did not ask"),
        }
    }
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let prepared = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap();
    let upgrade = UpdateSession::start(
        prepared,
        target_id.clone(),
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
    let completed = done(upgrade.state().unwrap());
    upgrade.apply(&completed, false, &runner, None).unwrap();
    assert!(!target.join("optional.txt").exists());
    let retracted: Value =
        serde_json::from_slice(&std::fs::read(target.join(".toha.json")).unwrap()).unwrap();
    let application = &retracted["applications"][0];
    assert_eq!(
        application["parts"],
        json!(["core"]),
        "retraction_record_parts"
    );
    assert!(
        application["paths"].get("optional.txt").is_none(),
        "retraction_record_ownership"
    );
    let old_resolved =
        toha::resolve::resume_template(&address, &v1, false, &config, &registry, &paths, &root)
            .unwrap();
    assert_eq!(old_resolved.commit, v1);
    assert!(
        std::fs::read_to_string(old_resolved.folder.join("template.yml"))
            .unwrap()
            .find("new_answer")
            .is_none()
    );
    commit(&target);
    let before_constraint = std::fs::read_to_string(source.join("template.yml")).unwrap();
    write(
        &source,
        "template.yml",
        before_constraint.replace(
            "prompt: Label, required: true",
            "prompt: Label, required: true, validate: {regex: '^X'}",
        ),
    );
    commit(&source);
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let prepared = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap();
    let invalid_recorded = UpdateSession::start(
        prepared,
        target_id.clone(),
        UpdateBase::Update,
        false,
        Default::default(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    match invalid_recorded.state().unwrap() {
        SessionState::Asking {
            document,
            rejections,
        } => {
            assert!(
                document["schema"]["properties"]["label"].is_object(),
                "invalid recorded answer was hidden"
            );
            assert!(
                rejections.iter().any(|r| r.id.as_str() == "label"),
                "invalid recorded answer lost its question error"
            );
        }
        _ => panic!("invalid recorded answer completed"),
    }
    write(&source, "template.yml", before_constraint);
    commit(&source);
    // Structured conflict outcomes still commit the supported conflict-marked merge.
    commit(&target);
    let text = std::fs::read_to_string(target.join("result.txt")).unwrap();
    write(
        &target,
        "result.txt",
        text.replace("first: template", "first: operator"),
    );
    commit(&target);
    let text = std::fs::read_to_string(source.join("template/result.txt")).unwrap();
    write(
        &source,
        "template/result.txt",
        text.replace("first: template", "first: newer"),
    );
    commit(&source);
    let mut environment = update_environment(&config, &registry, &root, &mut env_source);
    let prep = updates::prepare(
        &UpdateBase::Update,
        &address,
        &target_id,
        &paths,
        true,
        PrepareMode::Apply,
        false,
        &mut environment,
    )
    .unwrap();
    let conflict = UpdateSession::start(
        prep,
        target_id.clone(),
        UpdateBase::Update,
        false,
        Default::default(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    let completed = done(conflict.state().unwrap());
    let (merged, _) = conflict.apply(&completed, false, &runner, None).unwrap();
    match merged {
        toha::applied::Merged::Written { conflicted, .. } => {
            assert!(!conflicted.is_empty(), "conflict outcome lost paths")
        }
        _ => panic!("conflict not reported"),
    }
    assert!(
        std::fs::read_to_string(target.join("result.txt"))
            .unwrap()
            .contains("<<<<<<<"),
        "conflict markers absent"
    );
    catalog_and_assets(&root, &paths, &address);
    assert!(!toha::assets::list().unwrap().is_empty());
    abort_contract();
    domain_diagnostics();
    warning_refusal_contract();
    flow_preview_contract();
}

fn catalog_and_assets(root: &Path, paths: &Paths, address: &str) {
    use toha::catalog::{self, Catalog, CatalogError, Event};
    let load = || Catalog::load(paths.clone(), root).unwrap();
    assert!(matches!(
        &catalog::add(&load(), address, Some("sample-alias".into()), false, root).unwrap()[..],
        [Event::Added(_)]
    ));
    let listing = catalog::list(&load(), None).unwrap();
    assert!(
        listing
            .entries
            .iter()
            .any(|row| row.listed.entry.aliases == ["sample-alias"])
    );
    assert!(matches!(
        &catalog::trust(&load(), "sample-alias").unwrap()[..],
        [Event::Trusted { already: false, .. }]
    ));
    assert!(matches!(
        &catalog::trust(&load(), "sample-alias").unwrap()[..],
        [Event::Trusted { already: true, .. }]
    ));
    let before = std::fs::read(paths.user_data.join("templates.yml")).unwrap();
    assert!(matches!(
        catalog::add_alias(&load(), "sample-alias", "sample-alias"),
        Err(CatalogError::AliasInUse { .. })
    ));
    assert_eq!(
        std::fs::read(paths.user_data.join("templates.yml")).unwrap(),
        before
    );
    assert!(matches!(
        &catalog::untrust(&load(), "sample-alias").unwrap()[..],
        [Event::Untrusted { already: false, .. }]
    ));
    let _ = catalog::update(&load(), Some("sample-alias".into())).unwrap();
    let _ = catalog::add_alias(&load(), "sample-alias", "second-alias").unwrap();
    let _ = catalog::remove_alias(&load(), "second-alias").unwrap();
    let _ = catalog::remove(&load(), "sample-alias").unwrap();
    assert!(
        !catalog::list(&load(), None)
            .unwrap()
            .entries
            .iter()
            .any(|row| row.listed.entry.aliases == ["sample-alias"])
    );
    let skill = toha::assets::list().unwrap().remove(0);
    assert!(
        !toha::assets::read(&skill.name, Path::new("SKILL.md"))
            .unwrap()
            .is_empty()
    );
    assert!(toha::assets::read(&skill.name, Path::new("../outside")).is_err());
    let export = root.join("export");
    toha::assets::export(&skill.name, &export).unwrap();
    let bytes = std::fs::read(export.join(&skill.name).join("SKILL.md")).unwrap();
    assert!(toha::assets::export(&skill.name, &export).is_err());
    assert_eq!(
        std::fs::read(export.join(&skill.name).join("SKILL.md")).unwrap(),
        bytes
    );
    let (config, registry, _) = toha::resolve::load_context(paths, root).unwrap();
    let demo = resolve_template("toha-demo", &config, &registry, paths, root).unwrap();
    assert_eq!(demo.commit, toha::bundled::commit());
}

fn request_in_thread(
    record: StagedRecord,
    target: toha::staging::CanonicalTarget,
    paths: Paths,
    config: toha::config::Config,
    registry: toha::registry::Registry,
    cwd: std::path::PathBuf,
) -> std::thread::JoinHandle<Result<SessionState, updates::SessionError>> {
    std::thread::spawn(move || {
        let mut source = NoEnvironment;
        let mut environment = update_environment(&config, &registry, &cwd, &mut source);
        environment.now = "2030-01-02T03:04:05+00:00[UTC]".parse().unwrap();
        environment.host =
            HostFacts::new("worker-os".into(), "worker-arch".into(), None, None, vec![]);
        let operation = UpdateSession::resume(record, target, &paths, true, &mut environment)?;
        operation.state()
    })
}

fn abort_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let source = root.join("source");
    let target_path = root.join("target");
    write(
        &source,
        "template.yml",
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label, required: true}\n  - flow: abort\n    when: label == 'example'\n",
    );
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::create_dir_all(&target_path).unwrap();
    let paths = paths(&root);
    let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
    let resolved =
        resolve_template(source.to_str().unwrap(), &config, &registry, &paths, &root).unwrap();
    let preparation = toha::operations::Preparation::new(
        Template::load(&resolved.folder).unwrap(),
        resolved,
        canonical_target(&target_path).unwrap(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    let context = preparation
        .context(
            EnvironmentDecision::RequireStageGrant,
            HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
            ExecutionFacts::new(false, false),
            &mut NoEnvironment,
        )
        .unwrap();
    let mut session = preparation
        .start(
            context,
            "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            config,
            Default::default(),
            &RawAnswers::new(),
            None,
        )
        .unwrap();
    let store = Store::new(paths.state.clone());
    session.persist(&store).unwrap();
    let previous = session.record.submissions.len();
    assert!(
        matches!(session.answer(raw("label", json!("example"))).unwrap(), SessionState::Ended(ended) if ended.kind() == toha::EndKind::Abort)
    );
    assert_eq!(
        session.record.submissions.len(),
        previous,
        "consumer_abort_has_no_submission"
    );
    session.persist(&store).unwrap();
    assert!(
        store.load(&session.target).unwrap().is_none(),
        "consumer_abort_consumes_stage"
    );
    assert!(
        matches!(session.state().unwrap(), SessionState::Ended(_)),
        "consumer_abort_remains_terminal"
    );
}

fn warning_refusal_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let source = root.join("source");
    let target = root.join("target");
    write(
        &source,
        "template.yml",
        "name: sample\ninterview:\n  - {id: label, type: text, prompt: Label}\n",
    );
    write(&source, "template/result.txt", "{{ toha_env_user }}\n");
    write(&target, "README.md", "operator\n");
    git(&target, &["init", "-q"]);
    commit(&target);
    let paths = paths(&root);
    write(
        &root,
        "user/config.yml",
        format!(
            "template-defaults:\n  {:?}:\n    removed_question: unused\n",
            source.to_str().unwrap()
        ),
    );
    let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
    let mut source_env = NoEnvironment;
    let mut environment = update_environment(&config, &registry, &root, &mut source_env);
    let failure = updates::prepare(
        &UpdateBase::Baseline,
        source.to_str().unwrap(),
        &canonical_target(&target).unwrap(),
        &paths,
        false,
        PrepareMode::Stage,
        false,
        &mut environment,
    )
    .unwrap_err();
    let updates::UpdateError::WithWarnings { warnings, source } = failure else {
        panic!("consumer_warning_refusal_is_structured");
    };
    assert_eq!(warnings.len(), 1, "consumer_warning_survives_refusal");
    assert!(
        matches!(*source, updates::UpdateError::Admission(_)),
        "consumer_admission_retains_typed_source"
    );
}

fn domain_diagnostics() {
    use toha::{catalog::CatalogError, resolve::ResolveError};
    let commit = "0123456789abcdef0123456789abcdef01234567";
    for (revision, expected) in [
        (
            toha::applied::Revision::Commit(toha::applied::CommitId::parse(commit).unwrap()),
            commit.to_string(),
        ),
        (
            toha::applied::Revision::Content(
                toha::applied::ContentDigest::parse(&"a".repeat(64)).unwrap(),
            ),
            "a".repeat(64),
        ),
        (toha::applied::Revision::Unversioned, "unversioned".into()),
    ] {
        let error = updates::UpdateError::Base {
            template: "sample".into(),
            revision,
            reason: "unavailable".into(),
        };
        assert_eq!(
            error.to_string(),
            format!("cannot render merge base for sample at {expected}: unavailable"),
            "consumer_update_base_display"
        );
    }
    let ambiguous = ResolveError::Ambiguous {
        name: "sample".into(),
        matches: vec!["first".into(), "second".into()],
    };
    assert_eq!(
        ambiguous.to_string(),
        "template sample is ambiguous: first, second",
        "consumer_resolve_display"
    );
    let alias = CatalogError::AliasInUse {
        template: "sample".into(),
        alias: "choice".into(),
        removable: true,
    };
    assert_eq!(
        alias.to_string(),
        "alias choice is already in use",
        "consumer_catalog_display"
    );
    assert_eq!(
        updates::UpdateError::Resolve(ambiguous).to_string(),
        "template sample is ambiguous: first, second",
        "consumer_update_display"
    );
    assert_eq!(
        updates::UpdateError::StaleApplication.to_string(),
        "the applied record entry changed",
        "consumer_domain_has_no_command_instructions"
    );
    assert_eq!(
        toha::operations::SeedError::MissingCopy.to_string(),
        "the staged seed has no copied application",
        "consumer_seed_display"
    );
}

fn flow_preview_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let source = root.join("source");
    let target = root.join("target");
    write(
        &source,
        "template.yml",
        "name: sample\ninterview:\n  - flow: dry-run\n",
    );
    write(&source, "template/result.txt", "preview\n");
    std::fs::create_dir_all(&target).unwrap();
    let paths = paths(&root);
    let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
    let resolved =
        resolve_template(source.to_str().unwrap(), &config, &registry, &paths, &root).unwrap();
    let preparation = toha::operations::Preparation::new(
        Template::load(&resolved.folder).unwrap(),
        resolved,
        canonical_target(&target).unwrap(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    let context = preparation
        .context(
            EnvironmentDecision::GrantIfNeeded,
            HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
            ExecutionFacts::new(false, false),
            &mut NoEnvironment,
        )
        .unwrap();
    let session = preparation
        .start(
            context,
            "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            config,
            Default::default(),
            &RawAnswers::new(),
            None,
        )
        .unwrap();
    let completed = done(session.state().unwrap());
    assert!(
        matches!(
            session
                .apply(
                    &completed,
                    ApplyOptions {
                        force: false,
                        trusted: true
                    },
                    false,
                    &FailingRunner,
                    None
                )
                .unwrap(),
            ApplicationResult::Planned(_)
        ),
        "consumer_flow_dry_run_suppresses_write"
    );
    assert!(!target.join("result.txt").exists());
}

// A direct-library request follows the same state-to-apply convention as a CLI driver.
fn update_start_end_contract() {
    for flow in ["abort", "stop"] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let source = root.join("source");
        let target = root.join("target");
        write(
            &source,
            "template.yml",
            format!(
                "name: sample\ninterview:\n  - {{id: label, type: text, prompt: Label, required: true}}\n  - flow: {flow}\n    when: label == 'cancel'\n"
            ),
        );
        write(&source, "template/result.txt", "{{ label }}\n");
        std::fs::create_dir_all(&target).unwrap();
        git(&source, &["init", "-q"]);
        commit(&source);
        git(&target, &["init", "-q"]);
        write(&target, "README.md", "operator\n");
        commit(&target);
        let paths = paths(&root);
        let address = format!("file://{}", source.display());
        let target_id = canonical_target(&target).unwrap();
        let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
        let mut source_env = NoEnvironment;
        let mut environment = update_environment(&config, &registry, &root, &mut source_env);
        let prepared = updates::Preparation::new(
            &UpdateBase::Baseline,
            &address,
            &target_id,
            &paths,
            &config,
            &registry,
            &root,
        )
        .unwrap()
        .finish(true, PrepareMode::Apply, false, &mut environment)
        .unwrap();
        let session = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Baseline,
            false,
            Default::default(),
            &[],
            &raw("label", json!("example")),
        )
        .unwrap();
        let completed = done(session.state().unwrap());
        session
            .apply(&completed, false, &FailingRunner, None)
            .unwrap();
        commit(&target);
        let record_before = std::fs::read(target.join(".toha.json")).unwrap();
        let result_before = std::fs::read(target.join("result.txt")).unwrap();
        write(&source, "template/extra.txt", "extra\n");
        commit(&source);
        let prepared = updates::prepare(
            &UpdateBase::Update,
            &address,
            &target_id,
            &paths,
            true,
            PrepareMode::Apply,
            false,
            &mut environment,
        )
        .unwrap();
        let mut session = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Update,
            false,
            Default::default(),
            &[],
            &raw("label", json!("cancel")),
        )
        .unwrap();
        let state = session.state().unwrap();
        let ended = matches!(&state, SessionState::Ended(end) if end.kind() == if flow == "abort" { toha::EndKind::Abort } else { toha::EndKind::Stop });
        let store = Store::new(paths.state.clone());
        session.persist(&store).unwrap();
        let no_stage = store.load(&target_id).unwrap().is_none();
        // Exercise the real damage path too, rather than asserting only the enum.
        if let SessionState::Completed(completed) = state {
            session
                .apply(&completed, false, &FailingRunner, None)
                .unwrap();
        }
        assert!(
            !target.join("extra.txt").exists(),
            "consumer_update_start_end_wrote_extra"
        );
        assert_eq!(
            std::fs::read(target.join("result.txt")).unwrap(),
            result_before,
            "consumer_update_start_end_target_changed"
        );
        assert_eq!(
            std::fs::read(target.join(".toha.json")).unwrap(),
            record_before,
            "consumer_update_start_end_record_changed"
        );
        assert!(no_stage, "consumer_update_start_end_has_no_stage");
        assert!(ended, "consumer_update_start_end_lost");
    }
}

/// The same raw start inputs must retain their outcome after an owned-record boundary.
fn update_start_replay_contract() {
    for new_question in [false, true] {
        for value in ["x", "changed", "abort", "stop"] {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().canonicalize().unwrap();
            let source = root.join("source");
            let target = root.join("target");
            let label =
                "  - {id: label, type: text, prompt: Label, required: true, validate: {min: 3}}\n";
            write(
                &source,
                "template.yml",
                format!("name: sample\ninterview:\n{label}"),
            );
            write(&source, "template/result.txt", "{{ label }}\n");
            std::fs::create_dir_all(&target).unwrap();
            git(&source, &["init", "-q"]);
            commit(&source);
            git(&target, &["init", "-q"]);
            write(&target, "README.md", "operator\n");
            commit(&target);
            let paths = paths(&root);
            let address = format!("file://{}", source.display());
            let target_id = canonical_target(&target).unwrap();
            let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
            let mut source_env = NoEnvironment;
            let mut environment = update_environment(&config, &registry, &root, &mut source_env);
            let prepared = updates::prepare(
                &UpdateBase::Baseline,
                &address,
                &target_id,
                &paths,
                true,
                PrepareMode::Apply,
                false,
                &mut environment,
            )
            .unwrap();
            let baseline = UpdateSession::start(
                prepared,
                target_id.clone(),
                UpdateBase::Baseline,
                false,
                Default::default(),
                &[],
                &raw("label", json!("example")),
            )
            .unwrap();
            let completed = done(baseline.state().unwrap());
            baseline
                .apply(&completed, false, &FailingRunner, None)
                .unwrap();
            commit(&target);
            let record_before = std::fs::read(target.join(".toha.json")).unwrap();
            let result_before = std::fs::read(target.join("result.txt")).unwrap();
            let id = if new_question { "color" } else { "label" };
            let extra_question = if new_question {
                "  - {id: color, type: text, prompt: Color, required: true, validate: {min: 3}}\n"
            } else {
                ""
            };
            write(
                &source,
                "template.yml",
                format!(
                    "name: sample\ninterview:\n{label}{extra_question}  - flow: abort\n    when: {id} == 'abort'\n  - flow: stop\n    when: {id} == 'stop'\n"
                ),
            );
            write(&source, "template/extra.txt", format!("{{{{ {id} }}}}\n"));
            commit(&source);
            let prepared = updates::prepare(
                &UpdateBase::Update,
                &address,
                &target_id,
                &paths,
                true,
                PrepareMode::Apply,
                false,
                &mut environment,
            )
            .unwrap();
            let invalid_parts = toha::parts::Selection {
                with: vec!["extra".into()],
                ..Default::default()
            };
            assert!(
                matches!(
                    UpdateSession::start(
                        prepared,
                        target_id.clone(),
                        UpdateBase::Update,
                        false,
                        invalid_parts,
                        &[],
                        &RawAnswers::new()
                    ),
                    Err(updates::SessionError::Operation(
                        toha::operations::OperationError::Parts(_)
                    ))
                ),
                "consumer_update_parts_typed"
            );
            let prepared = updates::prepare(
                &UpdateBase::Update,
                &address,
                &target_id,
                &paths,
                true,
                PrepareMode::Apply,
                false,
                &mut environment,
            )
            .unwrap();
            let mut session = UpdateSession::start(
                prepared,
                target_id.clone(),
                UpdateBase::Update,
                false,
                Default::default(),
                &[],
                &raw(id, json!(value)),
            )
            .unwrap();
            let store = Store::new(paths.state.clone());
            let mut wire_before = None;
            for reconstructed in [false, true] {
                let state = session.state().unwrap();
                // A broken state-to-apply driver must expose its writes before enum assertions.
                if value == "x"
                    && let SessionState::Completed(completed) = &state
                {
                    session
                        .apply(completed, false, &FailingRunner, None)
                        .unwrap();
                    assert!(
                        !target.join("extra.txt").exists(),
                        "consumer_start_replay_rejected_no_write"
                    );
                    panic!("consumer_start_replay_rejected_completed");
                }
                match (&state, value) {
                    (
                        SessionState::Asking {
                            document,
                            rejections,
                        },
                        "x",
                    ) => {
                        assert!(
                            rejections.iter().any(|r| r.id.as_str() == id),
                            "consumer_start_replay_rejection_retained"
                        );
                        assert!(
                            document["errors"].get(id).is_some(),
                            "consumer_start_replay_error_document"
                        );
                        let properties = document["schema"]["properties"].as_object().unwrap();
                        assert!(
                            properties.contains_key(id),
                            "consumer_start_replay_rejected_field_visible"
                        );
                        assert_eq!(
                            properties.len(),
                            1,
                            "consumer_start_replay_only_uncovered_field"
                        );
                        match session.replay().unwrap() {
                            toha::applied::UpdateDrive::Ask { rejections, .. } => assert!(
                                rejections.iter().any(|r| r.id.as_str() == id),
                                "consumer_start_replay_raw_rejections"
                            ),
                            _ => panic!("consumer_start_replay_raw_state"),
                        }
                    }
                    (SessionState::Completed(_), "changed") => {}
                    (SessionState::Ended(end), "abort") => {
                        assert_eq!(end.kind(), toha::EndKind::Abort)
                    }
                    (SessionState::Ended(end), "stop") => {
                        assert_eq!(end.kind(), toha::EndKind::Stop)
                    }
                    _ => panic!(
                        "consumer_start_replay_outcome_matrix: new={new_question} value={value} rebuilt={reconstructed}: {state:?}"
                    ),
                }
                assert!(
                    !target.join("extra.txt").exists(),
                    "consumer_start_replay_no_early_file"
                );
                assert_eq!(
                    std::fs::read(target.join(".toha.json")).unwrap(),
                    record_before,
                    "consumer_start_replay_record_unchanged"
                );
                assert_eq!(
                    std::fs::read(target.join("result.txt")).unwrap(),
                    result_before,
                    "consumer_start_replay_file_unchanged"
                );
                if value == "x" {
                    assert!(
                        !session
                            .record
                            .submissions
                            .iter()
                            .flat_map(|s| s.values())
                            .any(|v| v == &json!("x")),
                        "consumer_start_replay_rejected_not_accepted"
                    );
                }
                session.persist(&store).unwrap();
                if matches!(value, "abort" | "stop") {
                    assert!(
                        store.load(&target_id).unwrap().is_none(),
                        "consumer_start_replay_end_consumes_stage"
                    );
                } else {
                    let saved =
                        serde_json::to_vec(&store.load(&target_id).unwrap().unwrap()).unwrap();
                    if let Some(before) = &wire_before {
                        assert_eq!(&saved, before, "consumer_start_replay_persist_stable");
                    }
                    wire_before = Some(saved);
                }
                if !reconstructed {
                    let wire = serde_json::to_vec(&session.record).unwrap();
                    let frozen = (session.record.commit.clone(), session.record.now.clone());
                    drop(session);
                    let worker_root = root.clone();
                    let worker_target = target_id.clone();
                    let (record, state) = std::thread::spawn(move || {
                        let record: StagedRecord = serde_json::from_slice(&wire).unwrap();
                        let paths = crate::paths(&worker_root);
                        let (config, registry, _) =
                            toha::resolve::load_context(&paths, &worker_root).unwrap();
                        let mut source = NoEnvironment;
                        let mut environment =
                            update_environment(&config, &registry, &worker_root, &mut source);
                        let session = UpdateSession::resume(
                            record,
                            worker_target,
                            &paths,
                            true,
                            &mut environment,
                        )
                        .unwrap();
                        let state = session.state().unwrap();
                        (session.record, state)
                    })
                    .join()
                    .unwrap();
                    if value == "x" {
                        assert!(
                            matches!(state, SessionState::Asking { ref rejections, .. } if rejections.iter().any(|r| r.id.as_str() == id)),
                            "consumer_start_replay_worker_rejection"
                        );
                    }
                    assert_eq!(
                        (record.commit.clone(), record.now.clone()),
                        frozen,
                        "consumer_start_replay_pinned_record"
                    );
                    session = UpdateSession::resume(
                        record,
                        target_id.clone(),
                        &paths,
                        true,
                        &mut environment,
                    )
                    .unwrap();
                }
            }
            if value == "x" {
                let completed = done(session.answer(raw(id, json!("corrected"))).unwrap());
                session
                    .apply(&completed, false, &FailingRunner, Some(&store))
                    .unwrap();
                assert_eq!(
                    std::fs::read_to_string(target.join("extra.txt")).unwrap(),
                    "corrected\n",
                    "consumer_start_replay_correction_used"
                );
                assert!(
                    store.load(&target_id).unwrap().is_none(),
                    "consumer_start_replay_correction_consumes_stage"
                );
            } else if value == "changed" {
                let completed = done(session.state().unwrap());
                session
                    .apply(&completed, false, &FailingRunner, Some(&store))
                    .unwrap();
                assert_eq!(
                    std::fs::read_to_string(target.join("extra.txt")).unwrap(),
                    "changed\n",
                    "consumer_start_replay_valid_used"
                );
            }
        }
    }
}

/// Optional raw nulls remain empty answers; making either question required re-asks it.
fn update_null_replay_contract() {
    for required in [None, Some("remote"), Some("verbose")] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let source = root.join("source");
        let target = root.join("target");
        let body = |required: Option<&str>| {
            format!(
                "name: sample\ninterview:\n  - {{id: remote, type: git, prompt: Remote, required: {}}}\n  - {{id: verbose, type: confirm, prompt: Verbose, required: {}}}\n",
                required == Some("remote"),
                required == Some("verbose")
            )
        };
        write(&source, "template.yml", body(None));
        write(&source, "template/result.txt", "original\n");
        std::fs::create_dir_all(&target).unwrap();
        git(&source, &["init", "-q"]);
        commit(&source);
        git(&target, &["init", "-q"]);
        commit(&target);
        let paths = paths(&root);
        let target_id = canonical_target(&target).unwrap();
        let address = format!("file://{}", source.display());
        let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
        let mut source_env = NoEnvironment;
        let mut environment = update_environment(&config, &registry, &root, &mut source_env);
        let mut answers = raw("remote", json!(null));
        answers.extend(raw("verbose", json!(null)));
        let resolved = resolve_template(&address, &config, &registry, &paths, &root).unwrap();
        let preparation = toha::operations::Preparation::new(
            Template::load(&resolved.folder).unwrap(),
            resolved,
            target_id.clone(),
            &[],
            &answers,
        )
        .unwrap();
        let context = preparation
            .context(
                EnvironmentDecision::Deny,
                HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
                ExecutionFacts::new(false, false),
                &mut NoEnvironment,
            )
            .unwrap();
        let mut create = preparation
            .start(
                context,
                "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                config.clone(),
                Default::default(),
                &answers,
                None,
            )
            .unwrap();
        let completed = done(create.answer_all(answers).unwrap());
        create
            .apply(
                &completed,
                ApplyOptions::default(),
                false,
                &FailingRunner,
                None,
            )
            .unwrap();
        commit(&target);
        let record_before = std::fs::read(target.join(".toha.json")).unwrap();
        let result_before = std::fs::read(target.join("result.txt")).unwrap();
        write(&source, "template.yml", body(required));
        write(&source, "template/extra.txt", "extra\n");
        commit(&source);
        let prepared = updates::prepare(
            &UpdateBase::Update,
            &address,
            &target_id,
            &paths,
            true,
            PrepareMode::Stage,
            false,
            &mut environment,
        )
        .unwrap();
        let started = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Update,
            false,
            Default::default(),
            &[],
            &RawAnswers::new(),
        );
        assert!(
            started.is_ok(),
            "consumer_optional_null_replay_admitted: {:?}",
            started.as_ref().err()
        );
        let mut session = started.unwrap();
        let store = Store::new(paths.state.clone());
        for rebuilt in [false, true] {
            let state = session.state().unwrap();
            if required.is_some()
                && let SessionState::Completed(completed) = &state
            {
                session
                    .apply(completed, false, &FailingRunner, None)
                    .unwrap();
                assert!(
                    !target.join("extra.txt").exists(),
                    "consumer_required_null_no_write"
                );
                panic!("consumer_required_null_completed");
            }
            match (state, required) {
                (SessionState::Completed(_), None) => {}
                (
                    SessionState::Asking {
                        document,
                        rejections,
                    },
                    Some(id),
                ) => {
                    assert!(
                        rejections.iter().any(|r| r.id.as_str() == id),
                        "consumer_required_null_replay_rejected"
                    );
                    assert!(
                        document["errors"].get(id).is_some(),
                        "consumer_required_null_replay_errors"
                    );
                }
                (other, _) => panic!(
                    "consumer_null_replay_state: required={required:?}, rebuilt={rebuilt}: {other:?}"
                ),
            }
            assert!(
                !target.join("extra.txt").exists(),
                "consumer_null_replay_no_early_write"
            );
            assert_eq!(
                std::fs::read(target.join(".toha.json")).unwrap(),
                record_before,
                "consumer_null_replay_record_unchanged"
            );
            assert_eq!(
                std::fs::read(target.join("result.txt")).unwrap(),
                result_before,
                "consumer_null_replay_file_unchanged"
            );
            session.persist(&store).unwrap();
            if !rebuilt {
                let wire = serde_json::to_vec(&session.record).unwrap();
                drop(session);
                session = UpdateSession::resume(
                    serde_json::from_slice(&wire).unwrap(),
                    target_id.clone(),
                    &paths,
                    true,
                    &mut environment,
                )
                .unwrap();
            }
        }
        if required.is_none() {
            let completed = done(session.state().unwrap());
            session
                .apply(&completed, false, &FailingRunner, Some(&store))
                .unwrap();
            let applied = toha::applied::AppliedRecord::parse(
                &std::fs::read(target.join(".toha.json")).unwrap(),
            )
            .unwrap();
            let submissions = applied.applications()[0].submissions();
            for id in ["remote", "verbose"] {
                assert_eq!(
                    submissions.iter().find_map(|s| s.get(id)),
                    Some(&json!(null)),
                    "consumer_optional_null_persisted"
                );
            }
            assert!(store.load(&target_id).unwrap().is_none());
        }
    }
}
