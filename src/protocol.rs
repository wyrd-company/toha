// ---
// relationships:
//   implements: interview-protocol
// ---
use crate::{
    AnswerError, Batch, Completed, EndKind, Ended, EvalError, Id, Interview, Item, Pending, Planned,
    PlannedHook, Prompt, PromptKind, RawAnswer, RawAnswers, Rejections, Template,
    staging::{CanonicalTarget, StagedRecord},
};
use indexmap::IndexMap;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct Context {
    target: String,
    template: String,
    /// The resolved commit of a git template; `None` for a local folder.
    commit: Option<String>,
}
impl Context {
    pub fn new(target: &CanonicalTarget, record: &StagedRecord) -> Self {
        Self {
            target: target.to_string(),
            template: record.template.clone(),
            commit: Some(record.commit.clone()).filter(|commit| !commit.is_empty()),
        }
    }
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
    if let Some(v) = &prompt.placeholder {
        property.insert("examples".into(), json!([v]));
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
        // The enum lists only the options; null is accepted beside it.
        if let Some(options) = property.remove("enum") {
            property.insert("anyOf".into(), json!([{"enum": options}, {"type": "null"}]));
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
/// One written target and the action that produced it: `create`, `overwrite`,
/// `inject`, or `update`. The CLI classifies each against the plan and target.
pub struct AppliedFile {
    pub path: String,
    pub action: String,
}
/// One hook that ran during a successful apply: its id when the hook declares
/// one, its exit code, and whether it succeeded. Captured streams never appear
/// here, so a result document carries no hook output bytes.
pub struct AppliedHook {
    pub id: Option<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
}
/// The record of a completed apply, built by the CLI from the plan, the written
/// paths, the completed interview's messages, and the hooks that ran. The
/// scripted route serializes it as the `applied` result document.
pub struct ApplyReport {
    pub files: Vec<AppliedFile>,
    pub messages: Vec<String>,
    pub hooks: Vec<AppliedHook>,
}
/// The `applied` result document: every written path with its action, the
/// completion messages, and the hooks that ran. Exit 0.
pub fn applied_document(report: &ApplyReport, context: &Context) -> Value {
    let files: Vec<Value> = report
        .files
        .iter()
        .map(|file| json!({"path": file.path, "action": file.action}))
        .collect();
    let hooks: Vec<Value> = report
        .hooks
        .iter()
        .map(|hook| json!({"id": hook.id, "exit_code": hook.exit_code, "success": hook.success}))
        .collect();
    json!({"protocol":1, "status":"applied", "context":context_value(context),
        "files":files, "messages":report.messages, "hooks":hooks})
}
/// Whether a planned apply's hooks would run without an extra grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustState {
    Trusted,
    Untrusted,
}
/// One planned hook previewed for a dry run: its rendered argument vector and
/// cwd. A deferred hook shows its `{{ id.field }}` placeholders unrendered.
fn planned_hook_value(planned: &Planned<PlannedHook>) -> Value {
    let (argv, cwd) = match planned {
        Planned::Ready(hook) => (hook.argv(), hook.cwd.as_ref().map(ToString::to_string)),
        Planned::AfterHooks(deferred) => deferred
            .hook_preview()
            .expect("a deferred plan hook previews as a hook"),
    };
    json!({"command": argv, "cwd": cwd.unwrap_or_else(|| ".".into())})
}
/// The `planned` result document: every planned path with its action, the
/// completion messages, the planned hooks, and whether they are trusted. Exit 0,
/// or exit 3 when the hooks are untrusted.
pub fn planned_document(
    plan: &crate::Plan,
    messages: &[String],
    context: &Context,
    trust: TrustState,
) -> Value {
    let mut files: Vec<Value> = plan
        .files
        .iter()
        .map(|file| {
            let action = if plan.conflicts.contains(&file.path) {
                "conflict"
            } else {
                "create"
            };
            json!({"path": file.path.to_string(), "action": action})
        })
        .collect();
    // Injected targets are planned writes too; a create flag marks a target the
    // edit would author, an update an existing target it would change.
    for edit in &plan.edits {
        let create = match edit {
            crate::PlannedEdit::Region(region) => region.create,
            crate::PlannedEdit::JsonValue(json) => json.create,
        };
        files.push(json!({
            "path": edit.path().to_string(),
            "action": if create { "inject" } else { "update" },
        }));
    }
    let hooks: Vec<Value> = plan.hooks.iter().map(planned_hook_value).collect();
    json!({"protocol":1, "status":"planned", "context":context_value(context),
        "files":files, "messages":messages, "hooks":hooks,
        "trusted": trust == TrustState::Trusted})
}
/// The kind of a scripted `error` result document. Every route-preparation and
/// document fault maps to exactly one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Input,
    Document,
    Identity,
    Staged,
    Source,
    Replay,
    Conflict,
    Render,
    Hook,
    Ambiguous,
}
impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Document => "document",
            Self::Identity => "identity",
            Self::Staged => "staged",
            Self::Source => "source",
            Self::Replay => "replay",
            Self::Conflict => "conflict",
            Self::Render => "render",
            Self::Hook => "hook",
            Self::Ambiguous => "ambiguous",
        }
    }
}
/// A scripted-route failure: its kind, a message, and the commands that finish,
/// discard, or retry the request, when there are any.
pub struct ResultError {
    pub kind: ErrorKind,
    pub message: String,
    pub commands: Vec<String>,
}
/// The `error` result document: a `kind`, a `message`, and optional `commands`.
/// `context` is present once the route established a target and template. Exit 1,
/// or exit 5 for an `ambiguous` template.
pub fn error_document(error: &ResultError, context: Option<&Context>) -> Value {
    let mut document = json!({"protocol":1, "status":"error", "kind":error.kind.as_str(),
        "message":error.message});
    if let Some(context) = context {
        document["context"] = context_value(context);
    }
    if !error.commands.is_empty() {
        document["commands"] = json!(error.commands);
    }
    document
}
/// The document for a flow `stop`/`abort`. The only serializer of an end, used
/// by the headless and staged drivers, so their bytes match for identical
/// submissions.
pub fn ended_document(ended: &Ended, context: &Context) -> Value {
    let kind = match ended.kind() {
        EndKind::Stop => "stop",
        EndKind::Abort => "abort",
    };
    let mut document = json!({"protocol":1, "status":"ended", "context":context_value(context),
        "kind":kind, "messages":ended.last_messages});
    if let Some(label) = ended.label() {
        document["label"] = json!(label);
    }
    document
}

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_norway::from_str(include_str!(
        "../docs/specifications/interview-protocol.schema.yml"
    ))
    .expect("embedded protocol schema")
});
#[derive(Debug, thiserror::Error)]
pub enum SubmitDocumentError {
    #[error(transparent)]
    Document(#[from] AnswersDocumentError),

    #[error(transparent)]
    Evaluation(#[from] EvalError),
}

#[derive(Debug, thiserror::Error)]
pub enum AnswersDocumentError {
    #[error("invalid JSON: {message}")]
    Json { message: String },

    #[error("invalid answers document: {message}")]
    Shape { message: String },

    #[error("answers document has no template identity")]
    MissingIdentity,

    #[error("answers document template must be a non-empty string")]
    MalformedIdentity,

    #[error("answers template {declared:?} does not match expected template {expected:?}")]
    TemplateMismatch { declared: String, expected: String },
}

/// The parsed envelope, before identity verification. Private, so external JSON
/// never yields a raw answer map without passing the identity gate first.
struct WireAnswersDocument {
    template: String,
    answers: Map<String, Value>,
}
/// A verified submission: the raw answers for the engine and the inner accepted
/// map for storage. Private, has no unchecked constructor, and cannot be turned
/// into `RawAnswers` by a caller.
struct VerifiedSubmission {
    raw: RawAnswers,
    stored: IndexMap<String, Value>,
}

/// Parses one external answers document and verifies its declared identity
/// against `expected_template` by exact string equality, before any answer is
/// evaluated. The declaration is never resolved, trimmed, canonicalized,
/// alias-mapped, fetched, or trust-checked.
fn parse_and_verify(
    expected_template: &str,
    text: &str,
) -> Result<VerifiedSubmission, AnswersDocumentError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| AnswersDocumentError::Json { message: e.to_string() })?;
    let object = value.as_object().ok_or_else(|| AnswersDocumentError::Shape {
        message: "expected a JSON object with \"template\" and \"answers\"".into(),
    })?;
    // No template assertion is a legacy bare answer map; the identity diagnostic
    // shows the required wrapper and the expected formal name.
    let template = match object.get("template") {
        None => return Err(AnswersDocumentError::MissingIdentity),
        Some(Value::String(template)) if template.is_empty() => {
            return Err(AnswersDocumentError::MalformedIdentity);
        }
        Some(Value::String(template)) => template.clone(),
        Some(_) => return Err(AnswersDocumentError::MalformedIdentity),
    };
    // The envelope carries exactly `template` and `answers`.
    if let Some(key) = object
        .keys()
        .find(|key| *key != "template" && *key != "answers")
    {
        return Err(AnswersDocumentError::Shape {
            message: format!(
                "unexpected member {key:?}; an answers document has only \"template\" and \"answers\""
            ),
        });
    }
    let answers = match object.get("answers") {
        None => {
            return Err(AnswersDocumentError::Shape {
                message: "missing \"answers\" object".into(),
            });
        }
        Some(Value::Object(answers)) => answers.clone(),
        Some(_) => {
            return Err(AnswersDocumentError::Shape {
                message: "\"answers\" must be a JSON object keyed by question id".into(),
            });
        }
    };
    let wire = WireAnswersDocument { template, answers };
    // Exact string equality: a mismatch wins over every nested question or value
    // fault, and reaches no answer evaluation.
    if wire.template != expected_template {
        return Err(AnswersDocumentError::TemplateMismatch {
            declared: wire.template,
            expected: expected_template.to_string(),
        });
    }
    let mut raw = RawAnswers::new();
    let mut stored = IndexMap::new();
    for (key, value) in wire.answers {
        let id = Id::parse(&key).map_err(|message| AnswersDocumentError::Shape { message })?;
        raw.insert(id, RawAnswer(value.clone()));
        stored.insert(key, value);
    }
    Ok(VerifiedSubmission { raw, stored })
}

