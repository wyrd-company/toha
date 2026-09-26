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
#[derive(Debug, thiserror::Error)]
#[error("{id}.{field}: {message}")]
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum AnswerError<'a> {
    Rejected {
        pending: Pending<'a>,
        rejections: Rejections,
    },
    Eval(EvalError),
}

impl EvalError {
    fn render(id: &Id, field: &'static str, error: minijinja::Error) -> Self {
        Self {
            id: id.clone(),
            field,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.id, self.message)
    }
}

impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, _seed: Seed) -> Result<Self, EvalError> {
        advance(template, Answers::new(), RawAnswers::new())
    }
}

fn advance<'a>(
    template: &'a Template,
    answers: Answers,
    held: RawAnswers,
) -> Result<Interview<'a>, EvalError> {
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
        let title = question
            .prompt
            .render(&context)
            .map_err(|error| EvalError::render(&question.id, "prompt", error))?;
        let description = question
            .description
            .as_ref()
            .map(|tmpl| {
                tmpl.render(&context)
                    .map_err(|error| EvalError::render(&question.id, "description", error))
            })
            .transpose()?;
        let default = default
            .as_ref()
            .map(|tmpl| {
                tmpl.render(&context)
                    .map_err(|error| EvalError::render(&question.id, "default", error))
            })
            .transpose()?;
        batch.items.push(Item::Prompt(Prompt {
            id: question.id.clone(),
            kind: PromptKind::Text,
            title,
            description,
            default,
            required: question.required,
        }));
    }
    if batch.items.is_empty() {
        Ok(Interview::Complete(Completed { answers }))
    } else {
        Ok(Interview::Asking(Pending {
            template,
            answers,
            held,
            batch,
        }))
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
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>> {
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
        advance(self.template, answers, held).map_err(AnswerError::Eval)
    }
}

#[cfg(test)]
mod tests {
    use super::{AnswerError, Interview, Item, RawAnswer, RawAnswers, Seed};
    use crate::{
        jinja::Tmpl,
        template::{Id, Node, Question, QuestionKind, Template},
    };
    use std::path::Path;

    #[test]
    fn later_batch_render_failure_does_not_record_submission() {
        let template = Template::load(Path::new("tests/fixtures/err-default-render/template"))
            .expect("fixture loads");
        let Interview::Asking(pending) = Interview::start(&template, Seed {}).expect("first batch")
        else {
            panic!("expected first batch")
        };
        assert_eq!(pending.batch().items.len(), 1);
        let mut answers = RawAnswers::new();
        answers.insert(
            Id::parse("first").unwrap(),
            RawAnswer(serde_json::json!("value")),
        );
        let AnswerError::Eval(error) = pending.answer(answers).unwrap_err() else {
            panic!("expected evaluation failure")
        };
        assert_eq!(error.id.as_str(), "second");
        assert_eq!(error.field, "default");

        let Interview::Asking(restarted) = Interview::start(&template, Seed {}).unwrap() else {
            panic!("the failed submission must not complete the interview")
        };
        let Item::Prompt(prompt) = &restarted.batch().items[0];
        assert_eq!(prompt.id.as_str(), "first");
    }

    #[test]
    fn start_reports_the_failing_field() {
        for field in ["prompt", "description", "default"] {
            let bad = || Tmpl::compile("{{ 'value' | nope }}".into()).unwrap();
            let question = Question {
                id: Id::parse("item").unwrap(),
                prompt: if field == "prompt" {
                    bad()
                } else {
                    Tmpl::compile("Item?".into()).unwrap()
                },
                description: (field == "description").then(bad),
                required: false,
                kind: QuestionKind::Text {
                    default: (field == "default").then(bad),
                },
            };
            let template = Template {
                name: "sample".into(),
                description: None,
                source_dir: std::path::PathBuf::new(),
                interview: vec![Node::Question(question)],
            };
            let error = Interview::start(&template, Seed {}).unwrap_err();
            assert_eq!(error.id.as_str(), "item");
            assert_eq!(error.field, field);
            assert!(error.message.contains("unknown filter"));
        }
    }
}
