// ---
// relationships:
//   implements: command-line-interface
// ---
#[allow(dead_code)]
mod support;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn command(root: &TempDir) -> Command {
    let mut command = support::isolated_command(root.path());
    command.current_dir(root.path());
    command
}
fn run(root: &TempDir, args: &[&str], exit: i32) -> Output {
    let output = command(root).args(args).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "args={args:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn repository(root: &TempDir) -> (String, String) {
    let repo = root.path().join("owner/repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Sample"]);
    git(&repo, &["config", "user.email", "sample@example.invalid"]);
    for (folder, name, content) in [("one", "same", "old"), ("two", "same", "second")] {
        fs::create_dir_all(repo.join(folder).join("template")).unwrap();
        fs::write(repo.join(folder).join("template.yml"), format!("name: {name}\ninterview:\n  - {{ id: label, type: text, prompt: Label?, default: Template }}\n")).unwrap();
        fs::write(
            repo.join(folder).join("template/result.txt"),
            format!("{content} {{{{ label }}}}"),
        )
        .unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    (support::file_url(&repo), commit)
}
fn write_config(root: &TempDir, content: &str) {
    let path = root.path().join("config/toha/config.yml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
/// The leading JSON result document on standard output. Agent routes
/// (`stage --async`, `continue PATH FILE`, `apply PATH`) append plain-text
/// instructions after the document, so only the first value is parsed.
fn document(output: &Output) -> Value {
    support::first_document(&output.stdout)
}

/// The formal template identity a route establishes for `template`, discovered
/// from the route itself: an identity-less document is refused with an `error`
/// document whose `context.template` is the formal name every answers envelope
/// must copy. A registry name or git address has a formal name that is not a
/// folder path, so it is read from the route rather than guessed. The scripted
/// route never stages and identity fails before any write, so the probe leaves
/// nothing behind.
fn formal(root: &TempDir, template: &str) -> String {
    let probe = root.path().join(".formal-probe");
    let answers = root.path().join(".formal-probe.json");
    fs::write(&answers, "{}").unwrap();
    let output = command(root)
        .args([
            "apply",
            template,
            probe.to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let document = document(&output);
    document["context"]["template"]
        .as_str()
        .unwrap_or_else(|| panic!("probe did not publish context.template: {document}"))
        .to_string()
}

/// Writes the identity envelope `{"template": formal, "answers": <bare>}` the
/// scripted (`apply --answers`) and agent (`continue PATH FILE`) routes require,
/// and returns its path.
fn envelope(root: &TempDir, formal: &str, answers: &str) -> String {
    let value: Value = serde_json::from_str(answers).unwrap();
    let document = serde_json::json!({ "template": formal, "answers": value });
    let path = root.path().join("envelope.json");
    fs::write(&path, document.to_string()).unwrap();
    path.to_str().unwrap().to_string()
}

/// The envelope path for `template` with the given bare answers map, discovering
/// the formal identity from the route.
fn answers_for(root: &TempDir, template: &str, answers: &str) -> String {
    let formal = formal(root, template);
    envelope(root, &formal, answers)
}

#[test]
fn alias_short_ambiguous_and_address_forms() {
    let root = TempDir::new().unwrap();
    let (url, commit) = repository(&root);
    write_config(
        &root,
        &format!("hosts:\n  local: {}\n", support::file_url(root.path())),
    );
    let first = "local:owner/repo#one";
    run(
        &root,
        &[
            "templates",
            "add",
            &format!("{url}#one"),
            "--alias",
            "chosen",
        ],
        0,
    );
    write_config(
        &root,
        &format!(
            "hosts:\n  local: {}\ntemplate-defaults:\n  {first:?}: {{ label: Formal }}\n  chosen: {{ label: Alias }}\n",
            support::file_url(root.path())
        ),
    );
    let target = root.path().join("target");
    let batch = document(&run(
        &root,
        &["stage", "chosen", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(batch["context"]["template"], first);
    assert_eq!(batch["context"]["commit"], commit);
    assert_eq!(batch["schema"]["properties"]["label"]["default"], "Formal");
    run(&root, &["abort", target.to_str().unwrap()], 0);
    let short = document(&run(
        &root,
        &["stage", "same", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(short["context"]["template"], first);
    run(&root, &["abort", target.to_str().unwrap()], 0);
    let second = "local:owner/repo#two";
    run(&root, &["templates", "add", &format!("{url}#two")], 0);
    let ambiguous = run(
        &root,
        &["stage", "same", target.to_str().unwrap(), "--async"],
        5,
    );
    let error = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(
        error.contains(first) && error.contains(second) && error.contains("templates alias"),
        "{error}"
    );
    let explicit = format!("{url}@main#one");
    let explicit_answers = answers_for(&root, &explicit, r#"{"label":"Headless"}"#);
    run(
        &root,
        &[
            "apply",
            &explicit,
            root.path().join("explicit").to_str().unwrap(),
            "--answers",
            &explicit_answers,
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("explicit/result.txt")).unwrap(),
        "old Headless"
    );
    let shorthand = "local:owner/repo@main#one";
    let shorthand_answers = answers_for(&root, shorthand, r#"{"label":"Headless"}"#);
    run(
        &root,
        &[
            "apply",
            shorthand,
            root.path().join("shorthand").to_str().unwrap(),
            "--answers",
            &shorthand_answers,
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("shorthand/result.txt")).unwrap(),
        "old Headless"
    );
    let missing = run(
        &root,
        &["stage", "absent", target.to_str().unwrap(), "--async"],
        1,
    );
    assert!(String::from_utf8_lossy(&missing.stderr).contains("template not found"));
    let missing_path = run(
        &root,
        &[
            "stage",
            &format!("{url}#absent"),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(!missing_path.stderr.is_empty());
    let missing_repo = run(
        &root,
        &[
            "stage",
            &format!("{}/absent", support::file_url(root.path())),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(!missing_repo.stderr.is_empty());
    let escaped = run(
        &root,
        &[
            "stage",
            &format!("{url}#{}", root.path().join("owner/repo/one").display()),
            target.to_str().unwrap(),
            "--async",
        ],
        1,
    );
    assert!(String::from_utf8_lossy(&escaped.stderr).contains("leaves repository"));
}

#[test]
fn cached_commit_survives_source_disappearance() {
    let root = TempDir::new().unwrap();
    let (url, commit) = repository(&root);
    let address = format!("{url}@{commit}#one");
    // Discover the envelope while the source is present, then reuse it after the
    // source disappears; the commit-pinned formal name does not change.
    let answers = answers_for(&root, &address, r#"{"label":"Cached"}"#);
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("first").to_str().unwrap(),
            "--answers",
            &answers,
        ],
        0,
    );
    fs::rename(
        root.path().join("owner/repo"),
        root.path().join("owner/removed"),
    )
    .unwrap();
    run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("second").to_str().unwrap(),
            "--answers",
            &answers,
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("second/result.txt")).unwrap(),
        "old Cached"
    );
}

#[test]
fn configured_default_is_visible_and_answers_win() {
    let root = TempDir::new().unwrap();
    let (url, _) = repository(&root);
    let address = format!("{url}#one");
    let config = |value: &str| {
        format!(
            "presets:\n  label_value: {value}\ntemplate-defaults:\n  {address:?}:\n    label: {{ preset: label_value }}\n    removed_question: unused\n"
        )
    };
    write_config(&root, &config("Configured"));
    let target = root.path().join("staged");
    let warning = format!(
        "warning: {}: template-defaults.\"{}\".removed_question: question is not defined by the selected template; ignored",
        root.path().join("config/toha/config.yml").display(),
        address
    );
    let staged = run(
        &root,
        &["stage", &address, target.to_str().unwrap(), "--async"],
        4,
    );
    assert!(String::from_utf8_lossy(&staged.stderr).contains(&warning));
    let batch = document(&staged);
    assert_eq!(
        batch["schema"]["properties"]["label"]["default"],
        "Configured"
    );
    // The formal identity of the mapped and unmapped templates, read from the
    // route while the good config resolves.
    let formal_one = formal(&root, &address);
    let two = format!("{url}#two");
    let formal_two = formal(&root, &two);
    write_config(&root, &config("Resumed"));
    // Resuming re-reads the configured default: the peeked batch (`apply PATH`, the
    // agent route) shows the new value, and config warnings reach stderr on every
    // route.
    let peeked = run(&root, &["apply", target.to_str().unwrap()], 4);
    assert!(String::from_utf8_lossy(&peeked.stderr).contains(&warning));
    let resumed = document(&peeked);
    assert_eq!(
        resumed["schema"]["properties"]["label"]["default"],
        "Resumed"
    );
    // Completing with the empty envelope lets `label` take its configured default;
    // a completing `continue PATH FILE` emits instructions only, no document.
    run(
        &root,
        &[
            "continue",
            target.to_str().unwrap(),
            &envelope(&root, &formal_one, "{}"),
        ],
        0,
    );
    let applied = run(&root, &["apply", target.to_str().unwrap()], 0);
    assert!(String::from_utf8_lossy(&applied.stderr).contains(&warning));
    assert_eq!(
        fs::read_to_string(root.path().join("staged/result.txt")).unwrap(),
        "old Resumed"
    );
    write_config(&root, &config("Configured"));
    let defaulted = run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("defaulted").to_str().unwrap(),
            "--answers",
            &envelope(&root, &formal_one, "{}"),
        ],
        0,
    );
    assert!(String::from_utf8_lossy(&defaulted.stderr).contains(&warning));
    assert_eq!(
        fs::read_to_string(root.path().join("defaulted/result.txt")).unwrap(),
        "old Configured"
    );
    let overridden = run(
        &root,
        &[
            "apply",
            &address,
            root.path().join("overridden").to_str().unwrap(),
            "--answers",
            &envelope(&root, &formal_one, r#"{"label":"Override"}"#),
        ],
        0,
    );
    assert!(String::from_utf8_lossy(&overridden.stderr).contains(&warning));
    assert_eq!(
        fs::read_to_string(root.path().join("overridden/result.txt")).unwrap(),
        "old Override"
    );
    run(
        &root,
        &[
            "apply",
            &two,
            root.path().join("unmapped").to_str().unwrap(),
            "--answers",
            &envelope(&root, &formal_two, "{}"),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("unmapped/result.txt")).unwrap(),
        "second Template"
    );
    write_config(&root, &config("[wrong]"));
    let wrong = run(
        &root,
        &[
            "stage",
            &address,
            root.path().join("wrong").to_str().unwrap(),
            "--async",
        ],
        1,
    );
    let error = String::from_utf8_lossy(&wrong.stderr);
    assert!(error.contains("template-defaults."), "{error}");
    assert!(
        error.contains("presets.\"label_value\" ([\"wrong\"]): must be a string"),
        "{error}"
    );
}

#[test]
fn constraint_invalid_configured_default_is_replaceable_on_every_command_route() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("template");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: sample\ninterview: [{ id: mode, type: select, prompt: Mode?, options: [fast, slow] }]\n",
    )
    .unwrap();
    fs::write(folder.join("template/result.txt"), "{{ mode }}").unwrap();
    let formal = folder
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    write_config(
        &root,
        &format!("template-defaults:\n  {formal:?}: {{ mode: medium }}\n"),
    );
    let source = format!(
        "{}: template-defaults.\"{}\".mode",
        root.path().join("config/toha/config.yml").display(),
        formal.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let continued_target = root.path().join("continued");
    let staged = run(
        &root,
        &[
            "stage",
            &support::folder_address(&folder),
            continued_target.to_str().unwrap(),
            "--async",
        ],
        4,
    );
    let batch = document(&staged);
    assert_eq!(
        batch["errors"]["mode"][0],
        format!("default \"medium\" from {source} is not allowed: must be one of: fast, slow")
    );
    assert!(
        batch["schema"]["properties"]["mode"]
            .get("default")
            .is_none()
    );
    // A local folder's formal identity is its canonical path, the value every
    // answers envelope names. The valid `fast` replaces the invalid default.
    let fast = r#"{"mode":"fast"}"#;
    run(
        &root,
        &[
            "continue",
            continued_target.to_str().unwrap(),
            &envelope(&root, &formal, fast),
        ],
        0,
    );

    // The staged route: the scripted `apply --answers` never uses staged state, so
    // a staged interview is resumed with the replacing answer through the agent
    // route (`continue PATH FILE`) and written with `apply PATH`.
    let staged_target = root.path().join("staged-apply");
    run(
        &root,
        &[
            "stage",
            &support::folder_address(&folder),
            staged_target.to_str().unwrap(),
            "--async",
        ],
        4,
    );
    run(
        &root,
        &[
            "continue",
            staged_target.to_str().unwrap(),
            &envelope(&root, &formal, fast),
        ],
        0,
    );
    run(&root, &["apply", staged_target.to_str().unwrap()], 0);
    assert_eq!(
        fs::read_to_string(staged_target.join("result.txt")).unwrap(),
        "fast"
    );

    // The direct scripted route replaces the invalid default from a fresh apply.
    let direct_target = root.path().join("direct");
    run(
        &root,
        &[
            "apply",
            &support::folder_address(&folder),
            direct_target.to_str().unwrap(),
            "--answers",
            &envelope(&root, &formal, fast),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(direct_target.join("result.txt")).unwrap(),
        "fast"
    );
}

#[test]
fn resume_uses_recorded_commit_after_registry_update() {
    let root = TempDir::new().unwrap();
    let (url, old) = repository(&root);
    let address = format!("{url}#one");
    run(
        &root,
        &["templates", "add", &address, "--alias", "chosen"],
        0,
    );
    let target = root.path().join("target");
    let formal_chosen = formal(&root, "chosen");
    let staged = document(&run(
        &root,
        &["stage", "chosen", target.to_str().unwrap(), "--async"],
        4,
    ));
    assert_eq!(staged["context"]["commit"], old);
    let repo = root.path().join("owner/repo");
    fs::write(repo.join("one/template/result.txt"), "new {{ label }}").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "next"]);
    run(&root, &["templates", "update", "chosen"], 0);
    // Resuming after the registry update still resolves the recorded commit; the
    // peeked batch (`apply PATH`, the agent route) names it in its context.
    let resumed = document(&run(&root, &["apply", target.to_str().unwrap()], 4));
    assert_eq!(resumed["context"]["commit"], old);
    run(
        &root,
        &[
            "continue",
            target.to_str().unwrap(),
            &envelope(&root, &formal_chosen, r#"{"label":"Resumed"}"#),
        ],
        0,
    );
    run(&root, &["apply", target.to_str().unwrap()], 0);
    assert_eq!(
        fs::read_to_string(target.join("result.txt")).unwrap(),
        "old Resumed"
    );
}

#[cfg(unix)]
#[test]
fn registry_trust_runs_hooks_but_discovery_and_local_registry_do_not() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("hook-template");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(
        folder.join("template.yml"),
        "name: hooked\nhooks:\n  - run: [/bin/sh, -c, 'printf ok > hook.txt']\n",
    )
    .unwrap();
    fs::write(folder.join("template/file.txt"), "file").unwrap();
    // A local folder's formal identity is its canonical path; every envelope names
    // it. The empty answers map drives the hookless walk to completion.
    let folder_formal = support::formal_name(&folder);
    let direct = run(
        &root,
        &[
            "apply",
            folder.to_str().unwrap(),
            root.path().join("direct").to_str().unwrap(),
            "--answers",
            &envelope(&root, &folder_formal, "{}"),
        ],
        3,
    );
    // The scripted route reports untrusted hooks structurally: a `planned`
    // document with `trusted: false` and exit 3, not a stderr "--trust" notice.
    let direct = document(&direct);
    assert_eq!(direct["status"], "planned");
    assert_eq!(direct["trusted"], false);
    let staged = root.path().join("staged-trust");
    run(
        &root,
        &[
            "stage",
            folder.to_str().unwrap(),
            staged.to_str().unwrap(),
            "--async",
        ],
        0,
    );
    run(
        &root,
        &[
            "templates",
            "add",
            folder.to_str().unwrap(),
            "--alias",
            "chosen",
            "--trust",
        ],
        0,
    );
    let record_path = fs::read_dir(support::staged_dir(root.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(record["named"], false);
    record.as_object_mut().unwrap().remove("named");
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    run(
        &root,
        &[
            "apply",
            folder.to_str().unwrap(),
            root.path().join("trusted-folder").to_str().unwrap(),
            "--answers",
            &envelope(&root, &folder_formal, "{}"),
        ],
        3,
    );
    run(
        &root,
        &[
            "apply",
            folder.to_str().unwrap(),
            root.path().join("trusted-folder").to_str().unwrap(),
            "--answers",
            &envelope(&root, &folder_formal, "{}"),
            "--trust",
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("trusted-folder/hook.txt")).unwrap(),
        "ok"
    );
    run(
        &root,
        &[
            "apply",
            "hooked",
            root.path().join("trusted").to_str().unwrap(),
            "--answers",
            &envelope(&root, &folder_formal, "{}"),
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("trusted/hook.txt")).unwrap(),
        "ok"
    );
    run(
        &root,
        &[
            "apply",
            "chosen",
            root.path().join("alias").to_str().unwrap(),
            "--answers",
            &envelope(&root, &folder_formal, "{}"),
        ],
        0,
    );
    run(&root, &["apply", staged.to_str().unwrap()], 3);
    run(&root, &["apply", staged.to_str().unwrap(), "--trust"], 0);
    assert_eq!(fs::read_to_string(staged.join("hook.txt")).unwrap(), "ok");
    let named_staged = root.path().join("named-staged");
    run(
        &root,
        &["stage", "hooked", named_staged.to_str().unwrap(), "--async"],
        0,
    );
    let record_path = fs::read_dir(support::staged_dir(root.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let record: Value = serde_json::from_slice(&fs::read(record_path).unwrap()).unwrap();
    assert_eq!(record["named"], true);
    run(&root, &["apply", named_staged.to_str().unwrap()], 0);
    assert_eq!(
        fs::read_to_string(named_staged.join("hook.txt")).unwrap(),
        "ok"
    );
    let revoked = root.path().join("revoked-trust");
    run(
        &root,
        &["stage", "hooked", revoked.to_str().unwrap(), "--async"],
        0,
    );
    run(&root, &["templates", "remove", "hooked"], 0);
    run(&root, &["apply", revoked.to_str().unwrap()], 3);
    let discovered = root.path().join(".templates/discovered");
    fs::create_dir_all(discovered.join("template")).unwrap();
    fs::write(
        discovered.join("template.yml"),
        "name: discovered\nhooks:\n  - run: [/bin/sh, -c, 'printf ok > hook.txt']\n",
    )
    .unwrap();
    fs::write(discovered.join("template/file.txt"), "file").unwrap();
    fs::write(
        root.path().join(".templates/templates.yml"),
        format!(
            "templates:\n  {}:\n    aliases: [local-name]\n",
            discovered.canonicalize().unwrap().display()
        ),
    )
    .unwrap();
    let discovered_formal = support::formal_name(&discovered);
    run(
        &root,
        &[
            "apply",
            "local-name",
            root.path().join("untrusted").to_str().unwrap(),
            "--answers",
            &envelope(&root, &discovered_formal, "{}"),
        ],
        3,
    );
    run(
        &root,
        &[
            "apply",
            "local-name",
            root.path().join("allowed").to_str().unwrap(),
            "--answers",
            &envelope(&root, &discovered_formal, "{}"),
            "--trust",
        ],
        0,
    );
    assert_eq!(
        fs::read_to_string(root.path().join("allowed/hook.txt")).unwrap(),
        "ok"
    );
}

#[cfg(unix)]
#[test]
fn trusted_git_address_requires_explicit_trust_even_when_registered() {
    let root = TempDir::new().unwrap();
    let repo = root.path().join("source");
    fs::create_dir_all(repo.join("template")).unwrap();
    fs::write(
        repo.join("template.yml"),
        "name: hooked\nhooks:\n  - run: [/bin/sh, -c, 'printf ok > hook.txt']\n",
    )
    .unwrap();
    fs::write(repo.join("template/file.txt"), "file").unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Sample"]);
    git(&repo, &["config", "user.email", "sample@example.invalid"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    let address = format!("file://{}", repo.display());
    run(
        &root,
        &["templates", "add", &address, "--alias", "chosen", "--trust"],
        0,
    );
    // Every name here (the git address, its alias `chosen`, and its short name
    // `hooked`) resolves to the same formal identity — the address — which the
    // scripted envelope must name.
    let apply = |template: &str, target: &str, extra: &[&str], exit| {
        let target = root.path().join(target);
        let ans = envelope(&root, &address, "{}");
        let mut args = vec![
            "apply",
            template,
            target.to_str().unwrap(),
            "--answers",
            ans.as_str(),
        ];
        args.extend_from_slice(extra);
        run(&root, &args, exit);
        target
    };
    apply(&address, "direct", &[], 3);
    apply(&address, "allowed", &["--trust"], 0);
    apply("chosen", "alias", &[], 0);
    apply("hooked", "short", &[], 0);
    let staged = root.path().join("staged-git");
    run(
        &root,
        &["stage", &address, staged.to_str().unwrap(), "--async"],
        0,
    );
    run(&root, &["apply", staged.to_str().unwrap()], 3);
    run(&root, &["apply", staged.to_str().unwrap(), "--trust"], 0);
    let named = root.path().join("staged-name");
    run(
        &root,
        &["stage", "chosen", named.to_str().unwrap(), "--async"],
        0,
    );
    run(&root, &["apply", named.to_str().unwrap()], 0);
}

#[test]
fn named_template_resumes_its_staged_interview_by_formal_name() {
    let root = TempDir::new().unwrap();
    let (url, _) = repository(&root);
    write_config(
        &root,
        &format!("hosts:\n  local: {}\n", support::file_url(root.path())),
    );
    run(
        &root,
        &[
            "templates",
            "add",
            &format!("{url}#one"),
            "--alias",
            "chosen",
        ],
        0,
    );
    let formal_one = formal(&root, "chosen");
    let complete = root.path().join("complete");
    let complete = complete.to_str().unwrap();
    run(&root, &["stage", "chosen", complete, "--async"], 4);
    run(
        &root,
        &[
            "continue",
            complete,
            &envelope(&root, &formal_one, r#"{"label":"Staged"}"#),
        ],
        0,
    );
    run(&root, &["apply", "local:owner/repo#one", complete], 0);
    assert_eq!(
        fs::read_to_string(root.path().join("complete/result.txt")).unwrap(),
        "old Staged"
    );

    let incomplete = root.path().join("incomplete");
    let incomplete = incomplete.to_str().unwrap();
    run(
        &root,
        &["stage", "local:owner/repo#one", incomplete, "--async"],
        4,
    );
    // Naming the staged template (by its alias) resumes that interview: the person
    // route `apply TEMPLATE PATH` prompts, so with no terminal it refuses (exit 1)
    // and names the agent route to answer the current batch — not a mismatch.
    let resumed = run(&root, &["apply", "chosen", incomplete], 1);
    assert!(
        String::from_utf8_lossy(&resumed.stderr)
            .contains(&format!("toha continue {incomplete} <ANSWERS>")),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let other = format!("{url}#two");
    let refused = run(&root, &["apply", &other, incomplete], 1);
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(
        error.contains("local:owner/repo#one")
            && error.contains("local:owner/repo#two")
            && error.contains(&format!(
                "toha apply {} {incomplete}",
                support::shell_quoted(&other)
            )),
        "{error}"
    );
}

#[test]
fn ambiguous_name_names_the_same_command_with_each_formal_name() {
    let root = TempDir::new().unwrap();
    let (url, _) = repository(&root);
    write_config(
        &root,
        &format!("hosts:\n  local: {}\n", support::file_url(root.path())),
    );
    run(&root, &["templates", "add", &url], 0);
    let answers = root.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let answers = answers.to_str().unwrap();
    let target = root.path().join("target");
    let target = target.to_str().unwrap();
    let formal = ["local:owner/repo#one", "local:owner/repo#two"];
    type Retry<'a> = Box<dyn Fn(&str) -> String + 'a>;
    let cases: [(Vec<&str>, Retry); 4] = [
        (
            vec!["apply", "--answers", answers, "same", target],
            Box::new(|formal| {
                format!(
                    "toha apply --answers {answers} {} {target}",
                    support::shell_quoted(formal)
                )
            }),
        ),
        (
            vec!["stage", "same", target, "--async"],
            Box::new(|formal| {
                format!(
                    "toha stage {} {target} --async",
                    support::shell_quoted(formal)
                )
            }),
        ),
        (
            vec!["templates", "remove", "same"],
            Box::new(|formal| format!("toha templates remove {}", support::shell_quoted(formal))),
        ),
        (
            vec!["templates", "update", "same"],
            Box::new(|formal| format!("toha templates update {}", support::shell_quoted(formal))),
        ),
    ];
    for (args, retry) in cases {
        let output = run(&root, &args, 5);
        // The scripted route reports an ambiguous template with an `error`
        // document (kind "ambiguous", exit 5) whose `commands` list the retry per
        // match; the agent and registry routes name them on standard error.
        let haystack = if args[0] == "apply" {
            let document = document(&output);
            assert_eq!(document["status"], "error");
            assert_eq!(document["kind"], "ambiguous");
            document["commands"]
                .as_array()
                .unwrap_or_else(|| panic!("ambiguous document has no commands: {document}"))
                .iter()
                .map(|command| command.as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::from_utf8_lossy(&output.stderr).into_owned()
        };
        for formal in formal {
            let command = retry(formal);
            assert!(
                haystack.contains(&command),
                "{args:?}: does not name `{command}`:\n{haystack}"
            );
        }
    }
}
