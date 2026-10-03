// ---
// relationships:
//   implements: interview-protocol
// ---
#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toha::config::{ConfigEntry, ConfigLayer, ConfigOrigin, DefaultSource, PresetName};
use toha::{AnswerError, Interview, Seed, Template, protocol};

fn ctx() -> toha::context::InvocationContext {
    toha::context::InvocationContext::for_target(
        toha::staging::canonical_target(std::path::Path::new(".")).unwrap(),
    )
}
fn seed() -> Seed {
    Seed {
        now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
        defaults: Default::default(),
        context: ctx(),
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
    let raw = support::raw_answers(&document.to_string()).unwrap();
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

/// The leading JSON value of standard output, or `None` when it does not begin
/// with one (a completing continue writes instructions only; a template fault
/// writes to standard error).
fn leading_document(stdout: &[u8]) -> Option<Value> {
    serde_json::Deserializer::from_slice(stdout)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
}

/// Feeds `answers` to `continue PATH -` inside the identity envelope naming the
/// staged template. Returns the batch document (accepted-with-questions or a
/// rejection) when the interview still asks; on completion the agent route
/// writes instructions only, so the completed answers and last-step messages
/// are recovered by replaying the saved record.
fn continue_with(state: &Path, target: &Path, answers: Value) -> (i32, Value) {
    use std::io::Write;
    let canonical = toha::staging::canonical_target(target).unwrap();
    let store = toha::staging::Store::new(support::staged_dir(state));
    let formal = store
        .load(&canonical)
        .unwrap()
        .expect("a staged record")
        .template;
    let envelope = json!({ "template": formal, "answers": answers });
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
        .write_all(envelope.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let code = output.status.code().unwrap();
    if let Some(document) = leading_document(&output.stdout) {
        return (code, document);
    }
    if code == 0 {
        let record = store.load(&canonical).unwrap().expect("a staged record");
        let template = Template::load(Path::new(&record.template)).unwrap();
        if let Interview::Complete(completed) = record.replay(&template, &canonical).unwrap() {
            return (0, support::completed_projection(&completed));
        }
    }
    (
        code,
        json!({ "stderr": String::from_utf8_lossy(&output.stderr) }),
    )
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
    support::first_document(&output.stdout)
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

fn apply_agent(state: &Path, target: &Path) -> std::process::Output {
    support::isolated_command(state)
        .args(["apply", target.to_str().unwrap()])
        .output()
        .unwrap()
}

#[test]
fn agent_apply_does_not_ask_answers_held_from_continue() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "enabled": false, "mode": "slow"}),
    );
    assert_eq!(code, 4);
    // `apply PATH` on an incomplete staged interview emits the remaining batch
    // and never re-asks the answers held from continue.
    let output = apply_agent(state.path(), target.path());
    assert_eq!(output.status.code(), Some(4));
    let document = support::first_document(&output.stdout);
    assert_eq!(
        questions(&document),
        ["code", "extras", "flavor", "items", "label"]
    );
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "code": "abc", "flavor": "slow-rich", "items": ["x"], "extras": []}),
    );
    assert_eq!(code, 0);
    let output = apply_agent(state.path(), target.path());
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
fn continue_replaces_an_answer_held_from_continue() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "flavor": "slow-plain"}),
    );
    assert_eq!(code, 4);
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "mode": "slow", "flavor": "slow-rich", "items": [], "extras": []}),
    );
    assert_eq!(code, 0);
    let output = apply_agent(state.path(), target.path());
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
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "mode": "fast", "items": [], "extras": []}),
    );
    assert_eq!(code, 4);
    // `apply PATH` replays to the held answer, stops with its origin error, and
    // writes nothing.
    let output = apply_agent(state.path(), target.path());
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document = support::first_document(&output.stdout);
    assert_eq!(questions(&document), ["flavor"]);
    assert_eq!(
        document["errors"],
        json!({"flavor": ["value recorded earlier is not allowed: must be one of: fast-plain, fast-rich"]})
    );
    assert!(!target.path().join("out.txt").exists());
}

#[test]
fn basic_example_empty_tags_batch_completes_with_an_empty_list() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("docs/examples/basic").canonicalize().unwrap();
    stage(state.path(), target.path(), &template);
    let (code, _) = continue_with(state.path(), target.path(), json!({"title": "Sample"}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(state.path(), target.path(), json!({}));
    assert_eq!(code, 4, "{result}");
    assert_eq!(questions(&result), ["has_summary"]);
    assert_eq!(
        result["schema"]["properties"]["has_summary"]["default"],
        json!(false)
    );
    let (code, result) = continue_with(state.path(), target.path(), json!({}));
    assert_eq!(code, 4, "{result}");
    let (code, result) = continue_with(state.path(), target.path(), json!({}));
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["answers"]["tags"], json!([]));
    assert_eq!(result["answers"]["has_summary"], json!(false));
}

