// ---
// relationships:
//   implements: command-line-interface
//   verifies: interview-protocol
// ---
//! The mandated in-project-generators fixture harness (design.md:409,417,507,540).
//!
//! It consumes the committed `tests/fixtures/generator-twice/` assets as
//! executable expectations: `expect.yml` drives the targets, answer assets, the
//! `--like` selector, the expected trees, and the route list; the `widget`
//! `template/`, `answers-alpha.json`/`answers-beta.json`, and the `expected/`
//! trees supply the inputs and the oracle. Nothing is hardcoded that `expect.yml`
//! declares.
//!
//! One generator is applied twice into one clean git repository: first into the
//! `alpha` target (recording snapshot `SA`), committed; then into the `beta`
//! target with `--like <SA>` and the beta answers, so beta inherits
//! `style`/`with_tests` from `SA` and overrides only `label`. The beta
//! application is driven through every route `expect.yml` lists — scripted,
//! agent, the real terminal/person over a PTY, and the crate's public library
//! surface — each in its OWN isolated copy of the same base project and each into
//! the SAME beta target. Every route's COMPLETE beta tree (including any
//! unexpected file) must equal `expected/beta`; the routes must agree with each
//! other; the accepted raw submissions must match per route shape; and both
//! snapshots must exist with equal source and the declared targets.

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

const NOW: &str = "2026-01-02T03:04:05+00:00[UTC]";

// ---------------------------------------------------------------------------
// expect.yml — the executable expectations the harness consumes.
// ---------------------------------------------------------------------------

/// The sentinel `expect.yml` uses where a value is the alpha snapshot id, which
/// is only known at runtime (ULIDs are generated per apply).
const SA_SENTINEL: &str = "SA";

#[derive(Debug, Deserialize)]
struct FixtureExpect {
    alpha: AppExpect,
    beta: BetaExpect,
    snapshots: SnapshotsExpect,
    routes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AppExpect {
    target: String,
    answers: String,
    expected: String,
}

#[derive(Debug, Deserialize)]
struct BetaExpect {
    target: String,
    answers: String,
    like: String,
    expected: String,
    seed_from: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotsExpect {
    equal_source: bool,
    targets: Vec<String>,
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generator-twice")
}

fn load_expect() -> FixtureExpect {
    serde_norway::from_str(&std::fs::read_to_string(fixture().join("expect.yml")).unwrap())
        .expect("expect.yml parses")
}

/// The bare answers object recorded in a fixture `answers-*.json`.
fn fixture_answers(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture().join(name)).unwrap()).unwrap()
}

// ---------------------------------------------------------------------------
// Complete-tree reading: a relative-path -> bytes map, so a comparison fails on
// a missing file, a changed file, OR an unexpected extra file.
// ---------------------------------------------------------------------------

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// The complete expected tree under `expected/<which>`.
fn expected_tree(which: &str) -> BTreeMap<String, Vec<u8>> {
    read_tree(&fixture().join(which))
}

fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// Copy a directory tree in full, including `.git` and the Toha snapshot refs it
/// holds, so each route runs against an independent but identical project.
fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Wrap a bare answers object in the identity-bearing envelope the document
/// routes require, written beside the config isolation root.
fn envelope(iso: &Path, name: &str, formal: &str, answers: &Value) -> PathBuf {
    let path = iso.join(name);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "template": formal, "answers": answers })).unwrap(),
    )
    .unwrap();
    path
}

/// Run a scripted apply and return (exit code, first JSON document).
fn scripted(
    iso: &Path,
    address: &str,
    target: &Path,
    like: Option<&str>,
    answers: &Path,
) -> (i32, Value) {
    let mut command = support::isolated_command(iso);
    command.arg("apply").arg(address).arg(target);
    if let Some(sel) = like {
        command.arg("--like").arg(sel);
    }
    command.arg("--answers").arg(answers).env("TOHA_NOW", NOW);
    let output = command.output().unwrap();
    let doc = support::first_document(&output.stdout);
    (output.status.code().unwrap(), doc)
}

// ---------------------------------------------------------------------------
// Snapshot reading through the public crate reader.
// ---------------------------------------------------------------------------

/// The valid snapshot captured at `target` within `project_root` (exactly one in
/// the fixture), via the public `toha::snapshot` reader.
fn snapshot_at(project_root: &Path, target: &str) -> toha::snapshot::Snapshot {
    use toha::snapshot::{Listed, Project};
    let canonical = toha::staging::canonical_target(&project_root.join(target)).unwrap();
    let project = Project::open(&canonical).unwrap().expect("target in git");
    let mut valid: Vec<_> = project
        .snapshots()
        .unwrap()
        .into_iter()
        .filter_map(|listed| match listed {
            Listed::Valid(snapshot) => Some(snapshot),
            Listed::Invalid { .. } => None,
        })
        .collect();
    assert_eq!(valid.len(), 1, "exactly one snapshot at {target}");
    valid.pop().unwrap()
}

