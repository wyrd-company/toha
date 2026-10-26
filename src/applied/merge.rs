// ---
// relationships:
//   implements: architecture
// ---
//! The in-process three-way merge that applies a template update to the
//! operator's working tree and index.
//!
//! The three trees are whole-repository trees so conflict paths are
//! repository-relative and nothing outside the target can change: base is
//! `HEAD` with the target replaced by the base application's verified files (empty for a
//! baseline), ours is `HEAD` (the operator), theirs is `HEAD` with the target
//! replaced by the candidate's captured files. The operator is "ours", so index
//! stage 2 holds the operator's bytes. Toha never runs a merge or filter driver;
//! a path whose attributes name one is kept as the operator's content and
//! reported as a `driver` conflict.

use std::num::NonZeroU8;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use gix::bstr::{BString, ByteSlice};
use gix::filter::plumbing::pipeline::convert::ToWorktreeOutcome;
use gix::objs::tree::EntryKind;

use crate::applied::capture::{self, CaptureInputs};
use crate::applied::project::Project;
use crate::applied::record::{Application, Origin, RepoPath, Revision};
/// The base an update merges from.
#[allow(clippy::large_enum_variant)]
pub enum Base {
    Application(Application),
    Empty,
}

/// Options for an update merge.
#[derive(Debug, Default, Clone, Copy)]
pub struct MergeOptions {
    /// Replace managed-region drift only for this invocation.
    pub force: bool,
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
        record: String,
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
#[non_exhaustive]
pub enum MergeError {
    #[error("the update was prepared for another target")]
    WrongTarget,
    #[error("git error during merge: {0}")]
    Git(String),
    #[error("git error during merge: apply: {0}")]
    Apply(Box<crate::ApplyError>),
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

/// The resolved identity of the candidate application.
pub struct RecordInputs {
    pub template: String,
    pub revision: Revision,
}

/// Apply a template update to the project: build the candidate application through
/// the throwaway checkout, stop early when the result is already current, and
/// otherwise merge it into the working tree. `--update` merges from a base
/// application; `--baseline` merges from an empty base.
#[allow(clippy::too_many_arguments)]
pub fn merge_apply(
    project: &Project,
    base: Base,
    template: &crate::template::Template,
    completed: &crate::interview::Completed,
    inputs: RecordInputs,
    options: MergeOptions,
    runner: &dyn crate::hook::HookRunner,
) -> Result<Merged, MergeError> {
    merge_apply_reported(project, base, template, completed, inputs, options, runner)
        .map(|(merged, _)| merged)
}

/// Messages and hook outcomes from the candidate apply. Captured hook streams
/// remain private to the apply loop.
#[derive(Debug, Default)]
pub struct UpdateReport {
    pub messages: Vec<String>,
    pub hooks: Vec<crate::apply::HookReport>,
}

/// Apply an update and retain its messages and hook outcomes for caller output.
#[allow(clippy::too_many_arguments)]
pub fn merge_apply_reported(
    project: &Project,
    base: Base,
    template: &crate::template::Template,
    completed: &crate::interview::Completed,
    inputs: RecordInputs,
    options: MergeOptions,
    runner: &dyn crate::hook::HookRunner,
) -> Result<(Merged, UpdateReport), MergeError> {
    // Build the plan for the real target with the application's frozen instant.
    let target_path = candidate_target(project)?;
    let mut plan = crate::plan::Plan::build(template, completed, &target_path)
        .map_err(|err| MergeError::Git(err.to_string()))?;

    let mut report = UpdateReport {
        messages: completed.messages.clone(),
        hooks: vec![],
    };
    if let Some(before) = &plan.before_apply {
        report.messages.push(before.clone());
    }

    // Untrusted hooks: report the plan without running anything.
    if !options.trusted && !plan.hooks.is_empty() {
        return Ok((Merged::NeedsTrust, report));
    }

    plan.record_identity(inputs.template.clone(), inputs.revision.clone());
    let capture_inputs = plan
        .record
        .take()
        .expect("a completed plan has record inputs");
    let (new, candidate_report) = build_candidate_reported(
        project,
        &base,
        plan,
        capture_inputs,
        !options.dry_run,
        options.force,
        runner,
    )?;

    report.messages.extend(candidate_report.messages);
    report.hooks = candidate_report.hooks;
    if let Base::Application(base_application) = &base
        && is_already_current(project, base_application, &new, &inputs)?
    {
        return Ok((Merged::AlreadyCurrent, report));
    }
    let result = merge_into_worktree(project, &base, &new, &options);
    result.map(|merged| (merged, report))
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

/// Build the candidate application through the throwaway checkout: check out HEAD,
/// retract uncovered base ownership, apply the plan (and hooks unless it is a dry
/// run), then capture with the base. The checkout is deleted afterwards.
///
/// `merge_apply` already refused untrusted hooks before calling this function
/// (`Merged::NeedsTrust`), so trust is always established here, or there are no
/// hooks to run. A dry run still renders every file into the checkout so the
/// candidate reflects the real change — it just drops the plan's hooks first,
/// so the apply loop runs none of them; `trusted: true` is therefore always
/// correct and never re-triggers the untrusted refusal for files alone.
#[cfg(test)]
fn build_candidate(
    project: &Project,
    base: &Base,
    plan: crate::plan::Plan,
    inputs: CaptureInputs,
    run_hooks: bool,
    runner: &dyn crate::hook::HookRunner,
) -> Result<Application, MergeError> {
    build_candidate_reported(project, base, plan, inputs, run_hooks, false, runner)
        .map(|(application, _)| application)
}

fn build_candidate_reported(
    project: &Project,
    base: &Base,
    plan: crate::plan::Plan,
    inputs: CaptureInputs,
    run_hooks: bool,
    force: bool,
    runner: &dyn crate::hook::HookRunner,
) -> Result<(Application, UpdateReport), MergeError> {
    let mut report = UpdateReport::default();
    let repo = project.repo();
    let target = project.target().clone();
    let checkout = tempfile::tempdir().map_err(|e| MergeError::Io(e.to_string()))?;

    checkout_head(repo, checkout.path())?;

    // Group the plan once, since Plan::apply consumes it.
    let plan_paths = capture::group_plan(&plan);

    // Step 3: retract uncovered base ownership on the checkout before the apply.
    let base_application = match base {
        Base::Application(application) => Some(application),
        Base::Empty => None,
    };
    if let Some(base_application) = base_application {
        capture::retract_uncovered(checkout.path(), &target, &plan_paths, base_application)
            .map_err(|e| MergeError::Git(e.to_string()))?;
    }

    // Step 4: apply the plan into the checkout. A dry run drops the hooks from
    // the plan first, so every file still renders but the apply loop has none
    // to run.
    let mut plan = plan;
    if !run_hooks {
        plan.hooks.clear();
    }
    // An update's project is already a repository; the scratch checkout is
    // never initialized or given a remote.
    plan.git = None;
    plan.record = None;
    let checkout_target_path = if target.is_root() {
        checkout.path().to_owned()
    } else {
        checkout.path().join(target.as_str())
    };
    let checkout_target = crate::staging::canonical_target(&checkout_target_path)
        .map_err(|e| MergeError::Io(e.to_string()))?;
    // Retractions and planned whole files define the effective input image.
    // Refuse region drift before candidate apply uses its internal overwrite.
    plan.preflight_edits(&checkout_target, force)
        .map_err(|e| MergeError::Apply(Box::new(e)))?;
    let before = capture::observe(checkout_target.as_path()).map_err(git)?;
    match plan
        .apply(
            &checkout_target,
            crate::apply::ApplyOptions {
                force: true,
                trusted: true,
            },
            runner,
        )
        .map_err(|e| MergeError::Apply(Box::new(e)))?
    {
        crate::apply::Applied::Written {
            hooks, after_apply, ..
        } => {
            report.hooks = hooks;
            // The scratch checkout is written even for a preview; only a
            // real apply may report its after-apply message to the caller.
            if run_hooks {
                report.messages.extend(after_apply);
            }
        }
        // Unreachable given the precondition above (trust established, or no
        // hooks left to trigger the refusal): refuse to capture an incomplete
        // checkout rather than silently proceed.
        crate::apply::Applied::NeedsTrust(_) => {
            return Err(MergeError::Git(
                "internal: a candidate build reached an untrusted-hooks refusal \
                 after trust was already established"
                    .to_owned(),
            ));
        }
    }

    let application = capture::capture(
        repo,
        checkout.path(),
        &target,
        &plan_paths,
        base_application,
        inputs,
        Some(&before),
    )
    .map_err(|e| MergeError::Git(e.to_string()))?;
    Ok((application, report))
}

fn is_already_current(
    project: &Project,
    base: &Application,
    new: &Application,
    inputs: &RecordInputs,
) -> Result<bool, MergeError> {
    if base.revision() != &inputs.revision
        || base.answers != new.answers
        || base.parts != new.parts
        || base.context != new.context
    {
        return Ok(false);
    }
    if base.files == new.files {
        return Ok(true);
    }
    // Captured identities remain immutable. Compare their built-in normalized
    // forms so a historical CRLF index entry cannot invent another update.
    Ok(normalized_files(project, base)? == normalized_files(project, new)?)
}

fn normalized_files(project: &Project, application: &Application) -> Result<Entries, MergeError> {
    use gix::filter::plumbing::pipeline::convert::ToGitOutcome;
    let repo = project.repo();
    let (mut pipeline, _) = repo.filter_pipeline(None).map_err(git)?;
    // This pipeline only compares bytes and computes hashes. It never admits
    // content to the index or writes blobs, so safecrlf's check-in round-trip
    // policy must not reject historical inputs during comparison.
    let (comparison, _) = repo.filter_pipeline(None).map_err(git)?;
    let (mut comparison, mut attributes) = comparison.into_parts();
    comparison.options_mut().crlf_roundtrip_check =
        gix::filter::plumbing::pipeline::CrlfRoundTripCheck::Skip;
    let mut entries = Entries::new();
    for (path, (id, kind)) in &application.files {
        let mode = gix::index::entry::Mode::from(*kind);
        let id = if *kind == EntryKind::Link {
            *id
        } else {
            let relative = repo_path_for(project.target(), &BString::from(path.as_str()));
            let bytes = worktree_bytes(repo, &mut pipeline, *id, relative.as_str(), mode)?;
            let entry = attributes
                .at_path(Path::new(relative.as_str()), None, &repo.objects)
                .map_err(git)?;
            let converted = comparison
                .convert_to_git(
                    bytes.as_slice(),
                    Path::new(relative.as_str()),
                    &mut |_, attrs| {
                        entry.matching_attributes(attrs);
                    },
                    // Renormalize independently of historical indexed CRLF.
                    &mut |_| Ok(None),
                )
                .map_err(git)?;
            match converted {
                ToGitOutcome::Unchanged(bytes) | ToGitOutcome::Buffer(bytes) => {
                    gix::objs::compute_hash(repo.object_hash(), gix::objs::Kind::Blob, bytes)
                }
                ToGitOutcome::Process(_) => {
                    return Err(MergeError::Git(
                        "unexpected process filter after driver strip".into(),
                    ));
                }
            }
            .map_err(git)?
        };
        entries.insert(BString::from(path.as_str()), (id, mode));
    }
    Ok(entries)
}

/// Check out `HEAD`'s whole tree into `dest`, with no filter driver (drivers are
/// stripped on the repository), never a linked worktree.
pub(super) fn checkout_head(repo: &gix::Repository, dest: &Path) -> Result<(), MergeError> {
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

/// Merge the new application into the operator's working tree against `base`. The
/// candidate and reconstructed base hold transient verified file entries.
pub(crate) fn merge_into_worktree(
    project: &Project,
    base: &Base,
    new: &Application,
    options: &MergeOptions,
) -> Result<Merged, MergeError> {
    merge_inner(project, base, new, options)
}

fn merge_inner(
    project: &Project,
    base: &Base,
    new: &Application,
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
        Base::Application(application) => {
            let files = application_files_tree(project, application)?;
            let mut editor = repo.edit_tree(files).map_err(git)?;
            // The previous update captured the retracted operator file with
            // empty edit ownership. Once the candidate stops capturing it,
            // omit it from both merge inputs so absence cannot mean deletion.
            // A candidate that owns it again still needs its original base.
            for entry in application.paths() {
                if matches!(entry.origin(), Origin::Edit { regions, values } if regions.is_empty() && values.is_empty())
                    && !new
                        .paths()
                        .iter()
                        .any(|candidate| candidate.path() == entry.path())
                {
                    editor
                        .remove(entry.path().to_string().as_str())
                        .map_err(git)?;
                }
            }
            editor.write().map_err(git)?.detach()
        }
        // Write the empty tree so the merge can read it as the base; the
        // well-known id is otherwise not guaranteed to be present in the odb.
        Base::Empty => repo
            .write_object(gix::objs::Tree::empty())
            .map_err(git)?
            .detach(),
    };
    let new_files = application_files_tree(project, new)?;

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

    // Compare all three blobs in repository form, including older CRLF blobs.
    // Only built-in filters remain on Project's repository. Renormalization
    // respects -text/binary attributes and leaves symlink targets untouched.
    let mut merge_repo = repo.clone();
    let mut config = merge_repo.config_snapshot_mut();
    config
        .set_raw_value("merge.renormalize", "true")
        .map_err(git)?;
    config.commit().map_err(git)?;
    // Only the merge input converter gets this exception. It reads stored
    // objects into memory buffers; capture and the write transaction keep the
    // repository's safecrlf policy. The merge result writer receives bytes
    // separately and never uses this filter pipeline.
    let mut blob_merge = merge_repo
        .merge_resource_cache(Default::default())
        .map_err(git)?;
    blob_merge.filter.filter.options_mut().crlf_roundtrip_check =
        gix::filter::plumbing::pipeline::CrlfRoundTripCheck::Skip;
    let mut diff_cache = merge_repo
        .diff_resource_cache_for_tree_diff()
        .map_err(git)?;
    let mut outcome = gix::merge::plumbing::tree(
        base_full.as_ref(),
        ours_full.as_ref(),
        theirs_full.as_ref(),
        labels,
        &merge_repo,
        |buf| merge_repo.write_blob(buf).map(|id| id.detach()),
        &mut Default::default(),
        &mut diff_cache,
        &mut blob_merge,
        opts,
    )
    .map_err(git)?;
    let how = gix::merge::tree::TreatAsUnresolved::git();

    // Compute the change list from the merged tree vs the operator's target.
    let merged_full = outcome.tree.write(|tree| {
        // Preserve the porcelain editor's entry validation before this tree
        // is stored. The plumbing editor cannot be wrapped directly.
        use gix::objs::WriteTo;
        let mut data = Vec::new();
        tree.write_to(&mut data).map_err(git)?;
        let id = gix::objs::compute_hash(merge_repo.object_hash(), gix::objs::Kind::Tree, &data)
            .map_err(git)?;
        gix::Tree::from_data(id, data, &merge_repo)
            .edit()
            .map_err(git)?
            .write()
            .map(|id| id.detach())
            .map_err(git)
    })?;
    let mut outcome = gix::merge::tree::Outcome {
        tree: merge_repo.edit_tree(merged_full).map_err(git)?,
        conflicts: outcome.conflicts,
        failed_on_first_unresolved_conflict: outcome.failed_on_first_unresolved_conflict,
    };
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

    // A missing recorded object is never an empty-base deletion. Retain the
    // operator path and expose the deletion in conflict stages.
    if let Base::Application(application) = base {
        for owned in application.paths() {
            let path = owned.path().to_string();
            if !matches!(owned.origin(), Origin::Edit { .. })
                && !application.files.contains_key(&path)
                && !theirs_entries.contains_key(path.as_bytes())
                && let Some(ours) = ours_entries.get(path.as_bytes())
            {
                let relative = repo_path_for(target, &BString::from(path.as_str())).to_string();
                conflict_kinds.insert(relative.clone(), ConflictKind::ModifyDelete);
                driver_stages.insert(relative, [None, Some(*ours), None]);
            }
        }
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
    for (path, kind) in &conflict_kinds {
        if !changes.iter().any(|c| c.path.as_str() == path) {
            changes.push(Change {
                path: RepoPath::parse(path).map_err(git)?,
                action: Action::Conflicted(*kind),
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
            if on_disk.symlink_metadata().is_ok() {
                occupied.push(repo_path);
            }
        }
    }
    if !occupied.is_empty() {
        return Err(MergeError::Occupied(occupied));
    }

    // Only regular-file text conflicts receive diff3 markers. Symlink and
    // file/link conflicts retain the operator's node, even when gitoxide
    // classifies them as content conflicts; their stages still carry all sides.
    let keep_operator: std::collections::BTreeSet<String> = conflict_kinds
        .iter()
        .filter(|(path, kind)| {
            !matches!(kind, ConflictKind::Content)
                || outcome.conflicts.iter().any(|conflict| {
                    conflict.is_unresolved(how)
                        && conflict.ours.location().as_bstr() == path.as_bytes().as_bstr()
                        && conflict
                            .entries()
                            .iter()
                            .flatten()
                            .any(|entry| entry.mode.is_link())
                })
        })
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
        new,
    )?;

    Ok(Merged::Written {
        record: project.record_relative().map_err(git)?,
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
    application: &Application,
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
        || project.ensure_record_clean().is_err()
        || !matches!(
            project.cleanliness(),
            Ok(crate::applied::Cleanliness::Clean)
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

    let record_path = project.record_path().map_err(git)?;
    let previous_record = std::fs::read(&record_path).ok();
    if let Err(error) = project.write_application(application.clone()) {
        rollback(repo, &mut pipeline, state, &workdir, &done)?;
        return Err(git(error));
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
        match previous_record {
            Some(bytes) => {
                std::fs::write(&record_path, bytes).map_err(|e| MergeError::Io(e.to_string()))?;
            }
            None => {
                std::fs::remove_file(&record_path).map_err(|e| MergeError::Io(e.to_string()))?;
            }
        }
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
        let bytes = worktree_bytes(repo, pipeline, *id, &repo_rel, *mode)?;
        write_atomically(workdir, &repo_rel, &bytes, *mode)?;
        done.push(Restorable {
            repo_rel,
            head: expected.copied(),
            wrote: Some(*id),
        });
        #[cfg(test)]
        inject_fail::check(done, workdir)?;
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
        if full.symlink_metadata().is_ok() {
            std::fs::remove_file(&full).map_err(|e| MergeError::Io(e.to_string()))?;
        }
        done.push(Restorable {
            repo_rel,
            head: ours.get(path).copied(),
            wrote: None,
        });
        #[cfg(test)]
        inject_fail::check(done, workdir)?;
    }
    Ok(())
}

/// Test-only deterministic failure injection for the write transaction. Armed by
/// a test, [`check`] fails immediately after the Nth `done.push` — after the real
/// write or delete happened but before the index is committed — recording the
/// on-disk bytes of every path written so far, so a test can prove the mutations
/// actually occurred before rollback restored them. Compiled only under `cfg(test)`.
#[cfg(test)]
mod inject_fail {
    use super::{MergeError, Restorable};
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    thread_local! {
        static AFTER: Cell<Option<usize>> = const { Cell::new(None) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
        static OBSERVED: RefCell<Vec<(String, Option<Vec<u8>>)>> = const { RefCell::new(Vec::new()) };
    }

    /// Arm the hook to fail after the `after`-th push (1-based). Resets the count.
    pub(super) fn arm(after: usize) {
        AFTER.with(|a| a.set(Some(after)));
        COUNT.with(|c| c.set(0));
        OBSERVED.with(|o| o.borrow_mut().clear());
    }

    pub(super) fn disarm() {
        AFTER.with(|a| a.set(None));
    }

    /// The bytes or raw link target of each path at the moment of failure.
    pub(super) fn observed() -> Vec<(String, Option<Vec<u8>>)> {
        OBSERVED.with(|o| o.borrow().clone())
    }

    /// Called after each `done.push`. When the push count reaches the armed point,
    /// record every done path's current on-disk bytes and return an injected error.
    pub(super) fn check(done: &[Restorable], workdir: &Path) -> Result<(), MergeError> {
        let n = COUNT.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        });
        if AFTER.with(|a| a.get()) == Some(n) {
            let snap: Vec<_> = done
                .iter()
                .map(|r| {
                    (r.repo_rel.clone(), {
                        let full = workdir.join(&r.repo_rel);
                        if full.symlink_metadata().is_ok_and(|md| md.is_symlink()) {
                            std::fs::read_link(&full)
                                .ok()
                                .map(|target| gix::path::into_bstr(target).to_vec())
                        } else {
                            std::fs::read(full).ok()
                        }
                    })
                })
                .collect();
            OBSERVED.with(|o| *o.borrow_mut() = snap);
            return Err(MergeError::Io(
                "injected transaction failure after push".into(),
            ));
        }
        Ok(())
    }
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

/// Raw Git bytes for a symlink target; worktree bytes for a regular file,
/// applying only built-in conversions.
fn worktree_bytes(
    repo: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline<'_>,
    id: gix::ObjectId,
    repo_rel: &str,
    mode: gix::index::entry::Mode,
) -> Result<Vec<u8>, MergeError> {
    let data = repo.find_object(id).map_err(git)?.data.clone();
    if mode == gix::index::entry::Mode::SYMLINK {
        return Ok(data); // Link targets are raw Git bytes, never smudged text.
    }
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

/// Write a temporary node and rename it over `workdir/repo_rel`. Git mode
/// selects a raw-target symlink or a regular file with its executable bit.
/// Failure before rename leaves the operator's node in place.
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
    if mode == gix::index::entry::Mode::SYMLINK {
        // Own the temporary name until rename without opening/following the
        // target. The directory also removes an uncommitted link on failure.
        let temporary = tempfile::Builder::new()
            .prefix(".toha-")
            .tempdir_in(full.parent().expect("a worktree path has a parent"))
            .map_err(|e| MergeError::Io(e.to_string()))?;
        let link = temporary.path().join("link");
        let target = gix::path::from_bstr(gix::bstr::BStr::new(bytes));
        #[cfg(not(windows))]
        gix::fs::symlink::create(target.as_ref(), &link)
            .map_err(|e| MergeError::Io(e.to_string()))?;
        #[cfg(windows)]
        {
            // Resolve the target type relative to the final link, rather than
            // the temporary directory (Windows distinguishes directory links).
            let result = if full.parent().expect("parent").join(&target).is_dir() {
                std::os::windows::fs::symlink_dir(&target, &link)
            } else {
                std::os::windows::fs::symlink_file(&target, &link)
            };
            result.map_err(|e| MergeError::Io(e.to_string()))?;
        }
        std::fs::rename(&link, &full).map_err(|e| MergeError::Io(e.to_string()))?;
        return Ok(());
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(".toha-");
    // Preserve ordinary file creation permissions (subject to the process umask).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let mut tmp = builder
        .tempfile_in(full.parent().expect("a worktree path has a parent"))
        .map_err(|e| MergeError::Io(e.to_string()))?;
    use std::io::Write;
    tmp.write_all(bytes)
        .map_err(|e| MergeError::Io(e.to_string()))?;
    #[cfg(unix)]
    if mode == gix::index::entry::Mode::FILE_EXECUTABLE {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))
            .map_err(|e| MergeError::Io(e.to_string()))?;
    }
    tmp.persist(&full)
        .map_err(|e| MergeError::Io(e.to_string()))?;
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
                let bytes = worktree_bytes(repo, pipeline, oid, &entry.repo_rel, mode)?;
                write_atomically(workdir, &entry.repo_rel, &bytes, mode)?;
            }
            None => {
                // An add: remove the created file.
                let full = workdir.join(&entry.repo_rel);
                if full.symlink_metadata().is_ok() {
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
    // Publish the record in the same index transaction as the merged files.
    let relative = project.record_relative().map_err(git)?;
    let record = std::fs::read(project.record_path().map_err(git)?)
        .map_err(|e| MergeError::Io(e.to_string()))?;
    let oid = repo.write_blob(&record).map_err(git)?.detach();
    index.remove_entries(|_, path, _| path == relative.as_bytes());
    index.dangerously_push_entry(
        Default::default(),
        oid,
        gix::index::entry::Flags::empty(),
        gix::index::entry::Mode::FILE,
        gix::bstr::BStr::new(relative.as_bytes()),
    );
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

fn application_files_tree(
    project: &Project,
    application: &Application,
) -> Result<gix::ObjectId, MergeError> {
    let repo = project.repo();
    let mut editor = repo
        .edit_tree(gix::ObjectId::empty_tree(repo.object_hash()))
        .map_err(git)?;
    for (path, (oid, kind)) in &application.files {
        editor.upsert(path.as_str(), *kind, *oid).map_err(git)?;
    }
    Ok(editor.write().map_err(git)?.detach())
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

#[cfg(test)]
mod tests;
