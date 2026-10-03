// ---
// relationships:
//   implements: architecture
// ---
//! Repository access for snapshots.
//!
//! [`Project`] opens the target's repository once through gitoxide and, before
//! any status, checkout, conversion, or merge, removes every configured merge
//! and filter driver from the in-memory configuration so no driver program can
//! start (see [`strip_drivers`]). It changes nothing on disk. Built-in
//! conversions still apply, so a path stored through a filter is compared and
//! written in its stored form.

use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::EntryKind;

use crate::snapshot::record::{RepoPath, SNAPSHOT_REF_PREFIX, Snapshot, SnapshotError, SnapshotId};
use crate::staging::CanonicalTarget;
use std::collections::BTreeSet;

/// A target directory inside a git repository, opened with driver programs
/// disabled in memory.
pub struct Project {
    repo: gix::Repository,
    /// The target directory relative to the repository root.
    rel: RepoPath,
}

/// Whether a target is clean, or the repository-relative paths that are not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cleanliness {
    Clean,
    Dirty { paths: Vec<RepoPath> },
}

impl Project {
    /// Open the repository that contains `target`, or `Ok(None)` when the target
    /// is not inside a git repository. The returned project has every configured
    /// driver removed from its in-memory config.
    ///
    /// Discovery starts from the nearest ancestor of `target` that exists on
    /// disk, so a target subdirectory an `apply` has not created yet still
    /// resolves its repository (an approved clean-git generate into a fresh
    /// subpath, and its repository-wide snapshot reader, both open the project
    /// before the subpath exists). The target-relative identity, path validation,
    /// and repository-containment bound are unchanged: `rel` is still computed
    /// from `target` itself against the canonical working tree, and a target
    /// outside the working tree is still refused. For a target that already
    /// exists, the nearest existing ancestor is the target, so discovery is
    /// byte-for-byte as before.
    pub fn open(target: &CanonicalTarget) -> Result<Option<Project>, ProjectError> {
        let discover_from = nearest_existing_ancestor(target.as_path());
        let mut repo = match gix::discover(&discover_from) {
            Ok(repo) => repo,
            Err(gix::discover::Error::Discover(err)) if is_not_a_repository(&err) => {
                return Ok(None);
            }
            Err(err) => return Err(ProjectError::Open(err.to_string())),
        };
        strip_drivers(&mut repo)?;

        let workdir = repo
            .workdir()
            .ok_or(ProjectError::Bare)?
            .to_owned()
            .canonicalize()
            .map_err(|err| ProjectError::Io(err.to_string()))?;
        let rel = match target.as_path().strip_prefix(&workdir) {
            Ok(suffix) if suffix.as_os_str().is_empty() => RepoPath::root(),
            Ok(suffix) => {
                // Git records slash-separated components on every host. Convert
                // the OS path at this boundary before validating the git path.
                let git_path = suffix
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                RepoPath::parse(&git_path).map_err(|err| ProjectError::Path(err.to_string()))?
            }
            Err(_) => return Err(ProjectError::Outside(target.to_string())),
        };
        Ok(Some(Project { repo, rel }))
    }

    /// The target directory relative to the repository root.
    pub fn target(&self) -> &RepoPath {
        &self.rel
    }

    /// The opened repository, with drivers already stripped. Crate-internal seams
    /// (capture, merge) read and write git objects through it.
    pub(crate) fn repo(&self) -> &gix::Repository {
        &self.repo
    }

    /// Capture a snapshot of the applied target read at `source_dir` (the project
    /// working tree for a plain apply, the throwaway checkout for `--from`),
    /// against an optional `base` snapshot. Returns the saved id.
    pub fn capture(
        &self,
        source_dir: &std::path::Path,
        plan: &crate::plan::Plan,
        base: Option<&Snapshot>,
        inputs: crate::snapshot::capture::CaptureInputs,
    ) -> Result<SnapshotId, SnapshotError> {
        let plan_paths = crate::snapshot::capture::group_plan(plan);
        crate::snapshot::capture::capture(
            &self.repo,
            source_dir,
            &self.rel,
            &plan_paths,
            base,
            inputs,
        )
    }

