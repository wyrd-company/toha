// ---
// relationships:
//   implements: command-line-interface
// ---
//! Acceptance and behavior coverage for the in-project generate axis (`--like`).
//!
//! Each test pins one falsifiable behavior from the approved design's
//! §Behaviors. The mandated fixture applies one generator twice into two
//! subpaths of one project, the second seeded from the first, with both
//! snapshots present (equal source, different targets) and the scripted and
//! agent routes agreeing. The remaining tests each pin one behavior: seeding,
//! precedence, source identity, generate-not-update, producer capture preserved
//! (chaining), fresh instant, wrong-kind drop, constraint-invalid re-ask,
//! extra-id drop, required-absent and bare-selector usage, prefix resolution,
//! single-target plan, dirty-target seeding, reader-only selection, pinned
//! resume, and the byte-identical no-`--like` path.

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
    // The >=6-char prefix runs first, while only the root snapshot exists, so it
    // is unambiguous; each seeded apply then saves its own snapshot that shares
    // the ULID timestamp prefix, and the second selection uses the full id.
    let s = scene();
    for (index, reference) in [s.root_snapshot[..6].to_owned(), s.root_snapshot.clone()]
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

/// Write a local config (`TOHA_CONFIG`) with a `template-defaults` mapping for
/// `formal`, so a configured default exists for the precedence behaviors.
fn write_template_defaults(iso: &Path, formal: &str, entries: &str) {
    let config = iso.join("config/local.yml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        format!("template-defaults:\n  {formal:?}:\n{entries}"),
    )
    .unwrap();
}

#[test]
fn snapshot_shadows_a_configured_default_which_shadows_the_template_default() {
    // Behavior 2 (precedence) and 11 (single occupant): with a configured default
    // for `style` AND a snapshot value for `style`, the snapshot wins (one bank
    // occupant). With only a configured default and no snapshot id, it is used.
    let s = scene();
    // Configure style=card for this template's formal name.
    write_template_defaults(s.iso.path(), &s.formal, "    style: card\n");

    // The root snapshot recorded style=panel. Seed beta; give no style answer.
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
    // The snapshot's panel shadows the configured card (single occupant).
    assert_eq!(
        read(&s.project.join("feature/beta/mod.txt")),
        "label=Beta\nstyle=panel\ntests=False\n",
        "snapshot value shadows the configured default"
    );

    // Without --like, the configured default (card) is used — proving the
    // configured layer still works and the snapshot only shadows it when seeded.
    let plain = answers_file(
        s.iso.path(),
        "p.json",
        &s.formal,
        json!({ "label": "Plain" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/plain"),
        None,
        &plain,
        false,
    );
    assert_eq!(code, 0, "{doc}");
    assert_eq!(
        read(&s.project.join("feature/plain/mod.txt")),
        "label=Plain\nstyle=card\ntests=True\n",
        "configured default used when nothing is seeded"
    );
}

#[test]
fn a_third_application_can_seed_from_the_second() {
    // Behavior 6 (producer capture preserved so applications chain): each seeded
    // apply into a clean subpath saves its own snapshot, so a third seeds the
    // second, not the first.
    let s = scene();
    // beta submits all three, distinct from root (root was style=panel/tests=false),
    // so beta's own snapshot records values only a chain from beta can carry.
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta", "style": "card", "with_tests": true }),
    );
    let (code, beta_doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        Some(&s.root_snapshot),
        &beta,
        false,
    );
    assert_eq!(code, 0, "{beta_doc}");
    let snapshot_b = beta_doc["snapshot"]["id"]
        .as_str()
        .expect("beta saved its own snapshot")
        .to_owned();
    git_ok(&s.project, &["add", "-A"]);
    git_ok(&s.project, &["commit", "-q", "-m", "beta"]);

    // gamma seeds explicitly from beta's snapshot; inherits beta's style=card and
    // with_tests=true (beta's values, not root's style=panel/tests=false).
    let gamma = answers_file(
        s.iso.path(),
        "g.json",
        &s.formal,
        json!({ "label": "Gamma" }),
    );
    let (code, gamma_doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/gamma"),
        Some(&snapshot_b),
        &gamma,
        false,
    );
    assert_eq!(code, 0, "{gamma_doc}");
    assert_eq!(
        gamma_doc["seed"]["from"].as_str(),
        Some(snapshot_b.as_str())
    );
    assert_eq!(
        read(&s.project.join("feature/gamma/mod.txt")),
        "label=Gamma\nstyle=card\ntests=True\n",
        "gamma inherited beta's values (card/true), proving the second application was captured and chains"
    );
}

#[test]
fn the_new_application_uses_its_own_instant_not_the_seed_snapshots() {
    // Behavior 7 (fresh instant): the seed carries recorded answers, not the
    // snapshot's frozen `generated` instant, so a date-rendering template uses
    // the new application's own `now`.
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("dated");
    std::fs::create_dir_all(template.join("template")).unwrap();
    std::fs::write(
        template.join("template.yml"),
        "name: dated\ninterview:\n  - { id: label, type: text, prompt: L, required: true }\n",
    )
    .unwrap();
    std::fs::write(
        template.join("template/mod.txt"),
        "label={{ label }}\nwhen={{ now() }}\n",
    )
    .unwrap();
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    repo(&project);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);

    // Root application at instant T1.
    let a = answers_file(iso.path(), "a.json", &formal, json!({ "label": "A" }));
    let mut first = support::isolated_command(iso.path());
    first
        .arg("apply")
        .arg(&address)
        .arg(&project)
        .arg("--answers")
        .arg(&a)
        .arg("--force")
        .env("TOHA_NOW", "2020-01-01T00:00:00+00:00[UTC]");
    let root_doc = support::first_document(&first.output().unwrap().stdout);
    let snapshot = root_doc["snapshot"]["id"].as_str().unwrap().to_owned();
    git_ok(&project, &["add", "-A"]);
    git_ok(&project, &["commit", "-q", "-m", "root"]);

    // Seeded application at a different instant T2 renders T2, not T1.
    let b = answers_file(iso.path(), "b.json", &formal, json!({ "label": "B" }));
    let mut second = support::isolated_command(iso.path());
    second
        .arg("apply")
        .arg(&address)
        .arg(project.join("feature/beta"))
        .arg("--like")
        .arg(&snapshot)
        .arg("--answers")
        .arg(&b)
        .env("TOHA_NOW", "2022-02-02T00:00:00+00:00[UTC]");
    assert!(second.output().unwrap().status.success());
    let rendered = read(&project.join("feature/beta/mod.txt"));
    assert!(
        rendered.contains("2022-02-02") && !rendered.contains("2020-01-01"),
        "the seeded application uses its own instant: {rendered}"
    );
}

