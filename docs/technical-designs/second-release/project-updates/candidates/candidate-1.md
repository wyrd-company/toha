# Project updates — candidate 1: record the inputs, derive the base

## Problem

Toha renders a template into a target and forgets. To move a generated project
from template T at commit A to T at commit B, Toha needs three images of every
owned thing: what it wrote (base), what the operator has now (theirs), and what
it would write now (ours). Ours is cheap: `Plan::build` at B. Theirs is on disk.
The base is the hard part, and it decides the shape of everything else.

Several constraints make the shape non-obvious:

- The record must travel with the project (clone, move, re-checkout), so it
  cannot live in XDG state. It also must not reuse `.toha.yml`, which is the
  local config file.
- The record is operator-editable and may arrive through someone else's commit.
  It is untrusted input and must be parsed at a boundary. A forged record must
  never make Toha delete or overwrite a file the template did not produce.
- Injection (content-injection design) defines three ownership identities with
  different semantics: `Whole` replaces, `Region` has a checksum and refuses on
  drift, and `JsonValue` converges. The update must not collapse them into
  whole-file merges. Injection leaves identity removal and three-way merge to
  this design.
- Injection also makes one project the target of **several** templates, for
  example a base scaffold plus a feature template that adds a region. The
  record must hold more than one application per target.
- The only version identity is the resolved git commit. Folder templates have
  an empty commit (`src/cli/resolve.rs:202,276`).
- The pure engine, every caller route (script/agent/person/crate), the trust
  gate, and the per-apply hook-result lifetime must not change.
- No subprocess. `gix` is already a dependency.

The central decision is that **the record stores the render inputs, not the
rendered outputs**: formal name, commit, frozen `now`, and the completed
answers. The base is re-derived when needed by re-rendering T@A through the same
pure `Plan::build`. The record also holds one small SHA-256 fingerprint per
owned identity. The fingerprint does not act as a source of content. It
**verifies** that the re-derived base matches what was actually written, and it
is the safe fallback when the base cannot be re-derived.

## Usage (caller's view)

### README / quickstart

> **Updating a generated project**
>
> Every `toha apply` records what it applied in `.toha/applied/` in the project.
> Commit that directory with your project.
>
> ```console
> $ toha apply forge:catalog/receipt@stable ./sample-service
> $ git -C sample-service add -A && git -C sample-service commit -m "scaffold"
> ```
>
> When the template changes upstream, bring the project forward:
>
> ```console
> $ toha templates update receipt        # optional: move the installed template
> $ toha update ./sample-service --dry-run
> update  src/app.txt               (whole)            template changed, you did not
> merge   README.md                 (whole)            your edits kept, template edits merged
> update  config/app.toml           (region features)
> converge package.json             (value scripts.build)
> add     src/health.txt            (whole)
> retract docs/legacy.md            (whole)            the template no longer produces it
> release notes/todo.md             (whole)            template dropped it; you edited it, so it is yours now
> conflict src/routes.txt           (whole)            2 hunks
> $ toha update ./sample-service --on-conflict markers
> ```
>
> Toha merges your edits with the template's edits line by line. When a line
> was changed by both, the default is to write nothing and list the conflicts.
> `--on-conflict markers` writes git-style markers so you can resolve them.
> `--on-conflict template` takes the template's version. Toha never deletes a
> file you edited. If the template stops producing a file you changed, Toha
> hands that file over to you.
>
> A project made before this feature, or by hand, has no record. Adopt it once:
>
> ```console
> $ toha adopt forge:catalog/receipt@stable ./sample-service --at 3f9a1c2
> ```
>
> Adoption asks the questions and writes only the record. It never changes your
> files. Any difference between your files and the template's output at that
> commit counts as your edit on the next update.

### Call site 1 — person route (prompts only for new questions)

```console
$ toha update ./sample-service
receipt: template moved 3f9a1c2 → 8be0d41
? Currency code for totals (new question): EUR
merge   README.md (whole)
add     src/health.txt (whole)
updated ./sample-service to 8be0d41 (2 files); hooks ran: 1
```

The recorded answers are replayed. The person is asked only for questions that
are new at B or whose recorded answers B rejects. Without a terminal on
standard input the command refuses and names `update PATH --async`.

### Call site 2 — agent and script routes

```console
# agent: a new question at B → batch + instructions, exit 4; nothing written, nothing staged
$ toha update ./sample-service --async
{ "protocol": 1, "status": "questions", "context": { "template": "forge:catalog/receipt@stable", ... }, ... }

answer the new questions with an answers document:
  {"template": "forge:catalog/receipt@stable", "answers": { ... }}
then run: toha update ./sample-service --async <ANSWERS> (- reads standard input)

$ toha update ./sample-service --async answers.json   # applies when complete

# script: one JSON result document for every outcome
$ toha update ./sample-service --answers answers.json --on-conflict refuse --trust
{ "protocol": 1, "status": "error", "kind": "conflict",
  "conflicts": [ { "path": "src/routes.txt", "owner": "whole", "kind": "content", "hunks": 2 } ] }
$ echo $?
1
```

Several applications in one target: `--template forge:catalog/receipt@stable`
selects one. With exactly one application the flag is optional. With several
and no flag, the command refuses and lists them. `--to COMMIT` moves to a
specific commit of the same source. By default the target revision is what a
fresh `apply` of that formal name resolves now: the registry-installed commit
for a named template, otherwise the address's ref.

### Call site 3 — crate

