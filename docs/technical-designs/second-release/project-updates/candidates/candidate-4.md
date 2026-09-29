# Problem

Toha needs to update a generated project from one resolved template commit to another while retaining operator edits. The update must run through the existing pure interview and plan flow, preserve the approved Whole/Region/JsonValue ownership distinctions, and write its applied state with the project. This is non-obvious because current plans describe only the new render and treat existing files as conflicts; the update needs a durable old-render base and ownership-aware reconciliation without confusing project state with XDG staging or per-apply hook results.

## Usage (caller's view)

README / quickstart:

```text
toha apply sample/widget ./widget --answers answers.json
toha update ./widget
```

The first apply writes a `.toha-project.json` record beside generated files. Update resolves the recorded formal template identity at the recorded commit, loads its accepted answers, resolves the current template commit, renders the new plan, and reconciles it against the recorded base and files on disk. Review conflicts, resolve them in the project, then run `toha update ./widget` again. The update does not ask interview questions.

Person route, using the approved route grammar:

```text
toha stage sample/widget ./widget
toha continue ./widget
toha apply sample/widget ./widget
toha update ./widget
```

Script route:

```text
toha apply sample/widget ./widget --answers answers.json
toha update ./widget
```

Crate call site:

```rust
let outcome = ProjectUpdate::prepare(&target, &resolver)?
    .reconcile(&ProcessRunner)?;
match outcome {
    UpdateOutcome::Applied(report) => println!("updated {} files", report.changed.len()),
    UpdateOutcome::Conflicted(report) => review(report.conflicts),
}
```

The crate caller receives structured conflicts and can choose when to retry; the public operation owns record parsing, resolution, rendering, merging, and atomic persistence.

## Shape

### Durable model

Use one project-root `.toha-project.json` as the authoritative applied record. It is ordinary version-controlled project content and survives clone, move, and checkout. It stores formal name, resolved commit, accepted answer map, target-relative source-root identity, and a map keyed by the approved ownership identity. Each ownership entry contains the exact bytes/value Toha last wrote for that identity (the merge base), plus encoding needed to reinsert it. Whole owns a file; Region owns marker pair and body; JsonValue owns a typed JSON value and path. For Whole entries the full baseline bytes are stored, including binary copied files; for Region only the owned span is stored; for JsonValue the canonical typed value is stored. The record itself is excluded from generated ownership and is replaced last using temp-file plus rename.

The record is parsed and validated once at the filesystem boundary into domain types. Inside update logic, keys cannot mismatch payloads and malformed records cannot become partial state. The accepted answers are the accepted inner answer map, not the external `{template, answers}` envelope and not staged submissions. The current resolved commit is always the version identity; no template `version` is inferred.

### Rust sketch

```rust
struct ProjectRecord {
    template: FormalName,
    commit: CommitId,
    answers: Answers,
    source_root: RelativePath,
    entries: BTreeMap<OwnershipId, AppliedEntry>,
}

enum OwnershipId {
    Whole(TargetPath),
    Region { path: TargetPath, key: RegionKey },
    JsonValue { path: TargetPath, path_key: JsonPath },
}

enum AppliedEntry {
    Whole { base: FileBytes },
    Region { base_body: String, start_marker: String, end_marker: String },
    JsonValue { base_value: serde_json::Value },
}

struct UpdateRequest<'a> {
    target: &'a Path,
    resolver: &'a dyn TemplateResolver,
}
struct PreparedUpdate {
    prior: ProjectRecord,
    next: ProjectRecord,
    merged: Vec<ReconciledFile>,
    hooks: Vec<PlannedHook>,
}
enum ReconciledFile {
    Write { path: TargetPath, bytes: FileBytes },
    Delete { path: TargetPath },
    Conflict(OwnershipConflict),
}
struct OwnershipConflict {
    id: OwnershipId,
    reason: ConflictReason,
    base: Option<FileBytes>,
    operator: Option<FileBytes>,
    proposed: Option<FileBytes>,
}
enum ConflictReason { TextOverlap, OperatorDeleted, OwnershipRemoved, InvalidRegion, InvalidJson }

enum UpdateOutcome { Applied(UpdateReport), Conflicted(UpdateReport) }

impl ProjectUpdate {
    fn prepare(target: &Path, resolver: &dyn TemplateResolver)
        -> Result<PreparedUpdate, UpdateError>;
    fn reconcile(self, runner: &dyn HookRunner)
        -> Result<UpdateOutcome, UpdateError>;
}
```

