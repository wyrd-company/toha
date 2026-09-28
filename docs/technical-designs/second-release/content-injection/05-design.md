# Content injection into existing files — final design

Design 1068, paired implementation 1031, epic 1065 (Toha 0.2.0). Synthesized
from the arena (base: candidate-1 managed-region markers; grafts and rejections
in `04-synthesis.md`). Sketch only — `not implemented` bodies and proposed
contract edits in prose; no production code, no canonical schema/spec edits in
this package (1031 owns those).

## Problem

Today every planned mutation replaces a whole file: `Content` is
`Rendered(String)` or `Copied(PathBuf)`, and `Plan::add` treats a target's
*existence* as a conflict (`plan.rs:295`). Content injection is the first write
*into a file Toha does not own* — a bounded region of an existing file, the rest
left to its owner. Five constraints hold at once (from `01-grounding.md`): the
interview engine stays pure and staging replay reproduces the same result;
injection output is a pure function of `(Template, Completed, target state at
apply time)`; the `TargetPath` / `has_symlink_component` / `.git` guards do not
weaken; no new timeout, permission, subprocess, or pinned-check mechanic; and
injection composes with whole-file writes inside one `Plan`. Heuristic
idempotency (re-matching the injected text after the user edits around it) is
forbidden, and a jq dependency must not dictate the core.

Managed-region markers resolve all five: the marked span is a self-describing,
exactly-bounded unit that re-apply finds by key and replaces, so idempotency and
ownership are structural facts.

## Usage (caller's view)

### Template author (`template.yml`)

A new top-level `inject` list, sibling to `files`. Each entry names the target,
a stable region key, and the body to place; everything else is optional.

```yaml
name: service-scaffold
interview:
  - id: add_router
    type: confirm
    prompt: Add HTTP routes?
    default: true
  - id: features
    type: multiselect
    prompt: Feature modules
    options: ["health", "metrics"]

# Whole-file generation — unchanged.
files:
  - each: "[1] as _"
    source: support/main.rs
    path: "src/main.rs"

# NEW: bounded regions written into files Toha does not own.
inject:
  - into: "src/app.rs"            # target -> TargetPath (same guards as files)
    region: "routes"             # stable key; partitions one file's regions
    when: "add_router"
    source: support/routes.rs     # body from a support file (rendered)
    # comment omitted -> inferred from `.rs` -> `// ...`
    anchor: { after: "fn build_router() {", occurrence: only }  # first placement only

  - into: ".gitignore"
    region: "build-artifacts"
    content: |                    # inline body (rendered); mutually exclusive with source
      /target
      *.tmp
    # no anchor -> first placement appends at end of file
    create: true                  # allow creating .gitignore if absent (default false)

  - into: "config/app.toml"
    region: "features"
    each: "features as feature"   # one entry may fan out to N regions (keys suffixed by item)
    marker: { open: "#", close: "" }   # explicit override instead of inference
    content: "{{ feature }}_enabled = true"
```

First apply wraps the rendered body in comment-delimited markers and places it
(at the anchor, or at end of file):

```rust
fn build_router() {
// >>> toha:region routes >>>
    router.route("/health", get(health));
// <<< toha:end routes sha256:9f2c… <<<
    router.finish()
}
```

A second apply with the same answers is a **no-op**: Toha re-renders the body,
finds the region by key, sees the on-disk body equals the desired body, writes
nothing. A changed template body replaces only the span between the markers. A
body the *user* edited inside the markers is drift — Toha refuses and names
`--force`, exactly as whole-file conflicts do.

### Crate / CLI view

The crate surface grows by one field and one pure function; existing
`Plan::build(..).apply(..)` callers are unchanged.

```rust
use toha::{Plan, ApplyOptions, Applied};