```rust
use toha::{
    project::{ConflictPolicy, ProjectRecord, Revision, UpdateOptions, Updated},
    protocol::{self, Headless},
    staging::canonical_target,
    template::Template,
};

let target = canonical_target(Path::new("./sample-service"))?;
let record = ProjectRecord::open(&target)?          // boundary parse of .toha/applied/*.json
    .ok_or(MyError::NotAdopted)?;                    // None ⇒ no record: adopt first
let app = record.application(None)?;                 // the only application

// The caller owns source I/O, as it does today. `None` for base_tpl is allowed:
// the update then runs in fingerprint mode (see Shape).
let base_tpl = my_sources.checkout(app.template(), app.revision()).map(Template::load).transpose()?;
let next_tpl = Template::load(&next_folder)?;

let begun = record.begin_update(app, base_tpl.as_ref(), &next_tpl, Revision::commit(&next_hex)?)?;
let completed = match protocol::answer_headless(&next_tpl, begun.interview, begun.seed)? {
    Headless::Completed { completed, .. } => completed,
    other => return Ok(ask_caller(other)),           // new questions at B
};
let update = begun.plan(completed)?;                 // reads owned target bytes once; resolves everything
for change in update.changes() { log(change); }      // dry-run view
match update.apply(UpdateOptions { conflicts: ConflictPolicy::Refuse, trusted: true }, &runner)? {
    Updated::UpToDate => {}
    Updated::Written { changes, conflicted, hooks_run, after_apply, .. } => { /* ... */ }
    Updated::NeedsTrust(update) => { /* unchanged trust gate; nothing written */ }
}
```

Fresh generation writes the record through one call that replaces the
`Plan::apply` call in the CLI. Crate callers who do not want a record keep
calling `Plan::apply` unchanged:

```rust
let applied = toha::project::apply(plan, &Provenance::new(formal, revision, now), &completed,
                                   &target, ApplyOptions { force, trusted }, &runner)?;
```

### The record on disk

`.toha/applied/receipt-5e1d0c7a.json` (one file per application; the filename
is a locator only, and the content is the authority):

```json
{
  "template": "forge:catalog/receipt@stable",
  "commit": "3f9a1c2e8d7b6a5f4e3d2c1b0a9f8e7d6c5b4a39",
  "generated": "2026-03-14T09:26:53Z",
  "answers": { "label": "Sample", "quantity": 4, "use_vcs": true },
  "owned": {
    "files":   { "README.md": "sha256:9c1e…", "src/app.txt": "sha256:04ab…" },
    "regions": { "config/app.toml": ["features"] },
    "values":  { "package.json": { "scripts.build": "sha256:77f2…" } }
  }
}
```

`{ "template", "answers" }` is exactly an answers document as the
headless-recovery design defines it. An operator can reuse it for
`apply T OTHER_PATH --answers`, and headless-recovery allows that reuse
explicitly.

## Shape

### Decision 1 — the applied record: carrier, shape, ownership

| Carrier | Travels with clone/move/checkout | Needs git | Readable/diffable | Multi-application | Verdict |
|---|---|---|---|---|---|
| **Project-root files** `.toha/applied/*.json` | Yes: ordinary tracked files | No | Yes: reviewed in PRs like any config | One file per application | **Chosen** |
| Commit trailer (`Toha-Applied: …`) | Only while that commit is in history. A squash, rebase, or shallow clone loses or hides it. | Yes, and Toha would have to **author commits** in the operator's repo | Poorly: the data is in commit messages. Answers do not fit in a trailer. | Must scan history for the newest trailer per template | Rejected |
| Custom ref / note (`refs/notes/toha`) | **No.** Notes and custom refs are not fetched by a default `git clone`, and a directory copy loses them. | Yes | Hidden from normal review | Possible | Rejected: fails the travel requirement |

Files win because the travel requirement is structural. Anything outside the
tracked tree fails clone or copy, and a trailer would force Toha to write
commits, which is a new effect on the operator's repository. Toha also works on
non-git targets today, and a file keeps that working.

**Location and naming.** The directory is `.toha/applied/`, with one JSON file
per application. JSON is used because answers are already `serde_json::Value`
and the injection design already embeds a JSON parser. One file per application
applies per-actor state (per separate-before-serializing-shared-state). Two
branches that each update a different template change different files, so git
merges them without conflict. A single shared file would conflict textually. The
filename is `<short-slug>-<first 8 hex of sha256(formal name)>.json`. On
load, Toha reads every `*.json` in the directory and indexes by content.
Filenames are never trusted, and two files that name the same formal name are
a `RecordError`. `Plan::build` rejects any template output under `.toha/`. This
reserves the prefix, so a template cannot overwrite a record.

**What the record stores** (and why each field exists):

- `template`: formal name. It is the identity authority for this application
  and is compared by exact string equality, as in the headless-recovery design.
- `commit`: 40-hex commit, or `null` for an unversioned (folder) template.
- `generated`: the frozen `now()` instant of the first apply. **Both** the base
  and the ours render reuse it forever. Without it, a template that renders a
  date would differ on every update, and idempotency would be impossible.
- `answers`: the **completed** flat answer map, including accepted defaults.
  Staged-style `submissions` are not stored. Replaying submissions would
  re-resolve configured defaults, which can drift. The completed map pins them.
- `owned`: the ownership identities of the last apply, each with a fingerprint.
  `files` maps each path to sha256 of the bytes written. `values` maps each
  (path, json path) to sha256 of the value's canonical JSON. `regions` records
  only keys, because the region end marker already carries the checksum. The
  marker is the single source of truth, and the record does not duplicate it.

**What it deliberately does not store:**

- **The target.** The target is the directory that contains `.toha/`. It is
  derived, so a moved project stays correct. An absolute path in the record
  would be wrong after the first `mv`.
- **File bodies.** The base is re-derived by rendering. Bodies would bloat the
  repository, duplicate the template, and create a second source of truth.
- **Hook output.** Per the hook-results design, hook output lasts one apply.
- **Trust.** The registry's approval digest stays the only trust authority.
- **A format-version member.** The format has no version member, matching the
  answers envelope. Toha is pre-1.0, and a strict schema with unknown fields
  denied is enough.

**Commit point.** The record file is written **last**, after files and hooks,
by atomic temp-file-and-rename. The next section shows why a crash at any
earlier point self-heals.

### Data structures

