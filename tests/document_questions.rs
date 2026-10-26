// ---
// relationships:
//   verifies: template-composition
// ---
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Output};
use toha::{Id, RawAnswer, RawAnswers, Template};
const URI: &str = "https://schemas.example.test/catalog/1";
const OTHER: &str = "https://schemas.example.test/catalog/2";
struct Fixture {
    root: tempfile::TempDir,
    template: PathBuf,
    target: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let template = root.path().join("template");
        let target = root.path().join("target");
        fs::create_dir_all(template.join("schemas")).unwrap();
        fs::create_dir_all(template.join("template")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(template.join("template.yml"), format!("name: sample\nschemas: schemas\ninterview:\n  - id: catalog\n    type: document\n    prompt: Catalog file\n    schema: {URI}\n    input: catalog.json\n")).unwrap();
        fs::write(
            template.join("template/result.txt"),
            "{{ catalog.title }}\n",
        )
        .unwrap();
        let schema = json!({"$id": URI, "type":"object", "required":["title"], "properties":{"title":{"type":"string"}}});
        fs::write(template.join("schemas/catalog.json"), schema.to_string()).unwrap();
        Self {
            root,
            template,
            target,
        }
    }
    fn doc(&self, file: &str, title: &str) -> PathBuf {
        let path = self.root.path().join(file);
        fs::write(&path, json!({"$schema": URI, "title":title}).to_string()).unwrap();
        path
    }
    fn answers(&self, value: Value) -> PathBuf {
        let path = self.root.path().join("answers.json");
        fs::write(
            &path,
            json!({"template":support::formal_name(&self.template), "answers":value}).to_string(),
        )
        .unwrap();
        path
    }
    fn apply(&self, args: &[&str]) -> Output {
        support::isolated_command(self.root.path())
            .arg("apply")
            .arg(support::folder_address(&self.template))
            .arg(&self.target)
            .args(args)
            .output()
            .unwrap()
    }
    fn load(&self) -> Template {
        Template::load(&self.template).unwrap()
    }
    fn default(&self, title: &str) {
        fs::write(
            self.target.join("catalog.json"),
            json!({"$schema":URI,"title":title}).to_string(),
        )
        .unwrap();
    }
}
fn assert_success(out: &Output) {
    assert!(
        out.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn error_text(out: &Output) -> String {
    format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
fn selected(template: &Template) -> &Value {
    let toha::QuestionKind::Document { contract } = &template
        .question(&Id::parse("catalog").unwrap())
        .unwrap()
        .kind
    else {
        panic!()
    };
    contract.admitted().unwrap()
}
#[test]
fn default_file_is_read_for_every_apply_and_preview() {
    let f = Fixture::new();
    let answers = f.answers(json!({}));
    f.default("First");
    assert_success(&f.apply(&["--answers", answers.to_str().unwrap()]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "First\n"
    );
    f.default("Second");
    assert_success(&f.apply(&[
        "--answers",
        answers.to_str().unwrap(),
        "--force",
        "--dry-run",
    ]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "First\n"
    );
    assert_success(&f.apply(&["--answers", answers.to_str().unwrap(), "--force"]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Second\n"
    );
}
#[test]
fn input_overrides_answers_document_and_default_file() {
    let f = Fixture::new();
    f.default("Default");
    let answers = f.answers(json!({"catalog":{"$schema":URI,"title":"Answer"}}));
    let input = f.doc("input.json", "Input");
    assert_success(&f.apply(&[
        "--answers",
        answers.to_str().unwrap(),
        "--input",
        input.to_str().unwrap(),
    ]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Input\n"
    );
}
#[test]
fn answers_document_overrides_invalid_default_file() {
    let f = Fixture::new();
    fs::write(f.target.join("catalog.json"), "invalid").unwrap();
    let answers = f.answers(json!({"catalog":{"title":"Answer"}}));
    assert_success(&f.apply(&["--answers", answers.to_str().unwrap()]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Answer\n"
    );
}
#[test]
fn input_without_answers_uses_scripted_route_and_routes_to_every_accepting_question() {
    let f = Fixture::new();
    let config = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(f.template.join("template.yml"),format!("{config}  - id: second\n    type: document\n    prompt: Second file\n    schema: {URI}\n    input: second.json\n")).unwrap();
    fs::write(
        f.template.join("template/result.txt"),
        "{{ catalog.title }} {{ second.title }}\n",
    )
    .unwrap();
    let input = f.doc("input.json", "Input");
    assert_success(&f.apply(&["--input", input.to_str().unwrap()]));
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Input Input\n"
    );
}
#[test]
fn missing_default_stays_unanswered_in_script_and_stage_batches() {
    let f = Fixture::new();
    let answers = f.answers(json!({}));
    let out = f.apply(&["--answers", answers.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(4), "{}", error_text(&out));
    let result = support::first_document(&out.stdout);
    assert_eq!(
        result["schema"]["properties"]["catalog"]["x-toha-type"],
        "document"
    );
    assert_eq!(
        result["schema"]["properties"]["catalog"]["x-toha-schemas"],
        json!([URI])
    );
    assert_eq!(
        result["schema"]["properties"]["catalog"]["x-toha-input"],
        "catalog.json"
    );
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{}", error_text(&out));
}
fn invalid_default(value: &str, diagnostic: &str) {
    let f = Fixture::new();
    let answers = f.answers(json!({}));
    fs::write(f.target.join("catalog.json"), value).unwrap();
    fs::write(f.target.join("keep.txt"), "keep").unwrap();
    let before = fs::read(f.target.join("catalog.json")).unwrap();
    let out = f.apply(&["--answers", answers.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", error_text(&out));
    let text = error_text(&out);
    assert!(
        text.contains("catalog.json") && text.contains(diagnostic),
        "{text}"
    );
    assert!(!f.target.join("result.txt").exists());
    assert_eq!(fs::read(f.target.join("catalog.json")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(f.target.join("keep.txt")).unwrap(),
        "keep"
    );
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", error_text(&out));
    let staged = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert!(
        error_text(&staged).contains("no interview is staged"),
        "{}",
        error_text(&staged)
    );
}
#[test]
fn non_object_default_stops_without_writes() {
    invalid_default("[]", "JSON object");
}
#[test]
fn unaccepted_schema_default_stops_without_writes() {
    invalid_default(
        &json!({"$schema":OTHER,"title":"Item"}).to_string(),
        "unaccepted",
    );
}
#[test]
fn schema_invalid_default_stops_without_writes() {
    invalid_default(&json!({"$schema":URI,"title":2}).to_string(), "string");
}
#[test]
fn unaccepted_input_is_a_usage_error_with_no_writes() {
    let f = Fixture::new();
    let input = f.doc("input.json", "Item");
    fs::write(&input, json!({"$schema":OTHER,"title":"Item"}).to_string()).unwrap();
    let out = f.apply(&["--input", input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(error_text(&out).contains("no document question accepts"));
    assert!(!f.target.join("result.txt").exists());
}
#[test]
fn repeatable_input_routes_multiple_schema_uris() {
    let f = Fixture::new();
    let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(f.template.join("template.yml"),format!("{text}  - id: second\n    type: document\n    prompt: Second\n    schema: {OTHER}\n    input: missing.json\n")).unwrap();
    fs::write(
        f.template.join("schemas/second.json"),
        json!({"$id":OTHER,"type":"object"}).to_string(),
    )
    .unwrap();
    let one = f.doc("one.json", "One");
    let two = f.doc("two.json", "Two");
    fs::write(&two, json!({"$schema":OTHER,"title":"Two"}).to_string()).unwrap();
    let out = f.apply(&[
        "--input",
        one.to_str().unwrap(),
        "--input",
        two.to_str().unwrap(),
    ]);
    assert_success(&out);
}
#[test]
fn default_path_cannot_leave_target_without_repository() {
    let f = Fixture::new();
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace("input: catalog.json", "input: ../outside.json");
    fs::write(f.template.join("template.yml"), text).unwrap();
    let mut template = f.load();
    let error =
        toha::document::prepare(&mut template, &f.target, &[], &RawAnswers::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("inside target repository or target")
    );
}
#[test]
fn default_path_can_reach_repository_parent_from_target_subdirectory() {
    let f = Fixture::new();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&f.target)
            .status()
            .unwrap()
            .success()
    );
    f.default("Parent");
    let child = f.target.join("child");
    fs::create_dir(&child).unwrap();
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace("input: catalog.json", "input: ../catalog.json");
    fs::write(f.template.join("template.yml"), text).unwrap();
    let mut template = f.load();
    toha::document::prepare(&mut template, &child, &[], &RawAnswers::new()).unwrap();
    assert_eq!(selected(&template)["title"], "Parent");
}
#[test]
fn missing_input_is_a_load_error() {
    let f = Fixture::new();
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace("    input: catalog.json\n", "");
    fs::write(f.template.join("template.yml"), text).unwrap();
    assert!(Template::load(&f.template).is_err());
}
#[test]
fn yaml_schema_and_nested_json_reference_validate() {
    let f = Fixture::new();
    fs::remove_file(f.template.join("schemas/catalog.json")).unwrap();
    fs::create_dir(f.template.join("schemas/nested")).unwrap();
    fs::write(f.template.join("schemas/catalog.yaml"),format!("$id: {URI}\ntype: object\nrequired: [title]\nproperties:\n  title:\n    $ref: '{OTHER}'\n")).unwrap();
    fs::write(
        f.template.join("schemas/nested/title.json"),
        json!({"$id":OTHER,"type":"string"}).to_string(),
    )
    .unwrap();
    fs::write(f.template.join("schemas/ignore.txt"), "not a schema").unwrap();
    let input = f.doc("input.json", "Vendored");
    let out = f.apply(&["--input", input.to_str().unwrap()]);
    assert_success(&out);
}
#[test]
fn duplicate_ids_name_both_schema_files() {
    let f = Fixture::new();
    fs::copy(
        f.template.join("schemas/catalog.json"),
        f.template.join("schemas/duplicate.json"),
    )
    .unwrap();
    let error = Template::load(&f.template).unwrap_err().to_string();
    assert!(
        error.contains("duplicate")
            && error.contains("catalog.json")
            && error.contains("duplicate.json"),
        "{error}"
    );
}
#[test]
fn missing_schema_id_names_file() {
    let f = Fixture::new();
    fs::write(f.template.join("schemas/missing.yml"), "type: object").unwrap();
    let error = Template::load(&f.template).unwrap_err().to_string();
    assert!(error.contains("missing.yml") && error.contains("$id"));
}
#[test]
fn malformed_schema_file_names_file() {
    let f = Fixture::new();
    fs::write(f.template.join("schemas/broken.json"), "{").unwrap();
    let error = Template::load(&f.template).unwrap_err().to_string();
    assert!(error.contains("broken.json"));
}
#[test]
fn failed_hosted_fetch_names_uri_and_writes_nothing() {
    let f = Fixture::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("https://{}/schema", listener.local_addr().unwrap());
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace(URI, &uri);
    fs::write(f.template.join("template.yml"), text).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
    });
    let answers = f.answers(json!({}));
    let out = f.apply(&["--answers", answers.to_str().unwrap()]);
    server.join().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", error_text(&out));
    assert!(error_text(&out).contains(&uri));
    assert_eq!(fs::read_dir(&f.target).unwrap().count(), 0);
}
#[test]
fn schema_list_selects_exact_document_uri_and_requires_identity_when_ambiguous() {
    let f = Fixture::new();
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace(
            &format!("schema: {URI}"),
            &format!("schema: [{URI}, {OTHER}]"),
        );
    fs::write(f.template.join("template.yml"), text).unwrap();
    fs::write(f.template.join("schemas/two.json"),json!({"$id":OTHER,"type":"object","required":["code"],"properties":{"code":{"type":"integer"}}}).to_string()).unwrap();
    let mut template = f.load();
    let id = Id::parse("catalog").unwrap();
    let answers = RawAnswers::from([(id.clone(), RawAnswer(json!({"$schema":OTHER,"code":4})))]);
    toha::document::prepare(&mut template, &f.target, &[], &answers).unwrap();
    assert_eq!(selected(&template)["code"], 4);
    let answers = RawAnswers::from([(id, RawAnswer(json!({"title":"Item"})))]);
    assert!(
        toha::document::prepare(&mut template, &f.target, &[], &answers)
            .unwrap_err()
            .to_string()
            .contains("$schema")
    );
}
#[test]
fn stage_input_and_continue_input_persist_whole_document_for_apply() {
    for start_with_input in [true, false] {
        let f = Fixture::new();
        let input = f.doc("input.json", "Staged");
        let mut cmd = support::isolated_command(f.root.path());
        cmd.arg("stage")
            .arg(support::folder_address(&f.template))
            .arg(&f.target)
            .arg("--async");
        if start_with_input {
            cmd.arg("--input").arg(&input);
        }
        let out = cmd.output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(if start_with_input { 0 } else { 4 }),
            "{}",
            error_text(&out)
        );
        if !start_with_input {
            let out = support::isolated_command(f.root.path())
                .arg("continue")
                .arg(&f.target)
                .arg("--input")
                .arg(&input)
                .output()
                .unwrap();
            assert_success(&out);
        }
        let out = support::isolated_command(f.root.path())
            .arg("apply")
            .arg(&f.target)
            .output()
            .unwrap();
        assert_success(&out);
        assert_eq!(
            fs::read_to_string(f.target.join("result.txt")).unwrap(),
            "Staged\n"
        );
    }
}
#[test]
fn document_answer_reaches_completed_raw_and_rendering_without_losing_members() {
    let f = Fixture::new();
    f.default("Recorded");
    let mut template = f.load();
    toha::document::prepare(&mut template, &f.target, &[], &RawAnswers::new()).unwrap();
    let resolution = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let interview = resolution.start(&template, jiff::Zoned::now()).unwrap();
    let toha::Interview::Complete(done) = interview else {
        panic!()
    };
    assert_eq!(
        done.accepted_raw()[&Id::parse("catalog").unwrap()].0,
        json!({"$schema":URI,"title":"Recorded"})
    );
}

#[test]
fn schema_directory_must_be_outside_source_and_inside_template() {
    let f = Fixture::new();
    for directory in ["template", ".", "../"] {
        let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
        let changed = text
            .lines()
            .map(|line| {
                if line.starts_with("schemas:") {
                    format!("schemas: {directory}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(f.template.join("template.yml"), changed).unwrap();
        let error = Template::load(&f.template).unwrap_err().to_string();
        assert!(error.contains("schemas directory must stay"), "{error}");
    }
}
#[test]
fn hosted_non_https_uri_is_refused_without_a_network_read() {
    let f = Fixture::new();
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace(URI, "http://127.0.0.1:1/schema");
    fs::write(f.template.join("template.yml"), text).unwrap();
    let error = Template::load(&f.template).unwrap_err().to_string();
    assert!(
        error.contains("schema http://127.0.0.1:1/schema: schema fetch requires HTTPS"),
        "{error}"
    );
}
#[test]
fn empty_or_absolute_default_paths_are_load_errors() {
    let f = Fixture::new();
    let original = fs::read_to_string(f.template.join("template.yml")).unwrap();
    for input in [
        "\"\"".to_owned(),
        serde_json::to_string(&f.target.join("catalog.json")).unwrap(),
    ] {
        fs::write(
            f.template.join("template.yml"),
            original.replace("input: catalog.json", &format!("input: {input}")),
        )
        .unwrap();
        assert!(Template::load(&f.template).is_err());
    }
}
#[test]
fn staged_apply_preserves_recorded_default_file_answer() {
    let f = Fixture::new();
    f.default("First");
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_success(&out);
    f.default("Second");
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "First\n"
    );
}
#[test]
fn duplicate_routed_inputs_are_refused_before_writes() {
    let f = Fixture::new();
    let first = f.doc("first.json", "First");
    let second = f.doc("second.json", "Second");
    let out = f.apply(&[
        "--input",
        first.to_str().unwrap(),
        "--input",
        second.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(error_text(&out).contains("more than one input"));
    assert!(!f.target.join("result.txt").exists());
}
#[test]
fn external_answers_still_validate_document_schema_in_the_pure_engine() {
    let f = Fixture::new();
    let template = f.load();
    let resolution = toha::interview::configured_defaults(
        "sample",
        &template,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let toha::Interview::Asking(pending) = resolution.start(&template, jiff::Zoned::now()).unwrap()
    else {
        panic!()
    };
    let raw = RawAnswers::from([(
        Id::parse("catalog").unwrap(),
        RawAnswer(json!({"$schema":URI,"title":2})),
    )]);
    assert!(pending.answer(raw).is_err());
}
#[cfg(unix)]
#[test]
fn default_symlink_cannot_leave_target() {
    let f = Fixture::new();
    let outside = f.doc("outside.json", "Outside");
    std::os::unix::fs::symlink(outside, f.target.join("catalog.json")).unwrap();
    let mut template = f.load();
    assert!(
        toha::document::prepare(&mut template, &f.target, &[], &RawAnswers::new())
            .unwrap_err()
            .to_string()
            .contains("inside target repository or target")
    );
}

#[test]
fn document_does_not_replay_a_seeded_default_when_its_file_is_missing() {
    let f = Fixture::new();
    let template = f.load();
    let id = Id::parse("catalog").unwrap();
    let interview = toha::Interview::start(
        &template,
        toha::Seed {
            now: jiff::Zoned::now(),
            defaults: RawAnswers::from([(id, RawAnswer(json!({"$schema":URI,"title":"Seed"})))]),
            context: toha::context::InvocationContext::for_target(
                toha::staging::canonical_target(&f.target).unwrap(),
            ),
        },
    )
    .unwrap();
    let result = toha::protocol::answer_headless(&template, interview, RawAnswers::new()).unwrap();
    assert!(matches!(result, toha::protocol::Headless::Pending { .. }));
}

#[cfg(unix)]
#[test]
fn terminal_input_keeps_other_questions_interactive() {
    use expectrl::{Expect, Session};
    let f = Fixture::new();
    let input = f.doc("input.json", "Selected");
    let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(
        f.template.join("template.yml"),
        format!("{text}  - id: note\n    type: text\n    prompt: Note?\n    required: true\n"),
    )
    .unwrap();
    fs::write(
        f.template.join("template/result.txt"),
        "{{ catalog.title }} {{ note }}\n",
    )
    .unwrap();
    let mut command = support::isolated_command(f.root.path());
    command
        .arg("apply")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--input")
        .arg(&input);
    let mut session = Session::spawn(command).unwrap();
    session.expect("Note?").unwrap();
    session.send_line("Entered").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Selected Entered\n"
    );
}

#[test]
fn continue_input_saves_document_while_other_required_questions_remain() {
    let f = Fixture::new();
    let input = f.doc("input.json", "Selected");
    let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(
        f.template.join("template.yml"),
        format!("{text}  - id: note\n    type: text\n    prompt: Note?\n    required: true\n"),
    )
    .unwrap();
    fs::write(
        f.template.join("template/result.txt"),
        "{{ catalog.title }} {{ note }}\n",
    )
    .unwrap();
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg("--input")
        .arg(input)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "{}", error_text(&out));
    let batch = support::first_document(&out.stdout);
    assert!(batch.get("seed").is_none(), "cli_continue_input_omits_seed");
    assert!(batch["schema"]["properties"].get("catalog").is_none());
    assert!(batch["schema"]["properties"].get("note").is_some());
    let answers = f.answers(json!({"note":"Entered"}));
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg(answers)
        .output()
        .unwrap();
    assert_success(&out);
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Selected Entered\n"
    );
}

#[test]
fn continue_input_preserves_flow_stop_for_resume() {
    let f = Fixture::new();
    let input = f.doc("input.json", "Selected");
    let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(
        f.template.join("template.yml"),
        format!("{text}  - flow: stop\n    when: catalog.title == 'Selected'\n"),
    )
    .unwrap();
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg("--input")
        .arg(input)
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(support::first_document(&out.stdout)["status"], "ended");
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_success(&out);
    assert!(error_text(&out).contains("stopped"), "{}", error_text(&out));
    assert!(!f.target.join("result.txt").exists());
}

#[test]
fn vendored_root_and_reference_make_no_network_connections() {
    let f = Fixture::new();
    // Poll one nonblocking socket while the owned child runs. No duplicated
    // socket or monitor thread can block cleanup on another host.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let root_uri = format!("https://{}/catalog", listener.local_addr().unwrap());
    let reference_uri = format!("{root_uri}/title");
    let mut connections = 0;
    let text = fs::read_to_string(f.template.join("template.yml"))
        .unwrap()
        .replace(URI, &root_uri);
    fs::write(f.template.join("template.yml"), text).unwrap();
    fs::remove_file(f.template.join("schemas/catalog.json")).unwrap();
    fs::write(
        f.template.join("schemas/catalog.yaml"),
        format!(
            "$id: {root_uri}\ntype: object\nproperties:\n  title:\n    $ref: '{reference_uri}'\n"
        ),
    )
    .unwrap();
    fs::write(
        f.template.join("schemas/title.json"),
        json!({"$id":reference_uri,"type":"string"}).to_string(),
    )
    .unwrap();
    let input = f.doc("input.json", "Vendored");
    fs::write(
        &input,
        json!({"$schema":root_uri,"title":"Vendored"}).to_string(),
    )
    .unwrap();
    let mut command = support::isolated_command(f.root.path());
    command
        .arg("apply")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--input")
        .arg(&input);
    let out = bounded_output(command, std::time::Duration::from_secs(15), || {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    connections += 1;
                    drop(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("schema listener: {error}"),
            }
        }
    });
    assert_success(&out);
    assert_eq!(connections, 0, "a vendored schema caused a fetch attempt");
    assert!(
        matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock),
        "a connection remained queued"
    );
}

#[test]
fn inputs_without_a_string_schema_are_usage_errors() {
    for value in [
        json!({"title":"Item"}),
        json!({"$schema":null}),
        json!({"$schema":42}),
    ] {
        let f = Fixture::new();
        let input = f.doc("input.json", "Item");
        fs::write(&input, value.to_string()).unwrap();
        let out = f.apply(&["--input", input.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(2), "{}", error_text(&out));
        assert_eq!(support::first_document(&out.stdout)["kind"], "input");
        assert!(!f.target.join("result.txt").exists());
        let out = support::isolated_command(f.root.path())
            .arg("stage")
            .arg(support::folder_address(&f.template))
            .arg(&f.target)
            .arg("--async")
            .arg("--input")
            .arg(input)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{}", error_text(&out));
    }
}

#[test]
fn continue_unaccepted_input_is_a_usage_error_and_keeps_session_pending() {
    let f = Fixture::new();
    let input = f.doc("input.json", "Item");
    fs::write(&input, json!({"$schema":OTHER}).to_string()).unwrap();
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg("--input")
        .arg(input)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", error_text(&out));
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(!f.target.join("result.txt").exists());
}

#[test]
fn document_required_property_is_a_load_error() {
    for required in ["true", "false"] {
        let f = Fixture::new();
        let text = fs::read_to_string(f.template.join("template.yml")).unwrap();
        fs::write(
            f.template.join("template.yml"),
            text.replace(
                "    type: document",
                &format!("    type: document\n    required: {required}"),
            ),
        )
        .unwrap();
        assert!(
            Template::load(&f.template).is_err(),
            "document required: {required} was accepted"
        );
    }
}

#[test]
fn recorded_staged_answer_survives_later_default_and_input() {
    let f = Fixture::new();
    let answers = f.answers(json!({"catalog":{"$schema":URI,"title":"Recorded"}}));
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg(answers)
        .output()
        .unwrap();
    assert_success(&out);
    f.default("Default");
    let input = f.doc("input.json", "Replacement");
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg("--input")
        .arg(input)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(error_text(&out).contains("already answered"));
    fs::write(f.target.join("catalog.json"), "not JSON").unwrap();
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(!f.target.join("result.txt").exists());
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Recorded\n"
    );
}

#[test]
fn incoming_staged_answer_takes_precedence_over_new_default_file() {
    let f = Fixture::new();
    let answers = f.answers(json!({"catalog":{"$schema":URI,"title":"Submitted"}}));
    let out = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    fs::write(f.target.join("catalog.json"), "not JSON").unwrap();
    let out = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&f.target)
        .arg(answers)
        .output()
        .unwrap();
    assert_success(&out);
    f.default("Default");
    let out = support::isolated_command(f.root.path())
        .arg("apply")
        .arg(&f.target)
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        fs::read_to_string(f.target.join("result.txt")).unwrap(),
        "Submitted\n"
    );
}

#[cfg(unix)]
#[test]
fn person_input_routing_error_exits_two() {
    use expectrl::{Expect, Session, process::unix::WaitStatus};
    let f = Fixture::new();
    let input = f.doc("input.json", "Item");
    fs::write(&input, json!({"$schema":OTHER}).to_string()).unwrap();
    let mut command = support::isolated_command(f.root.path());
    command
        .arg("apply")
        .arg(support::folder_address(&f.template))
        .arg(&f.target)
        .arg("--input")
        .arg(input);
    let mut session = Session::spawn(command).unwrap();
    session.expect("no document question accepts").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        WaitStatus::Exited(_, 2)
    ));
    assert!(!f.target.join("result.txt").exists());
}

#[test]
fn seeded_continue_input_omits_seed_but_stage_and_answered_batch_keep_it() {
    let f = Fixture::new();
    let body = fs::read_to_string(f.template.join("template.yml")).unwrap();
    fs::write(
        f.template.join("template.yml"),
        format!("{body}  - {{id: note, type: text, prompt: Note, required: true}}\n"),
    )
    .unwrap();
    let input = f.doc("input.json", "Selected");
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec!["commit", "-qm", "fixture", "--allow-empty"],
    ] {
        let out = std::process::Command::new("git")
            .current_dir(&f.target)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Example User")
            .env("GIT_AUTHOR_EMAIL", "user@example.invalid")
            .env("GIT_COMMITTER_NAME", "Example User")
            .env("GIT_COMMITTER_EMAIL", "user@example.invalid")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
    let answers = f.answers(json!({"note":"Example"}));
    assert_success(&f.apply(&[
        "--answers",
        answers.to_str().unwrap(),
        "--input",
        input.to_str().unwrap(),
    ]));
    let nested = f.target.join("nested");
    let staged = support::isolated_command(f.root.path())
        .arg("stage")
        .arg(support::folder_address(&f.template))
        .arg(&nested)
        .arg("--like")
        .arg(".")
        .arg("--async")
        .output()
        .unwrap();
    assert_eq!(staged.status.code(), Some(4), "{}", error_text(&staged));
    assert_eq!(
        support::first_document(&staged.stdout)["seed"]["from"],
        ".",
        "seeded_stage_has_seed"
    );
    let continued = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&nested)
        .arg("--input")
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(
        continued.status.code(),
        Some(4),
        "{}",
        error_text(&continued)
    );
    let doc = support::first_document(&continued.stdout);
    assert!(
        doc["schema"]["properties"].get("note").is_some(),
        "seeded_input_keeps_pending_question"
    );
    assert!(
        doc.get("seed").is_none(),
        "cli_seeded_continue_input_omits_seed"
    );
    let bad = f.answers(json!({"note":42}));
    let rejected = support::isolated_command(f.root.path())
        .arg("continue")
        .arg(&nested)
        .arg(bad)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(4), "{}", error_text(&rejected));
    assert_eq!(
        support::first_document(&rejected.stdout)["seed"]["from"],
        ".",
        "seeded_answered_batch_keeps_seed"
    );
}

fn bounded_output(
    mut command: std::process::Command,
    timeout: std::time::Duration,
    mut poll: impl FnMut(),
) -> Output {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // Files avoid pipe backpressure and reader threads during a faulty child.
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    command
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap());
    let mut child = Child(command.spawn().unwrap());
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        poll();
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "schema apply child did not stop before the deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    use std::io::{Read, Seek};
    let read = |mut file: std::fs::File| {
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    };
    Output {
        status,
        stdout: read(stdout),
        stderr: read(stderr),
    }
}

#[test]
fn a_blocked_schema_child_fails_with_a_bounded_diagnostic() {
    const CHILD: &str = "TOHA_TEST_BLOCK_SCHEMA_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // A finite rescue duration also bounds this control if the guard is
        // removed. The real guard kills and reaps the child before this ends.
        std::thread::sleep(std::time::Duration::from_secs(1));
        return;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("a_blocked_schema_child_fails_with_a_bounded_diagnostic")
        .env(CHILD, "1");
    let start = std::time::Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bounded_output(command, std::time::Duration::from_millis(100), || {});
    }));
    let panic = result.expect_err("the blocking fault must reach the deadline diagnostic");
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .expect("deadline diagnostic text");
    assert_eq!(
        message,
        "schema apply child did not stop before the deadline"
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn cloned_listener_mode_probe() {
    use std::{
        net::{TcpListener, TcpStream},
        sync::mpsc,
        time::Duration,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cloned = listener.try_clone().unwrap();
    // Each accept is released with a real connection if it blocks. This probe
    // never leaves a blocked socket thread behind, on either host.
    for (name, socket) in [("original after try_clone", listener), ("clone", cloned)] {
        let addr = socket.local_addr().unwrap();
        let (send, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = socket
                .accept()
                .map(|(stream, _)| {
                    drop(stream);
                    "accepted"
                })
                .unwrap_or_else(|error| {
                    if error.kind() == std::io::ErrorKind::WouldBlock {
                        "WouldBlock"
                    } else {
                        panic!("probe: {error}")
                    }
                });
            send.send(result).unwrap();
        });
        let result = match receive.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1)).unwrap();
                drop(stream);
                receive.recv_timeout(Duration::from_secs(2)).unwrap();
                "blocked until connected"
            }
            Err(error) => panic!("probe: {error}"),
        };
        worker.join().unwrap();
        eprintln!("socket mode probe: {name}: {result}");
    }
}