let plan = Plan::build(&template, &completed, target)?;   // also fills plan.edits
match plan.apply(target, ApplyOptions { force: false, trusted: true }, &runner)? {
    Applied::Written { files, .. } => { /* `files` includes injection targets that changed */ }
    Applied::NeedsTrust(plan)      => { /* unchanged: hooks still gate on trust */ }
}
```

CLI behavior:

- `toha apply <PATH>` — whole-file writes first, then injections. A region that
  already matches prints nothing (idempotent). A first-placed or changed region
  prints its path (`apply` prints each written path,
  `command-line-interface.yml`).
- `toha apply --dry-run <PATH>` — extends the `create <path>` vocabulary with
  `inject <path> (<region>)` for first placement and `update <path> (<region>)`
  for a changed region; unchanged regions print nothing; drift/missing-anchor
  print as the error they would raise. Dry-run reads target bytes (consistent
  with reading the target listing today).
- `toha apply <PATH>` with a drifted region and no `--force` — writes nothing,
  lists each drifted `path (region)` after `region drifted:`, names the same
  command with `--force`, exit 1 (mirrors the conflict gate).
- Missing anchor → exit 1 (names the anchor text). Ambiguous anchor
  (`occurrence: only`, >1 match) → exit 5 (the existing "ambiguous" code).

## Shape

Data structures first.

### Injection is a sibling to `PlannedFile`, not a `Content` variant

Whole-file writes carry the bytes; an injection carries *intent resolved against
the target's current bytes at apply time*. Folding it into `Content` would put
"I must read the target" behind a type that today means "I already hold the
bytes," leaking the read requirement into every `Content` match (`per
boundary-discipline`). A sibling keeps each type honest. (Candidate-4's unified
enum was rejected for blast radius and a whole-file semantics regression; its
*downstream contract* instinct is preserved as a derived view below.)

```rust
// src/plan.rs — additions

pub struct Plan {
    pub files: Vec<PlannedFile>,     // whole-file writes (unchanged)
    pub edits: Vec<PlannedEdit>,     // NEW: region injections, applied after files
    pub conflicts: Vec<TargetPath>,  // whole-file existence conflicts (unchanged)
    pub hooks: Vec<PlannedHook>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}

/// One managed region to write into an existing file. A pure function of
/// (Template, Completed): the *rendered* body plus marker/anchor policy, but
/// NOT the target's bytes. Bytes are read only when this edit is resolved at
/// apply / dry-run time, never in the engine.
pub struct PlannedEdit {
    pub path: TargetPath,     // same newtype + `.git`/symlink guards as files
    pub region: RegionKey,    // stable key; keys partition one file's regions
    pub body: String,         // rendered content between the markers (no markers)
    pub marker: MarkerStyle,  // comment delimiters, resolved at build time
    pub anchor: Option<Anchor>, // first-placement only; None => end of file
    pub create: bool,         // may apply create the file if absent? (default false)
    pub source: Option<PathBuf>, // support file, for diagnostics; None for inline
}

/// `^[a-z0-9_-]+$`, validated at build; embedded verbatim in one marker line.
/// Two edits to one path with the same key are a build-time `Duplicate`.
pub struct RegionKey(String);

/// Line/block comment delimiters (a line style has `close == ""`). Resolved from
/// the author's `comment`/`marker` or inferred from the target extension; an
/// unknown extension with no override is a build error.
pub struct MarkerStyle { pub open: String, pub close: String }

