// ---
// relationships:
//   implements: interview-protocol
// ---
#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use toha::{AnswerError, Interview, Seed, Template, protocol};

fn seed() -> Seed {
    Seed {
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        defaults: Default::default(),
    }
}

fn inline(yaml: &str) -> (tempfile::TempDir, Template) {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("template")).unwrap();
    fs::write(folder.path().join("template.yml"), yaml).unwrap();
    let template = Template::load(folder.path()).unwrap();
    (folder, template)
}

/// The errors of the first batch answered with `document`, grouped by id.
fn first_batch_errors(template: &Template, document: Value) -> BTreeMap<String, Vec<String>> {
    let Interview::Asking(pending) = Interview::start(template, seed()).unwrap() else {
        panic!("expected questions")
    };
    let raw = protocol::parse_answers(&document.to_string()).unwrap();
    match pending.answer(raw) {
        Ok(_) => BTreeMap::new(),
        Err(AnswerError::Rejected { rejections, .. }) => {
            let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for r in rejections {
                grouped.entry(r.id.to_string()).or_default().push(r.message);
            }
            grouped
        }
        Err(AnswerError::Eval(e)) => panic!("evaluation: {e}"),
    }
}

#[test]
fn each_answer_error_is_a_sentence_that_names_its_constraint() {
    let cases: &[(&str, Value, &str)] = &[
        (
            "{ id: q, type: text, prompt: Q, required: true }",
            json!(""),
            "is required",
        ),
        (
            "{ id: q, type: text, prompt: Q, required: true }",
            Value::Null,
            "is required",
        ),
        (
            "{ id: q, type: text, prompt: Q, validate: { min: 2 } }",
            json!("a"),
            "must be at least 2 characters",
        ),
        (
            "{ id: q, type: text, prompt: Q, validate: { min: 1, max: 3 } }",
            json!("abcd"),
            "must be at most 3 characters",
        ),
        (
            "{ id: q, type: text, prompt: Q, validate: { max: 1 } }",
            json!("ab"),
            "must be at most 1 character",
        ),
        (
            "{ id: q, type: text, prompt: Q, validate: { regex: '^[a-z0-9-]+$' } }",
            json!("BAD NAME"),
            "must match ^[a-z0-9-]+$",
        ),
        (
            "{ id: q, type: select, prompt: Q, options: [none, docker-in-docker, docker-outside-of-docker] }",
            json!("podman"),
            "must be one of: none, docker-in-docker, docker-outside-of-docker",
        ),
        (
            "{ id: q, type: multiselect, prompt: Q, options: [a, b] }",
            json!(["a", "c"]),
            "each item must be one of: a, b",
        ),
        (
            "{ id: q, type: multiselect, prompt: Q, options: [a, b] }",
            json!(["a", "a"]),
            "must not repeat an item",
        ),
        (
            "{ id: q, type: multiselect, prompt: Q, options: [a, b], validate: { min: 1 } }",
            json!([]),
            "must have at least 1 item",
        ),
        (
            "{ id: q, type: multiselect, prompt: Q, options: [a, b], validate: { max: 1 } }",
            json!(["a", "b"]),
            "must have at most 1 item",
        ),
        (
            "{ id: q, type: text, prompt: Q, loop: { min: 2 } }",
            json!(["a"]),
            "must have at least 2 items",
        ),
        (
            "{ id: q, type: text, prompt: Q, loop: { max: 1 } }",
            json!(["a", "b"]),
            "must have at most 1 item",
        ),
        (
            "{ id: q, type: text, prompt: Q, loop: { max: 3 }, validate: { regex: '^[a-z]+$' } }",
            json!(["ok", "NO"]),
            "each item must match ^[a-z]+$",
        ),
        (
            "{ id: q, type: text, prompt: Q, loop: { max: 3 }, validate: { min: 2 } }",
            json!(["ok", "n"]),
            "each item must be at least 2 characters",
        ),
        (
            "{ id: q, type: text, prompt: Q }",
            json!(1),
            "must be a string",
        ),
        (
            "{ id: q, type: confirm, prompt: Q }",
            json!("yes"),
            "must be true or false",
        ),
        (
            "{ id: q, type: text, prompt: Q, loop: { max: 2 } }",
            json!("a"),
            "must be an array of strings",
        ),
    ];
    for (question, value, message) in cases {
        let (_folder, template) = inline(&format!("name: sample\ninterview: [{question}]\n"));
        assert_eq!(
            first_batch_errors(&template, json!({ "q": value })),
            BTreeMap::from([("q".to_string(), vec![message.to_string()])]),
            "{question} answered {value}"
        );
    }
}