/// The outcome of `answer_document_once`: the interview advanced by one accepted
/// document, or the same batch with the rejections of a refused document.
pub enum DocumentStep<'a> {
    Accepted {
        interview: Interview<'a>,
        submission: IndexMap<String, Value>,
    },
    Rejected {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
    },
}

/// `continue PATH FILE`: verifies the document identity, then submits it as one
/// step (`Pending::answer` at most once). On acceptance it returns the advanced
/// interview and the inner accepted map to store; on rejection it returns the
/// same batch and the rejections and nothing is stored.
pub fn answer_document_once<'a>(
    expected_template: &str,
    pending: Pending<'a>,
    text: &str,
) -> Result<DocumentStep<'a>, SubmitDocumentError> {
    let verified = parse_and_verify(expected_template, text)?;
    match pending.answer(verified.raw) {
        Ok(interview) => Ok(DocumentStep::Accepted {
            interview,
            submission: verified.stored,
        }),
        Err(AnswerError::Rejected {
            pending,
            rejections,
        }) => Ok(DocumentStep::Rejected {
            pending: Box::new(pending),
            rejections,
        }),
        Err(AnswerError::Eval(error)) => Err(SubmitDocumentError::Evaluation(error)),
    }
}

/// The scripted route: verifies the document identity, then drives the existing
/// multi-batch headless walk. The whole document is one submission; later batches
/// take their defaults.
pub fn answer_document_headless<'a>(
    expected_template: &str,
    template: &'a Template,
    interview: Interview<'a>,
    text: &str,
) -> Result<Headless<'a>, SubmitDocumentError> {
    let verified = parse_and_verify(expected_template, text)?;
    Ok(answer_headless(template, interview, verified.raw)?)
}
pub fn protocol_schema() -> &'static Value {
    &SCHEMA
}

