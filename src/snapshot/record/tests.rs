use super::*;

/// A valid `snapshot.json` for a snapshot whose `files/` holds exactly the two
/// paths this document names. Tests mutate the returned string to forge.
fn valid_json(id: &str) -> String {
    format!(
        r#"{{
  "snapshot": 1,
  "id": "{id}",
  "template": "forge:catalog/receipt@stable",
  "source": "forge:catalog/receipt",
  "commit": "8be0d41c2f0000000000000000000000000000a1",
  "target": "services/sample-service",
  "created": "2026-09-30T04:12:00Z",
  "generated": "2026-03-14T09:26:53+00:00[UTC]",
  "project": {{ "commit": "a41c0de00000000000000000000000000000beef", "branch": "main" }},
  "built_from": null,
  "submissions": [ {{ "label": "Sample", "quantity": 4 }} ],
  "paths": {{
    "src/app.txt": {{ "origin": "toha" }},
    "config/app.toml": {{ "origin": "edit", "regions": ["features"], "values": [] }}
  }}
}}"#
    )
}

fn files() -> BTreeSet<String> {
    ["src/app.txt".to_owned(), "config/app.toml".to_owned()]
        .into_iter()
        .collect()
}

const SAMPLE_ID: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6E";

#[test]
fn ulid_round_trips_through_display_and_from_str() {
    let id: SnapshotId = SAMPLE_ID.parse().expect("valid ulid");
    assert_eq!(id.to_string(), SAMPLE_ID);
}

#[test]
fn from_parts_orders_by_timestamp() {
    let earlier = SnapshotId::from_parts(1000, [0; 10]);
    let later = SnapshotId::from_parts(2000, [0; 10]);
    assert!(
        later > earlier,
        "a later timestamp sorts after an earlier one"
    );
    // Entropy breaks ties within a millisecond.
    let a = SnapshotId::from_parts(1000, [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let b = SnapshotId::from_parts(1000, [0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    assert!(b > a);
}

#[test]
fn from_str_rejects_wrong_length_and_bad_digits() {
    assert!("TOOSHORT".parse::<SnapshotId>().is_err());
    assert!(format!("{SAMPLE_ID}EXTRA").parse::<SnapshotId>().is_err());
    // `U` is not a Crockford digit.
    let with_u: String = SAMPLE_ID.chars().take(25).chain(['U']).collect();
    assert!(with_u.parse::<SnapshotId>().is_err());
}

#[test]
fn from_str_rejects_overflow_in_the_first_character() {
    // A first character above `7` would need more than 128 bits.
    let overflowing: String = std::iter::once('Z')
        .chain(SAMPLE_ID.chars().skip(1))
        .collect();
    assert!(overflowing.parse::<SnapshotId>().is_err());
}

#[test]
fn parse_prefix_normalises_and_bounds_length() {
    // `O` -> `0`; `I`/`L` -> `1`; case folded; surrounding space trimmed.
    assert_eq!(SnapshotId::parse_prefix("  oilz9a ").unwrap(), "011Z9A");
    assert!(
        SnapshotId::parse_prefix("short").is_err(),
        "five chars is too short"
    );
    assert!(
        SnapshotId::parse_prefix("01jab*").is_err(),
        "star is not a digit"
    );
    assert!(
        SnapshotId::parse_prefix(&format!("{SAMPLE_ID}0")).is_err(),
        "longer than a full id"
    );
}

#[test]
fn has_prefix_matches_the_rendered_id() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let prefix = SnapshotId::parse_prefix(&SAMPLE_ID[..8]).unwrap();
    assert!(id.has_prefix(&prefix));
    let other = SnapshotId::parse_prefix("ZZZZZZ").unwrap();
    assert!(!id.has_prefix(&other));
}

#[test]
fn validate_accepts_a_well_formed_snapshot() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let snapshot =
        Snapshot::validate(id, valid_json(SAMPLE_ID).as_bytes(), &files()).expect("valid snapshot");
    assert_eq!(snapshot.id(), &id);
    assert_eq!(snapshot.template(), "forge:catalog/receipt@stable");
    assert_eq!(snapshot.source(), "forge:catalog/receipt");
    assert_eq!(snapshot.built_from(), None);
    assert!(matches!(snapshot.revision(), Revision::Commit(_)));
    assert_eq!(snapshot.project().branch(), Some("main"));
    assert_eq!(snapshot.paths().len(), 2);
}

// ---- Behavior 30: forgery refusal (document-level cases) ----

#[test]
fn forged_unknown_member_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID).replace(
        "\"snapshot\": 1,",
        "\"snapshot\": 1,\n  \"surprise\": true,",
    );
    assert!(matches!(
        Snapshot::validate(id, json.as_bytes(), &files()),
        Err(SnapshotError::Json(_))
    ));
}

