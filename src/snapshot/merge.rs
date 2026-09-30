// ---
// relationships:
//   implements: architecture
// ---
//! The in-process three-way merge that applies a template update to the
//! operator's working tree and index.
//!
//! The three trees are whole-repository trees so conflict paths are
//! repository-relative and nothing outside the target can change: base is
//! `HEAD` with the target replaced by the base snapshot's `files/` (empty for a
//! baseline), ours is `HEAD` (the operator), theirs is `HEAD` with the target
//! replaced by the new snapshot's `files/`. The operator is "ours", so index
//! stage 2 holds the operator's bytes. Toha never runs a merge or filter driver;
//! a path whose attributes name one is kept as the operator's content and
//! reported as a `driver` conflict.

use std::num::NonZeroU8;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use gix::bstr::{BString, ByteSlice};
use gix::filter::plumbing::pipeline::convert::ToWorktreeOutcome;
use gix::objs::tree::EntryKind;

use crate::snapshot::capture::{self, CaptureInputs};
use crate::snapshot::project::Project;
use crate::snapshot::record::{
    CommitId, FrozenNow, ProjectPoint, RepoPath, Revision, SNAPSHOT_REF_PREFIX, Snapshot,
    SnapshotId, Timestamp,
};

/// The base an update merges from.
#[allow(clippy::large_enum_variant)]
pub enum Base {
    Snapshot(Snapshot),
    Empty,
}

/// Options for an update merge.
pub struct MergeOptions {
    pub trusted: bool,
    pub dry_run: bool,
}

/// The outcome of an update.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Merged {
    AlreadyCurrent,
    NeedsTrust,
    Planned {
        changes: Vec<Change>,
    },
    Written {
        snapshot: SnapshotId,
        changes: Vec<Change>,
        conflicted: Vec<RepoPath>,
    },
}

/// One path the update changed, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: RepoPath,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Added,
    Updated,
    Merged,
    Deleted,
    Conflicted(ConflictKind),
    NotPreviewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Content,
    AddAdd,
    ModifyDelete,
    FileDirectory,
    Binary,
    Driver,
}

/// A failure of the merge transaction. Every variant names paths or a
/// diagnostic and never an answer value or file content.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("git error during merge: {0}")]
    Git(String),
    #[error("a path the update would write is occupied: {0:?}")]
    Occupied(Vec<RepoPath>),
    #[error("the target or index changed during the update")]
    Changed,
    #[error("{0}")]
    Io(String),
}

fn git(err: impl std::fmt::Display) -> MergeError {
    MergeError::Git(err.to_string())
}

/// What the update records besides its files. The runtime supplies the id and
/// created instant; `generated` is carried from the base so a date-rendering
/// template stays byte-stable across updates.
pub struct SnapshotInputs {
    pub template: String,
    pub revision: Revision,
    pub generated: FrozenNow,
    pub submissions: Vec<indexmap::IndexMap<crate::template::Id, serde_json::Value>>,
}

/// Apply a template update to the project: build the candidate snapshot through
/// the throwaway checkout, stop early when the result is already current, and
/// otherwise merge it into the working tree. `--from` merges from a base
/// snapshot; `--baseline` merges from an empty base.
#[allow(clippy::too_many_arguments)]
pub fn merge_apply(
    project: &Project,
    base: Base,
    template: &crate::template::Template,
    completed: &crate::interview::Completed,
    inputs: SnapshotInputs,
    options: MergeOptions,
    runner: &dyn crate::hook::HookRunner,
) -> Result<Merged, MergeError> {
    // Build the plan for the real target with the snapshot's frozen instant.
    let target_path = candidate_target(project)?;
    let plan = crate::plan::Plan::build(template, completed, &target_path)
        .map_err(|err| MergeError::Git(err.to_string()))?;

    // Untrusted hooks: report the plan without running anything.
    if !options.trusted && !plan.hooks.is_empty() {
        return Ok(Merged::NeedsTrust);
    }

    let capture_inputs = capture_inputs(project, &inputs)?;
    let new = build_candidate(
        project,
        &base,
        plan,
        capture_inputs,
        !options.dry_run,
        runner,
    )?;

    let published = published_commit(project, new.id())
        .unwrap_or_else(|| gix::ObjectId::null(project.repo().object_hash()));

    // Already current: the new snapshot equals the base — discard it and stop.
    if let Base::Snapshot(base_snapshot) = &base {
        if is_already_current(project, base_snapshot, &new, &inputs)? {
            remove_candidate(project, new.id(), published)?;
            return Ok(Merged::AlreadyCurrent);
        }
    }

    let result = merge_into_worktree(project, &base, &new, &options);
    if options.dry_run {
        // A dry run saves nothing: remove the candidate ref the builder created.
        remove_candidate(project, new.id(), published)?;
    }
    result
}

