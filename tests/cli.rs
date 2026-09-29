// ---
// relationships:
//   implements: architecture
// ---
#[allow(dead_code)]
mod support;

#[test]
fn every_fixture_through_cli() {
    for fixture in support::fixtures() {
        let expect = support::expectation(&fixture);
        if !expect.cli
            || (!expect.hooks.is_empty()
                && fixture.file_name().unwrap() != "hooks-untrusted"
                && fixture.file_name().unwrap() != "script-cli")
        {
            continue;
        }
        let target = tempfile::tempdir().unwrap();
        let isolation = tempfile::tempdir().unwrap();
        support::copy_tree(&fixture.join("existing"), target.path());
        let template_folder = fixture.join("template").canonicalize().unwrap();
        // The scripted route (`apply TEMPLATE PATH --answers FILE`) requires the
        // identity-bearing envelope naming the formal template.
        let formal = support::formal_name(&template_folder);
        let answers =
            support::envelope_file(isolation.path(), &formal, &fixture.join("answers.json"));
        let mut command = support::isolated_command(isolation.path());
        command
            .arg("apply")
            .arg(support::folder_address(&template_folder))
            .arg(target.path())
            .arg("--answers")
            .arg(&answers)
            // The hidden TOHA_NOW seed hook fixes the clock for CLI fixtures.
            .env("TOHA_NOW", expect.now.clone());
        if expect.options.force {
            command.arg("--force");
        }
        if expect.options.trust {
            command.arg("--trust");
        }
        if expect.options.dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(support::expectation(&fixture).exit.into()),
            "{}: {}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        // Every scripted outcome is exactly one JSON result document on stdout.
        let document = support::first_document(&output.stdout);
        // A `planned` document reports untrusted hooks structurally (exit 3,
        // already asserted; the crate harness carries the "--trust" message).
        // Every other fault is an `error` document whose decoded `message`, or a
        // `questions` document's per-question errors, carries the diagnostic.
        if document["status"] != "planned" {
            let diagnostic = support::diagnostic_text(&document);
            for part in &expect.error_contains {
                assert!(
                    diagnostic.contains(part),
                    "{}: {diagnostic}",
                    fixture.display()
                );
            }
        }
        // Plan-line expectations match the plan text reconstructed from the
        // document's structured files and hooks.
        let plan_text = support::plan_text_from_document(&document);
        for part in &expect.stdout_contains {
            assert!(
                plan_text.contains(part),
                "{}: {plan_text}",
                fixture.display()
            );
        }
        // The interview messages lead the document's `messages`.
        let messages = support::document_messages(&document);
        assert_eq!(
            messages.get(..expect.messages.len()),
            Some(expect.messages.as_slice()),
            "{}: {document}",
            fixture.display()
        );
        if fixture.join("expected").exists() {
            support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
        } else {
            assert!(
                std::fs::read_dir(target.path()).unwrap().next().is_none(),
                "{}",
                fixture.display()
            );
        }
    }
}

#[test]
fn commands_without_documents_require_a_terminal() {
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let template = std::path::Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    for args in [
        vec![
            "stage".to_string(),
            support::folder_address(&template),
            target.path().display().to_string(),
        ],
        vec![
            "apply".to_string(),
            support::folder_address(&template),
            target.path().display().to_string(),
        ],
    ] {
        let output = support::isolated_command(isolation.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        let template = support::folder_address(&template);
        let target = target.path().display();
        for command in [
            format!("toha stage {template} {target} --async"),
            format!("toha apply --answers <FILE> {template} {target}"),
        ] {
            assert!(
                stderr.contains(&command),
                "stderr does not name `{command}`:\n{stderr}"
            );
        }
    }
    let staged = support::isolated_command(isolation.path())
        .args(["stage", &support::folder_address(&template)])
        .arg(target.path())
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(4));
    let output = support::isolated_command(isolation.path())
        .arg("continue")
        .arg(target.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "toha continue {} <ANSWERS>",
        target.path().display()
    )));
}

