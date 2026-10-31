// ---
// relationships:
//   verifies: template-format
// ---
use std::collections::BTreeMap;
use toha::context::{EnvironmentDecision, FixedEnvironment, FixedEnvironmentSource};
struct Environment(usize);
impl FixedEnvironmentSource for Environment {
    fn capture(&mut self) -> FixedEnvironment {
        self.0 += 1;
        FixedEnvironment::new(Some("sample".into()), None, None, None, None)
    }
}
pub fn verify() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("template")).unwrap();
    // Each compiled handler surface must participate in environment admission.
    for field in [
        "run: [tool, '{{ toha_env_user }}']",
        "run: [tool]\n      when: toha_env_user == 'sample'",
        "run: [tool]\n      each: '[toha_env_user]'",
        "run: [tool]\n      cwd: '{{ toha_env_user }}'",
    ] {
        std::fs::write(
            root.path().join("template.yml"),
            format!("name: example\nevents:\n  refresh:\n    - {field}\n"),
        )
        .unwrap();
        let template = toha::Template::load(root.path()).unwrap();
        let mut env = Environment(0);
        assert!(
            template
                .admit_environment(EnvironmentDecision::RequireStageGrant, &mut env)
                .is_err()
        );
        template
            .admit_environment(EnvironmentDecision::Deny, &mut env)
            .unwrap();
        assert_eq!(env.0, 0);
        template
            .admit_environment(EnvironmentDecision::GrantIfNeeded, &mut env)
            .unwrap();
        assert_eq!(env.0, 1);
    }
    std::fs::write(root.path().join("template.yml"),
        "name: example\nevents:\n  refresh:\n    - run: [tool, '{{ toha_target_name }}', '{{ toha_template_name }}', '{{ toha_template_formal_name }}', '{{ toha_env_user }}']\n").unwrap();
    let template = toha::Template::load(root.path()).unwrap();
    let app = toha::applied::Application::new(
        "formal".into(),
        toha::applied::Revision::Unversioned,
        toha::applied::RepoPath::parse(".").unwrap(),
        toha::applied::FrozenNow::parse("2026-01-01T00:00:00Z[UTC]").unwrap(),
        BTreeMap::new(),
        BTreeMap::from([("toha_env_user".into(), serde_json::json!("recorded"))]),
        vec![],
        vec![],
    );
    let prepared = toha::events::prepare(&template, &app, "refresh", root.path()).unwrap();
    assert_eq!(
        prepared[0].hook.argv(),
        [
            "tool",
            root.path().file_name().unwrap().to_str().unwrap(),
            "example",
            "formal",
            "recorded"
        ]
    );
}

pub fn fire_with_effective_registry_approval() {
    use toha::applied::{Application, AppliedRecord, FrozenNow, RepoPath, Revision};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let repo = root.path().join("repo");
    std::fs::create_dir_all(source.join("template")).unwrap();
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        source.join("template.yml"),
        "name: example\nevents:\n  refresh:\n    - run: [git, --version]\n",
    )
    .unwrap();
    super::git(&repo, &["init", "-q"]);
    let source = source.canonicalize().unwrap();
    let formal = source.to_str().unwrap();
    let template = toha::Template::load(&source).unwrap();
    let digest = toha::HookSurface::of(&template).unwrap().digest();
    let dirs = super::paths(root.path());
    let mut record = AppliedRecord::default();
    record.replace(Application::new(
        formal.into(),
        Revision::Unversioned,
        RepoPath::parse(".").unwrap(),
        FrozenNow::parse("2026-01-01T00:00:00Z[UTC]").unwrap(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![],
        vec![],
    ));
    std::fs::write(repo.join(".toha.json"), record.to_json_bytes().unwrap()).unwrap();
    let entry =
        serde_json::json!({"name":"example","source":formal,"path":source,"approval":digest});
    let registry = serde_json::json!({"templates":{formal:entry}});
    std::fs::create_dir_all(&dirs.system_data).unwrap();
    std::fs::write(dirs.system_data.join("templates.yml"), registry.to_string()).unwrap();
    let runner = super::Runner {
        calls: std::cell::RefCell::new(vec![]),
    };
    let fire = |trust| {
        toha::events::fire(
            "refresh",
            &repo,
            toha::events::EventOptions {
                trust,
                dry_run: false,
            },
            &dirs,
            root.path(),
            &runner,
        )
    };
    assert_eq!(fire(false).unwrap().handler_count, 1);
    assert_eq!(runner.calls.borrow().len(), 1);
    // A writable user-layer denial must mask the system approval.
    std::fs::create_dir_all(&dirs.user_data).unwrap();
    std::fs::write(dirs.user_data.join("templates.yml"),
        serde_json::json!({"templates":{formal:{"name":"example","source":formal,"path":source,"trusted":false}}}).to_string()).unwrap();
    assert!(fire(false).unwrap_err().contains("require trust"));
    assert_eq!(runner.calls.borrow().len(), 1);
    assert_eq!(fire(true).unwrap().handler_count, 1);
    assert_eq!(runner.calls.borrow().len(), 2);
    // Git revisions retain the same effective approval without changing the
    // recorded revision to the registry's current selection.
    super::git(&source, &["init", "-q"]);
    super::commit(&source);
    let commit = super::git(&source, &["rev-parse", "HEAD"]);
    let git_formal = "https://example.invalid/templates.git";
    let mut entry = entry;
    entry["commit"] = serde_json::json!(commit);
    entry["source"] = serde_json::json!(git_formal);
    std::fs::write(
        dirs.user_data.join("templates.yml"),
        serde_json::json!({"templates":{git_formal:entry}}).to_string(),
    )
    .unwrap();
    let (config, registry, _) = toha::resolve::load_context(&dirs, root.path()).unwrap();
    let resolved = toha::resolve::resume_application_template(
        git_formal,
        &Revision::Commit(toha::applied::CommitId::parse(&commit).unwrap()),
        &config,
        &registry,
        &dirs,
        root.path(),
    )
    .unwrap();
    assert_eq!(resolved.commit, commit);
    assert_eq!(resolved.approval, Some(digest));
}