/// The candidate target: a `CanonicalTarget` for the project's real target.
fn candidate_target(project: &Project) -> Result<crate::staging::CanonicalTarget, MergeError> {
    let repo = project.repo();
    let workdir = repo
        .workdir()
        .ok_or_else(|| MergeError::Io("bare".into()))?;
    let path = if project.target().is_root() {
        workdir.to_owned()
    } else {
        workdir.join(project.target().as_str())
    };
    crate::staging::canonical_target(&path).map_err(|err| MergeError::Io(err.to_string()))
}

/// Build the capture inputs, stamping the id, created instant, and HEAD point.
fn capture_inputs(project: &Project, inputs: &SnapshotInputs) -> Result<CaptureInputs, MergeError> {
    let repo = project.repo();
    let head = repo.head_id().map_err(git)?.detach();
    let commit =
        CommitId::parse(&head.to_hex().to_string()).map_err(|e| MergeError::Git(e.to_string()))?;
    let branch = repo
        .head_name()
        .map_err(git)?
        .map(|name| name.shorten().to_string());
    Ok(CaptureInputs {
        id: SnapshotId::generate(),
        template: inputs.template.clone(),
        revision: inputs.revision.clone(),
        generated: inputs.generated.clone(),
        created: Timestamp::now(),
        project: ProjectPoint::new(commit, branch),
        submissions: inputs
            .submissions
            .iter()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.as_str().to_owned(), v.clone()))
                    .collect()
            })
            .collect(),
    })
}

/// Build the candidate snapshot through the throwaway checkout: check out HEAD,
/// retract uncovered base ownership, apply the plan (and hooks unless it is a dry
/// run), then capture with the base. The checkout is deleted afterwards.
fn build_candidate(
    project: &Project,
    base: &Base,
    plan: crate::plan::Plan,
    inputs: CaptureInputs,
    run_hooks: bool,
    runner: &dyn crate::hook::HookRunner,
) -> Result<Snapshot, MergeError> {
    let repo = project.repo();
    let target = project.target().clone();
    let checkout = tempfile::tempdir().map_err(|e| MergeError::Io(e.to_string()))?;

    checkout_head(repo, checkout.path())?;

    // Group the plan once, since Plan::apply consumes it.
    let plan_paths = capture::group_plan(&plan);

    // Step 3: retract uncovered base ownership on the checkout before the apply.
    let base_snapshot = match base {
        Base::Snapshot(snapshot) => Some(snapshot),
        Base::Empty => None,
    };
    if let Some(base_snapshot) = base_snapshot {
        capture::retract_uncovered(checkout.path(), &target, &plan_paths, base_snapshot)
            .map_err(|e| MergeError::Git(e.to_string()))?;
    }

    // Step 4: apply the plan into the checkout, running hooks unless it is a dry run.
    let checkout_target_path = if target.is_root() {
        checkout.path().to_owned()
    } else {
        checkout.path().join(target.as_str())
    };
    let checkout_target = crate::staging::canonical_target(&checkout_target_path)
        .map_err(|e| MergeError::Io(e.to_string()))?;
    plan.apply(
        &checkout_target,
        crate::apply::ApplyOptions {
            force: true,
            trusted: run_hooks,
        },
        runner,
    )
    .map_err(|e| MergeError::Git(format!("apply: {e}")))?;
    eprintln!("DBG after apply");

    // Step 6: capture the new snapshot from the checkout, then read it back.
    let id = capture::capture(
        repo,
        checkout.path(),
        &target,
        &plan_paths,
        base_snapshot,
        inputs,
    )
    .map_err(|e| MergeError::Git(e.to_string()))?;

    // Read the captured snapshot back through the validating reader.
    project
        .find(&id.to_string())
        .map_err(|e| MergeError::Git(e.to_string()))
}

/// Whether the new snapshot equals the base: same revision, submissions, and
/// `files/` tree, so nothing would change.
fn is_already_current(
    project: &Project,
    base: &Snapshot,
    new: &Snapshot,
    inputs: &SnapshotInputs,
) -> Result<bool, MergeError> {
    let same_revision = base.revision() == &inputs.revision;
    let base_files = snapshot_files_tree(project, base.id())?;
    let new_files = snapshot_files_tree(project, new.id())?;
    Ok(same_revision && base.submissions() == new.submissions() && base_files == new_files)
}