#[test]
fn unanswered_list_questions_are_empty_lists_and_others_are_null() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: flag, type: confirm, prompt: F? }\n  - { id: picks, type: multiselect, prompt: P?, options: [a], when: flag }\n  - { id: lines, type: text, prompt: L?, loop: { max: 2 }, when: flag }\n  - { id: more, type: text, prompt: M?, loop: { max: 2 } }\n  - { id: others, type: multiselect, prompt: O?, options: [a] }\n  - { id: word, type: text, prompt: W? }\n  - { id: skipped, type: text, prompt: S?, when: flag }\n",
    );
    let mut interview = Interview::start(&template, seed()).unwrap();
    let mut document = support::raw_answers(r#"{"flag": false, "others": null}"#).unwrap();
    let completed = loop {
        match interview {
            Interview::Complete(completed) => break completed,
            Interview::Ended(_) => panic!("unexpected flow end"),
            Interview::Asking(pending) => {
                interview = pending.answer(std::mem::take(&mut document)).unwrap()
            }
        }
    };
    let answers: serde_json::Map<String, Value> = completed
        .answers
        .iter()
        .map(|(id, answer)| (id.to_string(), answer.to_json()))
        .collect();
    assert_eq!(
        Value::Object(answers),
        json!({"flag": false, "picks": [], "lines": [], "more": [], "others": [],
            "word": null, "skipped": null})
    );
}

#[test]
fn template_fault_names_the_field_and_the_expression() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let template = Path::new("tests/fixtures/err-default-render/template")
        .canonicalize()
        .unwrap();
    stage(state.path(), target.path(), &template);
    let (code, result) = continue_with(state.path(), target.path(), json!({"first": "value"}));
    assert_eq!(code, 1, "{result}");
    let stderr = result["stderr"].as_str().unwrap();
    assert!(
        stderr.contains("template error in second.default `{{ first | nope }}`: "),
        "{stderr}"
    );
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: word, type: text, prompt: W? }\n  - { id: long, type: confirm, prompt: L?, default: 'word | length > 3' }\n",
    );
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!()
    };
    let raw = support::raw_answers(r#"{"word": null}"#).unwrap();
    let Err(AnswerError::Eval(error)) = pending.answer(raw) else {
        panic!("expected a template fault")
    };
    assert!(
        error
            .to_string()
            .starts_with("template error in long.default `word | length > 3`: "),
        "{error}"
    );
}

#[test]
fn placeholder_is_a_schema_example() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, result) = continue_with(state.path(), target.path(), json!({"name": "Alpha"}));
    assert_eq!(code, 4, "{result}");
    let properties = &result["schema"]["properties"];
    assert_eq!(properties["code"]["examples"], json!(["alphacode"]));
    assert!(properties["label"].get("examples").is_none(), "{result}");
}

/// A template whose second question is skipped unless the first is `fancy`.
const SKIPS: &str = "name: skips\ninterview:\n  - { id: kind, type: select, prompt: Kind?, options: [plain, fancy], required: true }\n  - { id: style, type: text, prompt: Style?, when: \"kind == 'fancy'\", format: value | lower }\n  - { id: title, type: text, prompt: Title?, required: true }\n  - { id: extra, type: text, prompt: 'Extra for {{ title }}?', required: true }\n";

/// A template whose `format` appends to the answer.
const SUFFIX: &str = "name: suffix\ninterview:\n  - { id: word, type: text, prompt: Word?, format: \"value ~ 'x'\" }\n  - { id: next, type: text, prompt: 'Next after {{ word }}?', required: true }\n";

/// A template with two skippable questions whose ids sort against interview
/// order, then a message that waits for a later answer.
const ORDERED: &str = "name: ordered\ninterview:\n  - { id: flag, type: confirm, prompt: Flag? }\n  - { id: zeta, type: text, prompt: Zeta?, when: flag }\n  - { id: alpha, type: text, prompt: Alpha?, when: flag }\n  - { id: mid, type: text, prompt: Mid?, required: true }\n  - message: 'Got {{ mid }}'\n  - { id: last, type: text, prompt: Last?, required: true }\n";

#[test]
fn repeated_answer_is_compared_after_format() {
    let (folder, _template) = inline(SUFFIX);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, _) = continue_with(state.path(), target.path(), json!({"word": "a"}));
    assert_eq!(code, 4);
    // "ax" formats to "axx", which differs from the recorded "ax".
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"word": "ax", "next": "n"}),
    );
    assert_eq!(code, 4, "{result}");
    let error = result["errors"]["word"][0].as_str().unwrap_or_default();
    assert!(
        error.starts_with("is already answered with \"ax\";"),
        "{result}"
    );
    assert_eq!(submissions(state.path(), target.path()), 1);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"word": "a", "next": "n"}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["answers"]["word"], json!("ax"));
}

