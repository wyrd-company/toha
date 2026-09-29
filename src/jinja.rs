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

/// Whether `name` is one of the seventeen reserved Toha context names. Used by
/// current-context load-time reference checking and runtime readiness so a
/// reserved name resolves like a global rather than an undefined id.
pub(crate) fn is_reserved(name: &str) -> bool {
    crate::context::RESERVED_NAMES.contains(&name)
}

/// Whether a template-mode source can observe any fixed environment value.
pub(crate) fn template_needs_environment(source: &str) -> bool {
    match minijinja::machinery::parse(source, "<analysis>", Default::default(), Default::default())
    {
        Ok(stmt) => {
            let mut walk = NeedWalk::new();
            walk.stmt(&stmt);
            walk.needed
        }
        // A source that compiled through the real environment should parse
        // here too; an unexpected failure is treated conservatively as a need.
        Err(_) => true,
    }
}

/// Whether an expression-mode source can observe any fixed environment value.
pub(crate) fn expr_needs_environment(source: &str) -> bool {
    match minijinja::machinery::parse_expr(source) {
        Ok(expr) => {
            let mut walk = NeedWalk::new();
            walk.expr(&expr);
            walk.needed
        }
        Err(_) => true,
    }
}

impl Tmpl {
    pub(crate) fn needs_environment(&self) -> bool {
        template_needs_environment(&self.source)
    }
}
impl Expr {
    pub(crate) fn needs_environment(&self) -> bool {
        expr_needs_environment(&self.source)
    }
}
impl<T: Clone + DeserializeOwned> Typed<T> {
    pub(crate) fn needs_environment(&self) -> bool {
        match self {
            Self::Literal(_) => false,
            Self::Expr(expr) => expr.needs_environment(),
        }
    }
}