`Answers`, `FormalName`, `CommitId`, `TargetPath`, `RegionKey`, and `JsonPath` are existing or validated domain concepts; `OwnershipId` is derived from `Plan::mutations()`, not duplicated authoring metadata. Runtime record decoding validates path safety, version tag, ownership uniqueness, and payload-kind match once. Record format version is explicit; unsupported versions fail with upgrade guidance.

### Update semantics

For every identity in old or new plan, define `base` as the recorded old Toha output, `theirs` as current operator state, and `ours` as new Toha output.

- **Whole:** missing operator file is distinct from empty bytes. If `theirs == base`, write ours. Otherwise run `diffy`'s pure-Rust three-way text merge on UTF-8 text. Clean merge writes the merged result; markers/overlap return a conflict and do not modify that identity. Non-UTF-8 files use byte equality: unchanged replaces; edited conflicts. `diffy` is explicitly [diffy](https://github.com/bmwill/diffy), in-process and no subprocess. Text conflicts are presented as structured base/operator/proposed payloads; the operator edits the file and retries. No marker syntax is injected automatically.
- **Region:** locate and validate the keyed marker pair in operator text. If its current body equals base body, replace body and checksum with ours. Otherwise merge only the body with `diffy`; clean result is reinserted between the existing markers with a fresh checksum. Missing, duplicated, malformed, or overlapping region is a conflict. Operator text outside the marker span is always retained.
- **JsonValue:** parse current document with the approved embedded parser. Compare typed value at path. If operator value equals base, write ours; if ours equals base, retain operator value; if theirs equals ours, retain it. If both independently differ, report a value conflict. Re-serialize only the owned value through the approved source-preserving mutation path; comments, order, spacing, and unrelated keys remain intact. Invalid/missing path is a conflict unless the template newly creates the file/path.

A clean result updates the record base to the bytes/value Toha now owns and updates commit and answers together. A conflict leaves the applied record at the old commit, so retry uses the same base and does not accidentally accept partial output. Writes and record replacement are staged in memory, then each affected target is atomically replaced; filesystem multi-file atomicity is not promised. If a crash occurs mid-write, the old record remains authoritative; a retry detects changed files against old base and safely merges or reports conflict. For strict crash safety, implementation should use a journal in the project root; this is an open question because the task requires idempotence but does not require transactionality.

Identity handling is set-based. New identity: write ours (or conflict if an unrelated existing file occupies it). Removed identity: if operator state still equals recorded base, delete it; if operator changed/deleted it, return `OwnershipRemoved` conflict and preserve it. Same identity: apply the rules above. After success, update the record's identity set. Running update again at the same template commit with no edits is byte-no-op: plan output, merged files, and serialized record are deterministic; a no-change record write is skipped.

### Adoption

A record-less project is refused with guidance to generate a fresh project or restore its applied record. Re-interview cannot prove the old resolved commit, old rendered baseline, or accepted answer state, so it cannot construct a valid three-way base. No `--adopt` heuristic is included in this design.

### Flow and module seams

```text
CLI / crate caller
  └─ project_update::prepare
       ├─ record.rs: read + validate .toha-project.json once
       ├─ resolver: resolve recorded formal name at old commit, then current commit
       ├─ replay accepted Answers through pure interview completion
       ├─ Plan::build(old) + Plan::build(new) using same answers
       └─ reconcile.rs: Plan::mutations + recorded bases + disk snapshot
            ├─ Whole / Region → diffy three-way merge
            └─ JsonValue → approved injection source-preserving value mutation
  └─ project_update::reconcile
       ├─ return conflicts without writes, or
       ├─ existing hook trust gate and per-apply hook runner
       └─ apply.rs: atomic per-file writes, then applied record replacement
```

The public method hides format decoding, template resolution, merge mechanics, and persistence. Callers see only `prepare`, `reconcile`, and a report. The interface is intentionally two-step so a CLI can show a conflict report before deciding whether to retry, while no caller constructs merge bases or manipulates record wire types. The pure interview engine remains unchanged; all driver routes reach this new project-side operation without changing answer documents, staged records, or adapter modality.

Function seams:

```rust
fn load_record(root: &Path) -> Result<ProjectRecord, RecordError>;
fn record_from_plan(plan: &Plan, answers: &Answers, template: &ResolvedTemplate)
    -> Result<ProjectRecord, RecordError>;
fn render_at(template: &ResolvedTemplate, answers: &Answers, target: &Path)
    -> Result<Plan, UpdateError>;
fn reconcile_identity(
    id: &OwnershipId, old: Option<&AppliedEntry>, new: Option<&PlannedMutation>,
    disk: &DiskSnapshot,
) -> ReconciledFile;
fn persist_success(root: &Path, files: Vec<ReconciledFile>, next: ProjectRecord)
    -> Result<UpdateReport, UpdateError>;
```

