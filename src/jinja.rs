// ---
// relationships:
//   implements: architecture
// ---
use std::collections::{BTreeMap, HashSet};

use minijinja::{Environment, Error, context};
use serde::Serialize;

pub type RenderError = Error;

pub fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_keep_trailing_newline(true);
    env
}

pub struct Tmpl {
    source: String,
    env: Environment<'static>,
    referenced_ids: HashSet<String>,
}

impl std::fmt::Debug for Tmpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tmpl")
            .field("source", &self.source)
            .finish()
    }
}

impl Tmpl {
    pub fn compile(source: String) -> Result<Self, Error> {
        let mut env = environment();
        env.add_template_owned("value", source.clone())?;
        let referenced_ids = env.get_template("value")?.undeclared_variables(false);
        Ok(Self {
            source,
            env,
            referenced_ids,
        })
    }

    pub fn references(&self) -> &HashSet<String> {
        &self.referenced_ids
    }

    pub fn render<S: Serialize>(&self, ctx: S) -> Result<String, RenderError> {
        self.env.get_template("value")?.render(ctx)
    }
}

pub fn context_from_answers(answers: &crate::interview::Answers) -> minijinja::Value {
    let values: BTreeMap<String, String> = answers
        .iter()
        .map(|(id, answer)| (id.as_str().to_owned(), answer.as_text().to_owned()))
        .collect();
    context!(..values)
}
