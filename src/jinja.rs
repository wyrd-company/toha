// ---
// relationships:
//   implements: architecture
// ---
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase,
};
use minijinja::{Environment, Error, ErrorKind, State, Value};
use serde::{Serialize, de::DeserializeOwned};

pub type RenderError = Error;

pub fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_keep_trailing_newline(true);
    env.add_filter("kebab", |s: String| s.to_kebab_case());
    env.add_filter("snake", |s: String| s.to_snake_case());
    env.add_filter("camel", |s: String| s.to_lower_camel_case());
    env.add_filter("pascal", |s: String| s.to_upper_camel_case());
    env.add_filter("constant", |s: String| s.to_shouty_snake_case());
    env.add_filter("title", |s: String| s.to_title_case());
    env.add_function("now", |state: &State| {
        state.lookup("__toha_now").unwrap_or(Value::UNDEFINED)
    });
    env.add_filter(
        "dateformat",
        |value: String, pattern: Option<String>| -> Result<String, Error> {
            let zoned: jiff::Zoned = value.parse().map_err(|err: jiff::Error| {
                Error::new(ErrorKind::InvalidOperation, err.to_string())
            })?;
            Ok(zoned
                .strftime(pattern.as_deref().unwrap_or("%Y-%m-%d"))
                .to_string())
        },
    );
    env
}

static GLOBALS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    environment()
        .globals()
        .map(|(name, _)| name.to_owned())
        .collect()
});
pub fn is_global(name: &str) -> bool {
    GLOBALS.contains(name)
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

pub struct Expr {
    source: String,
    referenced_ids: HashSet<String>,
}
impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expr")
            .field("source", &self.source)
            .finish()
    }
}
impl Expr {
    pub fn compile(source: String) -> Result<Self, Error> {
        let env = environment();
        let referenced_ids = env.compile_expression(&source)?.undeclared_variables(false);
        Ok(Self {
            source,
            referenced_ids,
        })
    }
    pub fn references(&self) -> &HashSet<String> {
        &self.referenced_ids
    }
    pub fn eval<S: Serialize>(&self, ctx: S) -> Result<Value, Error> {
        environment().compile_expression(&self.source)?.eval(ctx)
    }
}
#[derive(Debug)]
pub enum Typed<T> {
    Literal(T),
    Expr(Expr),
}
impl<T: Clone + DeserializeOwned> Typed<T> {
    pub fn eval<S: Serialize>(&self, ctx: S) -> Result<T, Error> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Expr(expr) => {
                let value = expr.eval(ctx)?;
                serde_json::from_value(
                    serde_json::to_value(value)
                        .map_err(|err| Error::new(ErrorKind::InvalidOperation, err.to_string()))?,
                )
                .map_err(|err| {
                    Error::new(
                        ErrorKind::InvalidOperation,
                        format!("typed expression: {err}"),
                    )
                })
            }
        }
    }
    pub fn references(&self) -> HashSet<String> {
        match self {
            Self::Literal(_) => HashSet::new(),
            Self::Expr(expr) => expr.references().clone(),
        }
    }
}

pub fn context_from_answers(
    answers: &crate::interview::Answers,
    data: &indexmap::IndexMap<crate::template::Id, serde_json::Value>,
    now: &jiff::Zoned,
) -> BTreeMap<String, serde_json::Value> {
    let mut values: BTreeMap<String, serde_json::Value> = data
        .iter()
        .map(|(id, value)| (id.as_str().to_owned(), value.clone()))
        .collect();
    for (id, answer) in answers {
        values.insert(id.as_str().to_owned(), answer.to_json());
    }
    values.insert(
        "__toha_now".into(),
        serde_json::Value::String(now.to_string()),
    );
    values
}
