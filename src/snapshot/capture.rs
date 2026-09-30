// ---
// relationships:
//   implements: architecture
// ---
//! Capturing a snapshot from an applied target.
//!
//! After a plain `apply` (or a `--baseline` adoption) has written the target,
//! capture records what Toha produced: every plan target — a whole file (origin
//! `toha`) or a file Toha edits inside (origin `edit`), captured even when its
//! bytes did not change — and every path a hook created or changed that differs
//! from `HEAD` (origin `hook`). It reads content in its stored form through
//! gitoxide's built-in conversions only, never a filter driver, records symbolic
//! links as links and the executable bit, and leaves out an untracked file git
//! ignores.
//!
//! Carry-forward, retraction, and release apply only with a base snapshot
//! (`--from`); this base-less path is what a plain apply and a baseline use.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use indexmap::IndexMap;

use crate::plan::{FileMutation, Plan, TargetPath};
use crate::snapshot::record::{
    self, CapturedBlob, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision,
    SnapshotDoc, SnapshotError, SnapshotId, Timestamp,
};

/// What a capture records about the applied project besides its files.
pub struct CaptureInputs {
    pub id: SnapshotId,
    pub template: String,
    pub revision: Revision,
    pub generated: FrozenNow,
    pub created: Timestamp,
    pub project: ProjectPoint,
    pub submissions: Vec<IndexMap<String, serde_json::Value>>,
}

/// The ownership a plan asserts over one path.
#[derive(Default)]
struct PlanOwnership {
    whole: bool,
    regions: Vec<String>,
    values: Vec<String>,
}

/// Capture a snapshot from a plain apply or a baseline: no base snapshot, so no
/// carry-forward, retraction, or release. Returns the saved id.
pub(crate) fn capture_baseless(
    repo: &gix::Repository,
    target: &RepoPath,
    plan: &Plan,
    inputs: CaptureInputs,
) -> Result<SnapshotId, SnapshotError> {
    let plan_paths = group_plan(plan);

    let (mut pipeline, index) = repo
        .filter_pipeline(None)
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    let state: &gix::index::State = &index;
    let head = head_target_oids(repo, target)?;

    let mut blobs: Vec<CapturedBlob> = Vec::new();
    let mut paths: Vec<PathOwnership> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // 1. Every plan target, in plan order, captured whether or not its bytes changed.
    for (rel, ownership) in &plan_paths {
        let repo_rel = repo_relative(target, rel);
        let Some((oid, kind, _md)) = pipeline
            .worktree_file_to_object(gix::bstr::BStr::new(repo_rel.as_bytes()), state)
            .map_err(|err| SnapshotError::Git(err.to_string()))?
        else {
            // A plan target the apply did not leave on disk is skipped.
            continue;
        };
        let origin = if ownership.whole {
            Origin::Toha
        } else {
            Origin::Edit {
                regions: ownership.regions.clone(),
                values: ownership.values.clone(),
            }
        };
        let path = TargetPath::parse(rel).map_err(|message| SnapshotError::Path {
            path: rel.clone(),
            message,
        })?;
        blobs.push(CapturedBlob {
            path: path.clone(),
            kind,
            oid,
        });
        paths.push(PathOwnership::new(path, origin));
        seen.insert(rel.clone());
    }

    // 2. Hook changes: files under the target that differ from HEAD and are not
    //    plan targets, excluding an untracked file git ignores.
    let workdir = repo
        .workdir()
        .ok_or_else(|| SnapshotError::Git("bare repository".into()))?;
    let target_dir = if target.is_root() {
        workdir.to_owned()
    } else {
        workdir.join(target.as_str())
    };
    let mut on_disk = Vec::new();
    walk_files(&target_dir, Path::new(""), &mut on_disk)
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    on_disk.sort();

    for rel in on_disk {
        if seen.contains(&rel) {
            continue;
        }
        let repo_rel = repo_relative(target, &rel);
        let Some((oid, kind, _md)) = pipeline
            .worktree_file_to_object(gix::bstr::BStr::new(repo_rel.as_bytes()), state)
            .map_err(|err| SnapshotError::Git(err.to_string()))?
        else {
            continue;
        };
        match head.get(&rel) {
            Some(head_oid) if *head_oid == oid => continue, // unchanged tracked file
            Some(_) => {}                                   // changed tracked file -> hook
            None => {
                // New file: skip when git ignores it.
                if is_ignored(repo, state, &repo_rel)? {
                    continue;
                }
            }
        }
        let path = TargetPath::parse(&rel).map_err(|message| SnapshotError::Path {
            path: rel.clone(),
            message,
        })?;
        blobs.push(CapturedBlob {
            path: path.clone(),
            kind,
            oid,
        });
        paths.push(PathOwnership::new(path, Origin::Hook));
        seen.insert(rel);
    }

    let doc = SnapshotDoc::new(
        inputs.id,
        inputs.template,
        inputs.revision,
        target.clone(),
        inputs.created,
        inputs.generated,
        inputs.project,
        None,
        inputs.submissions,
        paths,
    );
    record::save(repo, &doc, &blobs)
}

