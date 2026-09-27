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
    env.set_formatter(|out, state, value| {
        minijinja::escape_formatter(
            out,
            state,
            if value.is_none() {
                &Value::UNDEFINED
            } else {
                value
            },
        )
    });
    env.add_filter("kebab", |s: String| s.to_kebab_case());
    env.add_filter("snake", |s: String| s.to_snake_case());
    env.add_filter("camel", |s: String| s.to_lower_camel_case());
    env.add_filter("pascal", |s: String| s.to_upper_camel_case());
    env.add_filter("constant", |s: String| s.to_shouty_snake_case());
    env.add_filter("title", |s: String| s.to_title_case());
    env.add_filter("toyaml", toyaml);
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

/// Serializes a value as a YAML document body with no leading `---`. The
/// document's trailing newline is dropped, so the result composes with
/// `indent`, unless it is data: a block scalar ending the document keeps it.
fn toyaml(value: &Value) -> Result<Value, Error> {
    let yaml = serde_norway::to_string(value).map_err(|err| {
        Error::new(ErrorKind::InvalidOperation, "cannot serialize to YAML").with_source(err)
    })?;
    let body = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    let parse = |text: &str| serde_norway::from_str::<serde_json::Value>(text).ok();
    let body = match body.strip_suffix('\n') {
        Some(stripped) if parse(stripped).is_some_and(|v| Some(v) == parse(body)) => stripped,
        _ => body,
    };
    Ok(Value::from_safe_string(body.to_owned()))
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
    pub fn source(&self) -> &str {
        &self.source
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
    pub fn source(&self) -> &str {
        &self.source
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
    /// The expression source, or `None` for a literal.
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Literal(_) => None,
            Self::Expr(expr) => Some(expr.source()),
        }
    }
    /// The value when it is a literal, known before evaluation.
    pub fn literal(&self) -> Option<&T> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Expr(_) => None,
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

#[cfg(test)]
mod tests {
    use super::Tmpl;
    use serde_json::json;

    fn render(source: &str, ctx: serde_json::Value) -> String {
        Tmpl::compile(source.into())
            .unwrap_or_else(|err| panic!("compile {source:?}: {err}"))
            .render(ctx)
            .unwrap_or_else(|err| panic!("render {source:?}: {err}"))
    }

    #[test]
    fn tojson_serializes_values() {
        let ctx = json!({
            "flag": true,
            "text": "say \"hi\"",
            "item": {"b": [1, 2], "a": "x"},
        });
        assert_eq!(render("{{ flag | tojson }}", ctx.clone()), "true");
        assert_eq!(
            render("{{ text | tojson }}", ctx.clone()),
            r#""say \"hi\"""#
        );
        assert_eq!(render("{{ item | tojson }}", ctx), r#"{"a":"x","b":[1,2]}"#);
    }

    #[test]
    fn toyaml_serializes_nested_object() {
        let ctx = json!({
            "item": {
                "name": "demo",
                "enabled": true,
                "ports": [80, 443],
                "inner": {"key": "value"},
            },
        });
        assert_eq!(
            render("{{ item | toyaml }}", ctx.clone()),
            "enabled: true\ninner:\n  key: value\nname: demo\nports:\n- 80\n- 443"
        );
        assert_eq!(
            render("root:\n  {{ item.inner | toyaml | indent(2) }}\n", ctx),
            "root:\n  key: value\n"
        );
    }

    #[test]
    fn toyaml_round_trips_values() {
        let failures = [
            json!("hello\n"),
            json!("\n"),
            json!({"inner": {"k": "v\n"}}),
            json!(["a", "b\n"]),
            json!({"a": 2, "b": [1, true, null], "c": "x"}),
            json!("true"),
            json!({}),
            json!(null),
        ]
        .into_iter()
        .filter_map(|value| {
            let yaml = render("{{ value | toyaml }}", json!({ "value": value }));
            let parsed: serde_json::Value =
                serde_norway::from_str(&yaml).unwrap_or_else(|err| panic!("parse {yaml:?}: {err}"));
            (parsed != value).then(|| format!("{value} -> {yaml:?} -> {parsed}"))
        })
        .collect::<Vec<_>>();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn macro_is_defined_and_called_in_one_template() {
        assert_eq!(
            render(
                "{% macro greet(who) %}hello {{ who }}{% endmacro %}{{ greet(name) }}",
                json!({"name": "world"}),
            ),
            "hello world"
        );
    }

    #[test]
    fn loop_break_stops_iteration() {
        assert_eq!(
            render(
                "{% for n in items %}{% if n > 2 %}{% break %}{% endif %}{{ n }}{% endfor %}",
                json!({"items": [1, 2, 3, 4]}),
            ),
            "12"
        );
    }
}
