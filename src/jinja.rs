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
    // File bodies register under their real root-relative names, so the default
    // extension-based auto-escape would JSON-quote a `.yml`/`.yaml`/`.json` body
    // and HTML-escape an `.html` one. Toha renders every body as raw text (the
    // loaderless `Tmpl` always compiled under an extensionless name and so never
    // escaped); force that one rule for every surface.
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
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
        // The `multi_template` feature is crate-wide, so `{% include %}` and its
        // sibling statements now parse in every field. A loaderless field admits
        // none of them. Rejecting them after syntax validation but before the
        // template is used keeps includes to file bodies by type. A no-loader
        // environment is not enough on its own: the internal template registers
        // under `"value"`, so `{% include "value" %}` would resolve this very
        // template.
        reject_loaderless_statements(&source)?;
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
            // Multi-template statements. Only `include` reaches a valid file
            // body; its literal target is a constant, and the included body's
            // own need is unioned separately by walking each closure member's
            // source (see `FileTmpl::needs_environment`), so the statement adds
            // no direct need here. The sibling statements are rejected before a
            // template is registered, so these arms are unreachable in a valid
            // program; they visit their operands to stay sound and exhaustive.
            Stmt::Include(i) => self.expr(&i.name),
            Stmt::Block(b) => self.block(&b.body),
            Stmt::Import(i) => {
                self.expr(&i.expr);
                self.bind_target(&i.name);
            }
            Stmt::FromImport(f) => {
                self.expr(&f.expr);
                for (name, alias) in &f.names {
                    self.bind_target(alias.as_ref().unwrap_or(name));
                }
            }
            Stmt::Extends(e) => self.expr(&e.name),
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

/// The statement grammar Toha admits differs by render surface. A loaderless
/// field (`Tmpl`) admits none of the multi-template statements; a file body
/// (`FileTmpl`) admits only `include`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Loaderless,
    FileBody,
}

/// One `{% include %}` directive collected from a file body: its ordered literal
/// candidate names and whether `ignore missing` is present.
struct IncludeSpec {
    candidates: Vec<String>,
    ignore_missing: bool,
}

/// A statement a surface refuses, formatted by the caller into that surface's
/// error type.
enum StatementReject {
    /// A multi-template statement unavailable on this surface, named for the
    /// diagnostic (`block`, `import`, `from import`, `extends`, or `include`).
    Unsupported(&'static str),
    /// An `include` whose target is not a literal string or literal list of
    /// string constants.
    DynamicTarget,
}

/// The outcome of one AST inspection parse.
enum Analysis {
    /// A file body's ordered include specs (empty for a loaderless field, which
    /// admits no includes).
    Ok(Vec<IncludeSpec>),
    /// The first refused statement.
    Reject(StatementReject),
}

/// A control-flow-insensitive walk over a parsed Jinja AST. It rejects the
/// statements unavailable on `surface` anywhere in the tree — including a
/// statement nested in an unreachable branch, macro, loop, or capture — and, on
/// a file body, collects the ordered literal include specs. It recurses through
/// every statement-body vector, matching MiniJinja's own visitor exhaustiveness,
/// which is why enabling `multi_template` cannot smuggle a sibling statement
/// past this gate through a nested block.
struct StmtWalk {
    surface: Surface,
    includes: Vec<IncludeSpec>,
    reject: Option<StatementReject>,
}
impl StmtWalk {
    /// Parses `source` and analyzes it for `surface`. Syntax errors surface as
    /// the parse `Error`.
    fn analyze(source: &str, surface: Surface) -> Result<Analysis, Error> {
        let stmt =
            minijinja::machinery::parse(source, "value", Default::default(), Default::default())?;
        let mut walk = StmtWalk {
            surface,
            includes: Vec::new(),
            reject: None,
        };
        walk.stmt(&stmt);
        Ok(match walk.reject {
            Some(reject) => Analysis::Reject(reject),
            None => Analysis::Ok(walk.includes),
        })
    }
    fn block(&mut self, stmts: &[minijinja::machinery::ast::Stmt]) {
        for stmt in stmts {
            if self.reject.is_some() {
                return;
            }
            self.stmt(stmt);
        }
    }
    fn refuse(&mut self, statement: &'static str) {
        if self.reject.is_none() {
            self.reject = Some(StatementReject::Unsupported(statement));
        }
    }
    fn include(&mut self, inc: &minijinja::machinery::ast::Include) {
        if self.surface == Surface::Loaderless {
            self.refuse("include");
            return;
        }
        match literal_candidates(&inc.name) {
            Some(candidates) => self.includes.push(IncludeSpec {
                candidates,
                ignore_missing: inc.ignore_missing,
            }),
            None => {
                if self.reject.is_none() {
                    self.reject = Some(StatementReject::DynamicTarget);
                }
            }
        }
    }
    fn stmt(&mut self, stmt: &minijinja::machinery::ast::Stmt) {
        use minijinja::machinery::ast::Stmt;
        if self.reject.is_some() {
            return;
        }
        match stmt {
            Stmt::Template(t) => self.block(&t.children),
            Stmt::EmitExpr(_) | Stmt::EmitRaw(_) => {}
            Stmt::ForLoop(f) => {
                self.block(&f.body);
                self.block(&f.else_body);
            }
            Stmt::IfCond(c) => {
                self.block(&c.true_body);
                self.block(&c.false_body);
            }
            Stmt::WithBlock(w) => self.block(&w.body),
            Stmt::Set(_) => {}
            Stmt::SetBlock(s) => self.block(&s.body),
            Stmt::AutoEscape(a) => self.block(&a.body),
            Stmt::FilterBlock(fb) => self.block(&fb.body),
            Stmt::Macro(m) => self.block(&m.body),
            Stmt::CallBlock(cb) => self.block(&cb.macro_decl.body),
            Stmt::Continue(_) | Stmt::Break(_) | Stmt::Do(_) => {}
            Stmt::Include(i) => self.include(i),
            Stmt::Block(_) => self.refuse("block"),
            Stmt::Import(_) => self.refuse("import"),
            Stmt::FromImport(_) => self.refuse("from import"),
            Stmt::Extends(_) => self.refuse("extends"),
        }
    }
}

/// The ordered string constants of an include target: a single literal string,
/// or a literal list whose items are all string constants. Any other shape — a
/// variable, an expression, or a list with a non-string or computed item — is
/// `None`, which the caller reports as a dynamic target.
fn literal_candidates(expr: &minijinja::machinery::ast::Expr) -> Option<Vec<String>> {
    use minijinja::machinery::ast::Expr;
    fn constant(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Const(c) => c.value.as_str().map(str::to_owned),
            _ => None,
        }
    }
    match expr {
        Expr::Const(_) => constant(expr).map(|s| vec![s]),
        Expr::List(list) => list.items.iter().map(constant).collect(),
        _ => None,
    }
}