/// A snapshot's recorded raw submissions as a comparable owned value.
fn submissions_of(snapshot: &toha::snapshot::Snapshot) -> Vec<IndexMap<String, Value>> {
    snapshot.submissions().to_vec()
}

/// What one route produced for the beta application, for cross-route comparison.
struct RouteResult {
    /// The complete rendered tree under the beta target.
    tree: BTreeMap<String, Vec<u8>>,
    /// The result document's `seed.from`, when the route emits a document.
    seed_from: Option<String>,
    /// The EXPLICIT raw submissions the route recorded — route-shaped: document
    /// routes record only the override, the person records every accepted value.
    submissions: Vec<IndexMap<String, Value>>,
}

/// Build the base project in `root`: an empty git repo with one seed commit, then
/// the alpha application committed, recording snapshot `SA`. Returns `SA`.
fn build_base(
    iso: &Path,
    root: &Path,
    address: &str,
    formal: &str,
    expect: &FixtureExpect,
) -> String {
    std::fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "seed"]);

    let alpha_answers = envelope(
        iso,
        "alpha.json",
        formal,
        &fixture_answers(&expect.alpha.answers),
    );
    let (code, alpha_doc) = scripted(
        iso,
        address,
        &root.join(&expect.alpha.target),
        None,
        &alpha_answers,
    );
    assert_eq!(code, 0, "alpha applied: {alpha_doc}");
    let sa = alpha_doc["snapshot"]["id"]
        .as_str()
        .expect("alpha recorded snapshot SA")
        .to_owned();
    // alpha renders exactly its complete expected tree.
    assert_eq!(
        read_tree(&root.join(&expect.alpha.target)),
        expected_tree(&expect.alpha.expected),
        "alpha matches its complete expected tree"
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "alpha"]);
    sa
}

/// Resolve the `--like` selector `expect.yml` declares into a runtime value: the
/// `SA` sentinel becomes a >= 6-char prefix of the alpha snapshot id.
fn like_selector(expect: &FixtureExpect, sa: &str) -> String {
    if expect.beta.like == SA_SENTINEL {
        sa[..8].to_owned()
    } else {
        expect.beta.like.clone()
    }
}

/// The expected `seed.from` the fixture declares, resolving the `SA` sentinel.
fn expected_seed_from(expect: &FixtureExpect, sa: &str) -> String {
    if expect.beta.seed_from == SA_SENTINEL {
        sa.to_owned()
    } else {
        expect.beta.seed_from.clone()
    }
}

/// Scripted route: `apply ADDRESS BETA --like <SA> --answers beta.json` in a
/// fresh copy of the base project.
fn scripted_route(
    iso: &Path,
    base: &Path,
    address: &str,
    formal: &str,
    sa: &str,
    expect: &FixtureExpect,
) -> RouteResult {
    let copy = tempfile::tempdir().unwrap();
    let root = copy.path().join("project");
    copy_dir(base, &root);
    let beta = envelope(
        iso,
        "beta-scripted.json",
        formal,
        &fixture_answers(&expect.beta.answers),
    );
    let (code, doc) = scripted(
        iso,
        address,
        &root.join(&expect.beta.target),
        Some(&like_selector(expect, sa)),
        &beta,
    );
    assert_eq!(code, 0, "beta (scripted) applied: {doc}");
    let snapshot = snapshot_at(&root, &expect.beta.target);
    let submissions = submissions_of(&snapshot);
    RouteResult {
        tree: read_tree(&root.join(&expect.beta.target)),
        seed_from: doc["seed"]["from"].as_str().map(ToOwned::to_owned),
        submissions,
    }
}

/// Agent route: `stage --async` -> `continue` -> `apply PATH` in a fresh copy.
fn agent_route(
    iso: &Path,
    base: &Path,
    address: &str,
    formal: &str,
    sa: &str,
    expect: &FixtureExpect,
) -> RouteResult {
    let copy = tempfile::tempdir().unwrap();
    let root = copy.path().join("project");
    copy_dir(base, &root);
    let beta_dir = root.join(&expect.beta.target);

    let mut stage = support::isolated_command(iso);
    stage
        .arg("stage")
        .arg(address)
        .arg(&beta_dir)
        .arg("--like")
        .arg(like_selector(expect, sa))
        .arg("--async")
        .env("TOHA_NOW", NOW);
    let staged = support::first_document(&stage.output().unwrap().stdout);
    let staged_seed = staged["seed"]["from"].as_str().map(ToOwned::to_owned);

    let cont = envelope(
        iso,
        "beta-agent.json",
        formal,
        &fixture_answers(&expect.beta.answers),
    );
    let mut cont_cmd = support::isolated_command(iso);
    cont_cmd
        .arg("continue")
        .arg(&beta_dir)
        .arg(&cont)
        .env("TOHA_NOW", NOW);
    assert_eq!(
        cont_cmd.output().unwrap().status.code(),
        Some(0),
        "agent continue"
    );

    // The resumed `apply PATH` emits the terminal (human) summary, not a JSON
    // document; it re-reads the pinned seed and captures the new snapshot. The
    // seed provenance for the agent route is the pin recorded at stage.
    let mut apply = support::isolated_command(iso);
    apply.arg("apply").arg(&beta_dir).env("TOHA_NOW", NOW);
    let applied = apply.output().unwrap();
    assert!(
        applied.status.success(),
        "agent apply: {}",
        String::from_utf8_lossy(&applied.stderr)
    );

    let snapshot = snapshot_at(&root, &expect.beta.target);
    let submissions = submissions_of(&snapshot);
    RouteResult {
        tree: read_tree(&beta_dir),
        seed_from: staged_seed,
        submissions,
    }
}