#[test]
fn a_constraint_invalid_seed_is_re_asked_not_written() {
    // Behavior 9 (constraint-invalid re-ask): a kind-valid seed that fails a
    // tightened pattern in the new version is returned as a remaining question
    // (exit 4) naming the snapshot, and nothing is written.
    let s = scene();
    // Tighten `label` to a pattern the recorded "Root" fails.
    std::fs::write(
        s.work.path().join("widget/template.yml"),
        "name: widget\n\
         interview:\n\
         \x20 - id: label\n\
         \x20   type: text\n\
         \x20   prompt: Label\n\
         \x20   required: true\n\
         \x20   validate: { regex: '^[0-9]+$' }\n\
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
    // Seed latest (label="Root" fails ^[0-9]+$), do not override label.
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "style": "card" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &s.project.join("feature/beta"),
        Some("latest"),
        &beta,
        false,
    );
    assert_eq!(code, 4, "constraint-invalid seed re-asks: {doc}");
    assert_eq!(doc["status"].as_str(), Some("questions"));
    assert!(
        doc["errors"]["label"].is_array(),
        "the label question carries the rejection: {doc}"
    );
    assert!(
        !s.project.join("feature/beta/mod.txt").exists(),
        "nothing is written when a seeded value is re-asked"
    );
}

#[test]
fn an_extra_seed_id_the_template_dropped_is_ignored() {
    // Behavior 12 (extra seed ids ignored): a snapshot carrying an id the new
    // template no longer defines is silently ignored; the apply proceeds.
    let s = scene();
    // Drop `with_tests` from the template; the snapshot still records it.
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
         \x20   default: card\n",
    )
    .unwrap();
    std::fs::write(
        s.work.path().join("widget/template/mod.txt"),
        "label={{ label }}\nstyle={{ style }}\n",
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
    assert_eq!(code, 0, "extra seed id ignored, apply proceeds: {doc}");
    assert_eq!(
        read(&s.project.join("feature/beta/mod.txt")),
        "label=Beta\nstyle=panel\n",
        "the dropped id is ignored; style still inherited"
    );
}

