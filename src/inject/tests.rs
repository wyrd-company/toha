use super::*;
use crate::plan::TargetPath;
use serde_json::json;

fn target(path: &str) -> TargetPath {
    TargetPath::parse(path).unwrap()
}

fn region(key: &str, body: &str, anchor: Option<Anchor>, create: bool) -> PlannedRegionEdit {
    PlannedRegionEdit {
        path: target("config/app.toml"),
        region: RegionKey::parse(key).unwrap(),
        body: body.into(),
        marker: MarkerStyle::line("#"),
        anchor,
        create,
        source: None,
    }
}

fn json_edit(path: &str, value: Value, format: JsonFormat, create: bool) -> PlannedJsonEdit {
    PlannedJsonEdit {
        path: target("package.json"),
        json_path: JsonPath::parse(path).unwrap(),
        desired: value,
        format,
        create,
    }
}

fn written(resolution: EditResolution) -> String {
    match resolution {
        EditResolution::Write(bytes) => String::from_utf8(bytes).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    }
}

// --- region key & path parsing -----------------------------------------

#[test]
fn region_key_accepts_lowercase_digits_underscore_dash() {
    assert!(RegionKey::parse("features-2_x").is_ok());
    for bad in ["", "Features", "a b", "a.b", "café"] {
        assert!(RegionKey::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn json_path_parses_keys_indexes_and_escapes() {
    let path = JsonPath::parse("routes[0].name").unwrap();
    assert_eq!(
        path.segments(),
        &[
            JsonPathSegment::Key("routes".into()),
            JsonPathSegment::Index(0),
            JsonPathSegment::Key("name".into()),
        ]
    );
    let escaped = JsonPath::parse(r"a\.b.c").unwrap();
    assert_eq!(
        escaped.segments(),
        &[
            JsonPathSegment::Key("a.b".into()),
            JsonPathSegment::Key("c".into()),
        ]
    );
    // Round-trips through Display.
    assert_eq!(escaped.to_string(), r"a\.b.c");
    assert_eq!(path.to_string(), "routes[0].name");
}

#[test]
fn json_path_rejects_malformed() {
    for bad in ["", "[0]", "a.", "a..b", "a[x]", "a[0", "a]"] {
        assert!(JsonPath::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn json_path_overlap_detects_ancestor_descendant_and_equal() {
    let a = JsonPath::parse("compilerOptions").unwrap();
    let b = JsonPath::parse("compilerOptions.strict").unwrap();
    let c = JsonPath::parse("scripts.build").unwrap();
    assert!(a.overlaps(&b) && b.overlaps(&a));
    assert!(a.overlaps(&a));
    assert!(!a.overlaps(&c));
    assert!(!c.overlaps(&b));
}

// --- converter ----------------------------------------------------------

#[test]
fn converter_maps_every_json_kind() {
    assert!(matches!(
        json_value_to_cst_input(&Value::Null),
        CstInputValue::Null
    ));
    assert!(matches!(
        json_value_to_cst_input(&json!(true)),
        CstInputValue::Bool(true)
    ));
    match json_value_to_cst_input(&json!(3.5)) {
        CstInputValue::Number(text) => assert_eq!(text, "3.5"),
        other => panic!("{other:?}"),
    }
    match json_value_to_cst_input(&json!("hi")) {
        CstInputValue::String(text) => assert_eq!(text, "hi"),
        other => panic!("{other:?}"),
    }
    match json_value_to_cst_input(&json!([1, "two"])) {
        CstInputValue::Array(items) => assert_eq!(items.len(), 2),
        other => panic!("{other:?}"),
    }
    // Object iteration order is the serde map order (sorted, no preserve_order).
    match json_value_to_cst_input(&json!({"b": 1, "a": 2})) {
        CstInputValue::Object(pairs) => {
            let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, ["a", "b"]);
        }
        other => panic!("{other:?}"),
    }
}

// --- ParseOptions bundles ----------------------------------------------

fn parses(text: &str, format: JsonFormat) -> bool {
    CstRootNode::parse(text, &format.parse_options()).is_ok()
}

#[test]
fn parse_options_bundles_are_constructed_explicitly() {
    // Every field is set explicitly, not inherited from the dependency default.
    let json = JsonFormat::Json.parse_options();
    assert!(
        !json.allow_comments
            && !json.allow_loose_object_property_names
            && !json.allow_trailing_commas
            && !json.allow_missing_commas
            && !json.allow_single_quoted_strings
            && !json.allow_hexadecimal_numbers
            && !json.allow_unary_plus_numbers
            && !json.allow_bare_decimal_point_numbers
            && !json.allow_non_finite_numbers
            && !json.allow_extended_string_escapes
    );
    let jsonc = JsonFormat::Jsonc.parse_options();
    assert!(jsonc.allow_comments && jsonc.allow_trailing_commas);
    assert!(
        !jsonc.allow_loose_object_property_names
            && !jsonc.allow_missing_commas
            && !jsonc.allow_single_quoted_strings
    );
    let json5 = JsonFormat::Json5.parse_options();
    assert!(
        json5.allow_comments
            && json5.allow_loose_object_property_names
            && json5.allow_trailing_commas
            && json5.allow_missing_commas
            && json5.allow_single_quoted_strings
            && json5.allow_hexadecimal_numbers
            && json5.allow_unary_plus_numbers
            && json5.allow_bare_decimal_point_numbers
            && json5.allow_non_finite_numbers
            && json5.allow_extended_string_escapes
    );
}

#[test]
fn parse_options_json_bundle_rejects_relaxations() {
    assert!(parses(r#"{"a": 1}"#, JsonFormat::Json));
    // Trailing commas, single quotes, and loose (unquoted) keys are rejected.
    assert!(!parses(r#"{"a": 1,}"#, JsonFormat::Json));
    assert!(!parses(r#"{'a': 1}"#, JsonFormat::Json));
    assert!(!parses("{a: 1}", JsonFormat::Json));
}

#[test]
fn strict_json_resolver_rejects_a_comment_but_jsonc_and_json5_keep_it() {
    // The concrete syntax tree keeps a comment regardless of `allow_comments`,
    // so the strict-`.json` contract is enforced by a serde_json validation step
    // in the resolver before any mutation.
    let commented = "{\n  // note\n  \"scripts\": {}\n}";
    let strict = json_edit("scripts.build", json!("tsc"), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(Some(commented.as_bytes()), &strict),
        Err(JsonEditError::Parse { .. })
    ));
    // JSONC and JSON5 keep their comment policy and preserve the comment.
    for format in [JsonFormat::Jsonc, JsonFormat::Json5] {
        let edit = json_edit("scripts.build", json!("tsc"), format, false);
        let out = written(resolve_json_edit(Some(commented.as_bytes()), &edit).unwrap());
        assert!(out.contains("// note"), "{format:?}: {out}");
        assert!(out.contains("\"build\": \"tsc\""), "{format:?}: {out}");
    }
}

#[test]
fn parse_options_jsonc_bundle_allows_comments_and_trailing_commas_only() {
    assert!(parses("{\n  // note\n  \"a\": 1,\n}", JsonFormat::Jsonc));
    // Single-quoted strings, loose keys, and missing commas remain rejected.
    assert!(!parses(r#"{'a': 1}"#, JsonFormat::Jsonc));
    assert!(!parses("{a: 1}", JsonFormat::Jsonc));
    assert!(!parses("{\"a\": 1 \"b\": 2}", JsonFormat::Jsonc));
}

#[test]
fn parse_options_json5_bundle_is_permissive() {
    assert!(parses(
        "{\n  // note\n  a: 1,\n  b: 'two',\n  c: 0xFF,\n}",
        JsonFormat::Json5
    ));
    // The JSON5-compatible superset also accepts missing commas.
    assert!(parses("{\n  a: 1\n  b: 2\n}", JsonFormat::Json5));
}

// --- region resolver ----------------------------------------------------

const APP_TOML: &str = "[application]\nname = \"sample-service\"\n";

#[test]
fn region_first_placement_at_anchor() {
    let edit = region(
        "features",
        "analytics_enabled = true\nanalytics_sample_rate = 0.1\n",
        Some(Anchor {
            after: "[application]".into(),
            occurrence: Occurrence::Only,
        }),
        false,
    );
    let out = written(resolve_region_edit(Some(APP_TOML.as_bytes()), &edit).unwrap());
    let sha = checksum("analytics_enabled = true\nanalytics_sample_rate = 0.1\n");
    let expected = format!(
        "[application]\n# >>> toha:region features >>>\nanalytics_enabled = true\nanalytics_sample_rate = 0.1\n# <<< toha:end features sha256:{sha} <<<\nname = \"sample-service\"\n"
    );
    assert_eq!(out, expected);
    // Reporting classifies a first placement as an inject.
    assert_eq!(
        report_region_edit(Some(APP_TOML.as_bytes()), &edit).unwrap(),
        EditReport::Inject
    );
}

#[test]
fn region_first_placement_at_end_of_file_without_anchor() {
    let edit = region("footer", "final\n", None, false);
    let out = written(resolve_region_edit(Some(b"a\nb\n"), &edit).unwrap());
    let sha = checksum("final\n");
    assert_eq!(
        out,
        format!(
            "a\nb\n# >>> toha:region footer >>>\nfinal\n# <<< toha:end footer sha256:{sha} <<<\n"
        )
    );
    // A file without a trailing newline gains one before the region.
    let out = written(resolve_region_edit(Some(b"a\nb"), &edit).unwrap());
    assert!(out.starts_with("a\nb\n# >>> toha:region footer >>>"));
}

#[test]
fn region_repeat_apply_is_unchanged() {
    let edit = region(
        "features",
        "analytics_enabled = true\n",
        Some(Anchor {
            after: "[application]".into(),
            occurrence: Occurrence::Only,
        }),
        false,
    );
    let once = written(resolve_region_edit(Some(APP_TOML.as_bytes()), &edit).unwrap());
    assert_eq!(
        resolve_region_edit(Some(once.as_bytes()), &edit).unwrap(),
        EditResolution::Unchanged
    );
    assert_eq!(
        report_region_edit(Some(once.as_bytes()), &edit).unwrap(),
        EditReport::Unchanged
    );
}

#[test]
fn region_template_change_updates_and_preserves_outside_bytes() {
    let anchor = Anchor {
        after: "[application]".into(),
        occurrence: Occurrence::Only,
    };
    let first = region("features", "old = 1\n", Some(anchor.clone()), false);
    let placed = written(resolve_region_edit(Some(APP_TOML.as_bytes()), &first).unwrap());
    let second = region("features", "new = 2\n", Some(anchor), false);
    let updated = written(resolve_region_edit(Some(placed.as_bytes()), &second).unwrap());
    assert!(updated.contains("new = 2\n"));
    assert!(!updated.contains("old = 1"));
    // Every byte outside the region is preserved.
    assert!(updated.starts_with("[application]\n"));
    assert!(updated.ends_with("name = \"sample-service\"\n"));
    assert_eq!(
        report_region_edit(Some(placed.as_bytes()), &second).unwrap(),
        EditReport::Update
    );
}

#[test]
fn region_drift_refuses_without_force_and_force_replaces_only_the_region() {
    let anchor = Anchor {
        after: "[application]".into(),
        occurrence: Occurrence::Only,
    };
    let edit = region("features", "owned = 1\n", Some(anchor), false);
    let placed = written(resolve_region_edit(Some(APP_TOML.as_bytes()), &edit).unwrap());
    // The operator edits inside the owned span.
    let drifted = placed.replace("owned = 1", "owned = 999");
    let forced = match resolve_region_edit(Some(drifted.as_bytes()), &edit).unwrap() {
        EditResolution::Drift { forced } => String::from_utf8(forced).unwrap(),
        other => panic!("expected Drift, got {other:?}"),
    };
    // Forced replacement restores the owned body and preserves outside bytes.
    assert!(forced.contains("owned = 1\n"));
    assert!(!forced.contains("owned = 999"));
    assert!(forced.ends_with("name = \"sample-service\"\n"));
    assert_eq!(
        report_region_edit(Some(drifted.as_bytes()), &edit).unwrap(),
        EditReport::Drift
    );
}

#[test]
fn region_missing_target_needs_create() {
    let edit = region("features", "x\n", None, false);
    assert!(matches!(
        resolve_region_edit(None, &edit),
        Err(RegionError::TargetMissing(_))
    ));
    let created = region("features", "x\n", None, true);
    let out = written(resolve_region_edit(None, &created).unwrap());
    assert!(out.starts_with("# >>> toha:region features >>>\nx\n"));
}

#[test]
fn region_missing_anchor_and_ambiguous_anchor_are_errors() {
    let missing = region(
        "features",
        "x\n",
        Some(Anchor {
            after: "[nope]".into(),
            occurrence: Occurrence::Only,
        }),
        false,
    );
    assert!(matches!(
        resolve_region_edit(Some(APP_TOML.as_bytes()), &missing),
        Err(RegionError::AnchorMissing(_))
    ));
    let doc = "row\nrow\n";
    let ambiguous = region(
        "features",
        "x\n",
        Some(Anchor {
            after: "row".into(),
            occurrence: Occurrence::Only,
        }),
        false,
    );
    assert!(matches!(
        resolve_region_edit(Some(doc.as_bytes()), &ambiguous),
        Err(RegionError::AnchorAmbiguous(_))
    ));
}

#[test]
fn region_malformed_markers_error() {
    // A begin marker with no end marker is malformed.
    let doc = "# >>> toha:region features >>>\nx\n";
    let edit = region("features", "y\n", None, false);
    assert!(matches!(
        resolve_region_edit(Some(doc.as_bytes()), &edit),
        Err(RegionError::MalformedMarkers(_))
    ));
}

// --- JSON-family resolver ----------------------------------------------

const PACKAGE_JSON: &str =
    "{\n  \"name\": \"sample-app\",\n  \"scripts\": {\n    \"start\": \"node index.js\"\n  }\n}";

#[test]
fn json_insert_and_byte_no_op_on_repeat() {
    let edit = json_edit("scripts.build", json!("tsc"), JsonFormat::Json, false);
    let out = written(resolve_json_edit(Some(PACKAGE_JSON.as_bytes()), &edit).unwrap());
    assert!(out.contains("\"build\": \"tsc\""));
    // Existing keys and text are retained.
    assert!(out.contains("\"start\": \"node index.js\""));
    // Second apply is a byte no-op.
    assert_eq!(
        resolve_json_edit(Some(out.as_bytes()), &edit).unwrap(),
        EditResolution::Unchanged
    );
    assert_eq!(
        report_json_edit(Some(PACKAGE_JSON.as_bytes()), &edit).unwrap(),
        EditReport::Inject
    );
}

#[test]
fn json_replace_converges_owned_value_without_force() {
    let edit = json_edit("scripts.start", json!("deno run"), JsonFormat::Json, false);
    let out = written(resolve_json_edit(Some(PACKAGE_JSON.as_bytes()), &edit).unwrap());
    assert!(out.contains("\"start\": \"deno run\""));
    assert_eq!(
        report_json_edit(Some(PACKAGE_JSON.as_bytes()), &edit).unwrap(),
        EditReport::Update
    );
}

#[test]
fn json_typed_values_retain_type() {
    for (value, needle) in [
        (json!(true), "\"flag\": true"),
        (json!(3), "\"flag\": 3"),
        (json!(null), "\"flag\": null"),
        (json!([1, 2]), "\"flag\": [1, 2]"),
    ] {
        let edit = json_edit("flag", value, JsonFormat::Json, false);
        let out = written(resolve_json_edit(Some(b"{}"), &edit).unwrap());
        assert!(out.contains(needle), "{needle} in {out}");
    }
}

#[test]
fn json_object_value_emits_sorted_keys() {
    // serde_json has no preserve_order, so a new owned object emits sorted keys.
    let edit = json_edit(
        "config",
        json!({"beta": 1, "alpha": 2}),
        JsonFormat::Json,
        false,
    );
    let out = written(resolve_json_edit(Some(b"{}"), &edit).unwrap());
    let alpha = out.find("alpha").unwrap();
    let beta = out.find("beta").unwrap();
    assert!(alpha < beta, "keys not sorted: {out}");
}

#[test]
fn json_missing_object_parents_are_created() {
    let edit = json_edit("a.b.c", json!(1), JsonFormat::Json, false);
    let out = written(resolve_json_edit(Some(b"{}"), &edit).unwrap());
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value, json!({"a": {"b": {"c": 1}}}));
}

#[test]
fn json_scalar_traversal_and_out_of_range_and_missing_array_error() {
    // A scalar where an object is needed.
    let scalar = json_edit("name.deep", json!(1), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(Some(r#"{"name": "x"}"#.as_bytes()), &scalar),
        Err(JsonEditError::Traversal { .. })
    ));
    // An array index out of range.
    let oob = json_edit("list[5]", json!(1), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(Some(r#"{"list": [0]}"#.as_bytes()), &oob),
        Err(JsonEditError::IndexOutOfRange { .. })
    ));
    // A missing array parent is never created.
    let missing_array = json_edit("list[0]", json!(1), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(Some(b"{}"), &missing_array),
        Err(JsonEditError::Traversal { .. })
    ));
}

#[test]
fn json_array_index_terminal_replaces_element() {
    let edit = json_edit("list[1]", json!("z"), JsonFormat::Json, false);
    let out =
        written(resolve_json_edit(Some(r#"{"list": ["a", "b"]}"#.as_bytes()), &edit).unwrap());
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value, json!({"list": ["a", "z"]}));
}

#[test]
fn jsonc_preserves_comments_and_trailing_commas() {
    let source = "{\n  // build tools\n  \"scripts\": {\n    \"start\": \"go\", // run\n  },\n}";
    let edit = json_edit("scripts.build", json!("tsc"), JsonFormat::Jsonc, false);
    let out = written(resolve_json_edit(Some(source.as_bytes()), &edit).unwrap());
    assert!(out.contains("// build tools"));
    assert!(out.contains("// run"));
    assert!(out.contains("\"build\": \"tsc\""));
}

#[test]
fn json5_preserves_unquoted_keys_and_comments() {
    let source = "{\n  // config\n  scripts: {\n    start: 'go',\n  },\n}";
    let edit = json_edit("scripts.build", json!("tsc"), JsonFormat::Json5, false);
    let out = written(resolve_json_edit(Some(source.as_bytes()), &edit).unwrap());
    assert!(out.contains("// config"));
    assert!(out.contains("start: 'go'"));
    assert!(out.contains("\"build\": \"tsc\"") || out.contains("build: \"tsc\""));
}

#[test]
fn json_missing_target_needs_create_and_created_starts_empty() {
    let edit = json_edit("a", json!(1), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(None, &edit),
        Err(JsonEditError::TargetMissing { .. })
    ));
    let created = json_edit("a", json!(1), JsonFormat::Json, true);
    let out = written(resolve_json_edit(None, &created).unwrap());
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value, json!({"a": 1}));
}

#[test]
fn json_malformed_source_errors() {
    let edit = json_edit("a", json!(1), JsonFormat::Json, false);
    assert!(matches!(
        resolve_json_edit(Some(b"{ not json"), &edit),
        Err(JsonEditError::Parse { .. })
    ));
}

// ---- Retraction helpers (capture uses these when a template drops an edit) ----

#[test]
fn retract_region_removes_the_owned_block_and_keeps_surroundings() {
    let marker = MarkerStyle::line("#");
    let edit = PlannedRegionEdit {
        path: TargetPath::parse("f.txt").unwrap(),
        region: RegionKey::parse("blk").unwrap(),
        body: "OWNED\n".to_owned(),
        marker: marker.clone(),
        anchor: None,
        create: true,
        source: None,
    };
    let injected = match resolve_region_edit(Some(b"top\nbottom\n"), &edit).unwrap() {
        EditResolution::Write(bytes) => bytes,
        other => panic!("expected a write, got {other:?}"),
    };
    assert!(
        injected.windows(5).any(|w| w == b"OWNED"),
        "region was injected"
    );

    let retracted = super::retract_region(&injected, &edit.region, &marker).unwrap();
    assert_eq!(
        retracted, b"top\nbottom\n",
        "only the operator's lines remain"
    );

    // Idempotent: retracting again is a no-op.
    let again = super::retract_region(&retracted, &edit.region, &marker).unwrap();
    assert_eq!(again, retracted);
}

#[test]
fn retract_json_value_removes_a_key_and_an_array_element() {
    let path = JsonPath::parse("b.c").unwrap();
    let out = super::retract_json_value(br#"{"a":1,"b":{"c":2,"d":3}}"#, &path, JsonFormat::Json)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"a":1,"b":{"d":3}}),
        "b.c removed, rest kept"
    );

    // Array element removal.
    let idx = JsonPath::parse("list[1]").unwrap();
    let out = super::retract_json_value(br#"{"list":[10,20,30]}"#, &idx, JsonFormat::Json).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value, serde_json::json!({"list":[10,30]}));

    // Absent path is a no-op.
    let absent = JsonPath::parse("missing").unwrap();
    let unchanged = super::retract_json_value(br#"{"a":1}"#, &absent, JsonFormat::Json).unwrap();
    assert_eq!(unchanged, br#"{"a":1}"#);
}

#[test]
fn retract_json_value_refuses_a_relaxed_json_target() {
    // A comment makes this invalid strict JSON; retraction refuses it.
    let path = JsonPath::parse("a").unwrap();
    let result = super::retract_json_value(b"{\n// c\n\"a\":1}", &path, JsonFormat::Json);
    assert!(matches!(result, Err(JsonEditError::Parse { .. })));
}