/// Check out `HEAD`'s whole tree into `dest`, with no filter driver (drivers are
/// stripped on the repository), never a linked worktree.
fn checkout_head(repo: &gix::Repository, dest: &Path) -> Result<(), MergeError> {
    let tree_id = repo.head_tree_id().map_err(git)?;
    let mut index = repo.index_from_tree(&tree_id).map_err(git)?;
    let mut opts = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(git)?;
    opts.destination_is_initially_empty = true;
    std::fs::create_dir_all(dest).map_err(|e| MergeError::Io(e.to_string()))?;
    gix::worktree::state::checkout(
        &mut index,
        dest,
        repo.objects.clone().into_arc().map_err(git)?,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &AtomicBool::new(false),
        opts,
    )
    .map_err(git)?;
    Ok(())
}

/// Merge the new snapshot into the operator's working tree against `base`. The
/// new snapshot must already be captured (its ref exists); on any unsuccessful
/// exit its candidate ref is removed.
pub(crate) fn merge_into_worktree(
    project: &Project,
    base: &Base,
    new: &Snapshot,
    options: &MergeOptions,
) -> Result<Merged, MergeError> {
    // The commit this invocation published, so cleanup only removes the ref while
    // it still points there (a concurrent replacement is preserved).
    let published = published_commit(project, new.id());
    let result = merge_inner(project, base, new, options);
    if !options.dry_run && !matches!(result, Ok(Merged::Written { .. })) {
        // Any unsuccessful post-capture exit removes this candidate ref. A cleanup
        // failure keeps the original error and names the ref that remains.
        if let (Err(original), Some(published)) = (&result, published) {
            if let Err(cleanup) = remove_candidate(project, new.id(), published) {
                return Err(MergeError::Io(format!(
                    "{original}; additionally, cleanup of {}{} failed: {cleanup}",
                    SNAPSHOT_REF_PREFIX,
                    new.id()
                )));
            }
        }
    }
    result
}

/// The commit a snapshot's ref currently points at, if it exists.
fn published_commit(project: &Project, id: &SnapshotId) -> Option<gix::ObjectId> {
    let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
    project
        .repo()
        .find_reference(name.as_str())
        .ok()?
        .peel_to_commit()
        .ok()
        .map(|commit| commit.id().detach())
}

