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
use std::sync::atomic::AtomicBool;

use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::EntryKind;

use crate::snapshot::project::Project;
use crate::snapshot::record::{RepoPath, SNAPSHOT_REF_PREFIX, Snapshot, SnapshotId};

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

/// Merge the new snapshot into the operator's working tree against `base`. The
/// new snapshot must already be captured (its ref exists); on any unsuccessful
/// exit its candidate ref is removed.
pub(crate) fn merge_into_worktree(
    project: &Project,
    base: &Base,
    new: &Snapshot,
    options: &MergeOptions,
) -> Result<Merged, MergeError> {
    let result = merge_inner(project, base, new, options);
    if !options.dry_run && !matches!(result, Ok(Merged::Written { .. })) {
        // Any unsuccessful post-capture exit removes this candidate ref.
        remove_candidate(project, new.id());
    }
    result
}

fn merge_inner(
    project: &Project,
    base: &Base,
    new: &Snapshot,
    options: &MergeOptions,
) -> Result<Merged, MergeError> {
    let repo = project.repo();
    let target = project.target();

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

    let conflicted: Vec<RepoPath> = outcome
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(how))
        .filter_map(|c| RepoPath::parse(&c.ours.location().to_str_lossy()).ok())
        .collect();

    let mut changes = Vec::new();
    for (path, (id, mode)) in &merged_entries {
        let repo_path = repo_path_for(target, path);
        let action = if conflicted.iter().any(|p| p.as_str() == repo_path.as_str()) {
            Action::Conflicted(ConflictKind::Content)
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
            changes.push(Change {
                path: repo_path_for(target, path),
                action: Action::Deleted,
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

    // Write the merged target files that differ from the operator, and delete
    // those the template removed, each only after checking the path still holds
    // its HEAD content (or is absent).
    write_result(project, &ours_entries, &merged_entries, target)?;

    // Apply the merged index with conflict stages.
    apply_index(project, &mut outcome, merged_full, target, how)?;

    Ok(Merged::Written {
        snapshot: *new.id(),
        changes,
        conflicted,
    })
}

/// Write adds and modifications through gix checkout (stored form, exec bit) and
/// delete files the template removed.
fn write_result(
    project: &Project,
    ours: &Entries,
    merged: &Entries,
    target: &RepoPath,
) -> Result<(), MergeError> {
    let repo = project.repo();
    let workdir = repo
        .workdir()
        .ok_or_else(|| MergeError::Io("bare".into()))?
        .to_owned();

    // Deletions first.
    for path in ours.keys() {
        if !merged.contains_key(path) {
            let repo_path = repo_path_for(target, path);
            let full = workdir.join(repo_path.as_str());
            if full.exists() {
                std::fs::remove_file(&full).map_err(|e| MergeError::Io(e.to_string()))?;
            }
        }
    }

    // Writes via a tiny in-memory index and gix checkout.
    let mut write_state = gix::index::State::new(repo.object_hash());
    let mut any = false;
    for (path, (id, mode)) in merged {
        if ours.get(path) != Some(&(*id, *mode)) {
            let repo_path = repo_path_for(target, path);
            let full: BString = repo_path.as_str().into();
            write_state.dangerously_push_entry(
                Default::default(),
                *id,
                gix::index::entry::Flags::empty(),
                *mode,
                full.as_bstr(),
            );
            any = true;
        }
    }
    if !any {
        return Ok(());
    }
    write_state.sort_entries();
    let mut copts = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping)
        .map_err(git)?;
    copts.overwrite_existing = true;
    copts.destination_is_initially_empty = false;
    let out = gix::worktree::state::checkout(
        &mut write_state,
        &workdir,
        repo.objects.clone().into_arc().map_err(git)?,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &AtomicBool::new(false),
        copts,
    )
    .map_err(git)?;
    if !out.errors.is_empty() || !out.collisions.is_empty() {
        return Err(MergeError::Io(format!(
            "checkout errors {:?} collisions {:?}",
            out.errors, out.collisions
        )));
    }
    Ok(())
}

/// Replace the target's index entries with the merged tree's, keeping everything
/// outside the target, dropping the cached tree, and adding conflict stages.
fn apply_index(
    project: &Project,
    outcome: &mut gix::merge::tree::Outcome<'_>,
    merged_full: gix::ObjectId,
    target: &RepoPath,
    how: gix::merge::tree::TreatAsUnresolved,
) -> Result<(), MergeError> {
    let repo = project.repo();
    let mut index = repo.open_index().map_err(git)?;
    let prefix = if target.is_root() {
        String::new()
    } else {
        format!("{}/", target.as_str())
    };
    if !prefix.is_empty() {
        index.remove_entries(|_, path, _| path.starts_with(prefix.as_bytes()));
    } else {
        index.remove_entries(|_, _, _| true);
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
    index.write(Default::default()).map_err(git)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tree helpers
// ---------------------------------------------------------------------------

type Entries = std::collections::BTreeMap<BString, (gix::ObjectId, gix::index::entry::Mode)>;

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

/// Remove this invocation's candidate ref, only if it still points at the commit
/// the capture published.
fn remove_candidate(project: &Project, id: &SnapshotId) {
    let repo = project.repo();
    let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
    if let Ok(reference) = repo.find_reference(name.as_str()) {
        let _ = reference.delete();
    }
}

#[cfg(test)]
mod tests;
