// ---
// relationships:
//   implements: architecture
// ---
//! The generate axis (`--like`) CLI-side boundary: pure, reader-only snapshot
//! selection.
//!
//! This is the only code outside `toha::snapshot` that names `Snapshot` or
//! `Project` on the generate axis. It reads snapshot refs and writes none; it
//! never prompts (the person picker lives in the CLI adapter and calls these
//! functions), so the surface is crate-callable and free of UI. A selected
//! snapshot's recorded answers are folded to one default per id and handed to
//! the interview engine as a [`toha::interview::SnapshotSeed`]; no `Snapshot`,
//! `Project`, ULID, or gitoxide type reaches the engine.

use indexmap::IndexMap;
use serde_json::Value;
use std::fmt;

use toha::{
    Id, RawAnswer,
    interview::SnapshotSeed,
    snapshot::{Listed, Project, ProjectError, Snapshot, SnapshotError, SnapshotId},
};

/// How `--like` names the snapshot to seed from, parsed once from the CLI. A
/// bare `--like` (no value) is a person-route picker resolved in the CLI
/// adapter to one of these or to "none"; it is a usage error on the script and
/// agent routes, so it never reaches these pure functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LikeSelector {
    /// A full id or a >= 6-char prefix; must resolve to exactly one snapshot of
    /// the template's source identity.
    Reference(String),
    /// The newest snapshot of the template's source in this project (max ULID).
    Latest,
}

/// The resolved seed: the engine-facing folded defaults plus the snapshot id and
/// a human label, for reporting and the staged record. Carries no `Snapshot`.
#[derive(Debug, Clone)]
pub struct LikeSeed {
    /// What the engine consumes: one default per id, plus the snapshot id as
    /// provenance.
    pub engine: SnapshotSeed,
    /// The selected snapshot's id, for the staged record's pin and the result
    /// document's `seed: { from }`.
    pub id: SnapshotId,
    /// The one-line human description a person and agent route print.
    pub label: String,
}

/// Why a `--like` request could not be honoured. Each variant names the snapshot
/// or the reason; none carries an answer value or file content. Every variant is
/// exit 1 (a `snapshot` or route error), never exit 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LikeError {
    /// The target is not inside a git repository and a selector was explicit.
    NoProject,
    /// A required selector (`latest` or a reference) matched no snapshot of the
    /// template's source.
    Missing { source: String },
    /// A reference (id or prefix) matched nothing, or was too short or malformed.
    Unknown { reference: String },
    /// A reference prefix matched several snapshots.
    Ambiguous { reference: String },
    /// The selected snapshot's source is not the template's source identity.
    WrongSource {
        want: String,
        got: String,
        id: SnapshotId,
    },
}

impl fmt::Display for LikeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LikeError::NoProject => write!(
                f,
                "the target is not inside a git repository, so there is no snapshot to seed from"
            ),
            LikeError::Missing { source } => {
                write!(f, "no snapshot of {source} to seed from")
            }
            LikeError::Unknown { reference } => {
                write!(f, "no snapshot matches {reference}")
            }
            LikeError::Ambiguous { reference } => {
                write!(
                    f,
                    "snapshot prefix {reference} matches more than one snapshot"
                )
            }
            LikeError::WrongSource { want, got, id } => write!(
                f,
                "snapshot {id} is of source {got}, not the template's source {want}"
            ),
        }
    }
}

/// Snapshots of `source` in this repository, newest first (ULID descending).
/// Reads the repository-wide snapshot set (all `refs/toha/snapshots/*`) through
/// the producer reader and keeps the valid ones whose `source()` equals
/// `source`. Repository-wide, not target-scoped: a prior application lives at a
/// different subpath, so its snapshot's `target` differs from the new one.
pub fn candidates(project: &Project, source: &str) -> Result<Vec<Snapshot>, ProjectError> {
    Ok(project
        .snapshots_all()?
        .into_iter()
        .filter_map(|entry| match entry {
            Listed::Valid(snapshot) => Some(snapshot),
            Listed::Invalid { .. } => None,
        })
        .filter(|snapshot| snapshot.source() == source)
        .collect())
}

/// The automatic, non-interactive choice (script/agent routes; preselected for
/// the person): the newest snapshot of the source, or `None` when there are
/// none. Total and deterministic because ULIDs are a total creation order.
pub fn latest(candidates: &[Snapshot]) -> Option<&Snapshot> {
    candidates.iter().max_by(|a, b| a.id().cmp(b.id()))
}

/// Resolve an explicit `--like <id/prefix>` against the repository and verify the
/// source. Fails clearly per each [`LikeError`]. Never prompts.
pub fn find_required(
    project: Option<&Project>,
    source: &str,
    reference: &str,
) -> Result<Snapshot, LikeError> {
    let project = project.ok_or(LikeError::NoProject)?;
    let snapshot = match project.find(reference) {
        Ok(snapshot) => snapshot,
        Err(SnapshotError::Ambiguous { .. }) => {
            return Err(LikeError::Ambiguous {
                reference: reference.to_owned(),
            });
        }
        // Unknown (matched nothing), a too-short or malformed prefix, and any
        // read fault all mean no usable snapshot is named by this reference.
        Err(_) => {
            return Err(LikeError::Unknown {
                reference: reference.to_owned(),
            });
        }
    };
    // Source identity, not formal name: a same-source snapshot seeds across
    // template versions; a foreign source is refused.
    if snapshot.source() != source {
        return Err(LikeError::WrongSource {
            want: source.to_owned(),
            got: snapshot.source().to_owned(),
            id: *snapshot.id(),
        });
    }
    Ok(snapshot)
}

