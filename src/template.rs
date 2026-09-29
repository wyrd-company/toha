// ---
// relationships:
//   implements: architecture
// ---
use crate::context::{
    EnvironmentAdmissionError, EnvironmentDecision, EnvironmentNeed, EnvironmentSnapshot,
    FixedEnvironmentSource, RenderOrigin,
};
use crate::jinja::{Expr, FileTmpl, Partials, Tmpl, Typed, is_global, is_reserved};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct Id(String);
impl Id {
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut chars = value.chars();
        if !matches!(chars.next(), Some('a'..='z' | '_'))
            || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(format!("invalid identifier: {value}"));
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub struct Template {
    pub name: String,
    pub description: Option<String>,
    pub source_dir: PathBuf,
    pub data: IndexMap<Id, Value>,
    pub interview: Vec<Node>,
    pub root: PathBuf,
    /// The single include-confinement owner for this template's file bodies,
    /// created once from the canonical `root`. It is retained so a source-tree
    /// body compiled during a later load step, and every stored `files:` body,
    /// resolves includes against the same root without a runtime loader.
    pub partials: Partials,
    pub files: Vec<FileRule>,
    pub ignore: GlobSet,
    pub static_files: GlobSet,
    pub hooks: Vec<HookNode>,
    pub messages: ApplyMessages,
    /// Parsed, post-`!include`, pre-render interview hook node values, in
    /// interview order. The reviewable surface reads these, so any field —
    /// known or future — is covered without changing review code.
    pub interview_hook_nodes: Vec<Value>,
    /// Parsed, post-`!include`, pre-render top-level hook node values, in list
    /// order.
    pub top_hook_nodes: Vec<Value>,
    /// The retained, compiled source-tree render program, in deterministic walk
    /// order. Planning renders these; it does not reopen a Jinja source after
    /// admission.
    pub render_program: Vec<SourceEntry>,
    /// Whether this template reserves and exposes the seventeen context names.
    /// A current load reserves them; a legacy staged load keeps the pre-context
    /// available/reserved set so an existing authored identifier is not turned
    /// into a collision mid-stage.
    reserves_context: bool,
    /// The immutable analysis result over the complete render program: whether
    /// any compiled surface can observe a fixed environment value, and the first
    /// location that does.
    environment_need: EnvironmentNeed,
}
/// One compiled source-tree file: its relative path (as compiled path-segment
/// templates) and its body, retained at load so planning renders without
/// reopening the file.
#[derive(Debug)]
pub struct SourceEntry {
    pub relative: PathBuf,
    pub source: PathBuf,
    pub segments: Vec<Tmpl>,
    pub body: SourceBody,
}
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SourceBody {
    /// A static file, copied verbatim from its source path.
    Static,
    /// A rendered body, compiled at load as an include-capable file body.
    Rendered(FileTmpl),
    /// A non-static, non-UTF-8 body. Planning surfaces the existing
    /// "add it to static" error without reopening the file.
    NonUtf8,
}
impl Template {
    /// Combines the immutable render-program analysis with one caller decision,
    /// capturing the closed five-value environment snapshot exactly once when
    /// granted and never otherwise.
    pub fn admit_environment(
        &self,
        decision: EnvironmentDecision,
        source: &mut impl FixedEnvironmentSource,
    ) -> Result<EnvironmentSnapshot, EnvironmentAdmissionError> {
        crate::context::admit(&self.environment_need, decision, source)
    }