```rust
// ─── src/project/record.rs — project-resident record; boundary parse, trusted inside ───

/// Reserved directory under the target root. `Plan::build` rejects template
/// output under `.toha/` (PlanError::ReservedPath).
pub const RECORD_DIR: &str = ".toha/applied";

/// Every application recorded in one target. Constructed only by `open` (from
/// disk, validated) or by a successful apply/update/adopt. No public constructor.
pub struct ProjectRecord {
    target: CanonicalTarget,                         // derived from where the record lives
    applications: IndexMap<FormalName, Application>, // unique formal names by construction
}

pub struct Application {
    template: FormalName,
    revision: Revision,
    generated: FrozenNow,
    answers: RecordedAnswers,
    owned: Ownership,
    file: PathBuf, // locator of the backing file, relative to the target; never an identity
}

/// Exact-equality identity; non-empty. The same string type the
/// headless-recovery design compares.
pub struct FormalName(String);

pub enum Revision {
    Commit(CommitId),
    /// A folder template: no version identity. The base cannot be re-derived,
    /// so updates of this application always run in fingerprint mode.
    Unversioned,
}
pub struct CommitId([u8; 20]); // parsed from exactly 40 lowercase hex characters

/// The generation instant, in the representation `StagedRecord.now` already uses.
pub struct FrozenNow(String);

/// Completed answers keyed by validated `Id`. Keys are syntax-checked at the
/// boundary. Values are checked by the engine when replayed.
pub struct RecordedAnswers(IndexMap<Id, serde_json::Value>);

pub struct Fingerprint([u8; 32]); // "sha256:<64 hex>" on disk

/// Region ownership carries no fingerprint by type: the end marker's checksum
/// is the only authority for a region's last-written body.
pub struct Ownership {
    files: BTreeMap<TargetPath, Fingerprint>,
    regions: BTreeMap<TargetPath, BTreeSet<RegionKey>>,
    values: BTreeMap<TargetPath, BTreeMap<JsonPath, Fingerprint>>,
}

/// Owned twin of injection's borrowed `FileMutation<'_>`. This is the join key
/// between base plan, ours plan, and record. Ordered so that joins are
/// deterministic merges over sorted sequences (no map rebuilt later).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    Whole { path: TargetPath },
    Region { path: TargetPath, region: RegionKey },
    JsonValue { path: TargetPath, json_path: JsonPath },
}
impl From<FileMutation<'_>> for Identity { /* clone the borrowed keys */ }

// Wire form: private, never escapes the module.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct WireApplication {
    template: String,
    commit: Option<String>,
    generated: String,
    answers: serde_json::Map<String, serde_json::Value>,
    owned: WireOwnership, // { files: {path: "sha256:…"}, regions: {path: [key]}, values: {path: {jsonpath: "sha256:…"}} }
}
```

```rust
// ─── src/project/update.rs — reconciliation types ───

pub struct Provenance { template: FormalName, revision: Revision, generated: FrozenNow }

/// The three images of one identity, as reconcile sees them.
enum BaseImage<'a> {
    /// Re-derived from T@A, and matched to the fingerprint (or the region marker checksum).
    Verified(&'a [u8]),
    /// Not re-derivable or failed verification; only the fingerprint is known.
    FingerprintOnly(Option<&'a Fingerprint>), // None for regions: marker checksum used instead
    /// Toha did not own this identity at A.
    Absent,
}

pub enum ConflictPolicy {
    /// Default: any conflict ⇒ write nothing (the same contract as today's unchanged-on-conflict).
    Refuse,
    /// Write diff3-style markers into text conflicts. Binary and delete/modify conflicts still refuse.
    Markers,
    /// Take the template's image for every conflict (today's `--force` meaning, per identity).
    Template,
}
pub struct UpdateOptions { pub conflicts: ConflictPolicy, pub trusted: bool }

/// Per-identity decision; also the dry-run vocabulary.
pub enum Action {
    Add,       // new at B; target absent (or identical bytes already present)
    Update,    // template changed; operator did not
    Merge,     // template and operator both changed; clean three-way
    Converge,  // JsonValue reset to the declared value (injection semantics)
    Retract,   // gone at B; operator did not touch it ⇒ removed
    Release,   // gone at B, or the operator deleted it; operator content kept; ownership dropped
    Conflict(ConflictKind),
}
pub enum ConflictKind {
    Content { hunks: usize }, // text, both sides changed the same lines
    Binary,                   // non-text, both sides changed
    DeleteModify,             // operator deleted it; template changed it
    AddAdd,                   // new at B, but a different operator file already occupies it
}
pub struct Change { pub identity: Identity, pub action: Action }

/// Fully resolved update: every final byte image computed before the first write.
pub struct UpdatePlan {
    target: CanonicalTarget,
    next: Plan,                                  // hooks + messages come from here, unchanged
    images: Vec<(TargetPath, FinalImage)>,       // one entry per touched path, plan order
    changes: Vec<Change>,
    record: Application,                         // the record that the commit point writes
    degraded: Option<Degraded>,                  // why (some) identities used fingerprint mode
}
enum FinalImage { Write(Vec<u8>), Delete }

pub enum Updated {
    UpToDate, // no image, no record change ⇒ no write, no hook: a byte no-op
    Written {
        changes: Vec<Change>,
        conflicted: Vec<TargetPath>, // non-empty only under ConflictPolicy::Markers
        hooks_run: usize,
        after_apply: Option<String>,
        degraded: Option<Degraded>,
    },
    NeedsTrust(UpdatePlan),
}
```

The dominant access patterns traced through these structures:

- **"Is this identity owned at A, B, or both?"** Three sorted `Identity`
  sequences (record, base plan `mutations()`, ours plan `mutations()`) are
  joined in one linear merge. No later index is needed.
- **"Did the operator edit it?"** Compare the disk image with the verified base
  image. In fingerprint mode, compare `sha256(disk)` with the fingerprint.
- **"What does the target file become?"** Group changes by `TargetPath`. A
  `Whole` identity on a path subsumes the bounded identities on the same path:
  the merge unit is the composed image (whole bytes with edits folded, exactly
  as injection composes them). Without a `Whole`, the bounded steps fold over
  the disk bytes in plan order, as injection's apply does.

### Flow through the signatures

```rust
// record.rs — boundary
impl ProjectRecord {
    /// Reads `.toha/applied/*.json` under the target once. `Ok(None)` when the
    /// directory is absent. Every field is validated into domain types; see RecordError.
    pub fn open(target: &CanonicalTarget) -> Result<Option<ProjectRecord>, RecordError>;
    /// `None` selects the only application; otherwise exact formal-name match.
    pub fn application(&self, select: Option<&str>) -> Result<&Application, UpdateError>;
    pub fn begin_update<'t>(&self, app: &Application, base: Option<&'t Template>,
                            next: &'t Template, next_revision: Revision)
        -> Result<Begun<'t>, UpdateError>;
}
impl Application {
    pub fn template(&self) -> &FormalName;
    pub fn revision(&self) -> &Revision;
    /// `{ "template", "answers" }`: an answers document (headless-recovery envelope).
    pub fn answers_document(&self) -> serde_json::Value;
}

// update.rs — orchestration (thin shell) + pure core
pub struct Begun<'t> {
    pub interview: Interview<'t>,  // T@B interview, started with the frozen `now`
    pub seed: RawAnswers,          // recorded answers (+ verified overrides) for answer_headless / prompting
    baseline: Baseline,            // base plan (or degraded marker), computed here
    // private: record snapshot, next template/revision
}
impl<'t> Begun<'t> {
    /// Merges an external answers document over the recorded answers. The
    /// document is checked against the application's formal name, exactly as
    /// headless-recovery's parse_and_verify does; it overrides or extends answers.
    pub fn with_document(self, text: &str) -> Result<Self, SubmitDocumentError>;
    /// Builds ours, reads the owned target bytes once, reconciles, and resolves
    /// every final image. No write.
    pub fn plan(self, completed: Completed) -> Result<UpdatePlan, UpdateError>;
}
impl UpdatePlan {
    pub fn changes(&self) -> &[Change];
    pub fn apply(self, options: UpdateOptions, runner: &dyn HookRunner) -> Result<Updated, UpdateError>;
}

