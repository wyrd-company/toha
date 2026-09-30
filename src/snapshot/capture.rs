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
//! With a base snapshot (`--from`) it also carries hook files forward, retracts
//! an injection the new version dropped, and releases an edit that owns nothing;
//! without one (a plain apply or a baseline) only the plan targets and hook
//! changes are captured.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use indexmap::IndexMap;

use crate::plan::{FileMutation, Plan, TargetPath};
use crate::snapshot::record::{
    self, CapturedBlob, FrozenNow, Origin, PathOwnership, ProjectPoint, RepoPath, Revision,
    Snapshot, SnapshotDoc, SnapshotError, SnapshotId, Timestamp,
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
pub(crate) struct PlanOwnership {
    pub(crate) whole: bool,
    pub(crate) regions: Vec<String>,
    pub(crate) values: Vec<String>,
}

/// A plan grouped by target path, preserving first-seen order. Computed once by
/// the candidate-tree builder before the plan is consumed by apply.
pub(crate) type PlanPaths = Vec<(String, PlanOwnership)>;

/// Capture a snapshot from an applied target read at `source_dir` (the project
/// working tree for a plain apply, the throwaway checkout for `--from`). With a
/// `base` snapshot it also carries hook files forward, retracts an injection the
/// new version dropped, releases an edit that owns nothing, and lets the merge
/// see a removed whole-file. Returns the saved id.
/// Capture the snapshot and save it under its ref. Used by an update, which
/// always saves (already-current is decided earlier, by the merge).
pub(crate) fn capture(
    repo: &gix::Repository,
    source_dir: &Path,
    target: &RepoPath,
    plan_paths: &PlanPaths,
    base: Option<&Snapshot>,
    inputs: CaptureInputs,
) -> Result<SnapshotId, SnapshotError> {
    let (doc, blobs, _changed) = collect(repo, source_dir, target, plan_paths, base, inputs)?;
    record::save(repo, &doc, &blobs)
}

/// Capture the snapshot of a plain apply, saving it only when at least one
/// captured path differs from `HEAD` in content or entry kind. Returns `None`
/// when nothing the apply owns changed, so the apply saves no snapshot. The test
/// is path-scoped to the captured set, so an unrelated working-tree edit never
/// creates a snapshot and an ignored output never suppresses one.
pub(crate) fn capture_if_changed(
    repo: &gix::Repository,
    source_dir: &Path,
    target: &RepoPath,
    plan_paths: &PlanPaths,
    inputs: CaptureInputs,
) -> Result<Option<SnapshotId>, SnapshotError> {
    let (doc, blobs, changed) = collect(repo, source_dir, target, plan_paths, None, inputs)?;
    if !changed {
        return Ok(None);
    }
    Ok(Some(record::save(repo, &doc, &blobs)?))
}

/// Build the snapshot document and its captured blobs, and report whether any
/// captured path differs from `HEAD` in content or entry kind.
fn collect(
    repo: &gix::Repository,
    source_dir: &Path,
    target: &RepoPath,
    plan_paths: &PlanPaths,
    base: Option<&Snapshot>,
    inputs: CaptureInputs,
) -> Result<(SnapshotDoc, Vec<CapturedBlob>, bool), SnapshotError> {
    let plan_index: BTreeMap<&str, &PlanOwnership> =
        plan_paths.iter().map(|(p, o)| (p.as_str(), o)).collect();

    let (mut pipeline, index) = repo
        .filter_pipeline(None)
        .map_err(|err| SnapshotError::Git(err.to_string()))?;
    let state: &gix::index::State = &index;
    let head = head_target_oids(repo, target)?;

    let mut blobs: Vec<CapturedBlob> = Vec::new();
    let mut paths: Vec<PathOwnership> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // Retraction of uncovered base ownership is applied to the source tree by the
    // candidate-tree builder before the plan is applied (see `retract_uncovered`),
    // so by the time capture reads the tree the dropped injections are already
    // gone. A plain apply and a baseline have no base and no retraction.

    // 2. Every plan target, in plan order, captured whether or not its bytes changed.
    for (rel, ownership) in plan_paths {
        let repo_rel = repo_relative(target, rel);
        let Some((oid, kind)) = read_blob(repo, &mut pipeline, source_dir, &repo_rel, state)?
        else {
            continue; // a plan target the apply did not leave on disk
        };
        let origin = if ownership.whole {
            Origin::Toha
        } else {
            Origin::Edit {
                regions: ownership.regions.clone(),
                values: ownership.values.clone(),
            }
        };
        push(&mut blobs, &mut paths, &mut seen, rel, kind, oid, origin)?;
    }

    // 3. Base-aware capture of paths the new plan does not target.
    if let Some(base) = base {
        for owned in base.paths() {
            let rel = owned.path().to_string();
            if seen.contains(&rel) || plan_index.contains_key(rel.as_str()) {
                continue;
            }
            let repo_rel = repo_relative(target, &rel);
            let on_disk = read_blob(repo, &mut pipeline, source_dir, &repo_rel, state)?;
            match owned.origin() {
                // A whole-file the new plan no longer produces: not captured, so
                // the merge sees the template remove it.
                Origin::Toha => {}
                // A hook file the new version did not change is carried forward
                // from the base; a changed one is captured below as a hook change.
                Origin::Hook => {
                    let changed = on_disk
                        .as_ref()
                        .map(|(oid, _)| head.get(&rel).map(|(head_oid, _)| head_oid) != Some(oid))
                        .unwrap_or(false);
                    if !changed {
                        if let Some((oid, kind)) = base_file_blob(repo, base.id(), &rel)? {
                            push(
                                &mut blobs,
                                &mut paths,
                                &mut seen,
                                &rel,
                                kind,
                                oid,
                                Origin::Hook,
                            )?;
                        }
                    }
                }
                // An edit path: released when it owns nothing, otherwise captured
                // with its (now retracted) content and the ownership that remains.
                Origin::Edit { regions, values } => {
                    if regions.is_empty() && values.is_empty() {
                        continue; // release: not captured, merge leaves it with the operator
                    }
                    // A non-plan-target edit path keeps no ownership after
                    // retraction; its content is preserved for the operator.
                    if let Some((oid, kind)) = on_disk {
                        let origin = Origin::Edit {
                            regions: Vec::new(),
                            values: Vec::new(),
                        };
                        push(&mut blobs, &mut paths, &mut seen, &rel, kind, oid, origin)?;
                    }
                }
            }
        }
    }

    // 4. Hook changes: files under the target that differ from HEAD and are not
    //    plan targets, excluding an untracked file git ignores.
    let target_dir = if target.is_root() {
        source_dir.to_owned()
    } else {
        source_dir.join(target.as_str())
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
        let Some((oid, kind)) = read_blob(repo, &mut pipeline, source_dir, &repo_rel, state)?
        else {
            continue;
        };
        match head.get(&rel) {
            Some((head_oid, _)) if *head_oid == oid => continue, // unchanged tracked file
            Some(_) => {}                                        // changed tracked file -> hook
            None => {
                if is_ignored(repo, state, &repo_rel)? {
                    continue;
                }
            }
        }
        push(
            &mut blobs,
            &mut paths,
            &mut seen,
            &rel,
            kind,
            oid,
            Origin::Hook,
        )?;
    }

    // A captured path changed when its content (oid) or entry kind (exec bit,
    // file vs link) differs from HEAD, or it is not in HEAD at all. Scoped to the
    // captured set, so an unrelated edit or an ignored output never counts.
    let changed = blobs
        .iter()
        .any(|blob| head.get(&blob.path.to_string()) != Some(&(blob.oid, blob.kind)));

    let doc = SnapshotDoc::new(
        inputs.id,
        inputs.template,
        inputs.revision,
        target.clone(),
        inputs.created,
        inputs.generated,
        inputs.project,
        base.map(|b| *b.id()),
        inputs.submissions,
        paths,
    );
    Ok((doc, blobs, changed))
}

/// Retract, in the source tree, every base injection the new plan does not
/// cover, so the captured content — and the merge's "theirs" — drops it. Applied
/// by the candidate-tree builder before the destination-seam plan apply.
pub(crate) fn retract_uncovered(
    source_dir: &Path,
    target: &RepoPath,
    plan_paths: &PlanPaths,
    base: &Snapshot,
) -> Result<(), SnapshotError> {
    let plan_index: BTreeMap<&str, &PlanOwnership> =
        plan_paths.iter().map(|(p, o)| (p.as_str(), o)).collect();
    for owned in base.paths() {
        let rel = owned.path().to_string();
        if let Origin::Edit { regions, values } = owned.origin() {
            let uncovered = uncovered_ownership(regions, values, plan_index.get(rel.as_str()));
            apply_retraction(source_dir, target, &rel, &uncovered)?;
        }
    }
    Ok(())
}

/// Record one captured file and its ownership.
fn push(
    blobs: &mut Vec<CapturedBlob>,
    paths: &mut Vec<PathOwnership>,
    seen: &mut BTreeSet<String>,
    rel: &str,
    kind: gix::objs::tree::EntryKind,
    oid: gix::ObjectId,
    origin: Origin,
) -> Result<(), SnapshotError> {
    let path = TargetPath::parse(rel).map_err(|message| SnapshotError::Path {
        path: rel.to_owned(),
        message,
    })?;
    blobs.push(CapturedBlob {
        path: path.clone(),
        kind,
        oid,
    });
    paths.push(PathOwnership::new(path, origin));
    seen.insert(rel.to_owned());
    Ok(())
}

/// Group a plan's mutations by target path, preserving first-seen order.
pub(crate) fn group_plan(plan: &Plan) -> PlanPaths {
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

/// The `HEAD` tree's blobs under the target, keyed by target-relative path, each
/// with its object id and entry kind so a capture can tell an exec-bit or
/// file/link change from an unchanged path, not only a content change.
fn head_target_oids(
    repo: &gix::Repository,
    target: &RepoPath,
) -> Result<BTreeMap<String, (gix::ObjectId, gix::objs::tree::EntryKind)>, SnapshotError> {
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
    oids: &mut BTreeMap<String, (gix::ObjectId, gix::objs::tree::EntryKind)>,
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
        let kind = entry.mode().kind();
        match kind {
            EntryKind::Tree => collect_tree(repo, entry.oid().to_owned(), &path, oids)?,
            EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                oids.insert(path, (entry.oid().to_owned(), kind));
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

/// Read one file from `source_dir` at repository-relative `repo_rel` into a git
/// blob in its stored form (built-in conversions only, no filter driver),
/// recording a symbolic link as a link and the executable bit. `None` when the
/// path is absent or is neither a file nor a link.
fn read_blob(
    repo: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline<'_>,
    source_dir: &Path,
    repo_rel: &str,
    state: &gix::index::State,
) -> Result<Option<(gix::ObjectId, gix::objs::tree::EntryKind)>, SnapshotError> {
    use gix::filter::plumbing::pipeline::convert::ToGitOutcome;
    use gix::objs::tree::EntryKind;
    let full = source_dir.join(repo_rel);
    let md = match std::fs::symlink_metadata(&full) {
        Ok(md) => md,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(SnapshotError::Git(err.to_string())),
    };
    if md.is_symlink() {
        let link = std::fs::read_link(&full).map_err(|e| SnapshotError::Git(e.to_string()))?;
        let oid = repo
            .write_blob(gix::path::into_bstr(link).as_ref())
            .map_err(|e| SnapshotError::Git(e.to_string()))?
            .detach();
        return Ok(Some((oid, EntryKind::Link)));
    }
    if md.is_file() {
        let file = std::fs::File::open(&full).map_err(|e| SnapshotError::Git(e.to_string()))?;
        let outcome = pipeline
            .convert_to_git(file, Path::new(repo_rel), state)
            .map_err(|e| SnapshotError::Git(e.to_string()))?;
        let oid = match outcome {
            ToGitOutcome::Unchanged(mut read) => repo.write_blob_stream(&mut read),
            ToGitOutcome::Buffer(buf) => repo.write_blob(buf),
            // Symmetric with the write side (merge.rs `worktree_bytes`): a process
            // filter cannot appear after the driver strip, so reject it rather
            // than stream its output. Unreachable today on a driver-stripped repo
            // — this keeps the no-subprocess invariant from resting on the strip
            // alone on the read path, so a future strip regression cannot silently
            // spawn a long-running filter during capture.
            ToGitOutcome::Process(_) => {
                return Err(SnapshotError::Git(
                    "unexpected process filter after driver strip".into(),
                ));
            }
        }
        .map_err(|e| SnapshotError::Git(e.to_string()))?
        .detach();
        let kind = if is_executable(&md) {
            EntryKind::BlobExecutable
        } else {
            EntryKind::Blob
        };
        return Ok(Some((oid, kind)));
    }
    Ok(None)
}

#[cfg(unix)]
fn is_executable(md: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    md.mode() & 0o111 != 0
}
#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

/// The base ownership a new plan does not cover: regions the plan does not
/// re-own, and values no plan value sits at or above.
struct Uncovered {
    regions: Vec<String>,
    values: Vec<String>,
}

fn uncovered_ownership(
    regions: &[String],
    values: &[String],
    plan: Option<&&PlanOwnership>,
) -> Uncovered {
    let (whole, new_regions, new_values): (bool, &[String], &[String]) = match plan {
        Some(p) => (p.whole, &p.regions, &p.values),
        None => (false, &[], &[]),
    };
    if whole {
        return Uncovered {
            regions: Vec::new(),
            values: Vec::new(),
        };
    }
    let regions = regions
        .iter()
        .filter(|r| !new_regions.contains(r))
        .cloned()
        .collect();
    let values = values
        .iter()
        .filter(|v| !new_values.iter().any(|nv| covers(nv, v)))
        .cloned()
        .collect();
    Uncovered { regions, values }
}

/// Whether the JSON path `ancestor` sits at or above `descendant`.
fn covers(ancestor: &str, descendant: &str) -> bool {
    descendant == ancestor
        || descendant.starts_with(&format!("{ancestor}."))
        || descendant.starts_with(&format!("{ancestor}["))
}

/// Retract, in the source tree, the uncovered regions and values of one path.
fn apply_retraction(
    source_dir: &Path,
    target: &RepoPath,
    rel: &str,
    uncovered: &Uncovered,
) -> Result<(), SnapshotError> {
    if uncovered.regions.is_empty() && uncovered.values.is_empty() {
        return Ok(());
    }
    let repo_rel = repo_relative(target, rel);
    let full = source_dir.join(&repo_rel);
    let mut bytes = match std::fs::read(&full) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(SnapshotError::Git(err.to_string())),
    };
    let tpath = TargetPath::parse(rel).map_err(|message| SnapshotError::Path {
        path: rel.to_owned(),
        message,
    })?;

    for key in &uncovered.regions {
        let region =
            crate::inject::RegionKey::parse(key).map_err(|message| SnapshotError::Path {
                path: rel.to_owned(),
                message,
            })?;
        let marker = crate::inject::MarkerStyle::infer(&tpath)
            .unwrap_or(crate::inject::MarkerStyle::line("#"));
        bytes = crate::inject::retract_region(&bytes, &region, &marker)
            .map_err(|e| SnapshotError::Git(e.to_string()))?;
    }

    // Remove array elements from the highest index first so earlier ones stay valid.
    let format = json_format(&tpath);
    let mut values = uncovered.values.clone();
    values.sort();
    values.reverse();
    for value in values {
        let json_path =
            crate::inject::JsonPath::parse(&value).map_err(|message| SnapshotError::Path {
                path: rel.to_owned(),
                message,
            })?;
        bytes = crate::inject::retract_json_value(&bytes, &json_path, format)
            .map_err(|e| SnapshotError::Git(e.to_string()))?;
    }

    std::fs::write(&full, bytes).map_err(|e| SnapshotError::Git(e.to_string()))?;
    Ok(())
}

/// The JSON family of a target by extension, defaulting to strict JSON.
fn json_format(path: &TargetPath) -> crate::inject::JsonFormat {
    match path
        .as_path()
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jsonc") => crate::inject::JsonFormat::Jsonc,
        Some("json5") => crate::inject::JsonFormat::Json5,
        _ => crate::inject::JsonFormat::Json,
    }
}

/// The blob a base snapshot holds under `files/<rel>`, if any.
fn base_file_blob(
    repo: &gix::Repository,
    base_id: &SnapshotId,
    rel: &str,
) -> Result<Option<(gix::ObjectId, gix::objs::tree::EntryKind)>, SnapshotError> {
    let name = format!("{}{base_id}", crate::snapshot::record::SNAPSHOT_REF_PREFIX);
    let mut reference = repo
        .find_reference(name.as_str())
        .map_err(|e| SnapshotError::Git(e.to_string()))?;
    let commit = reference
        .peel_to_commit()
        .map_err(|e| SnapshotError::Git(e.to_string()))?;
    let tree = commit
        .tree()
        .map_err(|e| SnapshotError::Git(e.to_string()))?;
    match tree
        .lookup_entry_by_path(format!("files/{rel}"))
        .map_err(|e| SnapshotError::Git(e.to_string()))?
    {
        Some(entry) => Ok(Some((entry.oid().to_owned(), entry.mode().kind()))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests;