fn merge_inner(
    project: &Project,
    base: &Base,
    new: &Snapshot,
    options: &MergeOptions,
) -> Result<Merged, MergeError> {
    let repo = project.repo();
    let target = project.target();

    // Step 0: record HEAD and the index checksum, so the locked re-check before
    // the write can refuse if either drifted.
    let head_start = repo.head_id().map_err(git)?.detach();
    let index_digest_start = index_digest(repo);

    let head_tree = repo.head_tree().map_err(git)?.id().detach();
    let base_files = match base {
        Base::Snapshot(snapshot) => snapshot_files_tree(project, snapshot.id())?,
        Base::Empty => gix::ObjectId::empty_tree(repo.object_hash()),
    };
    let new_files = snapshot_files_tree(project, new.id())?;

    let base_full = graft(project, head_tree, target, base_files)?;
    let theirs_full = graft(project, head_tree, target, new_files)?;
    let ours_full = head_tree;

    // Options: take the repo config, force diff3 markers, disable rename tracking.
    let mut opts: gix::merge::plumbing::tree::Options =
        repo.tree_merge_options().map_err(git)?.into();
    opts.rewrites = None;
    opts.blob_merge.text.conflict = gix::merge::blob::builtin_driver::text::Conflict::Keep {
        style: gix::merge::blob::builtin_driver::text::ConflictStyle::Diff3,
        marker_size: NonZeroU8::new(7).expect("nonzero"),
    };
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: Some("toha (old template)".into()),
        current: Some("operator".into()),
        other: Some("toha (new template)".into()),
    };

    let mut outcome = repo
        .merge_trees(base_full, ours_full, theirs_full, labels, opts.into())
        .map_err(git)?;
    let how = gix::merge::tree::TreatAsUnresolved::git();

    // Compute the change list from the merged tree vs the operator's target.
    let merged_full = outcome.tree.write().map_err(git)?.detach();
    let theirs_sub = subtree(project, theirs_full, target)?;
    let merged_sub = subtree(project, merged_full, target)?;
    let ours_sub = subtree(project, ours_full, target)?;
    let ours_entries = flat_entries(project, ours_sub)?;
    let merged_entries = flat_entries(project, merged_sub)?;
    let theirs_entries = flat_entries(project, theirs_sub)?;

    let base_sub = subtree(project, base_full, target)?;
    let base_entries = flat_entries(project, base_sub)?;

    // Classify each unresolved conflict by kind, keyed by repository-relative path.
    let mut conflict_kinds: std::collections::BTreeMap<String, ConflictKind> =
        std::collections::BTreeMap::new();
    for c in outcome.conflicts.iter().filter(|c| c.is_unresolved(how)) {
        let location = c.ours.location().to_str_lossy().into_owned();
        conflict_kinds.insert(location, classify_conflict(repo, c));
    }

    // A path whose attributes name a merge or filter driver is never merged: keep
    // the operator's content, force a `driver` conflict, and stage 1/2/3 manually.
    let mut driver_stages: DriverStages = std::collections::BTreeMap::new();
    for path in driver_attributed_paths(repo, target, &base_entries, &theirs_entries)? {
        let repo_rel = repo_path_for(target, &path).as_str().to_owned();
        conflict_kinds.insert(repo_rel.clone(), ConflictKind::Driver);
        driver_stages.insert(
            repo_rel,
            [
                base_entries.get(&path).copied(),
                ours_entries.get(&path).copied(),
                theirs_entries.get(&path).copied(),
            ],
        );
    }

    let conflicted: Vec<RepoPath> = conflict_kinds
        .keys()
        .filter_map(|p| RepoPath::parse(p).ok())
        .collect();

    let mut changes = Vec::new();
    for (path, (id, mode)) in &merged_entries {
        let repo_path = repo_path_for(target, path);
        let action = if let Some(kind) = conflict_kinds.get(repo_path.as_str()) {
            Action::Conflicted(*kind)
        } else if let Some((ours_id, _)) = ours_entries.get(path) {
            if ours_id == id {
                continue; // unchanged for the operator
            } else if theirs_entries.contains_key(path) {
                Action::Merged
            } else {
                Action::Updated
            }
        } else {
            Action::Added
        };
        let _ = mode;
        changes.push(Change {
            path: repo_path,
            action,
        });
    }
    for path in ours_entries.keys() {
        if !merged_entries.contains_key(path) {
            let repo_path = repo_path_for(target, path);
            let action = match conflict_kinds.get(repo_path.as_str()) {
                Some(kind) => Action::Conflicted(*kind),
                None => Action::Deleted,
            };
            changes.push(Change {
                path: repo_path,
                action,
            });
        }
    }
    changes.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));

    if options.dry_run {
        return Ok(Merged::Planned { changes });
    }

    // Occupied-path refusal: a path the merge adds where the operator has an
    // untracked file or directory on disk.
    let workdir = repo
        .workdir()
        .ok_or_else(|| MergeError::Io("bare".into()))?;
    let mut occupied = Vec::new();
    for path in merged_entries.keys() {
        if !ours_entries.contains_key(path) {
            let repo_path = repo_path_for(target, path);
            let on_disk = workdir.join(repo_path.as_str());
            if on_disk.exists() {
                occupied.push(repo_path);
            }
        }
    }
    if !occupied.is_empty() {
        return Err(MergeError::Occupied(occupied));
    }

    // Paths whose conflict is not a content merge keep the operator's file on
    // disk (their diff3 body is not written); a content conflict writes markers.
    let keep_operator: std::collections::BTreeSet<String> = conflict_kinds
        .iter()
        .filter(|(_, kind)| !matches!(kind, ConflictKind::Content))
        .map(|(path, _)| path.clone())
        .collect();

    // Steps 6-7: take the index lock, re-check for drift, then write the merged
    // files and index in one transaction that rolls back on any write failure.
    commit_transaction(
        project,
        &ours_entries,
        &merged_entries,
        target,
        &keep_operator,
        &driver_stages,
        &mut outcome,
        merged_full,
        how,
        head_start,
        index_digest_start,
    )?;

    Ok(Merged::Written {
        snapshot: *new.id(),
        changes,
        conflicted,
    })
}

/// The digest of the on-disk index, or `None` when it cannot be read (an unborn
/// repository has no index yet).
fn index_digest(repo: &gix::Repository) -> Option<Vec<u8>> {
    let path = repo.git_dir().join("index");
    std::fs::read(path).ok().map(|bytes| {
        use sha2::{Digest, Sha256};
        Sha256::digest(&bytes).to_vec()
    })
}

/// One path written or deleted during the transaction, with what it needs to be
/// restored to `HEAD` if the transaction rolls back.
struct Restorable {
    repo_rel: String,
    /// The `HEAD` blob to restore (a modify or a delete), or `None` for an add
    /// (which is removed on rollback).
    head: Option<(gix::ObjectId, gix::index::entry::Mode)>,
    /// The git blob id Toha wrote, so rollback only touches a path still holding
    /// Toha's bytes.
    wrote: Option<gix::ObjectId>,
}

