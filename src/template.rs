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
    collections::{HashMap, HashSet},
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
    pub inject: Vec<InjectRule>,
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
    /// Every hook-result name a template author may read: each producer `id` and
    /// each declared `status_id`. A reference whose root is in this set is a hook
    /// result read, resolved by `settle_hook_results` at load, not an ordinary
    /// undefined id. Used at plan time to decide whether a hook surface must
    /// render after its producers run.
    result_ids: HashSet<String>,
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
    /// Opt-in result identity. `None` is a hook exactly as before this feature:
    /// it produces no readable result and is byte-identical in every path.
    pub id: Option<Id>,
    /// The streams piped to Toha (and so readable), empty unless a stream is
    /// read. An uncaptured stream inherits the terminal as before.
    pub capture: Capture,
    /// Literal opt-in; a nonzero exit is tolerated. Only ever true with a read
    /// `id` (text) or a read `status_id` (JSON).
    pub allow_failure: bool,
    /// `parse: json`: the parsed stdout is read directly at `id`; execution
    /// metadata moves to `status_id`.
    pub parse_json: bool,
    /// The author-declared name that carries `{exit_code, stdout, stderr,
    /// parsed}` for a `parse: json` hook.
    pub status_id: Option<Id>,
}
/// The streams a hook pipes to Toha so a later surface can read them. An
/// uncaptured stream inherits the terminal exactly as before this feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capture {
    pub stdout: bool,
    pub stderr: bool,
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
/// One `inject` rule: a visible managed region in a non-JSON text target, or a
/// typed value at a path in a JSON-family target. Rendered into a
/// `crate::inject::PlannedEdit` at plan time.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum InjectRule {
    Region(RegionRule),
    Struct(StructRule),
}
/// A visible-region rule. Its body comes from an inline `content` template or a
/// `source` support file. The marker style is an explicit override or inferred
/// from the target extension at plan time.
#[derive(Debug)]
pub struct RegionRule {
    pub into: Tmpl,
    pub region: crate::inject::RegionKey,
    pub body: RegionBody,
    pub anchor: Option<AnchorRule>,
    pub marker: Option<crate::inject::MarkerStyle>,
    pub create: bool,
    pub when: Option<Expr>,
}
#[derive(Debug)]
pub enum RegionBody {
    Content(Tmpl),
    Source { path: PathBuf, body: FileTmpl },
}
#[derive(Debug)]
pub struct AnchorRule {
    pub after: Tmpl,
    pub occurrence: crate::inject::Occurrence,
}
/// A structured JSON-family rule. `path` is parsed at load; `value` renders its
/// string leaves at plan time and keeps every other JSON type.
#[derive(Debug)]
pub struct StructRule {
    pub into: Tmpl,
    pub path: crate::inject::JsonPath,
    pub value: InjectValue,
    pub create: bool,
    pub when: Option<Expr>,
}
/// A JSON-compatible value whose string leaves are templates. Rendered to a
/// `serde_json::Value` at plan time.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum InjectValue {
    Template(Tmpl),
    Literal(Value),
    Array(Vec<InjectValue>),
    Object(Vec<(String, InjectValue)>),
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
    inject: Option<Vec<Value>>,
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
    /// Every hook `id` and `status_id` declared anywhere, pre-scanned before
    /// parsing so a reference to one is recognised as a hook-result read rather
    /// than an undefined id, on any surface. `settle_hook_results` then enforces
    /// which surfaces may actually read one.
    result_ids: HashSet<String>,
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
/// Records a hook command object's `id` and `status-id` string values.
fn hook_result_ids(command: &Value, ids: &mut HashSet<String>) {
    for key in ["id", "status-id"] {
        if let Some(name) = command.get(key).and_then(Value::as_str) {
            ids.insert(name.into());
        }
    }
}
/// Pre-scans every raw hook — top-level and interview (through groups) — for the
/// `id` and `status-id` names, so a reference to one is a hook-result read rather
/// than an undefined id on any surface.
fn collect_result_ids(interview: &[Value], hooks: &[Value], ids: &mut HashSet<String>) {
    fn interview_hooks(values: &[Value], ids: &mut HashSet<String>) {
        for map in values.iter().filter_map(Value::as_object) {
            if let Some(hook) = map.get("hook") {
                hook_result_ids(hook, ids);
            } else if let Some(nodes) = map.get("nodes").and_then(Value::as_array) {
                interview_hooks(nodes, ids);
            }
        }
    }
    interview_hooks(interview, ids);
    for command in hooks {
        hook_result_ids(command, ids);
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
                // A hook `id`/`status_id` is a hook-result read, not an undefined
                // id, on every surface. `settle_hook_results` decides whether the
                // surface may read one; here it only stops the undefined-id error.
                && !self.result_ids.contains(name)
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
    /// Parses an optional hook `id`/`status-id`. A present name joins the authored
    /// id space: it is registered for duplicate detection (so a second use of the
    /// name, including `status-id` equal to `id`, is `duplicate id`) and checked
    /// against the reserved context names.
    fn optional_id(&mut self, value: Option<&Value>, path: &str) -> Option<Id> {
        let value = value?;
        match value.as_str().map(Id::parse) {
            Some(Ok(id)) => {
                if !self.names.insert(id.as_str().into()) {
                    problem(&mut self.problems, path, format!("duplicate id: {id}"));
                }
                self.reserve(&id, path);
                Some(id)
            }
            _ => {
                problem(&mut self.problems, path, "invalid identifier");
                None
            }
        }
    }
    /// Parses a hook `capture` list into the two-stream flags. The schema already
    /// bounds it to a unique, non-empty list of `stdout`/`stderr`.
    fn capture(&mut self, value: Option<&Value>, path: &str) -> Capture {
        let mut capture = Capture::default();
        for item in value.and_then(Value::as_array).into_iter().flatten() {
            match item.as_str() {
                Some("stdout") => capture.stdout = true,
                Some("stderr") => capture.stderr = true,
                _ => problem(
                    &mut self.problems,
                    path,
                    "capture entries must be stdout or stderr",
                ),
            }
        }
        capture
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
        // Opt-in result fields. Order matters: `id` registers before `status-id`,
        // so `status-id` equal to `id` is caught as a duplicate.
        let id = self.optional_id(map.get("id"), &format!("{path}.id"));
        let capture = self.capture(map.get("capture"), &format!("{path}.capture"));
        let allow_failure = map
            .get("allow-failure")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let parse_json = matches!(map.get("parse").and_then(Value::as_str), Some("json"));
        let status_id = self.optional_id(map.get("status-id"), &format!("{path}.status-id"));
        // A hook with `each` runs once per item and records no single result.
        if id.is_some() && each.is_some() {
            problem(
                &mut self.problems,
                format!("{path}.id"),
                "a hook with each cannot declare id",
            );
        }
        let has_stream = capture.stdout || capture.stderr;
        if has_stream && id.is_none() {
            problem(
                &mut self.problems,
                format!("{path}.capture"),
                "capture requires id",
            );
        }
        if allow_failure && id.is_none() {
            problem(
                &mut self.problems,
                format!("{path}.allow-failure"),
                "allow-failure requires id",
            );
        }
        if parse_json && id.is_none() {
            problem(
                &mut self.problems,
                format!("{path}.parse"),
                "parse requires id",
            );
        }
        if status_id.is_some() && id.is_none() {
            problem(
                &mut self.problems,
                format!("{path}.status-id"),
                "status-id requires id",
            );
        }
        if parse_json && !capture.stdout {
            problem(
                &mut self.problems,
                format!("{path}.parse"),
                "parse: json requires capture: [stdout]",
            );
        }
        if status_id.is_some() && !parse_json {
            problem(
                &mut self.problems,
                format!("{path}.status-id"),
                "status-id requires parse: json",
            );
        }
        Some(HookNode {
            command,
            each,
            when,
            id,
            capture,
            allow_failure,
            parse_json,
            status_id,
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
    /// Parses one `inject` rule into a region or structured rule. The schema has
    /// already checked that exactly one of `region`/`struct` is present and, for
    /// a region, exactly one of `content`/`source`.
    fn inject_rule(&mut self, map: &Map<String, Value>, path: &str) -> Option<InjectRule> {
        let into = self.tmpl(map.get("into"), &format!("{path}.into"))?;
        let when = self.expr(map.get("when"), &format!("{path}.when"), &[]);
        let create = map.get("create").and_then(Value::as_bool).unwrap_or(false);
        if map.contains_key("struct") {
            let object = map.get("struct").and_then(Value::as_object);
            let path_value = object
                .and_then(|m| m.get("path"))
                .and_then(Value::as_str)
                .and_then(|value| match crate::inject::JsonPath::parse(value) {
                    Ok(parsed) => Some(parsed),
                    Err(message) => {
                        problem(&mut self.problems, format!("{path}.struct.path"), message);
                        None
                    }
                })?;
            let value = self.inject_value(
                object.and_then(|m| m.get("value"))?,
                &format!("{path}.struct.value"),
            )?;
            return Some(InjectRule::Struct(StructRule {
                into,
                path: path_value,
                value,
                create,
                when,
            }));
        }
        let region = map
            .get("region")
            .and_then(Value::as_str)
            .and_then(|value| match crate::inject::RegionKey::parse(value) {
                Ok(key) => Some(key),
                Err(message) => {
                    problem(&mut self.problems, format!("{path}.region"), message);
                    None
                }
            })?;
        let marker = self.marker_override(map.get("marker"), &format!("{path}.marker"));
        let anchor = match map.get("anchor") {
            Some(value) => Some(self.anchor_rule(value, &format!("{path}.anchor"))?),
            None => None,
        };
        let body = if map.contains_key("content") {
            RegionBody::Content(self.tmpl(map.get("content"), &format!("{path}.content"))?)
        } else {
            let source = map.get("source").and_then(Value::as_str)?;
            let source = self.confined_file(source, &format!("{path}.source"), false)?;
            let text = match fs::read_to_string(self.root.join(&source)) {
                Ok(text) => text,
                Err(error) => {
                    problem(
                        &mut self.problems,
                        format!("{path}.source"),
                        error.to_string(),
                    );
                    return None;
                }
            };
            let label = source.to_string_lossy().replace('\\', "/");
            let body = self.file_tmpl(text, &label, &format!("{path}.source"))?;
            RegionBody::Source { path: source, body }
        };
        Some(InjectRule::Region(RegionRule {
            into,
            region,
            body,
            anchor,
            marker,
            create,
            when,
        }))
    }
    /// Parses an explicit marker override: a string is a line-comment prefix; an
    /// object `{ open, close }` is a block comment.
    fn marker_override(
        &mut self,
        value: Option<&Value>,
        path: &str,
    ) -> Option<crate::inject::MarkerStyle> {
        match value {
            None => None,
            Some(Value::String(prefix)) => Some(crate::inject::MarkerStyle::line(prefix)),
            Some(Value::Object(map)) => {
                let open = map.get("open").and_then(Value::as_str);
                let close = map.get("close").and_then(Value::as_str);
                match (open, close) {
                    (Some(open), Some(close)) => {
                        Some(crate::inject::MarkerStyle::block(open, close))
                    }
                    _ => {
                        problem(
                            &mut self.problems,
                            path,
                            "marker object needs open and close",
                        );
                        None
                    }
                }
            }
            Some(_) => {
                problem(
                    &mut self.problems,
                    path,
                    "marker must be a string or { open, close }",
                );
                None
            }
        }
    }
    fn anchor_rule(&mut self, value: &Value, path: &str) -> Option<AnchorRule> {
        let map = value.as_object()?;
        let after = self.tmpl(map.get("after"), &format!("{path}.after"))?;
        let occurrence = match map.get("occurrence").and_then(Value::as_str) {
            None | Some("only") => crate::inject::Occurrence::Only,
            Some("first") => crate::inject::Occurrence::First,
            Some("last") => crate::inject::Occurrence::Last,
            Some(_) => {
                problem(
                    &mut self.problems,
                    format!("{path}.occurrence"),
                    "occurrence must be only, first, or last",
                );
                return None;
            }
        };
        Some(AnchorRule { after, occurrence })
    }
    /// Parses a JSON-compatible value whose string leaves are templates.
    fn inject_value(&mut self, value: &Value, path: &str) -> Option<InjectValue> {
        match value {
            Value::String(_) => self.tmpl(Some(value), path).map(InjectValue::Template),
            Value::Bool(_) | Value::Number(_) | Value::Null => {
                Some(InjectValue::Literal(value.clone()))
            }
            Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(self.inject_value(item, &format!("{path}[{i}]"))?);
                }
                Some(InjectValue::Array(out))
            }
            Value::Object(map) => {
                let mut out = Vec::with_capacity(map.len());
                for (key, item) in map {
                    out.push((
                        key.clone(),
                        self.inject_value(item, &format!("{path}.{key}"))?,
                    ));
                }
                Some(InjectValue::Object(out))
            }
        }
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
            result_ids: HashSet::new(),
        };
        if let Some(values) = &raw.data {
            b.answer_ids.extend(values.keys().cloned());
        }
        answer_ids(
            raw.interview.as_deref().unwrap_or_default(),
            &mut b.answer_ids,
        );
        // Pre-scan every hook `id`/`status-id` so a reference to one is a
        // hook-result read on any surface, not an undefined id.
        collect_result_ids(
            raw.interview.as_deref().unwrap_or_default(),
            raw.hooks.as_deref().unwrap_or_default(),
            &mut b.result_ids,
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
        let mut inject = Vec::new();
        for (i, value) in raw.inject.as_deref().unwrap_or_default().iter().enumerate() {
            let path = format!("inject[{i}]");
            let Some(map) = value.as_object() else {
                continue;
            };
            if let Some(rule) = b.inject_rule(map, &path) {
                inject.push(rule);
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
            &SourceScan {
                source_root: &source_dir,
                template_root: &root,
                partials: &partials,
                ignore: &ignore,
                static_files: &static_files,
            },
            &source_dir,
            &mut render_program,
            &mut b.problems,
        );
        if !b.problems.is_empty() {
            return Err(LoadError {
                problems: b.problems,
            });
        }
        // Resolve every hook-result read to a strictly-earlier producer, enforce
        // the readable surfaces, capture, and JSON rules, before admission. This
        // runs on a fully parsed, otherwise-clean template.
        settle_hook_results(
            &interview,
            &files,
            &inject,
            &hooks,
            &messages,
            &render_program,
            &b.result_ids,
            &mut b.problems,
        );
        if !b.problems.is_empty() {
            return Err(LoadError {
                problems: b.problems,
            });
        }
        let environment_need = compute_environment_need(
            &interview,
            &files,
            &inject,
            &hooks,
            &messages,
            &render_program,
        );
        Ok(Self {
            name: raw.name,
            description: raw.description,
            source_dir,
            data,
            interview,
            root,
            partials,
            files,
            inject,
            ignore,
            static_files,
            hooks,
            messages,
            interview_hook_nodes,
            top_hook_nodes,
            render_program,
            reserves_context,
            environment_need,
            result_ids: b.result_ids,
        })
    }
}

/// The invariant surface of a source-tree walk: the roots and the rules that do
/// not change as recursion descends into subdirectories.
struct SourceScan<'a> {
    /// The source subdirectory root, against which each entry's emitted path is
    /// relative.
    source_root: &'a Path,
    /// The template root, against which each body's include label is relative.
    template_root: &'a Path,
    /// The include-confinement owner used to compile each file body.
    partials: &'a Partials,
    ignore: &'a GlobSet,
    static_files: &'a GlobSet,
}

/// Recursively compiles the source tree in deterministic (sorted) order,
/// retaining each file's compiled path segments and body. Mirrors planning's
/// walk: it skips the root `template.yml` and `ignore` matches, and rejects a
/// source symlink.
fn compile_source_tree(
    scan: &SourceScan,
    dir: &Path,
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
        let relative = source.strip_prefix(scan.source_root).unwrap().to_owned();
        if scan.source_root == scan.template_root
            && dir == scan.source_root
            && entry.file_name() == "template.yml"
        {
            continue;
        }
        if scan.ignore.is_match(&relative) {
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
            compile_source_tree(scan, &source, out, problems);
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
        let body = if scan.static_files.is_match(&relative) {
            SourceBody::Static
        } else {
            // The include label is the file's path relative to the template
            // root, in the `/`-separated template namespace — the same string
            // any file would use to include it.
            let label = source
                .strip_prefix(scan.template_root)
                .unwrap_or(&relative)
                .to_string_lossy()
                .replace('\\', "/");
            match fs::read(&source) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => match scan.partials.compile(text, &label) {
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
    inject: &[InjectRule],
    hooks: &[HookNode],
    messages: &ApplyMessages,
    render_program: &[SourceEntry],
) -> EnvironmentNeed {
    let mut find = NeedScan::default();
    find.nodes(interview, "interview");
    for (i, rule) in inject.iter().enumerate() {
        find.inject(rule, &format!("inject[{i}]"));
    }
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
    fn inject(&mut self, rule: &InjectRule, label: &str) {
        match rule {
            InjectRule::Region(region) => {
                self.opt_expr(region.when.as_ref(), &format!("{label}.when"));
                self.tmpl(&region.into, &format!("{label}.into"));
                if let Some(anchor) = &region.anchor {
                    self.tmpl(&anchor.after, &format!("{label}.anchor.after"));
                }
                match &region.body {
                    RegionBody::Content(tmpl) => self.tmpl(tmpl, &format!("{label}.content")),
                    RegionBody::Source { body, .. } => {
                        self.file_tmpl(body, &format!("{label}.source"))
                    }
                }
            }
            InjectRule::Struct(rule) => {
                self.opt_expr(rule.when.as_ref(), &format!("{label}.when"));
                self.tmpl(&rule.into, &format!("{label}.into"));
                self.inject_value(&rule.value, &format!("{label}.struct.value"));
            }
        }
    }
    fn inject_value(&mut self, value: &InjectValue, label: &str) {
        match value {
            InjectValue::Template(tmpl) => self.tmpl(tmpl, label),
            InjectValue::Literal(_) => {}
            InjectValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    self.inject_value(item, &format!("{label}[{i}]"));
                }
            }
            InjectValue::Object(pairs) => {
                for (key, item) in pairs {
                    self.inject_value(item, &format!("{label}.{key}"));
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

// ---------------------------------------------------------------------------
// Hook results: load-time resolution (settle) and plan-time helpers.
// ---------------------------------------------------------------------------

impl Template {
    /// Whether any reference names a declared hook result.
    pub(crate) fn references_result(&self, refs: &HashSet<String>) -> bool {
        refs.iter().any(|r| self.result_ids.contains(r))
    }

    /// Whether a top-level hook reads any hook result on one of its readable
    /// surfaces, so planning must retain it and render it after its producers
    /// run. A valid template never references a result from `run[0]` (a load
    /// error), so scanning every argument is safe.
    pub(crate) fn reads_results(&self, node: &HookNode) -> bool {
        if self.result_ids.is_empty() {
            return false;
        }
        let reads = |refs: &HashSet<String>| self.references_result(refs);
        node.when.as_ref().is_some_and(|w| reads(w.references()))
            || node
                .command
                .cwd
                .as_ref()
                .is_some_and(|c| reads(c.references()))
            || match &node.command.program {
                HookProgram::Run(args) => args.iter().any(|a| reads(a.references())),
                HookProgram::Script { args, .. } => args.iter().any(|a| reads(a.references())),
            }
    }

    /// Every declared producer's result spec, in run order (interview producers
    /// first, then top-level), used to seed a not-run value for each before the
    /// apply loop so every reader sees a bound value.
    pub(crate) fn result_specs(&self) -> Vec<crate::hook::ResultSpec> {
        fn walk(nodes: &[Node], specs: &mut Vec<crate::hook::ResultSpec>) {
            for node in nodes {
                match node {
                    Node::Hook(h) => {
                        if let Some(id) = &h.id {
                            specs.push(crate::hook::ResultSpec {
                                id: id.clone(),
                                json: h.parse_json,
                                status_id: h.status_id.clone(),
                            });
                        }
                    }
                    Node::Group(g) => walk(&g.nodes, specs),
                    _ => {}
                }
            }
        }
        let mut specs = Vec::new();
        walk(&self.interview, &mut specs);
        for h in &self.hooks {
            if let Some(id) = &h.id {
                specs.push(crate::hook::ResultSpec {
                    id: id.clone(),
                    json: h.parse_json,
                    status_id: h.status_id.clone(),
                });
            }
        }
        specs
    }
}

/// One declared hook-result producer, with its position in the global run order.
struct Producer {
    id: Id,
    path: String,
    order: usize,
    json: bool,
    capture: Capture,
    status_id: Option<Id>,
    allow_failure: bool,
    /// Whether the producer can be skipped, so a JSON `none` at `<id>` would be
    /// ambiguous without a `status_id` to disambiguate via `exit_code is none`.
    may_not_run: bool,
}
/// A resolved result name: which producer it belongs to and whether it is the
/// producer's own `id` or its `status_id`.
struct ByName {
    producer: usize,
    is_status: bool,
}
/// The reads observed for one result name across every allowed surface.
#[derive(Default)]
struct ReadInfo {
    /// A bare reference to the whole result (no attribute), including a subscript
    /// access, which reads the object.
    bare: bool,
    /// The attribute fields read, e.g. `exit_code`, `stdout`.
    fields: HashSet<String>,
}

/// Splits a nested reference into its root id and first attribute, if any.
/// `pkg` → (`pkg`, None); `pkg.name` → (`pkg`, `name`); `pkg.a.b` → (`pkg`, `a`).
fn split_reference(reference: &str) -> (&str, Option<&str>) {
    match reference.split_once('.') {
        Some((root, rest)) => (root, Some(rest.split('.').next().unwrap_or(rest))),
        None => (reference, None),
    }
}

/// Collects interview hook producers in interview order, tracking whether each
/// is skippable: an enclosing group `when`, its own `when`, `allow_failure`, or a
/// preceding flow that can skip make it able to not run.
fn collect_interview_producers(
    nodes: &[Node],
    prefix: &str,
    group_when: bool,
    seen_skip: &mut bool,
    out: &mut Vec<Producer>,
) {
    for (i, node) in nodes.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        match node {
            Node::Flow(f) => {
                if matches!(
                    f.action,
                    FlowAction::Stop | FlowAction::Abort | FlowAction::Skip(_)
                ) {
                    *seen_skip = true;
                }
            }
            Node::Group(group) => {
                let inner = group_when || group.when.is_some();
                collect_interview_producers(
                    &group.nodes,
                    &format!("{path}.nodes"),
                    inner,
                    seen_skip,
                    out,
                );
            }
            Node::Hook(h) => {
                if let Some(id) = &h.id {
                    let order = out.len();
                    out.push(Producer {
                        id: id.clone(),
                        path: format!("{path}.hook"),
                        order,
                        json: h.parse_json,
                        capture: h.capture,
                        status_id: h.status_id.clone(),
                        allow_failure: h.allow_failure,
                        may_not_run: h.when.is_some()
                            || group_when
                            || h.allow_failure
                            || *seen_skip,
                    });
                }
            }
            _ => {}
        }
    }
}

/// Resolves every hook-result read to a strictly-earlier producer, enforces the
/// readable surfaces, capture agreement, and the JSON rules, aggregating every
/// violation into `problems`. Runs at load, before admission; results are values
/// bound only in the apply loop.
#[allow(clippy::too_many_arguments)]
fn settle_hook_results(
    interview: &[Node],
    files: &[FileRule],
    inject: &[InjectRule],
    hooks: &[HookNode],
    messages: &ApplyMessages,
    render_program: &[SourceEntry],
    result_ids: &HashSet<String>,
    problems: &mut Vec<Problem>,
) {
    if result_ids.is_empty() {
        return;
    }
    let mut producers = Vec::new();
    let mut seen_skip = false;
    collect_interview_producers(
        interview,
        "interview",
        false,
        &mut seen_skip,
        &mut producers,
    );
    let interview_count = producers.len();
    for (j, h) in hooks.iter().enumerate() {
        if let Some(id) = &h.id {
            producers.push(Producer {
                id: id.clone(),
                path: format!("hooks[{j}]"),
                order: interview_count + j,
                json: h.parse_json,
                capture: h.capture,
                status_id: h.status_id.clone(),
                allow_failure: h.allow_failure,
                may_not_run: h.when.is_some() || h.allow_failure,
            });
        }
    }
    let mut by_name: HashMap<String, ByName> = HashMap::new();
    for (idx, p) in producers.iter().enumerate() {
        by_name.insert(
            p.id.as_str().to_owned(),
            ByName {
                producer: idx,
                is_status: false,
            },
        );
        if let Some(status) = &p.status_id {
            by_name.insert(
                status.as_str().to_owned(),
                ByName {
                    producer: idx,
                    is_status: true,
                },
            );
        }
    }
    let mut settle = Settle {
        result_ids,
        producers: &producers,
        by_name: &by_name,
        reads: HashMap::new(),
        problems,
    };
    // Disallowed surfaces: any result reference is a load error.
    settle.deny_interview(interview, "interview");
    for (i, rule) in files.iter().enumerate() {
        let path = format!("files[{i}]");
        settle.deny_opt_expr(rule.when.as_ref(), &format!("{path}.when"));
        settle.deny(rule.each.expr.references(), &format!("{path}.each"));
        settle.deny(rule.path.references(), &format!("{path}.path"));
        if let Some(body) = &rule.body {
            settle.deny(body.references(), &format!("{path}.source"));
        }
    }
    for entry in render_program {
        let label = entry.source.display().to_string();
        for segment in &entry.segments {
            settle.deny(segment.references(), &format!("{label} (path)"));
        }
        if let SourceBody::Rendered(body) = &entry.body {
            settle.deny(body.references(), &label);
        }
    }
    settle.deny_opt_tmpl(messages.before_apply.as_ref(), "messages.before-apply");
    // Injection renders at plan time, before hooks run, so it is a disallowed
    // surface for a hook result, like a file body or path.
    for (i, rule) in inject.iter().enumerate() {
        let path = format!("inject[{i}]");
        match rule {
            InjectRule::Region(region) => {
                settle.deny_opt_expr(region.when.as_ref(), &format!("{path}.when"));
                settle.deny(region.into.references(), &format!("{path}.into"));
                if let Some(anchor) = &region.anchor {
                    settle.deny(anchor.after.references(), &format!("{path}.anchor.after"));
                }
                match &region.body {
                    RegionBody::Content(tmpl) => {
                        settle.deny(tmpl.references(), &format!("{path}.content"))
                    }
                    RegionBody::Source { body, .. } => {
                        settle.deny(body.references(), &format!("{path}.source"))
                    }
                }
            }
            InjectRule::Struct(rule) => {
                settle.deny_opt_expr(rule.when.as_ref(), &format!("{path}.when"));
                settle.deny(rule.into.references(), &format!("{path}.into"));
                deny_inject_value(&mut settle, &rule.value, &format!("{path}.struct.value"));
            }
        }
    }
    // Allowed surfaces: later top-level hook when/run[1..]/args/cwd and after-apply.
    for (j, h) in hooks.iter().enumerate() {
        let order = interview_count + j;
        let path = format!("hooks[{j}]");
        if let Some(each) = &h.each {
            settle.deny(each.expr.references(), &format!("{path}.each"));
        }
        if let Some(when) = &h.when {
            settle.allow(&when.nested_references(), &format!("{path}.when"), order);
        }
        if let Some(cwd) = &h.command.cwd {
            settle.allow(&cwd.nested_references(), &format!("{path}.cwd"), order);
        }
        match &h.command.program {
            HookProgram::Run(args) => {
                for (k, arg) in args.iter().enumerate() {
                    if k == 0 {
                        settle.deny(arg.references(), &format!("{path}.run[0]"));
                    } else {
                        settle.allow(&arg.nested_references(), &format!("{path}.run[{k}]"), order);
                    }
                }
            }
            HookProgram::Script { args, .. } => {
                for (k, arg) in args.iter().enumerate() {
                    settle.allow(
                        &arg.nested_references(),
                        &format!("{path}.args[{k}]"),
                        order,
                    );
                }
            }
        }
    }
    if let Some(after) = &messages.after_apply {
        settle.allow(
            &after.nested_references(),
            "messages.after-apply",
            usize::MAX,
        );
    }
    settle.finish();
}

/// Denies a hook-result read in any string leaf of an injected structured value.
fn deny_inject_value(settle: &mut Settle, value: &InjectValue, label: &str) {
    match value {
        InjectValue::Template(tmpl) => settle.deny(tmpl.references(), label),
        InjectValue::Literal(_) => {}
        InjectValue::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                deny_inject_value(settle, item, &format!("{label}[{i}]"));
            }
        }
        InjectValue::Object(pairs) => {
            for (key, item) in pairs {
                deny_inject_value(settle, item, &format!("{label}.{key}"));
            }
        }
    }
}

/// The mutable state of one settle pass.
struct Settle<'a> {
    result_ids: &'a HashSet<String>,
    producers: &'a [Producer],
    by_name: &'a HashMap<String, ByName>,
    reads: HashMap<String, ReadInfo>,
    problems: &'a mut Vec<Problem>,
}
impl Settle<'_> {
    /// A disallowed surface: any reference whose root is a result name is a load
    /// error naming the readable surfaces.
    fn deny(&mut self, refs: &HashSet<String>, path: &str) {
        for name in refs {
            if self.result_ids.contains(name) {
                problem(
                    self.problems,
                    path,
                    format!(
                        "hook results are readable only in a later top-level hook \
                         when/run[1..]/args/cwd and messages.after-apply: {name}"
                    ),
                );
            }
        }
    }
    fn deny_opt_tmpl(&mut self, tmpl: Option<&Tmpl>, path: &str) {
        if let Some(tmpl) = tmpl {
            self.deny(tmpl.references(), path);
        }
    }
    fn deny_opt_expr(&mut self, expr: Option<&Expr>, path: &str) {
        if let Some(expr) = expr {
            self.deny(expr.references(), path);
        }
    }
    fn deny_typed<T: Clone + serde::de::DeserializeOwned>(&mut self, typed: &Typed<T>, path: &str) {
        self.deny(&typed.references(), path);
    }
    fn deny_opt_typed<T: Clone + serde::de::DeserializeOwned>(
        &mut self,
        typed: Option<&Typed<T>>,
        path: &str,
    ) {
        if let Some(typed) = typed {
            self.deny_typed(typed, path);
        }
    }
    /// Walks every interview surface, denying a result read on any of them
    /// (interview fields, and interview hook fields — interview hooks may
    /// produce a result but never read one).
    fn deny_interview(&mut self, nodes: &[Node], prefix: &str) {
        for (i, node) in nodes.iter().enumerate() {
            let label = format!("{prefix}[{i}]");
            match node {
                Node::Question(q) => {
                    self.deny(q.prompt.references(), &format!("{label}.prompt"));
                    self.deny_opt_tmpl(q.description.as_ref(), &format!("{label}.description"));
                    self.deny_opt_tmpl(q.placeholder.as_ref(), &format!("{label}.placeholder"));
                    self.deny_typed(&q.required, &format!("{label}.required"));
                    self.deny_opt_expr(q.when.as_ref(), &format!("{label}.when"));
                    self.deny_opt_typed(q.validate.min.as_ref(), &format!("{label}.validate.min"));
                    self.deny_opt_typed(q.validate.max.as_ref(), &format!("{label}.validate.max"));
                    self.deny_opt_expr(q.format.as_ref(), &format!("{label}.format"));
                    match &q.kind {
                        QuestionKind::Text { default } | QuestionKind::Multiline { default } => {
                            self.deny_opt_tmpl(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::Confirm { default } => {
                            self.deny_opt_typed(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::Select { options, default } => {
                            self.deny_typed(options, &format!("{label}.options"));
                            self.deny_opt_tmpl(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::MultiSelect { options, default } => {
                            self.deny_typed(options, &format!("{label}.options"));
                            self.deny_opt_typed(default.as_ref(), &format!("{label}.default"));
                        }
                        QuestionKind::TextLoop { default, min, max } => {
                            self.deny_opt_typed(default.as_ref(), &format!("{label}.default"));
                            self.deny_opt_typed(min.as_ref(), &format!("{label}.loop.min"));
                            self.deny_opt_typed(max.as_ref(), &format!("{label}.loop.max"));
                        }
                    }
                }
                Node::Computed(c) => {
                    self.deny_opt_expr(c.when.as_ref(), &format!("{label}.when"));
                    self.deny(c.expr.references(), &format!("{label}.computed"));
                }
                Node::Group(g) => {
                    self.deny_opt_expr(g.when.as_ref(), &format!("{label}.when"));
                    self.deny_interview(&g.nodes, &format!("{label}.nodes"));
                }
                Node::Message(m) => {
                    self.deny_opt_expr(m.when.as_ref(), &format!("{label}.when"));
                    self.deny(m.text.references(), &format!("{label}.message"));
                }
                Node::Hook(h) => {
                    self.deny_opt_expr(h.when.as_ref(), &format!("{label}.when"));
                    if let Some(each) = &h.each {
                        self.deny(each.expr.references(), &format!("{label}.each"));
                    }
                    self.deny_opt_tmpl(h.command.cwd.as_ref(), &format!("{label}.cwd"));
                    match &h.command.program {
                        HookProgram::Run(args) => {
                            for (k, arg) in args.iter().enumerate() {
                                self.deny(arg.references(), &format!("{label}.run[{k}]"));
                            }
                        }
                        HookProgram::Script { args, .. } => {
                            for (k, arg) in args.iter().enumerate() {
                                self.deny(arg.references(), &format!("{label}.args[{k}]"));
                            }
                        }
                    }
                }
                Node::Flow(f) => {
                    self.deny_opt_expr(f.when.as_ref(), &format!("{label}.when"));
                }
            }
        }
    }
    /// An allowed surface reading results at `reader_order`. Resolves each result
    /// reference to a strictly-earlier producer and enforces field, capture, and
    /// JSON rules.
    fn allow(&mut self, nested: &HashSet<String>, path: &str, reader_order: usize) {
        for reference in nested {
            let (root, field) = split_reference(reference);
            if !self.result_ids.contains(root) {
                continue;
            }
            let Some(by_name) = self.by_name.get(root) else {
                problem(
                    self.problems,
                    path,
                    format!("hook result read before hook `{root}` runs: {root}"),
                );
                continue;
            };
            let producer = &self.producers[by_name.producer];
            if producer.order >= reader_order {
                problem(
                    self.problems,
                    path,
                    format!("hook result read before hook `{root}` runs: {root}"),
                );
                continue;
            }
            let info = self.reads.entry(root.to_owned()).or_default();
            match field {
                Some(f) => {
                    info.fields.insert(f.to_owned());
                }
                None => info.bare = true,
            }
            if by_name.is_status {
                if let Some(f) = field {
                    match f {
                        "exit_code" | "parsed" | "stdout" => {}
                        "stderr" => {
                            if !producer.capture.stderr {
                                problem(
                                    self.problems,
                                    path,
                                    format!(
                                        "hook `{}` does not capture stderr; add `capture: [stderr]`",
                                        producer.id
                                    ),
                                );
                            }
                        }
                        other => problem(
                            self.problems,
                            path,
                            format!("unknown hook result field: {root}.{other}"),
                        ),
                    }
                }
            } else if producer.json {
                // The parsed value is read directly; a metadata-field attribute is
                // a likely mistake, so name the subscript escape and status-id.
                if let Some(f @ ("exit_code" | "stdout" | "stderr")) = field {
                    let status = producer
                        .status_id
                        .as_ref()
                        .map(Id::as_str)
                        .unwrap_or("status-id");
                    problem(
                        self.problems,
                        path,
                        format!(
                            "hook {root} parses stdout as JSON; use {root}['{f}'] for a JSON key \
                             or {status}.{f} for metadata"
                        ),
                    );
                }
            } else if let Some(f) = field {
                match f {
                    "exit_code" => {}
                    "stdout" => {
                        if !producer.capture.stdout {
                            problem(
                                self.problems,
                                path,
                                format!(
                                    "hook `{root}` does not capture stdout; add `capture: [stdout]`"
                                ),
                            );
                        }
                    }
                    "stderr" => {
                        if !producer.capture.stderr {
                            problem(
                                self.problems,
                                path,
                                format!(
                                    "hook `{root}` does not capture stderr; add `capture: [stderr]`"
                                ),
                            );
                        }
                    }
                    other => problem(
                        self.problems,
                        path,
                        format!("unknown hook result field: {root}.{other}"),
                    ),
                }
            }
        }
    }
    /// Per-producer checks that need every read observed: allow-failure must be
    /// read, a captured stream must be read, and a skippable JSON producer needs
    /// a status-id.
    fn finish(&mut self) {
        for producer in self.producers {
            let id = producer.id.as_str();
            let read = self.reads.contains_key(id);
            let status_read = producer
                .status_id
                .as_ref()
                .is_some_and(|s| self.reads.contains_key(s.as_str()));
            if producer.allow_failure {
                if producer.json {
                    match &producer.status_id {
                        None => problem(
                            self.problems,
                            &producer.path,
                            "allow-failure on a parse: json hook requires status-id; \
                             add status-id: <name> and read <name>.exit_code",
                        ),
                        Some(status) if !status_read => problem(
                            self.problems,
                            &producer.path,
                            format!(
                                "the result of {status} is never read; read {status}.exit_code \
                                 in a later hook or in messages.after-apply"
                            ),
                        ),
                        Some(_) => {}
                    }
                } else if !read {
                    problem(
                        self.problems,
                        &producer.path,
                        format!(
                            "the result of {id} is never read; read {id}.exit_code in a later \
                             hook or in messages.after-apply"
                        ),
                    );
                }
            }
            if producer.json
                && producer.status_id.is_none()
                && producer.may_not_run
                && !producer.allow_failure
            {
                problem(
                    self.problems,
                    &producer.path,
                    format!(
                        "parse: json hook `{id}` may not run; declare status-id so its \
                         exit_code is readable"
                    ),
                );
            }
            // Captured-never-read.
            if producer.json {
                // stdout is the parse source, always used; stderr is metadata.
                if producer.capture.stderr {
                    let ok = producer.status_id.as_ref().is_some_and(|s| {
                        self.reads
                            .get(s.as_str())
                            .is_some_and(|r| r.bare || r.fields.contains("stderr"))
                    });
                    if !ok {
                        problem(
                            self.problems,
                            &producer.path,
                            format!("captured stderr of hook `{id}` is never read"),
                        );
                    }
                }
            } else {
                for (captured, stream) in [
                    (producer.capture.stdout, "stdout"),
                    (producer.capture.stderr, "stderr"),
                ] {
                    if captured {
                        let ok = self
                            .reads
                            .get(id)
                            .is_some_and(|r| r.bare || r.fields.contains(stream));
                        if !ok {
                            problem(
                                self.problems,
                                &producer.path,
                                format!("captured {stream} of hook `{id}` is never read"),
                            );
                        }
                    }
                }
            }
        }
    }
}
