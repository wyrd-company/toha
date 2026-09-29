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

A project generated from a template can follow that template's later changes.
Every apply writes an **applied record** into the project. `toha update PATH`
renders the template again at a newer commit with the recorded answers, and
merges the result with the operator's edits. The base of the merge is what Toha
wrote last, "theirs" is the operator's file on disk, and "ours" is what Toha
would write now. A project without a record is adopted once with `toha adopt`,
which asks the interview again and writes only the record.

The merge is git's three-way text merge, run in process by gitoxide. Toha starts
no subprocess.

## Caller usage

### Generate, then update

```console
$ toha apply forge:catalog/receipt@stable ./sample-service
src/app.txt
README.md
$ ls ./sample-service/.toha/applied
receipt-5e1d0c7a.json
```

The operator commits `.toha/` with the project. Later the template changes:

```console
$ toha update ./sample-service --dry-run
receipt: 3f9a1c2 -> 8be0d41
update   src/app.txt        (whole)
merge    README.md          (whole)
update   config/app.toml    (region features)
converge package.json       (value scripts.build)
add      src/health.txt     (whole)
retract  docs/legacy.md     (whole)
release  notes/todo.md      (whole)
conflict src/routes.txt     (whole, 2 hunks)
$ toha update ./sample-service
conflicting changes, nothing written:
  src/routes.txt (whole, 2 hunks)
to write git-style conflict markers: toha update --on-conflict markers ./sample-service
$ toha update --on-conflict markers ./sample-service
...
$ toha update ./sample-service        # after the operator resolves the markers
```

### Person route

```console
$ toha update ./sample-service
receipt: 3f9a1c2 -> 8be0d41
? Currency code for totals: EUR
merge    README.md          (whole)
add      src/health.txt     (whole)
```

Recorded answers are replayed. The person is asked only for a question that the
new template adds without a default, or whose recorded answer the new template
rejects. A terminal on standard input is needed only while such a question
remains.

### Script and agent route

```console
$ toha update --answers answers.json ./sample-service
{ "protocol": 1, "status": "updated", "context": { ... },
  "changes": [ { "path": "README.md", "owner": "whole", "action": "merge" } ],
  "conflicted": [] }
```

`answers.json` is the approved answers document, `{ "template": "<formal
name>", "answers": { ... } }`. Its answers extend or replace the recorded answers
for this run. `{ "template": "<formal name>", "answers": {} }` updates with the
recorded answers only. The route writes one JSON result document for every
outcome and never prompts or stages.

### Adopt a project that has no record

```console
$ toha adopt forge:catalog/receipt@stable ./sample-service --at 3f9a1c2
? Label: Sample
matches  src/app.txt
differs  README.md
absent   docs/legacy.md
recorded ./sample-service/.toha/applied/receipt-5e1d0c7a.json
```

### Crate

```rust
let target = canonical_target(Path::new("./sample-service"))?;
let record = ProjectRecord::open(&target)?.ok_or(NotAdopted)?;
let app = record.application(None)?;                 // the only application

let base = sources.checkout(app.template(), app.revision()); // Option<Template>
let next = Template::load(&next_folder)?;

let begun = record.begin_update(app, base.as_ref(), &next, next_revision, context)?;
let completed = match protocol::answer_headless(
    &next, begun.interview, begun.answers,
)? {
    Headless::Completed { completed, .. } => completed,
    other => return ask_caller(other),               // the new template needs answers
};
let update = begun.plan(completed)?;                 // resolves every final image
for change in update.changes() { show(change); }     // dry-run view
let options = UpdateOptions { conflicts: ConflictPolicy::Refuse, trusted };
match update.apply(options, &runner)? {
    Updated::UpToDate => {}
    Updated::Written { changes, conflicted, .. } => { /* report */ }
    Updated::NeedsTrust(update) => { /* nothing written */ }
}
```

A crate caller that writes a record on generation calls `project::apply`. A
crate caller that calls `Plan::apply` directly gets no record, as today.

## The applied record

### Carrier

