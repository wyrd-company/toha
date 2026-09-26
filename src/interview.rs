// ---
// relationships:
//   implements: architecture
// ---
use std::fmt;

use indexmap::IndexMap;

use crate::{
    jinja::context_from_answers,
    template::{Id, Node, QuestionKind, Template},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Text(String),
}
impl Answer {
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text(value) => value,
        }
    }
}
pub type Answers = IndexMap<Id, Answer>;
#[derive(Debug, Clone)]
pub struct RawAnswer(pub serde_json::Value);
pub type RawAnswers = IndexMap<Id, RawAnswer>;
#[derive(Debug, Default, Clone, Copy)]
pub struct Seed {}

#[derive(Debug)]
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
}
#[derive(Debug)]
pub struct Pending<'a> {
    template: &'a Template,
    answers: Answers,
    held: RawAnswers,
    batch: Batch,
}
#[derive(Debug)]
pub struct Completed {
    pub answers: Answers,
}
#[derive(Debug, Default)]
pub struct Batch {
    pub items: Vec<Item>,
}
#[derive(Debug)]
pub enum Item {
    Prompt(Prompt),
}
#[derive(Debug)]
pub struct Prompt {
    pub id: Id,
    pub kind: PromptKind,
    pub title: String,
    pub description: Option<String>,
    pub default: Option<String>,
    pub required: bool,
}
#[derive(Debug)]
pub enum PromptKind {
    Text,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub id: Id,
    pub message: String,
}
pub type Rejections = Vec<Rejection>;
impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.id, self.message)
    }
}

impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, _seed: Seed) -> Self {
        advance(template, Answers::new(), RawAnswers::new())
    }
}

fn advance<'a>(template: &'a Template, answers: Answers, held: RawAnswers) -> Interview<'a> {
    let mut batch = Batch::default();
    for node in &template.interview {
        let Node::Question(question) = node;
        if answers.contains_key(&question.id) {
            continue;
        }
        let refs_available = question
            .prompt
            .references()
            .iter()
            .all(|id| answers.keys().any(|key| key.as_str() == id))
            && question.description.as_ref().is_none_or(|tmpl| {
                tmpl.references()
                    .iter()
                    .all(|id| answers.keys().any(|key| key.as_str() == id))
            })
            && match &question.kind {
                QuestionKind::Text { default } => default.as_ref().is_none_or(|tmpl| {
                    tmpl.references()
                        .iter()
                        .all(|id| answers.keys().any(|key| key.as_str() == id))
                }),
            };
        if !refs_available {
            break;
        }
        let context = context_from_answers(&answers);
        let QuestionKind::Text { default } = &question.kind;
        batch.items.push(Item::Prompt(Prompt {
            id: question.id.clone(),
            kind: PromptKind::Text,
            title: question.prompt.render(&context).unwrap_or_default(),
            description: question
                .description
                .as_ref()
                .map(|tmpl| tmpl.render(&context).unwrap_or_default()),
            default: default
                .as_ref()
                .map(|tmpl| tmpl.render(&context).unwrap_or_default()),
            required: question.required,
        }));
    }
    if batch.items.is_empty() {
        Interview::Complete(Completed { answers })
    } else {
        Interview::Asking(Pending {
            template,
            answers,
            held,
            batch,
        })
    }
}

impl<'a> Pending<'a> {
    pub fn batch(&self) -> &Batch {
        &self.batch
    }

    pub fn check(&self, id: &Id, raw: RawAnswer) -> Result<Answer, Rejection> {
        let Some(prompt) = self
            .batch
            .items
            .iter()
            .map(|item| match item {
                Item::Prompt(prompt) => prompt,
            })
            .find(|prompt| &prompt.id == id)
        else {
            return Err(Rejection {
                id: id.clone(),
                message: "not in current batch".into(),
            });
        };
        match raw.0 {
            serde_json::Value::String(value) if !prompt.required || !value.is_empty() => {
                Ok(Answer::Text(value))
            }
            serde_json::Value::String(_) => Err(Rejection {
                id: id.clone(),
                message: "required answer is missing".into(),
            }),
            _ => Err(Rejection {
                id: id.clone(),
                message: "expected text answer".into(),
            }),
        }
    }

    // The state-machine contract returns the original Pending value on rejection.
    #[allow(clippy::result_large_err)]
    pub fn answer(
        mut self,
        incoming: RawAnswers,
    ) -> Result<Interview<'a>, (Pending<'a>, Rejections)> {
        let mut rejections = Vec::new();
        let mut held = self.held.clone();
        for (id, raw) in incoming {
            if !self
                .template
                .interview
                .iter()
                .any(|node| matches!(node, Node::Question(question) if question.id == id))
            {
                rejections.push(Rejection {
                    id,
                    message: "unknown answer id".into(),
                });
            } else {
                held.insert(id, raw);
            }
        }
        let mut next = Answers::new();
        for item in &self.batch.items {
            let Item::Prompt(prompt) = item;
            let raw = held.get(&prompt.id).cloned().unwrap_or_else(|| {
                RawAnswer(serde_json::Value::String(
                    prompt.default.clone().unwrap_or_default(),
                ))
            });
            match self.check(&prompt.id, raw) {
                Ok(answer) => {
                    next.insert(prompt.id.clone(), answer);
                }
                Err(rejection) => rejections.push(rejection),
            }
        }
        if !rejections.is_empty() {
            return Err((self, rejections));
        }
        self.answers.extend(next);
        for id in self.answers.keys() {
            held.shift_remove(id);
        }
        Ok(advance(self.template, self.answers, held))
    }
}
