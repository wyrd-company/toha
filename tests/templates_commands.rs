#[allow(dead_code)]
mod support;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn run(root: &TempDir, args: &[&str]) -> std::process::Output {
    support::isolated_command(root.path())
        .args(args)
        .current_dir(root.path())
        .output()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
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
fn repo(root: &TempDir) -> String {
    let path = root.path().join("remote");
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.invalid"]);
    git(&path, &["config", "user.name", "Test"]);
    for (dir, name) in [("one", "common"), ("two", "other")] {
        fs::create_dir_all(path.join(dir).join("template")).unwrap();
        fs::write(path.join(dir).join("template/.gitkeep"), "").unwrap();
        fs::write(
            path.join(dir).join("template.yml"),
            format!("name: {name}\ninterview: []\n"),
        )
        .unwrap();
    }
    git(&path, &["add", "."]);
    git(&path, &["commit", "-m", "initial"]);
    support::file_url(&path)
}
fn assert_exit(output: &std::process::Output, code: i32) -> String {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}
#[test]
fn add_list_alias_remove_and_shared_clone() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let first = run(&root, &["templates", "add", &url]);
    assert_exit(&first, 0);
    let list = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let json: serde_json::Value = serde_json::from_str(&list).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
    let first_name = format!("{url}#one");
    let second_name = format!("{url}#two");
    assert_exit(&run(&root, &["templates", "alias", &first_name, "mine"]), 0);
    assert_exit(
        &run(&root, &["templates", "alias", &second_name, "mine"]),
        1,
    );
    assert_exit(&run(&root, &["templates", "alias", "-r", "mine"]), 0);
    assert_exit(&run(&root, &["templates", "alias", "-r", "mine"]), 1);
    assert_exit(&run(&root, &["templates", "remove", &first_name]), 0);
    let list = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let json: serde_json::Value = serde_json::from_str(&list).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_exit(&run(&root, &["templates", "remove", &second_name]), 0);
    assert_exit(&run(&root, &["templates", "remove", &second_name]), 1);
}
#[test]
fn add_selected_trust_and_update_branch() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(
        &run(
            &root,
            &[
                "templates",
                "add",
                &format!("{url}#one"),
                "-a",
                "selected",
                "--trust",
            ],
        ),
        0,
    );
    let first = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let json: serde_json::Value = serde_json::from_str(&first).unwrap();
    let old_commit = json[0]["commit"].as_str().unwrap();
    assert_eq!(json[0]["trusted"], true);
    fs::write(
        root.path().join("remote/one/template.yml"),
        "name: common\ninterview: []\ndescription: changed\n",
    )
    .unwrap();
    git(&root.path().join("remote"), &["add", "."]);
    git(&root.path().join("remote"), &["commit", "-m", "move"]);
    assert_exit(&run(&root, &["templates", "update", "selected"]), 0);
    let updated = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_ne!(json[0]["commit"].as_str().unwrap(), old_commit);
    assert_eq!(json[0]["aliases"], serde_json::json!(["selected"]));
    assert_eq!(json[0]["trusted"], true);
}
#[test]
fn readd_sparse_entry_persists_alias_and_trust() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let address = format!("{url}#one");
    assert_exit(&run(&root, &["templates", "add", &address]), 0);
    let registry_path = support::user_data_dir(root.path()).join("templates.yml");
    let mut sparse: serde_json::Value =
        serde_norway::from_str(&fs::read_to_string(&registry_path).unwrap()).unwrap();
    let entry = sparse["templates"][&address].as_object_mut().unwrap();
    entry.remove("aliases");
    entry.remove("approval");
    fs::write(&registry_path, serde_norway::to_string(&sparse).unwrap()).unwrap();
    assert_exit(
        &run(
            &root,
            &["templates", "add", &address, "-a", "kept", "--trust"],
        ),
        0,
    );
    let written: serde_json::Value =
        serde_norway::from_str(&fs::read_to_string(&registry_path).unwrap()).unwrap();
    assert_eq!(
        written["templates"][&address]["aliases"],
        serde_json::json!(["kept"])
    );
    // Re-adding with --trust persists an approval digest for the template.
    assert!(
        written["templates"][&address]["approval"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")),
        "{written}"
    );
}
#[test]
fn add_rejects_multiple_with_alias_and_bad_template() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(&run(&root, &["templates", "add", &url, "-a", "single"]), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&assert_exit(
            &run(&root, &["templates", "list", "--json"]),
            0
        ))
        .unwrap()
        .as_array()
        .unwrap()
        .len(),
        0
    );
    fs::write(
        root.path().join("remote/two/template.yml"),
        "name: INVALID\n",
    )
    .unwrap();
    git(&root.path().join("remote"), &["add", "."]);
    git(&root.path().join("remote"), &["commit", "-m", "invalid"]);
    assert_exit(&run(&root, &["templates", "add", &url]), 1);
}
#[test]
fn folder_and_ambiguous_short_name() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(&run(&root, &["templates", "add", &format!("{url}#one")]), 0);
    let folder = root.path().join("folder");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template.yml"), "name: common\ninterview: []\n").unwrap();
    assert!(
        assert_exit(
            &run(&root, &["templates", "add", folder.to_str().unwrap()]),
            0
        )
        .contains("shared short name")
    );
    let output = run(&root, &["templates", "remove", "common"]);
    assert_exit(&output, 5);
    assert!(String::from_utf8_lossy(&output.stderr).contains("toha templates alias"));
}