/// Fold a snapshot's raw submissions into one default per id. Later batches win;
/// a looped question's recorded JSON array stays one array value under its id (a
/// valid loop default). Pure, total, no validation — the new interview
/// re-validates and re-formats on submit through the unchanged engine.
pub fn fold_submissions(submissions: &[IndexMap<String, Value>]) -> IndexMap<Id, RawAnswer> {
    let mut out: IndexMap<Id, RawAnswer> = IndexMap::new();
    for batch in submissions {
        for (key, value) in batch {
            // A recorded key that is not a valid question id cannot match any
            // question, so it is skipped; the fold stays total.
            if let Ok(id) = Id::parse(key) {
                out.insert(id, RawAnswer(value.clone()));
            }
        }
    }
    out
}

/// Build the resolved seed from a selected snapshot: its folded defaults, its id
/// (for the pin and the result document), and a one-line label for the person
/// and agent routes.
pub fn seed(snapshot: &Snapshot) -> LikeSeed {
    let id = *snapshot.id();
    LikeSeed {
        engine: SnapshotSeed {
            defaults: fold_submissions(snapshot.submissions()),
            from: id.to_string(),
        },
        id,
        label: format!("{id} ({}, {})", snapshot.source(), snapshot.target()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;
    use toha::snapshot::{
        CommitId, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision, SnapshotDoc,
        Timestamp,
    };

    /// Build a validated in-memory snapshot (no git) from an id, source template,
    /// target, and recorded submission batches.
    fn snapshot(
        id_text: &str,
        template: &str,
        target: &str,
        submissions: Vec<IndexMap<String, Value>>,
    ) -> Snapshot {
        let id: SnapshotId = id_text.parse().unwrap();
        let doc = SnapshotDoc::new(
            id,
            template.to_owned(),
            Revision::Unversioned,
            RepoPath::parse(target).unwrap(),
            Timestamp::parse("2026-09-30T04:12:00Z").unwrap(),
            FrozenNow::parse("2026-03-14T09:26:53+00:00[UTC]").unwrap(),
            ProjectPoint::new(
                CommitId::parse("a41c0de00000000000000000000000000000beef").unwrap(),
                None,
            ),
            None,
            submissions,
            vec![PathOwnership::new(
                toha::TargetPath::parse("x.txt").unwrap(),
                Origin::Toha,
            )],
        );
        let bytes = doc.to_json_bytes();
        let files: BTreeSet<String> = ["x.txt".to_owned()].into_iter().collect();
        Snapshot::validate(id, &bytes, &files).unwrap()
    }

    fn batch(pairs: &[(&str, Value)]) -> IndexMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    const ID_A: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6E";
    const ID_B: &str = "01JA2B8M4R0C7W1Y5F3H9K2S6F";

    #[test]
    fn latest_picks_the_maximum_ulid_regardless_of_input_order() {
        let older = snapshot(ID_A, "gh:example/widget", "a", vec![]);
        let newer = snapshot(ID_B, "gh:example/widget", "b", vec![]);
        // Input order does not matter: the max ULID wins.
        let ascending = vec![older, newer];
        assert_eq!(latest(&ascending).unwrap().id().to_string(), ID_B);
        let descending: Vec<Snapshot> = ascending.into_iter().rev().collect();
        assert_eq!(latest(&descending).unwrap().id().to_string(), ID_B);
        assert!(latest(&[]).is_none());
    }

    #[test]
    fn fold_takes_the_last_value_per_id_and_keeps_a_looped_array_whole() {
        let submissions = vec![
            batch(&[("label", json!("first")), ("style", json!("card"))]),
            batch(&[
                ("label", json!("second")),
                ("items", json!(["a", "b", "c"])),
            ]),
        ];
        let folded = fold_submissions(&submissions);
        // Later batch wins for a repeated id.
        assert_eq!(folded[&Id::parse("label").unwrap()].0, json!("second"));
        // A first-batch-only id survives.
        assert_eq!(folded[&Id::parse("style").unwrap()].0, json!("card"));
        // A looped question's recorded array stays one array value.
        assert_eq!(
            folded[&Id::parse("items").unwrap()].0,
            json!(["a", "b", "c"])
        );
    }

    #[test]
    fn seed_carries_the_id_label_and_folded_defaults() {
        let snap = snapshot(
            ID_A,
            "gh:example/widget@v1",
            "src/widgets/alpha",
            vec![batch(&[("label", json!("Alpha"))])],
        );
        let built = seed(&snap);
        assert_eq!(built.id.to_string(), ID_A);
        assert_eq!(built.engine.from, ID_A);
        assert_eq!(
            built.engine.defaults[&Id::parse("label").unwrap()].0,
            json!("Alpha")
        );
        assert!(built.label.contains(ID_A));
        assert!(built.label.contains("gh:example/widget"));
        assert!(built.label.contains("src/widgets/alpha"));
    }
}