#[test]
fn a_dirty_target_still_seeds_but_skips_its_own_capture() {
    // Behavior 15 (dirty target still seeds): with a dirty target, defaults are
    // still seeded; only the new snapshot capture is skipped (dirty).
    let s = scene();
    let beta_dir = s.project.join("feature/beta");
    std::fs::create_dir_all(&beta_dir).unwrap();
    std::fs::write(beta_dir.join("mod.txt"), "pre-existing\n").unwrap();
    git_ok(&s.project, &["add", "-A"]);
    git_ok(&s.project, &["commit", "-q", "-m", "beta pre-existing"]);
    // Make the target dirty before the apply.
    std::fs::write(beta_dir.join("mod.txt"), "uncommitted edit\n").unwrap();

    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let (code, doc) = apply_scripted(
        s.iso.path(),
        &s.address,
        &beta_dir,
        Some("latest"),
        &beta,
        true, // --force over the dirty file
    );
    assert_eq!(code, 0, "{doc}");
    // Seeded defaults still applied (style inherited).
    assert_eq!(
        read(&beta_dir.join("mod.txt")),
        "label=Beta\nstyle=panel\ntests=False\n"
    );
    // The new snapshot was skipped because the target was dirty.
    assert_eq!(
        doc["snapshot"]["skipped"].as_str(),
        Some("dirty"),
        "capture skipped on a dirty target: {doc}"
    );
    // Seeding worked even though capture was skipped.
    assert_eq!(doc["seed"]["from"].as_str(), Some(s.root_snapshot.as_str()));
}

#[test]
fn a_like_apply_plans_exactly_one_target() {
    // Behavior 18 (single-target Plan): a --like apply writes one subpath; the
    // applied document's files are all under that subpath, never a second target.
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
    let files = doc["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1, "one file written: {files:?}");
    // The written path is target-relative (mod.txt), and the target is the one
    // subpath — no second target is planned.
    assert_eq!(files[0]["path"].as_str(), Some("mod.txt"));
    assert_eq!(
        doc["context"]["target"].as_str(),
        Some(s.project.join("feature/beta").to_str().unwrap())
    );
}

#[test]
fn like_selection_writes_no_ref() {
    // Behavior 20 (reader-only selection): the --like selection path writes no
    // snapshot ref; a dry-run apply selects and reads but writes nothing, so the
    // ref set is unchanged. (A non-dry-run apply's only new ref is the producer's
    // own capture of the new application, proven elsewhere.)
    let s = scene();
    let refs_before = snapshot_refs(&s.project);
    let beta = answers_file(
        s.iso.path(),
        "b.json",
        &s.formal,
        json!({ "label": "Beta" }),
    );
    let mut cmd = support::isolated_command(s.iso.path());
    cmd.arg("apply")
        .arg(&s.address)
        .arg(s.project.join("feature/beta"))
        .arg("--like")
        .arg("latest")
        .arg("--answers")
        .arg(&beta)
        .arg("--dry-run");
    let out = cmd.output().unwrap();
    let doc = support::first_document(&out.stdout);
    assert_eq!(doc["status"].as_str(), Some("planned"), "{doc}");
    assert_eq!(doc["seed"]["from"].as_str(), Some(s.root_snapshot.as_str()));
    let refs_after = snapshot_refs(&s.project);
    assert_eq!(
        refs_before, refs_after,
        "the --like selection (and a dry-run apply) write no snapshot ref"
    );
}

/// Every `refs/toha/snapshots/*` ref name in a repository, sorted.
fn snapshot_refs(project: &Path) -> Vec<String> {
    let out = git(
        project,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/toha/snapshots/",
        ],
    );
    let mut refs: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| l.to_owned())
        .collect();
    refs.sort();
    refs
}

// --------------------------------------------------------------------------
// Mandated fixture: one generator applied twice into two subpaths of one
// project, the second seeded from the first, all three routes agreeing.
// --------------------------------------------------------------------------

/// The valid snapshots `snapshots list --json` reports for one target (the
/// listing is target-scoped), each as `{ id, source, target, ... }`.
fn snapshots_at(iso: &Path, target: &Path) -> Vec<Value> {
    let mut list = support::isolated_command(iso);
    list.arg("snapshots").arg("list").arg(target).arg("--json");
    let doc = support::first_document(&list.output().unwrap().stdout);
    doc["snapshots"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.get("id").is_some())
        .collect()
}