#[test]
fn unknown_answer_id_is_not_a_question_in_this_template() {
    let (_folder, template) =
        inline("name: sample\ninterview: [{ id: q, type: text, prompt: Q }]\n");
    assert_eq!(
        first_batch_errors(&template, json!({"q": "a", "other": "x"})),
        BTreeMap::from([(
            "other".to_string(),
            vec!["is not a question in this template".to_string()]
        )])
    );
}

#[test]
fn rejected_continue_reports_the_sentence_in_the_batch() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("docs/examples/basic").canonicalize().unwrap();
    let initial = support::isolated_command(state.path())
        .args([
            "stage",
            support::folder_address(&template).as_str(),
            target.path().to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(initial.status.code(), Some(4));
    let (code, document) = continue_with(state.path(), target.path(), json!({"title": "Sample"}));
    assert_eq!(code, 4, "{document}");
    let (code, document) = continue_with(
        state.path(),
        target.path(),
        json!({"slug": "BAD X", "tags": ["a", "b", "c", "d", "e", "f"]}),
    );
    assert_eq!(code, 4);
    assert_eq!(
        document["errors"],
        json!({"slug": ["must match ^[a-z0-9-]+$"], "tags": ["must have at most 5 items"]})
    );
}

fn continue_with(state: &Path, target: &Path, answers: Value) -> (i32, Value) {
    use std::io::Write;
    let mut child = support::isolated_command(state)
        .args(["continue", target.to_str().unwrap(), "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let document = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| json!({"stderr": String::from_utf8_lossy(&output.stderr)}));
    (output.status.code().unwrap(), document)
}

fn early_template() -> std::path::PathBuf {
    Path::new("tests/fixtures/early-answers/template")
        .canonicalize()
        .unwrap()
}

fn stage(state: &Path, target: &Path, template: &Path) -> Value {
    let output = support::isolated_command(state)
        .args([
            "stage",
            support::folder_address(template).as_str(),
            target.to_str().unwrap(),
            "--async",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn questions(document: &Value) -> Vec<String> {
    let mut ids: Vec<String> = document["schema"]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("no batch: {document}"))
        .keys()
        .cloned()
        .collect();
    ids.sort();
    ids
}

#[test]
fn early_answers_are_applied_when_their_questions_are_reached() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, document) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "enabled": false, "mode": "slow", "code": "abc"}),
    );
    assert_eq!(code, 4, "{document}");
    assert_eq!(questions(&document), ["extras", "flavor", "items", "label"]);
    let (code, document) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "flavor": "slow-plain", "items": ["x"], "extras": ["one"]}),
    );
    assert_eq!(code, 0, "{document}");
    assert_eq!(
        document["answers"],
        json!({"name": "Alpha", "label": "First", "enabled": false, "mode": "slow",
            "code": "abc", "flavor": "slow-plain", "items": ["x"], "extras": ["one"]})
    );
}

#[test]
fn basic_example_early_boolean_and_select_are_not_asked_again() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("docs/examples/basic").canonicalize().unwrap();
    stage(state.path(), target.path(), &template);
    let (code, document) = continue_with(
        state.path(),
        target.path(),
        json!({"title": "Early", "has_summary": false, "status": "final"}),
    );
    assert_eq!(code, 4, "{document}");
    assert_eq!(questions(&document), ["slug", "tags"]);
    let (code, document) = continue_with(
        state.path(),
        target.path(),
        json!({"slug": "early", "tags": []}),
    );
    assert_eq!(code, 0, "{document}");
    assert_eq!(document["answers"]["has_summary"], json!(false));
    assert_eq!(document["answers"]["status"], json!("final"));
}

#[test]
fn headless_apply_does_not_ask_answers_held_from_continue() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "enabled": false, "mode": "slow"}),
    );
    assert_eq!(code, 4);
    let answers = state.path().join("answers.json");
    fs::write(&answers, "{}").unwrap();
    let apply = |answers: &Path| {
        support::isolated_command(state.path())
            .arg("apply")
            .arg(support::folder_address(&early_template()))
            .args([target.path().to_str().unwrap(), "--answers"])
            .arg(answers)
            .output()
            .unwrap()
    };
    let output = apply(&answers);
    assert_eq!(output.status.code(), Some(4));
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        questions(&document),
        ["code", "extras", "flavor", "items", "label"]
    );
    fs::write(
        &answers,
        json!({"label": "First", "code": "abc", "flavor": "slow-rich", "items": ["x"], "extras": []})
            .to_string(),
    )
    .unwrap();
    let output = apply(&answers);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(target.path().join("out.txt")).unwrap(),
        "Alpha First False slow abc slow-rich x \n"
    );
}