    /// Whether this template reserves and exposes the seventeen context names.
    pub fn reserves_context(&self) -> bool {
        self.reserves_context
    }
}
/// Collects interview hook node values in interview order (depth-first through
/// groups), so the reviewable surface matches the runtime hook order.
fn collect_hook_nodes(values: &[Value], out: &mut Vec<Value>) {
    for value in values.iter().filter_map(Value::as_object) {
        if value.contains_key("hook") {
            out.push(Value::Object(value.clone()));
        } else if let Some(nodes) = value.get("nodes").and_then(Value::as_array) {
            collect_hook_nodes(nodes, out);
        }
    }
}
impl Template {
    pub fn has_question_id(&self, id: &Id) -> bool {
        fn contains(nodes: &[Node], id: &Id) -> bool {
            nodes.iter().any(|node| match node {
                Node::Question(question) => &question.id == id,
                Node::Group(group) => contains(&group.nodes, id),
                _ => false,
            })
        }
        contains(&self.interview, id)
    }
}
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Node {
    Question(Question),
    Computed(Computed),
    Group(Group),
    Message(Message),
    Hook(HookNode),
    Flow(FlowNode),
}
/// A control node: a peer of message/hook/computed. It has no id, records no
/// answer, and fires once at its position when its `when` is true (or always,
/// when `when` is absent). The action is declared data, never inferred.
#[derive(Debug)]
pub struct FlowNode {
    /// Reused `when`; `None` ⇒ always fires at its position.
    pub when: Option<Expr>,
    /// Optional diagnostic name; the node has no id.
    pub label: Option<String>,
    pub action: FlowAction,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAction {
    /// End the interview: write no files, run no hooks.
    Stop,
    /// Stop, and additionally remove the target's staged record.
    Abort,
    /// Complete normally but suppress the apply (plan only).
    DryRun,
    Skip(SkipScope),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipScope {
    /// Skip every remaining question in the interview.
    Rest,
    /// Skip the remaining siblings of the enclosing group; a load error at the
    /// top level, which names `{ skip: rest }`.
    Group,
}
#[derive(Debug)]
pub struct Question {
    pub id: Id,
    pub prompt: Tmpl,
    pub description: Option<Tmpl>,
    pub placeholder: Option<Tmpl>,
    pub required: Typed<bool>,
    pub kind: QuestionKind,
    pub validate: Validate,
    pub format: Option<Expr>,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub enum QuestionKind {
    Text {
        default: Option<Tmpl>,
    },
    Multiline {
        default: Option<Tmpl>,
    },
    Confirm {
        default: Option<Typed<bool>>,
    },
    Select {
        options: Typed<Vec<String>>,
        default: Option<Tmpl>,
    },
    MultiSelect {
        options: Typed<Vec<String>>,
        default: Option<Typed<Vec<String>>>,
    },
    TextLoop {
        default: Option<Typed<Vec<String>>>,
        min: Option<Typed<u32>>,
        max: Option<Typed<u32>>,
    },
}
#[derive(Debug, Default)]
pub struct Validate {
    pub min: Option<Typed<u32>>,
    pub max: Option<Typed<u32>>,
    pub regex: Option<Regex>,
}
#[derive(Debug)]
pub struct Computed {
    pub id: Id,
    pub expr: Expr,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub struct Group {
    pub name: Id,
    pub nodes: Vec<Node>,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub struct Message {
    pub text: Tmpl,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub struct HookNode {
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub struct HookCommand {
    pub program: HookProgram,
    pub cwd: Option<Tmpl>,
}
#[derive(Debug)]
pub enum HookProgram {
    Run(Vec<Tmpl>),
    Script { path: PathBuf, args: Vec<Tmpl> },
}
#[derive(Debug)]
pub struct FileRule {
    pub each: Each,
    pub source: PathBuf,
    pub path: Tmpl,
    pub when: Option<Expr>,
    /// The rule's source file body, compiled at load as an include-capable file
    /// body with the `each` binding in scope, so planning renders it and its
    /// selected partial closure without reopening any file. `None` only for a
    /// non-UTF-8 source, which planning surfaces on its existing read path.
    pub body: Option<FileTmpl>,
}
/// `<expression> as <name>`: the expression yields a sequence, and each item is
/// bound to `binding` in turn.
#[derive(Debug)]
pub struct Each {
    pub expr: Expr,
    pub binding: Id,
}
impl Each {
    /// One evaluation context per item, in item order, each holding the item
    /// under the binding name.
    pub fn contexts(
        &self,
        ctx: &std::collections::BTreeMap<String, Value>,
    ) -> Result<Vec<std::collections::BTreeMap<String, Value>>, String> {
        let values = self.expr.eval(ctx).map_err(|e| e.to_string())?;
        let values = serde_json::to_value(values).map_err(|e| e.to_string())?;
        let Some(items) = values.as_array() else {
            return Err("expected array".into());
        };
        Ok(items
            .iter()
            .map(|item| {
                let mut local = ctx.clone();
                local.insert(self.binding.as_str().into(), item.clone());
                local
            })
            .collect())
    }
}
#[derive(Debug, Default)]
pub struct ApplyMessages {
    pub before_apply: Option<Tmpl>,
    pub after_apply: Option<Tmpl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}
#[derive(Debug)]
pub struct LoadError {
    pub problems: Vec<Problem>,
}
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.problems.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}: {}", p.path, p.message)?;
        }
        Ok(())
    }
}
impl std::error::Error for LoadError {}
fn problem(ps: &mut Vec<Problem>, path: impl Into<String>, message: impl Into<String>) {
    ps.push(Problem {
        path: path.into(),
        message: message.into(),
    });
}
fn error(path: impl Into<String>, message: impl Into<String>) -> LoadError {
    LoadError {
        problems: vec![Problem {
            path: path.into(),
            message: message.into(),
        }],
    }
}
/// The node-kind keys a flow node cannot be combined with.
const FLOW_CONFLICT_KEYS: [&str; 8] = [
    "id", "type", "prompt", "computed", "group", "nodes", "message", "hook",
];
/// Parses a `flow` action value, or returns the load message that names the
/// corrective form. `in_group` is true inside a group, which `{ skip: group }`
/// requires. This is the sole reading of the four-action set; the machine
/// schema states the same contract, and the loader reports it in prose.
fn flow_action_of(value: Option<&Value>, in_group: bool) -> Result<FlowAction, String> {
    let unknown =
        || "flow action must be stop, abort, dry-run, or { skip: rest | group }".to_string();
    match value {
        Some(Value::String(action)) => match action.as_str() {
            "stop" => Ok(FlowAction::Stop),
            "abort" => Ok(FlowAction::Abort),
            "dry-run" => Ok(FlowAction::DryRun),
            _ => Err(unknown()),
        },
        Some(Value::Object(map)) => match map.get("skip").and_then(Value::as_str) {
            Some("rest") => Ok(FlowAction::Skip(SkipScope::Rest)),
            Some("group") if in_group => Ok(FlowAction::Skip(SkipScope::Group)),
            Some("group") => Err("skip: group needs an enclosing group; use { skip: rest } to skip the rest of the interview".to_string()),
            _ => Err(unknown()),
        },
        _ => Err(unknown()),
    }
}
/// Checks the flow nodes of a raw interview before schema validation, so a flow
/// load error names the corrective form instead of a generic schema `oneOf`
/// failure. Recurses into group `nodes`.
fn check_flow(values: &[Value], in_group: bool, prefix: &str, problems: &mut Vec<Problem>) {
    for (i, value) in values.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        let Some(map) = value.as_object() else {
            continue;
        };
        if map.contains_key("flow") {
            for key in FLOW_CONFLICT_KEYS {
                if map.contains_key(key) {
                    problem(
                        problems,
                        format!("{path}.flow"),
                        format!("flow is a node kind; it cannot be combined with {key}"),
                    );
                }
            }
            if let Err(message) = flow_action_of(map.get("flow"), in_group) {
                problem(problems, format!("{path}.flow"), message);
            }
        } else if let Some(nodes) = map.get("nodes").and_then(Value::as_array) {
            check_flow(nodes, true, &format!("{path}.nodes"), problems);
        }
    }
}
#[derive(Deserialize)]
struct RawTemplate {
    name: String,
    description: Option<String>,
    source: Option<String>,
    data: Option<Map<String, Value>>,
    interview: Option<Vec<Value>>,
    files: Option<Vec<Value>>,
    ignore: Option<Vec<String>>,
    #[serde(rename = "static")]
    static_files: Option<Vec<String>>,
    hooks: Option<Vec<Value>>,
    messages: Option<Map<String, Value>>,
}
static SCHEMA: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema: Value = serde_norway::from_str(include_str!(
        "../docs/specifications/template-format.schema.yml"
    ))
    .expect("embedded schema YAML");
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .expect("embedded schema valid")
});
fn resolve(
    value: &mut serde_norway::Value,
    root: &Path,
    stack: &mut Vec<PathBuf>,
    path: &str,
    ps: &mut Vec<Problem>,
) {
    match value {
        serde_norway::Value::Tagged(tagged) => {
            if tagged.tag != "include" {
                problem(ps, path, format!("unknown tag: {}", tagged.tag));
                return;
            }
            let Some(name) = tagged.value.as_str() else {
                problem(ps, path, "include path must be string");
                return;
            };
            let candidate = root.join(name);
            let Ok(actual) = candidate.canonicalize() else {
                problem(ps, path, format!("include file not found: {name}"));
                return;
            };
            if !actual.starts_with(root) {
                problem(ps, path, "include escapes template root");
                return;
            }
            if stack.contains(&actual) {
                problem(ps, path, "include cycle");
                return;
            }
            let Ok(text) = fs::read_to_string(&actual) else {
                problem(ps, path, "include cannot be read");
                return;
            };
            let parsed: Result<serde_norway::Value, String> = match actual
                .extension()
                .and_then(|s| s.to_str())
            {
                Some("yml" | "yaml") => serde_norway::from_str(&text).map_err(|e| e.to_string()),
                Some("json") => serde_json::from_str::<Value>(&text)
                    .and_then(serde_json::to_value)
                    .map_err(|e| e.to_string())
                    .and_then(|v| serde_norway::to_value(v).map_err(|e| e.to_string())),
                Some("toml") => toml::from_str::<toml::Value>(&text)
                    .map_err(|e| e.to_string())
                    .and_then(|v| serde_norway::to_value(v).map_err(|e| e.to_string())),
                _ => {
                    problem(ps, path, "unsupported include extension");
                    return;
                }
            };
            match parsed {
                Ok(mut next) => {
                    stack.push(actual);
                    resolve(&mut next, root, stack, path, ps);
                    stack.pop();
                    *value = next;
                }
                Err(e) => problem(ps, path, e),
            }
        }
        serde_norway::Value::Sequence(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                resolve(item, root, stack, &format!("{path}[{i}]"), ps);
            }
        }
        serde_norway::Value::Mapping(items) => {
            for (key, item) in items.iter_mut() {
                resolve(
                    item,
                    root,
                    stack,
                    &format!("{path}.{}", key.as_str().unwrap_or("?")),
                    ps,
                );
            }
        }
        _ => {}
    }
}
struct Builder {
    problems: Vec<Problem>,
    seen: HashSet<String>,
    names: HashSet<String>,
    /// Every question id, computed id, and `data` key, wherever it is defined.
    answer_ids: HashSet<String>,
    root: PathBuf,
    /// The include-confinement owner, cloned from the one `Template::load`
    /// retains. A `files:` body compiles through it at load, so a missing or
    /// escaping partial fails before any interview starts.
    partials: Partials,
    /// Whether reserved context names collide with authored ids and resolve as
    /// available references (a current load) or not (a legacy staged load).
    reserves_context: bool,
}
/// Collects question and computed ids from raw interview nodes, including nested groups.
fn answer_ids(values: &[Value], ids: &mut HashSet<String>) {
    for map in values.iter().filter_map(Value::as_object) {
        if let Some(id) = map.get("id").and_then(Value::as_str) {
            ids.insert(id.into());
        }
        if let Some(nodes) = map.get("nodes").and_then(Value::as_array) {
            answer_ids(nodes, ids);
        }
    }
}
impl Builder {
    fn id(&mut self, value: Option<&Value>, path: &str) -> Option<Id> {
        match value
            .and_then(Value::as_str)
            .and_then(|s| Id::parse(s).ok())
        {
            Some(id) => {
                if !self.names.insert(id.as_str().into()) {
                    problem(&mut self.problems, path, format!("duplicate id: {id}"));
                }
                Some(id)
            }
            None => {
                problem(&mut self.problems, path, "invalid identifier");
                None
            }
        }
    }
    fn refs(&mut self, refs: &HashSet<String>, path: &str, local: &[&str]) {
        for name in refs {
            if !self.seen.contains(name)
                && !local.contains(&name.as_str())
                && !is_global(name)
                && !(self.reserves_context && is_reserved(name))
            {
                problem(
                    &mut self.problems,
                    path,
                    format!("id is not defined by an earlier node: {name}"),
                );
            }
        }
    }
    /// Rejects an authored id that collides with a reserved context name, for a
    /// current load. Every colliding location is named so the aggregate load
    /// error lists them all.
    fn reserve(&mut self, id: &Id, path: &str) {
        if self.reserves_context && is_reserved(id.as_str()) {
            problem(
                &mut self.problems,
                path,
                format!("\"{id}\" is a reserved Toha context name"),
            );
        }
    }
    fn tmpl(&mut self, value: Option<&Value>, path: &str) -> Option<Tmpl> {
        if value.is_some_and(|v| !v.is_string()) {
            problem(
                &mut self.problems,
                path,
                "wrong literal type: expected string",
            );
            return None;
        }
        value
            .and_then(Value::as_str)
            .and_then(|s| match Tmpl::compile(s.into()) {
                Ok(t) => {
                    self.refs(t.references(), path, &[]);
                    Some(t)
                }
                Err(e) => {
                    problem(&mut self.problems, path, e.to_string());
                    None
                }
            })
    }
    /// Compiles a `files:` rule body into an include-capable file body at load.
    /// `field` is `files[i].source` for load-error attribution; `label` is the
    /// source's root-relative path, used for include diagnostics and cycle
    /// paths. Called inside `bound(Some(&each), ...)`, so the unioned references
    /// — including those reached only through a selected partial — are checked
    /// against the `each` binding and every earlier id.
    fn file_tmpl(&mut self, source: String, label: &str, field: &str) -> Option<FileTmpl> {
        match self.partials.compile(source, label) {
            Ok(body) => {
                self.refs(body.references(), field, &[]);
                Some(body)
            }
            Err(error) => {
                problem(&mut self.problems, field, error.to_string());
                None
            }
        }
    }
    fn expr(&mut self, value: Option<&Value>, path: &str, local: &[&str]) -> Option<Expr> {
        value
            .and_then(Value::as_str)
            .and_then(|s| match Expr::compile(s.into()) {
                Ok(e) => {
                    self.refs(e.references(), path, local);
                    Some(e)
                }
                Err(e) => {
                    problem(&mut self.problems, path, e.to_string());
                    None
                }
            })
    }
    fn typed<T: serde::de::DeserializeOwned>(
        &mut self,
        value: Option<&Value>,
        path: &str,
    ) -> Option<Typed<T>> {
        match value {
            Some(Value::String(_)) => self.expr(value, path, &[]).map(Typed::Expr),
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(v) => Some(Typed::Literal(v)),
                Err(e) => {
                    problem(&mut self.problems, path, format!("wrong literal type: {e}"));
                    None
                }
            },
            None => None,
        }
    }
    fn confined_file(&mut self, value: &str, path: &str, executable: bool) -> Option<PathBuf> {
        if Path::new(value).is_absolute() {
            problem(
                &mut self.problems,
                path,
                "file path must be relative to template root",
            );
            return None;
        }
        let candidate = self.root.join(value);
        let actual = match candidate.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                problem(&mut self.problems, path, "file does not exist");
                return None;
            }
        };
        if !actual.starts_with(&self.root) || !actual.is_file() {
            problem(
                &mut self.problems,
                path,
                "file must stay inside template root and be a file",
            );
            return None;
        }
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            match actual.metadata() {
                Ok(m) if m.permissions().mode() & 0o111 != 0 => {}
                _ => {
                    problem(&mut self.problems, path, "script is not executable");
                    return None;
                }
            }
        }
        let _ = executable;
        Some(actual.strip_prefix(&self.root).unwrap().to_owned())
    }
    /// Parses `<expression> as <name>`. The schema has already checked the form.
    fn each(&mut self, value: Option<&Value>, path: &str) -> Option<Each> {
        let (expression, binding) = value.and_then(Value::as_str)?.rsplit_once(" as ")?;
        let binding = Id::parse(binding).ok()?;
        if self.answer_ids.contains(binding.as_str()) {
            problem(&mut self.problems, path, format!("duplicate id: {binding}"));
        }
        self.reserve(&binding, path);
        let expr = self.expr(Some(&Value::String(expression.into())), path, &[])?;
        Some(Each { expr, binding })
    }
    /// Runs `build` with the `each` binding visible to reference checks.
    fn bound<T>(&mut self, each: Option<&Each>, build: impl FnOnce(&mut Self) -> T) -> T {
        let inserted = each.is_some_and(|e| self.seen.insert(e.binding.as_str().into()));
        let result = build(self);
        if inserted {
            self.seen.remove(each.unwrap().binding.as_str());
        }
        result
    }
    fn hook(
        &mut self,
        map: &Map<String, Value>,
        path: &str,
        when: Option<Expr>,
    ) -> Option<HookNode> {
        let each = match map.get("each") {
            Some(value) => Some(self.each(Some(value), &format!("{path}.each"))?),
            None => None,
        };
        let command = self.bound(each.as_ref(), |b| b.hook_command(map, path))?;
        Some(HookNode {
            command,
            each,
            when,
        })
    }
    fn hook_command(&mut self, map: &Map<String, Value>, path: &str) -> Option<HookCommand> {
        let cwd = self.tmpl(map.get("cwd"), &format!("{path}.cwd"));
        let program = if let Some(run) = map.get("run").and_then(Value::as_array) {
            let args: Option<Vec<_>> = run
                .iter()
                .enumerate()
                .map(|(i, v)| self.tmpl(Some(v), &format!("{path}.run[{i}]")))
                .collect();
            HookProgram::Run(args?)
        } else {
            let value = map.get("script")?.as_str()?;
            let script = self.confined_file(value, &format!("{path}.script"), true)?;
            let args: Option<Vec<_>> = map
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, v)| self.tmpl(Some(v), &format!("{path}.args[{i}]")))
                .collect();
            HookProgram::Script {
                path: script,
                args: args?,
            }
        };
        Some(HookCommand { program, cwd })
    }
    fn globs(&mut self, patterns: Option<&Vec<String>>, path: &str) -> GlobSet {
        let mut builder = GlobSetBuilder::new();
        for (i, pattern) in patterns.into_iter().flatten().enumerate() {
            match GlobBuilder::new(pattern).literal_separator(true).build() {
                Ok(glob) => {
                    builder.add(glob);
                }
                Err(e) => problem(&mut self.problems, format!("{path}[{i}]"), e.to_string()),
            }
        }
        builder.build().expect("validated globs")
    }
    /// Parses a `flow` action value, recording the load message on failure.
    fn flow_action(
        &mut self,
        value: Option<&Value>,
        path: &str,
        in_group: bool,
    ) -> Option<FlowAction> {
        match flow_action_of(value, in_group) {
            Ok(action) => Some(action),
            Err(message) => {
                problem(&mut self.problems, path, message);
                None
            }
        }
    }
    fn nodes(&mut self, values: &[Value], prefix: &str, in_group: bool) -> Vec<Node> {
        let mut result = Vec::new();
        for (i, value) in values.iter().enumerate() {
            let path = format!("{prefix}[{i}]");
            let Some(map) = value.as_object() else {
                continue;
            };
            let when = self.expr(map.get("when"), &format!("{path}.when"), &[]);
            if map.contains_key("flow") {
                // `flow` is a node kind; it carries only `when` and `label`.
                // The load rules are also enforced before schema validation by
                // `check_flow`, so a bad flow node is reported there first.
                for key in FLOW_CONFLICT_KEYS {
                    if map.contains_key(key) {
                        problem(
                            &mut self.problems,
                            format!("{path}.flow"),
                            format!("flow is a node kind; it cannot be combined with {key}"),
                        );
                    }
                }
                if let Some(action) =
                    self.flow_action(map.get("flow"), &format!("{path}.flow"), in_group)
                {
                    let label = map.get("label").and_then(Value::as_str).map(str::to_owned);
                    result.push(Node::Flow(FlowNode {
                        when,
                        label,
                        action,
                    }));
                }
                continue;
            }
            if map.contains_key("hook") {
                if let Some(hook) = map
                    .get("hook")
                    .and_then(Value::as_object)
                    .and_then(|m| self.hook(m, &format!("{path}.hook"), when))
                {
                    result.push(Node::Hook(hook));
                }
                continue;
            }
            if map.contains_key("group") {
                let name = self.id(map.get("group"), &format!("{path}.group"));
                let children = map
                    .get("nodes")
                    .and_then(Value::as_array)
                    .map(|v| self.nodes(v, &format!("{path}.nodes"), true))
                    .unwrap_or_default();
                if let Some(name) = name {
                    result.push(Node::Group(Group {
                        name,
                        nodes: children,
                        when,
                    }));
                }
                continue;
            }
            if map.contains_key("message") {
                if let Some(text) = self.tmpl(map.get("message"), &format!("{path}.message")) {
                    result.push(Node::Message(Message { text, when }));
                }
                continue;
            }
            let id = self.id(map.get("id"), &format!("{path}.id"));
            if let Some(id) = &id {
                self.reserve(id, &format!("{path}.id"));
            }
            if map.contains_key("computed") {
                let expr = self.expr(map.get("computed"), &format!("{path}.computed"), &[]);
                if let (Some(id), Some(expr)) = (id, expr) {
                    self.seen.insert(id.as_str().into());
                    result.push(Node::Computed(Computed { id, expr, when }));
                }
                continue;
            }
            let Some(kind_name) = map.get("type").and_then(Value::as_str) else {
                continue;
            };
            if map.contains_key("loop") && kind_name != "text" {
                problem(
                    &mut self.problems,
                    format!("{path}.loop"),
                    "loop only allowed on text",
                );
            }
            if map.contains_key("options") && !["select", "multiselect"].contains(&kind_name) {
                problem(
                    &mut self.problems,
                    format!("{path}.options"),
                    "options only allowed on select or multiselect",
                );
            }
            if ["select", "multiselect"].contains(&kind_name) && !map.contains_key("options") {
                problem(
                    &mut self.problems,
                    format!("{path}.options"),
                    "options required",
                );
            }
            let prompt = self.tmpl(map.get("prompt"), &format!("{path}.prompt"));
            let description = self.tmpl(map.get("description"), &format!("{path}.description"));
            let placeholder = self.tmpl(map.get("placeholder"), &format!("{path}.placeholder"));
            let required = self
                .typed(map.get("required"), &format!("{path}.required"))
                .unwrap_or(Typed::Literal(false));
            let vm = map.get("validate").and_then(Value::as_object);
            let min = self.typed(
                vm.and_then(|m| m.get("min")),
                &format!("{path}.validate.min"),
            );
            let max = self.typed(
                vm.and_then(|m| m.get("max")),
                &format!("{path}.validate.max"),
            );
            let regex = vm
                .and_then(|m| m.get("regex"))
                .and_then(Value::as_str)
                .and_then(|s| match Regex::new(s) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        problem(
                            &mut self.problems,
                            format!("{path}.validate.regex"),
                            e.to_string(),
                        );
                        None
                    }
                });
            let format = self.expr(map.get("format"), &format!("{path}.format"), &["value"]);
            let default = map.get("default");
            let kind = match kind_name {
                "text" if map.contains_key("loop") => {
                    let lm = map.get("loop").and_then(Value::as_object);
                    QuestionKind::TextLoop {
                        default: self.typed(default, &format!("{path}.default")),
                        min: self.typed(lm.and_then(|m| m.get("min")), &format!("{path}.loop.min")),
                        max: self.typed(lm.and_then(|m| m.get("max")), &format!("{path}.loop.max")),
                    }
                }
                "text" => QuestionKind::Text {
                    default: self.tmpl(default, &format!("{path}.default")),
                },
                "multiline" => QuestionKind::Multiline {
                    default: self.tmpl(default, &format!("{path}.default")),
                },
                "confirm" => QuestionKind::Confirm {
                    default: self.typed(default, &format!("{path}.default")),
                },
                "select" => {
                    let options = self
                        .typed(map.get("options"), &format!("{path}.options"))
                        .unwrap_or(Typed::Literal(vec![]));
                    QuestionKind::Select {
                        options,
                        default: self.tmpl(default, &format!("{path}.default")),
                    }
                }
                "multiselect" => {
                    let options = self
                        .typed(map.get("options"), &format!("{path}.options"))
                        .unwrap_or(Typed::Literal(vec![]));
                    QuestionKind::MultiSelect {
                        options,
                        default: self.typed(default, &format!("{path}.default")),
                    }
                }
                _ => continue,
            };
            if let (Some(id), Some(prompt)) = (id, prompt) {
                self.seen.insert(id.as_str().into());
                result.push(Node::Question(Question {
                    id,
                    prompt,
                    description,
                    placeholder,
                    required,
                    kind,
                    validate: Validate { min, max, regex },
                    format,
                    when,
                }));
            }
        }
        result
    }
}
impl Template {
    /// Loads a template in the current context mode: the seventeen reserved
    /// names collide with authored ids and resolve as available references.
    pub fn load(folder: &Path) -> Result<Self, LoadError> {
        Self::load_with(folder, true)
    }

