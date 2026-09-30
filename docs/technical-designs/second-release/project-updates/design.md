---
relationships:
  depends-on:
    - content-injection
    - headless-recovery
    - hook-results
  informs: project-updates
---

# Project updates from template changes

## Purpose

A project generated from a template can take that template's later changes
without losing the operator's edits. Every apply in a git repository saves a
**snapshot**: the exact output of that apply, stored as a git commit under a ref
that Toha owns. `toha apply TEMPLATE PATH --from SNAPSHOT` renders the template
again, saves the new output as a new snapshot, and merges old snapshot → new
snapshot into the operator's working tree the way `git merge` would. The
operator resolves any conflict and commits with their own git tools.

Toha does the git work in process through gitoxide. It starts no subprocess
except a trusted template's hooks. It never commits to the operator's branches,
never runs `git init`, and never pushes or fetches.

## Caller usage

### First apply saves a snapshot

```console
$ toha apply forge:catalog/receipt@stable ./sample-service
src/app.txt
README.md
snapshot 01J9Z4K7QX (receipt 3f9a1c2)
share it: git push origin refs/toha/snapshots/01J9Z4K7QX6M2V8R0T5B3N1P9D
```

The target must be inside a git repository, and the target directory must have
no uncommitted change before the apply. Otherwise the apply writes the files as
it always has and reports why it saved no snapshot:

```console
no snapshot: ./sample-service has uncommitted changes
```

### A teammate updates the project

```console
$ git clone https://git.example.test/team/sample-service && cd sample-service
$ toha init
fetch for Toha snapshots added: git config --add remote.origin.fetch +refs/toha/snapshots/*:refs/toha/snapshots/*
$ git fetch
$ toha snapshots list .
ID          TEMPLATE  VERSION  CREATED           PROJECT  BRANCH  BUILT FROM
01J9Z4K7QX  receipt   3f9a1c2  2026-03-14 09:26  a41c0de  main    -          likely base
$ toha apply forge:catalog/receipt@stable . --from 01J9Z4K7QX
receipt 3f9a1c2 -> 8be0d41
updated    src/app.txt
added      src/health.txt
conflicted README.md
snapshot 01JA2B8M4R (receipt 8be0d41, built from 01J9Z4K7QX)
share it: git push origin refs/toha/snapshots/01JA2B8M4R0C7W1Y5F3H9K2S6E
resolve the conflicts, then commit with git
$ git status --short
UU README.md
M  src/app.txt
A  src/health.txt
```

`README.md` holds diff3 conflict markers and git records it as conflicted, so
`git commit` refuses until the operator resolves it (as after `git merge`,
`git commit -a` does not check). Because the target had to be clean,
`git restore --source=HEAD --staged --worktree -- .` and `git clean -d --force
-- .` return it to where it was.

After the operator commits, the same command with `--from 01JA2B8M4R` reports
`already at receipt 8be0d41` and changes nothing.

### Person route without `--from`

When the target has snapshots and the person does not name one, Toha shows the
snapshot list for the target and asks which one to update from. The likely base
is preselected.

### Change answers

```console
$ toha apply forge:catalog/receipt@stable . --from 01JA2B8M4R --reanswer
? Label: (Sample) Receipts
? Currency code for totals: (EUR)
```

`--from` alone replays the recorded answers and asks only a question the new
version adds without a default or a recorded answer it rejects. `--reanswer`
asks every question, with the recorded answers as defaults. On the script route,
`--answers FILE` replaces the answers it names.

### Adopt a project that has no snapshot

```console
$ toha apply forge:catalog/receipt@3f9a1c2 . --baseline
conflicted README.md
unchanged  src/app.txt
added      docs/legacy.md
snapshot 01JA3C1D2E (receipt 3f9a1c2)
```

`--baseline` merges the render against an empty base. The version adopted is the
version applied. Applying the version that first generated the project gives the
fewest conflicts.

### Agent route

```console
$ toha stage forge:catalog/receipt@stable . --from 01JA2B8M4R --async
{ ...question batch for the questions the new version adds... }
$ toha continue . answers.json
$ toha apply .
```

The staged interview records the base. The final `apply PATH` does the render,
the snapshot, and the merge.

### Script route

```console
$ toha apply forge:catalog/receipt@stable . --from 01JA2B8M4R --answers answers.json
{ "protocol": 1, "status": "applied", ..., "snapshot": { "id": "01JA..." },
  "merge": { "from": "01JA2B8M4R...", "changes": [...], "conflicted": ["README.md"] } }
```

### Snapshot housekeeping

```sh
toha snapshots list . --json
toha snapshots clean .                         # select in a list
toha snapshots clean . --keep 1                # preselect all but the newest
toha snapshots clean . --remove remove.json    # remove exactly these
```

After a removal Toha prints one `git push origin --delete <ref>` line per
removed snapshot.