#[test]
fn repeated_list_answer_is_compared_in_order() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: picks, type: multiselect, prompt: P?, options: [a, b] }\n  - { id: next, type: text, prompt: 'N {{ picks }}?' }\n",
    );
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!()
    };
    let raw = support::raw_answers(r#"{"picks": ["a", "b"]}"#).unwrap();
    let Interview::Asking(pending) = pending.answer(raw).unwrap() else {
        panic!()
    };
    let raw = support::raw_answers(r#"{"picks": ["b", "a"]}"#).unwrap();
    let Err(AnswerError::Rejected { rejections, .. }) = pending.answer(raw) else {
        panic!("expected a rejection")
    };
    assert_eq!(
        rejections[0].message,
        r#"is already answered with ["a","b"]"#
    );
}

#[test]
fn format_fault_on_a_repeated_answer_is_a_template_error() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: word, type: text, prompt: W?, format: \"(value ~ 'x') if value == 'a' else (value | dateformat)\" }\n  - { id: next, type: text, prompt: 'N {{ word }}?' }\n",
    );
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!()
    };
    let raw = support::raw_answers(r#"{"word": "a"}"#).unwrap();
    let Interview::Asking(pending) = pending.answer(raw).unwrap() else {
        panic!()
    };
    let raw = support::raw_answers(r#"{"word": "b"}"#).unwrap();
    let Err(AnswerError::Eval(error)) = pending.answer(raw) else {
        panic!("expected a template error")
    };
    assert!(
        error
            .to_string()
            .starts_with("template error in word.format "),
        "{error}"
    );
}

#[test]
fn invalid_answer_for_a_question_this_document_skips_is_a_warning() {
    let (folder, _template) = inline(SKIPS);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "plain", "style": 1}),
    );
    assert_eq!(code, 4, "{result}");
    assert!(result.get("errors").is_none(), "{result}");
    assert_eq!(questions(&result), ["title"]);
    assert_eq!(result["messages"], json!([skipped_warning("style")]));

    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "fancy", "style": 1}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(result["errors"], json!({"style": ["must be a string"]}));
    assert_eq!(questions(&result), ["kind"]);
    assert_eq!(submissions(state.path(), target.path()), 0);
}

#[test]
fn skipped_answer_warnings_follow_interview_order() {
    let (folder, _template) = inline(ORDERED);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, _) = continue_with(state.path(), target.path(), json!({"flag": false}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"alpha": "a", "zeta": "z", "mid": "m"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["messages"],
        json!([skipped_warning("zeta"), skipped_warning("alpha"), "Got m"])
    );
}

#[test]
fn warnings_for_an_interview_complete_at_start_follow_interview_order() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: zeta, type: text, prompt: Z?, when: 'false' }\n  - message: Hello\n  - { id: alpha, type: text, prompt: A?, when: 'false' }\n",
    );
    let document = support::raw_answers(r#"{"alpha": "a", "zeta": "z"}"#).unwrap();
    let Ok(protocol::Headless::Completed { completed, .. }) = protocol::answer_headless(
        &template,
        Interview::start(&template, seed()).unwrap(),
        document,
    ) else {
        panic!("expected a complete interview")
    };
    let expected = [
        skipped_warning("zeta"),
        "Hello".into(),
        skipped_warning("alpha"),
    ];
    assert_eq!(completed.messages, expected);
    assert_eq!(completed.last_messages, expected);
}

fn skipped_warning(id: &str) -> String {
    format!("warning: answer for \"{id}\" was not used: the question was skipped")
}

#[test]
fn repeated_answer_equal_to_the_recorded_answer_is_ignored() {
    let (folder, _template) = inline(SKIPS);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, _) = continue_with(state.path(), target.path(), json!({"kind": "fancy"}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"style": "BOLD", "title": "First"}),
    );
    assert_eq!(code, 4, "{result}");
    // `format` lowercases `style`; the same answer before formatting is equal.
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "fancy", "style": "Bold", "extra": "x"}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["answers"]["style"], json!("bold"));
    assert_eq!(result["messages"], json!([]), "{result}");
}

#[test]
fn repeated_answer_that_differs_rejects_the_document_and_names_the_commands() {
    let (folder, _template) = inline(SKIPS);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, _) = continue_with(state.path(), target.path(), json!({"kind": "plain"}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "fancy", "title": "First"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(questions(&result), ["title"]);
    // Temporary paths need no shell quoting.
    let t = target.path().to_str().unwrap();
    // `stage` names a folder by its drive path, as the product suggests it.
    let f = support::folder_address(&folder.path().canonicalize().unwrap());
    assert_eq!(
        result["errors"],
        json!({"kind": [format!(
            "is already answered with \"plain\"; to change it: toha abort {t}, then toha stage {f} {t} --async"
        )]})
    );
    assert_eq!(submissions(state.path(), target.path()), 1);
}

#[test]
fn held_answer_for_a_skipped_question_is_a_warning_reported_once() {
    let (folder, _template) = inline(SKIPS);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "plain", "style": "Bold"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(questions(&result), ["title"]);
    assert_eq!(result["messages"], json!([skipped_warning("style")]));
    let (code, result) = continue_with(state.path(), target.path(), json!({"title": "First"}));
    assert_eq!(code, 4, "{result}");
    assert_eq!(result["messages"], json!([]), "{result}");
    let (code, result) = continue_with(state.path(), target.path(), json!({"extra": "x"}));
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["messages"], json!([]), "{result}");
    assert_eq!(result["answers"]["style"], Value::Null);
}