#[cfg(windows)]
#[test]
fn registered_folder_canonical_formal_name_resolves_as_name() {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("folder");
    fs::create_dir_all(folder.join("template")).unwrap();
    fs::write(folder.join("template.yml"), "name: sample\ninterview: []\n").unwrap();
    fs::write(folder.join("template/result.txt"), "result").unwrap();

    assert_exit(
        &run(&root, &["templates", "add", folder.to_str().unwrap()]),
        0,
    );
    let formal = folder
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(formal.starts_with(r"\\?\"), "{formal}");
    let listed: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "--json"]),
        0,
    ))
    .unwrap();
    assert_eq!(listed[0]["formal_name"], formal);

    let target = root.path().join("target");
    assert_exit(
        &run(
            &root,
            &["stage", &formal, target.to_str().unwrap(), "--async"],
        ),
        0,
    );
    let record_path = fs::read_dir(support::staged_dir(root.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(record_path).unwrap()).unwrap();
    assert_eq!(record["named"], true);
    assert_exit(&run(&root, &["apply", target.to_str().unwrap()]), 0);
    assert_eq!(
        fs::read_to_string(target.join("result.txt")).unwrap(),
        "result"
    );
}
#[test]
fn pinned_tag_and_commit_do_not_move() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let remote = root.path().join("remote");
    let commit = git(&remote, &["rev-parse", "HEAD"]);
    git(&remote, &["tag", "release"]);
    assert_exit(
        &run(&root, &["templates", "add", &format!("{url}@release#one")]),
        0,
    );
    assert_exit(
        &run(
            &root,
            &["templates", "add", &format!("{url}@{}#two", &commit[..12])],
        ),
        0,
    );
    fs::write(
        remote.join("one/template.yml"),
        "name: common\ninterview: []\ndescription: next\n",
    )
    .unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "next"]);
    assert_exit(&run(&root, &["templates", "update"]), 0);
    let output = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
    assert!(
        json.as_array()
            .unwrap()
            .iter()
            .all(|e| e["commit"] == commit)
    );
}
#[test]
fn configured_file_host_shorthand_installs_and_round_trips() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let owner = root.path().join("owner");
    fs::create_dir(&owner).unwrap();
    fs::rename(root.path().join("remote"), owner.join("remote")).unwrap();
    let config = root.path().join("config/toha/config.yml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        &config,
        format!("hosts:\n  local: '{}'\n", support::file_url(root.path())),
    )
    .unwrap();
    assert_exit(
        &run(
            &root,
            &["templates", "add", "local:owner/remote#one", "--trust"],
        ),
        0,
    );
    let list = assert_exit(&run(&root, &["templates", "list", "--json"]), 0);
    let rows: serde_json::Value = serde_json::from_str(&list).unwrap();
    assert_eq!(rows[0]["formal_name"], "local:owner/remote#one");
    assert_eq!(
        rows[0]["source"],
        format!("{}/owner/remote", url.trim_end_matches("/remote"))
    );
    assert_eq!(rows[0]["trusted"], true);
}
#[test]
fn command_errors_and_usage_codes() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(&run(&root, &["templates", "add", &url, "-a", "single"]), 1);
    assert_exit(
        &run(&root, &["templates", "add", &format!("{url}#missing")]),
        1,
    );
    assert_exit(
        &run(
            &root,
            &["templates", "add", &format!("{url}#one"), "-a", "INVALID"],
        ),
        1,
    );
    assert_exit(&run(&root, &["templates", "alias"]), 2);
    assert_exit(&run(&root, &["templates", "alias", "missing"]), 2);
    assert_exit(&run(&root, &["templates", "list", "-l", "-u"]), 2);
    assert_exit(&run(&root, &["templates", "update"]), 1);
    assert_exit(&run(&root, &["templates", "remove", "missing"]), 1);
}
#[test]
fn layer_lists_and_local_aliases_of_discovered_templates() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(
        &run(
            &root,
            &["templates", "add", &format!("{url}#one"), "-a", "user-name"],
        ),
        0,
    );
    let local = root.path().join(".templates");
    let discovered = local.join("sample");
    fs::create_dir_all(discovered.join("template")).unwrap();
    fs::write(
        discovered.join("template.yml"),
        "name: sample\ninterview: []\n",
    )
    .unwrap();
    fs::write(local.join("templates.yml"), format!("templates:\n  '{}#one':\n    aliases: [local-name]\n  '{}':\n    aliases: [found-name]\n", url, discovered.canonicalize().unwrap().display())).unwrap();
    let merged: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "--json"]),
        0,
    ))
    .unwrap();
    assert_eq!(merged.as_array().unwrap().len(), 2);
    assert!(
        merged
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["layer"] == "discovered"
                && entry["aliases"] == serde_json::json!(["found-name"]))
    );
    let user: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    assert_eq!(user[0]["aliases"], serde_json::json!(["user-name"]));
    let local: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-l", "--json"]),
        0,
    ))
    .unwrap();
    assert_eq!(local.as_array().unwrap().len(), 2);
    assert!(
        local
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["aliases"] == serde_json::json!(["local-name"]))
    );
}
#[test]
fn unknown_local_alias_warns_once_and_commands_continue() {
    let root = TempDir::new().unwrap();
    let local = root.path().join(".templates");
    fs::create_dir_all(&local).unwrap();
    fs::write(
        local.join("templates.yml"),
        "templates:\n  absent:\n    aliases: [spare]\n",
    )
    .unwrap();
    let list = run(&root, &["templates", "list", "--json"]);
    assert_exit(&list, 0);
    assert_eq!(
        String::from_utf8_lossy(&list.stderr),
        "warning: local alias spare names absent, which is not installed\n"
    );
    let url = repo(&root);
    let add = run(&root, &["templates", "add", &url]);
    assert_exit(&add, 0);
    assert_eq!(
        String::from_utf8_lossy(&add.stderr),
        "warning: local alias spare names absent, which is not installed\n"
    );
}
#[cfg(unix)]
#[test]
fn add_rejects_template_symlink_outside_source() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("source");
    let outside = root.path().join("outside");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("template.yml"),
        "name: outside\ninterview: []\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
    let output = run(&root, &["templates", "add", source.to_str().unwrap()]);
    assert_exit(&output, 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("leaves repository"));
    let list: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    assert!(list.as_array().unwrap().is_empty());
}
#[test]
fn failed_update_keeps_prior_clones_and_registry() {
    let root = TempDir::new().unwrap();
    let first_url = repo(&root);
    let second_source = root.path().join("second");
    fs::create_dir(&second_source).unwrap();
    git(&second_source, &["init", "-b", "main"]);
    git(
        &second_source,
        &["config", "user.email", "test@example.invalid"],
    );
    git(&second_source, &["config", "user.name", "Test"]);
    fs::create_dir(second_source.join("template")).unwrap();
    fs::write(second_source.join("template/.gitkeep"), "").unwrap();
    fs::write(
        second_source.join("template.yml"),
        "name: second\ninterview: []\n",
    )
    .unwrap();
    git(&second_source, &["add", "."]);
    git(&second_source, &["commit", "-m", "initial"]);
    let second_url = support::file_url(&second_source);
    assert_exit(
        &run(&root, &["templates", "add", &format!("{first_url}#one")]),
        0,
    );
    assert_exit(&run(&root, &["templates", "add", &second_url]), 0);
    let before = assert_exit(&run(&root, &["templates", "list", "-u", "--json"]), 0);
    fs::write(
        root.path().join("remote/one/template.yml"),
        "name: common\ninterview: []\ndescription: moved\n",
    )
    .unwrap();
    git(&root.path().join("remote"), &["add", "."]);
    git(&root.path().join("remote"), &["commit", "-m", "move"]);
    fs::rename(&second_source, root.path().join("unavailable")).unwrap();
    assert_exit(&run(&root, &["templates", "update"]), 1);
    let after = assert_exit(&run(&root, &["templates", "list", "-u", "--json"]), 0);
    assert_eq!(before, after);
    let rows: serde_json::Value = serde_json::from_str(&after).unwrap();
    let first = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "common")
        .unwrap();
    let installed = Path::new(first["path"].as_str().unwrap());
    assert!(
        !fs::read_to_string(installed.join("template.yml"))
            .unwrap()
            .contains("moved")
    );
}
#[test]
fn readd_keeps_existing_alias_and_trust_and_warns_on_short_name() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let first = format!("{url}#one");
    let second = format!("{url}#two");
    assert_exit(
        &run(
            &root,
            &["templates", "add", &first, "-a", "kept", "--trust"],
        ),
        0,
    );
    assert_exit(&run(&root, &["templates", "add", &second]), 0);
    let warning = assert_exit(&run(&root, &["templates", "alias", &second, "common"]), 0);
    assert!(warning.contains("warning"));
    assert_exit(&run(&root, &["templates", "add", &first]), 0);
    let rows: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    let first_row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["formal_name"] == first)
        .unwrap();
    assert_eq!(first_row["aliases"], serde_json::json!(["kept"]));
    assert_eq!(first_row["trusted"], true);
}
#[test]
fn explicit_branch_ref_moves_on_update() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let name = format!("{url}@main#one");
    assert_exit(&run(&root, &["templates", "add", &name]), 0);
    let old = git(&root.path().join("remote"), &["rev-parse", "HEAD"]);
    fs::write(
        root.path().join("remote/one/template.yml"),
        "name: common\ninterview: []\ndescription: changed\n",
    )
    .unwrap();
    git(&root.path().join("remote"), &["add", "."]);
    git(&root.path().join("remote"), &["commit", "-m", "changed"]);
    assert_exit(&run(&root, &["templates", "update", &name]), 0);
    let rows: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    assert_ne!(rows[0]["commit"], old);
    assert_eq!(rows[0]["ref"], "main");
}
#[test]
fn update_two_dotted_install_keys() {
    let root = TempDir::new().unwrap();
    let mut addresses = Vec::new();
    for suffix in ["one", "two"] {
        let dir = root.path().join(format!("repo.{suffix}"));
        fs::create_dir_all(dir.join("template")).unwrap();
        fs::write(dir.join("template/.gitkeep"), "").unwrap();
        git(&dir, &["init", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.invalid"]);
        git(&dir, &["config", "user.name", "Test"]);
        fs::write(
            dir.join("template.yml"),
            format!("name: {suffix}\ninterview: []\n"),
        )
        .unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-m", "initial"]);
        let address = support::file_url(&dir);
        assert_exit(&run(&root, &["templates", "add", &address]), 0);
        addresses.push((dir, address));
    }
    let before: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    for (dir, _) in &addresses {
        fs::write(
            dir.join("template.yml"),
            format!(
                "name: {}\ninterview: []\ndescription: moved\n",
                dir.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches("repo.")
            ),
        )
        .unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "move"]);
    }
    assert_exit(&run(&root, &["templates", "update"]), 0);
    let after: serde_json::Value = serde_json::from_str(&assert_exit(
        &run(&root, &["templates", "list", "-u", "--json"]),
        0,
    ))
    .unwrap();
    for (old, new) in before
        .as_array()
        .unwrap()
        .iter()
        .zip(after.as_array().unwrap())
    {
        assert_ne!(old["commit"], new["commit"]);
        assert!(
            fs::read_to_string(new["path"].as_str().unwrap().to_string() + "/template.yml")
                .unwrap()
                .contains("moved")
        );
    }
}