The record is a set of ordinary files in the project:
`.toha/applied/<short-name>-<first 8 hex of sha256(formal name)>.json`. There is
one file for each template applied to the project. The files survive clone,
move, copy, and checkout because they are tracked project content, and they work
on targets that are not git repositories.

The file name locates a record and is never an identity. Toha reads every
`*.json` in the directory and indexes the records by their content. Two files
that name the same formal name are an error. With one file per template, two
branches that update different templates change different files, so git merges
them without a conflict.

The record directory is not `.toha.yml`, the local configuration file, and it
is not the staged interview in the user's state directory. A staged interview
holds an interview in progress. The applied record holds an apply that
completed.

### Content

```json
{
  "template": "forge:catalog/receipt@stable",
  "commit": "3f9a1c2e8d7b6a5f4e3d2c1b0a9f8e7d6c5b4a39",
  "generated": "2026-03-14T09:26:53+00:00[UTC]",
  "submissions": [
    { "label": "Sample", "quantity": 4 },
    { "use_vcs": true }
  ],
  "owned": {
    "files":   { "README.md": "sha256:9c1e…", "src/app.txt": "sha256:04ab…" },
    "regions": { "config/app.toml": ["features"] },
    "values":  { "package.json": { "scripts.build": "sha256:77f2…" } }
  }
}
```

| Member | Meaning |
|---|---|
| `template` | Formal name. It identifies the application and is compared by exact string equality. |
| `commit` | The resolved 40-hex commit, or `null` for a folder template. |
| `generated` | The frozen `now()` instant of the first apply. Every later render of this application uses it, so a template that renders a date stays byte-stable. |
| `submissions` | The accepted raw answer submissions in order, the same sequence a staged interview holds at completion. Values are the inputs before `format`. A value taken from a default is included. |
| `owned` | The ownership identities of the last apply, from `Plan::mutations()`. `files` and `values` hold the SHA-256 of the bytes or canonical JSON that Toha wrote. `regions` holds keys only, because each region's end marker already carries its checksum. |

The record holds no target path (the target is the directory that contains
`.toha/`), no file contents, no hook output, no trust, no host or environment
facts, and no format-version member. The schema denies unknown members.

### Boundary

`ProjectRecord::open` reads the directory once and parses every file into domain
types. The wire type is private. The record is operator-editable input, so every
member is validated: identifier syntax, commit syntax, instant syntax,
`TargetPath` rules (no absolute path, no `..`, not inside `.git` or `.toha/`),
region-key and JSON-path grammar, overlapping JSON paths, fingerprint syntax, and
symbolic links in `.toha`, `.toha/applied`, or a record file.

A forged record cannot cause data loss. Toha retracts (deletes) an identity only
when the identity is in the re-derived old plan **and** its rendered bytes match
the recorded fingerprint. Any other claim can produce only `release` (ownership
dropped, no bytes changed) or a conflict.

### When it is written

Every command apply writes or replaces the record of the applied template:
direct `apply TEMPLATE PATH`, staged `apply PATH`, and scripted
`apply TEMPLATE PATH --answers FILE`. `update` and `adopt` write it too. The
record file is written **last**, after all target files and hooks, through a
temporary file and a rename. It is the commit point.

## Update

### Inputs

1. **Record.** Select the application. With one application no flag is needed.
   With several, `--template FORMAL` selects one exactly, and without it the
   command refuses and lists them.
2. **Next revision.** By default, the commit a fresh `apply` of that formal name
   resolves now: the installed commit for a registry name, or the address
   reference for an address. `--to COMMIT` selects another commit of the same
   source.
3. **Base template.** The template at the recorded commit, taken from the
   source cache or fetched with gitoxide. When it cannot be obtained (offline,
   commit no longer reachable, or a folder template), the update continues in
   fingerprint mode. This never fails the update.
4. **Context.** Both renders use the current invocation context and the recorded
   `generated` instant. Environment values are read only under trust, as for a
   direct apply: `--trust` or a current matching registry approval.

### Answers

