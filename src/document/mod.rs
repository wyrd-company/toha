// ---
// relationships:
//   implements: template-composition
// ---
//! Document question contracts and the filesystem/network admission boundary.
use crate::{Id, Node, QuestionKind, RawAnswer, RawAnswers, Template};
use indexmap::IndexMap;
use jsonschema::{Uri, Validator};
mod schema;
pub(crate) use schema::SchemaIndex;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct DocumentQuestion {
    pub input: PathBuf,
    pub schemas: Vec<String>,
    validators: Vec<Validator>,
    /// The admitted document for this run, captured before the pure interview.
    pub(crate) value: Option<Value>,
    pub(crate) target: Option<PathBuf>,
}
impl DocumentQuestion {
    pub(crate) fn load(
        index: &SchemaIndex,
        schemas: Vec<String>,
        input: &str,
    ) -> Result<Self, String> {
        if input.is_empty() || Path::new(input).is_absolute() {
            return Err("document input must be a non-empty relative path".into());
        }
        let validators = schemas
            .iter()
            .map(|uri| {
                Uri::parse(uri.as_str()).map_err(|e| format!("schema {uri}: {e}"))?;
                let schema = index.resolve(uri)?;
                jsonschema::options()
                    .with_base_uri(uri)
                    .with_retriever(index.clone())
                    .build(&schema)
                    .map_err(|e| format!("schema {uri}: {e}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            input: input.into(),
            schemas,
            validators,
            value: None,
            target: None,
        })
    }
    pub fn admitted(&self) -> Option<&Value> {
        self.value.as_ref()
    }
    pub fn validate(&self, value: &Value) -> Result<(), String> {
        if !value.is_object() {
            return Err("must be a JSON object".into());
        }
        let index = match value.get("$schema") {
            Some(Value::String(uri)) => self
                .schemas
                .iter()
                .position(|s| s == uri)
                .ok_or_else(|| format!("unaccepted $schema {uri}"))?,
            None if self.schemas.len() == 1 => 0,
            _ => return Err("$schema must name one of the question's schema URIs".into()),
        };
        self.validators[index]
            .validate(value)
            .map_err(|e| format!("schema {}: {e}", self.schemas[index]))
    }
}

/// Select and validate documents before starting an interview. Inputs override
/// explicit answers; explicit answers override default files. No output is written.
pub fn prepare(
    template: &mut Template,
    target: &Path,
    inputs: &[PathBuf],
    answers: &RawAnswers,
) -> Result<(), String> {
    if template.document_question_ids().is_empty() && inputs.is_empty() {
        return Ok(());
    }
    let mut routed = RawAnswers::new();
    for path in inputs {
        let value = read(path)?;
        let uri = value
            .get("$schema")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{}: input needs a string $schema", path.display()))?;
        let mut accepted = false;
        for id in template.document_question_ids() {
            let QuestionKind::Document { contract } = &template.question(&id).unwrap().kind else {
                unreachable!()
            };
            if contract.schemas.iter().any(|s| s == uri) {
                contract
                    .validate(&value)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                if routed
                    .insert(id.clone(), RawAnswer(value.clone()))
                    .is_some()
                {
                    return Err(format!(
                        "{}: more than one input for question {id}",
                        path.display()
                    ));
                }
                accepted = true;
            }
        }
        if !accepted {
            return Err(format!(
                "{}: no document question accepts $schema {uri}",
                path.display()
            ));
        }
    }
    let target = crate::staging::canonical_target(target).map_err(|e| e.to_string())?;
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("target has no existing ancestor")?;
    }
    let bound = match gix::discover(ancestor) {
        Ok(repo) => repo
            .workdir()
            .ok_or("target repository is bare")?
            .canonicalize()
            .map_err(|e| e.to_string())?,
        Err(gix::discover::Error::Discover(
            gix::discover::upwards::Error::NoGitRepository { .. }
            | gix::discover::upwards::Error::NoGitRepositoryWithinFs { .. }
            | gix::discover::upwards::Error::NoGitRepositoryWithinCeiling { .. },
        )) => target.as_path().to_owned(),
        Err(e) => return Err(e.to_string()),
    };
    prepare_nodes(
        &mut template.interview,
        target.as_path(),
        &bound,
        &routed,
        answers,
    )
}
fn prepare_nodes(
    nodes: &mut [Node],
    target: &Path,
    bound: &Path,
    routed: &RawAnswers,
    answers: &RawAnswers,
) -> Result<(), String> {
    for node in nodes {
        match node {
            Node::Question(q) => {
                if let QuestionKind::Document { contract } = &mut q.kind {
                    let path = target.join(&contract.input);
                    let value = if let Some(raw) = routed.get(&q.id).or_else(|| answers.get(&q.id))
                    {
                        Some(raw.0.clone())
                    } else {
                        let canonical = crate::staging::canonical_target(&path)
                            .map_err(|e| format!("{}: {e}", path.display()))?;
                        if !canonical.as_path().starts_with(bound) {
                            return Err(format!(
                                "{}: document input must stay inside target repository or target",
                                path.display()
                            ));
                        }
                        match fs::read_to_string(canonical.as_path()) {
                            Ok(text) => Some(
                                serde_json::from_str(&text)
                                    .map_err(|e| format!("{}: {e}", path.display()))?,
                            ),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                            Err(e) => return Err(format!("{}: {e}", path.display())),
                        }
                    };
                    if let Some(value) = &value {
                        contract
                            .validate(value)
                            .map_err(|e| format!("{}: {e}", path.display()))?;
                    }
                    contract.value = value;
                    contract.target = Some(target.to_owned());
                }
            }
            Node::Group(g) => prepare_nodes(&mut g.nodes, target, bound, routed, answers)?,
            _ => {}
        }
    }
    Ok(())
}
/// Restore accepted staged documents only where this run admitted no document.
/// Current inputs and default files retain precedence over staged replay.
pub fn restore_missing(template: &mut Template, recorded: &RawAnswers) -> Result<(), String> {
    fn restore(nodes: &mut [Node], recorded: &RawAnswers) -> Result<(), String> {
        for node in nodes {
            match node {
                Node::Question(q) => {
                    if let QuestionKind::Document { contract } = &mut q.kind
                        && contract.value.is_none()
                        && let Some(raw) = recorded.get(&q.id)
                    {
                        contract
                            .validate(&raw.0)
                            .map_err(|e| format!("staged document {}: {e}", q.id))?;
                        contract.value = Some(raw.0.clone());
                    }
                }
                Node::Group(g) => restore(&mut g.nodes, recorded)?,
                _ => {}
            }
        }
        Ok(())
    }
    restore(&mut template.interview, recorded)
}

/// Read one external document as a JSON object.
pub fn read(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if !value.is_object() {
        return Err(format!("{}: must be a JSON object", path.display()));
    }
    Ok(value)
}

pub(crate) fn admitted(template: &Template) -> RawAnswers {
    template
        .document_question_ids()
        .into_iter()
        .filter_map(|id| {
            let QuestionKind::Document { contract } = &template.question(&id)?.kind else {
                return None;
            };
            contract.value.clone().map(|v| (id, RawAnswer(v)))
        })
        .collect::<IndexMap<Id, RawAnswer>>()
}
