// ---
// relationships:
//   verifies: applied-record
// ---
use serde_json::{Value, json};
use toha::applied::{
    Application, AppliedRecord, FrozenNow, Origin, PathOwnership, RepoPath, Revision, SCHEMA,
    SCHEMA_ID,
};
fn valid() -> Value {
    serde_json::from_str(include_str!("records/applied-record/valid.json")).unwrap()
}
#[test]
fn positive_and_required_member_negative_fixtures_follow_the_published_schema() {
    let schema: Value = serde_norway::from_str(SCHEMA).unwrap();
    assert_eq!(schema["$id"], SCHEMA_ID);
    let validator = jsonschema::validator_for(&schema).unwrap();
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/records/applied-record");
    let mut negatives = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let accepted = path.file_stem().unwrap() == "valid";
        assert_eq!(validator.is_valid(&value), accepted, "{}", path.display());
        assert_eq!(
            AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_ok(),
            accepted,
            "{}",
            path.display()
        );
        if !accepted {
            negatives += 1;
        }
    }
    assert_eq!(
        negatives, 17,
        "one fixture per required record, application, and ownership member"
    );
}
#[test]
fn serialized_records_are_validated_before_the_writer_can_publish_them() {
    let app = Application::new(
        "forge:example/document".into(),
        Revision::Unversioned,
        RepoPath::root(),
        FrozenNow::parse("2026-01-02T03:04:05+00:00[UTC]").unwrap(),
        Default::default(),
        Default::default(),
        vec![],
        vec![PathOwnership::new(
            toha::TargetPath::parse("file.txt").unwrap(),
            Origin::Toha,
        )],
    );
    let mut record = AppliedRecord::default();
    record.replace(app);
    assert!(
        record.to_json_bytes().is_err(),
        "a whole-file ownership without its blob must never reach disk"
    );
}
#[test]
fn duplicate_pairs_wrong_schema_unknown_members_and_revision_disagreement_are_refused() {
    for mutation in [0, 1, 2, 3, 4] {
        let mut value = valid();
        match mutation {
            0 => {
                let app = value["applications"][0].clone();
                value["applications"].as_array_mut().unwrap().push(app);
            }
            1 => value["$schema"] = json!("https://example.invalid/schemas/other/1"),
            2 => value["applications"][0]["extra"] = json!(true),
            3 => value["applications"][0]["content_digest"] = json!("a".repeat(64)),
            _ => value["applications"][0]["source"] = json!("forge:example/other"),
        }
        assert!(
            AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "mutation {mutation}"
        );
    }
}
#[test]
fn documents_parts_context_and_ownership_roundtrip_in_canonical_json() {
    let bytes = serde_json::to_vec(&valid()).unwrap();
    let first = AppliedRecord::parse(&bytes).unwrap();
    assert_eq!(
        first.applications()[0].to_value(),
        valid()["applications"][0]
    );
    let encoded = first.to_json_bytes().unwrap();
    let second = AppliedRecord::parse(&encoded).unwrap();
    assert_eq!(first.applications(), second.applications());
    assert_eq!(second.to_json_bytes().unwrap(), encoded);
    assert_eq!(
        second.applications()[0].answers["contract"]["names"],
        json!(["example"])
    );
    assert_eq!(second.applications()[0].parts, vec!["zeta", "alpha"]);
    assert!(
        String::from_utf8(encoded)
            .unwrap()
            .starts_with("{\n  \"$schema\":")
    );
}
#[test]
fn source_identity_keeps_the_template_subpath_when_it_removes_a_reference() {
    assert_eq!(
        toha::applied::source_identity("forge:example/catalog@stable#one"),
        "forge:example/catalog#one"
    );
    assert_eq!(
        toha::applied::source_identity("forge:example/catalog@next#two"),
        "forge:example/catalog#two"
    );
    assert_eq!(
        toha::applied::source_identity("forge:example/catalog@name@stable#one"),
        "forge:example/catalog@name#one"
    );
    assert_eq!(
        toha::applied::source_identity("/example/folder@name"),
        "/example/folder@name"
    );
}

#[test]
fn ownership_paths_use_the_existing_canonical_target_path_contract() {
    for path in ["file:label.txt", "nested/note:copy.txt", ".github/config"] {
        let mut value = valid();
        let owned = value["applications"][0]["paths"]["note.txt"].clone();
        value["applications"][0]["paths"] = json!({path:owned});
        assert!(
            AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_ok(),
            "{path}"
        );
    }
    for path in [
        "/absolute",
        "../file",
        "a/../b",
        "a//b",
        "nested/.git/file",
        "nested/C:/file",
        "a\\b",
        "dir/",
    ] {
        let mut value = valid();
        let owned = value["applications"][0]["paths"]["note.txt"].clone();
        value["applications"][0]["paths"] = json!({path:owned});
        assert!(
            AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut value = valid();
    value["applications"][0]["answers"]["Invalid"] = json!("example");
    assert!(AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn debug_output_redacts_recorded_answers_and_environment_values() {
    let mut value = valid();
    value["applications"][0]["answers"]["label"] = json!("private-example");
    value["applications"][0]["context"]["toha_env_user"] = json!("private-example");
    let record = AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!format!("{record:?}").contains("private-example"));
    let app = &record.applications()[0];
    let inputs = toha::applied::CaptureInputs {
        template: app.template().into(),
        revision: app.revision().clone(),
        generated: app.generated().clone(),
        answers: app.answers.clone(),
        context: app.context.clone(),
        parts: app.parts.clone(),
    };
    assert!(!format!("{inputs:?}").contains("private-example"));
    let target = tempfile::tempdir().unwrap();
    let context = toha::context::InvocationContext::for_target(
        toha::staging::canonical_target(target.path()).unwrap(),
    )
    .with_recorded(app.context.clone());
    assert!(!format!("{context:?}").contains("private-example"));
    assert!(
        String::from_utf8(record.to_json_bytes().unwrap())
            .unwrap()
            .contains("private-example")
    );
}

#[test]
fn the_published_schema_rejects_unknown_members_invalid_objects_and_duplicate_selections() {
    let schema: Value = serde_norway::from_str(SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for mutation in 0..12 {
        let mut value = valid();
        match mutation {
            0 => value["extra"] = json!(true),
            1 => value["applications"][0]["paths"]["note.txt"]["extra"] = json!(true),
            2 => value["applications"][0]["paths"]["note.txt"]["mode"] = json!("040000"),
            3 => value["applications"][0]["paths"]["note.txt"]["blob"] = json!("z".repeat(40)),
            4 => value["applications"][0]["commit"] = json!("z".repeat(40)),
            5 => {
                value["applications"][0]["commit"] = Value::Null;
                value["applications"][0]["content_digest"] = json!("z".repeat(64));
            }
            6 => value["applications"][0]["parts"] = json!(["same", "same"]),
            7 => value["applications"][0]["context"]["toha_env_other"] = json!("example"),
            8 => value["applications"][0]["context"]["toha_host_os"] = json!(true),
            9 => {
                value["applications"][0]["paths"]["config.json"]["regions"] =
                    json!(["same", "same"])
            }
            10 => {
                value["applications"][0]["paths"]["config.json"]["values"] =
                    json!(["/same", "/same"])
            }
            _ => value["applications"][0]["target"] = json!("../outside"),
        }
        assert!(!validator.is_valid(&value), "schema mutation {mutation}");
        assert!(
            AppliedRecord::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "reader mutation {mutation}"
        );
    }
}