Initial apply must create the record from the exact rendered plan and accepted answer map as part of successful apply. Update hooks execute only after conflict-free reconciliation, under the existing trust gate; hook results remain in the apply stack and never enter the record. Update re-renders without stored hook output, as required by the one-apply hook-results contract.

### Error/results contract (exhaustive)

- `RecordMissing`: no record; message says update requires a project created by a recording Toha apply; no interview is attempted.
- `RecordMalformed`: invalid JSON, unsupported record version, unsafe path, duplicate identity, or mismatched ownership payload; report record path and field, never continue with partial data.
- `TemplateUnavailable`: old or current commit cannot be resolved locally/remotely; no writes.
- `TemplateIdentityMismatch`: resolved formal name differs by exact string from record; no writes.
- `AnswerReplay`: stored accepted answers cannot complete the old/current pure interview; no writes.
- `Plan`: old/new render failure, including path and content rendering errors; no writes.
- `DiskRead`: permission/I/O failure while snapshotting relevant target paths; no writes.
- `MergeConflict`: returned as `UpdateOutcome::Conflicted`, not a fatal error; includes identity, reason, and base/operator/proposed descriptions. Record and files remain untouched for conflicting identities; clean identities are also not written when any conflict exists.
- `ApplyTrust`: existing NeedsTrust behavior; no files or record are written before approval.
- `Hook`: existing hook failure behavior; file writes may already have occurred under the established apply contract, but record is not advanced, making retry reconcile from old base.
- `Write`: atomic replacement or record write failed; record remains old if failure precedes final record rename; recovery re-runs comparison.
- `Applied`: changed paths, deleted paths, new commit, and whether the record changed. If bytes and record are unchanged, result is `Applied` with empty changed/deleted paths.

No error or debug output includes stored answer values or whole file contents. Conflict payloads are available only to the caller through the explicit report.

## Tradeoffs accepted

- We accept a readable, operator-editable project-root record in exchange for clone-safe provenance without requiring project Git history.
- We accept duplicated baseline content in the record in exchange for a self-contained merge base that survives template cache cleanup.
- We accept conservative conflicts for non-text Whole files in exchange for never corrupting binary content with a text merge.
- We accept refusing adoption in exchange for never inventing an old commit or a false merge base.
- We accept non-transactional multi-file writes in exchange for keeping the first implementation aligned with existing per-file apply semantics; the old record makes retries detectable.
- We accept adding one dependency, `diffy`, in exchange for in-process text three-way merge without application subprocesses.

## Alternatives considered

- **Git commit trailer:** travels only with project history and requires a commit to be made at apply time or a trustworthy HEAD association. It exposes Git repository/history assumptions to every caller and loses state when generated files are copied outside that history.
- **Custom git ref or note:** good for repository-local metadata, but refs/notes can be omitted by ordinary clone/fetch and are not part of a checked-out project tree. This pushes synchronization responsibility to operators and hides state from ordinary project tools.
- **Re-interview for adoption:** exposes interview and answer reconstruction to callers but cannot recover the old commit or Toha-authored base bytes. It is not a valid three-way update.
- **Whole-file replacement plus `--force`:** shallow API but pushes conflict policy to operators and discards edits on every update, so it fails the core requirement.

## Open questions and risks

- Should the applied record include a compressed blob store or content-addressed baseline table if generated projects are large, or is one baseline per ownership identity acceptable?
- Does the existing apply implementation expose a post-render hook plan and accepted answer map at the seam needed to write the initial record without changing `Plan` or `Applied` contracts?
- Can the approved JsonValue mutation seam be called by the update reconciler to replace a value while preserving source text, or does its contract need a new operation? Any interface change must be approved explicitly before implementation.
- Should all hook execution happen before file writes for update, or retain current apply ordering where hooks can fail after writes? Advancing the record only after hook success means hook failure causes a retry against the old baseline.
- Is `diffy`'s conflict marker / output behavior suitable for the CLI report, or should `diffy` be used only for merge decisions with the application constructing byte-free structured conflict reports?
- Multi-file crash consistency is limited: could a compact update journal be added before users rely on updates, or is retry-based recovery sufficient?
- Current answer storage may include sensitive values. Should the applied record be plaintext project content, or should answers be limited to values needed to re-render? Limiting them would require a distinct replay contract.

## Next implementation step

Prototype the domain record and pure `reconcile_identity` function against Whole, Region, and JsonValue fixtures, using `diffy` for Whole and Region text merges before connecting it to CLI routes.