#[test]
fn headless_answer_replaces_an_answer_held_from_continue() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "flavor": "slow-plain"}),
    );
    assert_eq!(code, 4);
    let answers = state.path().join("answers.json");
    fs::write(
        &answers,
        json!({"label": "First", "mode": "slow", "flavor": "slow-rich", "items": [], "extras": []})
            .to_string(),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("apply")
        .arg(support::folder_address(&early_template()))
        .args([target.path().to_str().unwrap(), "--answers"])
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(target.path().join("out.txt"))
            .unwrap()
            .contains(" slow-rich "),
    );
}

fn submissions(state: &Path, target: &Path) -> usize {
    let store = toha::staging::Store::new(support::staged_dir(state));
    store
        .load(&toha::staging::canonical_target(target).unwrap())
        .unwrap()
        .unwrap()
        .submissions
        .len()
}

#[test]
fn invalid_early_answer_rejects_the_document_when_submitted() {
    let cases = [
        (
            json!({"mode": "medium"}),
            "mode",
            "must be one of: fast, slow",
        ),
        (json!({"code": "ABC"}), "code", "must match ^[a-z]+$"),
        (
            json!({"enabled": "yes"}),
            "enabled",
            "must be true or false",
        ),
        (json!({"label": 3}), "label", "must be a string"),
        (json!({"label": ""}), "label", "is required"),
        (
            json!({"items": ["a", "b", "c"]}),
            "items",
            "must have at most 2 items",
        ),
        (
            json!({"items": ["a", "B"]}),
            "items",
            "each item must match ^[a-z]+$",
        ),
        (
            json!({"extras": ["one", "two"]}),
            "extras",
            "must have at most 1 item",
        ),
        (
            json!({"extras": ["three"]}),
            "extras",
            "each item must be one of: one, two",
        ),
    ];
    for (early, id, message) in cases {
        let state = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        stage(state.path(), target.path(), &early_template());
        let mut document = early.clone();
        document["name"] = json!("Alpha");
        let (code, result) = continue_with(state.path(), target.path(), document);
        assert_eq!(code, 4, "{early}: {result}");
        assert_eq!(result["errors"], json!({ id: [message] }), "{early}");
        assert_eq!(questions(&result), ["name"], "{early}");
        assert_eq!(submissions(state.path(), target.path()), 0, "{early}");
    }
}

#[test]
fn basic_example_invalid_early_select_is_rejected_with_its_own_document() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("docs/examples/basic").canonicalize().unwrap();
    stage(state.path(), target.path(), &template);
    let (code, _) = continue_with(state.path(), target.path(), json!({"title": "Early"}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"slug": "probe-x", "tags": [], "status": "podman"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({"status": ["must be one of: draft, review, final"]})
    );
    assert_eq!(questions(&result), ["slug", "tags"]);
    assert_eq!(submissions(state.path(), target.path()), 1);
}

#[test]
fn early_answer_checked_when_reached_is_asked_with_an_error_that_names_its_origin() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "flavor": "odd"}),
    );
    assert_eq!(code, 4, "{result}");
    assert!(result.get("errors").is_none(), "{result}");
    assert_eq!(questions(&result), ["code", "enabled", "label", "mode"]);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "mode": "fast"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(questions(&result), ["extras", "flavor", "items"]);
    assert_eq!(
        result["errors"],
        json!({"flavor": ["value recorded earlier is not allowed: must be one of: fast-plain, fast-rich"]})
    );
    assert_eq!(submissions(state.path(), target.path()), 2);
    let (code, result) = continue_with(state.path(), target.path(), json!({"flavor": "odd"}));
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({"flavor": ["must be one of: fast-plain, fast-rich"]})
    );
    let (code, result) = continue_with(state.path(), target.path(), json!({"flavor": "fast-rich"}));
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["answers"]["flavor"], json!("fast-rich"));
}

#[test]
fn headless_apply_stops_at_an_early_answer_that_fails_when_reached() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "flavor": "odd"}),
    );
    assert_eq!(code, 4);
    let answers = state.path().join("answers.json");
    fs::write(
        &answers,
        json!({"label": "First", "mode": "fast", "items": [], "extras": []}).to_string(),
    )
    .unwrap();
    let output = support::isolated_command(state.path())
        .arg("apply")
        .arg(support::folder_address(&early_template()))
        .args([target.path().to_str().unwrap(), "--answers"])
        .arg(&answers)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(questions(&document), ["extras", "flavor", "items"]);
    assert_eq!(
        document["errors"],
        json!({"flavor": ["value recorded earlier is not allowed: must be one of: fast-plain, fast-rich"]})
    );
    assert!(!target.path().join("out.txt").exists());
}