    /// Whether the target directory has no tracked modification, no staged
    /// change, and no untracked file git does not ignore. Dirt outside the
    /// target does not count.
    pub fn cleanliness(&self) -> Result<Cleanliness, ProjectError> {
        let patterns: Vec<BString> = if self.rel.is_root() {
            Vec::new()
        } else {
            vec![BString::from(format!(":(top){}/", self.rel))]
        };
        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|err| ProjectError::Status(err.to_string()))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(None)
            .into_iter(patterns)
            .map_err(|err| ProjectError::Status(err.to_string()))?;

        let mut paths = Vec::new();
        for item in iter {
            let item = item.map_err(|err| ProjectError::Status(err.to_string()))?;
            let rela = match &item {
                gix::status::Item::TreeIndex(change) => {
                    change.location().to_str_lossy().into_owned()
                }
                gix::status::Item::IndexWorktree(change) => {
                    change.rela_path().to_str_lossy().into_owned()
                }
            };
            paths.push(RepoPath::parse(&rela).map_err(|err| ProjectError::Path(err.to_string()))?);
        }
        paths.sort();
        paths.dedup();
        Ok(if paths.is_empty() {
            Cleanliness::Clean
        } else {
            Cleanliness::Dirty { paths }
        })
    }

    /// Every snapshot whose `target` is this project's target, newest first, plus
    /// every snapshot ref that cannot be read, listed as invalid with its reason.
    pub fn snapshots(&self) -> Result<Vec<Listed>, ProjectError> {
        let mut listed: Vec<Listed> = self
            .all_snapshots()?
            .into_iter()
            .filter(|entry| match entry {
                Listed::Valid(snapshot) => snapshot.target() == &self.rel,
                Listed::Invalid { .. } => true,
            })
            .collect();
        sort_listed_newest_first(&mut listed);
        Ok(listed)
    }

    /// Every snapshot in the repository, newest first, regardless of target, plus
    /// every snapshot ref that cannot be read, listed as invalid with its reason.
    ///
    /// The repository-wide sibling of [`Project::snapshots`], which is
    /// target-scoped. The generate axis (`--like`) seeds a new application from a
    /// prior one that lives at a *different* subpath, so its snapshot's `target`
    /// is not this project's target; a target-scoped listing would never find it.
    /// This is purely additive and reuses the same ref enumeration and validation;
    /// it changes no target-scoped behaviour and writes nothing.
    pub fn snapshots_all(&self) -> Result<Vec<Listed>, ProjectError> {
        let mut listed = self.all_snapshots()?;
        sort_listed_newest_first(&mut listed);
        Ok(listed)
    }

    /// Resolve a snapshot from any unique prefix of at least six characters,
    /// across every valid snapshot in the repository regardless of its target so
    /// that a snapshot for another target can still be named and refused later.
    pub fn find(&self, prefix: &str) -> Result<Snapshot, SnapshotError> {
        let normalised = SnapshotId::parse_prefix(prefix)?;
        let mut matches: Vec<Snapshot> = self
            .all_snapshots()
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .into_iter()
            .filter_map(|entry| match entry {
                Listed::Valid(snapshot) if snapshot.id().has_prefix(&normalised) => Some(snapshot),
                _ => None,
            })
            .collect();
        match matches.len() {
            0 => Err(SnapshotError::Unknown(prefix.to_owned())),
            1 => Ok(matches.pop().expect("one match")),
            _ => Err(SnapshotError::Ambiguous {
                prefix: prefix.to_owned(),
                matches: matches.iter().map(|s| s.id().to_string()).collect(),
            }),
        }
    }

    /// Mark one likely base per source among `snapshots`: the newest snapshot
    /// whose project commit is an ancestor of `HEAD`, or, when none is, the
    /// snapshot whose `files/` match the most paths in the target at `HEAD`,
    /// marked by content. A mark never selects a base.
    pub fn likely_bases(&self, snapshots: &[Snapshot]) -> Result<Vec<LikelyBase>, ProjectError> {
        let head = self.repo.head_id().ok().map(|id| id.detach());
        let head_target = match &head {
            Some(_) => self.head_target_oids()?,
            None => std::collections::BTreeMap::new(),
        };

        // Group indices by source, preserving discovery order of sources.
        let mut sources: Vec<&str> = Vec::new();
        for snapshot in snapshots {
            if !sources.contains(&snapshot.source()) {
                sources.push(snapshot.source());
            }
        }

        let mut marks = Vec::new();
        for source in sources {
            let group: Vec<&Snapshot> = snapshots.iter().filter(|s| s.source() == source).collect();

            // Ancestry: newest snapshot whose project commit is an ancestor of HEAD.
            let by_ancestry = head.and_then(|head| {
                group
                    .iter()
                    .filter(|s| self.is_ancestor(s.project().commit(), head))
                    .max_by(|a, b| a.id().cmp(b.id()))
                    .map(|s| (*s, LikelyBaseBy::Ancestry))
            });

            let chosen = match by_ancestry {
                Some(chosen) => Some(chosen),
                None => {
                    // By content: the snapshot matching the most paths at HEAD.
                    let mut best: Option<(&Snapshot, usize)> = None;
                    for snapshot in &group {
                        let score = self.content_score(snapshot, &head_target)?;
                        let better = match best {
                            None => true,
                            Some((current, best_score)) => {
                                score > best_score
                                    || (score == best_score && snapshot.id() > current.id())
                            }
                        };
                        if better {
                            best = Some((snapshot, score));
                        }
                    }
                    best.map(|(snapshot, _)| (snapshot, LikelyBaseBy::Content))
                }
            };

            if let Some((snapshot, by)) = chosen {
                marks.push(LikelyBase {
                    id: *snapshot.id(),
                    source: source.to_owned(),
                    by,
                });
            }
        }
        Ok(marks)
    }

    /// Remove the named snapshots, deleting their refs. Unknown ids are reported,
    /// not fatal. A request that would remove every snapshot of a source is
    /// refused as a whole unless `force`, and then removes nothing.
    pub fn remove(&self, ids: &[SnapshotId], force: bool) -> Result<Removed, SnapshotError> {
        let all: Vec<Snapshot> = self
            .all_snapshots()
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .into_iter()
            .filter_map(|entry| match entry {
                Listed::Valid(snapshot) => Some(snapshot),
                Listed::Invalid { .. } => None,
            })
            .collect();

        let mut removed = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            if all.iter().any(|s| s.id() == id) {
                removed.push(*id);
            } else {
                not_found.push(*id);
            }
        }

        if !force {
            // Refuse if the removal set empties any source.
            for snapshot in &all {
                let source = snapshot.source();
                let total = all.iter().filter(|s| s.source() == source).count();
                let removing = all
                    .iter()
                    .filter(|s| s.source() == source && removed.contains(s.id()))
                    .count();
                if total > 0 && removing == total {
                    return Err(SnapshotError::WholeSource {
                        name: source.to_owned(),
                    });
                }
            }
        }

        for id in &removed {
            let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
            self.repo
                .find_reference(name.as_str())
                .map_err(|err| SnapshotError::Git(err.to_string()))?
                .delete()
                .map_err(|err| SnapshotError::Git(err.to_string()))?;
        }
        Ok(Removed { removed, not_found })
    }

    /// Add the snapshot fetch refspec to a remote in the repository's local git
    /// config, unless it is already present. Writes nothing else, no push
    /// setting, and never fetches. Idempotent.
    pub fn add_fetch(&self, remote: &str) -> Result<FetchSetting, ProjectError> {
        const REFSPEC: &str = "+refs/toha/snapshots/*:refs/toha/snapshots/*";
        let path = self.repo.common_dir().join("config");
        let mut file =
            gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
                .map_err(|err| ProjectError::Config(err.to_string()))?;

        // The remote must already exist; init never creates one.
        if file
            .string(format!("remote.{remote}.url").as_str())
            .is_none()
        {
            return Err(ProjectError::RemoteMissing(remote.to_owned()));
        }

        let already = file
            .strings(format!("remote.{remote}.fetch").as_str())
            .unwrap_or_default()
            .iter()
            .any(|value| value.to_str_lossy() == REFSPEC);
        if already {
            return Ok(FetchSetting {
                remote: remote.to_owned(),
                refspec: REFSPEC.to_owned(),
                added: false,
            });
        }

        {
            let mut section = file
                .section_mut_or_create_new("remote", remote)
                .map_err(|err| ProjectError::Config(err.to_string()))?;
            section
                .push("fetch", REFSPEC)
                .map_err(|err| ProjectError::Config(err.to_string()))?;
        }
        let mut out =
            std::fs::File::create(&path).map_err(|err| ProjectError::Io(err.to_string()))?;
        file.write_to(&mut out)
            .map_err(|err| ProjectError::Io(err.to_string()))?;
        Ok(FetchSetting {
            remote: remote.to_owned(),
            refspec: REFSPEC.to_owned(),
            added: true,
        })
    }

    /// The `HEAD` tree's leaf blob ids under the target, keyed by target-relative
    /// path, for content scoring.
    fn head_target_oids(
        &self,
    ) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, ProjectError> {
        let head_tree = self
            .repo
            .head_tree()
            .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
        let target_tree = if self.rel.is_root() {
            head_tree.id().detach()
        } else {
            match head_tree
                .lookup_entry_by_path(self.rel.as_str())
                .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?
            {
                Some(entry) if entry.mode().is_tree() => entry.oid().to_owned(),
                _ => return Ok(std::collections::BTreeMap::new()),
            }
        };
        let mut oids = std::collections::BTreeMap::new();
        self.tree_leaf_oids(target_tree, "", &mut oids)
            .map_err(ProjectError::Snapshot)?;
        Ok(oids)
    }

    /// How many of a snapshot's `files/` paths are present at `HEAD` with the
    /// same blob id.
    fn content_score(
        &self,
        snapshot: &Snapshot,
        head_target: &std::collections::BTreeMap<String, gix::ObjectId>,
    ) -> Result<usize, ProjectError> {
        let files = self
            .snapshot_files_oids(snapshot.id())
            .map_err(ProjectError::Snapshot)?;
        Ok(files
            .iter()
            .filter(|(path, oid)| head_target.get(*path) == Some(oid))
            .count())
    }

    /// The leaf blob ids under a snapshot's `files/` tree.
    fn snapshot_files_oids(
        &self,
        id: &SnapshotId,
    ) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, SnapshotError> {
        let name = format!("{SNAPSHOT_REF_PREFIX}{id}");
        let mut reference = self
            .repo
            .find_reference(name.as_str())
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        let commit = reference
            .peel_to_commit()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        let tree = commit
            .tree()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        let mut oids = std::collections::BTreeMap::new();
        if let Some(entry) = tree
            .lookup_entry_by_path("files")
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            && entry.mode().is_tree()
        {
            self.tree_leaf_oids(entry.oid().to_owned(), "", &mut oids)?;
        }
        Ok(oids)
    }

    /// Walk a tree, collecting leaf blob ids keyed by path.
    fn tree_leaf_oids(
        &self,
        tree_oid: gix::ObjectId,
        prefix: &str,
        oids: &mut std::collections::BTreeMap<String, gix::ObjectId>,
    ) -> Result<(), SnapshotError> {
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        for entry in tree.iter() {
            let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
            let name = entry.filename().to_str_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match entry.mode().kind() {
                EntryKind::Tree => self.tree_leaf_oids(entry.oid().to_owned(), &path, oids)?,
                EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                    oids.insert(path, entry.oid().to_owned());
                }
                EntryKind::Commit => {}
            }
        }
        Ok(())
    }

    /// Whether `ancestor` is an ancestor of `head` (or equal to it).
    fn is_ancestor(
        &self,
        ancestor: &crate::snapshot::record::CommitId,
        head: gix::ObjectId,
    ) -> bool {
        let Ok(ancestor) = gix::ObjectId::from_hex(ancestor.as_str().as_bytes()) else {
            return false;
        };
        match self.repo.merge_base(ancestor, head) {
            Ok(base) => base.detach() == ancestor,
            Err(_) => false,
        }
    }

    /// Read every ref under the snapshot namespace, valid or not.
    fn all_snapshots(&self) -> Result<Vec<Listed>, ProjectError> {
        let platform = self
            .repo
            .references()
            .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
        let iter = platform
            .prefixed(SNAPSHOT_REF_PREFIX)
            .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
        let mut out = Vec::new();
        for reference in iter {
            let mut reference = reference
                .map_err(|err| ProjectError::Snapshot(SnapshotError::Git(err.to_string())))?;
            let full = reference.name().as_bstr().to_str_lossy().into_owned();
            let id_segment = full.strip_prefix(SNAPSHOT_REF_PREFIX).unwrap_or(&full);
            let id = match id_segment.parse::<SnapshotId>() {
                Ok(id) => id,
                Err(err) => {
                    out.push(Listed::Invalid {
                        r#ref: full,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            match self.read_snapshot(&mut reference, id) {
                Ok(snapshot) => out.push(Listed::Valid(snapshot)),
                Err(err) => out.push(Listed::Invalid {
                    r#ref: full,
                    reason: err.to_string(),
                }),
            }
        }
        Ok(out)
    }

    /// Read and validate one snapshot ref, applying the git-level checks the
    /// document boundary cannot: the commit is parentless, its tree holds exactly
    /// `snapshot.json` (a regular blob) and, unless empty, `files/` (a tree), and
    /// every entry under `files/` is a regular file, an executable, or a symlink
    /// with no gitlink and nothing outside `files/`.
    fn read_snapshot(
        &self,
        reference: &mut gix::Reference<'_>,
        id: SnapshotId,
    ) -> Result<Snapshot, SnapshotError> {
        let commit = reference
            .peel_to_commit()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        if commit.parent_ids().next().is_some() {
            return Err(SnapshotError::HasParent);
        }
        let tree = commit
            .tree()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;

        let mut snapshot_json = None;
        let mut files_tree = None;
        for entry in tree.iter() {
            let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
            let name = entry.filename().to_str_lossy().into_owned();
            match name.as_str() {
                "snapshot.json" => {
                    if entry.mode().kind() != EntryKind::Blob {
                        return Err(SnapshotError::SnapshotJsonNotBlob);
                    }
                    snapshot_json = Some(entry.oid().to_owned());
                }
                "files" => {
                    if entry.mode().kind() != EntryKind::Tree {
                        return Err(SnapshotError::FilesNotTree);
                    }
                    files_tree = Some(entry.oid().to_owned());
                }
                _ => return Err(SnapshotError::ExtraTreeEntry(name)),
            }
        }

        let json_oid = snapshot_json.ok_or(SnapshotError::MissingSnapshotJson)?;
        let mut files = BTreeSet::new();
        if let Some(tree_oid) = files_tree {
            self.collect_files(tree_oid, "", &mut files)?;
        }
        let bytes = self
            .repo
            .find_object(json_oid)
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .data
            .clone();
        Snapshot::validate(id, &bytes, &files)
    }

    /// Collect the leaf paths under a `files/` tree, refusing a gitlink.
    fn collect_files(
        &self,
        tree_oid: gix::ObjectId,
        prefix: &str,
        files: &mut BTreeSet<String>,
    ) -> Result<(), SnapshotError> {
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        for entry in tree.iter() {
            let entry = entry.map_err(|err| SnapshotError::Git(err.to_string()))?;
            let name = entry.filename().to_str_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match entry.mode().kind() {
                EntryKind::Tree => self.collect_files(entry.oid().to_owned(), &path, files)?,
                EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                    files.insert(path);
                }
                EntryKind::Commit => return Err(SnapshotError::BadFileEntry(path)),
            }
        }
        Ok(())
    }

    /// Decide and, when the rules allow, capture the snapshot of a plain apply.
    /// A plain apply saves a snapshot when the target is inside a repository with
    /// a commit, was clean before the apply, and the apply changed a path;
    /// otherwise it saves nothing and names why. `was_clean` is the target's
    /// cleanliness taken before the apply wrote. The apply wrote into the real
    /// working tree at the project's target, so capture reads from the
    /// repository working-tree root and addresses each file by its repo-relative
    /// path (root and subpath targets alike), never by joining a target-relative
    /// path onto the target directory. The caller reports [`SkipReason::NotGit`]
    /// when there is no project and [`SkipReason::DryRun`] when nothing was
    /// applied.
    pub fn save_after_apply(
        &self,
        plan: &crate::plan::Plan,
        template: String,
        revision: crate::snapshot::Revision,
        generated: crate::snapshot::FrozenNow,
        submissions: Vec<indexmap::IndexMap<String, serde_json::Value>>,
        was_clean: bool,
    ) -> Result<SnapshotOutcome, SnapshotError> {
        use crate::snapshot::record::{CommitId, ProjectPoint, Timestamp};

        // The apply wrote into the working tree; capture reads from its root so a
        // repo-relative path (`<target>/<file>`) resolves once, not doubled onto
        // the target directory. Canonicalized to match `Project::open`'s `rel`.
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| SnapshotError::Git("repository has no working tree".into()))?
            .to_owned()
            .canonicalize()
            .map_err(|err| SnapshotError::Git(err.to_string()))?;

        // No commit: there is no HEAD point to record.
        let head = match self.repo.head_id() {
            Ok(head) => head.detach(),
            Err(_) => return Ok(SnapshotOutcome::Skipped(SkipReason::NoCommit)),
        };
        // A target that was dirty before the apply saves nothing.
        if !was_clean {
            return Ok(SnapshotOutcome::Skipped(SkipReason::Dirty));
        }

        let commit = CommitId::parse(&head.to_hex().to_string())
            .map_err(|err| SnapshotError::Git(err.to_string()))?;
        let branch = self
            .repo
            .head_name()
            .map_err(|err| SnapshotError::Git(err.to_string()))?
            .map(|name| name.shorten().to_string());
        let inputs = crate::snapshot::capture::CaptureInputs {
            id: SnapshotId::generate(),
            template,
            revision,
            generated,
            created: Timestamp::now(),
            project: ProjectPoint::new(commit, branch),
            submissions,
        };
        // Nothing changed: no captured path differs from HEAD, so save nothing.
        // The test is scoped to the paths the apply owns, not whole-target
        // cleanliness, so an unrelated edit never makes a snapshot and an ignored
        // output never hides a real one.
        let plan_paths = crate::snapshot::capture::group_plan(plan);
        match crate::snapshot::capture::capture_if_changed(
            &self.repo,
            &workdir,
            &self.rel,
            &plan_paths,
            inputs,
        )? {
            Some(id) => Ok(SnapshotOutcome::Saved(id)),
            None => Ok(SnapshotOutcome::Skipped(SkipReason::NothingChanged)),
        }
    }
}