fn assert_stderr_names(output: &std::process::Output, commands: &[String]) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    for command in commands {
        assert!(
            stderr.contains(command.as_str()),
            "stderr does not name `{command}`:\n{stderr}"
        );
    }
}

#[test]
fn add_of_a_name_names_list_and_the_address_forms() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["templates", "add", "sample"]);
    assert_exit(&output, 1);
    assert_stderr_names(
        &output,
        &[
            "toha templates list".into(),
            "toha templates add <ADDRESS>".into(),
        ],
    );
    fs::create_dir(root.path().join("sample")).unwrap();
    let output = run(&root, &["templates", "add", "sample"]);
    assert_exit(&output, 1);
    assert_stderr_names(&output, &["toha templates add ./sample".into()]);
}

#[test]
fn alias_for_several_templates_names_one_add_per_template() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    let output = run(&root, &["templates", "add", &url, "--alias", "mine"]);
    assert_exit(&output, 1);
    assert_stderr_names(
        &output,
        &[
            format!(
                "toha templates add --alias mine {}",
                support::shell_quoted(&format!("{url}#one"))
            ),
            format!(
                "toha templates add --alias mine {}",
                support::shell_quoted(&format!("{url}#two"))
            ),
        ],
    );
}

#[test]
fn alias_in_use_names_the_remove_command() {
    let root = TempDir::new().unwrap();
    let url = repo(&root);
    assert_exit(&run(&root, &["templates", "add", &url]), 0);
    let first = format!("{url}#one");
    let second = format!("{url}#two");
    assert_exit(&run(&root, &["templates", "alias", &first, "mine"]), 0);
    let output = run(&root, &["templates", "alias", &second, "mine"]);
    assert_exit(&output, 1);
    assert_stderr_names(
        &output,
        &[
            "alias is in use".into(),
            "toha templates alias --remove mine".into(),
            format!(
                "toha templates alias {} mine",
                support::shell_quoted(&second)
            ),
        ],
    );
}