#[test]
fn complete_result_carries_the_messages_of_its_last_step() {
    let (folder, _template) = inline(SKIPS);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, _) = continue_with(state.path(), target.path(), json!({"kind": "plain"}));
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"title": "First", "extra": "x", "style": "Bold"}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["messages"], json!([skipped_warning("style")]));
    assert_eq!(result["answers"]["style"], Value::Null);
}

#[test]
fn headless_answer_for_a_question_skipped_at_start_is_a_warning() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: never, type: text, prompt: N?, when: 'false' }\n",
    );
    let document = support::raw_answers(r#"{"never": "x"}"#).unwrap();
    let Ok(protocol::Headless::Completed { completed, .. }) = protocol::answer_headless(
        &template,
        Interview::start(&template, seed()).unwrap(),
        document,
    ) else {
        panic!("expected a complete interview")
    };
    assert_eq!(completed.messages, [skipped_warning("never")]);
    assert_eq!(
        completed.answers[&toha::Id::parse("never").unwrap()],
        toha::Answer::None
    );
}

#[test]
fn rejected_document_keeps_the_error_of_an_answer_recorded_earlier() {
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), &early_template());
    let (code, _) = continue_with(
        state.path(),
        target.path(),
        json!({"name": "Alpha", "flavor": "odd"}),
    );
    assert_eq!(code, 4);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"label": "First", "mode": "fast"}),
    );
    assert_eq!(code, 4, "{result}");
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"flavor": "fast-rich", "items": ["NO"]}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({
            "flavor": ["value recorded earlier is not allowed: must be one of: fast-plain, fast-rich"],
            "items": ["each item must match ^[a-z]+$"],
        })
    );
    assert_eq!(submissions(state.path(), target.path()), 2);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"items": ["ok"], "extras": []}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({"flavor": ["value recorded earlier is not allowed: must be one of: fast-plain, fast-rich"]})
    );
    assert_eq!(questions(&result), ["extras", "flavor", "items"]);
    assert_eq!(submissions(state.path(), target.path()), 2);
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"flavor": "fast-rich", "items": ["ok"], "extras": []}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["answers"]["flavor"], json!("fast-rich"));
}

fn configured(
    values: Value,
) -> indexmap::IndexMap<String, indexmap::IndexMap<toha::Id, ConfigEntry<DefaultSource>>> {
    let values = values
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, value)| {
            (
                toha::Id::parse(id).unwrap(),
                ConfigEntry {
                    value: DefaultSource::Literal(value.clone()),
                    origin: ConfigOrigin {
                        layer: ConfigLayer::User,
                        path: PathBuf::from("user.yml"),
                    },
                },
            )
        })
        .collect();
    [("sample".into(), values)].into()
}

fn sourced<T>(value: T, layer: ConfigLayer, path: &str) -> ConfigEntry<T> {
    ConfigEntry {
        value,
        origin: ConfigOrigin {
            layer,
            path: PathBuf::from(path),
        },
    }
}

#[test]
fn configured_defaults_require_an_explicit_identity_mapping() {
    let (_first_folder, first) = inline(
        "name: first\ninterview:\n  - { id: email, type: text, prompt: Email? }\n  - { id: license, type: text, prompt: License? }\n",
    );
    let (_second_folder, second) =
        inline("name: second\ninterview: [{ id: contact, type: text, prompt: Contact? }]\n");
    let preset_name = PresetName::parse("primary_contact").unwrap();
    let presets = [(
        preset_name.clone(),
        sourced(
            json!("contact@example.invalid"),
            ConfigLayer::User,
            "user.yml",
        ),
    )]
    .into();
    let mappings = [
        (
            "first-template".into(),
            [
                (
                    toha::Id::parse("email").unwrap(),
                    sourced(
                        DefaultSource::Ref(preset_name.clone()),
                        ConfigLayer::User,
                        "user.yml",
                    ),
                ),
                (
                    toha::Id::parse("license").unwrap(),
                    sourced(
                        DefaultSource::Literal(json!("primary_contact")),
                        ConfigLayer::User,
                        "user.yml",
                    ),
                ),
                (
                    toha::Id::parse("removed_question").unwrap(),
                    sourced(
                        DefaultSource::Ref(PresetName::parse("missing_value").unwrap()),
                        ConfigLayer::Local,
                        "local.yml",
                    ),
                ),
            ]
            .into(),
        ),
        (
            "second-template".into(),
            [(
                toha::Id::parse("contact").unwrap(),
                sourced(
                    DefaultSource::Ref(preset_name),
                    ConfigLayer::User,
                    "user.yml",
                ),
            )]
            .into(),
        ),
    ]
    .into();

    let first_resolution =
        toha::interview::configured_defaults("first-template", &first, &presets, &mappings)
            .unwrap();
    let (first_defaults, first_warnings) = first_resolution.into_flat_defaults();
    assert_eq!(
        first_defaults[&toha::Id::parse("email").unwrap()].0,
        json!("contact@example.invalid")
    );
    assert_eq!(
        first_defaults[&toha::Id::parse("license").unwrap()].0,
        json!("primary_contact")
    );
    assert_eq!(
        first_warnings,
        [
            "local.yml: template-defaults.\"first-template\".removed_question: question is not defined by the selected template; ignored"
        ]
    );

    let second_resolution =
        toha::interview::configured_defaults("second-template", &second, &presets, &mappings)
            .unwrap();
    let (second_defaults, _) = second_resolution.into_flat_defaults();
    assert_eq!(
        second_defaults[&toha::Id::parse("contact").unwrap()].0,
        json!("contact@example.invalid")
    );

    let unmapped =
        toha::interview::configured_defaults("first", &first, &presets, &mappings).unwrap();
    assert!(unmapped.warnings().is_empty());
    assert!(unmapped.into_flat_defaults().0.is_empty());
}

