// ---
// relationships:
//   implements: command-line-interface
// ---
//! Acceptance and behavior coverage for the in-project generate axis (`--like`).
//!
//! Each test pins one falsifiable behavior from the approved design's
//! §Behaviors. The seed snapshot is produced by a first application into the
//! repository root (which the producer captures), and later applications into
//! subpaths seed from it; this exercises the whole generate path — repository-
//! wide source-filtered selection, folding, precedence, and route parity — with
//! the producer's own snapshot capture unchanged.
//!
//! The mandated two-subpath fixture and behavior 6 (a subpath application saving
//! its own snapshot so a third can chain) are marked `#[ignore]`: they depend on
//! the producer capturing a snapshot for a non-root target, which the merged
//! producer does not yet do (see the task's blocked handoff / task 1111).

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command as StdCommand, Output};

fn git(dir: &Path, args: &[&str]) -> Output {
    StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// A widget template folder: a text `label`, a select `style` defaulting to
/// `card`, and a confirm `with_tests` defaulting to true; one file renders all
/// three answers.
fn widget_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: widget\n\
         interview:\n\
         \x20 - id: label\n\
         \x20   type: text\n\
         \x20   prompt: Label\n\
         \x20   required: true\n\
         \x20 - id: style\n\
         \x20   type: select\n\
         \x20   prompt: Style\n\
         \x20   options: [card, panel]\n\
         \x20   default: card\n\
         \x20 - id: with_tests\n\
         \x20   type: confirm\n\
         \x20   prompt: With tests\n\
         \x20   default: true\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("template/mod.txt"),
        "label={{ label }}\nstyle={{ style }}\ntests={{ with_tests }}\n",
    )
    .unwrap();
}

/// A repository with one commit, so snapshot capture has a HEAD to record.
fn repo(dir: &Path) {
    git_ok(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-q", "-m", "seed"]);
}

fn answers_file(iso: &Path, name: &str, formal: &str, answers: Value) -> std::path::PathBuf {
    let path = iso.join(name);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "template": formal, "answers": answers })).unwrap(),
    )
    .unwrap();
    path
}

/// Run `apply <address> <target> [--like SEL] --answers FILE [--force]` scripted,
/// in the isolated environment. Returns the exit code and the first JSON document.
fn apply_scripted(
    iso: &Path,
    address: &str,
    target: &Path,
    like: Option<&str>,
    answers: &Path,
    force: bool,
) -> (i32, Value) {
    let mut command = support::isolated_command(iso);
    command.arg("apply").arg(address).arg(target);
    if let Some(selector) = like {
        if selector.is_empty() {
            command.arg("--like");
        } else {
            command.arg("--like").arg(selector);
        }
    }
    command.arg("--answers").arg(answers);
    if force {
        command.arg("--force");
    }
    let output = command.output().unwrap();
    let code = output.status.code().unwrap();
    let document = if output.stdout.is_empty() {
        Value::Null
    } else {
        support::first_document(&output.stdout)
    };
    (code, document)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// Assert a result document validates against the published interview-protocol
/// schema. This binds the `seed` member the CLI emits to the schema that must
/// accept it: drop `seed` from the schema and a seeded document fails here.
fn assert_valid(document: &Value) {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(toha::protocol::protocol_schema())
        .expect("protocol schema builds");
    if let Err(error) = validator.validate(document) {
        panic!("document does not match the protocol schema: {error}\n{document}");
    }
}

/// A scene: a repository whose root holds one applied widget (`label=Root`,
/// `style=panel`, `with_tests=False`) with a committed snapshot, plus the widget
/// address, its source formal name, and that snapshot's id.
struct Scene {
    iso: tempfile::TempDir,
    work: tempfile::TempDir,
    project: std::path::PathBuf,
    address: String,
    formal: String,
    root_snapshot: String,
}

fn scene() -> Scene {
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("widget");
    widget_template(&template);
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    repo(&project);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);
    let root_answers = answers_file(
        iso.path(),
        "root.json",
        &formal,
        json!({ "label": "Root", "style": "panel", "with_tests": false }),
    );
    let (code, doc) = apply_scripted(iso.path(), &address, &project, None, &root_answers, true);
    assert_eq!(code, 0, "root applied: {doc}");
    let root_snapshot = doc["snapshot"]["id"]
        .as_str()
        .expect("the root application recorded a snapshot")
        .to_owned();
    assert!(doc.get("seed").is_none(), "a plain apply carries no seed");
    git_ok(&project, &["add", "-A"]);
    git_ok(&project, &["commit", "-q", "-m", "root"]);
    Scene {
        iso,
        work,
        project,
        address,
        formal,
        root_snapshot,
    }
}

// --------------------------------------------------------------------------
// Seeding behaviors, proven by seeding a subpath from the root snapshot.
// --------------------------------------------------------------------------

