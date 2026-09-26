// ---
// relationships:
//   implements: interview-protocol
// ---
use crate::{
    AnswerError, Answers, Batch, Completed, EvalError, Id, Interview, Item, Pending, Prompt,
    PromptKind, RawAnswer, RawAnswers, Rejections, Template,
};
use indexmap::IndexMap;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct Context {
    pub target: String,
    pub template: String,
    pub commit: String,
}
fn context_value(context: &Context) -> Value {
    json!({"target": context.target, "template": context.template, "commit": context.commit})
}
fn text_rules(prompt: &Prompt) -> Map<String, Value> {
    let mut rules = Map::new();
    if let Some(v) = prompt.constraints.min {
        rules.insert("minLength".into(), json!(v));
    }
    if let Some(v) = prompt.constraints.max {
        rules.insert("maxLength".into(), json!(v));
    }
    if let Some(v) = &prompt.constraints.regex {
        rules.insert("pattern".into(), json!(v));
    }
    rules
}
fn prompt_schema(prompt: &Prompt) -> Value {
    let mut property = Map::new();
    property.insert("title".into(), json!(prompt.title));
    if let Some(v) = &prompt.description {
        property.insert("description".into(), json!(v));
    }
    if let Some(v) = &prompt.default {
        property.insert("default".into(), v.to_json());
    }
    let kind = match prompt.kind {
        PromptKind::Text | PromptKind::TextLoop => "text",
        PromptKind::Multiline => "multiline",
        PromptKind::Confirm => "confirm",
        PromptKind::Select => "select",
        PromptKind::MultiSelect => "multiselect",
    };
    property.insert("x-toha-type".into(), json!(kind));
    match prompt.kind {
        PromptKind::Text | PromptKind::Multiline => {
            property.insert("type".into(), json!("string"));
            property.extend(text_rules(prompt));
            if prompt.kind == PromptKind::Multiline {
                property.insert("x-toha-multiline".into(), json!(true));
            }
        }
        PromptKind::TextLoop => {
            property.insert("type".into(), json!("array"));
            let mut items = text_rules(prompt);
            items.insert("type".into(), json!("string"));
            property.insert("items".into(), Value::Object(items));
            if let Some(v) = prompt.constraints.loop_min {
                property.insert("minItems".into(), json!(v));
            }
            if let Some(v) = prompt.constraints.loop_max {
                property.insert("maxItems".into(), json!(v));
            }
        }
        PromptKind::Confirm => {
            property.insert("type".into(), json!("boolean"));
        }
        PromptKind::Select => {
            property.insert("type".into(), json!("string"));
            property.insert("enum".into(), json!(prompt.options));
        }
        PromptKind::MultiSelect => {
            property.insert("type".into(), json!("array"));
            property.insert(
                "items".into(),
                json!({"type":"string", "enum":prompt.options}),
            );
            property.insert("uniqueItems".into(), json!(true));
            if let Some(v) = prompt.constraints.min {
                property.insert("minItems".into(), json!(v));
            }
            if let Some(v) = prompt.constraints.max {
                property.insert("maxItems".into(), json!(v));
            }
        }
    }
    if !prompt.constraints.required {
        let kind = property.get("type").cloned().unwrap();
        property.insert("type".into(), json!([kind, "null"]));
        if let Some(Value::Array(options)) = property.get_mut("enum") {
            options.push(Value::Null);
        }
    }
    Value::Object(property)
}
pub fn batch_document(batch: &Batch, context: &Context, errors: Option<&Rejections>) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut messages = Vec::new();
    for item in &batch.items {
        match item {
            Item::Prompt(p) => {
                properties.insert(p.id.to_string(), prompt_schema(p));
                if p.constraints.required {
                    required.push(p.id.to_string());
                }
            }
            Item::Message(v) => messages.push(v.clone()),
        }
    }
    let mut result = json!({"protocol":1, "status":"questions", "context":context_value(context),
        "schema":{"$schema":"https://json-schema.org/draft/2020-12/schema", "type":"object", "properties":properties, "required":required}, "messages":messages});
    if errors.is_some() || !batch.errors.is_empty() {
        let errors = errors.map(Vec::as_slice).unwrap_or_default();
        let mut grouped: Map<String, Value> = Map::new();
        let carried = batch
            .errors
            .iter()
            .filter(|c| !errors.iter().any(|e| e.id == c.id));
        for error in carried.chain(errors) {
            grouped
                .entry(error.id.to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(json!(error.message));
        }
        result["errors"] = Value::Object(grouped);
    }
    result
}
pub fn complete_document(answers: &Answers, context: &Context) -> Value {
    let values: Map<String, Value> = answers
        .iter()
        .map(|(id, answer)| (id.to_string(), answer.to_json()))
        .collect();
    json!({"protocol":1, "status":"complete", "context":context_value(context), "answers":values})
}

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_norway::from_str(include_str!(
        "../docs/specifications/interview-protocol.schema.yml"
    ))
    .expect("embedded protocol schema")
});
static ANSWERS: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let mut schema = SCHEMA.clone();
    schema["$ref"] = json!("#/$defs/answers");
    schema.as_object_mut().unwrap().remove("oneOf");
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .expect("embedded answers schema valid")
});
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ProtocolError(pub String);
pub fn parse_answers(text: &str) -> Result<RawAnswers, ProtocolError> {
    let value: Value = serde_json::from_str(text).map_err(|e| ProtocolError(e.to_string()))?;
    if let Err(error) = ANSWERS.validate(&value) {
        return Err(ProtocolError(error.to_string()));
    }
    let object = value.as_object().expect("validated answers object");
    object
        .iter()
        .map(|(key, value)| {
            Id::parse(key)
                .map(|id| (id, RawAnswer(value.clone())))
                .map_err(ProtocolError)
        })
        .collect()
}
pub fn protocol_schema() -> &'static Value {
    &SCHEMA
}