For each render, Toha filters every recorded submission to the question ids of
the template being rendered, then replays the submissions in order through the
staged-replay walk. A recorded answer for a removed question is dropped. For the
new template, the route's answers follow as one more submission. A question the
new template adds without a default, or a recorded answer it rejects, is asked
on the person route and reported as a `questions` result on the scripted route.
A flow `stop` or `abort` ends the update with nothing written.

### The merge base

The base plan is `Plan::build` of the old template with the replayed answers.
Each identity in it is **verified** when its rendered image matches the
record: a `files` or `values` fingerprint, or the region end-marker checksum on
disk. An identity that cannot be verified is in **fingerprint mode**. This
happens when the base template is unavailable, the replay does not complete, or
the render differs, for example because it reads a host fact that differs on
this machine. In fingerprint mode Toha knows whether the operator changed the
file, but not what the old text was.

### Per-identity rules

B = base (old render), T = theirs (disk), O = ours (new render).

**Whole file** (the merge unit is the file's composed image: whole-file bytes
with that file's bounded edits folded in, exactly as apply composes them):

| Case | Result |
|---|---|
| In old and new; T = B | `update` to O, or nothing when O = B |
| T ≠ B, O = B | Keep T. Nothing written. |
| T ≠ B, O ≠ B, T = O | Nothing written. |
| T ≠ B, O ≠ B, T ≠ O, all text | Three-way merge: `merge`, or `conflict` with a hunk count |
| Any side binary (a NUL byte in the first 8000 bytes) | `conflict` (binary) |
| T ≠ B in fingerprint mode | `conflict` |
| Operator deleted the file | O = B: `release`. O ≠ B: `conflict` (delete/modify). |
| Only in old (disappeared), T = B, verified | `retract`: delete the file |
| Only in old, T ≠ B, or not verified | `release`: keep the file, drop ownership |
| Only in new, file absent | `add` |
| Only in new, T = O | Take ownership. Nothing written. |
| Only in new, T ≠ O | `conflict` (add/add) |

**Region** (Toha owns the marker pair and the span; B is verified when
`sha256(B)` equals the end-marker checksum on disk):

| Case | Result |
|---|---|
| Span = B | Replace the span with O and a new checksum (`update`), or nothing when O = B |
| Span ≠ B, O = B | Keep the operator's span. Nothing written. A plain `apply` still reports drift. |
| Span ≠ B, O ≠ B, verified | Three-way merge inside the span only. Clean: `merge`, with body = merged text and checksum = sha256(O). |
| Span ≠ B, O ≠ B, fingerprint mode | `conflict` |
| Markers missing on disk | O = B: `release`. O ≠ B: `conflict` (delete/modify). |
| Only in old, span = checksum | `retract`: remove the marker lines and the span |
| Only in old, span drifted | `release`: remove the two marker lines only, keep the body |
| Only in new | Place at the bootstrap anchor (`add`), with the approved anchor errors |

A merged region keeps checksum = sha256(O), the body Toha intends. The span
still contains operator bytes, so a plain `apply` reports drift and does not
overwrite it, and the next update merges from base O.

**JSON value** (Toha owns the typed value; ownership is convergent, as
approved):

| Case | Result |
|---|---|
| In old and new | Set to O. `converge` when T ≠ O. No three-way merge and no conflict. |
| Only in old, T = B | `retract`: remove the key. Object parents stay. |
| Only in old, T ≠ B | `release`: keep the value |
| Only in new | Insert (`add`) |

An identity that another application in the same record owns is an
`OwnershipCollision` error.

### Conflicts

`--on-conflict refuse` is the default. Any conflict writes nothing, lists every
conflict, and names the markers command. `--on-conflict markers` writes
diff3-style markers into text conflicts (`<<<<<<< operator`,
`||||||| toha 3f9a1c2`, `=======`, `>>>>>>> toha 8be0d41`) and writes everything
else. Binary, delete/modify, and add/add conflicts still refuse. Before any
update, Toha scans owned files and region spans for its own marker labels and
refuses while any remain. No pending-conflict state is stored.

### Order of effects

```text
0 refuse: interview staged at the target; Toha conflict markers remain
1 resolve every final image in memory (read owned paths once)
2 trust gate: new hooks and not trusted -> NeedsTrust, nothing written
3 conflicts under refuse -> error, nothing written
4 no image and no record change -> UpToDate, nothing written, no hook
5 replace each changed target (sibling temp file + rename) or delete it
6 run the new template's hooks in the existing hook loop
7 write the record (temp file + rename)  <- commit point
```

A crash or hook failure between steps 5 and 7 leaves the old record. The next
update finds T = O for every written identity, writes nothing again, runs the
hooks, and writes the record. A second update with no template or operator
change returns `UpToDate` and changes no byte.

## Merge engine

The engine is gitoxide's blob merge, enabled as the `merge` feature of the
existing `gix` dependency. The feature adds the
[`gix-merge`](https://docs.rs/gix-merge) crate from the
[gitoxide](https://github.com/GitoxideLabs/gitoxide) project. Its diff core,
`imara-diff`, is already in the dependency tree. Toha calls one pure function:

```rust
gix::merge::blob::builtin_driver::text::merge(
    out, input, labels, current /* T */, ancestor /* B */, other /* O */, options,
) -> Resolution // Complete | CompleteWithAutoResolvedConflict | Conflict
```

`options.conflict` is `Keep { style: Diff3, marker_size: 7 }`. Toha never uses
the configurable driver platform of `gix-merge`, which can start external merge
drivers. One private adapter, `merge3`, is the only code that names the engine.

## Adoption

`toha adopt TEMPLATE PATH [--at COMMIT] [--answers FILE] [--replace] [--dry-run]`:

1. Resolve TEMPLATE as a fresh apply does. `--at` selects the commit that
   generated the project. Without it, Toha uses the commit resolved now.
2. Run the interview on the route's modality (person prompts, or a scripted
   answers document). Nothing is staged.
3. Build the plan, and compute every fingerprint from the render, never from
   the disk. A difference on disk is then an operator edit at the next update.
4. Write the record only. Toha writes no target file and runs no hook. The
   report classifies each identity as `matches`, `differs`, or `absent`.
   `--dry-run` writes nothing.
5. An existing application for the same formal name is refused unless
   `--replace` is given. `--replace` sets a new baseline, for example after the
   recorded commit disappears upstream.

`toha update` on a project without a record refuses and names `adopt`.
`apply TEMPLATE PATH --force` also sets a new baseline for that template.

## Routes

| Caller | Command | Output |
|---|---|---|
| Person | `update PATH` | Prompts only for the questions the new template needs, then the change list |
| Script, agent | `update --answers FILE PATH` | One JSON result document for every outcome |
| Person | `adopt TEMPLATE PATH` | Prompts, then the adoption report |
| Script, agent | `adopt --answers FILE TEMPLATE PATH` | One JSON result document |

Update and adopt never stage. Each run derives everything again from the record,
so an agent that receives a `questions` result reruns with a fuller document.
Both refuse while an interview is staged at the path, as the scripted apply
route does. The expected identity for an update answers document is the
selected application's `template`, compared by exact string equality through the
approved `parse_and_verify` gate. `--dry-run`, `--trust`, `--template`, `--to`,
and `--on-conflict` apply to `update`.

## Public interfaces

```rust
// toha::project — new public module
pub struct ProjectRecord { /* private */ }
impl ProjectRecord {
    pub fn open(target: &CanonicalTarget) -> Result<Option<ProjectRecord>, RecordError>;
    pub fn application(&self, select: Option<&str>) -> Result<&Application, UpdateError>;
    pub fn begin_update<'t>(
        &self,
        app: &Application,
        base: Option<&'t Template>,
        next: &'t Template,
        next_revision: Revision,
        context: InvocationContext,
    ) -> Result<Begun<'t>, UpdateError>;
}
impl Application {
    pub fn template(&self) -> &str;
    pub fn revision(&self) -> &Revision;
}
pub enum Revision { Commit(CommitId), Unversioned }

pub struct Begun<'t> {
    pub interview: Interview<'t>,  // next template; recorded instant; replay applied
    pub answers: RawAnswers,       // route answers are merged here by `with_document`
    /* private: baseline, record snapshot */
}
impl<'t> Begun<'t> {
    pub fn with_document(self, text: &str) -> Result<Self, SubmitDocumentError>;
    pub fn plan(self, completed: Completed) -> Result<UpdatePlan, UpdateError>;
}

pub struct UpdatePlan { /* private: final images, next plan, next record */ }
impl UpdatePlan {
    pub fn changes(&self) -> &[Change];
    pub fn apply(self, options: UpdateOptions, runner: &dyn HookRunner)
        -> Result<Updated, UpdateError>;
}
pub struct UpdateOptions { pub conflicts: ConflictPolicy, pub trusted: bool }
pub enum ConflictPolicy { Refuse, Markers }

pub fn apply(plan: Plan, provenance: &Provenance, completed: &Completed,
             target: &CanonicalTarget, options: ApplyOptions, runner: &dyn HookRunner)
    -> Result<Applied, ApplyError>;
pub fn adopt(provenance: &Provenance, template: &Template, completed: &Completed,
             target: &CanonicalTarget, replace: bool) -> Result<Adopted, AdoptError>;
```

`Plan::build`, `Plan::apply`, `Plan::mutations`, the injection resolvers, the
interview engine, `StagedRecord`, and the answers-document operations keep their
signatures. The injection module gains crate-private helpers for retraction and
for writing a merged region body under a given checksum. The approved public
producer interface and ownership semantics do not change.

## Data structures

```rust
pub struct Application {
    template: FormalName,          // non-empty; exact-equality identity
    revision: Revision,
    generated: FrozenNow,          // the representation StagedRecord.now uses
    submissions: Vec<IndexMap<Id, serde_json::Value>>, // accepted, raw, in order
    owned: Ownership,
    file: PathBuf,                 // locator only
}
pub struct CommitId([u8; 20]);     // exactly 40 lowercase hex on disk
pub struct Fingerprint([u8; 32]);  // "sha256:<64 hex>" on disk
pub struct Ownership {
    files: BTreeMap<TargetPath, Fingerprint>,
    regions: BTreeMap<TargetPath, BTreeSet<RegionKey>>,  // no fingerprint by type
    values: BTreeMap<TargetPath, BTreeMap<JsonPath, Fingerprint>>,
}
pub enum Identity {               // owned twin of FileMutation<'_>
    Whole { path: TargetPath },
    Region { path: TargetPath, region: RegionKey },
    JsonValue { path: TargetPath, json_path: JsonPath },
}
pub struct Change { pub identity: Identity, pub action: Action }
pub enum Action {
    Add, Update, Merge, Converge, Retract, Release, Conflict(ConflictKind),
}
pub enum ConflictKind { Content { hunks: usize }, Binary, DeleteModify, AddAdd }
pub enum Updated {
    UpToDate,
    Written { changes: Vec<Change>, conflicted: Vec<TargetPath>,
              hooks_run: usize, after_apply: Option<String> },
    NeedsTrust(UpdatePlan),
}
```

Invariants held by types: a `CommitId` is always 40 hex; a region cannot carry a
second checksum; a `ProjectRecord` has unique formal names and no public
constructor; an `UpdatePlan` exists only when every image is resolved; a delete
image comes only from `Retract`, which requires a verified base; `UpToDate`
cannot report writes.

## Module and seam

```text
CLI  apply (records) · update · adopt
  │ canonical_target · cli::resolve (next) · source cache/fetch (base)
  ▼
project/            new public module
  record.rs   open · boundary parse · write (temp + rename, commit point)
  update.rs   begin_update · baseline · reconcile (pure) · merge3 → gix-merge
              UpdatePlan::apply → apply's commit and hook loop
  adopt.rs    render-only fingerprints · record only
  │ consumes, unchanged                  │ reuses crate-internally
  ▼                                      ▼
plan.rs  Plan::build · Plan::mutations    apply.rs  atomic replace · hook loop
inject   resolvers + pub(crate) retract   staging.rs canonical_target · replay walk
protocol parse_and_verify · results       interview.rs, hook.rs, registry.rs: unchanged
```

A trace from the command to a written byte touches three files: `main.rs`,
`project/update.rs`, and `apply.rs`.

## Errors and results

### `RecordError` (each names the record file)

`Io`, `Json`, `Shape` (missing, unknown, or mistyped member), `Template` (empty),
`Commit`, `Generated`, `AnswerId`, `Path` (absolute, `..`, `.git`, `.toha/`,
symbolic link, or invalid), `RegionKey`, `JsonPath`, `Overlap`, `Fingerprint`,
`DuplicateApplication`.

### `UpdateError`

| Variant | Condition | Result `kind` / exit |
|---|---|---|
| `Record(RecordError)` | invalid record | `record` / 1 |
| `NoRecord` | no `.toha/applied/`; names `adopt` | `record` / 1 |
| `SelectApplication`, `UnknownApplication` | several applications and no selection, or no match | `input` / 1 |
| `Staged` | an interview is staged at the target | `staged` / 1 |
| `Resolve` | next revision cannot be resolved | `source` / 1, `ambiguous` / 5 |
| `Document` | answers document shape or identity | `document`, `identity` / 1 |
| `Plan` | the next render fails | `render` / 1 |
| `OwnershipCollision` | another application owns the identity | `conflict` / 1 |
| `UnresolvedConflicts` | Toha markers remain in owned content | `conflict` / 1 |
| `Conflicts` | any conflict under refuse; binary, delete/modify, add/add under markers | `conflict` / 1 |
| `Region`, `Json` | the approved injection apply errors | `render` / 1 |
| `Symlink`, `Io` | reading owned bytes or replacing targets | `input` / 1 |
| `Apply` | the existing hook failures | `hook` / 1 |
| `RecordWrite` | the commit point failed after files were written; a rerun converges | `record` / 1 |

Fingerprint mode is not an error. The change list and result document report
`fingerprint mode: <reason>`: `unversioned`, `commit unavailable`,
`replay incomplete`, or `N identities unverified`. For `commit unavailable`, it
names `adopt --at --replace`.

No error, `Debug`, or `Display` output contains answer values or file contents.
Conflict content reaches a caller only through the explicit change report.

### Results

| Outcome | Crate | Scripted status | Exit |
|---|---|---|---|
| Nothing to do | `UpToDate` | `updated`, empty `changes` | 0 |
| Written | `Written` | `updated` | 0 |
| Written with markers | `Written { conflicted }` | `updated`, non-empty `conflicted` | 1 |
| Trust required | `NeedsTrust` | `planned`, `trusted: false` | 3 |
| Dry run | `changes()` | `planned` with actions | 0 |
| Questions remain | `Headless::Pending` | `questions` | 4 |
| Flow ended | `Headless::Ended` | `ended` | 0 |
| Adopted | `Adopted` | `adopted` with `matches`/`differs`/`absent` | 0 |

## Behaviors to prove

1. **Version A to B with an edited file** (`update-merge-conflict` fixture): the
   template changes two files between A and B; the operator edits one of them on
   the same lines. Update refuses and lists that file, and every target byte and
   the record are unchanged. With markers, the file holds diff3 markers labeled
   operator/A/B, the other file is updated, and the exit is 1.
2. A clean three-way merge keeps the operator's lines and the template's lines.
3. A second update with no change returns `UpToDate`, writes no byte, and runs
   no hook.
4. A file that disappears at B is deleted when it is unedited and kept (ownership
   dropped) when it is edited.
5. A forged record entry for a file the old template never produced cannot
   cause a deletion.
6. A region merge changes only the span, and a later plain `apply` reports drift.
7. A disappeared region is removed when intact, and only its markers are removed
   when it drifted.
8. An operator's change to an owned JSON value converges to B without `--force`,
   and unrelated keys and comments are unchanged.
9. A disappeared JSON value is removed when unedited and kept when edited.
10. When the base commit is unavailable, the update runs in fingerprint mode,
    never deletes, and refuses edited-and-changed files.
11. A template that renders a host fact is verified on the same host and falls
    back to fingerprint mode for that file only on another host.
12. Recorded submissions replay pre-`format` values: a question with a
    non-idempotent `format` produces the same answer on replay.
13. A recorded answer for a question removed at B is dropped, a question added
    at B is asked (person) or reported (`questions`, exit 4), and a rejected
    recorded answer is reported with its rejection.
14. A date-rendering template is byte-stable across updates because of
    `generated`.
15. Every apply route (direct, staged, scripted) writes the record last. A hook
    failure leaves the old record, and a rerun converges.
16. An untrusted template with hooks returns `NeedsTrust` and writes nothing.
17. Update and adopt refuse while an interview is staged at the path, and the
    staged record is unchanged.
18. Toha markers left in an owned file make the next update refuse.
19. Adopt writes only the record. Each fingerprint comes from the render, and
    the report classifies `matches`, `differs`, and `absent`.
20. Two templates applied to one project have two record files; updating one
    leaves the other's file and owned content unchanged, and a collision is
    refused.
21. A template output path under `.toha/applied/` fails at planning.
22. An answers document naming another template fails with the identity error
    before any answer is evaluated.
23. Person, scripted, and crate update produce the same changes and bytes for
    the same inputs.
24. The update starts no process except the template's trusted hooks.

## Sole-kill guards

- Replace `retract` verification with `true`: fails behavior 5.
- Record completed answers instead of raw submissions: fails behavior 12.
- Remove the removed-question filter: fails behavior 13.
- Use the current instant instead of `generated`: fails behavior 14.
- Write the record before hooks: fails behavior 15.
- Checksum a merged region as sha256(merged): fails behavior 6.
- Three-way merge JSON values: fails behavior 8.
- Write clean identities while a conflict is refused: fails behavior 1.
- Remove the marker preflight: fails behavior 18.
- Fingerprint from disk in adopt: fails behavior 19.
- Route the merge through the `gix-merge` driver platform: the process-spawn
  guard for behavior 24 fails.

## Out of scope

- Rename detection. A moved path is a retract or release plus an add.
- Searching project history for the commit that generated an adopted project.
- Changing the approved injection ownership semantics, the answers-document
  contract, the hook-result lifetime, the staged record, or trust.
- Authoring commits, refs, or notes in the operator's repository.
- Per-question exclusion of answers from the record.
- New permissions, timeouts, pinned-version checks, or application subprocesses.

## Compatibility and canonical documents

Toha is before 1.0.0 and has no dependent users. Every generated project now
contains `.toha/applied/`. Existing fixtures gain that record in their expected
trees.

The implementation updates, in the same change:

- `docs/concepts/toha.yml`: the update-projects feature and the applied-record
  property.
- `docs/specifications/command-line-interface.yml` and `.spec.yml`: `update`,
  `adopt`, `apply` recording, conflict policy, exit behavior.
- A new `docs/specifications/applied-record.yml` and its schema.
- `docs/specifications/interview-protocol.yml` and schema: the `updated`,
  `adopted`, and update `planned` result documents.
- `docs/specifications/template-format.yml` and schema: the reserved
  `.toha/applied/` output path.
- `docs/technical-designs/architecture.yml`: the `project` module and the `gix`
  `merge` feature.
- A user guide page on updating projects, and a generic example.
- Fixture harness: `tests/fixtures/update-*` with `template-a/`, `template-b/`,
  `answers.json`, an `edits/` overlay with a removal list, `expected/`, and
  `expect.yml`; each fixture also asserts a second update is `UpToDate`.

## Decisions

The decisions that need approval, with options and recommendations, are in
`design.yml` under `decisions-for-approval` and on the brief deck.
