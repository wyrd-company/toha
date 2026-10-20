// ---
// relationships:
//   implements: template-composition
// ---
//! Selection of the options of a template's part question.
use crate::{
    RawAnswers, Template,
    jinja::Typed,
    template::{Question, QuestionKind},
};
use serde::{Deserialize, Serialize};

/// Part flags and the copied selection they modify. Kept in staged interviews.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selection {
    pub with: Vec<String>,
    pub without: Vec<String>,
    pub base: Option<Vec<String>>,
}
impl Selection {
    pub fn is_present(&self) -> bool {
        !self.with.is_empty() || !self.without.is_empty()
    }
    /// Check usage before starting an interview. Answer constraints remain the
    /// interview engine's responsibility.
    pub fn admit(&self, template: &Template, supplied: &RawAnswers) -> Result<(), String> {
        if !self.is_present() {
            return Ok(());
        }
        let question = template
            .part_question()
            .ok_or("--with and --without need a template with a parts question")?;
        if supplied.contains_key(&question.id) {
            return Err(
                "an answers document cannot answer the parts question with --with or --without"
                    .into(),
            );
        }
        let options = options(question);
        for part in self.with.iter().chain(&self.without) {
            if !options.contains(part) {
                return Err(format!("unknown part {part:?}"));
            }
            if self.with.contains(part) && self.without.contains(part) {
                return Err(format!(
                    "part {part:?} is named by both --with and --without"
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn settle(&self, default: Vec<String>) -> Vec<String> {
        let mut selected = self.base.clone().unwrap_or(default);
        for part in &self.with {
            if !selected.contains(part) {
                selected.push(part.clone());
            }
        }
        selected.retain(|part| !self.without.contains(part));
        selected
    }
}
pub(crate) fn options(question: &Question) -> &[String] {
    match &question.kind {
        QuestionKind::MultiSelect {
            options: Typed::Literal(options),
            ..
        } => options,
        _ => &[],
    }
}
