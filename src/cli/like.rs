// ---
// relationships:
//   implements: applied-record
// ---
//! Seed defaults from an application of the same source, selected by target.
use serde_json::Value;
use toha::{
    Id, RawAnswer,
    applied::{Application, Project, ProjectError},
    interview::ApplicationSeed,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LikeSelector {
    Reference(String),
}
#[derive(Debug, Clone)]
pub struct LikeSeed {
    pub engine: ApplicationSeed,
    pub id: String,
    pub label: String,
    pub entry: Value,
}
#[derive(Debug, thiserror::Error)]
pub enum LikeError {
    #[error("the target is not inside a git repository; --like needs a recorded application")]
    NoProject,
    #[error(
        "no application of {name} at target {target}; --like selects a target in the applied record"
    )]
    Missing { name: String, target: String },
    #[error("{0}")]
    Record(String),
}
pub fn candidates(project: &Project, source: &str) -> Result<Vec<Application>, ProjectError> {
    Ok(project
        .applications()?
        .into_iter()
        .filter(|a| a.source() == source)
        .collect())
}
pub fn find_required(
    project: Option<&Project>,
    source: &str,
    target: &str,
) -> Result<Application, LikeError> {
    let project = project.ok_or(LikeError::NoProject)?;
    candidates(project, source)
        .map_err(|e| LikeError::Record(e.to_string()))?
        .into_iter()
        .find(|a| a.target().as_str() == target)
        .ok_or_else(|| LikeError::Missing {
            name: source.into(),
            target: target.into(),
        })
}
pub fn seed(application: &Application) -> LikeSeed {
    let target = application.target().to_string();
    LikeSeed {
        engine: ApplicationSeed {
            defaults: application
                .answers
                .iter()
                .filter_map(|(key, value)| {
                    Id::parse(key).ok().map(|id| (id, RawAnswer(value.clone())))
                })
                .collect(),
            from: target.clone(),
        },
        id: target.clone(),
        label: format!("{target} ({})", application.source()),
        entry: application.to_value(),
    }
}