#[test]
fn seed_is_a_default_the_document_overrides_not_a_replayed_answer() {
    // Behaviors 1 (seed-not-replay) and 3 (answer beats seed): --answers overrides
    // the seeded label, while style/with_tests take the seeded defaults.
    let s = scene();
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        Some("latest"),
        &beta,
        false,
    );
    assert_eq!(code, 0, "{doc}");
    assert_eq!(doc["seed"]["from"].as_str(), Some(s.root_snapshot.as_str()));
    // The seeded `applied` document matches the published protocol schema.
    assert_valid(&doc);
    // label overridden by the document; style/with_tests inherited from the seed.
    assert_eq!(
        read(&s.project.join("feature/beta/mod.txt")),
        "label=Beta\nstyle=panel\ntests=False\n"
    );
}

#[test]
fn a_full_id_and_a_six_char_prefix_both_select_the_snapshot() {
    // Behavior 16 (positive half): a full id and a >=6-char prefix each resolve.
    let s = scene();
    for (index, reference) in [s.root_snapshot.clone(), s.root_snapshot[..6].to_owned()]
        .into_iter()
        .enumerate()
    {
        let beta = answers_file(
            s.iso.path(),
            "b.json",
            &s.formal,
            json!({ "label": "Beta" }),
        );
        let (code, doc) = apply_scripted(
            s.iso.path(),
            &s.address,
            &s.project.join(format!("feature/beta{index}")),
            Some(&reference),
            &beta,
            false,
        );
        assert_eq!(code, 0, "reference {reference} resolves: {doc}");
        assert_eq!(doc["seed"]["from"].as_str(), Some(s.root_snapshot.as_str()));
    }
}

#[test]
fn a_short_or_unknown_or_ambiguous_prefix_fails_clearly() {
    // Behavior 16 (negative half): a <6-char prefix and an unknown prefix both
    // fail (exit 1, kind snapshot) and write nothing.
    let s = scene();
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    for reference in [&s.root_snapshot[..5], "ZZZZZZ"] {
        let target = s.project.join("feature/x");
        let (code, doc) = apply_scripted(
            s.iso.path(),
            &s.address,
            &target,
            Some(reference),
            &beta,
            false,
        );
        assert_eq!(code, 1, "reference {reference:?} fails: {doc}");
        assert_eq!(doc["kind"].as_str(), Some("snapshot"));
        assert!(
            !target.join("mod.txt").exists(),
            "nothing written for a failed selector"
        );
    }
}

#[test]
fn a_foreign_source_snapshot_is_refused_and_not_a_candidate() {
    // Behaviors 4 (source identity) and 17 (source-filtered selection): a snapshot
    // of a different source is neither an accepted reference nor a `latest`
    // candidate for another template.
    let s = scene();
    let other = s.work.path().join("gadget");
    widget_template(&other);
    let other_address = support::folder_address(&other.canonicalize().unwrap());
    let other_formal = support::formal_name(&other);
    let ans = answers_file(
        s.iso.path(),
        "o.json",
        &other_formal,
        json!({ "label": "X" }),
    );

    // The root snapshot is of the widget source; referencing it while applying
    // the gadget is refused on source.
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &other_address,
        &s.project.join("gadget/one"),
        Some(&s.root_snapshot),
        &ans,
        false,
    );
    assert_eq!(code, 1, "foreign-source reference refused: {doc}");
    assert_eq!(doc["kind"].as_str(), Some("snapshot"));

    // `--like latest` for the gadget source finds no candidate.
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &other_address,
        &s.project.join("gadget/two"),
        Some("latest"),
        &ans,
        false,
    );
    assert_eq!(code, 1, "no candidate of the gadget source: {doc}");
}

#[test]
fn generate_seeds_across_targets_without_a_target_match() {
    // Behavior 5 (generate, not update): the seed snapshot's target is the repo
    // root but the new application writes a subpath; no target match is required.
    let s = scene();
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        Some(&s.root_snapshot),
        &beta,
        false,
    );
    assert_eq!(
        code, 0,
        "seeding a subpath from the root snapshot works: {doc}"
    );
    assert!(s.project.join("feature/beta/mod.txt").exists());
}