## Snapshots

### What a snapshot holds

A snapshot is a git commit with **no parent**, referenced by
`refs/toha/snapshots/<id>`. Its tree has two entries:

```text
snapshot.json     metadata (below)
files/            the output, with paths relative to the target directory
```

The merge reads only `files/`, so the metadata never becomes a project file.

`<id>` is a ULID: 26 Crockford base32 characters, unique across clones,
ordered by creation time, and never changed. Commands accept any unique
prefix of at least 6 characters.

`snapshot.json`:

```json
{
  "snapshot": 1,
  "id": "01JA2B8M4R0C7W1Y5F3H9K2S6E",
  "template": "forge:catalog/receipt@stable",
  "source": "forge:catalog/receipt",
  "commit": "8be0d41c2f…",
  "target": "services/sample-service",
  "created": "2026-09-30T04:12:00Z",
  "generated": "2026-03-14T09:26:53+00:00[UTC]",
  "project": { "commit": "a41c0de…", "branch": "main" },
  "built_from": "01J9Z4K7QX6M2V8R0T5B3N1P9D",
  "submissions": [ { "label": "Sample", "quantity": 4 }, { "use_vcs": true } ],
  "paths": {
    "src/app.txt":       { "origin": "toha" },
    "package.json":      { "origin": "edit", "regions": [], "values": ["scripts.build"] },
    "config/app.toml":   { "origin": "edit", "regions": ["features"], "values": [] },
    "package-lock.json": { "origin": "hook" }
  }
}
```

| Member | Meaning |
|---|---|
| `snapshot` | Format version of this document. |
| `id` | The ULID. Equal to the ref's last segment. |
| `template` | The formal name as applied, including any `@reference`. |
| `source` | The source identity: the formal name without its `@reference`. Two snapshots belong to the same template when their `source` values are equal. |
| `commit` | The template's resolved 40-hex commit, or `null` for a folder template. |
| `target` | The target directory relative to the repository root, `.` for the root. |
| `created` | When the snapshot was saved. |
| `generated` | The frozen `now()` every render of this project uses, so a template that renders a date stays byte-stable across updates. Set at the first snapshot and carried forward. |
| `project` | `HEAD` commit and branch name when the snapshot was saved. `branch` is `null` on a detached `HEAD`. |
| `built_from` | The base snapshot's id for `--from`, `null` otherwise. |
| `submissions` | The accepted raw submissions in order, before `format`, as the staged record holds them. |
| `paths` | Every path under `files/` and its origin: `toha` for a file Toha owns whole; `edit` for a file Toha edits inside, with the region keys and JSON paths it owns there; `hook` for a file a hook created or changed. |

The schema is `docs/specifications/snapshot.schema.yml`. It denies unknown
members. Toha reads a snapshot at one boundary and refuses the whole snapshot
when any check fails:

- the ref is `refs/toha/snapshots/<id>` and points at a commit with no parent;
- the commit's tree has exactly `snapshot.json` (a regular blob) and, unless
  the snapshot is empty, `files/` (a tree);
- every entry under `files/` is a regular file, an executable file, or a
  symbolic link; no gitlink and no path outside `files/`;
- `snapshot.json` is valid against the schema; `id` equals the ref; identifier,
  commit, instant, region-key, and JSON-path syntax are valid; `target` and
  every path are relative, contain no `..`, and are not inside `.git`;
- the keys of `paths` equal the set of files under `files/`.

A snapshot fetched from a teammate is therefore data that can be refused, never
instructions.

### When a snapshot is saved

A plain apply saves a snapshot when all of these hold:

1. The target is inside a git repository with at least one commit.
2. Before the apply, the target directory has no tracked modification, no
   staged change, and no untracked file that git does not ignore.
3. The apply changed at least one path.

Otherwise the apply behaves exactly as it does without this feature and reports
the reason. `--from` and `--baseline` always save the new snapshot unless the
result is already current. A dry run saves nothing.

### What goes into `files/`

A path is captured when it is under the target and one of these holds:

- **Plan target:** it is a target of any mutation in the plan
  (`Plan::mutations()`): origin `toha` for a whole file, `edit` for a region or
  JSON value, with the keys and paths the plan owns there. This includes a
  mutation whose bytes did not change.
- **Hook change:** it differs from `HEAD` after the hooks ran and is not a plan
  target: origin `hook`.
- **Carry-forward (`--from` only):** it has origin `hook` in the base snapshot,
  and the new apply neither made it a plan target nor changed it. It keeps the
  base snapshot's content and origin, because a hook that rewrites a file
  unchanged cannot be told apart from a hook that no longer produces it.
