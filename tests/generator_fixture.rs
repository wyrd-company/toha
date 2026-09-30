// ---
// relationships:
//   implements: command-line-interface
//   verifies: interview-protocol
// ---
//! The mandated in-project-generators fixture harness (design.md:409,417,507,540).
//!
//! It consumes the committed `tests/fixtures/generator-twice/` assets — the
//! `widget` `template/`, `answers-alpha.json`/`answers-beta.json`, the
//! `expected/` trees, and `expect.yml` — rather than an inline template. One
//! generator is applied twice into one clean git repository: first into
//! `src/widgets/alpha` (recording snapshot SA), then into `src/widgets/beta`
//! with `--like <SA>`, so beta inherits `style`/`with_tests` from SA and
//! overrides only `label`. The second application is driven through all four
//! routes — scripted, agent, real terminal/person (a PTY), and the crate's
//! public library surface — and every route must produce the identical `beta`
//! tree from the identical inputs. Both snapshots must exist with equal source
//! and targets {alpha, beta}.

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

const NOW: &str = "2026-01-02T03:04:05+00:00[UTC]";

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

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generator-twice")
}

/// The bare answers object recorded in a fixture `answers-*.json`.
fn fixture_answers(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture().join(name)).unwrap()).unwrap()
}

/// Wrap bare answers in the identity-bearing envelope the routes require.
fn envelope(iso: &Path, name: &str, formal: &str, answers: &Value) -> PathBuf {
    let path = iso.join(name);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "template": formal, "answers": answers })).unwrap(),
    )
    .unwrap();
    path
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// The expected bytes of the fixture's `expected/<which>/mod.txt`.
fn expected(which: &str) -> String {
    read(&fixture().join("expected").join(which).join("mod.txt"))
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

#[test]
fn generator_twice_fixture_agrees_across_person_scripted_agent_and_crate_routes() {
    let iso = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("project");
    std::fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    std::fs::write(project.join("seed.txt"), "seed\n").unwrap();
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "seed"]);

    let template_root = fixture().join("template").canonicalize().unwrap();
    let address = support::folder_address(&template_root);
    let formal = support::formal_name(&template_root);

    // First application into src/widgets/alpha, recording snapshot SA.
    let alpha_answers = envelope(
        iso.path(),
        "alpha.json",
        &formal,
        &fixture_answers("answers-alpha.json"),
    );
    let (code, alpha_doc) = scripted(
        iso.path(),
        &address,
        &project.join("src/widgets/alpha"),
        None,
        &alpha_answers,
    );
    assert_eq!(code, 0, "alpha applied: {alpha_doc}");
    let sa = alpha_doc["snapshot"]["id"]
        .as_str()
        .expect("alpha recorded snapshot SA")
        .to_owned();
    assert_eq!(
        read(&project.join("src/widgets/alpha/mod.txt")),
        expected("alpha")
    );
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "alpha"]);

    // A >=6-char prefix of SA names it (only SA exists of this source so far).
    let sa_prefix = &sa[..8];
    let beta_answers = fixture_answers("answers-beta.json");

    // --- Route 1: scripted, into the canonical src/widgets/beta. ---
    let beta_env = envelope(iso.path(), "beta.json", &formal, &beta_answers);
    let (code, beta_doc) = scripted(
        iso.path(),
        &address,
        &project.join("src/widgets/beta"),
        Some(sa_prefix),
        &beta_env,
    );
    assert_eq!(code, 0, "beta (scripted) applied: {beta_doc}");
    assert_eq!(beta_doc["seed"]["from"].as_str(), Some(sa.as_str()));
    let sb = beta_doc["snapshot"]["id"]
        .as_str()
        .expect("beta recorded SB")
        .to_owned();
    let scripted_tree = read(&project.join("src/widgets/beta/mod.txt"));
    assert_eq!(
        scripted_tree,
        expected("beta"),
        "scripted route matches expected/beta"
    );
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "beta"]);

    // --- Route 2: agent (stage --async → continue → apply PATH). ---
    let agent_dir = project.join("src/widgets/beta_agent");
    let mut stage = support::isolated_command(iso.path());
    stage
        .arg("stage")
        .arg(&address)
        .arg(&agent_dir)
        .arg("--like")
        .arg(&sa)
        .arg("--async")
        .env("TOHA_NOW", NOW);
    let staged = support::first_document(&stage.output().unwrap().stdout);
    assert_eq!(staged["seed"]["from"].as_str(), Some(sa.as_str()));
    let cont = envelope(iso.path(), "agent.json", &formal, &beta_answers);
    let mut cont_cmd = support::isolated_command(iso.path());
    cont_cmd
        .arg("continue")
        .arg(&agent_dir)
        .arg(&cont)
        .env("TOHA_NOW", NOW);
    assert_eq!(cont_cmd.output().unwrap().status.code(), Some(0));
    let mut apply = support::isolated_command(iso.path());
    apply.arg("apply").arg(&agent_dir).env("TOHA_NOW", NOW);
    assert!(apply.output().unwrap().status.success());
    let agent_tree = read(&agent_dir.join("mod.txt"));
    assert_eq!(
        agent_tree,
        expected("beta"),
        "agent route matches expected/beta"
    );

    // --- Route 3: real terminal / person, driven through a PTY. ---
    let person_tree = person_route(
        iso.path(),
        &address,
        &project.join("src/widgets/beta_person"),
        &sa,
    );
    assert_eq!(
        person_tree,
        expected("beta"),
        "person route matches expected/beta"
    );

    // --- Route 4: the crate's public library surface. ---
    let crate_tree = crate_route(
        &project.join("src/widgets/beta_crate"),
        &template_root,
        &formal,
        &sa,
    );
    assert_eq!(
        crate_tree,
        expected("beta"),
        "crate route matches expected/beta"
    );

    // All four routes produced the identical tree from identical inputs.
    assert_eq!(scripted_tree, agent_tree);
    assert_eq!(scripted_tree, person_tree);
    assert_eq!(scripted_tree, crate_tree);

    // alpha is unchanged.
    assert_eq!(
        read(&project.join("src/widgets/alpha/mod.txt")),
        expected("alpha")
    );

    // Two snapshots exist (alpha's SA, beta's SB), equal source, targets alpha/beta.
    assert_ne!(sa, sb);
    let at_alpha = snapshot_list(iso.path(), &project.join("src/widgets/alpha"));
    let at_beta = snapshot_list(iso.path(), &project.join("src/widgets/beta"));
    assert_eq!(at_alpha[0]["id"].as_str(), Some(sa.as_str()));
    assert_eq!(at_alpha[0]["target"].as_str(), Some("src/widgets/alpha"));
    assert_eq!(at_beta[0]["id"].as_str(), Some(sb.as_str()));
    assert_eq!(at_beta[0]["target"].as_str(), Some("src/widgets/beta"));
    assert_eq!(
        at_alpha[0]["source"].as_str(),
        at_beta[0]["source"].as_str(),
        "both snapshots share one source"
    );
}