/// What a plain apply did about its snapshot: saved one, or skipped it and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotOutcome {
    Saved(SnapshotId),
    Skipped(SkipReason),
}

/// Why a plain apply saved no snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The target is not inside a git repository.
    NotGit,
    /// The repository has no commit yet.
    NoCommit,
    /// The target had uncommitted changes before the apply.
    Dirty,
    /// The apply changed no path.
    NothingChanged,
    /// The apply was a dry run.
    DryRun,
}

impl SkipReason {
    /// The stable name of a skip reason for a result document.
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::NotGit => "not-git",
            SkipReason::NoCommit => "no-commit",
            SkipReason::Dirty => "dirty",
            SkipReason::NothingChanged => "nothing-changed",
            SkipReason::DryRun => "dry-run",
        }
    }
}

/// One entry from [`Project::snapshots`]: a valid snapshot, or a ref that could
/// not be read together with the reason.
///
/// A valid snapshot carries its whole validated document, so it is much larger
/// than the invalid variant; the size gap is intended, as the common list holds
/// valid entries.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Listed {
    Valid(Snapshot),
    Invalid { r#ref: String, reason: String },
}

/// A likely-base mark for one source: which snapshot, and whether it was chosen
/// by ancestry or by content. A mark never selects a base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LikelyBase {
    pub id: SnapshotId,
    pub source: String,
    pub by: LikelyBaseBy,
}