    /// Loads a template in legacy staged mode: the seventeen names are neither
    /// reserved nor available, so a pre-context authored identifier is not
    /// turned into a collision while its old stage is completed.
    pub fn load_legacy(folder: &Path) -> Result<Self, LoadError> {
        Self::load_with(folder, false)
    }

    fn load_with(folder: &Path, reserves_context: bool) -> Result<Self, LoadError> {
        let root = folder
            .canonicalize()
            .map_err(|e| error("template.yml", e.to_string()))?;
        let text = fs::read_to_string(root.join("template.yml"))
            .map_err(|e| error("template.yml", e.to_string()))?;
        let mut yaml: serde_norway::Value =
            serde_norway::from_str(&text).map_err(|e| error("template.yml", e.to_string()))?;
        let mut problems = Vec::new();
        resolve(
            &mut yaml,
            &root,
            &mut vec![root.join("template.yml")],
            "template.yml",
            &mut problems,
        );
        if !problems.is_empty() {
            return Err(LoadError { problems });
        }
        let doc: Value =
            serde_json::to_value(yaml).map_err(|e| error("template.yml", e.to_string()))?;
        // Report flow load rules before the schema, so a bad flow node names
        // the corrective form rather than a generic schema `oneOf` failure.
        if let Some(interview) = doc.get("interview").and_then(Value::as_array) {
            check_flow(interview, false, "template.yml/interview", &mut problems);
        }
        if !problems.is_empty() {
            return Err(LoadError { problems });
        }
        for e in SCHEMA.iter_errors(&doc) {
            problem(
                &mut problems,
                format!("template.yml{}", e.instance_path()),
                e.to_string(),
            );
        }
        if !problems.is_empty() {
            return Err(LoadError { problems });
        }
        let raw: RawTemplate =
            serde_json::from_value(doc).map_err(|e| error("template.yml", e.to_string()))?;
        let source = raw.source.as_deref().unwrap_or("template");
        let source_dir = root.join(source);
        if !source_dir.is_dir() {
            problem(&mut problems, "source", "source directory does not exist");
        }
        if Path::new(source).is_absolute()
            || Path::new(source)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || source_dir
                .canonicalize()
                .is_ok_and(|p| !p.starts_with(&root))
        {
            problem(
                &mut problems,
                "source",
                "source must stay inside template root",
            );
        }
        let source_dir = source_dir.canonicalize().unwrap_or(source_dir);
        // The one include-confinement owner, created from the canonical root and
        // shared by the `files:` bodies (compiled here) and the source-tree
        // bodies (compiled below); it is retained on the returned Template.
        let partials = Partials::rooted(&root);
        let mut b = Builder {
            problems,
            seen: HashSet::new(),
            names: HashSet::new(),
            answer_ids: HashSet::new(),
            root: root.clone(),
            partials: partials.clone(),
            reserves_context,
        };
        if let Some(values) = &raw.data {
            b.answer_ids.extend(values.keys().cloned());
        }
        answer_ids(
            raw.interview.as_deref().unwrap_or_default(),
            &mut b.answer_ids,
        );
        let mut data = IndexMap::new();
        if let Some(values) = &raw.data {
            for (key, value) in values {
                if let Some(id) = b.id(Some(&Value::String(key.clone())), &format!("data.{key}")) {
                    b.reserve(&id, &format!("data.{key}"));
                    b.seen.insert(id.as_str().into());
                    data.insert(id, value.clone());
                }
            }
        }
        let interview = b.nodes(
            raw.interview.as_deref().unwrap_or_default(),
            "interview",
            false,
        );
        // Output expressions see the complete interview, including computed answers.
        fn collect_answers(nodes: &[Node], names: &mut HashSet<String>) {
            for node in nodes {
                match node {
                    Node::Question(q) => {
                        names.insert(q.id.as_str().into());
                    }
                    Node::Computed(c) => {
                        names.insert(c.id.as_str().into());
                    }
                    Node::Group(g) => collect_answers(&g.nodes, names),
                    Node::Message(_) | Node::Hook(_) | Node::Flow(_) => {}
                }
            }
        }
        b.seen = data.keys().map(|id| id.as_str().to_owned()).collect();
        collect_answers(&interview, &mut b.seen);
        let ignore = b.globs(raw.ignore.as_ref(), "ignore");
        let static_files = b.globs(raw.static_files.as_ref(), "static");
        let mut files = Vec::new();
        for (i, value) in raw.files.as_deref().unwrap_or_default().iter().enumerate() {
            let path = format!("files[{i}]");
            let Some(map) = value.as_object() else {
                continue;
            };
            let each = b.each(map.get("each"), &format!("{path}.each"));
            let when = b.expr(map.get("when"), &format!("{path}.when"), &[]);
            let Some(source) = map
                .get("source")
                .and_then(Value::as_str)
                .and_then(|v| b.confined_file(v, &format!("{path}.source"), false))
            else {
                continue;
            };
            let Some(each) = each else {
                continue;
            };
            let (target, body) = b.bound(Some(&each), |b| {
                let target = b.tmpl(map.get("path"), &format!("{path}.path"));
                let body = match fs::read_to_string(root.join(&source)) {
                    Ok(content) => {
                        let label = source.to_string_lossy().replace('\\', "/");
                        b.file_tmpl(content, &label, &format!("{path}.source"))
                    }
                    // A non-UTF-8 body is tolerated at load; planning surfaces
                    // it on its existing read path.
                    Err(error) if error.kind() == std::io::ErrorKind::InvalidData => None,
                    Err(error) => {
                        problem(&mut b.problems, format!("{path}.source"), error.to_string());
                        None
                    }
                };
                (target, body)
            });
            if let Some(path) = target {
                files.push(FileRule {
                    each,
                    source,
                    path,
                    when,
                    body,
                });
            }
        }
        let hooks: Vec<HookNode> = raw
            .hooks
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let map = v.as_object()?;
                let when = b.expr(map.get("when"), &format!("hooks[{i}].when"), &[]);
                b.hook(map, &format!("hooks[{i}]"), when)
            })
            .collect();
        let messages = raw.messages.as_ref();
        let messages = ApplyMessages {
            before_apply: b.tmpl(
                messages.and_then(|m| m.get("before-apply")),
                "messages.before-apply",
            ),
            after_apply: b.tmpl(
                messages.and_then(|m| m.get("after-apply")),
                "messages.after-apply",
            ),
        };
        if !b.problems.is_empty() {
            return Err(LoadError {
                problems: b.problems,
            });
        }
        let mut interview_hook_nodes = Vec::new();
        collect_hook_nodes(
            raw.interview.as_deref().unwrap_or_default(),
            &mut interview_hook_nodes,
        );
        let top_hook_nodes = raw.hooks.clone().unwrap_or_default();
        // Compile and retain the source-tree render program. Syntax errors in a
        // non-static body or a path segment become attributable load errors,
        // rather than first appearing during planning. Bodies are not
        // reference-checked: MiniJinja renders an undefined value leniently, so
        // a source body may reference an id that is only conditionally present.
        let mut render_program = Vec::new();
        compile_source_tree(
            &source_dir,
            &source_dir,
            &root,
            &partials,
            &ignore,
            &static_files,
            &mut render_program,
            &mut b.problems,
        );
        if !b.problems.is_empty() {
            return Err(LoadError {
                problems: b.problems,
            });
        }
        let environment_need =
            compute_environment_need(&interview, &files, &hooks, &messages, &render_program);
        Ok(Self {
            name: raw.name,
            description: raw.description,
            source_dir,
            data,
            interview,
            root,
            partials,
            files,
            ignore,
            static_files,
            hooks,
            messages,
            interview_hook_nodes,
            top_hook_nodes,
            render_program,
            reserves_context,
            environment_need,
        })
    }
}