/// Take the index lock, re-check HEAD/index/cleanliness, write the merged target
/// files (each only after checking the path still holds its HEAD content or is
/// absent) and the index, and roll back on any failure.
#[allow(clippy::too_many_arguments)]
fn commit_transaction(
    project: &Project,
    ours: &Entries,
    merged: &Entries,
    target: &RepoPath,
    keep_operator: &std::collections::BTreeSet<String>,
    driver_stages: &DriverStages,
    outcome: &mut gix::merge::tree::Outcome<'_>,
    merged_full: gix::ObjectId,
    how: gix::merge::tree::TreatAsUnresolved,
    head_start: gix::ObjectId,
    index_digest_start: Option<Vec<u8>>,
) -> Result<(), MergeError> {
    let repo = project.repo();
    let workdir = repo
        .workdir()
        .ok_or_else(|| MergeError::Io("bare".into()))?
        .to_owned();
    let index_path = repo.git_dir().join("index");

    // Step 6: take the index lock, then re-check for drift under it.
    let lock = gix::lock::File::acquire_to_update_resource(
        &index_path,
        gix::lock::acquire::Fail::Immediately,
        None,
    )
    .map_err(|err| MergeError::Io(err.to_string()))?;

    if repo.head_id().map_err(git)?.detach() != head_start
        || index_digest(repo) != index_digest_start
        || !matches!(
            project.cleanliness(),
            Ok(crate::snapshot::Cleanliness::Clean)
        )
    {
        return Err(MergeError::Changed);
    }

    let (mut pipeline, index) = repo.filter_pipeline(None).map_err(git)?;
    let state: &gix::index::State = &index;

    // Step 7: write, tracking each path for rollback.
    let mut done: Vec<Restorable> = Vec::new();
    let result = write_all(
        repo,
        &mut pipeline,
        state,
        &workdir,
        ours,
        merged,
        target,
        keep_operator,
        &mut done,
    );
    if let Err(err) = result {
        rollback(repo, &mut pipeline, state, &workdir, &done)?;
        return Err(err);
    }

    // The index is written last, through the held lock, so a write failure above
    // leaves the index untouched and staged work outside the target intact.
    if let Err(err) = write_index(
        project,
        outcome,
        merged_full,
        target,
        driver_stages,
        how,
        lock,
    ) {
        rollback(repo, &mut pipeline, state, &workdir, &done)?;
        return Err(err);
    }
    Ok(())
}

/// Write every merged target file that differs from the operator and delete the
/// files the template removed, each only after checking the path still holds its
/// HEAD content (or is absent).
#[allow(clippy::too_many_arguments)]
fn write_all(
    repo: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline<'_>,
    state: &gix::index::State,
    workdir: &Path,
    ours: &Entries,
    merged: &Entries,
    target: &RepoPath,
    keep_operator: &std::collections::BTreeSet<String>,
    done: &mut Vec<Restorable>,
) -> Result<(), MergeError> {
    // Writes and modifications.
    for (path, (id, mode)) in merged {
        let expected = ours.get(path);
        if expected == Some(&(*id, *mode)) {
            continue; // unchanged for the operator
        }
        let repo_rel = repo_path_for(target, path).as_str().to_owned();
        if keep_operator.contains(&repo_rel) || ancestor_kept(&repo_rel, keep_operator) {
            // A non-content conflict, or a path under a file/directory conflict
            // whose ancestor stays the operator's file, keeps the operator's disk.
            continue;
        }
        drift_check(pipeline, state, &repo_rel, expected.map(|(o, _)| *o))?;
        let bytes = worktree_bytes(repo, pipeline, *id, &repo_rel)?;
        write_atomically(workdir, &repo_rel, &bytes, *mode)?;
        done.push(Restorable {
            repo_rel,
            head: expected.copied(),
            wrote: Some(*id),
        });
    }
    // Deletions the template made.
    for (path, (id, _)) in ours {
        if merged.contains_key(path) {
            continue;
        }
        let repo_rel = repo_path_for(target, path).as_str().to_owned();
        if keep_operator.contains(&repo_rel) {
            continue; // a modify/delete conflict keeps the operator's file on disk
        }
        drift_check(pipeline, state, &repo_rel, Some(*id))?;
        let full = workdir.join(&repo_rel);
        if full.exists() {
            std::fs::remove_file(&full).map_err(|e| MergeError::Io(e.to_string()))?;
        }
        done.push(Restorable {
            repo_rel,
            head: ours.get(path).copied(),
            wrote: None,
        });
    }
    Ok(())
}

/// Refuse when the path on disk no longer holds its HEAD content (or is not
/// absent for an add), narrowing the window in which another program writes it.
fn drift_check(
    pipeline: &mut gix::filter::Pipeline<'_>,
    state: &gix::index::State,
    repo_rel: &str,
    expected: Option<gix::ObjectId>,
) -> Result<(), MergeError> {
    let current = pipeline
        .worktree_file_to_object(gix::bstr::BStr::new(repo_rel.as_bytes()), state)
        .map_err(git)?
        .map(|(oid, _, _)| oid);
    if current != expected {
        return Err(MergeError::Changed);
    }
    Ok(())
}