/// Rejects every multi-template statement in a loaderless field, after syntax
/// validation. The error carries MiniJinja's own kind so the message reads like
/// the other compile diagnostics on that surface.
fn reject_loaderless_statements(source: &str) -> Result<(), Error> {
    match StmtWalk::analyze(source, Surface::Loaderless)? {
        Analysis::Ok(_) => Ok(()),
        Analysis::Reject(StatementReject::Unsupported(statement)) => Err(Error::new(
            ErrorKind::SyntaxError,
            format!("jinja {statement} is unavailable outside file bodies"),
        )),
        // A loaderless field refuses `include` itself before inspecting its
        // target, so a dynamic target never reaches this arm.
        Analysis::Reject(StatementReject::DynamicTarget) => Err(Error::new(
            ErrorKind::SyntaxError,
            "jinja include is unavailable outside file bodies".to_owned(),
        )),
    }
}

/// A failure to compile a file body's include closure. Every message names a
/// "jinja include" (or a "jinja {statement}"); none reuses the YAML `!include`
/// tag's wording, so the two features stay distinguishable in diagnostics.
#[derive(Debug)]
pub enum IncludeError {
    /// A syntax error in a body or a partial, with MiniJinja's own wording.
    Template(Error),
    /// No candidate exists; `candidates` is in author order.
    NotFound { candidates: Vec<String>, by: String },
    /// A candidate is absolute, uses `\`, traverses with `..`, or canonicalizes
    /// outside the template root.
    Escape { name: String, by: String },
    /// A component of the candidate path is a symlink.
    Symlink { name: String, by: String },
    /// A resolved candidate is not valid UTF-8.
    NotUtf8 { name: String, by: String },
    /// A resolved candidate could not be read.
    Unreadable {
        name: String,
        by: String,
        source: std::io::Error,
    },
    /// The static include graph re-enters a file; `path` is the named cycle.
    Cycle { path: String },
    /// An include target is not a literal string or literal list of strings.
    Dynamic { by: String },
    /// A statement other than `include` appears in a file body.
    UnsupportedStatement {
        statement: &'static str,
        surface: &'static str,
        by: String,
    },
}
impl std::fmt::Display for IncludeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Template(e) => write!(f, "{e}"),
            Self::NotFound { candidates, by } if candidates.len() == 1 => {
                write!(f, "jinja include not found: {}, included by {by}", candidates[0])
            }
            Self::NotFound { candidates, by } => write!(
                f,
                "jinja include candidates not found: {}, included by {by}",
                candidates.join(", ")
            ),
            Self::Escape { name, by } => {
                write!(f, "jinja include escapes template root: {name}, included by {by}")
            }
            Self::Symlink { name, by } => {
                write!(f, "jinja include path is a symlink: {name}, included by {by}")
            }
            Self::NotUtf8 { name, by } => {
                write!(f, "jinja include is not utf-8: {name}, included by {by}")
            }
            Self::Unreadable { name, by, source } => write!(
                f,
                "jinja include is not readable: {name}, included by {by}: {source}"
            ),
            Self::Cycle { path } => write!(f, "jinja include cycle: {path}"),
            Self::Dynamic { by } => write!(
                f,
                "jinja include target must be a literal string or literal list of strings, in {by}"
            ),
            Self::UnsupportedStatement {
                statement,
                surface,
                by,
            } => write!(f, "jinja {statement} is unavailable {surface}, found in {by}"),
        }
    }
}
impl std::error::Error for IncludeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Template(e) => Some(e),
            Self::Unreadable { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The include boundary for one loaded template: a cheap, cloneable handle to
/// the canonical template root. It consumes that identity and defines no new
/// one. Every include name a file body pulls in passes through this one seam,
/// which is the sole gate on a target (in-root, `/`-separated, no `..`,
/// absolute, or symlink). Includes are reachable only by holding a `FileTmpl`,
/// and only `Partials::compile` mints one.
///
/// Invariant: the wrapped root is already canonical (`Template::load`
/// canonicalizes it before constructing this).
#[derive(Clone, Debug)]
pub struct Partials {
    root: std::sync::Arc<std::path::PathBuf>,
}
impl Partials {
    /// Builds from the canonical template root. Does not re-canonicalize.
    pub fn rooted(root: &std::path::Path) -> Self {
        Self {
            root: std::sync::Arc::new(root.to_path_buf()),
        }
    }

    /// Compiles a file body into an include-capable template. `label` is the
    /// body's root-relative path, used for diagnostics, cycle paths, and the
    /// body's registration name. The transitive confined include closure is
    /// resolved, read, and registered before the returned `FileTmpl` can render;
    /// no filesystem loader is installed on the render environment.
    pub fn compile(&self, source: String, label: &str) -> Result<FileTmpl, IncludeError> {
        let mut members = BTreeMap::new();
        let body_canonical = self
            .root
            .join(label)
            .canonicalize()
            .unwrap_or_else(|_| self.root.join(label));
        let mut stack = vec![(label.to_owned(), body_canonical)];
        self.collect(&source, label, &mut members, &mut stack)?;
        FileTmpl::build(source, label, members)
    }

    /// Walks the literal include graph of `src` (included by `by`), resolving
    /// each directive's first existing candidate through the confinement seam,
    /// recording it as a closure member, and recursing. `stack` carries the
    /// canonical path of each open body for cycle detection; `members` collects
    /// the resolved partials by their author-string names.
    fn collect(
        &self,
        src: &str,
        by: &str,
        members: &mut BTreeMap<String, String>,
        stack: &mut Vec<(String, std::path::PathBuf)>,
    ) -> Result<(), IncludeError> {
        let specs = match StmtWalk::analyze(src, Surface::FileBody).map_err(IncludeError::Template)?
        {
            Analysis::Ok(specs) => specs,
            Analysis::Reject(StatementReject::Unsupported(statement)) => {
                return Err(IncludeError::UnsupportedStatement {
                    statement,
                    surface: "in file bodies",
                    by: by.to_owned(),
                });
            }
            Analysis::Reject(StatementReject::DynamicTarget) => {
                return Err(IncludeError::Dynamic { by: by.to_owned() });
            }
        };
        for spec in specs {
            // An empty literal list selects nothing and emits nothing, matching
            // MiniJinja's empty-sequence behavior, with or without ignore_missing.
            if spec.candidates.is_empty() {
                continue;
            }
            let mut selected = None;
            for candidate in &spec.candidates {
                if let Some((canonical, text)) = self.locate(candidate, by)? {
                    selected = Some((candidate.clone(), canonical, text));
                    break;
                }
            }
            let Some((name, canonical, text)) = selected else {
                // No candidate exists. `ignore missing` emits nothing; otherwise
                // the ordered candidates are reported.
                if spec.ignore_missing {
                    continue;
                }
                return Err(IncludeError::NotFound {
                    candidates: spec.candidates,
                    by: by.to_owned(),
                });
            };
            if stack.iter().any(|(_, path)| path == &canonical) {
                let mut cycle: Vec<String> = stack.iter().map(|(n, _)| n.clone()).collect();
                cycle.push(name);
                return Err(IncludeError::Cycle {
                    path: cycle.join(" -> "),
                });
            }
            // A partial reached by more than one path is registered once; only a
            // re-entry of an open body is a cycle.
            if members.contains_key(&name) {
                continue;
            }
            members.insert(name.clone(), text.clone());
            stack.push((name.clone(), canonical));
            self.collect(&text, &name, members, stack)?;
            stack.pop();
        }
        Ok(())
    }

    /// Resolves one candidate name through the confinement seam. `Ok(Some(..))`
    /// is the confined, non-symlink, regular UTF-8 file; `Ok(None)` is a missing
    /// candidate a list may fall past; `Err(..)` is a hard failure that no
    /// fallback or `ignore missing` suppresses.
    fn locate(
        &self,
        name: &str,
        by: &str,
    ) -> Result<Option<(std::path::PathBuf, String)>, IncludeError> {
        use std::path::Component;
        let escape = || IncludeError::Escape {
            name: name.to_owned(),
            by: by.to_owned(),
        };
        // The template namespace always uses `/`; a backslash is an escape,
        // independent of the host path syntax.
        if name.contains('\\') {
            return Err(escape());
        }
        let relative = std::path::Path::new(name);
        if relative.is_absolute() {
            return Err(escape());
        }
        let mut current = self.root.to_path_buf();
        for component in relative.components() {
            match component {
                Component::Normal(part) => {
                    current.push(part);
                    match std::fs::symlink_metadata(&current) {
                        Ok(meta) if meta.file_type().is_symlink() => {
                            return Err(IncludeError::Symlink {
                                name: name.to_owned(),
                                by: by.to_owned(),
                            });
                        }
                        Ok(_) => {}
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                            return Ok(None);
                        }
                        Err(source) => {
                            return Err(IncludeError::Unreadable {
                                name: name.to_owned(),
                                by: by.to_owned(),
                                source,
                            });
                        }
                    }
                }
                Component::CurDir => {}
                // A parent, root, or drive-prefix component escapes the root.
                _ => return Err(escape()),
            }
        }
        let canonical = match current.canonicalize() {
            Ok(path) => path,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(IncludeError::Unreadable {
                    name: name.to_owned(),
                    by: by.to_owned(),
                    source,
                });
            }
        };
        if !canonical.starts_with(self.root.as_path()) {
            return Err(escape());
        }
        if !canonical.is_file() {
            return Ok(None);
        }
        match std::fs::read(&canonical) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(Some((canonical, text))),
                Err(_) => Err(IncludeError::NotUtf8 {
                    name: name.to_owned(),
                    by: by.to_owned(),
                }),
            },
            Err(source) => Err(IncludeError::Unreadable {
                name: name.to_owned(),
                by: by.to_owned(),
                source,
            }),
        }
    }
}