/// Pure, total: the base is always *something*; it degrades per identity and never errors.
fn baseline(app: &Application, base: Option<&Template>, target: &CanonicalTarget) -> Baseline;
//   base None or Revision::Unversioned        → Baseline::FingerprintOnly(reason)
//   answer_headless(T@A, recorded answers) fails
//     or stops short of Completed             → Baseline::FingerprintOnly(ReplayFailed)
//   else Plan::build(T@A, completed, target)  → Baseline::Rendered(plan)
//        per identity: sha256(rendered image) != fingerprint → marked unverified
//        (FingerprintOnly for that identity only)

/// Pure core: no I/O, no clock, no hooks.
fn reconcile(base: &Baseline, ours: &Plan, disk: &DiskImage, record: &Ownership,
             others: &OtherApplications) -> Result<Reconciliation, UpdateError>;

/// The merge engine adapter (below). Pure.
fn merge3(base: &[u8], theirs: &[u8], ours: &[u8], labels: &Labels) -> Merge3;
enum Merge3 { Clean(Vec<u8>), Conflicted { marked: Vec<u8>, hunks: usize }, Binary }

/// Shell: reads only the owned paths plus B's new paths. It refuses symlinks and
/// `.git`, with the same guard injection uses.
fn read_disk(target: &CanonicalTarget, paths: impl Iterator<Item = &TargetPath>) -> Result<DiskImage, UpdateError>;

// project.rs — fresh generation that records
pub fn apply(plan: Plan, provenance: &Provenance, completed: &Completed, target: &CanonicalTarget,
             options: ApplyOptions, runner: &dyn HookRunner) -> Result<Applied, ApplyError>;
pub fn adopt(provenance: &Provenance, template: &Template, completed: &Completed,
             target: &CanonicalTarget, replace: bool) -> Result<Adopted, AdoptError>;