#[test]
fn forged_id_not_equal_to_ref_is_refused() {
    let other: SnapshotId = "01JA2B8M4R0C7W1Y5F3H9K2S6F".parse().unwrap();
    let err = Snapshot::validate(other, valid_json(SAMPLE_ID).as_bytes(), &files()).unwrap_err();
    assert!(matches!(err, SnapshotError::IdRefMismatch { .. }));
}

#[test]
fn forged_path_with_dotdot_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID).replace("src/app.txt", "../escape.txt");
    let err = Snapshot::validate(id, json.as_bytes(), &files()).unwrap_err();
    assert!(matches!(err, SnapshotError::Path { .. }));
}

#[test]
fn paths_not_equal_to_files_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let mut only_one = BTreeSet::new();
    only_one.insert("src/app.txt".to_owned());
    let err = Snapshot::validate(id, valid_json(SAMPLE_ID).as_bytes(), &only_one).unwrap_err();
    assert!(matches!(err, SnapshotError::PathsFilesMismatch));
}

#[test]
fn ownership_on_a_whole_file_origin_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID).replace(
        "\"src/app.txt\": { \"origin\": \"toha\" }",
        "\"src/app.txt\": { \"origin\": \"toha\", \"regions\": [\"x\"] }",
    );
    let err = Snapshot::validate(id, json.as_bytes(), &files()).unwrap_err();
    assert!(matches!(err, SnapshotError::Ownership { .. }));
}

#[test]
fn source_mismatch_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID).replace(
        "\"source\": \"forge:catalog/receipt\",",
        "\"source\": \"forge:catalog/other\",",
    );
    let err = Snapshot::validate(id, json.as_bytes(), &files()).unwrap_err();
    assert!(matches!(err, SnapshotError::Source { .. }));
}

#[test]
fn wrong_format_version_is_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID).replace("\"snapshot\": 1,", "\"snapshot\": 2,");
    let err = Snapshot::validate(id, json.as_bytes(), &files()).unwrap_err();
    assert!(matches!(err, SnapshotError::Format(2)));
}

#[test]
fn bad_commit_and_instant_are_refused() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let bad_commit = valid_json(SAMPLE_ID).replace(
        "\"commit\": \"8be0d41c2f0000000000000000000000000000a1\",",
        "\"commit\": \"nothex\",",
    );
    assert!(matches!(
        Snapshot::validate(id, bad_commit.as_bytes(), &files()),
        Err(SnapshotError::Commit(_))
    ));
    let bad_instant = valid_json(SAMPLE_ID).replace(
        "\"created\": \"2026-09-30T04:12:00Z\",",
        "\"created\": \"not-a-time\",",
    );
    assert!(matches!(
        Snapshot::validate(id, bad_instant.as_bytes(), &files()),
        Err(SnapshotError::Instant(_))
    ));
}

#[test]
fn folder_template_has_unversioned_revision() {
    let id: SnapshotId = SAMPLE_ID.parse().unwrap();
    let json = valid_json(SAMPLE_ID)
        .replace(
            "\"commit\": \"8be0d41c2f0000000000000000000000000000a1\",",
            "\"commit\": null,",
        )
        .replace(
            "\"template\": \"forge:catalog/receipt@stable\",",
            "\"template\": \"forge:catalog/receipt\",",
        );
    let snapshot = Snapshot::validate(id, json.as_bytes(), &files()).unwrap();
    assert_eq!(snapshot.revision(), &Revision::Unversioned);
}