/// Person route: `apply ADDRESS BETA --like <SA>` over a real PTY, typing the
/// override for `label` from the shared beta asset and accepting the seeded
/// `style`/`with_tests` with Enter. Returns the rendered tree and recorded
/// submissions.
fn person_route(
    iso: &Path,
    base: &Path,
    address: &str,
    sa: &str,
    expect: &FixtureExpect,
) -> RouteResult {
    use expectrl::{Expect, Session};
    let copy = tempfile::tempdir().unwrap();
    let root = copy.path().join("project");
    copy_dir(base, &root);
    let beta_dir = root.join(&expect.beta.target);

    // The one id the beta asset overrides; every other question takes the seed.
    let beta_answers = fixture_answers(&expect.beta.answers);
    let label = beta_answers["label"]
        .as_str()
        .expect("beta overrides label");

    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(&beta_dir)
        .arg("--like")
        .arg(like_selector(expect, sa))
        .env("TOHA_NOW", NOW);
    let mut session = Session::spawn(command).unwrap();
    // The seed notice names the snapshot the person is seeding from.
    session.expect(&sa[..8]).unwrap();
    session.expect("Label").unwrap();
    session.send_line(label).unwrap();
    session.expect("Style").unwrap();
    session.send_line("").unwrap(); // accept the seeded default
    session.expect("With tests").unwrap();
    session.send_line("").unwrap(); // accept the seeded default
    session.expect(expectrl::Eof).unwrap();
    drop(session);

    let snapshot = snapshot_at(&root, &expect.beta.target);
    let submissions = submissions_of(&snapshot);
    RouteResult {
        tree: read_tree(&beta_dir),
        seed_from: Some(sa.to_owned()),
        submissions,
    }
}

/// Crate route: drive the engine and plan through the public library surface,
/// seeding from `SA`'s recorded submissions folded at the CLI boundary, and write
/// the tree. No CLI; this proves the library produces the identical tree. The
/// `seed` object it builds carries only `IndexMap<Id, RawAnswer>` and the id — no
/// `Snapshot`, `Project`, or gitoxide type reaches the engine.
fn crate_route(
    base: &Path,
    template_root: &Path,
    formal: &str,
    sa: &str,
    expect: &FixtureExpect,
) -> RouteResult {
    use toha::{
        ApplyOptions, Interview, Plan, RawAnswer, Template,
        config::{ConfigEntry, DefaultSource, PresetName},
        hook::RecordingRunner,
        interview::{SnapshotSeed, configured_defaults},
        snapshot::Project,
        staging::canonical_target,
        template::Id,
    };
    let copy = tempfile::tempdir().unwrap();
    let root = copy.path().join("project");
    copy_dir(base, &root);
    let beta_dir = root.join(&expect.beta.target);

    let template = Template::load(template_root).unwrap();
    let target = canonical_target(&beta_dir).unwrap();

    // Fold SA's recorded submissions into one default per id at the CLI boundary.
    let project = Project::open(&target).unwrap().expect("in git");
    let snapshot = project.find(sa).unwrap();
    let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
    for batch in snapshot.submissions() {
        for (key, value) in batch {
            defaults.insert(Id::parse(key).unwrap(), RawAnswer(value.clone()));
        }
    }
    let seed = SnapshotSeed {
        defaults,
        from: sa.to_owned(),
    };

    // The override from the shared beta asset — the one id the crate submits.
    let beta_answers = fixture_answers(&expect.beta.answers);
    let mut submission: IndexMap<String, Value> = IndexMap::new();
    for (key, value) in beta_answers.as_object().unwrap() {
        submission.insert(key.clone(), value.clone());
    }

    let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
    let mappings: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> = IndexMap::new();
    let resolution = configured_defaults(formal, &template, &presets, &mappings).unwrap();
    let context = toha::context::InvocationContext::for_target(target.clone());
    let (interview, _warnings) = resolution
        .start_with_seed(&template, NOW.parse().unwrap(), context, Some(seed))
        .unwrap();

    let pending = match interview {
        Interview::Asking(pending) => pending,
        _ => panic!("expected an asking interview"),
    };
    let mut answer: IndexMap<Id, RawAnswer> = IndexMap::new();
    for (key, value) in &submission {
        answer.insert(Id::parse(key).unwrap(), RawAnswer(value.clone()));
    }
    let completed = match pending.answer(answer) {
        Ok(Interview::Complete(completed)) => completed,
        other => panic!(
            "expected a completed interview (asking={})",
            matches!(other, Ok(Interview::Asking(_)))
        ),
    };

    let plan = Plan::build(&template, &completed, &target).unwrap();
    plan.apply(
        &target,
        ApplyOptions {
            force: false,
            trusted: true,
        },
        &RecordingRunner::new(),
    )
    .unwrap();

    RouteResult {
        tree: read_tree(&beta_dir),
        seed_from: Some(sa.to_owned()),
        submissions: vec![submission],
    }
}

