// ---
// relationships:
//   implements: architecture
// ---
use crate::jinja::{Expr, Tmpl, Typed, is_global};
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
    pub files: Vec<FileRule>,
    pub ignore: GlobSet,
    pub static_files: GlobSet,
    pub hooks: Vec<HookNode>,
    pub messages: ApplyMessages,
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
            if !self.seen.contains(name) && !local.contains(&name.as_str()) && !is_global(name) {
                problem(
                    &mut self.problems,
                    path,
                    format!("id is not defined by an earlier node: {name}"),
                );
            }
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
    fn nodes(&mut self, values: &[Value], prefix: &str) -> Vec<Node> {
        let mut result = Vec::new();
        for (i, value) in values.iter().enumerate() {
            let path = format!("{prefix}[{i}]");
            let Some(map) = value.as_object() else {
                continue;
            };
            let when = self.expr(map.get("when"), &format!("{path}.when"), &[]);
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
                    .map(|v| self.nodes(v, &format!("{path}.nodes")))
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
    pub fn load(folder: &Path) -> Result<Self, LoadError> {
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
        let mut b = Builder {
            problems,
            seen: HashSet::new(),
            names: HashSet::new(),
            answer_ids: HashSet::new(),
            root: root.clone(),
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
                    b.seen.insert(id.as_str().into());
                    data.insert(id, value.clone());
                }
            }
        }
        let interview = b.nodes(raw.interview.as_deref().unwrap_or_default(), "interview");
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
                    Node::Message(_) | Node::Hook(_) => {}
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
            let target = b.bound(Some(&each), |b| {
                let target = b.tmpl(map.get("path"), &format!("{path}.path"));
                match fs::read_to_string(root.join(&source)) {
                    Ok(content) => {
                        b.tmpl(Some(&Value::String(content)), &format!("{path}.source"));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {}
                    Err(error) => {
                        problem(&mut b.problems, format!("{path}.source"), error.to_string())
                    }
                }
                target
            });
            if let Some(path) = target {
                files.push(FileRule {
                    each,
                    source,
                    path,
                    when,
                });
            }
        }
        let hooks = raw
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
        Ok(Self {
            name: raw.name,
            description: raw.description,
            source_dir,
            data,
            interview,
            root,
            files,
            ignore,
            static_files,
            hooks,
            messages,
        })
    }
}