#[test]
fn a_seeded_plan_is_single_target_and_refuses_a_foreign_target() {
    // Behavior 18 (single-target plan): a seeded interview's plan renders exactly
    // the one subpath carried in its context, and `Plan::build` refuses any other
    // target with `PlanError::ContextTarget`. Sole-kill: remove that guard in
    // src/plan.rs and this `refuses` assertion fails.
    use indexmap::IndexMap;
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

/// The valid snapshots `snapshots list --json` reports for one target.
fn snapshot_list(iso: &Path, target: &Path) -> Vec<Value> {
    let mut list = support::isolated_command(iso);
    list.arg("snapshots").arg("list").arg(target).arg("--json");
    let doc = support::first_document(&list.output().unwrap().stdout);
    doc["snapshots"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.get("id").is_some())
        .collect()
}

/// The person route: `apply TEMPLATE PATH --like <SA>` in a real terminal (PTY).
/// The seeded defaults are shown; the person types `Beta` for `label` and accepts
/// the seeded `style` and `with_tests` with Enter. Returns the rendered tree.
fn person_route(iso: &Path, address: &str, target: &Path, sa_prefix: &str) -> String {
    use expectrl::{Expect, Session};
    let mut command = support::isolated_command(iso);
    command
        .arg("apply")
        .arg(address)
        .arg(target)
        .arg("--like")
        .arg(sa_prefix)
        .env("TOHA_NOW", NOW);
    let mut session = Session::spawn(command).unwrap();
    // The seed notice, then each prompt with its seeded default.
    session.expect("Label").unwrap();
    session.send_line("Beta").unwrap();
    session.expect("Style").unwrap();
    session.send_line("").unwrap(); // accept the seeded default (card)
    session.expect("With tests").unwrap();
    session.send_line("").unwrap(); // accept the seeded default (true)
    session.expect(expectrl::Eof).unwrap();
    read(&target.join("mod.txt"))
}

/// The crate route: drive the engine and plan through public library surfaces,
/// seeding from SA's recorded submissions, and write the tree. No CLI, no
/// snapshot capture — this proves the library produces the identical tree.
fn crate_route(target_path: &Path, template_root: &Path, formal: &str, sa: &str) -> String {
    use indexmap::IndexMap;
    use toha::{
        ApplyOptions, Interview, Plan, RawAnswer, Template,
        config::{ConfigEntry, DefaultSource, PresetName},
        hook::RecordingRunner,
        interview::{SnapshotSeed, configured_defaults},
        snapshot::Project,
        staging::canonical_target,
        template::Id,
    };

    let template = Template::load(template_root).unwrap();
    let target = canonical_target(target_path).unwrap();

    // Fold SA's recorded submissions into one default per id (public reader).
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

    let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
    let mappings: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> = IndexMap::new();
    let resolution = configured_defaults(formal, &template, &presets, &mappings).unwrap();
    let context = toha::context::InvocationContext::for_target(target.clone());
    let (interview, _warnings) = resolution
        .start_with_seed(&template, NOW.parse().unwrap(), context, Some(seed))
        .unwrap();

    // Answer only label; style/with_tests take the seeded defaults.
    let pending = match interview {
        Interview::Asking(pending) => pending,
        _ => panic!("expected an asking interview"),
    };
    let mut answer: IndexMap<Id, RawAnswer> = IndexMap::new();
    answer.insert(Id::parse("label").unwrap(), RawAnswer(json!("Beta")));
    let completed = match pending.answer(answer) {
        Ok(Interview::Complete(completed)) => completed,
        other => panic!(
            "expected a completed interview: {}",
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
    read(&target_path.join("mod.txt"))
}