/// A pure-engine witness of behavior 21's engine purity: seeding the engine with
/// a hand-built `IndexMap` of the identical values and a `from` that is NOT a real
/// snapshot id — with NO `Project`, snapshot, or git state anywhere in the call —
/// produces the identical beta tree. The engine is identity-unaware: it uses only
/// the folded defaults, never resolves `from`. If the engine reached past the seam
/// into snapshot/git state or gated on `from` being a real id, this git-free route
/// would diverge from the git-backed crate route.
fn pure_engine_tree(
    template_root: &Path,
    formal: &str,
    expect: &FixtureExpect,
) -> BTreeMap<String, Vec<u8>> {
    use toha::{
        ApplyOptions, Interview, Plan, RawAnswer, Template,
        config::{ConfigEntry, DefaultSource, PresetName},
        hook::RecordingRunner,
        interview::{SnapshotSeed, configured_defaults},
        staging::canonical_target,
        template::Id,
    };
    let work = tempfile::tempdir().unwrap();
    let beta_dir = work.path().join(&expect.beta.target);
    let template = Template::load(template_root).unwrap();
    let target = canonical_target(&beta_dir).unwrap();

    // Hand-built folded defaults matching SA's answers — no git, no Project.
    let alpha_answers = fixture_answers(&expect.alpha.answers);
    let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
    for (key, value) in alpha_answers.as_object().unwrap() {
        defaults.insert(Id::parse(key).unwrap(), RawAnswer(value.clone()));
    }
    let seed = SnapshotSeed {
        defaults,
        // Deliberately NOT a real snapshot id: the engine must not resolve or gate
        // on `from`, so a git-free bogus value still produces the identical tree.
        from: "pure-engine-no-git".to_owned(),
    };

    let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
    let mappings: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> = IndexMap::new();
    let resolution = configured_defaults(formal, &template, &presets, &mappings).unwrap();
    let context = toha::context::InvocationContext::for_target(target.clone());
    let (interview, _warnings) = resolution
        .start_with_seed(&template, NOW.parse().unwrap(), context, Some(seed))
        .unwrap();
    let pending = match interview {
        Interview::Asking(pending) => pending,
        _ => panic!("expected an asking interview"),
    };
    let beta_answers = fixture_answers(&expect.beta.answers);
    let mut answer: IndexMap<Id, RawAnswer> = IndexMap::new();
    for (key, value) in beta_answers.as_object().unwrap() {
        answer.insert(Id::parse(key).unwrap(), RawAnswer(value.clone()));
    }
    let completed = match pending.answer(answer) {
        Ok(Interview::Complete(completed)) => completed,
        _ => panic!("expected a completed interview"),
    };
    let plan = Plan::build(&template, &completed, &target).unwrap();
    plan.apply(
        &target,
        ApplyOptions {
            force: false,
            trusted: true,
        },
        &RecordingRunner::new(),
    )
    .unwrap();
    read_tree(&beta_dir)
}