/// A compiled file body that MAY resolve `{% include %}` against its root.
/// Holding one IS the include capability; it is a distinct type from `Tmpl`.
/// Its render environment has every reachable partial pre-registered and no
/// loader, so rendering performs no filesystem access.
pub struct FileTmpl {
    label: String,
    source: String,
    env: Environment<'static>,
    referenced_ids: HashSet<String>,
    needs_env: bool,
}
impl std::fmt::Debug for FileTmpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTmpl")
            .field("source", &self.source)
            .finish()
    }
}
impl FileTmpl {
    /// Registers the body and its closure members in one loader-free
    /// environment, unions their referenced ids, and unions their environment
    /// need across the closure. The body registers under `label`; a partial can
    /// never take that name without forming a cycle, which `collect` already
    /// refused.
    fn build(
        source: String,
        label: &str,
        members: BTreeMap<String, String>,
    ) -> Result<Self, IncludeError> {
        let mut env = environment();
        for (name, text) in &members {
            env.add_template_owned(name.clone(), text.clone())
                .map_err(IncludeError::Template)?;
        }
        env.add_template_owned(label.to_owned(), source.clone())
            .map_err(IncludeError::Template)?;
        let mut referenced_ids = HashSet::new();
        referenced_ids.extend(
            env.get_template(label)
                .map_err(IncludeError::Template)?
                .undeclared_variables(false),
        );
        for name in members.keys() {
            referenced_ids.extend(
                env.get_template(name)
                    .map_err(IncludeError::Template)?
                    .undeclared_variables(false),
            );
        }
        // The environment need is transitive: a fixed value reached only through
        // a nested partial still marks the body, because each member's source is
        // analyzed here. This is what makes a nested-include environment read
        // require stage trust at load time.
        let needs_env = template_needs_environment(&source)
            || members.values().any(|text| template_needs_environment(text));
        Ok(Self {
            label: label.to_owned(),
            source,
            env,
            referenced_ids,
            needs_env,
        })
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    /// References including those reached through partials.
    pub fn references(&self) -> &HashSet<String> {
        &self.referenced_ids
    }
    pub fn render<S: Serialize>(&self, ctx: S) -> Result<String, RenderError> {
        self.env.get_template(&self.label)?.render(ctx)
    }
    pub(crate) fn needs_environment(&self) -> bool {
        self.needs_env
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
