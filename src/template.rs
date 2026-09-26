// ---
// relationships:
//   implements: architecture
// ---
use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::jinja::Tmpl;

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
        Ok(Self(value.to_owned()))
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
    pub interview: Vec<Node>,
}

#[derive(Debug)]
pub enum Node {
    Question(Question),
}

#[derive(Debug)]
pub struct Question {
    pub id: Id,
    pub prompt: Tmpl,
    pub description: Option<Tmpl>,
    pub required: bool,
    pub kind: QuestionKind,
}

#[derive(Debug)]
pub enum QuestionKind {
    Text { default: Option<Tmpl> },
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
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{}: {}", problem.path, problem.message)?;
        }
        Ok(())
    }
}
impl std::error::Error for LoadError {}

#[derive(Deserialize)]
struct RawTemplate {
    name: Option<serde_norway::Value>,
    description: Option<serde_norway::Value>,
    source: Option<serde_norway::Value>,
    interview: Option<Vec<serde_norway::Value>>,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, serde_norway::Value>,
}

fn problem(problems: &mut Vec<Problem>, path: impl Into<String>, message: impl Into<String>) {
    problems.push(Problem {
        path: path.into(),
        message: message.into(),
    });
}

fn compile(value: String, path: &str, problems: &mut Vec<Problem>) -> Option<Tmpl> {
    match Tmpl::compile(value) {
        Ok(value) => Some(value),
        Err(error) => {
            problem(problems, path, error.to_string());
            None
        }
    }
}

fn string_field(
    map: &serde_norway::Mapping,
    key: &str,
    prefix: &str,
    required: bool,
    problems: &mut Vec<Problem>,
) -> Option<String> {
    match map.get(serde_norway::Value::String(key.into())) {
        Some(serde_norway::Value::String(value)) => Some(value.clone()),
        Some(_) => {
            problem(problems, format!("{prefix}.{key}"), "expected string");
            None
        }
        None if required => {
            problem(
                problems,
                format!("{prefix}.{key}"),
                "missing required value",
            );
            None
        }
        None => None,
    }
}