/// The worktree (smudged) bytes for a git blob, applying built-in conversions
/// only.
fn worktree_bytes(
    repo: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline<'_>,
    id: gix::ObjectId,
    repo_rel: &str,
) -> Result<Vec<u8>, MergeError> {
    let data = repo.find_object(id).map_err(git)?.data.clone();
    let outcome = pipeline
        .convert_to_worktree(
            &data,
            gix::bstr::BStr::new(repo_rel.as_bytes()),
            Default::default(),
        )
        .map_err(git)?;
    Ok(match outcome {
        ToWorktreeOutcome::Unchanged(bytes) => bytes.to_vec(),
        ToWorktreeOutcome::Buffer(bytes) => bytes.to_vec(),
        ToWorktreeOutcome::Process(_) => {
            return Err(MergeError::Git(
                "unexpected process filter after driver strip".into(),
            ));
        }
    })
}

/// Write `bytes` to `workdir/repo_rel` by a temporary file and rename, setting
/// the executable bit from `mode`.
fn write_atomically(
    workdir: &Path,
    repo_rel: &str,
    bytes: &[u8],
    mode: gix::index::entry::Mode,
) -> Result<(), MergeError> {
    let full = workdir.join(repo_rel);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MergeError::Io(e.to_string()))?;
    }
    let tmp = full.with_extension("toha-tmp");
    std::fs::write(&tmp, bytes).map_err(|e| MergeError::Io(e.to_string()))?;
    #[cfg(unix)]
    if mode == gix::index::entry::Mode::FILE_EXECUTABLE {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| MergeError::Io(e.to_string()))?;
    }
    std::fs::rename(&tmp, &full).map_err(|e| MergeError::Io(e.to_string()))?;
    Ok(())
}

/// Restore every path the transaction wrote or deleted, only when it still holds
/// what Toha wrote; a path changed by another program is left alone and named.
fn rollback(
    repo: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline<'_>,
    state: &gix::index::State,
    workdir: &Path,
    done: &[Restorable],
) -> Result<(), MergeError> {
    let mut stuck = Vec::new();
    for entry in done.iter().rev() {
        // Only touch a path that still holds Toha's bytes.
        let current = pipeline
            .worktree_file_to_object(gix::bstr::BStr::new(entry.repo_rel.as_bytes()), state)
            .map_err(git)?
            .map(|(oid, _, _)| oid);
        if current != entry.wrote {
            stuck.push(entry.repo_rel.clone());
            continue;
        }
        match entry.head {
            Some((oid, mode)) => {
                // Restore the HEAD content (a modify or a delete).
                let bytes = worktree_bytes(repo, pipeline, oid, &entry.repo_rel)?;
                write_atomically(workdir, &entry.repo_rel, &bytes, mode)?;
            }
            None => {
                // An add: remove the created file.
                let full = workdir.join(&entry.repo_rel);
                if full.exists() {
                    std::fs::remove_file(&full).map_err(|e| MergeError::Io(e.to_string()))?;
                }
            }
        }
    }
    if stuck.is_empty() {
        Ok(())
    } else {
        Err(MergeError::Io(format!(
            "rollback left paths changed by another program: {stuck:?}; recover with `git restore --source=HEAD --staged --worktree -- <target>` and `git clean -d --force -- <target>`"
        )))
    }
}