/// Group a plan's mutations by target path, preserving first-seen order.
fn group_plan(plan: &Plan) -> Vec<(String, PlanOwnership)> {
    let mut order: Vec<String> = Vec::new();
    let mut map: BTreeMap<String, PlanOwnership> = BTreeMap::new();
    for mutation in plan.mutations() {
        let path = match mutation {
            FileMutation::Whole { path }
            | FileMutation::Region { path, .. }
            | FileMutation::JsonValue { path, .. } => path.to_string(),
        };
        if !map.contains_key(&path) {
            order.push(path.clone());
        }
        let ownership = map.entry(path).or_default();
        match mutation {
            FileMutation::Whole { .. } => ownership.whole = true,
            FileMutation::Region { region, .. } => ownership.regions.push(region.to_string()),
            FileMutation::JsonValue { json_path, .. } => {
                ownership.values.push(json_path.to_string())
            }
        }
    }
    order
        .into_iter()
        .map(|path| {
            let ownership = map.remove(&path).expect("path was ordered");
            (path, ownership)
        })
        .collect()
}

/// The `HEAD` tree's blob ids under the target, keyed by target-relative path.
fn head_target_oids(
    repo: &gix::Repository,
    target: &RepoPath,
) -> Result<BTreeMap<String, gix::ObjectId>, SnapshotError> {
    let head_tree = match repo.head_tree() {
        Ok(tree) => tree,
        Err(_) => return Ok(BTreeMap::new()), // unborn HEAD
    };
    let subtree = if target.is_root() {
        head_tree.id().detach()
    } else {
        match head_tree
            .lookup_entry_by_path(target.as_str())
            .map_err(|err| SnapshotError::Git(err.to_string()))?
        {
            Some(entry) if entry.mode().is_tree() => entry.oid().to_owned(),
            _ => return Ok(BTreeMap::new()),
        }
    };
    let mut oids = BTreeMap::new();
    collect_tree(repo, subtree, "", &mut oids)?;
    Ok(oids)
}

fn collect_tree(
    repo: &gix::Repository,
    tree_oid: gix::ObjectId,
    prefix: &str,
    oids: &mut BTreeMap<String, gix::ObjectId>,
) -> Result<(), SnapshotError> {
    use gix::objs::tree::EntryKind;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    for entry in tree.iter() {
        let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
        let name = entry.filename().to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match entry.mode().kind() {
            EntryKind::Tree => collect_tree(repo, entry.oid().to_owned(), &path, oids)?,
            EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                oids.insert(path, entry.oid().to_owned());
            }
            EntryKind::Commit => {}
        }
    }
    Ok(())
}

/// Whether git ignores a repository-relative path (untracked-file rule).
fn is_ignored(
    repo: &gix::Repository,
    state: &gix::index::State,
    repo_rel: &str,
) -> Result<bool, SnapshotError> {
    let mut excludes = repo
        .excludes(
            state,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    let platform = excludes
        .at_path(Path::new(repo_rel), Some(gix::index::entry::Mode::FILE))
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    Ok(platform.is_excluded())
}

/// A repository-relative path for a target-relative one.
fn repo_relative(target: &RepoPath, rel: &str) -> String {
    if target.is_root() {
        rel.to_owned()
    } else {
        format!("{}/{}", target.as_str(), rel)
    }
}

/// Collect target-relative paths of files (not directories) under `root`,
/// skipping a nested `.git`.
fn walk_files(root: &Path, rel: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let dir = root.join(rel);
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let child = rel.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_files(root, &child, out)?;
        } else {
            out.push(child.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