impl Template {
    pub fn load(folder: &Path) -> Result<Self, LoadError> {
        let text = fs::read_to_string(folder.join("template.yml")).map_err(|error| LoadError {
            problems: vec![Problem {
                path: "template.yml".into(),
                message: error.to_string(),
            }],
        })?;
        let raw: RawTemplate = serde_norway::from_str(&text).map_err(|error| LoadError {
            problems: vec![Problem {
                path: "template.yml".into(),
                message: error.to_string(),
            }],
        })?;
        let mut problems = Vec::new();
        for key in raw.extra.keys() {
            let message = if ["data", "files", "ignore", "static", "hooks", "messages"]
                .contains(&key.as_str())
            {
                format!("not supported yet: {key}")
            } else {
                format!("unknown key: {key}")
            };
            problem(&mut problems, key, message);
        }
        let name = match raw.name {
            Some(serde_norway::Value::String(name)) => name,
            Some(_) => {
                problem(&mut problems, "name", "expected string");
                String::new()
            }
            None => {
                problem(&mut problems, "name", "missing required value");
                String::new()
            }
        };
        if !name.is_empty()
            && (!name
                .bytes()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
                || name.split('-').any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
                }))
        {
            problem(&mut problems, "name", "invalid short name");
        }
        let description = match raw.description {
            Some(serde_norway::Value::String(value)) => Some(value),
            Some(_) => {
                problem(&mut problems, "description", "expected string");
                None
            }
            None => None,
        };
        let source = match raw.source {
            Some(serde_norway::Value::String(value)) => value,
            Some(_) => {
                problem(&mut problems, "source", "expected string");
                "template".into()
            }
            None => "template".into(),
        };
        if Path::new(&source).is_absolute()
            || Path::new(&source)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            problem(
                &mut problems,
                "source",
                "source must stay inside template root",
            );
        }
        let source_dir = folder.join(&source);
        if !source_dir.is_dir() {
            problem(&mut problems, "source", "source directory does not exist");
        }
        if let (Ok(root), Ok(actual)) = (folder.canonicalize(), source_dir.canonicalize()) {
            if !actual.starts_with(root) {
                problem(
                    &mut problems,
                    "source",
                    "source must stay inside template root",
                );
            }
        }
        let mut interview = Vec::new();
        let mut seen = HashSet::new();
        for (index, value) in raw.interview.unwrap_or_default().into_iter().enumerate() {
            let prefix = format!("interview[{index}]");
            let Some(map) = value.as_mapping() else {
                problem(&mut problems, prefix, "expected node object");
                continue;
            };
            if !map.contains_key(serde_norway::Value::String("type".into())) {
                let kind = map
                    .keys()
                    .filter_map(serde_norway::Value::as_str)
                    .find(|key| ["computed", "group", "hook", "message"].contains(key))
                    .unwrap_or("node");
                problem(&mut problems, &prefix, format!("not supported yet: {kind}"));
                continue;
            }
            let kind = string_field(map, "type", &prefix, true, &mut problems);
            if kind.as_deref() != Some("text") {
                if let Some(kind) = kind {
                    problem(
                        &mut problems,
                        format!("{prefix}.type"),
                        format!("not supported yet: {kind}"),
                    );
                }
                continue;
            }
            for key in map.keys().filter_map(serde_norway::Value::as_str) {
                if !["id", "type", "prompt", "description", "required", "default"].contains(&key) {
                    let message = if [
                        "placeholder",
                        "options",
                        "loop",
                        "validate",
                        "format",
                        "when",
                    ]
                    .contains(&key)
                    {
                        format!("not supported yet: {key}")
                    } else {
                        format!("unknown key: {key}")
                    };
                    problem(&mut problems, format!("{prefix}.{key}"), message);
                }
            }
            let id = string_field(map, "id", &prefix, true, &mut problems).and_then(|value| {
                match Id::parse(&value) {
                    Ok(id) => Some(id),
                    Err(message) => {
                        problem(&mut problems, format!("{prefix}.id"), message);
                        None
                    }
                }
            });
            if let Some(id) = &id {
                if !seen.insert(id.as_str().to_owned()) {
                    problem(
                        &mut problems,
                        format!("{prefix}.id"),
                        format!("duplicate id: {id}"),
                    );
                }
            }
            let prompt = string_field(map, "prompt", &prefix, true, &mut problems)
                .and_then(|value| compile(value, &format!("{prefix}.prompt"), &mut problems));
            let description = string_field(map, "description", &prefix, false, &mut problems)
                .and_then(|value| compile(value, &format!("{prefix}.description"), &mut problems));
            let default = string_field(map, "default", &prefix, false, &mut problems)
                .and_then(|value| compile(value, &format!("{prefix}.default"), &mut problems));
            let required = match map.get(serde_norway::Value::String("required".into())) {
                Some(serde_norway::Value::Bool(value)) => *value,
                Some(_) => {
                    problem(
                        &mut problems,
                        format!("{prefix}.required"),
                        "not supported yet: required expression",
                    );
                    false
                }
                None => false,
            };
            for (key, tmpl) in [
                ("prompt", prompt.as_ref()),
                ("description", description.as_ref()),
                ("default", default.as_ref()),
            ] {
                if let Some(tmpl) = tmpl {
                    for reference in tmpl.references() {
                        if !seen.contains(reference)
                            || id.as_ref().is_some_and(|id| id.as_str() == reference)
                        {
                            problem(
                                &mut problems,
                                format!("{prefix}.{key}"),
                                format!("id is not defined by an earlier node: {reference}"),
                            );
                        }
                    }
                }
            }
            if let (Some(id), Some(prompt)) = (id, prompt) {
                interview.push(Node::Question(Question {
                    id,
                    prompt,
                    description,
                    required,
                    kind: QuestionKind::Text { default },
                }));
            }
        }
        if !problems.is_empty() {
            return Err(LoadError { problems });
        }
        Ok(Self {
            name,
            description,
            source_dir,
            interview,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Id;
    #[test]
    fn id_parse() {
        for good in ["a", "_", "a_1"] {
            assert!(Id::parse(good).is_ok());
        }
        for bad in ["", "1a", "A", "a-b", "é"] {
            assert!(Id::parse(bad).is_err());
        }
    }
}