/// Build the merged index with conflict stages and write it through the held
/// lock, committing atomically.
#[allow(clippy::too_many_arguments)]
fn write_index(
    project: &Project,
    outcome: &mut gix::merge::tree::Outcome<'_>,
    merged_full: gix::ObjectId,
    target: &RepoPath,
    driver_stages: &DriverStages,
    how: gix::merge::tree::TreatAsUnresolved,
    mut lock: gix::lock::File,
) -> Result<(), MergeError> {
    let repo = project.repo();
    let mut index = repo.open_index().map_err(git)?;
    let prefix = if target.is_root() {
        String::new()
    } else {
        format!("{}/", target.as_str())
    };
    if prefix.is_empty() {
        index.remove_entries(|_, _, _| true);
    } else {
        index.remove_entries(|_, path, _| path.starts_with(prefix.as_bytes()));
    }
    let merged_sub = subtree(project, merged_full, target)?;
    let merged_entries = flat_entries(project, merged_sub)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| MergeError::Io("bare".into()))?
        .to_owned();
    for (path, (id, mode)) in &merged_entries {
        let repo_path = repo_path_for(target, path);
        let full: BString = repo_path.as_str().into();
        let stat = gix::index::fs::Metadata::from_path_no_follow(&workdir.join(repo_path.as_str()))
            .ok()
            .and_then(|md| gix::index::entry::Stat::from_fs(&md).ok())
            .unwrap_or_default();
        index.dangerously_push_entry(
            stat,
            *id,
            gix::index::entry::Flags::empty(),
            *mode,
            full.as_bstr(),
        );
    }
    index.sort_entries();
    index.remove_tree();
    index.remove_resolve_undo();
    outcome.index_changed_after_applying_conflicts(
        &mut index,
        how,
        gix::merge::tree::apply_index_entries::RemovalMode::Prune,
    );

    // Stage driver-attributed paths manually as unmerged (1/2/3), since gitoxide
    // merged them with its built-in driver after the driver was stripped.
    for (repo_rel, stages) in driver_stages {
        let full: BString = repo_rel.as_str().into();
        index.remove_entries(|_, path, _| path == full.as_slice());
        for (i, entry) in stages.iter().enumerate() {
            if let Some((oid, mode)) = entry {
                let stage = match i {
                    0 => gix::index::entry::Stage::Base,
                    1 => gix::index::entry::Stage::Ours,
                    _ => gix::index::entry::Stage::Theirs,
                };
                index.dangerously_push_entry(
                    Default::default(),
                    *oid,
                    gix::index::entry::Flags::from_stage(stage),
                    *mode,
                    full.as_bstr(),
                );
            }
        }
    }
    index.sort_entries();

    lock.with_mut(|file| {
        index
            .write_to(file, Default::default())
            .map(|_| ())
            .map_err(std::io::Error::other)
    })
    .map_err(|e| MergeError::Io(e.to_string()))?;
    lock.commit().map_err(|e| MergeError::Io(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tree helpers
// ---------------------------------------------------------------------------

type Entries = std::collections::BTreeMap<BString, (gix::ObjectId, gix::index::entry::Mode)>;

/// For each driver-attributed path (repository-relative), its `[base, ours,
/// theirs]` stage entries to write into the index as an unmerged conflict.
type DriverStages =
    std::collections::BTreeMap<String, [Option<(gix::ObjectId, gix::index::entry::Mode)>; 3]>;

/// The target-relative paths the template changed whose attributes name a merge
/// or filter driver, which Toha never runs.
fn driver_attributed_paths(
    repo: &gix::Repository,
    target: &RepoPath,
    base_entries: &Entries,
    theirs_entries: &Entries,
) -> Result<Vec<BString>, MergeError> {
    let index = repo.index_or_empty().map_err(git)?;
    let mut stack = repo
        .attributes_only(
            &index,
            gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
        )
        .map_err(git)?;
    let mut out = gix::attrs::search::Outcome::default();
    out.initialize_with_selection(&Default::default(), ["merge", "filter"]);

    let mut paths = Vec::new();
    for (path, theirs) in theirs_entries {
        if base_entries.get(path) == Some(theirs) {
            continue; // the template did not change this path
        }
        let repo_rel = repo_path_for(target, path);
        out.reset();
        let platform = stack
            .at_entry(repo_rel.as_str(), Some(gix::index::entry::Mode::FILE))
            .map_err(|e| MergeError::Io(e.to_string()))?;
        platform.matching_attributes(&mut out);
        if is_driver_attributed(&out) {
            paths.push(path.clone());
        }
    }
    Ok(paths)
}

/// Whether the resolved `merge`/`filter` attributes name a driver program.
fn is_driver_attributed(out: &gix::attrs::search::Outcome) -> bool {
    use gix::attrs::StateRef;
    let mut selected = out.iter_selected();
    let merge = selected.next();
    let filter = selected.next();
    let merge_driver = merge.is_some_and(|m| match m.assignment.state {
        StateRef::Value(v) => !matches!(
            v.as_bstr().to_str_lossy().as_ref(),
            "text" | "binary" | "union"
        ),
        _ => false,
    });
    let filter_driver =
        filter.is_some_and(|m| matches!(m.assignment.state, StateRef::Value(_) | StateRef::Set));
    merge_driver || filter_driver
}

/// The `files/` tree oid of a snapshot.
fn snapshot_files_tree(project: &Project, id: &SnapshotId) -> Result<gix::ObjectId, MergeError> {
    let repo = project.repo();
    let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
    let mut reference = repo.find_reference(name.as_str()).map_err(git)?;
    let commit = reference.peel_to_commit().map_err(git)?;
    let tree = commit.tree().map_err(git)?;
    match tree.lookup_entry_by_path("files").map_err(git)? {
        Some(entry) if entry.mode().is_tree() => Ok(entry.oid().to_owned()),
        _ => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// Graft `sub` into `base` at the target path (or replace the whole tree at the
/// root).
fn graft(
    project: &Project,
    base: gix::ObjectId,
    target: &RepoPath,
    sub: gix::ObjectId,
) -> Result<gix::ObjectId, MergeError> {
    let repo = project.repo();
    if target.is_root() {
        return Ok(sub);
    }
    let mut editor = repo.edit_tree(base).map_err(git)?;
    editor
        .upsert(target.as_str(), EntryKind::Tree, sub)
        .map_err(git)?;
    Ok(editor.write().map_err(git)?.detach())
}

/// The subtree oid at the target path of a whole-repository tree.
fn subtree(
    project: &Project,
    tree: gix::ObjectId,
    target: &RepoPath,
) -> Result<gix::ObjectId, MergeError> {
    if target.is_root() {
        return Ok(tree);
    }
    let repo = project.repo();
    let t = repo.find_tree(tree).map_err(git)?;
    Ok(
        match t.lookup_entry_by_path(target.as_str()).map_err(git)? {
            Some(entry) if entry.mode().is_tree() => entry.oid().to_owned(),
            _ => gix::ObjectId::empty_tree(repo.object_hash()),
        },
    )
}

/// The flat leaf entries of a tree, keyed by path relative to that tree.
fn flat_entries(project: &Project, tree: gix::ObjectId) -> Result<Entries, MergeError> {
    let repo = project.repo();
    let state =
        gix::index::State::from_tree(&tree, &repo.objects, Default::default()).map_err(git)?;
    Ok(state
        .entries()
        .iter()
        .map(|e| (e.path(&state).to_owned(), (e.id, e.mode)))
        .collect())
}

fn repo_path_for(target: &RepoPath, sub: &BString) -> RepoPath {
    let sub = sub.to_str_lossy();
    let joined = if target.is_root() {
        sub.into_owned()
    } else {
        format!("{}/{}", target.as_str(), sub)
    };
    RepoPath::parse(&joined).unwrap_or_else(|_| RepoPath::root())
}

/// Classify one unresolved conflict into the kind Toha reports. Stages are
/// `[base, ours, theirs]`.
fn classify_conflict(
    repo: &gix::Repository,
    conflict: &gix::merge::tree::Conflict,
) -> ConflictKind {
    use gix::merge::tree::ResolutionFailure;
    let entries = conflict.entries();
    let is_dir = |i: usize| entries[i].as_ref().is_some_and(|e| e.mode.is_tree());
    let present = |i: usize| entries[i].is_some();

    if is_dir(0)
        || is_dir(1)
        || is_dir(2)
        || matches!(
            conflict.resolution,
            Err(ResolutionFailure::OursDirectoryTheirsNonDirectoryTheirsRenamed { .. })
                | Err(ResolutionFailure::OursModifiedTheirsDirectoryThenOursRenamed { .. })
        )
    {
        return ConflictKind::FileDirectory;
    }
    let (bp, op, tp) = (present(0), present(1), present(2));
    if !bp && op && tp {
        return ConflictKind::AddAdd;
    }
    if bp && (op ^ tp) {
        return ConflictKind::ModifyDelete;
    }
    // A content conflict: binary when either changed side is binary.
    if op && tp {
        let binary = [1usize, 2]
            .iter()
            .any(|&i| entries[i].as_ref().is_some_and(|e| is_binary(repo, e.id)));
        if binary {
            return ConflictKind::Binary;
        }
    }
    ConflictKind::Content
}

/// Whether any ancestor directory of `repo_rel` is a kept file/directory
/// conflict path (the operator's file stays, so the template's colliding
/// directory content beneath it is not written).
fn ancestor_kept(repo_rel: &str, keep_operator: &std::collections::BTreeSet<String>) -> bool {
    let mut prefix = repo_rel;
    while let Some((parent, _)) = prefix.rsplit_once('/') {
        if keep_operator.contains(parent) {
            return true;
        }
        prefix = parent;
    }
    false
}

/// Whether a blob is binary, by git's heuristic: a NUL byte in the first 8000
/// bytes.
fn is_binary(repo: &gix::Repository, id: gix::ObjectId) -> bool {
    match repo.find_object(id) {
        Ok(object) => object.data.iter().take(8000).any(|&b| b == 0),
        Err(_) => false,
    }
}

/// Remove this invocation's candidate ref, only if it still points at the commit
/// the capture published. A ref that another program replaced or already removed
/// is preserved; a genuine deletion failure is reported.
fn remove_candidate(
    project: &Project,
    id: &SnapshotId,
    published: gix::ObjectId,
) -> Result<(), MergeError> {
    let repo = project.repo();
    let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
    let mut reference = match repo.find_reference(name.as_str()) {
        Ok(reference) => reference,
        Err(_) => return Ok(()), // already gone
    };
    let current = reference.peel_to_commit().map_err(git)?.id().detach();
    if current != published {
        return Ok(()); // replaced by another program; preserve it
    }
    reference.delete().map_err(git)
}

#[cfg(test)]
mod tests;
