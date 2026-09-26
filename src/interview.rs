// ---
// relationships:
//   implements: architecture
// ---
use crate::{
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
    messages: Vec<String>,
    visited: HashSet<String>,
    batch: Batch,
}
#[derive(Debug)]
pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub now: jiff::Zoned,
}
#[derive(Debug, Default)]
pub struct Batch {
    pub items: Vec<Item>,
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
}
pub type Rejections = Vec<Rejection>;
#[derive(Debug, thiserror::Error)]
#[error("{id}.{field}: {message}")]
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
}
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
    v.eval(ctx).map_err(|e| eval_error(id, field, e))
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
    let kind = match &q.kind {
        QuestionKind::Text { .. } => PromptKind::Text,
        QuestionKind::Multiline { .. } => PromptKind::Multiline,
        QuestionKind::Confirm { .. } => PromptKind::Confirm,
        QuestionKind::Select { .. } => PromptKind::Select,
        QuestionKind::MultiSelect { .. } => PromptKind::MultiSelect,
        QuestionKind::TextLoop { .. } => PromptKind::TextLoop,
    };
    if let Some(raw) = seed.defaults.get(id) {
        return parse_kind(id, kind, raw.0.clone())
            .map(Some)
            .map_err(|e| eval_error(id, "default", e.message));
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
                    .map_err(|e| eval_error(id, "default", e))
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
        .map_err(|e| eval_error(id, "prompt", e))?;
    let description = q
        .description
        .as_ref()
        .map(|v| v.render(&ctx).map_err(|e| eval_error(id, "description", e)))
        .transpose()?;
    let placeholder = q
        .placeholder
        .as_ref()
        .map(|v| v.render(&ctx).map_err(|e| eval_error(id, "placeholder", e)))
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
    let fail = |msg: &str| Rejection {
        id: id.clone(),
        message: msg.into(),
    };
    match kind {
        PromptKind::Text | PromptKind::Multiline | PromptKind::Select => value
            .as_str()
            .map(|v| Answer::Text(v.into()))
            .ok_or_else(|| fail("expected text answer")),
        PromptKind::Confirm => value
            .as_bool()
            .map(Answer::Bool)
            .ok_or_else(|| fail("expected boolean answer")),
        PromptKind::MultiSelect | PromptKind::TextLoop => value
            .as_array()
            .and_then(|v| {
                v.iter()
                    .map(|x| x.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .map(Answer::List)
            .ok_or_else(|| fail("expected array of strings")),
    }
}
fn nodes_have_id(nodes: &[Node], id: &Id) -> bool {
    nodes.iter().any(|n| match n {
        Node::Question(q) => &q.id == id,
        Node::Group(g) => nodes_have_id(&g.nodes, id),
        _ => false,
    })
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
struct Advance<'a> {
    template: &'a Template,
    seed: Seed,
    answers: Answers,
    held: RawAnswers,
    messages: Vec<String>,
    visited: HashSet<String>,
    batch: Batch,
}
impl Advance<'_> {
    fn walk(&mut self, nodes: &[Node], prefix: &str, skip: bool) -> Result<bool, EvalError> {
        for (i, node) in nodes.iter().enumerate() {
            let key = format!("{prefix}/{i}");
            match node {
                Node::Question(q) => {
                    if self.answers.contains_key(&q.id) {
                        continue;
                    }
                    if !skip
                        && q.when
                            .as_ref()
                            .is_some_and(|w| !expr_ready(w, &self.answers, self.template))
                    {
                        return Ok(false);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && q.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| eval_error(&q.id, "when", e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    if active {
                        if !question_ready(q, &self.answers, self.template, &self.seed) {
                            return Ok(false);
                        }
                        let prompt = make_prompt(q, self.template, &self.answers, &self.seed)?;
                        self.batch.items.push(Item::Prompt(prompt));
                    } else {
                        if !default_ready(q, &self.answers, self.template, &self.seed) {
                            return Ok(false);
                        }
                        let default = render_default(q, self.template, &self.answers, &self.seed)?;
                        self.answers
                            .insert(q.id.clone(), default.unwrap_or(Answer::None));
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
                        return Ok(false);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && c.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| eval_error(&c.id, "when", e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    let value = if active {
                        let value = c
                            .expr
                            .eval(&ctx)
                            .map_err(|e| eval_error(&c.id, "computed", e))?;
                        serde_json::to_value(value)
                            .map(Answer::Value)
                            .map_err(|e| eval_error(&c.id, "computed", e))?
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
                        return Ok(false);
                    }
                    let ctx = context(self.template, &self.answers, &self.seed);
                    let active = !skip
                        && g.when
                            .as_ref()
                            .map(|w| {
                                w.eval(&ctx)
                                    .map(|v| v.is_true())
                                    .map_err(|e| eval_error(&g.name, "when", e))
                            })
                            .transpose()?
                            .unwrap_or(true);
                    if !self.walk(&g.nodes, &key, !active)? {
                        return Ok(false);
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
                        return Ok(false);
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
                            w.eval(&ctx)
                                .map(|v| v.is_true())
                                .map_err(|e| eval_error(&Id::parse("message").unwrap(), "when", e))
                        })
                        .transpose()?
                        .unwrap_or(true);
                    if active {
                        let text = m.text.render(&ctx).map_err(|e| {
                            eval_error(&Id::parse("message").unwrap(), "message", e)
                        })?;
                        if !text.trim().is_empty() {
                            self.batch.items.push(Item::Message(text.clone()));
                            self.messages.push(text);
                        }
                    }
                }
            }
        }
        Ok(true)
    }
}
fn advance<'a>(
    template: &'a Template,
    seed: Seed,
    answers: Answers,
    held: RawAnswers,
    messages: Vec<String>,
    visited: HashSet<String>,
) -> Result<Interview<'a>, EvalError> {
    let mut state = Advance {
        template,
        seed,
        answers,
        held,
        messages,
        visited,
        batch: Batch::default(),
    };
    let complete = state.walk(&template.interview, "", false)?;
    let has_prompt = state
        .batch
        .items
        .iter()
        .any(|i| matches!(i, Item::Prompt(_)));
    if !complete && !has_prompt {
        return Err(eval_error(
            &Id::parse("interview").unwrap(),
            "batch",
            "unresolved question dependencies",
        ));
    }
    if complete && !has_prompt {
        Ok(Interview::Complete(Completed {
            answers: state.answers,
            messages: state.messages,
            now: state.seed.now,
        }))
    } else {
        Ok(Interview::Asking(Pending {
            template,
            seed: state.seed,
            answers: state.answers,
            held: state.held,
            messages: state.messages,
            visited: state.visited,
            batch: state.batch,
        }))
    }
}
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError> {
        advance(
            template,
            seed,
            Answers::new(),
            RawAnswers::new(),
            vec![],
            HashSet::new(),
        )
    }
}
fn rejection(id: &Id, message: impl Into<String>) -> Rejection {
    Rejection {
        id: id.clone(),
        message: message.into(),
    }
}
impl Pending<'_> {
    pub fn batch(&self) -> &Batch {
        &self.batch
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
            .ok_or_else(|| rejection(id, "not in current batch"))?;
        let q = question_by_id(&self.template.interview, id).expect("prompt has question");
        let mut answer = parse_kind(id, prompt.kind, raw.0)?;
        let c = &prompt.constraints;
        if c.required
            && match &answer {
                Answer::Text(v) => v.is_empty(),
                Answer::List(v) => v.is_empty(),
                _ => false,
            }
        {
            return Err(rejection(id, "required answer is missing").into());
        }
        let strings: Vec<&str> = match &answer {
            Answer::Text(v) => vec![v],
            Answer::List(v) if prompt.kind == PromptKind::TextLoop => {
                v.iter().map(String::as_str).collect()
            }
            _ => vec![],
        };
        for s in strings {
            let len = s.chars().count() as u32;
            if c.min.is_some_and(|v| len < v) {
                return Err(rejection(id, "validate.min").into());
            }
            if c.max.is_some_and(|v| len > v) {
                return Err(rejection(id, "validate.max").into());
            }
            if q.validate.regex.as_ref().is_some_and(|r| !r.is_match(s)) {
                return Err(rejection(id, "validate.regex").into());
            }
        }
        if let Answer::List(v) = &answer {
            if prompt.kind == PromptKind::MultiSelect {
                if c.min.is_some_and(|n| v.len() < n as usize) {
                    return Err(rejection(id, "validate.min").into());
                }
                if c.max.is_some_and(|n| v.len() > n as usize) {
                    return Err(rejection(id, "validate.max").into());
                }
                let mut seen = HashSet::new();
                if v.iter()
                    .any(|s| !prompt.options.contains(s) || !seen.insert(s))
                {
                    return Err(rejection(id, "select options must be unique and allowed").into());
                }
            } else {
                if c.loop_min.is_some_and(|n| v.len() < n as usize) {
                    return Err(rejection(id, "loop.min").into());
                }
                if c.loop_max.is_some_and(|n| v.len() > n as usize) {
                    return Err(rejection(id, "loop.max").into());
                }
            }
        }
        if let Answer::Text(v) = &answer {
            if prompt.kind == PromptKind::Select && !prompt.options.contains(v) {
                return Err(rejection(id, "select option not allowed").into());
            }
        }
        if let Some(expr) = &q.format {
            let ctx = context(self.template, &self.answers, &self.seed);
            let apply = |value: Value| -> Result<Value, CheckError> {
                let mut c = ctx.clone();
                c.insert("value".into(), value);
                expr.eval(c)
                    .and_then(|v| {
                        serde_json::to_value(v).map_err(|e| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                e.to_string(),
                            )
                        })
                    })
                    .map_err(|e| eval_error(id, "format", e).into())
            };
            answer = match answer {
                Answer::Text(v) => Answer::Text(
                    serde_json::from_value(apply(Value::String(v))?)
                        .map_err(|e| eval_error(id, "format", e))?,
                ),
                Answer::Bool(v) => Answer::Bool(
                    serde_json::from_value(apply(Value::Bool(v))?)
                        .map_err(|e| eval_error(id, "format", e))?,
                ),
                Answer::List(v) if prompt.kind == PromptKind::MultiSelect => Answer::List(
                    serde_json::from_value(apply(serde_json::json!(v))?)
                        .map_err(|e| eval_error(id, "format", e))?,
                ),
                Answer::List(v) => Answer::List(
                    v.into_iter()
                        .map(|s| {
                            serde_json::from_value(apply(Value::String(s))?)
                                .map_err(|e| eval_error(id, "format", e).into())
                        })
                        .collect::<Result<Vec<_>, CheckError>>()?,
                ),
                a => a,
            };
        }
        Ok(answer)
    }
    pub fn check(&self, id: &Id, raw: RawAnswer) -> Result<Answer, Rejection> {
        self.check_inner(id, raw).map_err(|e| match e {
            CheckError::Rejected(r) => r,
            CheckError::Eval(e) => rejection(id, e.to_string()),
        })
    }
}
enum CheckError {
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
        for (id, raw) in incoming {
            if !nodes_have_id(&self.template.interview, &id) {
                rejections.push(rejection(&id, "unknown answer id"))
            } else {
                held.insert(id, raw);
            }
        }
        let mut next = Answers::new();
        for item in &self.batch.items {
            let Item::Prompt(p) = item else { continue };
            let raw = held.get(&p.id).cloned();
            let checked = match raw {
                Some(raw) => self.check_inner(&p.id, raw),
                None => match &p.default {
                    Some(v) => self.check_inner(&p.id, RawAnswer(v.to_json())),
                    None if !p.constraints.required => Ok(Answer::None),
                    None => Err(rejection(&p.id, "required answer is missing").into()),
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
        if !rejections.is_empty() {
            return Err(AnswerError::Rejected {
                pending: self,
                rejections,
            });
        }
        let mut answers = self.answers.clone();
        answers.extend(next);
        for id in answers.keys() {
            held.shift_remove(id);
        }
        advance(
            self.template,
            self.seed,
            answers,
            held,
            self.messages,
            self.visited,
        )
        .map_err(AnswerError::Eval)
    }
}