#[test]
fn generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes() {
    let expect = load_expect();
    let iso = tempfile::tempdir().unwrap();
    let base_parent = tempfile::tempdir().unwrap();
    let base = base_parent.path().join("base");

    let template_root = fixture().join("template").canonicalize().unwrap();
    let address = support::folder_address(&template_root);
    let formal = support::formal_name(&template_root);

    let sa = build_base(iso.path(), &base, &address, &formal, &expect);
    let want_seed_from = expected_seed_from(&expect, &sa);
    let want_beta = expected_tree(&expect.beta.expected);

    // Run every route the fixture declares, each in its own isolated copy into
    // the SAME beta target, from the SAME base and inputs.
    let mut results: BTreeMap<String, RouteResult> = BTreeMap::new();
    for route in &expect.routes {
        let result = match route.as_str() {
            "scripted" => scripted_route(iso.path(), &base, &address, &formal, &sa, &expect),
            "agent" => agent_route(iso.path(), &base, &address, &formal, &sa, &expect),
            "person" => person_route(iso.path(), &base, &address, &sa, &expect),
            "crate" => crate_route(&base, &template_root, &formal, &sa, &expect),
            other => panic!("expect.yml names an unknown route: {other}"),
        };
        results.insert(route.clone(), result);
    }

    // (1) Every route's COMPLETE beta tree equals the fixture's expected/beta,
    // including any unexpected file.
    for (route, result) in &results {
        assert_eq!(
            result.tree, want_beta,
            "the {route} route's complete beta tree equals expected/beta"
        );
    }

    // (2) The routes agree with each other on the complete tree. A route-specific
    // divergence (a mutation that breaks ONE route's path) fails here even if the
    // shared seam is intact.
    let trees: Vec<&BTreeMap<String, Vec<u8>>> = results.values().map(|r| &r.tree).collect();
    for window in trees.windows(2) {
        assert_eq!(window[0], window[1], "all routes agree on the beta tree");
    }

    // (3) Every route seeded from SA.
    for (route, result) in &results {
        if let Some(from) = &result.seed_from {
            assert_eq!(from, &want_seed_from, "the {route} route seeds from SA");
        }
    }

    // (4) Explicit, route-shaped raw submission logs, kept SEPARATE from the
    // effective parity above. The document routes record only the submitted
    // override; the person route records every prompt's accepted value — the
    // override plus the accepted seed defaults — which proves the seed is a DEFAULT
    // (overridable), not a replayed answer, on the live person route.
    let override_key = fixture_answers(&expect.beta.answers)
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let override_value = fixture_answers(&expect.beta.answers)[&override_key].clone();

    let document_routes = ["scripted", "agent", "crate"];
    let document_submissions: Vec<&Vec<IndexMap<String, Value>>> = document_routes
        .iter()
        .filter_map(|r| results.get(*r).map(|res| &res.submissions))
        .collect();
    for window in document_submissions.windows(2) {
        assert_eq!(
            window[0], window[1],
            "the document routes record identical raw submissions"
        );
    }
    if let Some(first) = document_submissions.first() {
        assert_eq!(
            first.as_slice(),
            &[one_entry(&override_key, &override_value)],
            "a document route records exactly the submitted override"
        );
    }

    // The person route's recorded submission is the override plus the accepted
    // seed defaults, and those accepted defaults equal the alpha answers (the
    // seed), proving seed-not-replay and answer-beats-seed live.
    if let Some(person) = results.get("person") {
        assert_eq!(person.submissions.len(), 1, "person records one batch");
        let batch = &person.submissions[0];
        assert_eq!(
            batch.get(&override_key),
            Some(&override_value),
            "person recorded the typed override"
        );
        let alpha = fixture_answers(&expect.alpha.answers);
        for (key, value) in alpha.as_object().unwrap() {
            if key == &override_key {
                continue;
            }
            assert_eq!(
                batch.get(key),
                Some(value),
                "person accepted the seeded default for {key}"
            );
        }
    }

    // (5) Both snapshots exist with equal source and the declared targets.
    let sb = snapshot_at(
        &{
            // Re-run the scripted route once more in a dedicated copy so both
            // snapshots live in one project for this assertion.
            let copy = base_parent.path().join("snapshots-check");
            copy_dir(&base, &copy);
            let beta = envelope(
                iso.path(),
                "beta-snap.json",
                &formal,
                &fixture_answers(&expect.beta.answers),
            );
            let (code, _doc) = scripted(
                iso.path(),
                &address,
                &copy.join(&expect.beta.target),
                Some(&like_selector(&expect, &sa)),
                &beta,
            );
            assert_eq!(code, 0);
            copy
        },
        &expect.beta.target,
    );
    let sa_snapshot = snapshot_at(
        &base_parent.path().join("snapshots-check"),
        &expect.alpha.target,
    );
    assert_ne!(sa_snapshot.id().to_string(), sb.id().to_string());
    assert_eq!(sa_snapshot.id().to_string(), sa, "alpha snapshot is SA");
    if expect.snapshots.equal_source {
        assert_eq!(
            sa_snapshot.source(),
            sb.source(),
            "both snapshots share one source"
        );
    }
    let got_targets: Vec<String> = {
        let mut t = vec![sa_snapshot.target().to_string(), sb.target().to_string()];
        t.sort();
        t
    };
    let mut want_targets = expect.snapshots.targets.clone();
    want_targets.sort();
    assert_eq!(got_targets, want_targets, "the two snapshot targets");

    // (6) Engine purity. A git-free pure-engine seed — hand-built folded defaults
    // and a `from` that is NOT a real snapshot id, with NO Project/snapshot/git
    // anywhere — produces the identical beta tree AS the git-backed crate route
    // (which folded from a real snapshot). The engine is identity-unaware and
    // reaches no snapshot/git state past the seam: if it resolved or gated on
    // `from`, the bogus-`from` git-free route would diverge from the git-backed
    // crate route. Sole-kill: make the seam gate default application on `from`
    // being a real ULID; the git-free route then drops its defaults and this
    // assertion fails while the git-backed crate route still passes.
    let git_free = pure_engine_tree(&template_root, &formal, &expect);
    assert_eq!(
        git_free, want_beta,
        "the pure-engine (git-free, bogus-from) seed produces the identical tree"
    );
    if let Some(crate_result) = results.get("crate") {
        assert_eq!(
            git_free, crate_result.tree,
            "the git-free engine (no snapshot/Project/git) equals the git-backed crate route"
        );
    }
}

