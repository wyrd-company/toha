// ---
// relationships:
//   implements:
//     - template-format
//     - interview-protocol
//   references:
//     - error-attribution
// ---
//! Named sole-kill and behavior proofs for the Toha-owned Jinja context, driven
//! through the public API. Projected values are observed by probing a computed
//! node, because the projection itself is crate-private.

use std::cell::Cell;

use toha::config::{ConfigEntry, ConfigLayer, ConfigOrigin, DefaultSource};
use toha::context::{
    EnvironmentDecision, EnvironmentSnapshot, ExecutionFacts, FixedEnvironment,
    FixedEnvironmentSource, HostFacts, InvocationContext, SelectedTemplate,
};
use toha::interview::configured_defaults;
use toha::staging::{StagedRecord, canonical_target};
use toha::{Answer, Id, Interview, RawAnswer, Seed, Template};

const NOW: &str = "2026-01-02T03:04:05+00:00[UTC]";

/// A fixed source returning known values and counting captures.
#[derive(Default)]
struct SpySource {
    captures: Cell<u32>,
}
impl FixedEnvironmentSource for &SpySource {
    fn capture(&mut self) -> FixedEnvironment {
        self.captures.set(self.captures.get() + 1);
        FixedEnvironment::new(
            Some("spy-user".into()),
            Some("spy-host".into()),
            Some("spy-editor".into()),
            Some("spy-shell".into()),
            Some("spy-visual".into()),
        )
    }
}

fn load(yaml: &str) -> (tempfile::TempDir, Template) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), yaml).unwrap();
    let template = Template::load(dir.path()).unwrap();
    (dir, template)
}

fn a_target() -> (tempfile::TempDir, toha::staging::CanonicalTarget) {
    let dir = tempfile::tempdir().unwrap();
    let target = canonical_target(dir.path()).unwrap();
    (dir, target)
}

fn context_with(
    target: &toha::staging::CanonicalTarget,
    environment: EnvironmentSnapshot,
) -> InvocationContext {
    InvocationContext::new(
        target.clone(),
        SelectedTemplate::new(
            "example.test/widget".into(),
            "widget".into(),
            vec!["w".into(), "wid".into()],
            Some("example.test/widget".into()),
        ),
        HostFacts::new(
            "linux".into(),
            "x86_64".into(),
            Some("Sample Linux".into()),
            Some("sample".into()),
            vec!["debian".into()],
        ),
        ExecutionFacts::new(true, false),
        environment,
    )
    .unwrap()
}

/// Projects `context` and returns the value a computed node reading `name` sees.
fn probe(context: InvocationContext, name: &str) -> Answer {
    let yaml = format!("name: obs\ninterview:\n  - {{ id: probe, computed: {name} }}\n");
    let (_dir, template) = load(&yaml);
    let interview = Interview::start(
        &template,
        Seed {
            now: NOW.parse().unwrap(),
            defaults: Default::default(),
            context,
        },
    )
    .unwrap();
    let Interview::Complete(completed) = interview else {
        panic!("probe interview did not complete");
    };
    completed
        .answers
        .get(&Id::parse("probe").unwrap())
        .expect("probe answer")
        .clone()
}

// A template with no fixed-environment reference, and one with a reference.
const NO_REF: &str = "name: widget\ninterview:\n  - { id: q, type: text, prompt: Q? }\n";
const WITH_REF: &str =
    "name: widget\ninterview:\n  - { id: q, type: text, prompt: '{{ toha_env_editor }}' }\n";

#[test]
fn stage_no_refs_without_trust_records_unavailable_without_reads() {
    let (_dir, template) = load(NO_REF);
    let spy = SpySource::default();
    let snapshot = template
        .admit_environment(EnvironmentDecision::RequireStageGrant, &mut &spy)
        .unwrap();
    assert!(matches!(snapshot, EnvironmentSnapshot::Unavailable));
    assert_eq!(spy.captures.get(), 0, "no need without trust reads nothing");
    // The snapshot projects five nulls.
    let (_t, target) = a_target();
    assert_eq!(
        probe(context_with(&target, snapshot), "toha_env_editor"),
        Answer::Value(serde_json::Value::Null),
    );
}

