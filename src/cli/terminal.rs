// ---
// relationships:
//   implements: architecture
// ---
use indexmap::IndexMap;
use inquire::{Confirm, Editor, MultiSelect, Select, Text};
use serde_json::{Value, json};
use toha::{
    Answer, AnswerError, CheckError, Completed, Interview, Item, Prompt, PromptKind, RawAnswer,
    RawAnswers,
};

pub(crate) trait Ask {
    fn text(&mut self, prompt: &Prompt, title: &str) -> Result<String, String>;
    fn multiline(&mut self, prompt: &Prompt) -> Result<String, String>;
    fn confirm(&mut self, prompt: &Prompt) -> Result<bool, String>;
    fn select(&mut self, prompt: &Prompt) -> Result<String, String>;
    fn multiselect(&mut self, prompt: &Prompt) -> Result<Vec<String>, String>;
}

pub(crate) struct InquireAsk;

impl Ask for InquireAsk {
    fn text(&mut self, prompt: &Prompt, title: &str) -> Result<String, String> {
        let mut input = Text::new(title);
        if prompt.kind != PromptKind::TextLoop {
            if let Some(Answer::Text(default)) = &prompt.default {
                input = input.with_default(default);
            }
        }
        if let Some(placeholder) = &prompt.placeholder {
            input = input.with_placeholder(placeholder);
        }
        if let Some(description) = &prompt.description {
            input = input.with_help_message(description);
        }
        input.prompt().map_err(|e| e.to_string())
    }

    fn multiline(&mut self, prompt: &Prompt) -> Result<String, String> {
        let mut input = Editor::new(&prompt.title);
        if let Some(Answer::Text(default)) = &prompt.default {
            input = input.with_predefined_text(default);
        }
        if let Some(description) = &prompt.description {
            input = input.with_help_message(description);
        }
        // An editor command can be absent on minimal systems. Keep the same
        // terminal available through a line reader in that case.
        if !editor_available() {
            return read_multiline_lines(prompt);
        }
        input.prompt().map_err(|e| e.to_string())
    }

    fn confirm(&mut self, prompt: &Prompt) -> Result<bool, String> {
        let mut input = Confirm::new(&prompt.title);
        if let Some(Answer::Bool(default)) = prompt.default {
            input = input.with_default(default);
        }
        if let Some(description) = &prompt.description {
            input = input.with_help_message(description);
        }
        input.prompt().map_err(|e| e.to_string())
    }

    fn select(&mut self, prompt: &Prompt) -> Result<String, String> {
        let mut input = Select::new(&prompt.title, prompt.options.clone());
        if let Some(Answer::Text(default)) = &prompt.default {
            if let Some(index) = prompt.options.iter().position(|option| option == default) {
                input = input.with_starting_cursor(index);
            }
        }
        if let Some(description) = &prompt.description {
            input = input.with_help_message(description);
        }
        input.prompt().map_err(|e| e.to_string())
    }

    fn multiselect(&mut self, prompt: &Prompt) -> Result<Vec<String>, String> {
        let defaults: Vec<usize> = match &prompt.default {
            Some(Answer::List(values)) => prompt
                .options
                .iter()
                .enumerate()
                .filter_map(|(i, option)| values.contains(option).then_some(i))
                .collect(),
            _ => vec![],
        };
        let mut input =
            MultiSelect::new(&prompt.title, prompt.options.clone()).with_default(&defaults);
        if let Some(description) = &prompt.description {
            input = input.with_help_message(description);
        }
        input.prompt().map_err(|e| e.to_string())
    }
}

fn editor_available() -> bool {
    let editor = std::env::var_os("VISUAL").or_else(|| std::env::var_os("EDITOR"));
    let command = editor.unwrap_or_else(|| if cfg!(windows) { "notepad" } else { "nano" }.into());
    let path = std::path::Path::new(&command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|dir| dir.join(&command).is_file())
}

fn read_multiline_lines(prompt: &Prompt) -> Result<String, String> {
    use std::io::{self, Write};
    print_line(&prompt.title)?;
    if let Some(description) = &prompt.description {
        print_line(description)?;
    }
    print_line("Enter lines; a line containing only . finishes.")?;
    let mut lines = Vec::new();
    loop {
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        if io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("input ended before multiline answer was complete".into());
        }
        if line.trim_end_matches(['\r', '\n']) == "." {
            break;
        }
        lines.push(line);
    }
    Ok(if lines.is_empty() {
        match &prompt.default {
            Some(Answer::Text(v)) => v.clone(),
            _ => String::new(),
        }
    } else {
        lines.concat().trim_end_matches(['\r', '\n']).to_owned()
    })
}