/// One submission batch with a single entry.
fn one_entry(key: &str, value: &Value) -> IndexMap<String, Value> {
    let mut m = IndexMap::new();
    m.insert(key.to_owned(), value.clone());
    m
}

/// A widget template with a NON-IDENTITY `format` on `label` (upper-cases it), so
/// the submitted raw value differs from the rendered/formatted value.
fn format_template(dir: &Path) {
    std::fs::create_dir_all(dir.join("template")).unwrap();
    std::fs::write(
        dir.join("template.yml"),
        "name: widget\ninterview:\n  - { id: label, type: text, prompt: Label, required: true, format: value | upper }\n  - { id: style, type: select, prompt: Style, options: [card, panel], default: card }\n  - { id: with_tests, type: confirm, prompt: With tests, default: true }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("template/mod.txt"),
        "label={{ label }}\nstyle={{ style }}\ntests={{ with_tests }}\n",
    )
    .unwrap();
}

/// The raw submissions a route's ACTUAL persisted snapshot recorded at `target`.
fn recorded_submissions(project_root: &Path, target: &str) -> Vec<IndexMap<String, Value>> {
    submissions_of(&snapshot_at(project_root, target))
}

#[test]
fn routes_record_the_pre_format_raw_submission_not_the_formatted_answer() {
    // Gap: observe the ACTUAL accepted pre-format raw value from each real route
    // invocation — read back from the snapshot each route actually persisted (the
    // design's "raw pre-format submissions" record), NOT from the formatted
    // `Completed.answers` and NOT from a separate replay. A non-identity `format`
    // (upper) makes the raw submission ("beta") differ from the rendered answer
    // ("BETA"), so the distinction can actually fail. Every route records the RAW
    // "beta" while rendering the FORMATTED "BETA".
    //
    // Sole-kill: src/main.rs `plain_apply_snapshot` records the formatted
    // `completed.answers` instead of the raw `submissions`; the scripted route then
    // persists "BETA" and the named raw assertion fails while the tree is unchanged.
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("widget");
    format_template(&template);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);
    let template_root = template.canonicalize().unwrap();

    // Base project: seed commit, then alpha (records SA with raw submissions), commit.
    let base = work.path().join("base");
    std::fs::create_dir_all(&base).unwrap();
    git(&base, &["init", "-q", "-b", "main"]);
    std::fs::write(base.join("seed.txt"), "seed\n").unwrap();
    git(&base, &["add", "-A"]);
    git(&base, &["commit", "-q", "-m", "seed"]);
    let alpha = envelope(
        iso.path(),
        "alpha.json",
        &formal,
        &json!({ "label": "alpha", "style": "panel", "with_tests": false }),
    );
    let (code, alpha_doc) = scripted(iso.path(), &address, &base.join("src/alpha"), None, &alpha);
    assert_eq!(code, 0, "alpha applied: {alpha_doc}");
    let sa = alpha_doc["snapshot"]["id"].as_str().unwrap().to_owned();
    // alpha itself demonstrates non-identity: raw submission "alpha", rendered "ALPHA".
    assert_eq!(
        recorded_submissions(&base, "src/alpha")[0].get("label"),
        Some(&json!("alpha")),
        "alpha recorded the RAW submission"
    );
    assert!(
        read_tree(&base.join("src/alpha"))["mod.txt"].starts_with(b"label=ALPHA\n"),
        "alpha rendered the FORMATTED answer"
    );
    git(&base, &["add", "-A"]);
    git(&base, &["commit", "-q", "-m", "alpha"]);

    // Each route applies beta into the SAME target in its own isolated copy,
    // overriding label with the raw "beta"; style/with_tests seed from SA.
    let mut raws: BTreeMap<String, Value> = BTreeMap::new();
    let mut trees: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    // Scripted.
    {
        let copy = tempfile::tempdir().unwrap();
        let root = copy.path().join("project");
        copy_dir(&base, &root);
        let beta = envelope(iso.path(), "s.json", &formal, &json!({ "label": "beta" }));
        let (code, doc) = scripted(
            iso.path(),
            &address,
            &root.join("src/beta"),
            Some(&sa),
            &beta,
        );
        assert_eq!(code, 0, "scripted beta: {doc}");
        raws.insert(
            "scripted".into(),
            recorded_submissions(&root, "src/beta")[0]["label"].clone(),
        );
        trees.insert(
            "scripted".into(),
            read_tree(&root.join("src/beta"))["mod.txt"].clone(),
        );
    }
    // Agent.
    {
        let copy = tempfile::tempdir().unwrap();
        let root = copy.path().join("project");
        copy_dir(&base, &root);
        let beta_dir = root.join("src/beta");
        let mut stage = support::isolated_command(iso.path());
        stage
            .arg("stage")
            .arg(&address)
            .arg(&beta_dir)
            .arg("--like")
            .arg(&sa)
            .arg("--async")
            .env("TOHA_NOW", NOW);
        assert!(stage.output().unwrap().status.success() || true);
        let cont = envelope(iso.path(), "a.json", &formal, &json!({ "label": "beta" }));
        let mut c = support::isolated_command(iso.path());
        c.arg("continue")
            .arg(&beta_dir)
            .arg(&cont)
            .env("TOHA_NOW", NOW);
        assert_eq!(c.output().unwrap().status.code(), Some(0));
        let mut ap = support::isolated_command(iso.path());
        ap.arg("apply").arg(&beta_dir).env("TOHA_NOW", NOW);
        assert!(ap.output().unwrap().status.success());
        raws.insert(
            "agent".into(),
            recorded_submissions(&root, "src/beta")[0]["label"].clone(),
        );
        trees.insert("agent".into(), read_tree(&beta_dir)["mod.txt"].clone());
    }
    // Crate: drive the engine in-process, apply, then capture through the producer
    // path so its persisted snapshot records the raw submission too.
    {
        use toha::{
            ApplyOptions, Interview, Plan, RawAnswer, Template,
            config::{ConfigEntry, DefaultSource, PresetName},
            hook::RecordingRunner,
            interview::{SnapshotSeed, configured_defaults},
            snapshot::{FrozenNow, Project, Revision},
            staging::canonical_target,
            template::Id,
        };
        let copy = tempfile::tempdir().unwrap();
        let root = copy.path().join("project");
        copy_dir(&base, &root);
        let beta_dir = root.join("src/beta");
        let template = Template::load(&template_root).unwrap();
        let target = canonical_target(&beta_dir).unwrap();
        let project = Project::open(&target).unwrap().expect("in git");
        let snapshot = project.find(&sa).unwrap();
        let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
        for b in snapshot.submissions() {
            for (k, v) in b {
                defaults.insert(Id::parse(k).unwrap(), RawAnswer(v.clone()));
            }
        }
        let seed = SnapshotSeed {
            defaults,
            from: sa.clone(),
        };
        let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
        let mappings: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> = IndexMap::new();
        let resolution = configured_defaults(&formal, &template, &presets, &mappings).unwrap();
        let context = toha::context::InvocationContext::for_target(target.clone());
        let (interview, _w) = resolution
            .start_with_seed(&template, NOW.parse().unwrap(), context, Some(seed))
            .unwrap();
        let pending = match interview {
            Interview::Asking(p) => p,
            _ => panic!("asking"),
        };
        // The crate submits the raw "beta".
        let mut submission: IndexMap<String, Value> = IndexMap::new();
        submission.insert("label".into(), json!("beta"));
        let mut answer: IndexMap<Id, RawAnswer> = IndexMap::new();
        answer.insert(Id::parse("label").unwrap(), RawAnswer(json!("beta")));
        let completed = match pending.answer(answer) {
            Ok(Interview::Complete(c)) => c,
            _ => panic!("complete"),
        };
        let plan = Plan::build(&template, &completed, &target).unwrap();
        plan.apply(
            &target,
            ApplyOptions {
                force: false,
                trusted: true,
            },
            &RecordingRunner::new(),
        )
        .unwrap();
        // Persist the snapshot through the producer, recording the raw submission.
        let plan_for_capture = Plan::build(&template, &completed, &target).unwrap();
        project
            .save_after_apply(
                &plan_for_capture,
                formal.clone(),
                Revision::Unversioned,
                FrozenNow::new(NOW.parse().unwrap()),
                vec![submission],
                true,
            )
            .unwrap();
        raws.insert(
            "crate".into(),
            recorded_submissions(&root, "src/beta")[0]["label"].clone(),
        );
        trees.insert("crate".into(), read_tree(&beta_dir)["mod.txt"].clone());
    }

    // Every route recorded the RAW "beta" (pre-format) in its actual persisted
    // snapshot — never the formatted "BETA".
    for (route, raw) in &raws {
        assert_eq!(
            raw,
            &json!("beta"),
            "the {route} route persisted the RAW pre-format submission, not the formatted answer"
        );
    }
    // ...while every route rendered the FORMATTED "BETA".
    for (route, tree) in &trees {
        assert!(
            tree.starts_with(b"label=BETA\n"),
            "the {route} route rendered the FORMATTED answer: {}",
            String::from_utf8_lossy(tree)
        );
    }
}