/// A control-flow-insensitive, read-accurate walk over a parsed Jinja AST that
/// marks whether the program can observe any of the five fixed environment
/// values or the built-in `debug` callable.
///
/// Soundness rules (design "Complete pre-interview analysis"): assignment
/// right-hand sides are visited before their targets bind; lexical shadowing is
/// tracked per block so a shadowed name is not a fixed read and a self-shadow
/// right-hand side still is; macro bodies and false branches are reachable;
/// dynamic item operands are visited while a literal property string is not a
/// root read; any un-shadowed reference to `debug` (read or called, directly or
/// through an alias) marks a need. An unparseable source is conservatively a
/// need. When the include runtime (task 1061) lands, each included body's need
/// unions into its containing body through this same walk; the real transitive
/// include proof is that task's obligation, not a seam unit test here.
struct NeedWalk {
    needed: bool,
    scopes: Vec<std::collections::HashSet<String>>,
}
impl NeedWalk {
    fn new() -> Self {
        Self {
            needed: false,
            scopes: vec![std::collections::HashSet::new()],
        }
    }
    fn push(&mut self) {
        self.scopes.push(std::collections::HashSet::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn bind(&mut self, name: &str) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_owned());
        }
    }
    fn bound(&self, name: &str) -> bool {
        self.scopes.iter().any(|frame| frame.contains(name))
    }
    fn bind_target(&mut self, target: &minijinja::machinery::ast::Expr) {
        use minijinja::machinery::ast::Expr;
        match target {
            Expr::Var(var) => self.bind(var.id),
            Expr::List(list) => {
                for item in &list.items {
                    self.bind_target(item);
                }
            }
            // An attribute/item assignment target reads its base object; it
            // introduces no new root name.
            other => self.expr(other),
        }
    }
    fn block(&mut self, stmts: &[minijinja::machinery::ast::Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }
    fn macro_decl(&mut self, decl: &minijinja::machinery::ast::Macro) {
        for default in &decl.defaults {
            self.expr(default);
        }
        self.push();
        for arg in &decl.args {
            self.bind_target(arg);
        }
        self.bind(decl.name);
        self.block(&decl.body);
        self.pop();
        // The macro name is visible to later siblings after its declaration.
        self.bind(decl.name);
    }
    fn call(&mut self, call: &minijinja::machinery::ast::Call) {
        self.expr(&call.expr);
        for arg in &call.args {
            self.call_arg(arg);
        }
    }
    fn call_arg(&mut self, arg: &minijinja::machinery::ast::CallArg) {
        use minijinja::machinery::ast::CallArg;
        match arg {
            CallArg::Pos(e) | CallArg::PosSplat(e) | CallArg::KwargSplat(e) => self.expr(e),
            CallArg::Kwarg(_, e) => self.expr(e),
        }
    }
    fn stmt(&mut self, stmt: &minijinja::machinery::ast::Stmt) {
        use minijinja::machinery::ast::Stmt;
        if self.needed {
            return;
        }
        match stmt {
            Stmt::Template(t) => self.block(&t.children),
            Stmt::EmitExpr(e) => self.expr(&e.expr),
            Stmt::EmitRaw(_) => {}
            Stmt::ForLoop(f) => {
                self.expr(&f.iter);
                self.push();
                self.bind_target(&f.target);
                self.bind("loop");
                if let Some(filter) = &f.filter_expr {
                    self.expr(filter);
                }
                self.block(&f.body);
                self.pop();
                // The else body runs in the enclosing scope.
                self.block(&f.else_body);
            }
            Stmt::IfCond(c) => {
                self.expr(&c.expr);
                self.push();
                self.block(&c.true_body);
                self.pop();
                self.push();
                self.block(&c.false_body);
                self.pop();
            }
            Stmt::WithBlock(w) => {
                for (_, value) in &w.assignments {
                    self.expr(value);
                }
                self.push();
                for (target, _) in &w.assignments {
                    self.bind_target(target);
                }
                self.block(&w.body);
                self.pop();
            }
            Stmt::Set(s) => {
                self.expr(&s.expr);
                self.bind_target(&s.target);
            }
            Stmt::SetBlock(s) => {
                if let Some(filter) = &s.filter {
                    self.expr(filter);
                }
                self.push();
                self.block(&s.body);
                self.pop();
                self.bind_target(&s.target);
            }
            Stmt::AutoEscape(a) => {
                self.expr(&a.enabled);
                self.push();
                self.block(&a.body);
                self.pop();
            }
            Stmt::FilterBlock(fb) => {
                self.expr(&fb.filter);
                self.push();
                self.block(&fb.body);
                self.pop();
            }
            Stmt::Macro(m) => self.macro_decl(m),
            Stmt::CallBlock(cb) => {
                self.call(&cb.call);
                self.macro_decl(&cb.macro_decl);
            }
            Stmt::Continue(_) | Stmt::Break(_) => {}
            Stmt::Do(d) => self.call(&d.call),
        }
    }
    fn expr(&mut self, expr: &minijinja::machinery::ast::Expr) {
        use minijinja::machinery::ast::Expr;
        if self.needed {
            return;
        }
        match expr {
            Expr::Var(var) => {
                let id = var.id;
                let sensitive = crate::context::ENVIRONMENT_NAMES.contains(&id) || id == "debug";
                if sensitive && !self.bound(id) {
                    self.needed = true;
                }
            }
            Expr::Const(_) => {}
            Expr::Slice(s) => {
                self.expr(&s.expr);
                for inner in [&s.start, &s.stop, &s.step].into_iter().flatten() {
                    self.expr(inner);
                }
            }
            Expr::UnaryOp(u) => self.expr(&u.expr),
            Expr::BinOp(b) => {
                self.expr(&b.left);
                self.expr(&b.right);
            }
            Expr::Compare(c) => {
                self.expr(&c.expr);
                for op in &c.ops {
                    self.expr(&op.expr);
                }
            }
            Expr::IfExpr(i) => {
                self.expr(&i.test_expr);
                self.expr(&i.true_expr);
                if let Some(f) = &i.false_expr {
                    self.expr(f);
                }
            }
            Expr::Filter(f) => {
                if let Some(inner) = &f.expr {
                    self.expr(inner);
                }
                for arg in &f.args {
                    self.call_arg(arg);
                }
            }
            Expr::Test(t) => {
                self.expr(&t.expr);
                for arg in &t.args {
                    self.call_arg(arg);
                }
            }
            // An attribute name is a literal, not a root read.
            Expr::GetAttr(g) => self.expr(&g.expr),
            // A dynamic subscript is a read; a literal string subscript is a
            // constant and contributes nothing.
            Expr::GetItem(g) => {
                self.expr(&g.expr);
                self.expr(&g.subscript_expr);
            }
            Expr::Call(c) => self.call(c),
            Expr::List(l) => {
                for item in &l.items {
                    self.expr(item);
                }
            }
            Expr::Map(m) => {
                for key in &m.keys {
                    self.expr(key);
                }
                for value in &m.values {
                    self.expr(value);
                }
            }
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
    fn analyzer_detects_direct_and_shadowed_environment_reads() {
        use super::{expr_needs_environment as expr, template_needs_environment as tmpl};
        // Direct root read of a fixed name, and an unrelated read.
        assert!(tmpl("{{ toha_env_editor }}"));
        assert!(!tmpl("{{ project_name }}"));
        // A reserved but non-environment name is not a fixed read.
        assert!(!tmpl("{{ toha_target_name }} {{ toha_host_os }}"));
        // Self-shadow: the right-hand side reads the real value before the
        // target binds; a constant right-hand side does not.
        assert!(tmpl(
            "{% set toha_env_editor = toha_env_editor %}{{ toha_env_editor }}"
        ));
        assert!(!tmpl(
            "{% set toha_env_editor = \"code\" %}{{ toha_env_editor }}"
        ));
        // Value alias through assignment.
        assert!(tmpl("{% set chosen = toha_env_shell %}{{ chosen }}"));
        // `debug`, read or called, directly or aliased.
        assert!(tmpl("{{ debug() }}"));
        assert!(tmpl("{% set d = debug %}{{ d() }}"));
        // Dynamic item operand is a read; a literal property string is not.
        assert!(tmpl("{{ config[toha_env_user] }}"));
        assert!(!tmpl("{{ config[\"toha_env_user\"] }}"));
        assert!(!tmpl("{{ config.toha_env_user }}"));
        // False branch and uncalled macro bodies are reachable.
        assert!(tmpl(
            "{% if flag %}a{% else %}{{ toha_env_visual }}{% endif %}"
        ));
        assert!(tmpl(
            "{% macro unused() %}{{ toha_env_shell }}{% endmacro %}"
        ));
        // A conditional shadow does not leak past its block.
        assert!(tmpl(
            "{% if flag %}{% set toha_env_editor = 1 %}{% endif %}{{ toha_env_editor }}"
        ));
        // A macro parameter shadows the fixed name inside the body.
        assert!(!tmpl(
            "{% macro m(toha_env_editor) %}{{ toha_env_editor }}{% endmacro %}"
        ));
        // Expression mode.
        assert!(expr("toha_env_user"));
        assert!(!expr("answer + 1"));
        assert!(expr("config[toha_env_user]"));
        assert!(!expr("config[\"toha_env_user\"]"));
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