pub enum Headless<'a> {
    Completed {
        completed: Completed,
        accepted: Vec<IndexMap<String, Value>>,
    },
    Pending {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
        accepted: Vec<IndexMap<String, Value>>,
    },
}
pub fn answer_headless<'a>(
    template: &Template,
    mut interview: Interview<'a>,
    document: RawAnswers,
) -> Result<Headless<'a>, EvalError> {
    let mut accepted = Vec::new();
    let mut remaining = document;
    let mut first = true;
    loop {
        let Interview::Asking(pending) = interview else {
            let Interview::Complete(completed) = interview else {
                unreachable!()
            };
            if let Some(id) = remaining.keys().find(|id| !template.has_question_id(id)) {
                return Err(EvalError {
                    id: id.clone(),
                    field: "answer",
                    message: "is not a question in this template".into(),
                });
            }
            return Ok(Headless::Completed {
                completed,
                accepted,
            });
        };
        let ids: Vec<Id> = pending
            .batch()
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Prompt(p) => Some(p.id.clone()),
                _ => None,
            })
            .collect();
        // An answer held from an earlier document that failed when its
        // question was reached is not replaced silently by a default.
        if pending
            .batch()
            .errors
            .iter()
            .any(|e| !remaining.contains_key(&e.id))
        {
            return Ok(Headless::Pending {
                pending: Box::new(pending),
                rejections: vec![],
                accepted,
            });
        }
        let mut submission = RawAnswers::new();
        if first {
            let unknown: Vec<Id> = remaining
                .keys()
                .filter(|id| !pending.accepts_id(id) || pending.holds(id))
                .cloned()
                .collect();
            for id in unknown {
                submission.insert(id.clone(), remaining.shift_remove(&id).unwrap());
            }
            first = false;
        }
        for id in ids {
            if let Some(value) = remaining.shift_remove(&id) {
                submission.insert(id, value);
            }
        }
        let raw: IndexMap<String, Value> = submission
            .iter()
            .map(|(id, value)| (id.to_string(), value.0.clone()))
            .collect();
        match pending.answer(submission) {
            Ok(next) => {
                accepted.push(raw);
                interview = next;
            }
            Err(AnswerError::Rejected {
                pending,
                rejections,
            }) => {
                return Ok(Headless::Pending {
                    pending: Box::new(pending),
                    rejections,
                    accepted,
                });
            }
            Err(AnswerError::Eval(error)) => return Err(error),
        }
    }
}
