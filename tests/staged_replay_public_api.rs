// ---
// relationships:
//   implements: interview-protocol
//   references:
//     - template-format
//     - error-attribution
// ---
//! External-crate proof that the three public staged-replay routes remain
//! callable without a private context constructor, restore a current record's
//! recorded context, and keep a pre-context record's legacy projection. Each
//! route is exercised for a current record and a pre-context legacy record.
//!
//! This file compiles as an external crate: it uses only the public
//! `CanonicalTarget`, `Store`, `StagedRecord`, `Template`, resolution, and
//! context APIs. It never builds the private legacy context mode or captures a
//! live environment on replay.

use toha::context::{
    EnvironmentSnapshot, ExecutionFacts, HostFacts, InvocationContext, SelectedTemplate,
};
use toha::interview::configured_defaults;
use toha::staging::{StagedRecord, canonical_target};
use toha::{Answer, Interview, Template};

/// A template whose only node is a computed that copies the reserved template
/// name. A current context projects the short name into it; a legacy record
/// projects nothing, so the computed is null. No prompts, so replay completes
/// immediately.
const YAML: &str = "name: widget\ninterview:\n  - { id: shown, computed: toha_template_name }\n";

fn template() -> (tempfile::TempDir, Template) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("template")).unwrap();
    std::fs::write(dir.path().join("template.yml"), YAML).unwrap();
    let template = Template::load(dir.path()).unwrap();
    (dir, template)
}

/// A canonical target from the sole factory, for an existing directory.
fn target() -> (tempfile::TempDir, toha::staging::CanonicalTarget) {
    let dir = tempfile::tempdir().unwrap();
    let target = canonical_target(dir.path()).unwrap();
    (dir, target)
}

fn current_context(target: &toha::staging::CanonicalTarget) -> InvocationContext {
    InvocationContext::new(
        target.clone(),
        SelectedTemplate::new(
            "example.test/widget".into(),
            "widget".into(),
            vec!["w".into()],
            Some("example.test/widget".into()),
        ),
        HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
        ExecutionFacts::new(false, false),
        EnvironmentSnapshot::Unavailable,
    )
    .unwrap()
}

fn current_record(target: &toha::staging::CanonicalTarget) -> StagedRecord {
    StagedRecord::new_with_context(
        current_context(target),
        String::new(),
        false,
        "2026-01-02T03:04:05+00:00[UTC]".into(),
        vec![],
    )
}

/// A pre-context record from the retained public constructor.
fn legacy_record(target: &toha::staging::CanonicalTarget) -> StagedRecord {
    StagedRecord::new(
        target,
        "example.test/widget".into(),
        String::new(),
        false,
        "2026-01-02T03:04:05+00:00[UTC]".into(),
        vec![],
    )
}

fn shown(interview: Interview) -> Answer {
    let Interview::Complete(completed) = interview else {
        panic!("expected a completed interview");
    };
    completed
        .answers
        .get(&toha::Id::parse("shown").unwrap())
        .expect("computed answer")
        .clone()
}

#[test]
fn external_crate_replay_restores_current_context_and_legacy_projection() {
    let (_dir, template) = template();
    let (_target_dir, target) = target();

    // Current record: the recorded context projects the short name.
    let current = current_record(&target);
    assert_eq!(
        shown(current.replay(&template, &target).unwrap()),
        Answer::Value(serde_json::json!("widget")),
        "current replay projects the recorded template name",
    );

    // Legacy record: the pre-context projection injects none of the reserved
    // names, so the computed reads an undefined value.
    let legacy = legacy_record(&target);
    assert_eq!(
        shown(legacy.replay(&template, &target).unwrap()),
        Answer::Value(serde_json::Value::Null),
        "legacy replay projects no reserved name",
    );
}

#[test]
fn external_crate_replay_with_defaults_restores_current_context_and_legacy_projection() {
    let (_dir, template) = template();
    let (_target_dir, target) = target();
    let defaults = indexmap::IndexMap::new();

    let current = current_record(&target);
    assert_eq!(
        shown(
            current
                .replay_with_defaults(&template, defaults.clone(), &target)
                .unwrap()
        ),
        Answer::Value(serde_json::json!("widget")),
    );

    let legacy = legacy_record(&target);
    assert_eq!(
        shown(
            legacy
                .replay_with_defaults(&template, defaults, &target)
                .unwrap()
        ),
        Answer::Value(serde_json::Value::Null),
    );
}

#[test]
fn external_crate_replay_with_resolution_preserves_origin_and_legacy_projection() {
    let (_dir, template) = template();
    let (_target_dir, target) = target();

    // The configured route consumes the origin-bearing Resolution. With no
    // configured mapping for this template, it is empty but still origin-typed.
    let resolution = configured_defaults(
        "example.test/widget",
        &template,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(resolution.warnings().is_empty());
    let current = current_record(&target);
    assert_eq!(
        shown(
            current
                .replay_with_resolution(&template, resolution, &target)
                .unwrap()
        ),
        Answer::Value(serde_json::json!("widget")),
    );

    // The same route restores a legacy record's pre-context projection.
    let resolution = configured_defaults(
        "example.test/widget",
        &template,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let legacy = legacy_record(&target);
    assert_eq!(
        shown(
            legacy
                .replay_with_resolution(&template, resolution, &target)
                .unwrap()
        ),
        Answer::Value(serde_json::Value::Null),
    );
}

/// The sole factory is the only way to obtain a `CanonicalTarget`; a raw path
/// cannot enter a consumer. The compile-fail doctests on `CanonicalTarget`
/// prove the negative; this asserts the positive round-trip.
#[test]
fn canonical_target_factory_is_the_only_constructor() {
    let dir = tempfile::tempdir().unwrap();
    let target = canonical_target(dir.path()).unwrap();
    assert_eq!(target.as_path(), dir.path().canonicalize().unwrap());
    // A record loads only against a matching factory-made carrier.
    let (_t, template) = template();
    let record = current_record(&target);
    assert!(record.replay(&template, &target).is_ok());
    let other = tempfile::tempdir().unwrap();
    let mismatched = canonical_target(other.path()).unwrap();
    assert!(
        record.replay(&template, &mismatched).is_err(),
        "a record replays only against its own target carrier",
    );
}