#[allow(clippy::large_enum_variant)]
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
    /// A flow `stop`/`abort` ended the interview.
    Ended {
        ended: Ended,
        accepted: Vec<IndexMap<String, Value>>,
    },
}
pub fn answer_headless<'a>(
    template: &Template,
    mut interview: Interview<'a>,
    document: RawAnswers,
) -> Result<Headless<'a>, EvalError> {
    let mut accepted = Vec::new();
    // The whole document is one submission, as through `continue`: answers
    // for later questions are validated and held, then applied when their
    // questions are reached. Later batches take their defaults.
    let mut remaining = document;
    loop {
        let pending = match interview {
            Interview::Asking(pending) => pending,
            Interview::Complete(mut completed) => {
                // Only an interview complete before any submission leaves answers.
                if let Some(id) = remaining.keys().find(|id| !template.has_question_id(id)) {
                    return Err(EvalError {
                        id: id.clone(),
                        field: "answer",
                        message: "is not a question in this template".into(),
                        expression: None,
                        config_key: None,
                    });
                }
                // An interview complete before any submission skipped each of
                // its questions, so none of these answers is used.
                completed.warn_unused(&remaining);
                return Ok(Headless::Completed {
                    completed,
                    accepted,
                });
            }
            // A flow stop/abort ended the interview; any remaining answers are
            // unused. The driver removes the staged record on `Abort`.
            Interview::Ended(ended) => return Ok(Headless::Ended { ended, accepted }),
        };
        let submission = std::mem::take(&mut remaining);
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