#[test]
fn configured_default_errors_name_mapping_and_preset_origins() {
    let (_folder, template) =
        inline("name: sample\ninterview: [{ id: enabled, type: confirm, prompt: Enabled? }]\n");
    let missing_mapping = [(
        "sample".into(),
        [(
            toha::Id::parse("enabled").unwrap(),
            sourced(
                DefaultSource::Ref(PresetName::parse("missing_value").unwrap()),
                ConfigLayer::Local,
                "local.yml",
            ),
        )]
        .into(),
    )]
    .into();
    let error = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &missing_mapping,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "local.yml: template-defaults.\"sample\".enabled: no preset named \"missing_value\""
    );

    let preset_name = PresetName::parse("enabled_value").unwrap();
    let presets = [(
        preset_name.clone(),
        sourced(json!("yes"), ConfigLayer::System, "system.yml"),
    )]
    .into();
    let referenced_mapping = [(
        "sample".into(),
        [(
            toha::Id::parse("enabled").unwrap(),
            sourced(
                DefaultSource::Ref(preset_name),
                ConfigLayer::User,
                "user.yml",
            ),
        )]
        .into(),
    )]
    .into();
    let error =
        toha::interview::configured_defaults("sample", &template, &presets, &referenced_mapping)
            .unwrap_err();
    assert_eq!(
        error.to_string(),
        "user.yml: template-defaults.\"sample\".enabled → system.yml: presets.\"enabled_value\" (\"yes\"): must be true or false"
    );

    let unused_bad_preset = [(
        PresetName::parse("unused_value").unwrap(),
        sourced(json!(42), ConfigLayer::System, "system.yml"),
    )]
    .into();
    let other_identity = [(
        "other-template".into(),
        [(
            toha::Id::parse("enabled").unwrap(),
            sourced(
                DefaultSource::Ref(PresetName::parse("missing_value").unwrap()),
                ConfigLayer::User,
                "user.yml",
            ),
        )]
        .into(),
    )]
    .into();
    let resolution = toha::interview::configured_defaults(
        "sample",
        &template,
        &unused_bad_preset,
        &other_identity,
    )
    .unwrap();
    assert!(resolution.into_flat_defaults().0.is_empty());
}

#[test]
fn configured_default_diagnostics_escape_the_formal_name() {
    let (_folder, template) = inline("name: sample\ninterview: []\n");
    let mappings = [(
        "sample\"quoted".into(),
        [(
            toha::Id::parse("removed_question").unwrap(),
            sourced(
                DefaultSource::Literal(json!("unused")),
                ConfigLayer::User,
                "user.yml",
            ),
        )]
        .into(),
    )]
    .into();
    let resolution = toha::interview::configured_defaults(
        "sample\"quoted",
        &template,
        &Default::default(),
        &mappings,
    )
    .unwrap();
    assert_eq!(
        resolution.warnings(),
        [
            "user.yml: template-defaults.\"sample\\\"quoted\".removed_question: question is not defined by the selected template; ignored"
        ]
    );
}

#[test]
fn invalid_configured_default_names_its_source() {
    let template = Template::load(&early_template()).unwrap();
    let error = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &configured(json!({"enabled": 3})),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "user.yml: template-defaults.\"sample\".enabled: must be true or false"
    );
    let (_folder, template) =
        inline("name: sample\ninterview: [{ id: word, type: text, prompt: W? }]\n");
    let seed = Seed {
        defaults: [(toha::Id::parse("word").unwrap(), toha::RawAnswer(json!(3)))].into(),
        ..seed()
    };
    let error = Interview::start(&template, seed).unwrap_err();
    assert_eq!(
        error.to_string(),
        "configured default for question \"word\": must be a string"
    );
}