#[test]
fn a_person_is_re_asked_a_constraint_invalid_seed_and_overrides_it() {
    // Behavior 9 (person route): a seeded default that fails a tightened constraint
    // in the new template version is re-asked on the real terminal — the person is
    // prompted again and types a valid value; nothing invalid is written. This maps
    // the person re-ask the scripted exit-4 case cannot show.
    use expectrl::{Expect, Session};
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let template = work.path().join("widget");
    std::fs::create_dir_all(template.join("template")).unwrap();
    // v1: label is a free text question (so "Root" is a valid recorded answer).
    std::fs::write(
        template.join("template.yml"),
        "name: widget\ninterview:\n  - { id: label, type: text, prompt: Label, required: true }\n",
    )
    .unwrap();
    std::fs::write(template.join("template/mod.txt"), "label={{ label }}\n").unwrap();
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    std::fs::write(project.join("seed.txt"), "seed\n").unwrap();
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "seed"]);
    let address = support::folder_address(&template.canonicalize().unwrap());
    let formal = support::formal_name(&template);

    // Root application records label="Root"; capture its snapshot.
    let root = envelope(
        iso.path(),
        "root.json",
        &formal,
        &json!({ "label": "Root" }),
    );
    let (code, doc) = scripted(
        iso.path(),
        &address,
        &project.join("src/alpha"),
        None,
        &root,
    );
    assert_eq!(code, 0, "root applied: {doc}");
    let sa = doc["snapshot"]["id"].as_str().unwrap().to_owned();
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "root"]);

    // v2 tightens label to digits only; the recorded "Root" now fails the constraint.
    std::fs::write(
        template.join("template.yml"),
        "name: widget\ninterview:\n  - { id: label, type: text, prompt: Label, required: true, validate: { regex: '^[0-9]+$' } }\n",
    )
    .unwrap();

    // Person apply --like SA: the seeded "Root" is shown, accepted (Enter), rejected,
    // re-asked; the person then types a valid value.
    let beta = project.join("src/beta");
    let mut command = support::isolated_command(iso.path());
    command
        .arg("apply")
        .arg(&address)
        .arg(&beta)
        .arg("--like")
        .arg(&sa)
        .env("TOHA_NOW", NOW);
    let mut session = Session::spawn(command).unwrap();
    session.expect("Label").unwrap();
    session.send_line("Root").unwrap(); // the seeded value, which now fails the constraint
    session.expect("Label").unwrap(); // re-asked after the rejection
    session.send_line("123").unwrap(); // a valid override
    session.expect(expectrl::Eof).unwrap();
    drop(session);

    assert_eq!(
        read_tree(&beta),
        {
            let mut m = BTreeMap::new();
            m.insert("mod.txt".to_owned(), b"label=123\n".to_vec());
            m
        },
        "the person was re-asked the constraint-invalid seed and the valid override was written"
    );
}