#[test]
fn wrong_kind_seed_is_dropped_and_the_apply_proceeds() {
    // Behavior 8: with `with_tests` now a text question, the recorded boolean seed
    // does not kind-match, is dropped, and the id falls back to its template
    // default; the apply still succeeds.
    let s = scene();
    // Edit the template in place: with_tests becomes text with a default.
    std::fs::write(
        s.work.path().join("widget/template.yml"),
        "name: widget\n\
         interview:\n\
         \x20 - id: label\n\
         \x20   type: text\n\
         \x20   prompt: Label\n\
         \x20   required: true\n\
         \x20 - id: style\n\
         \x20   type: select\n\
         \x20   prompt: Style\n\
         \x20   options: [card, panel]\n\
         \x20   default: card\n\
         \x20 - id: with_tests\n\
         \x20   type: text\n\
         \x20   prompt: With tests\n\
         \x20   default: unknown\n",
    )
    .unwrap();
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        Some("latest"),
        &beta,
        false,
    );
    assert_eq!(code, 0, "wrong-kind seed is not fatal: {doc}");
    // style (still a select) inherited; with_tests fell back to the template
    // default because the boolean seed did not apply to the text question.
    assert_eq!(
        read(&s.project.join("feature/beta/mod.txt")),
        "label=Beta\nstyle=panel\ntests=unknown\n"
    );
}

#[test]
fn latest_with_no_snapshot_is_required_absent_and_bare_is_a_usage_error() {
    // Behavior 14: --like latest with no matching snapshot fails (exit 1); a bare
    // --like on the scripted route is a usage error (exit 2); --like absent
    // applies plainly.
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("widget");
    widget_template(&template);
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    repo(&project);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);
    let ans = answers_file(iso.path(), "a.json", &formal, json!({ "label": "A" }));

    let (code, doc) = apply_scripted(
        iso.path(),
        &address,
        &project.join("feature/x"),
        Some("latest"),
        &ans,
        false,
    );
    assert_eq!(code, 1, "required-absent latest: {doc}");
    assert_eq!(doc["kind"].as_str(), Some("snapshot"));

    let (code, _doc) = apply_scripted(
        iso.path(),
        &address,
        &project.join("feature/y"),
        Some(""),
        &ans,
        false,
    );
    assert_eq!(code, 2, "bare --like usage error on the scripted route");

    let (code, doc) = apply_scripted(iso.path(), &address, &project, None, &ans, true);
    assert_eq!(code, 0, "plain apply: {doc}");
    assert!(doc.get("seed").is_none());
}

#[test]
fn like_combined_with_from_is_a_usage_error() {
    // Mutual exclusion (design exit row): --like with --from is exit 2 (clap).
    let s = scene();
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let mut command = support::isolated_command(s.iso.path());
    command
        .arg("apply")
        .arg(&s.address)
        .arg(s.project.join("feature/beta"))
        .arg("--like")
        .arg("latest")
        .arg("--from")
        .arg(&s.root_snapshot)
        .arg("--answers")
        .arg(&beta);
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "--like with --from is a usage error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_agent_route_shows_the_seeded_defaults_and_pins_the_snapshot() {
    // Behaviors 19 (resume re-seeds from the pin) and 21 (route parity): stage
    // --like latest pins the snapshot and the questions document shows the seeded
    // defaults; continue overriding one id and apply PATH resume rebuild the
    // identical seeded tree from the pin.
    let s = scene();
    let beta = s.project.join("feature/beta");

    let mut stage = support::isolated_command(s.iso.path());
    stage
        .arg("stage")
        .arg(&s.address)
        .arg(&beta)
        .arg("--like")
        .arg("latest")
        .arg("--async");
    let out = stage.output().unwrap();
    assert_eq!(out.status.code(), Some(4), "agent stage emits a batch");
    let staged = support::first_document(&out.stdout);
    assert_eq!(
        staged["seed"]["from"].as_str(),
        Some(s.root_snapshot.as_str()),
        "the questions document pins the snapshot"
    );
    // The seeded `questions` document matches the published protocol schema.
    assert_valid(&staged);
    let props = &staged["schema"]["properties"];
    assert_eq!(props["style"]["default"].as_str(), Some("panel"));
    assert_eq!(props["with_tests"]["default"].as_bool(), Some(false));

    // Override only label via continue, then resume with apply PATH.
    let cont = answers_file(
        s.iso.path(),
        "c.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let mut cont_cmd = support::isolated_command(s.iso.path());
    cont_cmd.arg("continue").arg(&beta).arg(&cont);
    let out = cont_cmd.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "continue completed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let mut apply = support::isolated_command(s.iso.path());
    apply.arg("apply").arg(&beta);
    let out = apply.output().unwrap();
    assert!(
        out.status.success(),
        "apply PATH resumed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        read(&beta.join("mod.txt")),
        "label=Beta\nstyle=panel\ntests=False\n"
    );
}

#[test]
fn no_like_path_carries_no_seed_member() {
    // Behavior 10 (no-`--like` path unchanged): a plain apply's document has no
    // seed member and the normal snapshot member. (The byte-for-byte equality of
    // the engine path is covered by the unchanged existing suite.)
    let s = scene();
    let ans = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta", "style": "card", "with_tests": true }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        None,
        &ans,
        false,
    );
    assert_eq!(code, 0, "{doc}");
    assert!(doc.get("seed").is_none(), "no seed member without --like");
    assert_eq!(doc["status"].as_str(), Some("applied"));
}