#[test]
fn configured_default_that_fails_a_constraint_is_attributed_to_configuration() {
    let template = Template::load(&early_template()).unwrap();
    let resolution = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &configured(json!({"mode": "medium"})),
    )
    .unwrap();
    let Interview::Asking(pending) = resolution
        .start_with_context(&template, seed().now, ctx())
        .unwrap()
    else {
        panic!("expected questions")
    };
    let raw = support::raw_answers(r#"{"name": "Alpha", "label": "First"}"#).unwrap();
    let Interview::Asking(pending) = pending.answer(raw).unwrap() else {
        panic!("expected questions")
    };
    let Err(AnswerError::Rejected {
        pending,
        rejections,
    }) = pending.answer(support::raw_answers("{}").unwrap())
    else {
        panic!("expected a rejection")
    };
    let messages: Vec<_> = rejections.iter().map(ToString::to_string).collect();
    assert_eq!(
        messages,
        [
            "mode: default \"medium\" from user.yml: template-defaults.\"sample\".mode is not allowed: must be one of: fast, slow"
        ]
    );

    let next = pending
        .answer(support::raw_answers(r#"{"mode":"fast"}"#).unwrap())
        .unwrap();
    assert!(matches!(
        next,
        Interview::Asking(_) | Interview::Complete(_)
    ));
}

#[test]
fn flattening_configured_defaults_explicitly_drops_their_source() {
    let template = Template::load(&early_template()).unwrap();
    let resolution = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &configured(json!({"mode": "medium"})),
    )
    .unwrap();
    let (defaults, _) = resolution.into_flat_defaults();
    let Interview::Asking(pending) =
        Interview::start(&template, Seed { defaults, ..seed() }).unwrap()
    else {
        panic!("expected questions")
    };
    let Interview::Asking(pending) = pending
        .answer(support::raw_answers(r#"{"name":"Alpha","label":"First"}"#).unwrap())
        .unwrap()
    else {
        panic!("expected questions")
    };
    let Err(AnswerError::Rejected { rejections, .. }) =
        pending.answer(support::raw_answers("{}").unwrap())
    else {
        panic!("expected a rejection")
    };
    assert_eq!(
        rejections[0].to_string(),
        "mode: default \"medium\" from configured default for question \"mode\" is not allowed: must be one of: fast, slow"
    );
}

#[test]
fn referenced_configured_constraint_error_keeps_mapping_preset_and_value() {
    let (_folder, template) = inline(
        "name: sample\ninterview: [{ id: mode, type: select, prompt: Mode?, options: [fast, slow] }]\n",
    );
    let name = PresetName::parse("display_mode").unwrap();
    let presets = [(
        name.clone(),
        sourced(json!("medium"), ConfigLayer::System, "system.yml"),
    )]
    .into();
    let mappings = [(
        "sample".into(),
        [(
            toha::Id::parse("mode").unwrap(),
            sourced(DefaultSource::Ref(name), ConfigLayer::Local, "local.yml"),
        )]
        .into(),
    )]
    .into();
    let resolution =
        toha::interview::configured_defaults("sample", &template, &presets, &mappings).unwrap();
    let Interview::Asking(pending) = resolution
        .start_with_context(&template, seed().now, ctx())
        .unwrap()
    else {
        panic!("expected prompt")
    };
    assert_eq!(
        pending.batch().errors[0].to_string(),
        "mode: default \"medium\" from local.yml: template-defaults.\"sample\".mode → system.yml: presets.\"display_mode\" (\"medium\") is not allowed: must be one of: fast, slow"
    );
}

#[test]
fn every_template_default_form_fails_constraints_as_a_template_fault() {
    let cases = [
        (
            "{ id: labels, type: multiselect, prompt: Labels?, options: [alpha], default: [unknown] }",
            "template error in labels.default: default [\"unknown\"] is not allowed: each item must be one of: alpha",
        ),
        (
            "{ id: labels, type: multiselect, prompt: Labels?, options: [alpha], default: \"['unknown']\" }",
            "template error in labels.default `['unknown']`: default [\"unknown\"] is not allowed: each item must be one of: alpha",
        ),
        (
            "{ id: label, type: text, prompt: Label?, default: x, validate: { min: 2 } }",
            "template error in label.default `x`: default \"x\" is not allowed: must be at least 2 characters",
        ),
    ];
    for (question, expected) in cases {
        let (_folder, template) = inline(&format!("name: sample\ninterview: [{question}]\n"));
        assert_eq!(
            Interview::start(&template, seed()).unwrap_err().to_string(),
            expected
        );
    }
}

#[test]
fn skipped_template_default_checks_ready_rules_without_rendering_presentation() {
    let (_folder, template) = inline(
        "name: sample\ndata: { skip_all: true }\ninterview:\n  - group: hidden\n    when: not skip_all\n    nodes:\n      - { id: label, type: text, prompt: \"{{ 'bad' | dateformat }}\", default: x, validate: { min: 2 } }\n",
    );
    let error = Interview::start(&template, seed()).unwrap_err().to_string();
    assert_eq!(
        error,
        "template error in label.default `x`: default \"x\" is not allowed: must be at least 2 characters"
    );

    let (_folder, template) = inline(
        "name: sample\ndata: { skip_all: true }\ninterview:\n  - { id: length, type: text, prompt: Length? }\n  - group: hidden\n    when: not skip_all\n    nodes:\n      - { id: label, type: text, prompt: \"{{ 'bad' | dateformat }}\", default: x, validate: { min: \"length | int + 2\" } }\n",
    );
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!("the unavailable dynamic constraint must not block the first batch")
    };
    assert!(pending.batch().items.iter().any(
        |item| matches!(item, toha::Item::Prompt(prompt) if prompt.id.to_string() == "length")
    ));
}

#[test]
fn interview_hook_fault_names_the_field_and_the_expression() {
    let cases = [
        (
            "{ hook: { run: [tool, \"{{ 'bad' | dateformat }}\"] } }",
            "template error in hook.run `{{ 'bad' | dateformat }}`: ",
        ),
        (
            "{ hook: { run: [tool], cwd: \"{{ 'bad' | dateformat }}\" } }",
            "template error in hook.cwd `{{ 'bad' | dateformat }}`: ",
        ),
        (
            "{ hook: { each: '42 as item', run: [tool, '{{ item }}'] } }",
            "template error in hook.each `42`: expected array",
        ),
    ];
    for (node, expected) in cases {
        let (_folder, template) = inline(&format!("name: sample\ninterview: [{node}]\n"));
        let error = Interview::start(&template, seed()).unwrap_err().to_string();
        assert!(error.starts_with(expected), "{node}: {error}");
    }
}

#[test]
fn unresolved_dependency_names_the_node_field_and_expression() {
    let (_folder, mut template) = inline(
        "name: sample\ninterview:\n  - { id: first, type: text, prompt: First? }\n  - { id: second, type: text, prompt: 'After {{ first }}' }\n",
    );
    // A loaded template has no forward reference; reordering the nodes makes
    // `second` wait for an answer that no batch can ask for.
    template.interview.swap(0, 1);
    let error = Interview::start(&template, seed()).unwrap_err().to_string();
    assert_eq!(
        error,
        "template error in second.prompt `After {{ first }}`: first has no answer when this node is reached"
    );
}

#[test]
fn unknown_id_for_an_interview_complete_before_any_submission_is_an_error() {
    // An interview with no question to ask completes at start, so the
    // headless document is never submitted; its ids are checked at completion.
    let (_folder, template) =
        inline("name: sample\ninterview:\n  - { id: fixed, computed: '1' }\n");
    let document = support::raw_answers(r#"{"other": "x"}"#).unwrap();
    let error = match protocol::answer_headless(
        &template,
        Interview::start(&template, seed()).unwrap(),
        document,
    ) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("expected an error"),
    };
    assert_eq!(error, "other.answer: is not a question in this template");
}

/// A template for classifying an early answer that fails a constraint:
/// `style` is skipped for `plain` and active for `fancy`; `deep` is not
/// reached until `title` has an answer; the message faults when `boom`;
/// `last` is never answered, so every run stops with questions.
const CLASSIFY: &str = "name: classify\ninterview:\n  - { id: kind, type: select, prompt: Kind?, options: [plain, fancy], required: true }\n  - { id: boom, type: confirm, prompt: Boom?, default: false }\n  - { id: style, type: text, prompt: Style?, when: \"kind == 'fancy'\", validate: { regex: '^[a-z]+$' } }\n  - message: \"{{ (1 | dateformat) if boom else 'fine' }}\"\n  - { id: title, type: text, prompt: Title?, required: true, validate: { regex: '^[a-z]+$' } }\n  - { id: deep, type: text, prompt: 'Deep {{ title }}?', required: true, validate: { regex: '^[a-z]+$' } }\n  - { id: last, type: text, prompt: 'Last after {{ deep }}?', required: true }\n";

#[test]
fn template_fault_after_a_skipped_invalid_answer_is_the_error() {
    let (folder, _template) = inline(CLASSIFY);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "plain", "boom": true, "style": 1}),
    );
    assert_eq!(code, 1, "{result}");
    let stderr = result["stderr"].as_str().unwrap();
    assert!(
        stderr.contains("template error in message.message "),
        "{stderr}"
    );
    assert!(!stderr.contains("style"), "{stderr}");
    assert_eq!(submissions(state.path(), target.path()), 0);

    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({
            "kind": "plain",
            "boom": false,
            "style": 1,
            "title": "good",
            "deep": "good",
            "last": "done"
        }),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(
        result["messages"],
        json!([
            "warning: answer for \"style\" was not used: the question was skipped",
            "fine"
        ])
    );
    assert_eq!(submissions(state.path(), target.path()), 1);
}