```

`UpdatePlan::apply` order (same resolve-before-write discipline as injection):

```text
0 preflight: another staged interview at target → refuse; unresolved Toha markers in owned files → refuse
1 trust gate: next.hooks non-empty && !trusted → NeedsTrust(self)       (nothing written)
2 conflicts: any Conflict && policy == Refuse → Err(Conflicts)          (nothing written)
3 if images empty && record unchanged → UpToDate                        (no write, no hook)
4 per path: atomic replace (sibling temp + rename, mode kept) or delete  (injection's commit helper)
5 run next's hooks via apply's hook loop (hook results live and die here)
6 write the record file (temp + rename)  ◀── commit point
```

**Crash-safety and idempotency follow from "record last".** Suppose a crash or
hook failure between steps 4 and 6. The record still says A, and disk holds B's
images. The next `update` re-derives base A, and for each identity it finds
theirs == ours. diff3 treats identical changes on both sides as clean, so no
image changes. The record differs (A → B), so the update is not `UpToDate`:
hooks run again and the record is written. The operation converges. A second
run with nothing changed has base == ours == theirs for every identity, no
images, and an identical record, so it returns `UpToDate` with zero bytes
written and no hooks run (per make-operations-idempotent).

### Decision 2 — update and merge semantics per identity

Notation: **B** = base (T@A render, verified), **T** = theirs (disk), **O** =
ours (T@B render). "=" is byte equality. In fingerprint mode, "T = B" means
`sha256(T) = fingerprint`, and B's bytes are unknown.

**Whole** (Toha owns the complete file; the merge unit is the composed image):

| Case | Result |
|---|---|
| in A and B; T = B | `Update` to O (or nothing when O = B) |
| in A and B; T ≠ B, O = B | keep T (operator edit; template unchanged). Nothing to write. |
| in A and B; T ≠ B, O ≠ B, T = O | nothing to write (both made the same change) |
| in A and B; T ≠ B, O ≠ B, T ≠ O, text | `merge3(B, T, O)` → `Merge`, or `Conflict(Content{hunks})` |
| same, but any side binary | `Conflict(Binary)` |
| same, fingerprint mode | `Conflict(Content{hunks: 0})`: no base, so no line merge. Honest refusal. |
| in A and B; T missing (operator deleted) | O = B → `Release` (stays deleted). O ≠ B → `Conflict(DeleteModify)`. |
| in A only (**disappeared**); T = B | `Retract`: delete the file. Parent directories are left. |
| in A only; T ≠ B | `Release`: file kept, ownership dropped from the record |
| in A only; T missing | drop from the record |
| in A only; fingerprint mode | always `Release`, **never** `Retract` (see the security rule below) |
| in B only; T missing | `Add` |
| in B only; T = O | adopt ownership silently (no write) |
| in B only; T ≠ O | `Conflict(AddAdd)`: diff3 with an empty base under `Markers` |

**Region** (Toha owns the marker pair and the enclosed span. B = the old body.
It is verified because `sha256(B)` must equal the end-marker checksum on disk.
If it does not match, the region is in fingerprint mode, and the marker checksum
stands in for B):

| Case | Result |
|---|---|
| span = B (checksum matches) | replace the span with O and a fresh checksum (`Update`), or nothing when O = B. This is injection's safe-replay path unchanged. |
| span ≠ B (injection "drift"), O = B | keep the operator span. Nothing to write. The checksum is not refreshed, so a plain `apply` still reports drift, as it does today. |
| span ≠ B, O ≠ B, verified base | `merge3(B, span, O)` **within the span only**. When clean → `Merge`: body = merged, checksum = sha256(**O**). |
| span ≠ B, O ≠ B, fingerprint mode | `Conflict(Content)` |
| markers absent at disk (operator removed the region) | O = B → `Release`. O ≠ B → `Conflict(DeleteModify)`. `Template` re-inserts at the bootstrap anchor. |
| in A only (**disappeared**); span = checksum | `Retract`: excise the marker lines and the span |
| in A only; span drifted | `Release`: **strip only the two marker lines** (Toha owns the markers) and keep the body. The file no longer carries a claim that no template makes. |
| in B only | injection placement at the bootstrap anchor (`Add`). Injection's anchor errors apply. |
| key present on disk but not in A (e.g. another template) | ownership collision, see Decision 4 |

The merged-region checksum rule is deliberate. The checksum records the body
**Toha intends** (O), not the merged body. A merged span still contains operator
bytes. If the checksum covered them, the next plain `apply` would see
"checksum matches" and silently overwrite the operator's lines. With checksum =
sha256(O), plain `apply` keeps reporting honest drift, and the next `update`
re-merges from base O. `Markers` on a region conflict writes the markers inside
the span with checksum = sha256(O), so the region reads as drifted until the
operator resolves it.

**JsonValue** (Toha owns the typed value at a path. Injection defines it as
**convergent**, and that semantic is retained):

| Case | Result |
|---|---|
| in A and B, any T | set to O. `Converge` if T ≠ O, and nothing if T = O. No three-way merge and no conflict: injection says convergent values never drift. |
| in A only (**disappeared**); T = B (value-equal, or `sha256(canonical(T))` = fingerprint) | `Retract`: remove the key. Object parents that Toha created are left. |
| in A only; T ≠ B | `Release`: value kept |
| in A only; key already absent | drop from the record |
| in B only | injection insert (`Add` or `Converge`) |

**Security rule for untrusted records.** Toha acts only on an identity that the
record claims **and** that some template produces. A retraction additionally
needs a **verified** base: the identity is in the re-derived T@A plan, and its
render equals the recorded fingerprint. A forged record that lists `README.md`
with its current hash therefore cannot cause a deletion. `README.md` is not in
T@A's plan, so the forged claim yields `Release`, which drops the claim and
changes no bytes. In fingerprint mode nothing is ever retracted. The worst case
of a forged record is a `Conflict` or a `Release`. It is never data loss.

**Operator-edited Toha-owned files** are therefore handled per identity: Whole
by line merge, Region by span merge, and JsonValue by convergence. None of them
is collapsed into whole-file overwrite.

**Answers across versions.** The recorded answers seed the T@B interview.
Unknown question ids at B (questions removed) are dropped silently. The
headless walk ignores held answers for questions it never reaches. A new
question enters the next batch. A recorded answer that B rejects becomes a
rejection with its error (agent/person), or an `error` document (script). This
mirrors staged-replay rejection and is not a new rule. An answers document
passed to update may override recorded answers: "same answers" is the default,
not a prison. The base still uses the recorded answers, and ours uses the
overridden ones.

**Unresolved markers.** Preflight scans every owned Whole file and every owned
region span for Toha's conflict label lines (`<<<<<<< operator` …
`>>>>>>> toha <short-commit>`). It refuses `UnresolvedConflicts` while any
remain. This is derived from the disk, and the record keeps no "pending" state
(per single-source-of-truth).

### Merge engine

**Recommendation: gitoxide's own blob merge, `gix-merge`, enabled as the
`merge` feature of the existing `gix` dependency.** Git's built-in text merge
driver becomes a pure in-process function over three byte slices. No
repository, index, or subprocess is involved.

- Crate: `gix-merge`, part of gitoxide.
  Repo <https://github.com/GitoxideLabs/gitoxide>. Docs
  <https://docs.rs/gix-merge>. Re-exported as `gix::merge` when the `gix`
  `merge` feature is on.
- Function used: the builtin text driver
  (`gix_merge::blob::builtin_driver::text::merge`), which takes
  `(out, interned input, labels, current, ancestor, other, options)` and
  returns a resolution: complete, or conflict with markers written to `out`.
  Options: `ConflictStyle::Diff3`, so the base section is shown, and marker
  size 7. Labels: `ancestor = "toha <A-short>"`, `current = "operator"`,
  `other = "toha <B-short>"`. The exact module path and argument order must be
  confirmed against the `gix` 0.87 line when the feature is enabled. See Open
  questions.
- Its diff core is `imara-diff` (<https://github.com/pascalkuthe/imara-diff>),
  which is **already in `Cargo.lock`** transitively through `gix-diff`. The new
  feature adds the `gix-merge` crate from the same, already-trusted project.
- Binary detection: a NUL byte in the first 8000 bytes of any side (git's
  heuristic). Binary sides are never line-merged: see `ConflictKind::Binary`.
  A `Content::Copied` file is merged as text when all three sides pass this
  check.
- Why not the others: `diffy` (<https://github.com/bmwill/diffy>) is pure Rust
  and has a direct `merge(ancestor, ours, theirs)`. It is the named
  **fallback** if the `gix` merge surface proves unusable. It loses as the
  first choice because it is a separate, smaller project, while `gix-merge`
  reuses a dependency family Toha already audits and produces git-identical
  marker output that operators already know. `git merge-file` is **not
  proposed**. No subprocess appears anywhere in this design.

The adapter `merge3` is the only place that names the engine. Swapping to
`diffy` changes one private function.

### Decision 3 — adoption of record-less projects

`toha update PATH` with no `.toha/applied/` refuses (exit 1). The message names
`toha adopt TEMPLATE PATH`. The project is not faulty; it was asked in the
wrong form.

`toha adopt TEMPLATE PATH [--at COMMIT] [--answers FILE | --async [FILE]] [--dry-run]`:

1. Resolve TEMPLATE as a fresh apply would. `--at` pins the commit that
   generated the project, if the operator knows it; otherwise the currently
   resolved commit is used.
2. Run the interview on that route's modality (person prompts, agent batches,
   script document). This reconstructs the answers. No staged state is used.
3. Build the plan. For every identity, compute its fingerprint **from the
   render, never from the disk**. Any disk difference is therefore correctly an
   operator edit on the next update.
4. Write **only** the record file. No target file changes, and no hooks run.
   The report classifies each identity against the disk as `matches`,
   `differs`, or `absent`, so the operator can tell whether `--at` was right.
   `--dry-run` shows the report and writes nothing.
5. An existing application for the same formal name is refused unless
   `--replace`, which is re-baselining.

Adoption doubles as **re-baselining**. When a recorded commit disappears
upstream (force-push), the update runs in fingerprint mode and names
`adopt T P --at <COMMIT> --replace` to restore full three-way merging.

Refusal was considered as the only answer to record-less projects, and it was
rejected. Adoption is cheap and changes nothing on disk, so refusing would
strand every 0.1.0 project with no gain in safety.

Fresh `apply T P` when the record already has an application for T behaves as
today: every Whole file exists, so the result is a conflict error. Its
`commands` now also names `update P`. `apply T P --force` still works and
**re-baselines**: it overwrites as today and replaces that application's record.
No existing capability is removed.

### Decision 4 — composition with the prerequisite contracts

**Content injection.** The update consumes `Plan::mutations()` on both plans
and the pub `PlannedEdit` fields (`body`, `desired`) for base and ours images.
Region replacement and placement reuse `resolve_region_edit`, and JSON
convergence reuses `resolve_json_edit`. Retraction and "merged body with
intended checksum" are the removal and three-way behaviours that injection
explicitly deferred to this design. They need two **additive** pure functions
in the injection module (see Open questions). No existing injection signature
or semantic changes. JsonValue stays convergent, and Region drift outside
`update` still refuses.

**Caller routes and answers identity.** Update follows the same three-route
grammar: person `update P`, agent `update P --async [F]`, script
`update P --answers F`. The route selects modality, and nothing inspects stdout
for a terminal. Update **never stages** because it is stateless and
re-derivable: each run recomputes everything from the record, so an agent
simply reruns with a fuller document. Update refuses when any staged interview
exists at P, as the scripted `apply` does. External answers documents keep the
exact `{template, answers}` envelope and go through `parse_and_verify`. The
**expected identity is the application's `template` field**. That makes the
applied record a third identity authority next to
`ResolvedTemplate.formal_name` and `StagedRecord.template`. This is an
additive row in that design's authority table, so it is raised in Open
questions. The recorded answers are replayed through the in-memory
`answer_headless` / `Pending::answer`. They are not re-parsed as an external
document: the record's own `template` field binds them. Binding answers to a
target and commit is exactly the design space headless-recovery left out of
scope. The applied record is where that binding lives. The XDG staged record is
untouched in shape and lifetime.

**Hook results.** The base render never runs hooks. File bodies cannot read
hook results (hook-results design), so the base is hook-independent by
construction. `UpdatePlan::apply` runs `next`'s hooks through the existing apply
hook loop, so deferred surfaces and the per-apply `HookResults` work unchanged
and are dropped at return. Nothing is persisted. Trust re-gates exactly as a
fresh apply: `NeedsTrust(UpdatePlan)` when hooks exist and the B surface is not
approved.

### Module and seam map

```text
                         CLI (src/main.rs, src/cli/*)
   update P [--async F | --answers F] [--template N] [--to C] [--on-conflict P] [--dry-run] [--trust]
   adopt  T P [--at C] [--replace] ...            apply T P (now records via project::apply)
          │                   │                                 │
          │ canonical_target  │ cli::resolve(formal) → T@B      │ source::materialize(address, commit)
          ▼                   ▼                                 ▼   (cache hit or gix fetch; None on failure)
 ┌──────────────── src/project/ (new, public `toha::project`) ─────────────────┐
 │ record.rs   ProjectRecord::open ─ WireApplication ─► Application  (boundary) │
 │             write_application (temp+rename; the commit point)                │
 │ update.rs   begin_update ─► baseline() ─► Plan::build(T@A)   [pure]          │
 │                         └─► Interview(T@B) + seed                            │
 │             Begun::plan ─► Plan::build(T@B) ─► read_disk ─► reconcile [pure] │
 │                                                   └─► merge3 ─► gix::merge   │
 │             UpdatePlan::apply ─► apply::commit_images ─► apply::run_hooks    │
 │ adopt.rs    adopt()  (render-only fingerprints; writes the record only)      │
 └──────────────────────────────────────────────────────────────────────────────┘
          │ consumes (read-only)                         │ reuses (pub(crate) split, no public change)
          ▼                                              ▼
 plan.rs  Plan::build, Plan::mutations, PlannedEdit    apply.rs  commit_images (atomic replace/delete),
          + PlanError::ReservedPath(.toha/)                      run_hooks (the one hook loop)
 inject   resolve_region_edit, resolve_json_edit       protocol.rs parse_and_verify via Begun::with_document,
          + retract_region, retract_json_value,                    answer_headless; result documents
            write_region_body (additive)                           gain `updated` / `adopted`
 interview.rs  untouched (pure)      staging.rs  untouched; canonical_target reused; staged-refusal check
 hook.rs  untouched                  registry.rs untouched (installed commit = default update target)
```

A trace from the CLI to a written byte touches `main.rs` → `project/update.rs`
→ `apply.rs`, three files (per minimize-reader-load). The engine, the planner,
and injection are called as libraries and are not layered through.

### Interface depth

The public surface is `ProjectRecord::open`, `begin_update`,
`Begun::{with_document, plan}`, `UpdatePlan::{changes, apply}`,
`project::apply`, and `project::adopt`, plus the result enums. Hidden behind it:

- record parsing and validation;
- T@A replay and re-rendering;
- per-identity base verification and degradation;
- the three-way join;
- the per-identity semantics for three ownership kinds;
- the merge engine and binary detection;
- composing Whole and bounded identities on one path;
- marker scanning;
- atomic commit and delete;
- hook orchestration;
- the commit-point record write.

The caller sees only what they must decide: which application, which
revision, answers for new questions, conflict policy, and trust. Wire types
(`WireApplication`) never cross the surface, per boundary-discipline. The one
exposed seam that a deeper design would hide is the caller-driven interview in
`Begun`. It is exposed so that every driver (terminal, headless, agent, crate)
keeps working with its own modality, and the pure engine stays the only answer
authority.

Invariants in types:

- A `CommitId` is always 40 lowercase hex characters.
- Region ownership cannot carry a second checksum.
- `ProjectRecord` has unique formal names and no public constructor.
- `UpdatePlan` exists only when every image is already resolved, so apply
  cannot fail on content.
- `FinalImage::Delete` is produced only by `Retract`, which requires a
  `BaseImage::Verified`.
- `Updated::UpToDate` carries nothing, so it cannot report writes.

What the system deliberately does not do:

- It does not detect renames. A path move is retract plus add, or
  release plus add.
- It does not search history for the commit that generated an adopted project.
- It keeps no "pending conflict" state.
- It creates no git commits in the operator's repository.

## Error and results contract

### `RecordError` (boundary; each carries the record file path)

| Variant | Condition |
|---|---|
| `Io { file, source }` | cannot list `.toha/applied/` or read a file |
| `Json { file, message }` | not valid JSON |
| `Shape { file, message }` | not an object, missing or unknown member, wrong member type |
| `Template { file }` | `template` empty |
| `Commit { file, value }` | `commit` is neither `null` nor exactly 40 lowercase hex characters |
| `Generated { file, value }` | `generated` does not parse as the frozen-instant format |
| `AnswerId { file, id }` | an answers key is not a valid `Id` |
| `Path { file, path, reason }` | absolute, contains `..`, enters `.git`, under `.toha/`, or not a valid `TargetPath` |
| `RegionKey { file, path, key }` | invalid `RegionKey` grammar |
| `JsonPath { file, path, json_path, message }` | invalid typed-path syntax |
| `Overlap { file, path, a, b }` | duplicate or ancestor/descendant JSON paths in one target (injection's rule) |
| `Fingerprint { file, value }` | not `sha256:` followed by 64 hex characters |
| `DuplicateApplication { template, files }` | two files name the same formal name |

A symlinked `.toha`, `.toha/applied`, or record file is `Path` with reason
`symlink`. The same guard applies as for target writes.

### `UpdateError`

| Variant | Condition | CLI kind / exit |
|---|---|---|
| `Record(RecordError)` | above | `record` / 1 |
| `NoRecord { target }` | no `.toha/applied/`; commands name `adopt` | `record` / 1 |
| `SelectApplication { available }` | several applications, no `--template` | `input` / 1 |
| `UnknownApplication { requested, available }` | `--template` names none | `input` / 1 |
| `Staged { commands }` | an interview is staged at the target | `staged` / 1 |
| `Resolve(ResolveError)` | T@B source resolution (ambiguity keeps exit 5) | `source` / 1 or `ambiguous` / 5 |
| `Plan(PlanError)` | T@B planning, including `ReservedPath` | `render` / 1 |
| `OwnershipCollision { identity, other }` | B claims an identity that another application in the record owns | `conflict` / 1 |
| `UnresolvedConflicts { paths }` | Toha marker lines remain in owned content | `conflict` / 1 |
| `Conflicts(Vec<Conflict>)` | any conflict under `Refuse`, or a binary / delete-modify / add-add conflict under `Markers` | `conflict` / 1 |
| `Region(RegionError)` / `Json(JsonEditError)` | injection's apply-time errors (missing anchor, malformed markers or JSON, traversal) | `render` / 1 |
| `Symlink(TargetPath)` / `Io { path, source }` | reading owned bytes or committing images | `input` / 1 |
| `Apply(ApplyError)` | the hook failures of the existing loop (`Hook`, `HookIo`, `Deferred`, `HookOutput`) | `hook` / 1 |
| `RecordWrite { source }` | the commit point failed; files are written. Rerunning converges. | `record` / 1 |

Interview outcomes are not errors. New or rejected questions go to a
`questions` batch with exit 4 on the agent route, or `questions` with errors on
the script route. The person route prompts. `Ended(Stop | Abort)` means no
update and exit 0, and nothing is staged.

Degradation is not an error. `Updated::Written.degraded` and the dry-run header
report `fingerprint mode: <reason>`. The reason is one of `unversioned`,
`commit unavailable`, `replay failed`, or `N identities unverified`. With
commit unavailable, the report also names `adopt --at --replace`.

### Results

| Outcome | Crate | Script document | Exit |
|---|---|---|---|
| nothing to do | `Updated::UpToDate` | `updated`, `changes: []` | 0 |
| written, no conflicts | `Updated::Written` | `updated` with `changes[{path, owner, action}]`, hooks, messages | 0 |
| written with markers | `Updated::Written { conflicted: [..] }` | `updated`, `conflicted` non-empty | **6** (new) |
| trust needed | `Updated::NeedsTrust` | `planned`, `trusted: false` | 3 |
| dry-run | `UpdatePlan::changes` | `planned` with update actions | 0 |
| adopt | `Adopted { report }` | `adopted` with `{identity, matches\|differs\|absent}` | 0 |

`owner` is `whole`, `region <key>`, or `value <json path>`. Each changed path
appears once in the written file list, as injection requires. The record file
never appears as a change, and dry-run lists it as `record .toha/applied/<file>`.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept that T@A must be re-fetchable (cache or network) for full
  three-way merging, in exchange for a record that stays tiny and reviewable
  and has one source of truth. When T@A is gone, fingerprint mode stays safe:
  it never deletes, and it refuses conflicts honestly.
- We accept that a moved project whose template renders its own absolute path
  loses verification for those identities. They fall back to fingerprint mode
  one by one. In exchange, the record never stores a location that goes stale.
- We accept freezing `now()` at first generation forever, in exchange for
  byte-idempotent updates. A template that wants "last updated" dates cannot
  get them from `now()` on update.
- We accept one JSON file per application under `.toha/applied/`. That is more
  files than a single lockfile. In exchange, independent template updates on
  divergent branches merge cleanly in git.
- We accept that JsonValue does not three-way merge. An operator change to an
  owned value is overwritten, as the injection contract requires, in exchange
  for not silently changing an approved semantic.
- We accept that a merged Region's checksum describes the template's intended
  body, not the bytes on disk. A plain `apply` keeps flagging that region as
  drifted until the operator reconciles it. That is honest, and it prevents
  silent overwrite.
- We accept that `Refuse` is the default conflict policy, even though git
  writes markers by default. In exchange, we keep Toha's existing
  "unchanged on conflict" contract and support targets that are not in git.
- We accept that update never stages, so an agent resends a cumulative
  document on each rerun. In exchange, there is zero new persistent state and
  no change to the staged-record shape.
- We accept that no renames are detected, in exchange for a join on exact
  identity keys.

## Alternatives considered

- **Store the base bodies in the record** (a snapshot of every Whole file,
  region body, and value). It hides T@A availability from the caller and makes
  every update fully three-way, even offline. It lost because it duplicates the
  template, balloons the tracked tree, and creates a second source of truth
  that drifts from the rendered truth. It also puts answers-derived content,
  possibly sensitive, into the repository a second time. Fingerprints give the
  verification and the safe fallback at 32 bytes per identity.
- **Git-native carrier: a commit trailer, or a `refs/toha/*` ref or note.**
  It hides the record from the working tree. It lost on the travel
  requirement: notes and refs are not cloned by default, and folder copies lose
  them. It would also make Toha author commits or refs in the operator's
  repository, a new effect with its own permission question. Non-git targets
  would lose updates.
- **Use git itself as the merge store: commit A's render to a hidden branch,
  then run a tree merge with T@B's render.** This is an excellent engine for
  whole files and would be deep. It lost because it collapses Region and
  JsonValue ownership into whole-file tree merges, contradicting the injection
  contract. It also requires the target to be a git repository and writes
  objects into it.
- **Single `.toha-applied.json` lockfile with an array of applications.** It is
  simpler to find. It lost to per-actor files on the shared-state axis:
  divergent branches updating different templates conflict in one file.
- **Refuse record-less projects.** Simplest. It lost because adoption writes
  nothing to the target, so refusal buys no safety and strands every existing
  project.

## Open questions and risks

1. **Injection additions.** Do you approve three additive pure functions in
   the injection module? They are:
   - `retract_region(current, path, key, mode: Excise | StripMarkers) -> Result<EditResolution, RegionError>`;
   - `retract_json_value(current, edit_path, format) -> Result<EditResolution, JsonEditError>`;
   - `write_region_body(current, edit, body, checksum_of: &str) -> Result<Vec<u8>, RegionError>`,
     which writes a merged or marked body under the checksum of the intended
     body.

   Injection deferred removal and three-way merging to this design, but these
   functions would live in its module. Is that acceptable as additive, or
   should they live in `project/`?
2. **Answers identity authority.** Headless-recovery lists two identity
   authorities. This design adds the applied record's `template` field as a
   third, for update answers documents. Do you approve that additive row?
3. **Hooks on update.** Update reruns all of B's hooks whenever anything
   changes. A scaffold hook such as `vcs init` or a dependency install may be
   wrong to repeat. Options:
   - (a) keep it as is, matching the grounding;
   - (b) expose a read-only `toha.update` flag in the Jinja context so authors
     can write `when: not toha.update`;
   - (c) add an authored hook attribute `on: [apply, update]`.

   Recommendation: (c), because it lives in the hook node, which the trust
   digest already covers. Options (b) and (c) touch the template-format and
   context contracts, so they need approval.
4. **Sensitive answers in the repository.** The record commits the completed
   answers to the project. Should a question be able to declare `persist:
   false`? Updates would then re-ask it every time, and base replay would need
   it too.
5. **New exit code 6** for "written with conflict markers." Do you approve,
   or should markers use exit 1 with status `updated`?
6. **`gix` merge feature.** Enabling `gix`'s `merge` feature adds the
   `gix-merge` crate. Its exact function path and signature in the 0.87 line
   must be confirmed at implementation, and this is the first task. If the
   surface is unsuitable, the fallback is `diffy`, a new direct dependency
   (<https://github.com/bmwill/diffy>), which needs the build-vs-buy check.
7. **`source::materialize(address, commit)`.** This fetches a specific
   historical commit of an address. A commit that is no longer reachable from
   any advertised ref (force-push) cannot be fetched by a shallow ref fetch. Is
   a full fetch of the source acceptable, or should an unreachable commit go
   straight to fingerprint mode?
8. **Risk: replay divergence.** Replaying the completed answer map through
   T@A could walk a different path than the original interview (flow `when`
   plus held answers). Per-identity fingerprint verification detects this and
   degrades safely. Implementation should confirm with a flow-heavy fixture.
9. **Risk: environment reads at render.** A template that reads the environment
   at plan time (under stage trust) makes the base non-reproducible. Those
   identities fail verification and degrade. That is safe, but it is a silent
   loss of merge quality. Should dry-run name the non-reproducible identities?
10. **Required trait additions.** `TargetPath`, `RegionKey`, and `JsonPath`
    need `Ord` derives for the sorted join. These are trivial, but they are
    edits to injection types.
11. **Fixture harness shape.** The harness needs a new shape:
    - `tests/fixtures/update-<name>/` with `template-a/`, `template-b/`,
      `answers.json`, an `edits/` overlay plus a `removed` list, `expected/`,
      and `expect.yml` (with `actions`, `conflicted`, `degraded`);
    - the harness commits A and B into a temporary git source, applies A,
      overlays the edits, updates to B, and compares the result with
      `expected/`;
    - it then updates **again** and asserts that zero bytes changed.

    The base is never a fixture input, because it is derived.

## Next implementation step

Enable `gix`'s `merge` feature and write `merge3` with its unit tests. The
tests cover clean merge, conflict with diff3 markers, identical changes on both
sides, and binary detection. This confirms the engine seam before any record or
reconcile code depends on it.