#[test]
fn stage_no_refs_with_trust_captures_fixed_five_once() {
    let (_dir, template) = load(NO_REF);
    let spy = SpySource::default();
    let snapshot = template
        .admit_environment(EnvironmentDecision::CarryStageGrant, &mut &spy)
        .unwrap();
    assert!(matches!(snapshot, EnvironmentSnapshot::Captured(_)));
    assert_eq!(spy.captures.get(), 1, "an explicit grant captures once");
    let (_t, target) = a_target();
    assert_eq!(
        probe(context_with(&target, snapshot), "toha_env_editor"),
        Answer::Value(serde_json::json!("spy-editor")),
    );
}

#[test]
fn stage_refs_without_trust_fails_before_progress() {
    let (_dir, template) = load(WITH_REF);
    let spy = SpySource::default();
    let error = template
        .admit_environment(EnvironmentDecision::RequireStageGrant, &mut &spy)
        .unwrap_err();
    assert!(
        error.to_string().contains("requires stage trust"),
        "{error}"
    );
    assert_eq!(spy.captures.get(), 0, "a refusal reads nothing");
}

#[test]
fn stage_refs_with_trust_captures_fixed_five_once() {
    let (_dir, template) = load(WITH_REF);
    let spy = SpySource::default();
    let snapshot = template
        .admit_environment(EnvironmentDecision::CarryStageGrant, &mut &spy)
        .unwrap();
    assert!(matches!(snapshot, EnvironmentSnapshot::Captured(_)));
    assert_eq!(spy.captures.get(), 1, "captures each field exactly once");
    let (_t, target) = a_target();
    assert_eq!(
        probe(context_with(&target, snapshot), "toha_env_shell"),
        Answer::Value(serde_json::json!("spy-shell")),
    );
}

#[test]
fn all_surfaces_source_body_reference_drives_the_need() {
    // An environment reference in a source-tree file body — not the interview —
    // is analyzed at load, so a no-trust stage refuses before progress.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), "name: widget\n").unwrap();
    std::fs::write(
        dir.path().join("template").join("note.txt"),
        "editor is {{ toha_env_editor }}\n",
    )
    .unwrap();
    let template = Template::load(dir.path()).unwrap();
    let spy = SpySource::default();
    assert!(
        template
            .admit_environment(EnvironmentDecision::RequireStageGrant, &mut &spy)
            .is_err(),
        "a source-tree body reference is a need",
    );
    assert_eq!(spy.captures.get(), 0);
}

#[test]
fn direct_denial_reads_nothing() {
    let (_dir, template) = load(WITH_REF);
    let spy = SpySource::default();
    let snapshot = template
        .admit_environment(EnvironmentDecision::Deny, &mut &spy)
        .unwrap();
    assert!(matches!(snapshot, EnvironmentSnapshot::Unavailable));
    assert_eq!(spy.captures.get(), 0);
}

#[test]
fn captured_values_are_redacted_in_debug() {
    let (_dir, template) = load(WITH_REF);
    let spy = SpySource::default();
    let snapshot = template
        .admit_environment(EnvironmentDecision::GrantIfNeeded, &mut &spy)
        .unwrap();
    let rendered = format!("{snapshot:?}");
    assert!(!rendered.contains("spy-editor"), "{rendered}");
    assert!(!rendered.contains("spy-user"), "{rendered}");
}

#[test]
fn identity_and_host_values_project_with_exact_types() {
    let (_t, target) = a_target();
    let ctx = || context_with(&target, EnvironmentSnapshot::Unavailable);
    assert_eq!(
        probe(ctx(), "toha_template_name"),
        Answer::Value(serde_json::json!("widget"))
    );
    assert_eq!(
        probe(ctx(), "toha_template_formal_name"),
        Answer::Value(serde_json::json!("example.test/widget"))
    );
    assert_eq!(
        probe(ctx(), "toha_template_aliases"),
        Answer::Value(serde_json::json!(["w", "wid"]))
    );
    assert_eq!(
        probe(ctx(), "toha_host_os"),
        Answer::Value(serde_json::json!("linux"))
    );
    assert_eq!(
        probe(ctx(), "toha_host_os_id_like"),
        Answer::Value(serde_json::json!(["debian"]))
    );
    assert_eq!(
        probe(ctx(), "toha_is_admin"),
        Answer::Value(serde_json::json!(true))
    );
    assert_eq!(
        probe(ctx(), "toha_is_interactive"),
        Answer::Value(serde_json::json!(false))
    );
}