/// Recursively compiles the source tree in deterministic (sorted) order,
/// retaining each file's compiled path segments and body. Mirrors planning's
/// walk: it skips the root `template.yml` and `ignore` matches, and rejects a
/// source symlink.
fn compile_source_tree(
    root: &Path,
    dir: &Path,
    template_root: &Path,
    partials: &Partials,
    ignore: &GlobSet,
    static_files: &GlobSet,
    out: &mut Vec<SourceEntry>,
    problems: &mut Vec<Problem>,
) {
    if !dir.exists() {
        return;
    }
    let mut entries = match fs::read_dir(dir) {
        Ok(entries) => match entries.collect::<Result<Vec<_>, _>>() {
            Ok(entries) => entries,
            Err(e) => {
                problem(problems, dir.display().to_string(), e.to_string());
                return;
            }
        },
        Err(e) => {
            problem(problems, dir.display().to_string(), e.to_string());
            return;
        }
    };
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let source = entry.path();
        let relative = source.strip_prefix(root).unwrap().to_owned();
        if root == template_root && dir == root && entry.file_name() == "template.yml" {
            continue;
        }
        if ignore.is_match(&relative) {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(e) => {
                problem(problems, source.display().to_string(), e.to_string());
                continue;
            }
        };
        if file_type.is_symlink() {
            problem(
                problems,
                source.display().to_string(),
                "source symlink not supported",
            );
            continue;
        }
        if file_type.is_dir() {
            compile_source_tree(
                root,
                &source,
                template_root,
                partials,
                ignore,
                static_files,
                out,
                problems,
            );
            continue;
        }
        let path = source.display().to_string();
        let mut segments = Vec::new();
        for component in relative.components() {
            let text = component.as_os_str().to_string_lossy();
            match Tmpl::compile(text.into_owned()) {
                Ok(segment) => segments.push(segment),
                Err(e) => problem(problems, format!("{path} (path)"), e.to_string()),
            }
        }
        let body = if static_files.is_match(&relative) {
            SourceBody::Static
        } else {
            // The include label is the file's path relative to the template
            // root, in the `/`-separated template namespace — the same string
            // any file would use to include it.
            let label = source
                .strip_prefix(template_root)
                .unwrap_or(&relative)
                .to_string_lossy()
                .replace('\\', "/");
            match fs::read(&source) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => match partials.compile(text, &label) {
                        Ok(body) => SourceBody::Rendered(body),
                        Err(e) => {
                            problem(problems, path.clone(), e.to_string());
                            SourceBody::NonUtf8
                        }
                    },
                    Err(_) => SourceBody::NonUtf8,
                },
                Err(e) => {
                    problem(problems, path.clone(), e.to_string());
                    SourceBody::NonUtf8
                }
            }
        };
        out.push(SourceEntry {
            relative,
            source,
            segments,
            body,
        });
    }
}

