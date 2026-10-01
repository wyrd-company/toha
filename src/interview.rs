// ---
// relationships:
//   implements: architecture
// ---
use crate::{
    config::{ConfigEntry, DefaultSource, PresetName},
    context::InvocationContext,
    jinja::{Expr, Typed, context_from_answers, is_global, is_reserved},
    template::{FlowAction, Id, Node, Question, QuestionKind, SkipScope, Template},
};
use indexmap::IndexMap;
use serde_json::Value;
use std::{collections::HashSet, fmt};

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Text(String),
    Bool(bool),
    List(Vec<String>),
    None,
    Value(Value),
}
impl Answer {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Text(v) => Value::String(v.clone()),
            Self::Bool(v) => Value::Bool(*v),
            Self::List(v) => serde_json::json!(v),
            Self::None => Value::Null,
            Self::Value(v) => v.clone(),
        }
    }
}
pub type Answers = IndexMap<Id, Answer>;
#[derive(Debug, Clone)]
pub struct RawAnswer(pub Value);
pub type RawAnswers = IndexMap<Id, RawAnswer>;
#[derive(Debug, Clone)]
pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}
#[derive(Debug, Clone)]
struct InterviewSeed {
    now: jiff::Zoned,
    defaults: IndexMap<Id, DefaultBankEntry>,
    /// The invocation context, or `None` for the pre-context legacy projection
    /// route used by [`Resolution::start`], which projects none of the reserved
    /// names.
    context: Option<InvocationContext>,
}
#[derive(Debug, Clone)]
enum DefaultBankEntry {
    Seed(RawAnswer),
    Configured(ResolvedDefault),
    /// A default sourced from a snapshot's recorded answers (the generate axis's
    /// `--like`). Carries the snapshot id as provenance so a prompt or a
    /// rejection can name where the default came from. Precedence is resolved
    /// before the bank is built, so a `Snapshot` entry and a `Configured` entry
    /// never coexist for one id: the bank still holds exactly one occupant per id.
    Snapshot {
        raw: RawAnswer,
        from: String,
    },
}
/// A snapshot-sourced seed for [`Resolution::start_with_seed`]: the folded
/// defaults (one raw answer per id) and the snapshot id they came from. Built at
/// the CLI-side boundary from a selected snapshot; the engine never sees a
/// `Snapshot`, a `Project`, or any gitoxide type.
#[derive(Debug, Clone)]
pub struct SnapshotSeed {
    pub defaults: IndexMap<Id, RawAnswer>,
    /// The snapshot id, for prompt and rejection provenance and the result docs.
    pub from: String,
}
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
    /// A flow `stop` or `abort` ended the interview. No `Completed` exists, so
    /// there is no API path to a plan or apply.
    Ended(Ended),
}
/// How a completed interview's plan step is treated. A `dry-run` flow action
/// rides here as [`Disposition::DryRun`]; re-derived on every walk, never
/// persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition {
    #[default]
    Proceed,
    DryRun,
}
/// How a flow terminated the interview. Both write no files and run no hooks;
/// `Abort` additionally asks the driver to remove the staged record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    Stop,
    Abort,
}
/// The routing outcome of a completed interview. Stop/abort are not here — they
/// are [`Interview::Ended`], handled before any [`Completed`] exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Plan { apply: bool },
}
/// A stop or abort. Carries only what a driver prints; nothing a plan consumes.
#[derive(Debug)]
pub struct Ended {
    /// `Abort` instructs the driver to remove the target's staged record.
    pub kind: EndKind,
    /// Every message reached, in interview order.
    pub messages: Vec<String>,
    /// The messages reached by the step that ended the interview.
    pub last_messages: Vec<String>,
    /// The diagnostic label of the flow node that ended the interview.
    pub label: Option<String>,
}
impl Ended {
    pub fn kind(&self) -> EndKind {
        self.kind
    }
    pub fn messages(&self) -> &[String] {
        &self.messages
    }
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}
#[derive(Debug)]
pub struct Pending<'a> {
    template: &'a Template,
    seed: InterviewSeed,
    answers: Answers,
    accepted_raw: IndexMap<Id, RawAnswer>,
    held: RawAnswers,
    skipped: Skipped,
    messages: Vec<String>,
    hooks: Vec<RenderedHook>,
    visited: HashSet<String>,
    batch: Batch,
}
#[derive(Debug)]
pub struct Completed {
    pub answers: Answers,
    /// The accepted EFFECTIVE RAW value per id BEFORE formatting — the submitted
    /// raw for an answered id, or the pre-format default (incl. a snapshot seed)
    /// for a defaulted id. An internal in-process readback seam; not serialized.
    accepted_raw: IndexMap<Id, RawAnswer>,
    /// Every message reached, in interview order.
    pub messages: Vec<String>,
    /// The messages reached by the step that completed the interview.
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    /// Whether a flow `dry-run` fired; re-derived each walk, never persisted.
    pub disposition: Disposition,
    /// The immutable invocation context carried from the seed, or `None` for a
    /// context-free start (`Resolution::start`).
    context: Option<InvocationContext>,
    skipped: Skipped,
    step_start: usize,
}
/// The questions skipped by a `when`, in interview order, each with the
/// number of messages reached before it was skipped.
type Skipped = IndexMap<Id, usize>;
impl Completed {
    /// The accepted effective raw values (pre-format) per id, including the
    /// snapshot-seeded defaults the interview applied.
    pub fn accepted_raw(&self) -> &IndexMap<Id, RawAnswer> {
        &self.accepted_raw
    }
    pub fn disposition(&self) -> Disposition {
        self.disposition
    }
    /// The immutable invocation context this interview carried, or `None` for a
    /// context-free start.
    pub fn context(&self) -> Option<&InvocationContext> {
        self.context.as_ref()
    }
    /// The one policy seam: how a completed interview's plan step is treated.
    pub fn step(&self) -> Step {
        match self.disposition {
            Disposition::Proceed => Step::Plan { apply: true },
            Disposition::DryRun => Step::Plan { apply: false },
        }
    }
    /// Adds the warning for each answer in `unused` whose question was
    /// skipped, at the position the question was skipped.
    pub(crate) fn warn_unused(&mut self, unused: &RawAnswers) {
        let mut added = 0;
        for (id, position) in &self.skipped {
            if unused.contains_key(id) {
                self.messages.insert(position + added, skipped_warning(id));
                added += 1;
            }
        }
        self.last_messages = self.messages[self.step_start..].to_vec();
    }
}
/// The warning for an answer to `id` that is not used because its question
/// is skipped.
pub fn skipped_warning(id: &Id) -> String {
    format!("warning: answer for \"{id}\" was not used: the question was skipped")
}
#[derive(Debug, Clone)]
pub struct RenderedHook {
    pub program: RenderedProgram,
    pub cwd: Option<String>,
    /// Opt-in result identity, carried from the hook node so the plan and apply
    /// can record this hook's result. `None` for a hook exactly as before.
    pub id: Option<crate::template::Id>,
    pub capture: crate::template::Capture,
    pub allow_failure: bool,
    pub parse_json: bool,
    pub status_id: Option<crate::template::Id>,
}
#[derive(Debug, Clone)]
pub enum RenderedProgram {
    Run(Vec<String>),
    Script {
        path: std::path::PathBuf,
        args: Vec<String>,
    },
}
/// A fault in the template expression `source` of a hook's `field`.
#[derive(Debug)]
pub(crate) struct HookFault {
    pub field: &'static str,
    pub source: String,
    pub message: String,
}
fn render_hook(
    hook: &crate::template::HookNode,
    ctx: &impl serde::Serialize,
) -> Result<RenderedHook, HookFault> {
    use crate::template::HookProgram;
    let render = |field: &'static str| {
        move |t: &crate::jinja::Tmpl| {
            t.render(ctx).map_err(|e| HookFault {
                field,
                source: t.source().into(),
                message: e.to_string(),
            })
        }
    };
    let program = match &hook.command.program {
        HookProgram::Run(args) => {
            RenderedProgram::Run(args.iter().map(render("run")).collect::<Result<_, _>>()?)
        }
        HookProgram::Script { path, args } => RenderedProgram::Script {
            path: path.clone(),
            args: args.iter().map(render("args")).collect::<Result<_, _>>()?,
        },
    };
    Ok(RenderedHook {
        program,
        cwd: hook.command.cwd.as_ref().map(render("cwd")).transpose()?,
        id: hook.id.clone(),
        capture: hook.capture,
        allow_failure: hook.allow_failure,
        parse_json: hook.parse_json,
        status_id: hook.status_id.clone(),
    })
}
/// Renders a hook once, or once per item of its `each`.
pub(crate) fn render_hooks(
    hook: &crate::template::HookNode,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<Vec<RenderedHook>, HookFault> {
    match &hook.each {
        None => Ok(vec![render_hook(hook, ctx)?]),
        Some(each) => each
            .contexts(ctx)
            .map_err(|message| HookFault {
                field: "each",
                source: each.expr.source().into(),
                message,
            })?
            .iter()
            .map(|local| render_hook(hook, local))
            .collect(),
    }
}
#[derive(Debug, Default)]
pub struct Batch {
    pub items: Vec<Item>,
    /// Recoverable errors for configured defaults and for answers held from an
    /// earlier document that failed validation when their question was reached.
    pub errors: Rejections,
    default_sources: IndexMap<Id, PreparedDefaultSource>,
}
#[derive(Debug)]
pub enum Item {
    Prompt(Prompt),
    Message(String),
}
#[derive(Debug)]
pub struct Prompt {
    pub id: Id,
    pub kind: PromptKind,
    pub title: String,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub default: Option<Answer>,
    pub options: Vec<String>,
    pub constraints: Constraints,
}
#[derive(Debug)]
struct PreparedPrompt {
    prompt: Prompt,
    default_source: Option<PreparedDefaultSource>,
    configured_rejection: Option<Rejection>,
}
#[derive(Debug, Clone)]
struct PreparedDefault {
    answer: Answer,
    source: PreparedDefaultSource,
}
#[derive(Debug, Clone)]
enum PreparedDefaultSource {
    Template {
        expression: Option<String>,
    },
    Configured(ConfiguredDefaultOrigin),
    Seed,
    /// A snapshot-seeded default, naming the snapshot it came from.
    Snapshot {
        from: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Text,
    Multiline,
    Confirm,
    Select,
    MultiSelect,
    TextLoop,
}
#[derive(Debug, Default)]
pub struct Constraints {
    pub required: bool,
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub regex: Option<String>,
    pub loop_min: Option<u32>,
    pub loop_max: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub id: Id,
    pub message: String,
    pub kind: RejectionKind,
}
/// Why an answer is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionKind {
    /// The answer fails a constraint of its question.
    Invalid,
    /// The question already has a different answer. A driver adds how to
    /// change it.
    Answered,
}
pub type Rejections = Vec<Rejection>;
/// A failure to evaluate the interview. When `expression` is set, the fault
/// is in that template expression at `id`.`field`. When `field` is
/// [`CONFIGURED_DEFAULT`], the fault is in the configured default of `id`.
#[derive(Debug)]
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
    pub expression: Option<String>,
    pub config_key: Option<String>,
}
/// The `field` of an [`EvalError`] in a configured default.
pub const CONFIGURED_DEFAULT: &str = "configured default";
/// Generic attribution retained after the flat configured value enters the engine.
fn configured_default_source(id: &Id) -> String {
    format!("configured default for question \"{id}\"")
}
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field == CONFIGURED_DEFAULT {
            let key = self
                .config_key
                .clone()
                .unwrap_or_else(|| configured_default_source(&self.id));
            return write!(f, "{key}: {}", self.message);
        }
        match (&self.expression, self.field) {
            (Some(source), _) => write!(
                f,
                "template error in {}.{} `{}`: {}",
                self.id, self.field, source, self.message
            ),
            (None, "default") => {
                write!(f, "template error in {}.default: {}", self.id, self.message)
            }
            (None, _) => write!(f, "{}.{}: {}", self.id, self.field, self.message),
        }
    }
}
impl std::error::Error for EvalError {}
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum AnswerError<'a> {
    Rejected {
        pending: Pending<'a>,
        rejections: Rejections,
    },
    Eval(EvalError),
}
impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.id, self.message)
    }
}
fn eval_error(id: &Id, field: &'static str, error: impl ToString) -> EvalError {
    EvalError {
        id: id.clone(),
        field,
        message: error.to_string(),
        expression: None,
        config_key: None,
    }
}
/// A fault in the template expression `source` at `id`.`field`.
fn fault(id: &Id, field: &'static str, source: Option<&str>, error: impl ToString) -> EvalError {
    EvalError {
        expression: source.map(str::to_owned),
        ..eval_error(id, field, error)
    }
}
fn context(
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
) -> std::collections::BTreeMap<String, Value> {
    let mut values = context_from_answers(answers, &template.data, &seed.now);
    // The single context builder: a current invocation projects the seventeen
    // reserved names; a legacy or context-free invocation projects none.
    // Readiness, interview rendering, and planning all build the context here.
    if let Some(context) = &seed.context {
        context.project(&mut values);
    }
    values
}
fn has_refs(refs: &HashSet<String>, answers: &Answers, template: &Template) -> bool {
    refs.iter().all(|r| {
        template.data.keys().any(|id| id.as_str() == r)
            || answers.keys().any(|id| id.as_str() == r)
            || is_global(r)
            || (template.reserves_context() && is_reserved(r))
    })
}
fn typed_ready<T: Clone + serde::de::DeserializeOwned>(
    v: &Typed<T>,
    a: &Answers,
    t: &Template,
) -> bool {
    has_refs(&v.references(), a, t)
}
fn tmpl_ready(v: &crate::jinja::Tmpl, a: &Answers, t: &Template) -> bool {
    has_refs(v.references(), a, t)
}
fn expr_ready(v: &Expr, a: &Answers, t: &Template) -> bool {
    has_refs(v.references(), a, t)
}
fn hook_ready(h: &crate::template::HookNode, a: &Answers, t: &Template) -> bool {
    use crate::template::HookProgram;
    let binding = h.each.as_ref().map(|e| e.binding.as_str());
    let ready = |v: &crate::jinja::Tmpl| {
        v.references()
            .iter()
            .filter(|name| Some(name.as_str()) != binding)
            .all(|name| has_refs(&HashSet::from([name.clone()]), a, t))
    };
    h.each.as_ref().is_none_or(|e| expr_ready(&e.expr, a, t))
        && h.command.cwd.as_ref().is_none_or(ready)
        && match &h.command.program {
            HookProgram::Run(v) => v.iter().all(ready),
            HookProgram::Script { args, .. } => args.iter().all(ready),
        }
}
fn question_ready(q: &Question, a: &Answers, t: &Template, seed: &InterviewSeed) -> bool {
    let base = tmpl_ready(&q.prompt, a, t)
        && q.description.as_ref().is_none_or(|v| tmpl_ready(v, a, t))
        && q.placeholder.as_ref().is_none_or(|v| tmpl_ready(v, a, t))
        && typed_ready(&q.required, a, t)
        && q.when.as_ref().is_none_or(|v| expr_ready(v, a, t))
        && q.validate.min.as_ref().is_none_or(|v| typed_ready(v, a, t))
        && q.validate.max.as_ref().is_none_or(|v| typed_ready(v, a, t))
        && q.format.as_ref().is_none_or(|v| {
            v.references()
                .iter()
                .filter(|name| name.as_str() != "value")
                .all(|name| has_refs(&HashSet::from([name.clone()]), a, t))
        });
    if !base {
        return false;
    }
    let override_default = seed.defaults.contains_key(&q.id);
    match &q.kind {
        QuestionKind::Text { default } | QuestionKind::Multiline { default } => {
            override_default || default.as_ref().is_none_or(|v| tmpl_ready(v, a, t))
        }
        QuestionKind::Confirm { default } => {
            override_default || default.as_ref().is_none_or(|v| typed_ready(v, a, t))
        }
        QuestionKind::Select { options, default } => {
            typed_ready(options, a, t)
                && (override_default || default.as_ref().is_none_or(|v| tmpl_ready(v, a, t)))
        }
        QuestionKind::MultiSelect { options, default } => {
            typed_ready(options, a, t)
                && (override_default || default.as_ref().is_none_or(|v| typed_ready(v, a, t)))
        }
        QuestionKind::TextLoop { default, min, max } => {
            (override_default || default.as_ref().is_none_or(|v| typed_ready(v, a, t)))
                && min.as_ref().is_none_or(|v| typed_ready(v, a, t))
                && max.as_ref().is_none_or(|v| typed_ready(v, a, t))
        }
    }
}
fn eval_typed<T: Clone + serde::de::DeserializeOwned>(
    v: &Typed<T>,
    ctx: &impl serde::Serialize,
    id: &Id,
    field: &'static str,
) -> Result<T, EvalError> {
    v.eval(ctx).map_err(|e| fault(id, field, v.source(), e))
}
fn default_ready(q: &Question, a: &Answers, t: &Template, seed: &InterviewSeed) -> bool {
    if seed.defaults.contains_key(&q.id) {
        return true;
    }
    match &q.kind {
        QuestionKind::Text { default }
        | QuestionKind::Multiline { default }
        | QuestionKind::Select { default, .. } => {
            default.as_ref().is_none_or(|v| tmpl_ready(v, a, t))
        }
        QuestionKind::Confirm { default } => default.as_ref().is_none_or(|v| typed_ready(v, a, t)),
        QuestionKind::MultiSelect { default, .. } | QuestionKind::TextLoop { default, .. } => {
            default.as_ref().is_none_or(|v| typed_ready(v, a, t))
        }
    }
}
fn render_default(
    q: &Question,
    t: &Template,
    a: &Answers,
    seed: &InterviewSeed,
) -> Result<Option<PreparedDefault>, EvalError> {
    let id = &q.id;
    let kind = prompt_kind(q);
    if let Some(entry) = seed.defaults.get(id) {
        let (raw, source) = match entry {
            DefaultBankEntry::Seed(raw) => (raw, PreparedDefaultSource::Seed),
            DefaultBankEntry::Configured(value) => (
                &value.raw,
                PreparedDefaultSource::Configured(value.origin.clone()),
            ),
            DefaultBankEntry::Snapshot { raw, from } => {
                (raw, PreparedDefaultSource::Snapshot { from: from.clone() })
            }
        };
        return parse_kind(id, kind, raw.0.clone())
            .map(|answer| Some(PreparedDefault { answer, source }))
            .map_err(|e| eval_error(id, CONFIGURED_DEFAULT, e.message));
    }
    let ctx = context(t, a, seed);
    match &q.kind {
        QuestionKind::Text { default }
        | QuestionKind::Multiline { default }
        | QuestionKind::Select { default, .. } => default
            .as_ref()
            .map(|v| {
                v.render(&ctx)
                    .map(|answer| PreparedDefault {
                        answer: Answer::Text(answer),
                        source: PreparedDefaultSource::Template {
                            expression: Some(v.source().into()),
                        },
                    })
                    .map_err(|e| fault(id, "default", Some(v.source()), e))
            })
            .transpose(),
        QuestionKind::Confirm { default } => default
            .as_ref()
            .map(|v| {
                eval_typed(v, &ctx, id, "default").map(|answer| PreparedDefault {
                    answer: Answer::Bool(answer),
                    source: PreparedDefaultSource::Template {
                        expression: v.source().map(str::to_owned),
                    },
                })
            })
            .transpose(),
        QuestionKind::MultiSelect { default, .. } | QuestionKind::TextLoop { default, .. } => {
            default
                .as_ref()
                .map(|v| {
                    eval_typed(v, &ctx, id, "default").map(|answer| PreparedDefault {
                        answer: Answer::List(answer),
                        source: PreparedDefaultSource::Template {
                            expression: v.source().map(str::to_owned),
                        },
                    })
                })
                .transpose()
        }
    }
}
fn make_prompt(
    q: &Question,
    t: &Template,
    a: &Answers,
    seed: &InterviewSeed,
) -> Result<PreparedPrompt, EvalError> {
    let ctx = context(t, a, seed);
    let id = &q.id;
    let title = q
        .prompt
        .render(&ctx)
        .map_err(|e| fault(id, "prompt", Some(q.prompt.source()), e))?;
    let description = q
        .description
        .as_ref()
        .map(|v| {
            v.render(&ctx)
                .map_err(|e| fault(id, "description", Some(v.source()), e))
        })
        .transpose()?;
    let placeholder = q
        .placeholder
        .as_ref()
        .map(|v| {
            v.render(&ctx)
                .map_err(|e| fault(id, "placeholder", Some(v.source()), e))
        })
        .transpose()?;
    let required = eval_typed(&q.required, &ctx, id, "required")?;
    let min = q
        .validate
        .min
        .as_ref()
        .map(|v| eval_typed(v, &ctx, id, "validate.min"))
        .transpose()?;
    let max = q
        .validate
        .max
        .as_ref()
        .map(|v| eval_typed(v, &ctx, id, "validate.max"))
        .transpose()?;
    let mut options = vec![];
    let mut loop_min = None;
    let mut loop_max = None;
    let kind = match &q.kind {
        QuestionKind::Text { .. } => PromptKind::Text,
        QuestionKind::Multiline { .. } => PromptKind::Multiline,
        QuestionKind::Confirm { .. } => PromptKind::Confirm,
        QuestionKind::Select { options: o, .. } => {
            options = eval_typed(o, &ctx, id, "options")?;
            PromptKind::Select
        }
        QuestionKind::MultiSelect { options: o, .. } => {
            options = eval_typed(o, &ctx, id, "options")?;
            PromptKind::MultiSelect
        }
        QuestionKind::TextLoop { min, max, .. } => {
            loop_min = min
                .as_ref()
                .map(|v| eval_typed(v, &ctx, id, "loop.min"))
                .transpose()?;
            loop_max = max
                .as_ref()
                .map(|v| eval_typed(v, &ctx, id, "loop.max"))
                .transpose()?;
            PromptKind::TextLoop
        }
    };
    let prepared = render_default(q, t, a, seed)?;
    let mut prompt = Prompt {
        id: id.clone(),
        kind,
        title,
        description,
        placeholder,
        default: prepared.as_ref().map(|value| value.answer.clone()),
        options,
        constraints: Constraints {
            required,
            min,
            max,
            regex: q.validate.regex.as_ref().map(|r| r.as_str().into()),
            loop_min,
            loop_max,
        },
    };
    let default_source = prepared.as_ref().map(|value| value.source.clone());
    let mut configured_rejection = None;
    if let Some(prepared) = prepared {
        if let Err(rejected) =
            validate(id, &Rules::of_prompt(&prompt, q), prepared.answer.to_json())
        {
            match prepared.source {
                PreparedDefaultSource::Template { expression } => {
                    return Err(EvalError {
                        id: id.clone(),
                        field: "default",
                        message: format!(
                            "default {} is not allowed: {}",
                            prepared.answer.to_json(),
                            rejected.message
                        ),
                        expression,
                        config_key: None,
                    });
                }
                PreparedDefaultSource::Configured(origin) => {
                    prompt.default = None;
                    configured_rejection = Some(rejection(
                        id,
                        format!(
                            "default {} from {} is not allowed: {}",
                            prepared.answer.to_json(),
                            origin.description(),
                            rejected.message
                        ),
                    ));
                }
                // A snapshot-seeded default is an optional default, like a replay
                // seed: keep it shown so the person can accept or override, and
                // let the headless walk re-ask (exit 4) rather than writing an
                // invalid value. It is never a hard error.
                PreparedDefaultSource::Seed | PreparedDefaultSource::Snapshot { .. } => {}
            }
        }
    }
    Ok(PreparedPrompt {
        prompt,
        default_source,
        configured_rejection,
    })
}
fn parse_kind(id: &Id, kind: PromptKind, value: Value) -> Result<Answer, Rejection> {
    let fail = |msg: &str| rejection(id, msg);
    match kind {
        PromptKind::Text | PromptKind::Multiline | PromptKind::Select => value
            .as_str()
            .map(|v| Answer::Text(v.into()))
            .ok_or_else(|| fail("must be a string")),
        PromptKind::Confirm => value
            .as_bool()
            .map(Answer::Bool)
            .ok_or_else(|| fail("must be true or false")),
        PromptKind::MultiSelect | PromptKind::TextLoop => value
            .as_array()
            .and_then(|v| {
                v.iter()
                    .map(|x| x.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .map(Answer::List)
            .ok_or_else(|| fail("must be an array of strings")),
    }
}
/// The answer of a question left empty with no default: `[]` for a list
/// answer, `none` otherwise.
fn empty_answer(kind: PromptKind) -> Answer {
    match kind {
        PromptKind::MultiSelect | PromptKind::TextLoop => Answer::List(vec![]),
        _ => Answer::None,
    }
}
fn count(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
/// The constraints an answer must satisfy. A constraint that is `None` is
/// absent or not yet known.
struct Rules<'q> {
    kind: PromptKind,
    required: Option<bool>,
    min: Option<u32>,
    max: Option<u32>,
    loop_min: Option<u32>,
    loop_max: Option<u32>,
    options: Option<Vec<String>>,
    regex: Option<&'q regex::Regex>,
}
impl<'q> Rules<'q> {
    fn of_prompt(prompt: &Prompt, q: &'q Question) -> Self {
        let c = &prompt.constraints;
        Self {
            kind: prompt.kind,
            required: Some(c.required),
            min: c.min,
            max: c.max,
            loop_min: c.loop_min,
            loop_max: c.loop_max,
            options: matches!(prompt.kind, PromptKind::Select | PromptKind::MultiSelect)
                .then(|| prompt.options.clone()),
            regex: q.validate.regex.as_ref(),
        }
    }
}
impl<'q> Rules<'q> {
    /// The constraints of `q` that are known before its question is reached:
    /// literal values only.
    fn of_question(q: &'q Question) -> Self {
        let literal = |v: &Option<Typed<u32>>| v.as_ref().and_then(|v| v.literal().copied());
        let (options, loop_min, loop_max) = match &q.kind {
            QuestionKind::Select { options, .. } | QuestionKind::MultiSelect { options, .. } => {
                (options.literal().cloned(), None, None)
            }
            QuestionKind::TextLoop { min, max, .. } => (None, literal(min), literal(max)),
            _ => (None, None, None),
        };
        Self {
            kind: prompt_kind(q),
            required: q.required.literal().copied(),
            min: literal(&q.validate.min),
            max: literal(&q.validate.max),
            loop_min,
            loop_max,
            options,
            regex: q.validate.regex.as_ref(),
        }
    }

    fn ready(
        q: &'q Question,
        answers: &Answers,
        template: &Template,
        seed: &InterviewSeed,
    ) -> Result<Self, EvalError> {
        let ctx = context(template, answers, seed);
        let number = |field: &'static str,
                      value: &'q Option<Typed<u32>>|
         -> Result<Option<u32>, EvalError> {
            value
                .as_ref()
                .filter(|value| typed_ready(value, answers, template))
                .map(|value| eval_typed(value, &ctx, &q.id, field))
                .transpose()
        };
        let required = typed_ready(&q.required, answers, template)
            .then(|| eval_typed(&q.required, &ctx, &q.id, "required"))
            .transpose()?;
        let (options, loop_min, loop_max) = match &q.kind {
            QuestionKind::Select { options, .. } | QuestionKind::MultiSelect { options, .. } => {
                let options = typed_ready(options, answers, template)
                    .then(|| eval_typed(options, &ctx, &q.id, "options"))
                    .transpose()?;
                (options, None, None)
            }
            QuestionKind::TextLoop { min, max, .. } => {
                (None, number("loop.min", min)?, number("loop.max", max)?)
            }
            _ => (None, None, None),
        };
        Ok(Self {
            kind: prompt_kind(q),
            required,
            min: number("validate.min", &q.validate.min)?,
            max: number("validate.max", &q.validate.max)?,
            loop_min,
            loop_max,
            options,
            regex: q.validate.regex.as_ref(),
        })
    }
}

fn skipped_default(
    q: &Question,
    prepared: Option<PreparedDefault>,
    answers: &Answers,
    template: &Template,
    seed: &InterviewSeed,
) -> Result<Option<Answer>, EvalError> {
    let Some(prepared) = prepared else {
        return Ok(None);
    };
    if let PreparedDefaultSource::Template { expression } = &prepared.source {
        if let Err(rejected) = validate(
            &q.id,
            &Rules::ready(q, answers, template, seed)?,
            prepared.answer.to_json(),
        ) {
            return Err(EvalError {
                id: q.id.clone(),
                field: "default",
                message: format!(
                    "default {} is not allowed: {}",
                    prepared.answer.to_json(),
                    rejected.message
                ),
                expression: expression.clone(),
                config_key: None,
            });
        }
    }
    Ok(Some(prepared.answer))
}
fn prompt_kind(q: &Question) -> PromptKind {
    match q.kind {
        QuestionKind::Text { .. } => PromptKind::Text,
        QuestionKind::Multiline { .. } => PromptKind::Multiline,
        QuestionKind::Confirm { .. } => PromptKind::Confirm,
        QuestionKind::Select { .. } => PromptKind::Select,
        QuestionKind::MultiSelect { .. } => PromptKind::MultiSelect,
        QuestionKind::TextLoop { .. } => PromptKind::TextLoop,
    }
}
/// Checks `value` against `rules`. Each error is one sentence that names the
/// constraint it fails.
fn validate(id: &Id, rules: &Rules, value: Value) -> Result<Answer, Rejection> {
    let fail = |msg: String| Err(rejection(id, msg));
    if value.is_null() {
        if rules.required == Some(true) {
            return fail("is required".into());
        }
        return Ok(empty_answer(rules.kind));
    }
    let answer = parse_kind(id, rules.kind, value)?;
    if rules.required == Some(true)
        && match &answer {
            Answer::Text(v) => v.is_empty(),
            Answer::List(v) => v.is_empty(),
            _ => false,
        }
    {
        return fail("is required".into());
    }
    let (strings, prefix): (Vec<&str>, &str) = match &answer {
        Answer::Text(v) => (vec![v], ""),
        Answer::List(v) if rules.kind == PromptKind::TextLoop => {
            (v.iter().map(String::as_str).collect(), "each item ")
        }
        _ => (vec![], ""),
    };
    for s in strings {
        let len = s.chars().count() as u32;
        if let Some(n) = rules.min.filter(|n| len < *n) {
            return fail(format!(
                "{prefix}must be at least {}",
                count(n, "character", "characters")
            ));
        }
        if let Some(n) = rules.max.filter(|n| len > *n) {
            return fail(format!(
                "{prefix}must be at most {}",
                count(n, "character", "characters")
            ));
        }
        if let Some(r) = rules.regex.filter(|r| !r.is_match(s)) {
            return fail(format!("{prefix}must match {}", r.as_str()));
        }
    }
    let (at_least, at_most) = match rules.kind {
        PromptKind::MultiSelect => (rules.min, rules.max),
        _ => (rules.loop_min, rules.loop_max),
    };
    if let Answer::List(v) = &answer {
        let len = v.len() as u32;
        if let Some(n) = at_least.filter(|n| len < *n) {
            return fail(format!("must have at least {}", count(n, "item", "items")));
        }
        if let Some(n) = at_most.filter(|n| len > *n) {
            return fail(format!("must have at most {}", count(n, "item", "items")));
        }
        if rules.kind == PromptKind::MultiSelect {
            if let Some(options) = &rules.options {
                if v.iter().any(|s| !options.contains(s)) {
                    return fail(format!("each item must be one of: {}", options.join(", ")));
                }
            }
            let mut seen = HashSet::new();
            if !v.iter().all(|s| seen.insert(s)) {
                return fail("must not repeat an item".into());
            }
        }
    }
    if let (Answer::Text(v), PromptKind::Select, Some(options)) =
        (&answer, rules.kind, &rules.options)
    {
        if !options.contains(v) {
            return fail(format!("must be one of: {}", options.join(", ")));
        }
    }
    Ok(answer)
}
#[derive(Debug)]
pub struct Resolution {
    defaults: IndexMap<Id, ResolvedDefault>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedDefault {
    raw: RawAnswer,
    origin: ConfiguredDefaultOrigin,
}

#[derive(Debug, Clone)]
struct ConfiguredDefaultOrigin {
    mapping: MappingSite,
    preset: Option<PresetSite>,
}

#[derive(Debug, Clone)]
struct MappingSite {
    origin: crate::config::ConfigOrigin,
    formal_name: String,
    id: Id,
}

#[derive(Debug, Clone)]
struct PresetSite {
    origin: crate::config::ConfigOrigin,
    name: PresetName,
    value: Value,
}

impl ConfiguredDefaultOrigin {
    fn description(&self) -> String {
        let identity =
            serde_json::to_string(&self.mapping.formal_name).expect("string is a JSON value");
        let mapping = format!(
            "{}: template-defaults.{}.{id}",
            self.mapping.origin.path.display(),
            identity,
            id = self.mapping.id
        );
        match &self.preset {
            Some(preset) => format!(
                "{mapping} → {}: presets.\"{}\" ({})",
                preset.origin.path.display(),
                preset.name,
                preset.value
            ),
            None => mapping,
        }
    }
}

impl Resolution {
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The existing consuming producer entry: moves each origin-bearing
    /// `ResolvedDefault` into the configured default bank and starts with the
    /// pre-context legacy projection (no reserved names). Its context-aware
    /// sibling is [`Self::start_with_context`].
    pub fn start<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
    ) -> Result<Interview<'a>, EvalError> {
        Interview::start_with_bank(
            template,
            InterviewSeed {
                now,
                defaults: self
                    .defaults
                    .into_iter()
                    .map(|(id, value)| (id, DefaultBankEntry::Configured(value)))
                    .collect(),
                context: None,
            },
        )
    }

    /// Consumes the origin-bearing configured defaults and the supplied
    /// invocation context, moving each `ResolvedDefault` into the configured
    /// default bank. This is the configured sibling of [`Interview::start`]; it
    /// never flattens provenance through `Seed` or `into_flat_defaults`.
    pub fn start_with_context<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
    ) -> Result<Interview<'a>, EvalError> {
        Interview::start_with_bank(
            template,
            InterviewSeed {
                now,
                defaults: self
                    .defaults
                    .into_iter()
                    .map(|(id, value)| (id, DefaultBankEntry::Configured(value)))
                    .collect(),
                context: Some(context),
            },
        )
    }

    /// Start an interview with snapshot-sourced defaults layered over the
    /// configured defaults (the generate axis, `--like`). For each question the
    /// template defines, precedence resolves at this seam, before the bank is
    /// built, so the single-occupant invariant holds:
    ///
    ///   1. a snapshot value that parses to the question's kind → a `Snapshot`
    ///      entry, shadowing any configured entry for that id;
    ///   2. else the configured default (this `Resolution`) → a `Configured` entry;
    ///   3. else the template's own `default` expression (no bank entry);
    ///   4. else the question is asked.
    ///
    /// A snapshot value whose kind no longer matches the question is dropped and
    /// returned as a warning; it falls through to (2)/(3) and is never a hard
    /// error and never forces an invalid value. A snapshot value for an id the
    /// template no longer defines is silently ignored (ordinary template
    /// evolution). With `seed == None` this is byte-for-byte `start_with_context`,
    /// so the no-`--like` path and every driver's parity are unchanged.
    pub fn start_with_seed<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
        seed: Option<SnapshotSeed>,
    ) -> Result<(Interview<'a>, Vec<String>), EvalError> {
        let Some(seed) = seed else {
            // None ≡ start_with_context: the configured defaults enter the bank
            // exactly as they do without a seed, and no warning is produced.
            return self
                .start_with_context(template, now, context)
                .map(|interview| (interview, Vec::new()));
        };
        let mut defaults: IndexMap<Id, DefaultBankEntry> = self
            .defaults
            .into_iter()
            .map(|(id, value)| (id, DefaultBankEntry::Configured(value)))
            .collect();
        let mut warnings = Vec::new();
        for (id, raw) in seed.defaults {
            // A snapshot value for an id the template no longer defines is
            // ignored: templates evolve, and a seed is optional.
            let Some(question) = question_by_id(&template.interview, &id) else {
                continue;
            };
            // Only a kind-matching value becomes a default; a wrong-kind value is
            // dropped with a warning and the id falls back to its configured or
            // template default. The bank keeps one occupant per id: a Snapshot
            // entry replaces the configured entry for that id in place.
            if parse_kind(&id, prompt_kind(question), raw.0.clone()).is_ok() {
                defaults.insert(
                    id,
                    DefaultBankEntry::Snapshot {
                        raw,
                        from: seed.from.clone(),
                    },
                );
            } else {
                warnings.push(format!(
                    "warning: snapshot {} default for \"{id}\" does not match the question's type; ignored",
                    seed.from
                ));
            }
        }
        Interview::start_with_bank(
            template,
            InterviewSeed {
                now,
                defaults,
                context: Some(context),
            },
        )
        .map(|interview| (interview, warnings))
    }

    pub fn into_flat_defaults(self) -> (IndexMap<Id, RawAnswer>, Vec<String>) {
        (
            self.defaults
                .into_iter()
                .map(|(id, value)| (id, value.raw))
                .collect(),
            self.warnings,
        )
    }
}

pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &IndexMap<PresetName, ConfigEntry<Value>>,
    mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
) -> Result<Resolution, EvalError> {
    let mut defaults = IndexMap::new();
    let mut warnings = Vec::new();
    let Some(values) = mappings.get(formal_name) else {
        return Ok(Resolution { defaults, warnings });
    };
    for (id, entry) in values {
        let identity = serde_json::to_string(formal_name).expect("string is a JSON value");
        let mapping = MappingSite {
            origin: entry.origin.clone(),
            formal_name: formal_name.into(),
            id: id.clone(),
        };
        let mapping_key = format!(
            "{}: template-defaults.{}.{id}",
            mapping.origin.path.display(),
            identity
        );
        let Some(question) = question_by_id(&template.interview, id) else {
            warnings.push(format!(
                "{mapping_key}: question is not defined by the selected template; ignored"
            ));
            continue;
        };
        let (value, preset) = match &entry.value {
            DefaultSource::Literal(value) => (value, None),
            DefaultSource::Ref(name) => {
                let Some(preset) = presets.get(name) else {
                    return Err(EvalError {
                        id: id.clone(),
                        field: CONFIGURED_DEFAULT,
                        message: format!("no preset named \"{name}\""),
                        expression: None,
                        config_key: Some(mapping_key),
                    });
                };
                (
                    &preset.value,
                    Some(PresetSite {
                        origin: preset.origin.clone(),
                        name: name.clone(),
                        value: preset.value.clone(),
                    }),
                )
            }
        };
        let origin = ConfiguredDefaultOrigin { mapping, preset };
        parse_kind(id, prompt_kind(question), value.clone()).map_err(|error| EvalError {
            id: id.clone(),
            field: CONFIGURED_DEFAULT,
            message: error.message,
            expression: None,
            config_key: Some(origin.description()),
        })?;
        defaults.insert(
            id.clone(),
            ResolvedDefault {
                raw: RawAnswer(value.clone()),
                origin,
            },
        );
    }
    Ok(Resolution { defaults, warnings })
}
fn question_by_id<'a>(nodes: &'a [Node], id: &Id) -> Option<&'a Question> {
    for n in nodes {
        match n {
            Node::Question(q) if &q.id == id => return Some(q),
            Node::Group(g) => {
                if let Some(q) = question_by_id(&g.nodes, id) {
                    return Some(q);
                }
            }
            _ => {}
        }
    }
    None
}
fn skipped_descendants_ready(
    nodes: &[Node],
    answers: &Answers,
    template: &Template,
    seed: &InterviewSeed,
) -> bool {
    let mut available = answers.clone();
    fn visit(
        nodes: &[Node],
        available: &mut Answers,
        template: &Template,
        seed: &InterviewSeed,
    ) -> bool {
        for node in nodes {
            match node {
                Node::Question(q) => {
                    if !default_ready(q, available, template, seed) {
                        return false;
                    }
                    available.insert(q.id.clone(), Answer::None);
                }
                Node::Computed(c) => {
                    available.insert(c.id.clone(), Answer::None);
                }
                Node::Group(g) => {
                    if !visit(&g.nodes, available, template, seed) {
                        return false;
                    }
                }
                Node::Message(_) | Node::Hook(_) | Node::Flow(_) => {}
            }
        }
        true
    }
    visit(nodes, &mut available, template, seed)
}
/// Validates `raw` against `prompt` and applies the question's `format`.
fn check_answer(
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
    prompt: &Prompt,
    raw: RawAnswer,
) -> Result<Answer, CheckError> {
    let id = &prompt.id;
    let q = question_by_id(&template.interview, id).expect("prompt has question");
    let empty = raw.0.is_null();
    let answer = validate(id, &Rules::of_prompt(prompt, q), raw.0)?;
    if empty {
        return Ok(answer);
    }
    format_answer(template, answers, seed, q, prompt.kind, answer).map_err(CheckError::Eval)
}
/// Applies the `format` expression of `q` to a non-empty `answer`.
fn format_answer(
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
    q: &Question,
    kind: PromptKind,
    answer: Answer,
) -> Result<Answer, EvalError> {
    let id = &q.id;
    let Some(expr) = &q.format else {
        return Ok(answer);
    };
    let ctx = context(template, answers, seed);
    let fail = |e: &dyn fmt::Display| fault(id, "format", Some(expr.source()), e);
    let apply = |value: Value| -> Result<Value, EvalError> {
        let mut c = ctx.clone();
        c.insert("value".into(), value);
        expr.eval(c)
            .and_then(|v| {
                serde_json::to_value(v).map_err(|e| {
                    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                })
            })
            .map_err(|e| fail(&e))
    };
    Ok(match answer {
        Answer::Text(v) => {
            Answer::Text(serde_json::from_value(apply(Value::String(v))?).map_err(|e| fail(&e))?)
        }
        Answer::Bool(v) => {
            Answer::Bool(serde_json::from_value(apply(Value::Bool(v))?).map_err(|e| fail(&e))?)
        }
        Answer::List(v) if kind == PromptKind::MultiSelect => Answer::List(
            serde_json::from_value(apply(serde_json::json!(v))?).map_err(|e| fail(&e))?,
        ),
        Answer::List(v) => Answer::List(
            v.into_iter()
                .map(|s| serde_json::from_value(apply(Value::String(s))?).map_err(|e| fail(&e)))
                .collect::<Result<Vec<_>, EvalError>>()?,
        ),
        a => a,
    })
}
/// Whether `raw`, parsed and formatted as the engine records an answer to
/// `q`, equals `recorded`. `null` records the empty answer without format.
/// A value of the wrong type differs; a fault in `format` is an error.
fn same_answer(
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
    q: &Question,
    raw: &Value,
    recorded: &Answer,
) -> Result<bool, EvalError> {
    let kind = prompt_kind(q);
    if raw.is_null() {
        return Ok(empty_answer(kind) == *recorded);
    }
    let Ok(answer) = parse_kind(&q.id, kind, raw.clone()) else {
        return Ok(false);
    };
    Ok(format_answer(template, answers, seed, q, kind, answer)? == *recorded)
}
/// A template expression of a node: its field, source, and the ids it
/// references.
type Expression<'n> = (&'static str, &'n str, HashSet<String>);
/// The id that names `node` in errors, and its template expressions.
fn expressions(node: &Node) -> (Id, Vec<Expression<'_>>) {
    use crate::template::HookProgram;
    fn tmpl<'n>(field: &'static str, v: &'n crate::jinja::Tmpl) -> Expression<'n> {
        (field, v.source(), v.references().clone())
    }
    fn expr<'n>(field: &'static str, v: &'n Expr) -> Expression<'n> {
        (field, v.source(), v.references().clone())
    }
    fn typed<'n, T: Clone + serde::de::DeserializeOwned>(
        field: &'static str,
        v: &'n Typed<T>,
    ) -> Option<Expression<'n>> {
        v.source().map(|source| (field, source, v.references()))
    }
    let mut out = Vec::new();
    let id = match node {
        Node::Question(q) => {
            out.push(tmpl("prompt", &q.prompt));
            out.extend(q.description.as_ref().map(|v| tmpl("description", v)));
            out.extend(q.placeholder.as_ref().map(|v| tmpl("placeholder", v)));
            out.extend(typed("required", &q.required));
            out.extend(q.when.as_ref().map(|v| expr("when", v)));
            out.extend(
                q.validate
                    .min
                    .as_ref()
                    .and_then(|v| typed("validate.min", v)),
            );
            out.extend(
                q.validate
                    .max
                    .as_ref()
                    .and_then(|v| typed("validate.max", v)),
            );
            if let Some(v) = &q.format {
                let (field, source, mut refs) = expr("format", v);
                refs.remove("value");
                out.push((field, source, refs));
            }
            match &q.kind {
                QuestionKind::Text { default } | QuestionKind::Multiline { default } => {
                    out.extend(default.as_ref().map(|v| tmpl("default", v)));
                }
                QuestionKind::Confirm { default } => {
                    out.extend(default.as_ref().and_then(|v| typed("default", v)));
                }
                QuestionKind::Select { options, default } => {
                    out.extend(typed("options", options));
                    out.extend(default.as_ref().map(|v| tmpl("default", v)));
                }
                QuestionKind::MultiSelect { options, default } => {
                    out.extend(typed("options", options));
                    out.extend(default.as_ref().and_then(|v| typed("default", v)));
                }
                QuestionKind::TextLoop { default, min, max } => {
                    out.extend(default.as_ref().and_then(|v| typed("default", v)));
                    out.extend(min.as_ref().and_then(|v| typed("loop.min", v)));
                    out.extend(max.as_ref().and_then(|v| typed("loop.max", v)));
                }
            }
            q.id.clone()
        }
        Node::Computed(c) => {
            out.extend(c.when.as_ref().map(|v| expr("when", v)));
            out.push(expr("computed", &c.expr));
            c.id.clone()
        }
        Node::Group(g) => {
            out.extend(g.when.as_ref().map(|v| expr("when", v)));
            g.name.clone()
        }
        Node::Hook(h) => {
            out.extend(h.when.as_ref().map(|v| expr("when", v)));
            out.extend(h.each.as_ref().map(|e| expr("each", &e.expr)));
            let binding = h.each.as_ref().map(|e| e.binding.as_str());
            let mut command = match &h.command.program {
                HookProgram::Run(v) => v.iter().map(|v| tmpl("run", v)).collect::<Vec<_>>(),
                HookProgram::Script { args, .. } => args.iter().map(|v| tmpl("args", v)).collect(),
            };
            command.extend(h.command.cwd.as_ref().map(|v| tmpl("cwd", v)));
            for (field, source, mut refs) in command {
                if let Some(binding) = binding {
                    refs.remove(binding);
                }
                out.push((field, source, refs));
            }
            Id::parse("hook").unwrap()
        }
        Node::Message(m) => {
            out.extend(m.when.as_ref().map(|v| expr("when", v)));
            out.push(tmpl("message", &m.text));
            Id::parse("message").unwrap()
        }
        Node::Flow(f) => {
            out.extend(f.when.as_ref().map(|v| expr("when", v)));
            Id::parse("flow").unwrap()
        }
    };
    (id, out)
}
/// The fault behind unresolved question dependencies: the first expression,
/// in `nodes` and their descendants, that references an id with no answer.
fn unresolved(nodes: &[Node], available: &mut HashSet<String>, t: &Template) -> Option<EvalError> {
    for node in nodes {
        let (id, fields) = expressions(node);
        for (field, source, refs) in fields {
            let mut missing: Vec<_> = refs
                .iter()
                .filter(|r| {
                    !available.contains(*r)
                        && !t.data.keys().any(|d| d.as_str() == r.as_str())
                        && !is_global(r)
                        && !(t.reserves_context() && is_reserved(r))
                })
                .collect();
            missing.sort();
            if let Some(name) = missing.first() {
                return Some(fault(
                    &id,
                    field,
                    Some(source),
                    format!("{name} has no answer when this node is reached"),
                ));
            }
        }
        if let Node::Group(g) = node {
            if let Some(error) = unresolved(&g.nodes, available, t) {
                return Some(error);
            }
        }
        if let Node::Question(Question { id, .. })
        | Node::Computed(crate::template::Computed { id, .. }) = node
        {
            available.insert(id.to_string());
        }
    }
    None
}
/// The outcome of one frame of the walk. Replaces the earlier `bool`, which
/// was `true` for [`Walk::Complete`] and `false` for [`Walk::Blocked`].
enum Walk {
    /// The frame walked to its end.
    Complete,
    /// The walk stopped at a node that waits for an answer.
    Blocked,
    /// A flow `stop`/`abort` ended the interview.
    Ended(EndKind),
    /// A flow `{ skip: rest }` fired; the frame and every ancestor skip the
    /// rest of their nodes.
    SkipRest,
}
struct Advance<'a> {
    template: &'a Template,
    seed: InterviewSeed,
    answers: Answers,
    accepted_raw: IndexMap<Id, RawAnswer>,
    held: RawAnswers,
    skipped: Skipped,
    messages: Vec<String>,
    /// The number of messages reached before this step.
    step_start: usize,
    hooks: Vec<RenderedHook>,
    visited: HashSet<String>,
    batch: Batch,
    /// The node that stopped the walk before its references had answers.
    blocked: Option<&'a Node>,
    /// Raised by a fired flow `dry-run`; re-derived each walk.
    disposition: Disposition,
    /// Set by a fired flow `stop`/`abort`, with its label.
    ended: Option<(EndKind, Option<String>)>,
    /// A flow `{ skip: rest }` fired; propagates to every frame.
    skip_rest: bool,
}
impl<'a> Advance<'a> {
    /// Stops the walk at `node`, which waits for an answer.
    fn block(&mut self, node: &'a Node) -> Result<Walk, EvalError> {
        self.blocked = Some(node);
        Ok(Walk::Blocked)
    }
    /// Reaches `message` in interview order.
    fn message(&mut self, message: String) {
        self.batch.items.push(Item::Message(message.clone()));
        self.messages.push(message);
    }
    /// Records the question `q` as skipped with `default`. An answer held for
    /// it is not used.
    fn skip(&mut self, q: &Question, default: Option<Answer>) {
        self.answers.insert(
            q.id.clone(),
            default.unwrap_or_else(|| empty_answer(prompt_kind(q))),
        );
        self.skipped.insert(q.id.clone(), self.messages.len());
        self.warn_if_held(&q.id);
    }
    /// Reaches the warning for an answer held for the skipped question `id`,
    /// which is not used.
    fn warn_if_held(&mut self, id: &Id) {
        if self.held.shift_remove(id).is_some() {
            self.message(skipped_warning(id));
        }
    }
    fn walk(
        &mut self,
        nodes: &'a [Node],
        prefix: &str,
        ancestor_skip: bool,
    ) -> Result<Walk, EvalError> {
        // A flow `{ skip: group }` raises `skip` for the rest of this frame; a
        // `{ skip: rest }` raises `self.skip_rest`, which forces skip here and
        // in every ancestor frame on return.
        let mut skip = ancestor_skip;
        for (i, node) in nodes.iter().enumerate() {
            if self.skip_rest {
                skip = true;
            }
            let key = format!("{prefix}/{i}");
            match node {
                Node::Question(q) => {
                    if self.answers.contains_key(&q.id) {
                        if self.skipped.contains_key(&q.id) {
                            self.warn_if_held(&q.id);
                        }
                        continue;
                    }
                    if skip {
                        if !default_ready(q, &self.answers, self.template, &self.seed) {
                            return self.block(node);
                        }
                        let default = skipped_default(
                            q,
                            render_default(q, self.template, &self.answers, &self.seed)?,
                            &self.answers,
                            self.template,
                            &self.seed,
                        )?;
                        self.skip(q, default);
                        continue;
                    }
                    if !question_ready(q, &self.answers, self.template, &self.seed) {
                        return self.block(node);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && q.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| fault(&q.id, "when", Some(w.source()), e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    if active {
                        let prepared = make_prompt(q, self.template, &self.answers, &self.seed)?;
                        let prompt = prepared.prompt;
                        if let Some(raw) = self.held.shift_remove(&q.id) {
                            match check_answer(
                                self.template,
                                &self.answers,
                                &self.seed,
                                &prompt,
                                raw,
                            ) {
                                Ok(answer) => {
                                    self.answers.insert(q.id.clone(), answer);
                                    continue;
                                }
                                Err(CheckError::Rejected(r)) => {
                                    self.batch.errors.push(rejection(
                                        &q.id,
                                        format!(
                                            "value recorded earlier is not allowed: {}",
                                            r.message
                                        ),
                                    ));
                                }
                                Err(CheckError::Eval(e)) => return Err(e),
                            }
                        }
                        if let Some(error) = prepared.configured_rejection {
                            self.batch.errors.push(error);
                        }
                        if let Some(source) = prepared.default_source {
                            self.batch.default_sources.insert(q.id.clone(), source);
                        }
                        self.batch.items.push(Item::Prompt(prompt));
                    } else {
                        if !default_ready(q, &self.answers, self.template, &self.seed) {
                            return self.block(node);
                        }
                        let default = skipped_default(
                            q,
                            render_default(q, self.template, &self.answers, &self.seed)?,
                            &self.answers,
                            self.template,
                            &self.seed,
                        )?;
                        self.skip(q, default);
                    }
                }
                Node::Computed(c) => {
                    if self.answers.contains_key(&c.id) {
                        continue;
                    }
                    if !skip
                        && (!expr_ready(&c.expr, &self.answers, self.template)
                            || c.when
                                .as_ref()
                                .is_some_and(|w| !expr_ready(w, &self.answers, self.template)))
                    {
                        return self.block(node);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && c.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| fault(&c.id, "when", Some(w.source()), e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    let value = if active {
                        let value = c
                            .expr
                            .eval(&ctx)
                            .map_err(|e| fault(&c.id, "computed", Some(c.expr.source()), e))?;
                        serde_json::to_value(value)
                            .map(Answer::Value)
                            .map_err(|e| fault(&c.id, "computed", Some(c.expr.source()), e))?
                    } else {
                        Answer::None
                    };
                    self.answers.insert(c.id.clone(), value);
                }
                Node::Group(g) => {
                    if !skip
                        && g.when
                            .as_ref()
                            .is_some_and(|w| !expr_ready(w, &self.answers, self.template))
                    {
                        return self.block(node);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && g.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| fault(&g.name, "when", Some(w.source()), e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    if !active
                        && !skipped_descendants_ready(
                            &g.nodes,
                            &self.answers,
                            self.template,
                            &self.seed,
                        )
                    {
                        return self.block(node);
                    }
                    match self.walk(&g.nodes, &key, !active)? {
                        Walk::Blocked => return Ok(Walk::Blocked),
                        Walk::Ended(kind) => return Ok(Walk::Ended(kind)),
                        // A `{ skip: rest }` inside the group is carried on
                        // `self.skip_rest`, which this frame observes at the top
                        // of the next iteration.
                        Walk::Complete | Walk::SkipRest => {}
                    }
                }
                Node::Hook(h) => {
                    if self.visited.contains(&key) {
                        continue;
                    }
                    if !skip
                        && (h
                            .when
                            .as_ref()
                            .is_some_and(|w| !expr_ready(w, &self.answers, self.template))
                            || !hook_ready(h, &self.answers, self.template))
                    {
                        return self.block(node);
                    }
                    self.visited.insert(key);
                    if skip {
                        continue;
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = h
                        .when
                        .as_ref()
                        .map(|w| {
                            w.eval(&ctx).map(|v| v.is_true()).map_err(|e| {
                                fault(&Id::parse("hook").unwrap(), "when", Some(w.source()), e)
                            })
                        })
                        .transpose()?
                        .unwrap_or(true);
                    if active {
                        self.hooks.extend(render_hooks(h, &ctx).map_err(|f| {
                            fault(
                                &Id::parse("hook").unwrap(),
                                f.field,
                                Some(&f.source),
                                f.message,
                            )
                        })?);
                    }
                }
                Node::Message(m) => {
                    if self.visited.contains(&key) {
                        continue;
                    }
                    if !skip
                        && (!tmpl_ready(&m.text, &self.answers, self.template)
                            || m.when
                                .as_ref()
                                .is_some_and(|w| !expr_ready(w, &self.answers, self.template)))
                    {
                        return self.block(node);
                    }
                    self.visited.insert(key);
                    if skip {
                        continue;
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = m
                        .when
                        .as_ref()
                        .map(|w| {
                            w.eval(&ctx).map(|v| v.is_true()).map_err(|e| {
                                fault(&Id::parse("message").unwrap(), "when", Some(w.source()), e)
                            })
                        })
                        .transpose()?
                        .unwrap_or(true);
                    if active {
                        let text = m.text.render(&ctx).map_err(|e| {
                            fault(
                                &Id::parse("message").unwrap(),
                                "message",
                                Some(m.text.source()),
                                e,
                            )
                        })?;
                        if !text.trim().is_empty() {
                            self.message(text);
                        }
                    }
                }
                Node::Flow(f) => {
                    // Same-batch readiness: a flow whose `when` references a
                    // question still pending in this batch is not ready and
                    // blocks, so it fires only after that answer commits.
                    if !skip
                        && f.when
                            .as_ref()
                            .is_some_and(|w| !expr_ready(w, &self.answers, self.template))
                    {
                        return self.block(node);
                    }
                    // Inert under an ancestor skip or a skip raised earlier in
                    // this frame.
                    if skip {
                        continue;
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = f
                        .when
                        .as_ref()
                        .map(|w| {
                            w.eval(&ctx).map(|v| v.is_true()).map_err(|e| {
                                fault(&Id::parse("flow").unwrap(), "when", Some(w.source()), e)
                            })
                        })
                        .transpose()?
                        .unwrap_or(true);
                    if active {
                        match f.action {
                            FlowAction::Stop => {
                                self.ended = Some((EndKind::Stop, f.label.clone()));
                                return Ok(Walk::Ended(EndKind::Stop));
                            }
                            FlowAction::Abort => {
                                self.ended = Some((EndKind::Abort, f.label.clone()));
                                return Ok(Walk::Ended(EndKind::Abort));
                            }
                            // Idempotent raise: re-derived on every walk.
                            FlowAction::DryRun => self.disposition = Disposition::DryRun,
                            // Rest of the current group's siblings.
                            FlowAction::Skip(SkipScope::Group) => skip = true,
                            // Rest of the whole interview, climbing every frame.
                            FlowAction::Skip(SkipScope::Rest) => {
                                self.skip_rest = true;
                                skip = true;
                            }
                        }
                    }
                }
            }
        }
        Ok(if self.skip_rest {
            Walk::SkipRest
        } else {
            Walk::Complete
        })
    }
}
fn advance(mut state: Advance<'_>) -> Result<Interview<'_>, EvalError> {
    let template = state.template;
    let outcome = state.walk(&template.interview, "", false)?;
    if let Walk::Ended(kind) = outcome {
        let label = state.ended.take().and_then(|(_, label)| label);
        return Ok(Interview::Ended(Ended {
            kind,
            last_messages: state.messages[state.step_start..].to_vec(),
            messages: state.messages,
            label,
        }));
    }
    // A `{ skip: rest }` walked the whole interview in skip mode; both finish it.
    let complete = matches!(outcome, Walk::Complete | Walk::SkipRest);
    let has_prompt = state
        .batch
        .items
        .iter()
        .any(|i| matches!(i, Item::Prompt(_)));
    if !complete && !has_prompt {
        let mut available = state.answers.keys().map(Id::to_string).collect();
        return Err(state
            .blocked
            .and_then(|node| unresolved(std::slice::from_ref(node), &mut available, template))
            .unwrap_or_else(|| {
                eval_error(
                    &Id::parse("interview").unwrap(),
                    "batch",
                    "unresolved question dependencies",
                )
            }));
    }
    if complete && !has_prompt {
        Ok(Interview::Complete(Completed {
            answers: state.answers,
            accepted_raw: state.accepted_raw,
            last_messages: state.messages[state.step_start..].to_vec(),
            messages: state.messages,
            hooks: state.hooks,
            now: state.seed.now,
            disposition: state.disposition,
            context: state.seed.context,
            skipped: state.skipped,
            step_start: state.step_start,
        }))
    } else {
        Ok(Interview::Asking(Pending {
            template,
            seed: state.seed,
            answers: state.answers,
            accepted_raw: state.accepted_raw,
            held: state.held,
            skipped: state.skipped,
            messages: state.messages,
            hooks: state.hooks,
            visited: state.visited,
            batch: state.batch,
        }))
    }
}
/// Walks tentatively to classify a step's early-answer failures: which
/// questions it skips, and whether a flow `stop`/`abort` ends it (which drops
/// the early failures for every question the end prevents reaching).
fn probe_skips(mut state: Advance<'_>) -> (Skipped, bool) {
    let template = state.template;
    let ended = matches!(
        state.walk(&template.interview, "", false),
        Ok(Walk::Ended(_))
    );
    (state.skipped, ended)
}
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError> {
        Self::start_with_bank(
            template,
            InterviewSeed {
                now: seed.now,
                defaults: seed
                    .defaults
                    .into_iter()
                    .map(|(id, value)| (id, DefaultBankEntry::Seed(value)))
                    .collect(),
                context: Some(seed.context),
            },
        )
    }

    fn start_with_bank(template: &'a Template, seed: InterviewSeed) -> Result<Self, EvalError> {
        advance(Advance {
            template,
            seed,
            answers: Answers::new(),
            accepted_raw: IndexMap::new(),
            held: RawAnswers::new(),
            skipped: Skipped::new(),
            messages: vec![],
            step_start: 0,
            hooks: vec![],
            visited: HashSet::new(),
            batch: Batch::default(),
            blocked: None,
            disposition: Disposition::default(),
            ended: None,
            skip_rest: false,
        })
    }
}
fn rejection(id: &Id, message: impl Into<String>) -> Rejection {
    Rejection {
        id: id.clone(),
        message: message.into(),
        kind: RejectionKind::Invalid,
    }
}
impl Pending<'_> {
    pub fn batch(&self) -> &Batch {
        &self.batch
    }
    pub fn messages_reached(&self) -> usize {
        self.messages.len()
    }
    pub fn accepts_id(&self, id: &Id) -> bool {
        self.template.has_question_id(id)
    }
    fn check_inner(&self, id: &Id, raw: RawAnswer) -> Result<Answer, CheckError> {
        let prompt = self
            .batch
            .items
            .iter()
            .find_map(|i| match i {
                Item::Prompt(p) if &p.id == id => Some(p),
                _ => None,
            })
            .ok_or_else(|| rejection(id, "is not in the current batch"))?;
        check_answer(self.template, &self.answers, &self.seed, prompt, raw)
    }
    /// Whether an answer is held for a question that has not been reached.
    pub fn holds(&self, id: &Id) -> bool {
        self.held.contains_key(id)
    }
    pub fn check(&self, id: &Id, raw: RawAnswer) -> Result<Answer, CheckError> {
        self.check_inner(id, raw)
    }
}
#[derive(Debug)]
pub enum CheckError {
    Rejected(Rejection),
    Eval(EvalError),
}
impl From<Rejection> for CheckError {
    fn from(v: Rejection) -> Self {
        Self::Rejected(v)
    }
}
impl From<EvalError> for CheckError {
    fn from(v: EvalError) -> Self {
        Self::Eval(v)
    }
}
impl<'a> Pending<'a> {
    #[allow(clippy::result_large_err)]
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>> {
        let mut held = self.held.clone();
        let mut rejections = vec![];
        let mut unless_skipped = vec![];
        for (id, raw) in incoming {
            let Some(q) = question_by_id(&self.template.interview, &id) else {
                rejections.push(rejection(&id, "is not a question in this template"));
                continue;
            };
            if self.skipped.contains_key(&id) {
                // Not used; the walk warns at the skipped question.
                held.insert(id, raw);
                continue;
            }
            if let Some(recorded) = self.answers.get(&id) {
                let same = same_answer(
                    self.template,
                    &self.answers,
                    &self.seed,
                    q,
                    &raw.0,
                    recorded,
                )
                .map_err(AnswerError::Eval)?;
                if !same {
                    rejections.push(Rejection {
                        kind: RejectionKind::Answered,
                        ..rejection(
                            &id,
                            format!("is already answered with {}", recorded.to_json()),
                        )
                    });
                }
                continue;
            }
            let early = !self.answers.contains_key(&id)
                && !self
                    .batch
                    .items
                    .iter()
                    .any(|i| matches!(i, Item::Prompt(p) if p.id == id));
            if early {
                // What can be checked now is checked now; the rest is checked
                // when the question is reached.
                // A failure stands unless this document skips the question.
                if let Err(r) = validate(&id, &Rules::of_question(q), raw.0.clone()) {
                    rejections.push(r);
                    unless_skipped.push(id.clone());
                }
            }
            held.insert(id, raw);
        }
        let mut next = Answers::new();
        let mut raw_next: IndexMap<Id, RawAnswer> = IndexMap::new();
        for item in &self.batch.items {
            let Item::Prompt(p) = item else { continue };
            let raw = held.get(&p.id).cloned();
            let carried = self.batch.errors.iter().find(|e| e.id == p.id);
            // The submitted raw, captured before the match consumes it.
            let raw_captured: Option<Value> = raw.as_ref().map(|r| r.0.clone());
            let checked = match (raw, carried) {
                (Some(raw), _) => self.check_inner(&p.id, raw),
                // A value recorded earlier that failed stays an error until
                // a document answers its question.
                (None, Some(error)) => Err(error.clone().into()),
                (None, None) => match &p.default {
                    Some(v)
                        if matches!(
                            self.batch.default_sources.get(&p.id),
                            Some(PreparedDefaultSource::Seed)
                        ) =>
                    {
                        self.check_inner(&p.id, RawAnswer(v.to_json()))
                            .map_err(|e| match e {
                                CheckError::Rejected(r) => rejection(
                                    &p.id,
                                    format!(
                                        "default {} from {} is not allowed: {}",
                                        v.to_json(),
                                        configured_default_source(&p.id),
                                        r.message
                                    ),
                                )
                                .into(),
                                e => e,
                            })
                    }
                    // A snapshot-seeded default the new template rejects on a
                    // constraint re-asks (the rejection stands, exit 4); it is
                    // never written. The message names the snapshot it came from.
                    Some(v)
                        if matches!(
                            self.batch.default_sources.get(&p.id),
                            Some(PreparedDefaultSource::Snapshot { .. })
                        ) =>
                    {
                        let from = match self.batch.default_sources.get(&p.id) {
                            Some(PreparedDefaultSource::Snapshot { from }) => from.clone(),
                            _ => unreachable!("matched a snapshot default source"),
                        };
                        self.check_inner(&p.id, RawAnswer(v.to_json()))
                            .map_err(|e| match e {
                                CheckError::Rejected(r) => rejection(
                                    &p.id,
                                    format!(
                                        "default {} from snapshot {from} is not allowed: {}",
                                        v.to_json(),
                                        r.message
                                    ),
                                )
                                .into(),
                                e => e,
                            })
                    }
                    Some(v) => self.check_inner(&p.id, RawAnswer(v.to_json())),
                    None if !p.constraints.required => Ok(empty_answer(p.kind)),
                    None => Err(rejection(&p.id, "is required").into()),
                },
            };
            match checked {
                Ok(v) => {
                    let raw_value = raw_captured
                        .or_else(|| p.default.as_ref().map(|d| d.to_json()))
                        .unwrap_or_else(|| v.to_json());
                    raw_next.insert(p.id.clone(), RawAnswer(raw_value));
                    next.insert(p.id.clone(), v);
                }
                Err(CheckError::Rejected(r)) => rejections.push(r),
                Err(CheckError::Eval(e)) => return Err(AnswerError::Eval(e)),
            }
        }
        let rejected = |pending, rejections| {
            Err(AnswerError::Rejected {
                pending,
                rejections,
            })
        };
        if unless_skipped.is_empty() && !rejections.is_empty() {
            return rejected(self, rejections);
        }
        for id in next.keys() {
            held.shift_remove(id);
        }
        let mut answers = self.answers.clone();
        answers.extend(next);
        let mut accepted_raw = self.accepted_raw.clone();
        accepted_raw.extend(raw_next);
        let (probe_skipped, probe_ended) = probe_skips(Advance {
            template: self.template,
            seed: self.seed.clone(),
            answers: answers.clone(),
            accepted_raw: accepted_raw.clone(),
            held: held.clone(),
            skipped: self.skipped.clone(),
            step_start: self.messages.len(),
            messages: self.messages.clone(),
            hooks: self.hooks.clone(),
            visited: self.visited.clone(),
            batch: Batch::default(),
            blocked: None,
            disposition: Disposition::default(),
            ended: None,
            skip_rest: false,
        });
        // The error stands only when the step stops with the question active
        // or not reached. A complete interview reached and skipped it; a
        // template fault in the step is the error instead. A flow stop/abort
        // ends the step, so an early failure for a question the end prevents
        // reaching is dropped and the stop/abort stands.
        let stands = |id: &Id| !probe_ended && !probe_skipped.contains_key(id);
        let dropped: Vec<Id> = unless_skipped
            .into_iter()
            .filter(|id| !stands(id))
            .collect();
        rejections.retain(|r| !dropped.contains(&r.id));
        if !rejections.is_empty() {
            return rejected(self, rejections);
        }
        // The probe is never published. Commit one fresh walk so that its
        // messages, warnings, hooks, and template faults occur exactly once.
        advance(Advance {
            template: self.template,
            seed: self.seed,
            answers,
            accepted_raw,
            held,
            skipped: self.skipped,
            step_start: self.messages.len(),
            messages: self.messages,
            hooks: self.hooks,
            visited: self.visited,
            batch: Batch::default(),
            blocked: None,
            disposition: Disposition::default(),
            ended: None,
            skip_rest: false,
        })
        .map_err(AnswerError::Eval)
    }
}