/// Where a region goes the FIRST time (no markers yet). Once markers exist THEY
/// are the selection, so anchor ambiguity/absence can only fail on first apply.
pub struct Anchor { pub matcher: AnchorMatch, pub place: Place, pub occ: Occurrence }
pub enum AnchorMatch { Literal(String), Regex(regex::Regex) }  // compiled at build
pub enum Place { Before, After }
pub enum Occurrence { First, Last, Only }  // `Only`: >1 match => ambiguous (exit 5)
```

On-disk managed region — the single source of truth for ownership and drift:

```text
{open} toha:region {key} {close}\n            <- begin marker, its own line
{body}\n                                       <- Toha's body, verbatim
{open} toha:end {key} {algo}:{hex} {close}\n   <- end marker records body checksum
```

`{hex}` is a `sha2` digest of the body bytes Toha wrote (`{algo}` names it, e.g.
`sha256`, so the algorithm can evolve). `sha2` is already a dependency
(`Cargo.toml:43`); no new crate. The checksum is what turns drift detection into
a fact: it distinguishes "the on-disk body is the one Toha last wrote" (safe to
replace) from "the user changed it" (drift).

### The load-bearing pure function

Apply and dry-run both route through one function. It reads no file; the caller
passes the current bytes. Idempotency, drift, and first placement are decided
here, and it is trivially fixture-testable (`per make-operations-idempotent`).

```rust
/// Whole-file outcome of resolving one edit against the target's current bytes.
/// `current == None` means the file is absent.
///
/// Purity: `resolve_edit(read(path), edit)` depends on nothing else, so
/// replaying a staged interview against a once-applied file yields `Unchanged`.
pub enum Resolution { Unchanged, Write(Vec<u8>) }

pub fn resolve_edit(current: Option<&[u8]>, edit: &PlannedEdit)
    -> Result<Resolution, EditError>
{
    // 1. Absent file: create (markers at EOF of empty content) iff `create`,
    //    else EditError::TargetMissing.
    // 2. Non-UTF-8 file: EditError::NotUtf8 (markers are text).
    // 3. Locate the region by its exact begin/end marker lines for `edit.region`:
    //      begin without end / two begins / unparseable end => MalformedRegion.
    //      found -> split into (before, body_on_disk, recorded_hash, after):
    //          body_on_disk == edit.body           => Unchanged     // idempotent
    //          sha(body_on_disk) == recorded_hash  => Write(rebuilt) // template changed
    //          else                                => EditError::Drift // user edit
    //      not found -> FIRST PLACEMENT:
    //          anchor None            => append region at end of file
    //          anchor Some, 1 match   => insert before/after the match
    //          anchor Some, 0 matches => AnchorMissing
    //          anchor Some, >1 & Only => AnchorAmbiguous
    // 4. `Write(bytes)` carries the FULL new file bytes with a freshly computed
    //    end-marker checksum, so the next resolve sees Unchanged. Splicing
    //    preserves every byte outside the region verbatim (line endings, final
    //    newline): the region is replaced by byte range, never via
    //    lines().join(), which would renormalize the rest of the file.
    todo!("not implemented")
}

pub enum EditError {
    TargetMissing   { path: TargetPath, region: RegionKey },
    NotUtf8         { path: TargetPath },
    AnchorMissing   { path: TargetPath, region: RegionKey, anchor: String },
    AnchorAmbiguous { path: TargetPath, region: RegionKey, count: usize },
    Drift           { path: TargetPath, region: RegionKey },
    MalformedRegion { path: TargetPath, region: RegionKey, why: String },
}
```

**Invariant (the reason idempotency holds):** `Write` always recomputes the
end-marker checksum over the body it emits. Therefore
`resolve_edit(bytes_written_by_a_prior_Write, same_edit) == Unchanged`. The
double-apply acceptance fixture is a direct consequence and survives staging
replay because `PlannedEdit` is a pure function of `(Template, Completed)`.

### Build path (pure over template + listing)

`Plan::build` gains a loop mirroring the `files` loop (`plan.rs:200`). It renders
each injection rule into `PlannedEdit`s — resolving the target through the same
`TargetPath::parse`, rendering the body from the support file (`read_render`) or
inline template, resolving the marker style, compiling the anchor. It reads **no
target bytes**, recording intent only. `Plan::add_edit` checks
`has_symlink_component` and rejects a duplicate `(path, region)` pair, the same
way `add` guards files (`plan.rs:272`). It also rejects a rendered body that
contains a line equal to this region's marker (a corruption vector).

```rust
// src/template.rs — new domain type, new Builder branch
pub struct InjectionRule {
    pub into: Tmpl,
    pub region: Tmpl,
    pub body: BodySource,        // Support(PathBuf) | Inline(Tmpl)
    pub marker: MarkerSpec,      // Infer | Family(CommentFamily) | Raw { open, close }
    pub anchor: Option<AnchorSpec>,
    pub create: Typed<bool>,
    pub each: Option<Each>,      // reuses `Each`; one entry -> N regions, key per item
    pub when: Option<Expr>,
}
// Template gains: pub injections: Vec<InjectionRule>,