/// The first surface, in deterministic order, whose compiled Jinja can observe
/// a fixed environment value, or `EnvironmentNeed::None`.
fn compute_environment_need(
    interview: &[Node],
    files: &[FileRule],
    hooks: &[HookNode],
    messages: &ApplyMessages,
    render_program: &[SourceEntry],
) -> EnvironmentNeed {
    let mut find = NeedScan::default();
    find.nodes(interview, "interview");
    for (i, hook) in hooks.iter().enumerate() {
        find.hook(hook, &format!("hooks[{i}]"));
    }
    if let Some(t) = &messages.before_apply {
        find.tmpl(t, "messages.before-apply");
    }
    if let Some(t) = &messages.after_apply {
        find.tmpl(t, "messages.after-apply");
    }
    for (i, rule) in files.iter().enumerate() {
        let path = format!("files[{i}]");
        find.opt_expr(rule.when.as_ref(), &format!("{path}.when"));
        find.expr(&rule.each.expr, &format!("{path}.each"));
        find.tmpl(&rule.path, &format!("{path}.path"));
        if let Some(body) = &rule.body {
            find.file_tmpl(body, &format!("{path}.source"));
        }
    }
    for entry in render_program {
        let label = entry.source.display().to_string();
        for segment in &entry.segments {
            find.tmpl(segment, &format!("{label} (path)"));
        }
        if let SourceBody::Rendered(body) = &entry.body {
            find.file_tmpl(body, &label);
        }
    }
    match find.found {
        Some(origin) => EnvironmentNeed::Needed(origin),
        None => EnvironmentNeed::None,
    }
}