#[test]
fn resume_honors_the_pin_not_the_newest_snapshot() {
    // Behavior 19 (the pin does not drift): stage pins the root snapshot; a newer
    // same-source snapshot recorded before resume must not change the seed.
    let s = scene();
    let gamma = s.project.join("feature/gamma");

    // Stage seeded from the current newest (the root snapshot), pinning it.
    let mut stage = support::isolated_command(s.iso.path());
    stage
        .arg("stage")
        .arg(&s.address)
        .arg(&gamma)
        .arg("--like")
        .arg("latest")
        .arg("--async");
    let out = stage.output().unwrap();
    assert_eq!(out.status.code(), Some(4));
    let staged = support::first_document(&out.stdout);
    assert_eq!(
        staged["seed"]["from"].as_str(),
        Some(s.root_snapshot.as_str())
    );

    // Record a NEWER snapshot of the same source at the root, with a different
    // style, so "latest" now differs from the pinned snapshot.
    let newer = answers_file(
        s.iso.path(),
        "newer.json",
        &s.formal,
        json!({ "label": "Newer", "style": "card", "with_tests": true }),
    );
    let (code, newer_doc) =
        apply_scripted(s.iso.path(), &s.address, &s.project, None, &newer, true);
    assert_eq!(code, 0, "{newer_doc}");
    let newer_snapshot = newer_doc["snapshot"]["id"].as_str().unwrap();
    assert_ne!(newer_snapshot, s.root_snapshot, "a newer snapshot exists");
    git_ok(&s.project, &["add", "-A"]);
    git_ok(&s.project, &["commit", "-q", "-m", "newer"]);

    // Resume: override only label; the seed must still be the pinned snapshot
    // (style=panel), not the newer one (style=card).
    let cont = answers_file(
        s.iso.path(),
        "c.json",
        &s.formal,
        json!({ "label": "Gamma" }),
    );
    let mut cont_cmd = support::isolated_command(s.iso.path());
    cont_cmd.arg("continue").arg(&gamma).arg(&cont);
    assert_eq!(cont_cmd.output().unwrap().status.code(), Some(0));
    let mut apply = support::isolated_command(s.iso.path());
    apply.arg("apply").arg(&gamma);
    assert!(apply.output().unwrap().status.success());
    assert_eq!(
        read(&gamma.join("mod.txt")),
        "label=Gamma\nstyle=panel\ntests=False\n",
        "resume seeded from the pinned snapshot, not the newer one"
    );
}

// --------------------------------------------------------------------------
// Blocked on producer subpath snapshot capture (task 1111 / root).
// --------------------------------------------------------------------------

#[test]
#[ignore = "blocked: producer snapshot capture does not save for a non-root target \
            (Project::open cannot discover a not-yet-created target, and \
            save_after_apply passes the target subpath as source_dir instead of \
            the repository workdir root). Owned by capture (task 1111)."]
fn mandated_fixture_generator_applied_twice_into_two_subpaths() {
    let s = scene();
    // First subpath application; expected to save snapshot SA once the producer
    // captures non-root targets.
    let alpha = answers_file(
        s.iso.path(),
        "alpha.json",
        &s.formal,
        json!({ "label": "Alpha", "style": "card", "with_tests": true }),
    );
    let (code, alpha_doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("src/widgets/alpha"),
        None,
        &alpha,
        false,
    );
    assert_eq!(code, 0, "{alpha_doc}");
    let snapshot_a = alpha_doc["snapshot"]["id"]
        .as_str()
        .expect("alpha saves a snapshot")
        .to_owned();
    git_ok(&s.project, &["add", "-A"]);
    git_ok(&s.project, &["commit", "-q", "-m", "alpha"]);

    let beta = answers_file(
        s.iso.path(),
        "beta.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let (code, beta_doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("src/widgets/beta"),
        Some(&snapshot_a[..8]),
        &beta,
        false,
    );
    assert_eq!(code, 0, "{beta_doc}");
    assert_eq!(beta_doc["seed"]["from"].as_str(), Some(snapshot_a.as_str()));
    let snapshot_b = beta_doc["snapshot"]["id"]
        .as_str()
        .expect("beta saves its own snapshot (producer capture preserved)");
    assert_ne!(snapshot_a, snapshot_b);
    assert_eq!(
        read(&s.project.join("src/widgets/beta/mod.txt")),
        "label=Beta\nstyle=card\ntests=True\n"
    );
    assert_eq!(
        read(&s.project.join("src/widgets/alpha/mod.txt")),
        "label=Alpha\nstyle=card\ntests=True\n"
    );
}
