// ---
// relationships:
//   verifies:
//     - content-injection
//     - project-updates
// ---
//! Real public-library caller: saved update requests grant no region force.
use super::*;
use std::{fs, path::PathBuf};
use toha::applied::{MergeError, Merged};
use toha::updates::{SessionError, UpdateOptions};

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    files: Vec<(PathBuf, Vec<u8>, u32)>,
    index: Vec<u8>,
    staged: String,
    refs: String,
    stage: Vec<u8>,
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
fn files(root: &Path, dir: &Path, output: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == ".git" {
            continue;
        }
        if path.is_dir() {
            files(root, &path, output);
        } else {
            output.push((
                path.strip_prefix(root).unwrap().into(),
                fs::read(&path).unwrap(),
                mode(&path),
            ));
        }
    }
}
fn snapshot(project: &Path, stage: &Path) -> Snapshot {
    let mut output = vec![];
    files(project, project, &mut output);
    output.sort();
    assert!(
        !project.join(".git/index.lock").exists(),
        "consumer_region_no_stale_lock"
    );
    Snapshot {
        files: output,
        index: fs::read(project.join(".git/index")).unwrap(),
        staged: git(project, &["diff", "--cached", "--binary"]),
        refs: git(project, &["show-ref", "--head"]),
        stage: fs::read(stage).unwrap(),
    }
}
fn drift(error: SessionError) {
    assert!(
        matches!(error, SessionError::Merge(MergeError::Apply(ref e)) if matches!(e.as_ref(), toha::ApplyError::Drift(_))),
        "consumer_region_typed_drift: {error:?}"
    );
}

pub(super) fn verify() {
    for transition in ["retained", "removal", "takeover"] {
        verify_transition(transition);
    }
}