#[cfg(unix)]
#[test]
fn terminal_text_and_confirm_write_rendered_file() {
    use expectrl::{Expect, Session};
    let folder = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("template")).unwrap();
    std::fs::write(folder.path().join("template.yml"), "name: sample\nsource: template\ninterview:\n  - id: label\n    type: text\n    prompt: Label?\n    required: true\n  - id: enabled\n    type: confirm\n    prompt: Enabled?\n").unwrap();
    std::fs::write(
        folder.path().join("template/result.txt"),
        "{{ label }} {{ enabled }}",
    )
    .unwrap();
    let mut command = support::isolated_command(isolation.path());
    command.arg("apply").arg(folder.path()).arg(target.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Label?").unwrap();
    session.send_line("sample").unwrap();
    session.expect("Enabled?").unwrap();
    session.send_line("y").unwrap();
    session.expect("result.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(target.path().join("result.txt")).unwrap(),
        "sample True"
    );
}

#[cfg(unix)]
#[test]
fn cancel_during_continue_preserves_staged_record() {
    use expectrl::{Expect, Session};
    let fixture = std::path::Path::new("tests/fixtures/text-basic/template")
        .canonicalize()
        .unwrap();
    let target = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let stage = support::isolated_command(state.path())
        .arg("stage")
        .arg(&fixture)
        .arg(target.path())
        .arg("--async")
        .env("XDG_STATE_HOME", state.path())
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4));
    let record = std::fs::read_dir(support::staged_dir(state.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let before = std::fs::read(&record).unwrap();
    let mut command = support::isolated_command(state.path());
    command
        .arg("continue")
        .arg(target.path())
        .env("XDG_STATE_HOME", state.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Name?").unwrap();
    session.send("\u{1b}").unwrap();
    session.expect("Operation was canceled").unwrap();
    assert_eq!(std::fs::read(record).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn continue_prints_prior_batch_messages_once() {
    use expectrl::{Expect, Session};
    let folder = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("template")).unwrap();
    std::fs::write(folder.path().join("template.yml"), "name: sample\ninterview:\n  - message: Start\n  - { id: first, type: text, prompt: First? }\n  - message: 'Thanks {{ first }}'\n  - { id: second, type: text, prompt: Second? }\n  - message: 'Done {{ second }}'\n").unwrap();
    let stage = support::isolated_command(state.path())
        .arg("stage")
        .arg(folder.path())
        .arg(target.path())
        .arg("--async")
        .env("XDG_STATE_HOME", state.path())
        .output()
        .unwrap();
    assert_eq!(stage.status.code(), Some(4));
    let answers = folder.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(
            &support::formal_name(folder.path()),
            &serde_json::json!({"first": "Ada"}),
        ),
    )
    .unwrap();
    let first = support::isolated_command(state.path())
        .arg("continue")
        .arg(target.path())
        .arg(&answers)
        .env("XDG_STATE_HOME", state.path())
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(4));

    let mut command = support::isolated_command(state.path());
    command
        .arg("continue")
        .arg(target.path())
        .env("XDG_STATE_HOME", state.path());
    let mut session = Session::spawn(command).unwrap();
    session.expect("Thanks Ada").unwrap();
    session.expect("Second?").unwrap();
    session.send_line("yes").unwrap();
    let final_message = session.expect("Done yes").unwrap();
    assert!(!String::from_utf8_lossy(final_message.before()).contains("Thanks Ada"));
}

#[test]
fn default_render_failure_exits_one_without_writing() {
    let fixture = std::path::Path::new("tests/fixtures/err-default-render");
    let template = fixture.join("template");
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let answers = support::envelope_file(
        isolation.path(),
        &support::formal_name(&template),
        &fixture.join("answers.json"),
    );
    let output = support::isolated_command(isolation.path())
        .arg("apply")
        .arg(support::folder_address(&template.canonicalize().unwrap()))
        .arg(target.path())
        .arg("--answers")
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    // The render fault is an `error` document whose decoded message names the
    // failing default.
    let message = support::diagnostic_text(&support::first_document(&output.stdout));
    assert!(message.contains("second.default"), "{message}");
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn trusted_script_runs_in_target() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("template.yml"),
        "name: script-cli\nsource: .\nhooks:\n  - script: write.sh\n",
    )
    .unwrap();
    let script = folder.path().join("write.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf marker > marker.txt\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let answers = folder.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&support::formal_name(folder.path()), &serde_json::json!({})),
    )
    .unwrap();
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let output = support::isolated_command(isolation.path())
        .args([
            "apply",
            folder.path().to_str().unwrap(),
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
            "--trust",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("marker.txt")).unwrap(),
        "marker"
    );
}

