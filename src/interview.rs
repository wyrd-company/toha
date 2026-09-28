// ---
// relationships:
//   implements: architecture
// ---
use crate::{
    config::{ConfigEntry, DefaultSource, PresetName},
    jinja::{Expr, Typed, context_from_answers, is_global},
    template::{Id, Node, Question, QuestionKind, Template},
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
}
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
}
#[derive(Debug)]
pub struct Pending<'a> {
    template: &'a Template,
    seed: Seed,
    answers: Answers,
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
    /// Every message reached, in interview order.
    pub messages: Vec<String>,
    /// The messages reached by the step that completed the interview.
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    skipped: Skipped,
    step_start: usize,
}
/// The questions skipped by a `when`, in interview order, each with the
/// number of messages reached before it was skipped.
type Skipped = IndexMap<Id, usize>;
impl Completed {
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
    /// Errors for questions in this batch whose answer, held from an earlier
    /// document, failed validation when the question was reached.
    pub errors: Rejections,
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
        match &self.expression {
            Some(source) => write!(
                f,
                "template error in {}.{} `{}`: {}",
                self.id, self.field, source, self.message
            ),
            None => write!(f, "{}.{}: {}", self.id, self.field, self.message),
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
    seed: &Seed,
) -> std::collections::BTreeMap<String, Value> {
    context_from_answers(answers, &template.data, &seed.now)
}
fn has_refs(refs: &HashSet<String>, answers: &Answers, template: &Template) -> bool {
    refs.iter().all(|r| {
        template.data.keys().any(|id| id.as_str() == r)
            || answers.keys().any(|id| id.as_str() == r)
            || is_global(r)
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
fn question_ready(q: &Question, a: &Answers, t: &Template, seed: &Seed) -> bool {
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
fn default_ready(q: &Question, a: &Answers, t: &Template, seed: &Seed) -> bool {
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
    seed: &Seed,
) -> Result<Option<Answer>, EvalError> {
    let id = &q.id;
    let kind = prompt_kind(q);
    if let Some(raw) = seed.defaults.get(id) {
        return parse_kind(id, kind, raw.0.clone())
            .map(Some)
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
                    .map(Answer::Text)
                    .map_err(|e| fault(id, "default", Some(v.source()), e))
            })
            .transpose(),
        QuestionKind::Confirm { default } => default
            .as_ref()
            .map(|v| eval_typed(v, &ctx, id, "default").map(Answer::Bool))
            .transpose(),
        QuestionKind::MultiSelect { default, .. } | QuestionKind::TextLoop { default, .. } => {
            default
                .as_ref()
                .map(|v| eval_typed(v, &ctx, id, "default").map(Answer::List))
                .transpose()
        }
    }
}
fn make_prompt(q: &Question, t: &Template, a: &Answers, seed: &Seed) -> Result<Prompt, EvalError> {
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
    let default = render_default(q, t, a, seed)?;
    Ok(Prompt {
        id: id.clone(),
        kind,
        title,
        description,
        placeholder,
        default,
        options,
        constraints: Constraints {
            required,
            min,
            max,
            regex: q.validate.regex.as_ref().map(|r| r.as_str().into()),
            loop_min,
            loop_max,
        },
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
    pub defaults: IndexMap<Id, RawAnswer>,
    pub warnings: Vec<String>,
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
        let mapping_key = format!(
            "{}: template-defaults.\"{}\".{id}",
            entry.origin.path.display(),
            formal_name
        );
        let Some(question) = question_by_id(&template.interview, id) else {
            warnings.push(format!(
                "{mapping_key}: question is not defined by the selected template; ignored"
            ));
            continue;
        };
        let (value, config_key) = match &entry.value {
            DefaultSource::Literal(value) => (value, mapping_key),
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
                    format!(
                        "{mapping_key}\n→ {}: presets.\"{}\" ({})",
                        preset.origin.path.display(),
                        name,
                        preset.value
                    ),
                )
            }
        };
        parse_kind(id, prompt_kind(question), value.clone()).map_err(|error| EvalError {
            id: id.clone(),
            field: CONFIGURED_DEFAULT,
            message: error.message,
            expression: None,
            config_key: Some(config_key),
        })?;
        defaults.insert(id.clone(), RawAnswer(value.clone()));
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
    seed: &Seed,
) -> bool {
    let mut available = answers.clone();
    fn visit(nodes: &[Node], available: &mut Answers, template: &Template, seed: &Seed) -> bool {
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
                Node::Message(_) | Node::Hook(_) => {}
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
    seed: &Seed,
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
    seed: &Seed,
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
    seed: &Seed,
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
struct Advance<'a> {
    template: &'a Template,
    seed: Seed,
    answers: Answers,
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
}
impl<'a> Advance<'a> {
    /// Stops the walk at `node`, which waits for an answer.
    fn block(&mut self, node: &'a Node) -> Result<bool, EvalError> {
        self.blocked = Some(node);
        Ok(false)
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
    fn walk(&mut self, nodes: &'a [Node], prefix: &str, skip: bool) -> Result<bool, EvalError> {
        for (i, node) in nodes.iter().enumerate() {
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
                        let default = render_default(q, self.template, &self.answers, &self.seed)?;
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
                        let prompt = make_prompt(q, self.template, &self.answers, &self.seed)?;
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
                        self.batch.items.push(Item::Prompt(prompt));
                    } else {
                        if !default_ready(q, &self.answers, self.template, &self.seed) {
                            return self.block(node);
                        }
                        let default = render_default(q, self.template, &self.answers, &self.seed)?;
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
                    if !self.walk(&g.nodes, &key, !active)? {
                        return Ok(false);
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
            }
        }
        Ok(true)
    }
}
fn advance(mut state: Advance<'_>) -> Result<Interview<'_>, EvalError> {
    let template = state.template;
    let complete = state.walk(&template.interview, "", false)?;
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
            last_messages: state.messages[state.step_start..].to_vec(),
            messages: state.messages,
            hooks: state.hooks,
            now: state.seed.now,
            skipped: state.skipped,
            step_start: state.step_start,
        }))
    } else {
        Ok(Interview::Asking(Pending {
            template,
            seed: state.seed,
            answers: state.answers,
            held: state.held,
            skipped: state.skipped,
            messages: state.messages,
            hooks: state.hooks,
            visited: state.visited,
            batch: state.batch,
        }))
    }
}
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError> {
        advance(Advance {
            template,
            seed,
            answers: Answers::new(),
            held: RawAnswers::new(),
            skipped: Skipped::new(),
            messages: vec![],
            step_start: 0,
            hooks: vec![],
            visited: HashSet::new(),
            batch: Batch::default(),
            blocked: None,
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
        let answered_early = rejections.len();
        let mut next = Answers::new();
        for item in &self.batch.items {
            let Item::Prompt(p) = item else { continue };
            let raw = held.get(&p.id).cloned();
            let carried = self.batch.errors.iter().find(|e| e.id == p.id);
            let checked = match (raw, carried) {
                (Some(raw), _) => self.check_inner(&p.id, raw),
                // A value recorded earlier that failed stays an error until
                // a document answers its question.
                (None, Some(error)) => Err(error.clone().into()),
                (None, None) => match &p.default {
                    Some(v) if self.seed.defaults.contains_key(&p.id) => self
                        .check_inner(&p.id, RawAnswer(v.to_json()))
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
                        }),
                    Some(v) => self.check_inner(&p.id, RawAnswer(v.to_json())),
                    None if !p.constraints.required => Ok(empty_answer(p.kind)),
                    None => Err(rejection(&p.id, "is required").into()),
                },
            };
            match checked {
                Ok(v) => {
                    next.insert(p.id.clone(), v);
                }
                Err(CheckError::Rejected(r)) => rejections.push(r),
                Err(CheckError::Eval(e)) => return Err(AnswerError::Eval(e)),
            }
        }
        let other_failures = rejections.len() > unless_skipped.len();
        let batch_failed = rejections.len() > answered_early;
        let rejected = |pending, rejections| {
            Err(AnswerError::Rejected {
                pending,
                rejections,
            })
        };
        // An answer that fails a constraint before its question is reached
        // is classified from one tentative step with this document. A batch
        // that fails cannot take the step, so each such error stands.
        if batch_failed || (other_failures && unless_skipped.is_empty()) {
            return rejected(self, rejections);
        }
        for id in next.keys() {
            held.shift_remove(id);
        }
        let mut answers = self.answers.clone();
        answers.extend(next);
        let advanced = advance(Advance {
            template: self.template,
            seed: self.seed.clone(),
            answers,
            held,
            skipped: self.skipped.clone(),
            step_start: self.messages.len(),
            messages: self.messages.clone(),
            hooks: self.hooks.clone(),
            visited: self.visited.clone(),
            batch: Batch::default(),
            blocked: None,
        });
        // The error stands only when the step stops with the question
        // active or not reached. A complete interview reached and skipped
        // it; a template fault in the step is the error instead.
        let stands = |id: &Id| match &advanced {
            Ok(Interview::Asking(next)) => !next.skipped.contains_key(id),
            _ => false,
        };
        let dropped: Vec<Id> = unless_skipped
            .into_iter()
            .filter(|id| !stands(id))
            .collect();
        rejections.retain(|r| !dropped.contains(&r.id));
        match advanced {
            // A rejected document never takes the step, so its fault is
            // not reached.
            Err(_) if !rejections.is_empty() => rejected(self, rejections),
            Err(error) => Err(AnswerError::Eval(error)),
            Ok(_) if !rejections.is_empty() => rejected(self, rejections),
            Ok(next) => Ok(next),
        }
    }
}