fn verify_transition(transition: &str) {
    println!("external region transition: {transition}");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    let project = root.join("project");
    let target = project.join("generated");
    fs::create_dir_all(&target).unwrap();
    write(&source, "template/.keep", "");
    let manifest = "name: sample\nignore: [.keep]\ninject:\n  - into: config.toml\n    region: features\n    content: |\n      alpha = 1\n    anchor: {after: '[application]', occurrence: only}\nhooks:\n  - run: [git, --version]\n";
    let manifest = if transition == "removal" {
        manifest.replace("    anchor:", "    marker: '# managed'\n    anchor:")
    } else {
        manifest.to_owned()
    };
    write(&source, "template.yml", &manifest);
    git(&source, &["init", "-q"]);
    commit(&source);
    git(&project, &["init", "-q"]);
    write(
        &target,
        "config.toml",
        "[application]\n\n\n[other]\nkeep = 1\n",
    );
    write(&project, "outside.txt", "original\n");
    commit(&project);
    let address = format!("file://{}", source.display());
    let paths = paths(&root);
    let target_id = canonical_target(&target).unwrap();
    let (config, registry, _) = toha::resolve::load_context(&paths, &root).unwrap();
    let resolved = resolve_template(&address, &config, &registry, &paths, &root).unwrap();
    let template = Template::load(&resolved.folder).unwrap();
    let mut source_env = NoEnvironment;
    let context = toha::operations::invocation_context(
        &target_id,
        &resolved,
        &template,
        EnvironmentDecision::GrantIfNeeded,
        HostFacts::capture(),
        ExecutionFacts::new(false, false),
        &mut source_env,
    )
    .unwrap();
    let initial = Session::start(
        template,
        resolved,
        target_id.clone(),
        context,
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        config.clone(),
        Default::default(),
        &[],
        &RawAnswers::new(),
        None,
    )
    .unwrap();
    let runner = Runner {
        calls: RefCell::new(vec![]),
    };
    let completed = done(initial.state().unwrap());
    initial
        .apply(
            &completed,
            ApplyOptions {
                trusted: true,
                ..Default::default()
            },
            false,
            &runner,
            None,
        )
        .unwrap();
    drop(initial);
    commit(&project);
    let current = fs::read_to_string(target.join("config.toml")).unwrap();
    write(
        &target,
        "config.toml",
        current
            .replace("alpha = 1", "alpha = 7")
            .replace("keep = 1", "keep = 2"),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            target.join("config.toml"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    commit(&project);
    write(&project, "outside.txt", "staged outside target\n");
    git(&project, &["add", "outside.txt"]);
    let next_manifest = if transition == "removal" {
        "name: sample\nignore: [.keep]\nhooks:\n  - run: [git, --version]\n".to_owned()
    } else {
        manifest.clone()
    };
    write(
        &source,
        "template.yml",
        format!(
            "{next_manifest}interview:\n  - {{id: color, type: text, prompt: Color, required: true}}\n"
        ),
    );
    if transition == "takeover" {
        write(
            &source,
            "template/config.toml",
            "[application]\n\n\n[other]\nkeep = 2\n",
        );
    }
    write(&source, "template/new.txt", "{{ color }}\n");
    commit(&source);
    let store = Store::new(paths.state.clone());
    let mut environment = update_environment(&config, &registry, &root, &mut source_env);
    if transition == "takeover" {
        write(
            &source,
            "template/config.toml",
            "[application]\n\n\n[other]\nkeep = 1\n",
        );
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
        let mut stage = UpdateSession::start(
            prepared,
            target_id.clone(),
            UpdateBase::Update,
            false,
            Default::default(),
            &[],
            &raw("color", json!("blue")),
        )
        .unwrap();
        stage.persist(&store).unwrap();
        drop(stage);
        let stage_path = store.path_for(&target_id);
        let before = snapshot(&project, &stage_path);
        for force in [false, true] {
            let wire = fs::read(&stage_path).unwrap();
            let record = serde_json::from_slice(&wire).unwrap();
            let session =
                UpdateSession::resume(record, target_id.clone(), &paths, true, &mut environment)
                    .unwrap();
            let completed = done(session.state().unwrap());
            let error = session
                .apply_with_options(
                    &completed,
                    UpdateOptions {
                        force,
                        dry_run: false,
                    },
                    &runner,
                    Some(&store),
                )
                .expect_err("consumer_takeover_outside_conflict");
            assert!(
                matches!(error, SessionError::Merge(MergeError::Apply(ref e)) if if force { matches!(e.as_ref(), toha::ApplyError::Conflicts(_)) } else { matches!(e.as_ref(), toha::ApplyError::Drift(_)) }),
                "consumer_takeover_outside_typed: {error:?}"
            );
            assert_eq!(
                snapshot(&project, &stage_path),
                before,
                "consumer_takeover_outside_state_preserved"
            );
        }
        store.remove(&target_id).unwrap();
        write(
            &source,
            "template/config.toml",
            "[application]\n\n\n[other]\nkeep = 2\n",
        );
        commit(&source);
    }
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
    let mut staged = UpdateSession::start(
        prepared,
        target_id.clone(),
        UpdateBase::Update,
        false,
        Default::default(),
        &[],
        &RawAnswers::new(),
    )
    .unwrap();
    assert!(
        matches!(staged.state().unwrap(), SessionState::Asking { .. }),
        "consumer_region_new_question"
    );
    done(staged.answer(raw("color", json!("blue"))).unwrap());
    staged.persist(&store).unwrap();
    drop(staged);
    let stage_path = store.path_for(&target_id);
    let before = snapshot(&project, &stage_path);
    runner.calls.borrow_mut().clear();
    // Re-load and deserialize the actual persisted record at each request boundary.
    let resume = || {
        let wire = fs::read(&stage_path).unwrap();
        let record: StagedRecord = serde_json::from_slice(&wire).unwrap();
        let mut source = NoEnvironment;
        let mut environment = update_environment(&config, &registry, &root, &mut source);
        UpdateSession::resume(record, target_id.clone(), &paths, true, &mut environment).unwrap()
    };
    for preview in [false, true] {
        let session = resume();
        let completed = done(session.state().unwrap());
        drift(
            session
                .apply(&completed, preview, &runner, Some(&store))
                .unwrap_err(),
        );
        assert_eq!(
            snapshot(&project, &stage_path),
            before,
            "consumer_region_default_refusal_atomic"
        );
    }
    let session = resume();
    let completed = done(session.state().unwrap());
    drift(
        session
            .apply_with_options(&completed, UpdateOptions::default(), &runner, Some(&store))
            .unwrap_err(),
    );
    let session = resume();
    let completed = done(session.state().unwrap());
    assert!(
        matches!(
            session
                .apply_with_options(
                    &completed,
                    UpdateOptions {
                        force: true,
                        dry_run: true
                    },
                    &runner,
                    Some(&store)
                )
                .expect("consumer_region_forced_preview must accept explicit force")
                .0,
            Merged::Planned { .. }
        ),
        "consumer_region_forced_preview"
    );
    assert_eq!(
        snapshot(&project, &stage_path),
        before,
        "consumer_region_forced_preview_atomic"
    );
    if transition == "retained" {
        // Existing low-level callers keep their literal options and safe default.
        let session = resume();
        let completed = done(session.state().unwrap());
        let prep = session.prepared;
        let revision =
            toha::resolve::applied_revision(&prep.resolved.formal_name, &prep.resolved.commit)
                .unwrap();
        let result = toha::applied::merge_apply_reported(
            &prep.project,
            prep.merge_base,
            &prep.template,
            &completed,
            toha::applied::RecordInputs {
                template: prep.resolved.formal_name,
                revision,
            },
            toha::applied::MergeOptions {
                trusted: true,
                dry_run: false,
            },
            &runner,
        );
        assert!(
            matches!(result, Err(MergeError::Apply(e)) if matches!(*e, toha::ApplyError::Drift(_))),
            "consumer_region_low_level_default_safe"
        );
        assert_eq!(snapshot(&project, &stage_path), before);
        let session = resume();
        let completed = done(session.state().unwrap());
        assert!(
            matches!(session.prepared.finish(&target_id, &completed, false, &runner), Err(MergeError::Apply(e)) if matches!(*e, toha::ApplyError::Drift(_))),
            "consumer_region_prepared_default_safe"
        );
        let session = resume();
        let completed = done(session.state().unwrap());
        assert!(
            matches!(
                session
                    .prepared
                    .finish_with_options(
                        &target_id,
                        &completed,
                        UpdateOptions {
                            force: true,
                            dry_run: true
                        },
                        &runner
                    )
                    .unwrap()
                    .0,
                Merged::Planned { .. }
            ),
            "consumer_region_prepared_force_preview"
        );
        assert_eq!(snapshot(&project, &stage_path), before);
    }
    assert!(
        runner.calls.borrow().is_empty(),
        "consumer_region_refusal_preview_do_not_run_hooks"
    );
    let session = resume();
    let completed = done(session.state().unwrap());
    drift(
        session
            .apply(&completed, false, &runner, Some(&store))
            .unwrap_err(),
    );
    assert_eq!(
        snapshot(&project, &stage_path),
        before,
        "consumer_region_force_never_persisted"
    );
    let session = resume();
    let completed = done(session.state().unwrap());
    let result = session
        .apply_with_options(
            &completed,
            UpdateOptions {
                force: true,
                dry_run: false,
            },
            &runner,
            Some(&store),
        )
        .unwrap();
    assert!(
        matches!(result.0, Merged::Written { ref conflicted, .. } if conflicted.is_empty()),
        "consumer_region_force_applies"
    );
    assert!(
        store.load(&target_id).unwrap().is_none(),
        "consumer_region_success_consumes_stage"
    );
    assert_eq!(
        runner.calls.borrow().len(),
        1,
        "consumer_region_caller_runner_used"
    );
    let content = fs::read_to_string(target.join("config.toml")).unwrap();
    assert!(
        (content.contains("alpha = 1") == (transition != "removal"))
            && !content.contains("alpha = 7")
            && content.contains("keep = 2"),
        "consumer_region_force_scoped: {content}"
    );
    assert_eq!(
        fs::read_to_string(target.join("new.txt")).unwrap(),
        "blue\n",
        "consumer_region_rebuilt_answer_retained"
    );
    assert_eq!(
        fs::read(project.join("outside.txt")).unwrap(),
        b"staged outside target\n"
    );
    let staged_after = git(
        &project,
        &["diff", "--cached", "--binary", "--", "outside.txt"],
    );
    assert_eq!(
        staged_after, before.staged,
        "consumer_region_outside_staged_state_preserved"
    );
    assert_eq!(git(&project, &["show-ref", "--head"]), before.refs);
    assert_eq!(
        mode(&target.join("config.toml")),
        before
            .files
            .iter()
            .find(|(p, _, _)| p == Path::new("generated/config.toml"))
            .unwrap()
            .2
    );
    println!(
        "external region consumer passed: typed refusal, serialized rebuild, safe defaults, forced preview/apply, atomicity, staged outside target and caller runner"
    );
}