fn print_line(line: &str) -> Result<(), String> {
    use std::io::Write;
    writeln!(std::io::stdout(), "{line}").map_err(|e| e.to_string())
}

fn print_error(line: &str) -> Result<(), String> {
    use std::io::Write;
    writeln!(std::io::stderr(), "{line}").map_err(|e| e.to_string())
}

fn ask_value(ask: &mut impl Ask, prompt: &Prompt) -> Result<Value, String> {
    let value = match prompt.kind {
        PromptKind::Text => json!(ask.text(prompt, &prompt.title)?),
        PromptKind::Multiline => json!(ask.multiline(prompt)?),
        PromptKind::Confirm => json!(ask.confirm(prompt)?),
        PromptKind::Select => json!(ask.select(prompt)?),
        PromptKind::MultiSelect => json!(ask.multiselect(prompt)?),
        PromptKind::TextLoop => {
            let title = if prompt.title.contains("empty to finish") {
                prompt.title.clone()
            } else {
                format!("{} (empty to finish)", prompt.title)
            };
            let mut values = Vec::new();
            loop {
                if prompt
                    .constraints
                    .loop_max
                    .is_some_and(|max| values.len() >= max as usize)
                {
                    break;
                }
                let item = ask.text(prompt, &title)?;
                if item.is_empty() {
                    break;
                }
                values.push(item);
            }
            json!(values)
        }
    };
    if value == "" && prompt.kind != PromptKind::TextLoop {
        if let Some(default) = &prompt.default {
            return Ok(default.to_json());
        }
        if !prompt.constraints.required {
            return Ok(Value::Null);
        }
    }
    Ok(value)
}

pub(crate) fn drive<'a>(
    interview: Interview<'a>,
    ask: &mut impl Ask,
    accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>,
) -> Result<Completed, String> {
    drive_to(interview, ask, accepted, &mut std::io::stdout())
}