- **Retraction (`--from` only):** it has origin `edit` in the base snapshot and
  owns a region or JSON value that the new plan neither produces nor covers. A
  new whole-file mutation of the path covers all of its old ownership; a new
  JSON value covers the old values at or below its JSON path; a new region
  covers the old region with the same key. Toha retracts only the uncovered
  ownership (a region loses its marker lines and span; a JSON value loses its
  key, array elements from the highest index down; with the same strict
  `.json` check the JSON resolver applies) and captures the file with origin
  `edit` and the ownership that remains, which may be none.

A path with origin `toha` in the base snapshot that the new plan does not
produce is not captured, so the merge sees the template removing it.

A path with origin `edit` that owns nothing in the base snapshot is
**released**: it is not captured, and the merge leaves it out of the base tree
as well (see The merge), so the file stays with the operator exactly as it is.
A file with origin `edit` is never removed by an update.

A path is not captured when it is untracked in `HEAD` and git ignores it, when
it is a gitlink or lies inside a submodule, or when it is outside the target
(a hook change outside the target is reported as a warning). A tracked file
stays capturable even if an ignore rule now matches it.

Ignore rules are the ones git applies in the project: the `.gitignore` files
of `HEAD`, the repository's `info/exclude`, and the user's global excludes.

Capture reads symbolic links without following them and records them as links.
It records the executable bit. It converts file content to its stored form with
git's built-in conversions only (line endings, `working-tree-encoding`), as
`git add` would, and never runs a filter driver.

## Applying from a snapshot

### Preconditions

`apply TEMPLATE PATH --from SNAPSHOT` refuses, writing nothing, when:

- PATH is not inside a git repository;
- the target directory has uncommitted changes;
- the snapshot id is unknown or ambiguous, or the snapshot is invalid;
- the snapshot's `source` differs from TEMPLATE's source identity;
- the snapshot's `target` differs from PATH relative to the repository root;
- an interview is staged at PATH for another operation (as today).

`--from` with `--baseline`, and either of them with `--force`, is a usage
error (exit 2).

### Answers

An update does not reuse the staged-replay walk as it is, because that walk
treats a rejected recorded answer as fatal, submits recorded batches whole, and
has no way to override an answer. A crate-private update replay adapter drives
the unchanged interview engine instead. Its outcome is one typed value:
completed (with the accepted raw submissions), ask (the pending batch, its
defaults, the rejections, and the adapter to continue), ended, or an engine
fault.

1. It turns the recorded submissions into a queue of values per question id,
   in recorded order, so a question asked more than once (in a loop) gets its
   values in order.
2. It applies the route's overrides first: `--answers FILE` values (verified
   against the template being applied through `parse_and_verify`) replace the
   queued value for their id. Values for ids the new version does not ask are
   dropped.
3. For each pending batch, it takes the next queued value for every question
   in the batch. It submits the batch when every question that needs an answer
   has one. A recorded value the new version rejects is removed and the
   question is treated as unanswered, with the rejection kept for the message.
4. At the first batch it cannot complete, it hands the batch to the route:
   the person is prompted with the recorded values as defaults; the script
   route returns `questions` (exit 4); the agent route stages the interview
   with the base and returns the batch.

`--reanswer` skips step 3: every batch goes to the route, with the recorded
raw values as defaults.

The new snapshot records the raw values actually submitted.

On the agent route, the staged interview holds `base`, `reanswer`, and, as
today, the raw submissions accepted so far. Resuming rebuilds the adapter from
the base snapshot, replays the staged submissions (which this template already
accepted) through the existing walk, removes from each id's queue as many
values as that id has been answered, and continues at the pending batch. No
recorded value is lost or used twice.

### Already current

After the render, when the new snapshot would have the same `commit`, the same
submissions, and the same `files/` tree as the base snapshot, the apply reports
`already at <short name> <commit>`, discards the new snapshot, and writes
nothing. For a folder template, whose `commit` is `null`, the `files/` tree
decides, so an edited folder template is never mistaken for an unchanged one.

### Dry run

`--dry-run` runs the same candidate-tree builder as a real update on in-memory
images of the target's files at `HEAD`: the same ownership transitions,
retractions, injection resolution, capture, carry-forward, and release rules,
with no throwaway checkout and no hooks. It then runs the same merge in memory
and reports the changes. A hook-origin path is carried forward unchanged, and
the report says `not previewed: hook output`. For every path no hook touches,
the preview equals what the real update writes. It writes nothing to the
project and saves no snapshot.

### The throwaway checkout

1. Check out the whole tree of `HEAD` into a new temporary directory outside the
   repository, with no filter driver, so a file stored through a filter such as
   git-lfs appears in its stored form.
2. Build the plan for the project's real target and with the snapshot's
   `generated` instant, so everything the template renders (paths, the
   invocation context, dates) is what an apply in the project would render.
3. Compute the ownership transitions from the base snapshot's `paths` and
   `Plan::mutations()`, and apply the uncovered retractions to the checkout
   first.