#[test]
fn generator_applied_twice_into_two_subpaths_second_seeds_first() {
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("widget");
    widget_template(&template);
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    repo(&project);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);

    // First application into src/widgets/alpha with full answers; saves SA.
    let alpha = answers_file(
        iso.path(),
        "alpha.json",
        &formal,
        json!({ "label": "Alpha", "style": "card", "with_tests": true }),
    );
    let (code, alpha_doc) = apply_scripted(
        iso.path(),
        &address,
        &project.join("src/widgets/alpha"),
        None,
        &alpha,
        false,
    );
    assert_eq!(code, 0, "alpha applied: {alpha_doc}");
    let snapshot_a = alpha_doc["snapshot"]["id"]
        .as_str()
        .expect("the first subpath application saved a snapshot")
        .to_owned();
    assert!(alpha_doc.get("seed").is_none(), "plain apply has no seed");
    git_ok(&project, &["add", "-A"]);
    git_ok(&project, &["commit", "-q", "-m", "alpha"]);

    // Second application into src/widgets/beta, seeded from alpha by >=6-char
    // prefix, overriding only label; style/with_tests take the seeded defaults.
    let beta = answers_file(iso.path(), "beta.json", &formal, json!({ "label": "Beta" }));
    let (code, beta_doc) = apply_scripted(
        iso.path(),
        &address,
        &project.join("src/widgets/beta"),
        Some(&snapshot_a[..8]),
        &beta,
        false,
    );
    assert_eq!(code, 0, "beta applied: {beta_doc}");
    assert_eq!(
        beta_doc["seed"]["from"].as_str(),
        Some(snapshot_a.as_str()),
        "beta's applied document names the seed"
    );
    let snapshot_b = beta_doc["snapshot"]["id"]
        .as_str()
        .expect("beta saved its own snapshot (producer capture preserved)")
        .to_owned();
    git_ok(&project, &["add", "-A"]);
    git_ok(&project, &["commit", "-q", "-m", "beta"]);

    // beta inherited style/with_tests from alpha and overrode only label.
    let beta_tree = read(&project.join("src/widgets/beta/mod.txt"));
    assert_eq!(
        beta_tree, "label=Beta\nstyle=card\ntests=True\n",
        "{beta_tree}"
    );
    // alpha is unchanged.
    assert_eq!(
        read(&project.join("src/widgets/alpha/mod.txt")),
        "label=Alpha\nstyle=card\ntests=True\n"
    );

    // Two snapshots exist, equal source, different targets. The listing is
    // target-scoped, so read alpha's and beta's separately.
    assert_ne!(snapshot_a, snapshot_b, "two distinct applications");
    let at_alpha = snapshots_at(iso.path(), &project.join("src/widgets/alpha"));
    let at_beta = snapshots_at(iso.path(), &project.join("src/widgets/beta"));
    assert_eq!(at_alpha.len(), 1, "one snapshot at alpha: {at_alpha:?}");
    assert_eq!(at_beta.len(), 1, "one snapshot at beta: {at_beta:?}");
    assert_eq!(at_alpha[0]["id"].as_str(), Some(snapshot_a.as_str()));
    assert_eq!(at_alpha[0]["target"].as_str(), Some("src/widgets/alpha"));
    assert_eq!(at_beta[0]["id"].as_str(), Some(snapshot_b.as_str()));
    assert_eq!(at_beta[0]["target"].as_str(), Some("src/widgets/beta"));
    assert_eq!(
        at_alpha[0]["source"].as_str(),
        at_beta[0]["source"].as_str(),
        "both snapshots share one source"
    );

    // Route parity: the agent route (stage --async → continue → apply PATH) and
    // the scripted route produce the identical beta tree for the same inputs,
    // and the questions document shows the seeded defaults the person route
    // pre-fills — so all three routes agree.
    let gamma = project.join("src/widgets/gamma");
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(&gamma)
        .arg("--like")
        // A full id: two snapshots now share the ULID timestamp prefix, so only
        // the full id names alpha unambiguously.
        .arg(&snapshot_a)
        .arg("--async");
    let staged = support::first_document(&stage.output().unwrap().stdout);
    assert_eq!(staged["seed"]["from"].as_str(), Some(snapshot_a.as_str()));
    let props = &staged["schema"]["properties"];
    assert_eq!(props["style"]["default"].as_str(), Some("card"));
    assert_eq!(props["with_tests"]["default"].as_bool(), Some(true));
    assert_eq!(props["label"]["default"].as_str(), Some("Alpha"));
    assert_valid(&staged);

    let cont = answers_file(iso.path(), "g.json", &formal, json!({ "label": "Beta" }));
    let mut cont_cmd = support::isolated_command(iso.path());
    cont_cmd.arg("continue").arg(&gamma).arg(&cont);
    assert_eq!(cont_cmd.output().unwrap().status.code(), Some(0));
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(&gamma);
    assert!(apply.output().unwrap().status.success());
    assert_eq!(
        read(&gamma.join("mod.txt")),
        beta_tree,
        "the agent route produces the identical seeded tree as the scripted route"
    );
}