/// How a likely base was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LikelyBaseBy {
    Ancestry,
    Content,
}

/// The outcome of [`Project::remove`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub removed: Vec<SnapshotId>,
    pub not_found: Vec<SnapshotId>,
}

/// The outcome of [`Project::add_fetch`]: the remote, the refspec, and whether
/// it was newly added (`false` when it was already present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSetting {
    pub remote: String,
    pub refspec: String,
    pub added: bool,
}

/// Sort a listing newest first: valid snapshots by descending ULID (ULIDs sort
/// by creation time), invalids after valids and among themselves by ref name.
/// Shared by the target-scoped [`Project::snapshots`] and the repository-wide
/// [`Project::snapshots_all`] so both order identically.
fn sort_listed_newest_first(listed: &mut [Listed]) {
    listed.sort_by(|a, b| match (a, b) {
        (Listed::Valid(x), Listed::Valid(y)) => y.id().cmp(x.id()),
        (Listed::Valid(_), Listed::Invalid { .. }) => std::cmp::Ordering::Less,
        (Listed::Invalid { .. }, Listed::Valid(_)) => std::cmp::Ordering::Greater,
        (Listed::Invalid { r#ref: x, .. }, Listed::Invalid { r#ref: y, .. }) => x.cmp(y),
    });
}

/// Remove every named `filter.<name>` and `merge.<name>` driver subsection from
/// the repository's in-memory configuration.
///
/// Removing the whole named subsection (not merely the driver value) is what
/// makes gitoxide fall back to its built-in behaviour: a leftover empty command
/// would make a tree merge try to spawn it and fail. Top-level `merge.*`/
/// `filter.*` keys and every built-in conversion stay in place, and nothing on
/// disk changes.
pub(crate) fn strip_drivers(repo: &mut gix::Repository) -> Result<(), ProjectError> {
    let mut config = repo.config_snapshot_mut();
    for section in ["filter", "merge"] {
        let ids: Vec<_> = match config.sections_and_ids_by_name(section) {
            Some(sections) => sections
                .filter(|(section, _)| section.header().subsection_name().is_some())
                .map(|(_, id)| id)
                .collect(),
            None => Vec::new(),
        };
        for id in ids {
            config.remove_section_by_id(id);
        }
    }
    config
        .commit()
        .map_err(|err| ProjectError::Config(err.to_string()))?;
    Ok(())
}

/// The nearest ancestor of `path` (including `path` itself) that exists on disk.
/// A `CanonicalTarget` always has an existing ancestor (its canonicalization
/// walked up to one), so this terminates at or above the working tree, and
/// discovery walks up to the repository from there.
fn nearest_existing_ancestor(path: &std::path::Path) -> std::path::PathBuf {
    let mut current = path;
    loop {
        if current.exists() {
            return current.to_owned();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return current.to_owned(),
        }
    }
}

/// Whether a discovery error means there simply is no repository, as opposed to
/// a real fault opening one that exists.
fn is_not_a_repository(err: &gix::discover::upwards::Error) -> bool {
    use gix::discover::upwards::Error;
    matches!(
        err,
        Error::NoGitRepository { .. }
            | Error::NoGitRepositoryWithinFs { .. }
            | Error::NoGitRepositoryWithinCeiling { .. }
    )
}

/// A failure to open or inspect the repository. Every variant names a path or a
/// diagnostic and never an answer value or a file's content.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("cannot open the repository: {0}")]
    Open(String),
    #[error("the repository has no working tree")]
    Bare,
    #[error("the target is outside the repository working tree: {0}")]
    Outside(String),
    #[error("cannot read repository status: {0}")]
    Status(String),
    #[error("cannot adjust in-memory git configuration: {0}")]
    Config(String),
    #[error("invalid repository path: {0}")]
    Path(String),
    #[error("{0}")]
    Io(String),
    #[error("no remote named {0}")]
    RemoteMissing(String),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
}

#[cfg(test)]
mod tests;
