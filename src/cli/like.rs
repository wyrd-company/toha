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

    // ----------------------------------------------------------------------
    // Git-backed source-identity behavior (behaviors 4 and 17), exercising the
    // real `Project` reader through `candidates` and `find_required`.
    // ----------------------------------------------------------------------

    use std::path::Path;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn git_out(repo: &Path, index: Option<&Path>, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(repo).args(args);
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        let out = cmd
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn hash_object(repo: &Path, bytes: &[u8]) -> String {
        use std::io::Write;
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_owned()
    }

    /// Write one snapshot ref (`snapshot.json` + `files/x.txt`) from an
    /// in-memory doc, so the real reader validates it.
    fn write_ref(repo: &Path, id: &str, template: &str, target: &str) {
        let doc = SnapshotDoc::new(
            id.parse().unwrap(),
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
            vec![batch(&[("label", json!("Alpha"))])],
            vec![PathOwnership::new(
                toha::TargetPath::parse("x.txt").unwrap(),
                Origin::Toha,
            )],
        );
        let index = repo.join(format!(".idx-{id}"));
        let _ = std::fs::remove_file(&index);
        let json_oid = hash_object(repo, &doc.to_json_bytes());
        let file_oid = hash_object(repo, b"x\n");
        git_out(
            repo,
            Some(&index),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{json_oid},snapshot.json"),
            ],
        );
        git_out(
            repo,
            Some(&index),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{file_oid},files/x.txt"),
            ],
        );
        let tree = git_out(repo, Some(&index), &["write-tree"]);
        let commit = git_out(repo, None, &["commit-tree", &tree, "-m", "snapshot"]);
        git_out(
            repo,
            None,
            &["update-ref", &format!("refs/toha/snapshots/{id}"), &commit],
        );
        let _ = std::fs::remove_file(&index);
    }

    fn open(repo: &Path, sub: &str) -> Project {
        let target = toha::staging::canonical_target(&repo.join(sub)).unwrap();
        Project::open(&target).unwrap().expect("in git")
    }

    #[test]
    fn candidates_and_find_required_match_on_source_identity_across_versions() {
        // Behaviors 4 (source identity, not formal name) and 17 (repository-wide
        // source-filtered selection): a snapshot recorded at `@v1` seeds an apply
        // of the same source at another version; a foreign source is refused and
        // is not a candidate.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("seed.txt"), "seed\n").unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "seed"]);
        // Two applications of one source at different subpaths and a foreign one.
        write_ref(repo, ID_A, "gh:example/widget@v1", "src/widgets/alpha");
        write_ref(repo, ID_B, "gh:example/widget@v2", "src/widgets/beta");
        write_ref(
            repo,
            "01JA2B8M4R0C7W1Y5F3H9K2S70",
            "gh:example/gadget@v1",
            "src/gadgets/one",
        );

        // Selection runs from a fresh, not-yet-created subpath.
        let project = open(repo, "src/widgets/gamma");

        // The source `gh:example/widget` matches both versions, newest first, and
        // excludes the foreign `gh:example/gadget`.
        let cands = candidates(&project, "gh:example/widget").unwrap();
        let ids: Vec<String> = cands.iter().map(|s| s.id().to_string()).collect();
        assert_eq!(ids, vec![ID_B.to_owned(), ID_A.to_owned()], "{ids:?}");

        // find_required accepts the `@v1` snapshot for the bare source identity.
        let found = find_required(Some(&project), "gh:example/widget", ID_A).unwrap();
        assert_eq!(found.id().to_string(), ID_A);

        // A foreign source is refused with WrongSource, naming both sources.
        let err = find_required(
            Some(&project),
            "gh:example/widget",
            "01JA2B8M4R0C7W1Y5F3H9K2S70",
        )
        .unwrap_err();
        assert!(
            matches!(err, LikeError::WrongSource { .. }),
            "foreign source refused: {err:?}"
        );
    }
}