/// Resolve comment delimiters at build (path known once rendered). Extension
/// table -> comment family (`#`, `//`, `/* */`, `<!-- -->`, `--`, `;`, `%`).
/// Unknown extension with `MarkerSpec::Infer` is a load/build error naming `comment`.
fn resolve_marker_style(path: &TargetPath, spec: &MarkerSpec)
    -> Result<MarkerStyle, PlanError> { todo!("not implemented") }
```

### Apply path (read-modify-write, one write per file)

`apply_reporting` (`apply.rs:66`) gains a stage between the conflict gate and the
file writes, preserving the "write nothing on conflict / needs-trust" gate:

```rust
// after the existing whole-file conflict recompute (apply.rs:73):
let mut resolved: Vec<(TargetPath, Vec<u8>)> = Vec::new();  // only the changed ones
let mut drift: Vec<(TargetPath, RegionKey)> = Vec::new();
for edit in &self.edits {
    let current = read_optional_bytes(target, &edit.path)?;   // read once
    match resolve_edit(current.as_deref(), edit) {
        Ok(Resolution::Unchanged)    => {}                     // idempotent skip
        Ok(Resolution::Write(bytes)) => resolved.push((edit.path.clone(), bytes)),
        Err(EditError::Drift { path, region }) => drift.push((path, region)),
        Err(other) => return Err(other.into()),                // hard error, nothing written
    }
}
if !options.force && (!conflicts.is_empty() || !drift.is_empty()) {
    return Err(ApplyError::from_conflicts_and_drift(conflicts, drift));  // writes nothing
}
// with --force: re-resolve drifted edits as forced Write, fold into `resolved`
// trust gate; symlink checks for files AND edit paths (unchanged in spirit)
// write whole-file `files` first, then each `resolved` edit
```

Each `Write` goes to a sibling temp file in the target directory and is renamed
into place (atomic), so a crash cannot leave a **user-owned** file half-written
(D7). `on_written` fires once per changed path, preserving "print each written
path before hooks" (`apply.rs:151`).

**Ordering / coexistence.** Whole-file `files` are written first, then `edits`,
so an injection can target a file a whole-file rule just wrote. A `PlannedFile`
and a `PlannedEdit` to the same path are allowed and ordered (write, then
inject) — a legitimate "generate then manage a region" flow, so it is *not*
refused (candidate-3's blanket `InvalidMixture` was rejected). Two edits to one
path with different keys apply in list order, each independently idempotent. Two
edits to one path with the *same* key are a build-time `Duplicate`.

### The mutation contract (for 1066 / 1031)

The plan is the single source of truth for what Toha owns in each target file;
1066 and 1031 consume a **derived** view, not a synced copy (`per
single-source-of-truth`). The view carries candidate-4's ownership vocabulary.

```rust
/// What Toha claims to own in the target tree. Derived from the plan.
pub enum FileMutation<'a> {
    /// Whole file: re-apply replaces it wholesale; owner is Toha.
    Whole  { path: &'a TargetPath },
    /// Marked region: re-apply replaces only the marked span; the rest is the
    /// file owner's. The end-marker checksum identifies the body Toha last
    /// wrote, so an update/replay can tell a safe re-apply from owner drift.
    Region { path: &'a TargetPath, region: &'a RegionKey },
}
impl Plan {
    pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>> { todo!() }
}
```

**Stated for 1066:** *Toha owns exactly the byte span between and including a
begin/end marker pair keyed by region id; everything else in that file belongs
to its owner. The end marker's checksum identifies the body Toha last wrote.* An
update or replay reasons per region: a region whose on-disk body still matches
its recorded checksum is safe to re-apply; a drifted region is a conflict. A
whole-file mutation replaces the file wholesale, as today.

### Error / results contract

- `Applied::Written.files` includes injection targets that changed; unchanged
  regions never appear.
- New `ApplyError` arms: `Drift(Vec<(TargetPath, RegionKey)>)` (message names
  `--force`, exit 1); `AnchorMissing` / `MalformedRegion` / `TargetMissing` /
  `NotUtf8` (exit 1, name file/region); `AnchorAmbiguous` (exit 5). `--force`
  governs **drift overwrite only** — it never invents placement, so
  anchor/malformed/missing-target errors persist even with `--force`. `--force`
  never silently rewrites a file Toha does not own outside its own markers.
- `PlanError` reuses `Duplicate` for `(path, region)` collisions and
  `Path`/`Render` for bad keys and unresolvable marker styles.

### Interface depth

Public surface added: one `template.yml` list (`inject`), one `Plan` field
(`edits`), one `FileMutation` view, one pure function (`resolve_edit`). Behind
that small surface sit marker synthesis, per-extension comment inference, anchor
placement, checksum drift detection, byte-level idempotency, and crash-safe
writes. Callers that already `build().apply()` are untouched; authors write a
few lines of YAML. Complexity is pulled into the callee (`per
boundary-discipline`); `MarkerStyle` is resolved before it reaches `PlannedEdit`,
so comment concerns never surface on the public type.

## Synthesis decision

Base: candidate-1 (managed-region markers), which scored 30/30 (judge 29/30) and
led every criterion. Grafted: candidate-4's downstream ownership vocabulary as a
*derived* `FileMutation` view (not its internal enum collapse or build-time byte
offsets); candidate-3's "owns the value at a named path" ownership as a reserved,
out-of-scope structured mode; candidate-2's anchor cardinality naming and its
exact-bytes lesson (splice, don't `lines().join`). Rejections and scores in
`04-synthesis.md`.

## Tradeoffs accepted

- **Visible marker comments in the user's file, in exchange for structural
  idempotency and exact ownership.** Removing them forces a heuristic re-match,
  which the grounding forbids.
- **A `sha2` checksum in the end marker, in exchange for non-heuristic drift
  detection.** No new dependency (`sha2` is already used). Without it Toha cannot
  tell a template-body change from a user edit.
- **A per-extension comment table with an explicit override, in exchange for
  working across arbitrary text files.** There is no universal comment; the table
  plus `comment`/`marker` covers the tail and fails loudly on the unknown.
- **`edits` as a separate `Plan` vector, not a `Content` variant**, keeping "I
  hold the bytes" and "I must read the target" distinct, non-leaky types.
- **Anchors are bootstrap-only**, so ambiguity can only bite on first placement;
  every later apply selects by marker and keeps working even if surrounding text
  moves.
- **Atomic temp-file-plus-rename for injection writes**, diverging from today's
  direct `fs::write` (`apply.rs:112`) because a partial write to a *user-owned*
  file is worse than to a generated one.

## Alternatives considered

- **Structured merge via jaq / serde (candidate-3).** Deep for structured
  targets, but reserializes (drops comments, key order, formatting) — a disguised
  whole-file rewrite of a user-owned file — and cannot touch arbitrary text.
  Reserved as an optional, out-of-scope mode; not the core.
- **Anchor-only, re-matching injected text on re-apply (candidate-2 core).**
  Smaller footprint, but idempotency becomes a heuristic that misfires once the
  user edits nearby. Rejected; its cardinality vocabulary was kept.
- **A third `Content::Injected` variant.** Puts a "reads the target" requirement
  behind a type whose other variants hold complete bytes; shallower, leaks.
  Rejected for the `edits` sibling.
- **One unified `FileMutation` enum as the internal representation
  (candidate-4).** Changes whole-file conflict semantics (regression risk) and
  enlarges blast radius; its build-time byte offsets are not replay-safe. Kept
  only as a derived read-model.
- **A separate `toha inject` subcommand / second apply pass.** Temporal
  decomposition breaking the single-`Plan` contract and the write-order print
  contract. Rejected.

## Decisions needed from Bob (with recommendations)

- **D1 — Visible markers.** Toha writes comment-delimited markers into
  user-owned files as the ownership record. *Recommend: yes* — the mechanism.
- **D2 — Author surface.** New top-level `inject` list, sibling to `files`, with
  `into`/`region`/`body`(source|content)/`marker`/`anchor`/`create`/`each`/`when`.
  *Recommend: adopt as shown.*
- **D3 — Comment-style inference + override**, unknown extension without override
  is a load error. *Recommend: yes.*
- **D4 — Drift policy / `--force` extension.** Drift refuses without `--force`
  (exit 1); `--force` overwrites the managed region only, never fabricates
  placement. This *extends* `--force` to also govern drift overwrite (a new
  behavior for `--force`, not a restriction). *Recommend: adopt.*
- **D5 — `create` default.** `create: false` by default (injection targets an
  existing file). *Recommend: false.*
- **D6 — Structured-merge mode out of scope for 1031**, reserved for a future
  design. *Recommend: yes.*
- **D7 — Atomic temp-file+rename for injection writes.** *Recommend: yes
  (injection only; whole-file writes unchanged).*
- **D8 — Reuse existing `sha2`** for the region checksum. *Recommend: yes (no new
  dependency).*

**Explicit disclosure for the checkpoint.** This design introduces **no**
supported-capability restriction (it only adds capability; `--force` is
extended, not narrowed), **no** permissions/access change, **no** timeout
mechanic, **no** pinned version check, and **no** application-subprocess
integration. If Bob's ruling on any decision changes that, the affected item
returns to this checkpoint for separate explicit approval.

## Proposed canonical-contract edits (described, not made here — owned by 1031)

- `docs/specifications/template-format.yml` + `template-format.schema.yml`: add
  the `inject` list and its fields; extend the "Files" narrative with an
  "Injection" section (regions, markers, anchors, idempotency, drift).
- `docs/specifications/command-line-interface.yml`: extend "Overwriting" and the
  dry-run vocabulary (`inject`/`update`/`region drifted:`) and the `--force`
  semantics for drift; keep the exit-code table (5 gains anchor-ambiguity).
- `docs/technical-designs/architecture.yml`: extend `plan-and-apply` with
  `PlannedEdit`/`resolve_edit`/`FileMutation` and the marker ownership model.
- `docs/` guides (`template-files.md`) and a fixture under `tests/fixtures/`:
  the twice-apply acceptance fixture and the scenario matrix in `04-synthesis.md`.

## Behaviors to prove (1031 fixtures; how each can fail)

1. **Twice-apply changes the file once** (sole idempotency assertion): fails if
   the second apply rewrites bytes or duplicates the region.
2. First placement at anchor / at EOF: fails if placed at the wrong offset.
3. Template-body change replaces only the region: fails if bytes outside the
   markers change.
4. User drift refuses without `--force` (exit 1) and overwrites with `--force`:
   fails if drift is silently clobbered or `--force` fabricates placement.
5. Missing anchor exit 1; ambiguous (`only`) exit 5: fails if the wrong code or a
   silent first-match.
6. Whole-file write + injection into one file in one plan: fails if order is
   wrong or the injection reads pre-write bytes.
7. Arbitrary text targets (`.gitignore`, `.rs`) exercise comment inference:
   fails on unknown-extension silent guess.
8. Staged-replay double-apply equals single apply: fails if replay is not a
   no-op.

## Next implementation step

Write `resolve_edit` and its byte-range region parser against fixtures —
first-placement at EOF, first-placement at anchor, byte-identical re-apply
(`Unchanged`), template-body change (`Write`), user drift (`Drift`) — since that
pure function is the whole contract and every apply / dry-run / mutation path
depends on it.