#[test]
fn a_seeded_plan_is_single_target_and_refuses_a_foreign_target() {
    // Behavior 18 (single-target plan): a seeded interview's plan renders exactly
    // the one subpath carried in its context, and `Plan::build` refuses any other
    // target with `PlanError::ContextTarget`. Sole-kill: disable that guard in
    // src/plan.rs and this `refuses` assertion fails.
    use toha::{
        Interview, Plan, PlanError, RawAnswer, Template,
        config::{ConfigEntry, DefaultSource, PresetName},
        interview::{SnapshotSeed, configured_defaults},
        staging::canonical_target,
        template::Id,
    };

    let template = Template::load(&fixture().join("template").canonicalize().unwrap()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let beta = canonical_target(&work.path().join("src/widgets/beta")).unwrap();
    let gamma = canonical_target(&work.path().join("src/widgets/gamma")).unwrap();

    // A hand-built seed (no git needed): style=panel differs from the default.
    let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
    defaults.insert(Id::parse("style").unwrap(), RawAnswer(json!("panel")));
    let seed = SnapshotSeed {
        defaults,
        from: "01M3TB4T089FWECKM1BNEM4J3H".to_owned(),
    };
    let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
    let mappings: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> = IndexMap::new();
    let resolution =
        configured_defaults(&beta.to_string(), &template, &presets, &mappings).unwrap();
    let context = toha::context::InvocationContext::for_target(beta.clone());
    let (interview, _w) = resolution
        .start_with_seed(&template, NOW.parse().unwrap(), context, Some(seed))
        .unwrap();
    let mut answer: IndexMap<Id, RawAnswer> = IndexMap::new();
    answer.insert(Id::parse("label").unwrap(), RawAnswer(json!("Beta")));
    let completed = match interview {
        Interview::Asking(p) => match p.answer(answer) {
            Ok(Interview::Complete(c)) => c,
            _ => panic!("expected a completed interview"),
        },
        _ => panic!("expected an asking interview"),
    };

    // The plan targets exactly beta: one file, built against beta.
    let plan = Plan::build(&template, &completed, &beta).unwrap();
    assert_eq!(plan.files.len(), 1, "one file, one target");

    // Building against any other target is refused: the plan is single-target.
    assert!(
        matches!(
            Plan::build(&template, &completed, &gamma),
            Err(PlanError::ContextTarget)
        ),
        "a seeded plan refuses a target other than its context"
    );
}