#[cfg(unix)]
#[test]
fn trusted_hook_runs_once_per_item() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("template.yml"),
        concat!(
            "name: script-each\n",
            "source: .\n",
            "ignore: [append.sh, answers.json]\n",
            "data:\n",
            "  items:\n",
            "    - { name: alpha, value: one }\n",
            "    - { name: beta, value: two }\n",
            "    - { name: gamma, value: three }\n",
            "hooks:\n",
            "  - each: \"items as item\"\n",
            "    script: append.sh\n",
            "    args: [ \"{{ item.name }}\", \"{{ item.value }}\" ]\n",
        ),
    )
    .unwrap();
    let script = folder.path().join("append.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s=%s\\n' \"$1\" \"$2\" >> log.txt\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let answers = folder.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&support::formal_name(folder.path()), &serde_json::json!({})),
    )
    .unwrap();
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    let output = support::isolated_command(isolation.path())
        .args([
            "apply",
            folder.path().to_str().unwrap(),
            target.path().to_str().unwrap(),
            "--answers",
            answers.to_str().unwrap(),
            "--trust",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.path().join("log.txt")).unwrap(),
        "alpha=one\nbeta=two\ngamma=three\n"
    );
}

#[cfg(unix)]
#[test]
fn trusted_scripted_apply_reports_files_and_runs_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("template.yml"),
        concat!(
            "name: files-then-hooks\n",
            "source: .\n",
            "ignore: [mark.sh, answers.json]\n",
            "hooks:\n",
            "  - script: mark.sh\n",
        ),
    )
    .unwrap();
    std::fs::write(folder.path().join("alpha.txt"), "alpha\n").unwrap();
    std::fs::write(folder.path().join("beta.txt"), "beta\n").unwrap();
    let script = folder.path().join("mark.sh");
    // The scripted route's stdout is a single JSON document, so the hook records
    // its run in a file rather than echoing to stdout.
    std::fs::write(&script, "#!/bin/sh\nprintf HOOK-MARKER > mark.done\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let answers = folder.path().join("answers.json");
    std::fs::write(
        &answers,
        support::envelope_text(&support::formal_name(folder.path()), &serde_json::json!({})),
    )
    .unwrap();
    let target = tempfile::tempdir().unwrap();
    let isolation = tempfile::tempdir().unwrap();
    // The second run overwrites both files with --force.
    for extra in [None, Some("--force")] {
        let output = support::isolated_command(isolation.path())
            .args([
                "apply",
                folder.path().to_str().unwrap(),
                target.path().to_str().unwrap(),
                "--answers",
                answers.to_str().unwrap(),
                "--trust",
            ])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{extra:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // The applied document lists the written files in write order, and the
        // hook ran (recorded in mark.done). The files-before-hooks ordering is an
        // apply invariant proved in apply.rs.
        let document = support::first_document(&output.stdout);
        let paths: Vec<&str> = document["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["alpha.txt", "beta.txt"], "{extra:?}");
        assert_eq!(document["hooks"].as_array().unwrap().len(), 1, "{extra:?}");
        assert_eq!(
            std::fs::read_to_string(target.path().join("mark.done")).unwrap(),
            "HOOK-MARKER",
            "{extra:?}"
        );
    }
}
