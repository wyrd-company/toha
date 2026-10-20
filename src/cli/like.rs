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

/// Legacy IDs remain selectors while their released commands are supported.
pub fn is_legacy_reference(reference: &str) -> bool {
    (6..=26).contains(&reference.len())
        && reference
            .bytes()
            .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte.to_ascii_uppercase()))
}

/// Read legacy refs only for an explicit legacy selector. The copied application
/// is an ephemeral seed envelope; its target retains the legacy provenance ID
/// and is never published into the applied record.
pub fn legacy_seed(
    target: &toha::staging::CanonicalTarget,
    source: &str,
    selector: &str,
) -> Result<LikeSeed, String> {
    let project = toha::snapshot::Project::open(target)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "the target is not inside a git repository; --like needs a recorded application"
                .to_owned()
        })?;
    let snapshot = if selector == "latest" {
        project
            .snapshots_all()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|entry| match entry {
                toha::snapshot::Listed::Valid(snapshot) if snapshot.source() == source => {
                    Some(snapshot)
                }
                _ => None,
            })
            .max_by_key(|snapshot| *snapshot.id())
            .ok_or_else(|| format!("no snapshot of {source} to seed from"))?
    } else {
        project.find(selector).map_err(|e| e.to_string())?
    };
    if snapshot.source() != source {
        return Err(format!(
            "snapshot {} is of source {}, not {source}",
            snapshot.id(),
            snapshot.source()
        ));
    }
    let revision = match snapshot.revision() {
        toha::snapshot::Revision::Commit(commit) => toha::applied::Revision::Commit(
            toha::applied::CommitId::parse(commit.as_str()).map_err(|e| e.to_string())?,
        ),
        toha::snapshot::Revision::Content(digest) => toha::applied::Revision::Content(
            toha::applied::ContentDigest::parse(digest.as_str()).map_err(|e| e.to_string())?,
        ),
        toha::snapshot::Revision::Unversioned => toha::applied::Revision::Unversioned,
    };
    let application = Application::new(
        snapshot.template().into(),
        revision,
        toha::applied::RepoPath::parse(&snapshot.id().to_string()).map_err(|e| e.to_string())?,
        toha::applied::FrozenNow::parse(&snapshot.generated().to_string())
            .map_err(|e| e.to_string())?,
        snapshot
            .submissions()
            .iter()
            .flat_map(|batch| batch.iter().map(|(id, value)| (id.clone(), value.clone())))
            .collect(),
        Default::default(),
        vec![],
        vec![],
    );
    Ok(seed(&application))
}