/// Accumulates the first environment-need location across the render program.
#[derive(Default)]
struct NeedScan {
    found: Option<RenderOrigin>,
}
impl NeedScan {
    fn mark(&mut self, label: &str, needed: bool) {
        if needed && self.found.is_none() {
            self.found = Some(RenderOrigin::new(label));
        }
    }
    fn tmpl(&mut self, tmpl: &Tmpl, label: &str) {
        self.mark(label, tmpl.needs_environment());
    }
    /// A file body's need is transitive: it is true when the body or any partial
    /// it selects can observe a fixed value, so an environment read reached only
    /// through a nested include still marks the load.
    fn file_tmpl(&mut self, body: &FileTmpl, label: &str) {
        self.mark(label, body.needs_environment());
    }
    fn opt_tmpl(&mut self, tmpl: Option<&Tmpl>, label: &str) {
        if let Some(tmpl) = tmpl {
            self.tmpl(tmpl, label);
        }
    }
    fn expr(&mut self, expr: &Expr, label: &str) {
        self.mark(label, expr.needs_environment());
    }
    fn opt_expr(&mut self, expr: Option<&Expr>, label: &str) {
        if let Some(expr) = expr {
            self.expr(expr, label);
        }
    }
    fn typed<T: Clone + serde::de::DeserializeOwned>(&mut self, typed: &Typed<T>, label: &str) {
        self.mark(label, typed.needs_environment());
    }
    fn opt_typed<T: Clone + serde::de::DeserializeOwned>(
        &mut self,
        typed: Option<&Typed<T>>,
        label: &str,
    ) {
        if let Some(typed) = typed {
            self.typed(typed, label);
        }
    }
    fn hook(&mut self, hook: &HookNode, label: &str) {
        self.opt_expr(hook.when.as_ref(), &format!("{label}.when"));
        if let Some(each) = &hook.each {
            self.expr(&each.expr, &format!("{label}.each"));
        }
        self.opt_tmpl(hook.command.cwd.as_ref(), &format!("{label}.cwd"));
        match &hook.command.program {
            HookProgram::Run(args) => {
                for (i, arg) in args.iter().enumerate() {
                    self.tmpl(arg, &format!("{label}.run[{i}]"));
                }
            }
            HookProgram::Script { args, .. } => {
                for (i, arg) in args.iter().enumerate() {
                    self.tmpl(arg, &format!("{label}.args[{i}]"));
                }
            }
        }
    }
    fn nodes(&mut self, nodes: &[Node], prefix: &str) {
        for (i, node) in nodes.iter().enumerate() {
            let label = format!("{prefix}[{i}]");
            match node {
                Node::Question(q) => {
                    self.tmpl(&q.prompt, &format!("{label}.prompt"));
                    self.opt_tmpl(q.description.as_ref(), &format!("{label}.description"));
                    self.opt_tmpl(q.placeholder.as_ref(), &format!("{label}.placeholder"));
                    self.typed(&q.required, &format!("{label}.required"));
                    self.opt_expr(q.when.as_ref(), &format!("{label}.when"));
                    self.opt_typed(q.validate.min.as_ref(), &format!("{label}.validate.min"));
                    self.opt_typed(q.validate.max.as_ref(), &format!("{label}.validate.max"));
                    self.opt_expr(q.format.as_ref(), &format!("{label}.format"));
                    match &q.kind {
                        QuestionKind::Text { default } | QuestionKind::Multiline { default } => {
                            self.opt_tmpl(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::Confirm { default } => {
                            self.opt_typed(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::Select { options, default } => {
                            self.typed(options, &format!("{label}.options"));
                            self.opt_tmpl(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::MultiSelect { options, default } => {
                            self.typed(options, &format!("{label}.options"));
                            self.opt_typed(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::TextLoop { default, min, max } => {
                            self.opt_typed(default.as_ref(), &format!("{label}.default"));
                            self.opt_typed(min.as_ref(), &format!("{label}.loop.min"));
                            self.opt_typed(max.as_ref(), &format!("{label}.loop.max"));
                        }
                    }
                }
                Node::Computed(c) => {
                    self.opt_expr(c.when.as_ref(), &format!("{label}.when"));
                    self.expr(&c.expr, &format!("{label}.computed"));
                }
                Node::Group(g) => {
                    self.opt_expr(g.when.as_ref(), &format!("{label}.when"));
                    self.nodes(&g.nodes, &format!("{label}.nodes"));
                }
                Node::Message(m) => {
                    self.opt_expr(m.when.as_ref(), &format!("{label}.when"));
                    self.tmpl(&m.text, &format!("{label}.message"));
                }
                Node::Hook(h) => self.hook(h, label.as_str()),
                Node::Flow(f) => self.opt_expr(f.when.as_ref(), &format!("{label}.when")),
            }
        }
    }
}
