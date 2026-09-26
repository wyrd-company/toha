#[doc = include_str!("../README.md")]
// ---
// relationships:
//   implements: architecture
// ---
pub mod apply;
pub mod config;
pub mod hook;
pub mod interview;
mod jinja;
pub mod plan;
pub mod protocol;
pub mod registry;
pub mod source;
pub mod staging;
pub mod template;

pub use apply::{Applied, ApplyError, ApplyOptions};
pub use interview::{
    Answer, AnswerError, Answers, Batch, CheckError, Completed, EvalError, Interview, Item,
    Pending, Prompt, PromptKind, RawAnswer, RawAnswers, Rejection, Rejections, Seed,
};
pub use plan::{Content, Plan, PlanError, PlannedFile, PlannedHook, PlannedProgram, TargetPath};
pub use template::{Id, LoadError, Node, Problem, Question, QuestionKind, Template};