4. Apply the plan with the throwaway checkout as the destination, through a
   crate-private destination seam in `apply.rs`: whole files replace what the
   checkout holds, and regions and JSON values are written as the template
   intends, as with `--force`. The checkout is a copy, so nothing the operator
   owns is at risk.
5. Run the new version's hooks there, in the existing hook loop, under the
   existing trust gate.
6. Capture the new snapshot by the rules above and save it under its ref.
7. Delete the checkout.

Steps 3, 4, and 6 are one candidate-tree builder that works on a directory or
on in-memory images; the dry run uses the same builder without step 5.

A failure in steps 1–6 ends the apply. The project is untouched, and no ref is
saved.

### The merge

Toha merges trees in process with gitoxide's tree merge. The three trees are
whole-repository trees, so conflict paths are repository-relative and files
outside the target cannot change:

- base = the `HEAD` tree with the target directory replaced by `files/` of the
  `--from` snapshot, without its released paths (an empty directory for
  `--baseline`);
- ours = the `HEAD` tree (the operator side);
- theirs = the `HEAD` tree with the target directory replaced by `files/` of the
  new snapshot (the template side).

When the target is the repository root, the whole tree is replaced; paths that
no snapshot names are then present only on the operator side, as below.

The operator is "ours" and the template is "theirs", as in `git merge` when the
operator merges the template in: index stage 2 is the operator's version and
`git checkout --ours` keeps it.

Each path merges by git's rules: changed on one side takes that side; changed on
both sides merges by line, and only overlapping changes conflict; removed by the
template and unedited is deleted; removed by the template and edited is a
modify/delete conflict; added by the template where the operator has nothing is
added; added by both with different content is an add/add conflict; a file on
one side where the other side has a directory is a file/directory conflict;
binary content changed on both sides is a conflict. Rename detection is off.
Operator files under the target that no snapshot names are present only on the
operator side, so the merge keeps them unchanged.

Owned regions and JSON values follow the same line rules. The new snapshot holds
the operator's file with Toha's new region body or value, so the merge applies
Toha's change and keeps operator edits around it. When the operator and the
template both changed lines inside an owned region or value, overlapping lines
conflict and separate lines both apply.

Toha never runs a merge driver or a filter driver (see Repository access). A
path whose git attributes name a merge driver or a `filter` is not merged; it
keeps the operator's content and is reported as a conflict of kind `driver`
with its index stages.

### Writing the result

Toha writes the result into the target directory and the index, as `git merge`
does:

- clean paths get their merged content and are staged;
- a text conflict gets diff3 markers labelled `operator`, `toha <old short
  commit>`, and `toha <new short commit>`, and index stages 1, 2, and 3;
- a modify/delete, add/add, file/directory, binary, or driver conflict keeps
  the operator's file on disk and gets the matching index stages;
- index entries outside the target are kept, including anything the operator
  had staged elsewhere; the index's cached tree is dropped so git recomputes
  it.

Git then lists every conflict as unmerged: `git status` shows it and plain
`git commit` refuses until it is resolved. As after any `git merge`, `git
commit -a` records the files as they are. Toha does not create `MERGE_HEAD` or
any merge state, and it never commits.

Toha refuses, writing nothing, when a path the merge would add or replace is
occupied on disk by an untracked file or directory, ignored or not, as `git
merge` does; the message names the paths. The check is repeated under the
index lock in step 6.

### Order of effects

```text
0  refuse: preconditions above; record HEAD and the index checksum
1  resolve the template, answers, and plan (questions -> ask or report, exit 4)
2  trust gate for the new version's hooks (untrusted -> planned, exit 3)
3  throwaway checkout, apply, hooks, retractions, capture; save the new ref
4  already current -> report, delete the new ref, stop
5  merge in memory; refuse on occupied paths
6  take the index lock; re-check HEAD, the index checksum, and that the target
   is still clean; any drift -> release the lock, delete the new ref, refuse
7  write merged files, each by temporary file and rename, each only after
   checking that the path on disk still holds its HEAD content (or is still
   absent); then the index through the lock
8  print the change list, the snapshot, and the share command
```

The check in step 7 narrows, but cannot close, the window in which another
program writes a target file; `git merge` has the same window. A path that
changed since step 6 stops the write and starts the rollback.