fn drive_to<'a>(
    mut interview: Interview<'a>,
    ask: &mut impl Ask,
    mut accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>,
    output: &mut impl std::io::Write,
) -> Result<Completed, String> {
    let mut reached_before = 0;
    loop {
        let pending = match interview {
            Interview::Complete(completed) => {
                for message in completed.messages.iter().skip(reached_before) {
                    writeln!(output, "{message}").map_err(|e| e.to_string())?;
                }
                return Ok(completed);
            }
            Interview::Asking(pending) => pending,
        };
        reached_before = pending.messages_reached();
        let mut submission = RawAnswers::new();
        for item in &pending.batch().items {
            match item {
                Item::Message(message) => {
                    writeln!(output, "{message}").map_err(|e| e.to_string())?;
                }
                Item::Prompt(prompt) => loop {
                    let value = ask_value(ask, prompt)?;
                    match pending.check(&prompt.id, RawAnswer(value.clone())) {
                        Ok(_) => {
                            submission.insert(prompt.id.clone(), RawAnswer(value));
                            break;
                        }
                        Err(CheckError::Rejected(rejection)) => {
                            print_error(&rejection.to_string())?
                        }
                        Err(CheckError::Eval(error)) => return Err(error.to_string()),
                    }
                },
            }
        }
        let raw = submission
            .iter()
            .map(|(id, value)| (id.to_string(), value.0.clone()))
            .collect();
        interview = match pending.answer(submission) {
            Ok(next) => next,
            Err(AnswerError::Rejected { rejections, .. }) => {
                return Err(rejections
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            Err(AnswerError::Eval(error)) => return Err(error.to_string()),
        };
        accepted(raw)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod support {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mod.rs"));
    }
    use std::{
        collections::{HashMap, VecDeque},
        fs,
    };
    use toha::{
        ApplyOptions, Plan, Seed, Template,
        hook::RecordingRunner,
        protocol::{self, Headless},
        staging::StagedRecord,
    };

    struct Script {
        values: serde_json::Map<String, Value>,
        loop_index: HashMap<String, usize>,
        text_responses: VecDeque<String>,
        attempts: usize,
    }
    impl Script {
        fn new(values: Value) -> Self {
            Self {
                values: values.as_object().unwrap().clone(),
                loop_index: HashMap::new(),
                text_responses: VecDeque::new(),
                attempts: 0,
            }
        }
        fn value(&self, prompt: &Prompt) -> Value {
            self.values
                .get(prompt.id.as_str())
                .cloned()
                .or_else(|| prompt.default.as_ref().map(Answer::to_json))
                .unwrap_or(Value::Null)
        }
    }
    impl Ask for Script {
        fn text(&mut self, prompt: &Prompt, _: &str) -> Result<String, String> {
            self.attempts += 1;
            if let Some(response) = self.text_responses.pop_front() {
                return Ok(response);
            }
            let value = self.value(prompt);
            if prompt.kind == PromptKind::TextLoop {
                let index = self.loop_index.entry(prompt.id.to_string()).or_default();
                let item = value
                    .as_array()
                    .and_then(|values| values.get(*index))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                *index += 1;
                Ok(item.into())
            } else {
                Ok(value.as_str().unwrap_or("").into())
            }
        }
        fn multiline(&mut self, prompt: &Prompt) -> Result<String, String> {
            Ok(self.value(prompt).as_str().unwrap_or("").into())
        }
        fn confirm(&mut self, prompt: &Prompt) -> Result<bool, String> {
            Ok(self.value(prompt).as_bool().unwrap_or(false))
        }
        fn select(&mut self, prompt: &Prompt) -> Result<String, String> {
            Ok(self
                .value(prompt)
                .as_str()
                .unwrap_or_else(|| prompt.options.first().map(String::as_str).unwrap_or(""))
                .into())
        }
        fn multiselect(&mut self, prompt: &Prompt) -> Result<Vec<String>, String> {
            Ok(self
                .value(prompt)
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default())
        }
    }

    fn template(yaml: &str) -> (tempfile::TempDir, Template) {
        let folder = tempfile::tempdir().unwrap();
        fs::create_dir(folder.path().join("template")).unwrap();
        fs::write(folder.path().join("template.yml"), yaml).unwrap();
        let template = Template::load(folder.path()).unwrap();
        (folder, template)
    }
    fn start(template: &Template) -> Interview<'_> {
        Interview::start(
            template,
            Seed {
                now: "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap(),
                defaults: Default::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn rejected_text_is_asked_again_and_records_only_accepted_raw_value() {
        let (_folder, template) = template(
            "name: sample\ninterview: [{ id: code, type: text, prompt: Code?, required: true, validate: { min: 2 } }]\n",
        );
        let mut script = Script::new(json!({}));
        script.text_responses = VecDeque::from(["x".into(), "ok".into()]);
        let mut records = Vec::new();
        let completed = drive(start(&template), &mut script, |raw| {
            records.push(raw);
            Ok(())
        })
        .unwrap();
        assert_eq!(script.attempts, 2);
        assert_eq!(
            completed.answers[&toha::Id::parse("code").unwrap()],
            Answer::Text("ok".into())
        );
        assert_eq!(records[0]["code"], json!("ok"));
    }

    #[test]
    fn loop_restarts_after_rejection_and_stops_on_empty_or_max() {
        for (responses, max, wanted_attempts) in [
            (vec!["one", "", "one", "two", ""], 3, 5),
            (vec!["one", "two"], 2, 2),
        ] {
            let (_folder, template) = template(&format!(
                "name: sample\ninterview: [{{ id: items, type: text, prompt: Items?, loop: {{ min: 2, max: {max} }} }}]\n"
            ));
            let mut script = Script::new(json!({}));
            script.text_responses = responses.into_iter().map(str::to_owned).collect();
            let completed = drive(start(&template), &mut script, |_| Ok(())).unwrap();
            assert_eq!(script.attempts, wanted_attempts);
            assert_eq!(
                completed.answers[&toha::Id::parse("items").unwrap()],
                Answer::List(vec!["one".into(), "two".into()])
            );
        }
    }

    #[test]
    fn blank_uses_default_and_optional_blank_is_explicit_null() {
        let (_folder, template) = template(
            "name: sample\ninterview:\n  - { id: first, type: text, prompt: First?, default: fallback }\n  - { id: second, type: text, prompt: Second?, required: false }\n",
        );
        let mut script = Script::new(json!({}));
        script.text_responses = VecDeque::from(["".into(), "".into()]);
        let mut records = Vec::new();
        let completed = drive(start(&template), &mut script, |raw| {
            records.push(raw);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            completed.answers[&toha::Id::parse("first").unwrap()],
            Answer::Text("fallback".into())
        );
        assert_eq!(
            completed.answers[&toha::Id::parse("second").unwrap()],
            Answer::None
        );
        assert_eq!(records[0]["first"], json!("fallback"));
        assert_eq!(records[0]["second"], Value::Null);
    }

    #[test]
    fn evaluation_error_stops_without_recording_batch() {
        let (_folder, template) = template(
            "name: sample\ninterview: [{ id: item, type: text, prompt: Item?, format: '42' }]\n",
        );
        let mut script = Script::new(json!({"item":"test"}));
        let mut records = Vec::new();
        let error = drive(start(&template), &mut script, |raw| {
            records.push(raw);
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("item.format"), "{error}");
        assert!(records.is_empty());
    }

    #[test]
    fn accepted_batch_save_failure_is_an_error() {
        let (_folder, template) =
            template("name: sample\ninterview: [{ id: item, type: text, prompt: Item? }]\n");
        let mut script = Script::new(json!({"item":"test"}));
        let error =
            drive(start(&template), &mut script, |_| Err("save failed".into())).unwrap_err();
        assert_eq!(error, "save failed");
    }

    #[test]
    fn continue_prints_each_reached_message_once() {
        let (_folder, template) = template(
            "name: sample\ninterview:\n  - message: Start\n  - { id: first, type: text, prompt: First? }\n  - message: 'Thanks {{ first }}'\n  - { id: second, type: text, prompt: Second? }\n  - message: 'Done {{ second }}'\n",
        );
        let mut full_output = Vec::new();
        let mut script = Script::new(json!({"first":"Ada", "second":"yes"}));
        drive_to(start(&template), &mut script, |_| Ok(()), &mut full_output).unwrap();

        let saved = StagedRecord {
            target: std::path::PathBuf::from("/tmp/sample-target"),
            template: "/tmp/sample-template".into(),
            commit: String::new(),
            now: "2026-01-02T03:04:05+00:00[UTC]".into(),
            submissions: vec![IndexMap::from([("first".into(), json!("Ada"))])],
        };
        let resumed = saved.replay(&template).unwrap();
        let Interview::Asking(pending) = &resumed else {
            panic!("expected remaining batch")
        };
        assert_eq!(pending.messages_reached(), 2);
        let mut resumed_output = Vec::new();
        let mut script = Script::new(json!({"second":"yes"}));
        drive_to(resumed, &mut script, |_| Ok(()), &mut resumed_output).unwrap();
        assert_eq!(
            String::from_utf8(full_output).unwrap(),
            "Start\nThanks Ada\nDone yes\n"
        );
        assert_eq!(
            String::from_utf8(resumed_output).unwrap(),
            "Thanks Ada\nDone yes\n"
        );
    }

    #[test]
    fn success_fixtures_match_headless_answers_records_and_trees() {
        for fixture in support::fixtures() {
            let expect = support::expectation(&fixture);
            if expect.exit != 0 {
                continue;
            }
            let name = fixture.file_name().unwrap().to_string_lossy();
            let template = Template::load(&fixture.join("template")).unwrap();
            let document: Value =
                serde_json::from_str(&fs::read_to_string(fixture.join("answers.json")).unwrap())
                    .unwrap();
            let seed = Seed {
                now: expect.now.parse().unwrap(),
                defaults: Default::default(),
            };
            let mut script = Script::new(document);
            let mut submissions = Vec::new();
            let completed = drive(
                Interview::start(&template, seed.clone()).unwrap(),
                &mut script,
                |raw| {
                    submissions.push(raw);
                    Ok(())
                },
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut all = RawAnswers::new();
            for submission in &submissions {
                for (key, value) in submission {
                    all.insert(toha::Id::parse(key).unwrap(), RawAnswer(value.clone()));
                }
            }
            let Headless::Completed {
                completed: headless,
                accepted,
            } = protocol::answer_headless(
                &template,
                Interview::start(&template, seed).unwrap(),
                all,
            )
            .unwrap()
            else {
                panic!("{name}: headless pending")
            };
            assert_eq!(completed.answers, headless.answers, "{name}");
            assert_eq!(submissions, accepted, "{name}");
            let staged = |submissions| StagedRecord {
                target: std::path::PathBuf::from("/tmp/sample-target"),
                template: "/tmp/sample-template".into(),
                commit: String::new(),
                now: expect.now.clone(),
                submissions,
            };
            assert_eq!(
                serde_json::to_value(staged(submissions)).unwrap(),
                serde_json::to_value(staged(accepted)).unwrap(),
                "{name}"
            );
            let target = tempfile::tempdir().unwrap();
            support::copy_tree(&fixture.join("existing"), target.path());
            let plan = Plan::build(&template, &completed, target.path()).unwrap();
            if !expect.options.dry_run {
                plan.apply(
                    target.path(),
                    ApplyOptions {
                        force: expect.options.force,
                        trusted: expect.options.trust,
                    },
                    &RecordingRunner::default(),
                )
                .unwrap();
            }
            if fixture.join("expected").exists() {
                support::assert_tree(target.path(), &fixture.join("expected"), &fixture);
            }
        }
    }
}