#[test]
fn reserved_name_collision_is_rejected_and_neighbors_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(
        dir.path().join("template.yml"),
        "name: widget\ndata: { toha_env_user: x }\ninterview:\n  - { id: toha_host_os, type: text, prompt: Q? }\n",
    )
    .unwrap();
    let error = Template::load(dir.path()).unwrap_err().to_string();
    assert!(error.contains("toha_env_user"), "{error}");
    assert!(error.contains("toha_host_os"), "{error}");
    assert!(error.contains("reserved"), "{error}");
    // A neighboring toha_ name is legal.
    let (_dir, _template) = load(
        "name: widget\ndata: { toha_custom: x }\ninterview:\n  - { id: toha_other, type: text, prompt: Q? }\n",
    );
}

// Configured-default origin preservation. A select question whose configured
// default is not an allowed option is rejected when the question is reached; the
// rejection names the winning mapping origin.
const SELECT: &str = "name: widget\ninterview:\n  - { id: mode, type: select, prompt: Mode?, options: [fast, slow] }\n";

fn invalid_mapping()
-> indexmap::IndexMap<String, indexmap::IndexMap<Id, ConfigEntry<DefaultSource>>> {
    let mut inner = indexmap::IndexMap::new();
    inner.insert(
        Id::parse("mode").unwrap(),
        ConfigEntry {
            value: DefaultSource::Literal(serde_json::json!("medium")),
            origin: ConfigOrigin {
                layer: ConfigLayer::User,
                path: "user.yml".into(),
            },
        },
    );
    let mut outer = indexmap::IndexMap::new();
    outer.insert("example.test/widget".into(), inner);
    outer
}