If step 7 fails, Toha restores every path it wrote or deleted from `HEAD` (the
target was clean, so `HEAD` holds the operator's content) and removes files it
created, in each case only when the path still holds what Toha wrote; a path
changed by another program is left alone and named. It releases the index lock without writing, and deletes the new ref. If
that rollback also fails, the error names the paths and the target-scoped
recovery: `git restore --source=HEAD --staged --worktree -- <target>` and
`git clean -d --force -- <target>`, which touch nothing outside the target and
no ignored file.

After a successful merge, the operator can abandon it with the same two
commands.

## Baseline

`apply TEMPLATE PATH --baseline` adopts a project that has files but no
snapshot. It follows "Applying from a snapshot" with an empty base and the
interview run normally (no recorded answers). Paths the render and the project
both hold with different content become add/add conflicts; identical paths are
unchanged; paths only the render has are added. The new snapshot has
`built_from: null` and a new `generated` instant.

Plain `apply` into existing files keeps its behaviour: refuse, or overwrite with
`--force`, and save a snapshot under the usual rule.

## Repository access

Toha opens the target's repository once per command through gitoxide. Before
any status, checkout, conversion, or merge, it removes from the in-memory
configuration every `filter.<name>.clean`, `filter.<name>.smudge`,
`filter.<name>.process`, and `merge.<name>.driver` value, so no configured
driver program can start. It changes nothing on disk. Built-in conversions
(line endings, `working-tree-encoding`) still apply. A path whose attributes
name a filter is therefore compared and written in its stored form; the clean
check reports it as changed when its working-tree bytes differ from that form,
which refuses rather than guesses.

## Routes

| Caller | Command | Base |
|---|---|---|
| Person | `apply TEMPLATE PATH [--from ID \| --baseline] [--reanswer]` | Prompts for the snapshot when `--from` is absent and the target has snapshots |
| Script | `apply TEMPLATE PATH (--from ID \| --baseline) --answers FILE` | Must be named |
| Agent | `stage TEMPLATE PATH (--from ID \| --baseline) [--reanswer] --async`, `continue`, `apply PATH` | Recorded in the staged interview |

The staged interview gains one member, `base`: absent, `{"snapshot": "<id>"}`,
or `"empty"`, plus `reanswer: true` when given. The clean-target check runs at
`stage` and again at the final `apply PATH`, which is authoritative.

## Snapshot commands

### `toha snapshots list [PATH] [--json]`

Lists the snapshots whose `target` is PATH (default: the current directory),
newest first, with id, template short name, template version, created, project
commit, branch, and built-from. It marks one **likely base** per source: the
newest snapshot whose project commit is an ancestor of `HEAD`. When none is an
ancestor, it marks the snapshot whose `files/` match the most paths in the
target at `HEAD`, labelled `likely base (by content)`. A mark never selects a
base. `--json` writes one document with the same fields and marks. An invalid
snapshot is listed as invalid with the reason.

### `toha snapshots clean [PATH] [--keep N] [--remove FILE] [--force]`

- Person route: a multi-select list of the target's snapshots. `--keep N`
  preselects all but the newest N per source. Nothing is removed until the
  person confirms.
- Script and agent route: `--remove FILE` (`-` reads standard input) names the
  exact snapshots: `{ "remove": ["<id>", ...] }`. It acts immediately and writes
  one JSON result: `{ "removed": [...], "not_found": [...] }`. A request that
  would remove every snapshot of a source is refused as a whole, and removes
  nothing, unless `--force` is given.
- Removal deletes the refs. Toha prints one `git push origin --delete <ref>`
  per removed snapshot. It never uses `--prune`.

### `toha init [PATH]`

Adds `+refs/toha/snapshots/*:refs/toha/snapshots/*` to the fetch setting of
the remote `origin` in the repository's local git config, unless it is present,
and prints what it added. `--remote NAME` selects another remote. It changes
nothing else, writes no push setting, and never fetches.

## Public interfaces

```rust
// toha::snapshot — new public module
pub struct SnapshotId(Ulid);                       // 26 chars; Display, FromStr (full id)
pub struct Snapshot { /* private: validated snapshot.json + ref */ }
impl Snapshot {
    pub fn id(&self) -> &SnapshotId;
    pub fn template(&self) -> &str;                // formal name as applied
    pub fn source(&self) -> &str;                  // formal name without @reference
    pub fn revision(&self) -> &Revision;           // Commit(CommitId) | Unversioned
    pub fn target(&self) -> &RepoPath;
    pub fn created(&self) -> Timestamp;
    pub fn generated(&self) -> &FrozenNow;
    pub fn project(&self) -> &ProjectPoint;        // commit, Option<branch>
    pub fn built_from(&self) -> Option<&SnapshotId>;
    pub fn submissions(&self) -> &[IndexMap<Id, serde_json::Value>];
}

pub struct Project { /* private: repository, canonical target, repo-relative path */ }
impl Project {
    pub fn open(target: &CanonicalTarget) -> Result<Option<Project>, ProjectError>; // None: not in git
    pub fn cleanliness(&self) -> Result<Cleanliness, ProjectError>;
    pub fn snapshots(&self) -> Result<Vec<Listed>, ProjectError>;   // Valid(Snapshot) | Invalid{ref, reason}
    pub fn find(&self, prefix: &str) -> Result<Snapshot, SnapshotError>;
    pub fn likely_bases(&self, snapshots: &[Snapshot]) -> Result<Vec<LikelyBase>, ProjectError>;
    pub fn remove(&self, ids: &[SnapshotId], force: bool) -> Result<Removed, SnapshotError>;
    pub fn add_fetch(&self, remote: &str) -> Result<FetchSetting, ProjectError>; // toha init
}
pub enum Cleanliness { Clean, Dirty { paths: Vec<RepoPath> } }

// Plain apply: Plan::apply, then capture when the project allows it.
pub fn apply(plan: Plan, inputs: SnapshotInputs, project: Option<&Project>,
             options: ApplyOptions, runner: &dyn HookRunner)
    -> Result<Applied, ApplyError>;               // Applied gains `snapshot: SnapshotOutcome`

// --from and --baseline, after the interview completed (the CLI uses the
// crate-private update replay adapter; a crate caller drives the interview).
pub fn merge_apply(project: &Project, base: Base, template: &Template,
                   completed: &Completed, inputs: SnapshotInputs,
                   options: MergeOptions, runner: &dyn HookRunner)
    -> Result<Merged, MergeError>;
pub enum Base { Snapshot(Snapshot), Empty }
pub struct MergeOptions { pub trusted: bool, pub dry_run: bool }
pub enum Merged {
    AlreadyCurrent,
    NeedsTrust,
    Planned { changes: Vec<Change> },              // dry run
    Written { snapshot: SnapshotId, changes: Vec<Change>, conflicted: Vec<RepoPath> },
}
pub struct Change { pub path: RepoPath, pub action: Action }
pub enum Action { Added, Updated, Merged, Deleted, Conflicted(ConflictKind), NotPreviewed }
pub enum ConflictKind { Content, AddAdd, ModifyDelete, FileDirectory, Binary, Driver }

pub struct SnapshotInputs {                        // what the snapshot records
    pub template: String, pub revision: Revision, pub generated: FrozenNow,
    pub submissions: Vec<IndexMap<Id, serde_json::Value>>,
}
pub enum SnapshotOutcome { Saved(SnapshotId), Skipped(SkipReason) }
pub enum SkipReason { NotGit, NoCommit, Dirty, NothingChanged, DryRun }
```

`Plan::build`, `Plan::apply`, `Plan::mutations`, the injection resolvers, the
interview engine, and the answers-document operations keep their public
signatures. Crate-private additions: the update replay adapter; a destination seam in
`apply.rs` that applies a plan built for the real target into another
directory or in-memory images; and retraction helpers in the injection module
that reuse its resolvers.
`StagedRecord` gains the optional `base` and `reanswer` members. No gitoxide
type appears in a public signature.

## Module and seam

```text
CLI  apply (--from, --baseline, --reanswer) · stage · snapshots list|clean · init
  │  canonical_target · resolve · staged record (base)
  ▼
snapshot/            new public module
  project.rs   open repo (drivers removed) · clean check · refs · likely base · init
  record.rs    snapshot boundary (commit, tree, snapshot.json) · ULID
  replay.rs    update replay adapter (crate-private) over the interview engine
  capture.rs   plan targets, hook changes, carry-forward, retraction → tree → commit → ref
  merge.rs     throwaway checkout · tree merge · occupied-path check · locked write · rollback
  │ consumes, unchanged                     │ reuses crate-internally
  ▼                                         ▼
plan.rs  Plan::build · Plan::mutations       apply.rs  destination seam, hook loop
interview.rs · protocol.rs parse_and_verify  inject    retraction helpers
                                             hook.rs, review.rs trust
```

Only `snapshot/` names gitoxide's repository, status, merge, and index APIs.
`source.rs` keeps its own template fetching.

## Errors and results

`ProjectError`, `SnapshotError`, and `MergeError` name the path or snapshot id
they concern. None of them, and no `Debug` or `Display` output, contains answer
values or file contents.

| Condition | Script result | Exit |
|---|---|---|
| Not in a git repository, for `--from` or `--baseline` | `error` `git` | 1 |
| Target has uncommitted changes | `error` `dirty` with the paths | 1 |
| Unknown, ambiguous, or invalid snapshot | `error` `snapshot` | 1 |
| Snapshot for another source or target | `error` `snapshot` naming both | 1 |
| Questions remain | `questions` | 4 |
| Untrusted hooks | `planned`, `trusted: false` | 3 |
| Render, hook, retraction, or checkout failure | `error` of that kind; project unchanged | 1 |
| Occupied path | `error` `occupied` with the paths; project unchanged | 1 |
| HEAD, index, or target changed during the run | `error` `changed`; project unchanged | 1 |
| Already current | `applied` with empty `merge.changes` | 0 |
| Merged cleanly | `applied` with `merge` | 0 |
| Merged with conflicts | `applied` with non-empty `merge.conflicted` | 1 |
| Write failure | `error` `write`; rolled back, or the target-scoped recovery | 1 |

Every refusal names the command that does what the caller meant: a dirty target
names committing or stashing and the same command again; an unknown snapshot
names `toha snapshots list`; a target with files and no snapshot names
`--baseline`; a snapshot for another template or target names `toha snapshots
list PATH`.

A plain apply's `applied` result gains `snapshot`: `{ "id": ... }` or
`{ "skipped": "<reason>" }`.

## Behaviors to prove

1. **Acceptance scenario.** Apply version A in a clean repo; commit; edit
   `README.md` on lines B changes; in a second clone with `toha init` and a
   fetch, `snapshots list` marks the snapshot as likely base; `apply …@B --from`
   updates `src/app.txt`, adds `src/health.txt`, leaves `README.md` with diff3
   markers and index stages 1–3 where stage 2 holds the operator's bytes
   (`git status` shows `UU`, plain `git commit` fails), and reports the
   conflict and a new snapshot. After resolving and committing, the commit's
   tree equals the working tree, and the same command with the new snapshot
   reports already current and changes no byte.
2. A clean three-way merge keeps the operator's lines and the template's lines.
3. A file the template removed is deleted when unedited and is a modify/delete
   conflict, with the operator's bytes on disk, when edited.
4. Operator files no snapshot names are unchanged by `--from` and `--baseline`,
   including with the target at the repository root.
5. An apply in a dirty target writes files as today, saves no snapshot, and
   reports `dirty`; `--from` in a dirty target refuses with nothing written.
6. A plain apply outside git, one that changes nothing, and a dry run save no
   snapshot.
7. The snapshot holds a file a hook creates, with origin `hook`, and leaves out
   an untracked ignored file.
8. A hook file rewritten unchanged by the new version is carried forward and
   not deleted from the project.
9. A template file rendered identical to `HEAD` stays in the snapshot, and the
   next update neither deletes nor reports it.
10. A region: the new body reaches the project with operator lines around it
    kept; separate-line edits inside the region from both sides both apply;
    overlapping edits conflict.
11. A JSON value changed only by the template is updated; a value both sides
    changed on the same line conflicts; unrelated keys are unchanged; an
    unchanged injection stays captured across an update that changes another
    file.
12. An injection the new version drops is retracted: the region's markers and
    span, or the JSON key, are removed; the rest of the operator's file stays;
    a relaxed `.json` target refuses the update. Across A→B→C, where B drops
    the file's last injection and C has none, the operator's file is never
    deleted. Ownership transitions keep new output: an owned value replaced by
    ownership of its parent object, a region replaced by whole-file ownership,
    and removal of two array elements.
13. Replay: raw submissions (non-idempotent `format` fixture) replay; answers
    for removed questions are dropped; an added required question in an early
    batch is asked with the other batch values as defaults (person) or
    reported (exit 4); a recorded value the new version rejects is asked with
    its rejection; a looped question replays its values in order.
14. `--answers FILE` replaces the named recorded answers and fails identity for
    another template before any evaluation; `--reanswer` offers every question
    with the recorded raw default; a changed answer at the same version merges
    the difference.
15. Same commit, answers, and files report already current and leave no new
    ref; an edited folder template with the same answers is not already
    current.
16. A date-rendering template is byte-stable across `--from` because of
    `generated`.
17. `--from …@B` with a snapshot of `…@A` of the same source proceeds; a
    snapshot of another source or another target refuses; `--from` with
    `--baseline` is a usage error.
18. `--baseline` produces add/add conflicts for differing files, keeps
    identical ones, adds the rest, and records `built_from: null`.
19. The agent route: `stage --from` records the base, `continue` answers the
    added questions, and `apply PATH` merges; a pause at an added early
    question resumes with every later recorded answer used exactly once; a
    target made dirty after `stage` refuses at `apply PATH`.
20. Untrusted hooks return `planned` with `trusted: false`, exit 3, and write
    nothing to the project; a dry run calls the hook runner zero times, and
    for every path no hook touches (including an injection-only removal) its
    report equals what the real update writes.
21. A hook failure in the throwaway checkout leaves the project unchanged and
    no new ref.
22. An untracked or ignored file occupying a path the merge adds refuses with
    nothing written; a tracked directory where the template adds a file is a
    file/directory conflict; a binary file changed on both sides keeps the
    operator's bytes on disk with stages 1–3.
23. A change to a target file, to `HEAD`, or to the index between the start and
    the write refuses with nothing written and no new ref.
24. A write failure injected after each file write rolls back to `HEAD`,
    removes created files, leaves the index unwritten, and leaves staged work
    outside the target intact.
25. Capture records a symbolic link as a link (a link to a file outside the
    repository copies nothing) and keeps the executable bit; a CRLF
    (`text=auto`) fixture captures the stored form.
26. `snapshots list --json` marks the likely base by ancestry, and by content
    after the operator's history was squashed; an invalid snapshot is listed
    as invalid.
27. `snapshots clean --remove` removes exactly the named snapshots, reports
    unknown ids, refuses a request that removes every snapshot of a source
    unless `--force`, and prints one delete command per ref; afterwards no ref
    reaches the removed snapshot commit.
28. `toha init` adds the fetch setting once, is idempotent, and writes no push
    setting.
29. No process starts during apply, `--from`, list, clean, or init except the
    template's trusted hooks, in a repository whose config defines a merge
    driver, a filter `process`, and `clean`/`smudge` commands, with matching
    attributes on paths inside and outside the target; the attributed snapshot
    path is reported as a `driver` conflict with the operator's content kept.
30. A forged snapshot (unknown member, path with `..`, `paths` not equal to
    `files/`, id not equal to the ref, a parent, a gitlink or extra tree entry)
    is refused before any render.

## Sole-kill guards

- Merge with the operator's tree as base instead of the snapshot: fails 2.
- Capture only paths whose bytes changed instead of every plan target: fails 9
  and 11.
- Drop hook paths the new version did not touch instead of carrying them
  forward: fails 8.
- Let a dropped injection omit its file instead of retracting inside it: fails
  12.
- Keep a released path in the merge base: the A→B→C file is deleted; fails 12.
- Retract after applying the new plan, or without coverage: the parent-object
  transition loses its child; fails 12.
- Build the dry-run tree with separate code: the injection-only removal
  previews as a deletion; fails 20.
- Rebuild the resumed queues without removing consumed values: a later answer
  is used twice; fails 19.
- Compare the formal name with its reference instead of the source identity:
  fails 17.
- Decide already current by commit and answers without the `files/` tree: fails
  15.
- Treat a rejected recorded value as fatal, or submit recorded batches whole:
  fails 13.
- Apply overrides after replay: fails 14.
- Write only markers without index stages: plain `git commit` succeeds; fails 1.
- Swap ours and theirs: stage 2 holds the template's bytes; fails 1 and 3.
- Keep the index's cached tree after replacing entries: the committed tree
  differs from the working tree; fails 1.
- Skip the re-check before writing: fails 23.
- Skip rollback: fails 24.
- Follow symbolic links during capture: fails 25.
- Leave configured drivers in the in-memory config: the process guard of 29
  fails.
- Skip the clean check at the final agent `apply PATH`: fails 19.
- Use the current instant instead of `generated`: fails 16.
- Run hooks during a dry run: fails 20.
- Give snapshots a parent: fails 30 (and 27's reachability check).
- Treat a whole-source removal request as partial: fails 27.

## Out of scope

- Rename detection; a moved path is a delete plus an add.
- Moving a target directory; `--from` refuses a snapshot for another path.
- Version ordering; Toha shows versions and never labels an upgrade or a
  downgrade.
- Pushing, fetching, or pruning refs; committing to operator branches;
  `git init`.
- Onboarding in `toha init` (backlog task 1108).
- Side effects of hooks that are not committed files.
- Changes to injection's apply-time ownership semantics, the answers-document
  contract, the hook-result lifetime, or trust.
- Honouring git merge drivers and filter drivers (see Repository access).
- New permissions, timeouts, pinned-version checks, or application
  subprocesses.

## Compatibility and canonical documents

Toha is before 1.0.0 and has no dependent users. Plain `apply` keeps its
behaviour and gains a snapshot in a clean git target. The implementation
updates, in the same change:

- `docs/concepts/toha.yml`: the update-projects feature and snapshots.
- `docs/specifications/command-line-interface.yml` and `.spec.yml`: `--from`,
  `--baseline`, `--reanswer`, `snapshots list|clean`, `init`, results, exits.
- A new `docs/specifications/snapshot.yml` and `snapshot.schema.yml`.
- `docs/specifications/interview-protocol.yml` and schema: the `snapshot` and
  `merge` members of `applied`, the clean result document, the staged `base`.
- `docs/technical-designs/architecture.yml`: the `snapshot` module and the
  added gitoxide features.
- The repository glossary (`AGENTS.md`): **Snapshot** (the stored output of one
  apply, under a Toha-owned git ref), **Base snapshot** (the snapshot an update
  merges from), and **Baseline** (an apply that adopts an existing project
  against an empty base).
- A user guide page on updating projects, and a generic example.
- Fixture harness `tests/fixtures/update-*`: `template-a/`, `template-b/`,
  `answers.json`, an `edits/` overlay with a removal list, `expected/`
  (worktree and `git ls-files -s` stages), and `expect.yml`.

## Decisions

The approved product decisions and the decisions still needed are in
`design.yml`.