#[test]
fn remove_of_a_template_outside_the_user_registry_names_list() {
    let root = TempDir::new().unwrap();
    let discovered = root.path().join(".templates/discovered");
    fs::create_dir_all(discovered.join("template")).unwrap();
    fs::write(discovered.join("template.yml"), "name: discovered\n").unwrap();
    let output = run(&root, &["templates", "remove", "discovered"]);
    assert_exit(&output, 1);
    assert_stderr_names(
        &output,
        &[
            "not installed in user registry: discovered".into(),
            "toha templates list --json".into(),
        ],
    );
}

/// Every command named for an ambiguous folder template, or for its hooks,
/// is accepted by the command it names.
#[test]
fn suggested_commands_for_folder_templates_are_accepted() {
    let root = TempDir::new().unwrap();
    let mut folders = Vec::new();
    for name in ["first", "second"] {
        let folder = root.path().join(name);
        fs::create_dir_all(folder.join("template")).unwrap();
        fs::write(
            folder.join("template.yml"),
            "name: same\nhooks:\n  - run: [tool, call]\n",
        )
        .unwrap();
        fs::write(folder.join("template/file.txt"), name).unwrap();
        let address = support::folder_address(&folder.canonicalize().unwrap());
        assert_exit(&run(&root, &["templates", "add", &address]), 0);
        folders.push(address);
    }
    fs::write(root.path().join("answers.json"), "{}").unwrap();
    let rerun = |args: Vec<String>, code: i32| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = run(&root, &args);
        assert_exit(&output, code);
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let stderr = |args: &[&str], code: i32| {
        let output = run(&root, args);
        assert_exit(&output, code);
        String::from_utf8_lossy(&output.stderr).into_owned()
    };

    let ambiguous = stderr(&["stage", "same", "staged", "--async"], 5);
    rerun(support::suggested(&ambiguous, "to use "), 0);

    let ambiguous = stderr(&["apply", "--answers", "answers.json", "same", "out"], 5);
    rerun(support::suggested(&ambiguous, "to use "), 3);

    let ambiguous = stderr(&["templates", "alias", "same", "picked"], 5);
    rerun(support::suggested(&ambiguous, "to use "), 0);
    let mut alias = support::suggested(&ambiguous, "an alias: ");
    *alias.last_mut().unwrap() = "chosen".into();
    rerun(alias, 0);
    let hooks = stderr(
        &["apply", "--answers", "answers.json", "chosen", "named"],
        3,
    );
    rerun(support::suggested(&hooks, "to trust "), 0);

    let ambiguous = stderr(&["templates", "remove", "same"], 5);
    rerun(support::suggested(&ambiguous, "to use "), 0);
    assert_eq!(folders.len(), 2);
}