fn batch_rejection(interview: Interview) -> String {
    let Interview::Asking(pending) = interview else {
        panic!("expected a batch");
    };
    pending
        .batch()
        .errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn configured_context_start_preserves_constraint_origin() {
    let (_dir, template) = load(SELECT);
    let (_t, target) = a_target();
    let resolution = configured_defaults(
        "example.test/widget",
        &template,
        &Default::default(),
        &invalid_mapping(),
    )
    .unwrap();
    let interview = resolution
        .start_with_context(
            &template,
            NOW.parse().unwrap(),
            context_with(&target, EnvironmentSnapshot::Unavailable),
        )
        .unwrap();
    let message = batch_rejection(interview);
    assert!(message.contains("user.yml"), "{message}");
    assert!(message.contains("template-defaults"), "{message}");
}

#[test]
fn configured_context_replay_preserves_constraint_origin() {
    let (_dir, template) = load(SELECT);
    let (_t, target) = a_target();
    let record = StagedRecord::new_with_context(
        context_with(&target, EnvironmentSnapshot::Unavailable),
        String::new(),
        false,
        NOW.into(),
        vec![],
    );
    let resolution = configured_defaults(
        "example.test/widget",
        &template,
        &Default::default(),
        &invalid_mapping(),
    )
    .unwrap();
    let interview = record
        .replay_with_resolution(&template, resolution, &target)
        .unwrap();
    let message = batch_rejection(interview);
    assert!(message.contains("user.yml"), "{message}");
    assert!(message.contains("template-defaults"), "{message}");
}

#[test]
fn ordinary_flat_seed_does_not_claim_configured_origin() {
    let (_dir, template) = load(SELECT);
    let (_t, target) = a_target();
    let mut defaults = indexmap::IndexMap::new();
    defaults.insert(
        Id::parse("mode").unwrap(),
        RawAnswer(serde_json::json!("medium")),
    );
    let interview = Interview::start(
        &template,
        Seed {
            now: NOW.parse().unwrap(),
            defaults,
            context: context_with(&target, EnvironmentSnapshot::Unavailable),
        },
    )
    .unwrap();
    // Answering the empty document applies the flat default, which the select
    // rejects. The attribution is the generic flat form, never a config origin.
    let toha::Interview::Asking(pending) = interview else {
        panic!("expected a batch");
    };
    let message = match pending.answer(Default::default()) {
        Err(toha::AnswerError::Rejected { rejections, .. }) => rejections
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected a rejection, got {other:?}"),
    };
    assert!(
        message.contains("configured default for question"),
        "flat route keeps generic attribution: {message}",
    );
    assert!(!message.contains("user.yml"), "{message}");
    assert!(!message.contains("template-defaults"), "{message}");
}

fn captured_context(target: &toha::staging::CanonicalTarget) -> InvocationContext {
    context_with(
        target,
        EnvironmentSnapshot::Captured(FixedEnvironment::new(
            Some("rec-user".into()),
            Some("rec-host".into()),
            Some("rec-editor".into()),
            Some("rec-shell".into()),
            Some("rec-visual".into()),
        )),
    )
}

// A folder mutated after staging to add a fixed-value reference.
const ADDED_REF: &str = "name: widget\ninterview:\n  - { id: later, computed: toha_env_editor }\n";

fn replay_probe(
    record: &StagedRecord,
    template: &Template,
    target: &toha::staging::CanonicalTarget,
) -> Answer {
    let Interview::Complete(completed) = record.replay(template, target).unwrap() else {
        panic!("expected complete replay");
    };
    completed
        .answers
        .get(&Id::parse("later").unwrap())
        .expect("later answer")
        .clone()
}

#[test]
fn continue_replays_snapshot_without_ambient_access() {
    // A recorded captured snapshot carries through replay; the replay path takes
    // no environment source, so no ambient read can occur.
    let (_dir, template) = load(ADDED_REF);
    let (_t, target) = a_target();
    let record = StagedRecord::new_with_context(
        captured_context(&target),
        String::new(),
        false,
        NOW.into(),
        vec![],
    );
    assert_eq!(
        replay_probe(&record, &template, &target),
        Answer::Value(serde_json::json!("rec-editor")),
    );
}

#[test]
fn mutable_folder_no_flag_added_reference_stays_null() {
    // The record was staged with no grant (Unavailable). A later-added folder
    // reference sees null and triggers no ambient read or access gate.
    let (_dir, template) = load(ADDED_REF);
    let (_t, target) = a_target();
    let record = StagedRecord::new_with_context(
        context_with(&target, EnvironmentSnapshot::Unavailable),
        String::new(),
        false,
        NOW.into(),
        vec![],
    );
    assert_eq!(
        replay_probe(&record, &template, &target),
        Answer::Value(serde_json::Value::Null),
    );
}

#[test]
fn mutable_folder_carried_grant_supplies_later_reference() {
    // The record was staged with an explicit grant (Captured), even without an
    // initial reference. A later-added reference reads the recorded value.
    let (_dir, template) = load(ADDED_REF);
    let (_t, target) = a_target();
    let record = StagedRecord::new_with_context(
        captured_context(&target),
        String::new(),
        false,
        NOW.into(),
        vec![],
    );
    assert_eq!(
        replay_probe(&record, &template, &target),
        Answer::Value(serde_json::json!("rec-editor")),
    );
}

// The reference analyzer composes with the include contract through its walk
// seam. The includes runtime (task 1061) owes the real transitive-include
// proof with actual templates and binary validation; this is only the seam.
#[test]
fn include_closure_seam_is_synthetic_not_a_transitive_proof() {
    // A body that references a fixed value directly is a need; the seam that
    // unions an included body's need is exercised by 1061 with real includes.
    let (_dir, template) = load(WITH_REF);
    let spy = SpySource::default();
    assert!(
        template
            .admit_environment(EnvironmentDecision::RequireStageGrant, &mut &spy)
            .is_err()
    );
}

// A pre-context (legacy) record keeps the prior projection: none of the new
// names are injected, so a computed reading one is null.
#[test]
fn legacy_record_injects_no_new_names() {
    let (_dir, template) =
        load("name: widget\ninterview:\n  - { id: shown, computed: toha_template_name }\n");
    let (_t, target) = a_target();
    let record = StagedRecord::new(
        &target,
        "widget".into(),
        String::new(),
        false,
        NOW.into(),
        vec![],
    );
    let Interview::Complete(completed) = record.replay(&template, &target).unwrap() else {
        panic!("expected complete");
    };
    assert_eq!(
        completed.answers.get(&Id::parse("shown").unwrap()).unwrap(),
        &Answer::Value(serde_json::Value::Null),
    );
}
