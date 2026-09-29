// ---
// relationships:
//   implements: architecture
// ---
//! Include behavior that the fixture harness cannot express: load-order and
//! snapshot lifecycle at the library boundary, a runtime-constructed symlink,
//! the named cycle path, and the composition proof that an environment value
//! reached only through a nested include drives the stage-trust gate on the
//! real binary.
#[allow(dead_code)]
mod support;

use std::{fs, path::Path, process::Stdio};

use toha::{
    Completed, Interview, Plan, Seed, Template,
    context::InvocationContext,
    staging::{Store, canonical_target},
};

/// Writes a file, creating parent directories.
fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A minimal template folder rooted at a fresh temp directory. `template.yml`
/// and files are written by the caller; the source subdirectory is `template/`.
fn scaffold() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("template/template")).unwrap();
    dir
}

/// Completes a question-free interview and plans it into a throwaway target.
fn complete(template: &Template) -> (Completed, toha::staging::CanonicalTarget) {
    let target = tempfile::tempdir().unwrap();
    let target = canonical_target(target.path()).unwrap();
    let Interview::Complete(completed) = Interview::start(
        template,
        Seed {
            now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
            defaults: Default::default(),
            context: InvocationContext::for_target(target.clone()),
        },
    )
    .unwrap() else {
        panic!("expected a complete interview");
    };
    (completed, target)
}

#[test]
fn files_include_missing_fails_template_load() {
    let dir = scaffold();
    let root = dir.path().join("template");
    write(
        &root.join("template.yml"),
        "name: t\ndata:\n  xs:\n    - a\nfiles:\n  - each: xs as x\n    source: parts/body.txt\n    path: \"out/{{ x }}.txt\"\n",
    );
    write(
        &root.join("parts/body.txt"),
        "{% include \"parts/missing.txt\" %}",
    );
    // Stop at the library boundary: only load is called.
    let error = Template::load(&root).unwrap_err();
    let problem = error
        .problems
        .iter()
        .find(|p| p.path == "files[0].source")
        .unwrap_or_else(|| panic!("no files[0].source problem in {:?}", error.problems));
    assert!(
        problem.message.contains("jinja include not found")
            && problem.message.contains("parts/missing.txt"),
        "{}",
        problem.message
    );
}

#[test]
fn files_include_reference_fails_template_load() {
    let dir = scaffold();
    let root = dir.path().join("template");
    write(
        &root.join("template.yml"),
        "name: t\ndata:\n  xs:\n    - a\nfiles:\n  - each: xs as x\n    source: parts/body.txt\n    path: \"out/{{ x }}.txt\"\n",
    );
    // The selected partial references an id that is neither an earlier node nor
    // the `each` binding, so the unioned reference check fails at load.
    write(
        &root.join("parts/body.txt"),
        "{% include \"parts/ref.txt\" %}",
    );
    write(&root.join("parts/ref.txt"), "{{ mystery }}");
    let error = Template::load(&root).unwrap_err();
    let problem = error
        .problems
        .iter()
        .find(|p| p.path == "files[0].source")
        .unwrap_or_else(|| panic!("no files[0].source problem in {:?}", error.problems));
    assert!(
        problem
            .message
            .contains("id is not defined by an earlier node")
            && problem.message.contains("mystery"),
        "{}",
        problem.message
    );
}

#[test]
fn files_body_snapshot_is_stable_after_load() {
    let dir = scaffold();
    let root = dir.path().join("template");
    write(
        &root.join("template.yml"),
        "name: t\ndata:\n  xs:\n    - a\nfiles:\n  - each: xs as x\n    source: parts/body.txt\n    path: \"out/{{ x }}.txt\"\n",
    );
    write(
        &root.join("parts/body.txt"),
        "{% include \"parts/frag.txt\" %}X",
    );
    write(&root.join("parts/frag.txt"), "A");
    let template = Template::load(&root).unwrap();
    // Mutate both the body and its selected partial after load; planning must
    // render the retained snapshot, not the live files.
    write(&root.join("parts/body.txt"), "MUTATED");
    write(&root.join("parts/frag.txt"), "B");
    let (completed, target) = complete(&template);
    let plan = Plan::build(&template, &completed, &target).unwrap();
    let file = plan
        .files
        .iter()
        .find(|f| f.path.to_string() == "out/a.txt")
        .expect("planned out/a.txt");
    match &file.content {
        toha::Content::Rendered(text) => assert_eq!(text, "AX"),
        other => panic!("expected rendered content, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn err_jinja_include_symlink_inside_root_pointing_outside() {
    use std::os::unix::fs::symlink;
    let dir = scaffold();
    let root = dir.path().join("template");
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("real.txt"), "secret").unwrap();
    // A directory-component symlink inside the root pointing outside: the vector
    // a normalizing join misses. The include name stays confined; the escape is
    // through the link.
    symlink(outside.path(), root.join("shared")).unwrap();
    write(&root.join("template.yml"), "name: t\n");
    write(
        &root.join("template/note.txt"),
        "{% include \"shared/real.txt\" %}",
    );
    let error = Template::load(&root).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("jinja include path is a symlink"),
        "{error}"
    );
}

#[test]
fn err_jinja_include_cycle_names_the_full_path() {
    let dir = scaffold();
    let root = dir.path().join("template");
    write(&root.join("template.yml"), "name: t\n");
    write(&root.join("template/note.txt"), "{% include \"a.txt\" %}");
    write(&root.join("a.txt"), "{% include \"b.txt\" %}");
    write(&root.join("b.txt"), "{% include \"a.txt\" %}");
    let error = Template::load(&root).unwrap_err();
    let text = error.to_string();
    assert!(text.contains("jinja include cycle"), "{text}");
    assert!(text.contains("a.txt -> b.txt -> a.txt"), "{text}");
}