#[test]
fn rejected_probe_leaks_no_message_or_hook_before_the_corrected_document() {
    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: kind, type: select, prompt: Kind?, options: [plain, fancy], required: true }\n  - { id: style, type: text, prompt: Style?, when: \"kind == 'fancy'\", validate: { regex: '^[a-z]+$' } }\n  - message: once\n  - { hook: { run: [tool] } }\n  - { id: title, type: text, prompt: Title?, required: true, validate: { regex: '^[a-z]+$' } }\n  - { id: last, type: text, prompt: Last?, required: true }\n",
    );
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!("expected questions")
    };
    let Err(AnswerError::Rejected {
        pending,
        rejections,
    }) =
        pending.answer(support::raw_answers(r#"{"kind":"plain","style":1,"title":"NO"}"#).unwrap())
    else {
        panic!("expected rejection")
    };
    assert_eq!(
        rejections
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["title: must match ^[a-z]+$"]
    );

    let Interview::Complete(completed) = pending
        .answer(
            support::raw_answers(r#"{"kind":"plain","style":1,"title":"good","last":"done"}"#)
                .unwrap(),
        )
        .unwrap()
    else {
        panic!("expected completion")
    };
    assert_eq!(
        completed.messages,
        [
            "warning: answer for \"style\" was not used: the question was skipped",
            "once"
        ]
    );
    assert_eq!(completed.hooks.len(), 1);
}

#[test]
fn rejected_probe_does_not_publish_a_template_fault() {
    let (_folder, template) = inline(CLASSIFY);
    let Interview::Asking(pending) = Interview::start(&template, seed()).unwrap() else {
        panic!("expected questions")
    };
    let error = pending
        .answer(
            support::raw_answers(r#"{"kind":"fancy","boom":true,"style":1,"title":"good"}"#)
                .unwrap(),
        )
        .unwrap_err();
    let AnswerError::Rejected { rejections, .. } = error else {
        panic!("the document rejection must suppress the speculative template fault: {error:?}")
    };
    assert_eq!(
        rejections
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["style: must be a string"]
    );
}

#[test]
fn skipped_invalid_answer_is_dropped_beside_another_failing_answer() {
    let (folder, _template) = inline(CLASSIFY);
    let state = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    stage(state.path(), target.path(), folder.path());
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "plain", "style": 1, "title": "NO"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(result["errors"], json!({"title": ["must match ^[a-z]+$"]}));
    assert_eq!(result["messages"], json!([]), "{result}");
    // The other failure is not an early answer: an unknown id.
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "plain", "style": 1, "nope": "x"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({"nope": ["is not a question in this template"]})
    );
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "fancy", "style": 1, "title": "NO"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({"style": ["must be a string"], "title": ["must match ^[a-z]+$"]})
    );
    let (code, result) = continue_with(
        state.path(),
        target.path(),
        json!({"kind": "unknown", "style": 1, "title": "NO"}),
    );
    assert_eq!(code, 4, "{result}");
    assert_eq!(
        result["errors"],
        json!({
            "kind": ["must be one of: plain, fancy"],
            "style": ["must be a string"],
            "title": ["must match ^[a-z]+$"]
        })
    );
    assert_eq!(submissions(state.path(), target.path()), 0);
}

#[test]
fn a_snapshot_seeded_id_is_prepared_exactly_once_with_a_single_occupant() {
    // Behavior 11 (duplicate preparation / single occupancy): for an id that has
    // BOTH a configured default and a snapshot-seeded value, the engine prepares
    // the id EXACTLY ONCE — one prompt in the batch (an ordered Vec where a
    // duplicate IS representable), carrying the single surviving occupant (the
    // snapshot's value). This observes occupancy at the preparation boundary, not
    // which value wins in a keyed JSON property.
    //
    // Sole-kill: src/interview.rs the prompt-push site, when the default source is
    // Snapshot, pushes the prompt twice (duplicate preparation); the batch then
    // holds two `style` prompts and the exactly-once assertion fails.
    use indexmap::IndexMap;
    use toha::interview::{SnapshotSeed, configured_defaults};
    use toha::{Id, Interview, Item, RawAnswer};

    let (_folder, template) = inline(
        "name: sample\ninterview:\n  - { id: name, type: text, prompt: N?, required: true }\n  - { id: style, type: select, prompt: S?, options: [card, panel], default: card }\n",
    );
    // A configured default for `style` (card) AND a snapshot seed (panel).
    let mappings = configured(json!({ "style": "card" }));
    let presets: IndexMap<PresetName, ConfigEntry<Value>> = IndexMap::new();
    let resolution = configured_defaults("sample", &template, &presets, &mappings).unwrap();
    let now = seed().now;
    let mut defaults: IndexMap<Id, RawAnswer> = IndexMap::new();
    defaults.insert(Id::parse("style").unwrap(), RawAnswer(json!("panel")));
    let snap_seed = SnapshotSeed {
        defaults,
        from: "01J9Z4K7QX6M2V8R0T5B3N1P9D".to_owned(),
    };
    let Interview::Asking(pending) = resolution
        .start_with_seed(&template, now, ctx(), Some(snap_seed))
        .unwrap()
        .0
    else {
        panic!("expected questions");
    };
    // Exactly one prepared prompt for `style` — not two (no duplicate occupancy).
    let style_prompts = pending
        .batch()
        .items
        .iter()
        .filter(|item| matches!(item, Item::Prompt(p) if p.id == Id::parse("style").unwrap()))
        .count();
    assert_eq!(
        style_prompts, 1,
        "the seeded+configured id is prepared exactly once (single occupant)"
    );
    // That single occupant is the snapshot's value (panel), not the configured card.
    let style_default = pending
        .batch()
        .items
        .iter()
        .find_map(|item| match item {
            Item::Prompt(p) if p.id == Id::parse("style").unwrap() => p.default.as_ref(),
            _ => None,
        })
        .expect("style has a prepared default");
    assert_eq!(
        style_default.to_json(),
        json!("panel"),
        "the single occupant is the snapshot value"
    );
}