/// The composition proof (task orchestrator requirement): with a real template
/// and the real binary, an environment value reachable ONLY through a nested
/// include requires an explicit stage `--trust`. Structural union-seam tests do
/// not establish this; the whole point is that the body itself reads no fixed
/// value — only its nested partial does.
mod nested_include_trust {
    use super::*;
    use std::process::Command;

    /// A template whose source body reads a fixed environment value only through
    /// a nested partial. Two questions force a later interview batch.
    fn nested_env_template(root: &Path) {
        write(
            &root.join("template.yml"),
            "name: nested-env\ninterview:\n  - id: first\n    type: text\n    prompt: First\n  - id: second\n    type: text\n    prompt: Second\n",
        );
        // The body has no environment reference; only the partial it includes.
        write(
            &root.join("template/out.txt"),
            "{% include \"partials/env.txt\" %}",
        );
        write(&root.join("partials/env.txt"), "user={{ toha_env_user }}\n");
    }

    /// An isolated `toha` invocation with a fixed `USER` and clock.
    fn toha(state: &Path, user: &str) -> Command {
        let mut command = support::isolated_command(state);
        command
            .env("USER", user)
            .env("TOHA_NOW", "2026-01-02T03:04:05+00:00[UTC]")
            .stdin(Stdio::null());
        command
    }

    #[test]
    fn stage_without_trust_is_refused_before_any_read_or_store() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("tpl");
        fs::create_dir_all(root.join("template")).unwrap();
        nested_env_template(&root);
        let state = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let output = toha(state.path(), "alice")
            .args([
                "stage",
                root.to_str().unwrap(),
                target.path().to_str().unwrap(),
                "--async",
            ])
            .output()
            .unwrap();
        assert_ne!(output.status.code(), Some(0), "stage should be refused");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("requires stage trust"),
            "expected trust refusal, got: {stderr}"
        );
        // No grant means no store: the refusal precedes seed/interview/store.
        let staged = Store::new(support::staged_dir(state.path()))
            .load(&canonical_target(target.path()).unwrap())
            .unwrap();
        assert!(staged.is_none(), "a refused stage must not store a record");
    }

    #[test]
    fn apply_reads_the_nested_value_only_under_a_grant() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("tpl");
        fs::create_dir_all(root.join("template")).unwrap();
        nested_env_template(&root);
        let answers = root.join("answers.json");
        fs::write(&answers, r#"{"first":"a","second":"b"}"#).unwrap();

        // Denied without trust: the fixed value never reaches the render, so the
        // nested-include read produces an empty value and captures nothing.
        let denied_target = tempfile::tempdir().unwrap();
        let denied_state = tempfile::tempdir().unwrap();
        let denied = toha(denied_state.path(), "alice")
            .args([
                "apply",
                root.to_str().unwrap(),
                denied_target.path().to_str().unwrap(),
                "--answers",
                answers.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            denied.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&denied.stderr)
        );
        assert_eq!(
            fs::read_to_string(denied_target.path().join("out.txt")).unwrap(),
            "user=\n"
        );

        // Granted with --trust: the nested-include read resolves the captured value.
        let granted_target = tempfile::tempdir().unwrap();
        let granted_state = tempfile::tempdir().unwrap();
        let granted = toha(granted_state.path(), "alice")
            .args([
                "apply",
                root.to_str().unwrap(),
                granted_target.path().to_str().unwrap(),
                "--answers",
                answers.to_str().unwrap(),
                "--trust",
            ])
            .output()
            .unwrap();
        assert_eq!(
            granted.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&granted.stderr)
        );
        assert_eq!(
            fs::read_to_string(granted_target.path().join("out.txt")).unwrap(),
            "user=alice\n"
        );
    }

    #[test]
    fn stage_grant_is_a_fixed_snapshot_across_a_later_batch_and_a_mutated_folder() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("tpl");
        fs::create_dir_all(root.join("template")).unwrap();
        nested_env_template(&root);
        let state = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();

        // Stage with the grant and USER=alice; the fixed five are captured once.
        let staged = toha(state.path(), "alice")
            .args([
                "stage",
                root.to_str().unwrap(),
                target.path().to_str().unwrap(),
                "--trust",
                "--async",
            ])
            .output()
            .unwrap();
        assert_eq!(
            staged.status.code(),
            Some(4),
            "{}",
            String::from_utf8_lossy(&staged.stderr)
        );

        // Change the environment and mutate the partial folder after staging.
        write(&root.join("partials/env.txt"), "USER={{ toha_env_user }}\n");

        // Resume in a later batch with a different USER; the mutated partial is
        // re-read for its text on the fresh load, while the projected value comes
        // from the staged snapshot rather than the live environment.
        let mut resume = toha(state.path(), "bob");
        resume
            .args(["continue", target.path().to_str().unwrap(), "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = resume.spawn().unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"first":"a","second":"b"}"#)
            .unwrap();
        let resumed = child.wait_with_output().unwrap();
        assert_eq!(
            resumed.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&resumed.stderr)
        );

        // Apply the staged complete interview under yet another USER.
        let applied = toha(state.path(), "carol")
            .args(["apply", target.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            applied.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&applied.stderr)
        );
        let written = fs::read_to_string(target.path().join("out.txt")).unwrap();
        assert_eq!(
            written, "USER=alice\n",
            "the staged fixed-five snapshot must survive the later batch and the mutated folder"
        );
    }
}
