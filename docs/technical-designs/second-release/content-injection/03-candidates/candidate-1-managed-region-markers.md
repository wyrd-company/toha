# Content injection — managed-region markers

Candidate design for Toha 0.2.0 content injection (design 1068, paired
implementation 1031). Center: Toha writes begin/end sentinel markers around each
injected region. Re-apply replaces the bytes between the markers. Idempotency is
by construction; ownership is exactly the span between the markers.

## Problem

Today every planned mutation replaces a whole target file: `Content` is
`Rendered(String)` or `Copied(PathBuf)`, and `Plan::add` treats *existence* as
conflict (`plan.rs:295`). Content injection is the first write into a file Toha
does **not** own — a bounded region of an existing file, the rest left to its
owner. The shape is non-obvious because five constraints must hold at once: the
interview engine stays pure and staging replay must reach the same result
(`interview.rs`, `staging.rs:128`); injection output must be a pure function of
`(Template, Completed, target-directory state at apply time)`; the
`TargetPath`/`has_symlink_component`/`.git` guards must not weaken (`plan.rs:18`,
`plan.rs:71`); no new timeout/permission/subprocess/pinned-check mechanic; and
injection must compose with ordinary whole-file writes inside one `Plan`. The
grounding also forbids *heuristic* idempotency that depends on the injected text
still matching after the user edits around it, and forbids letting a jq
dependency dictate the core contract.

Managed-region markers resolve all five. The marked span is a self-describing,
exactly-bounded unit: re-apply finds it by key and replaces its bytes, so
idempotency and ownership are structural facts, not heuristics.

## Usage (caller's view)

### Template author (`template.yml`)

A new top-level `inject` list, sibling to `files`. Each entry names the target
file, a stable region key, and the body to place. Everything else is optional.

```yaml
name: web-service
interview:
  - id: use_router
    type: confirm
    prompt: Add HTTP routes?
    default: true
  - id: features
    type: multiselect
    prompt: Feature modules
    options: ["health", "metrics"]

# Whole-file generation, unchanged.
files:
  - each: "[1] as _"
    source: support/main.rs
    path: "src/main.rs"

# New: bounded regions written into files Toha does not own.
inject:
  - into: "src/app.rs"              # target (template -> TargetPath)
    region: "routes"               # stable key (template -> RegionKey)
    when: "use_router"
    source: support/routes.rs.j2   # body from a support file (rendered)
    # comment omitted -> inferred from `.rs` extension -> `// ...`
    anchor:                        # first placement only; ignored once markers exist
      after: "fn build_router() {"
      occurrence: first

  - into: ".gitignore"
    region: "build-artifacts"
    content: |                     # inline body (rendered); mutually exclusive with source
      /target
      *.tmp
    # no anchor -> first placement appends at end of file
    create: true                   # allow creating .gitignore if absent

  - into: "config/app.toml"
    region: "features"
    each: "features as feature"    # one region... no: one entry, one region key
    marker: { open: "#", close: "" }   # explicit override instead of `comment`
    content: "{{ feature }}_enabled = true"
```

On the first apply, Toha wraps the rendered body in comment-delimited markers and
places it (at the anchor, or at end of file). The result in `src/app.rs`:

```rust
fn build_router() {
// >>> toha:region routes >>>
    router.route("/health", get(health));
// <<< toha:end routes sha256:9f2c... <<<
    router.finish()
}
```

A second apply with the same answers is a **no-op**: Toha re-renders the body,
finds the region by key, sees the on-disk body already equals the desired body,
and writes nothing. If the template's body changes, Toha replaces only the span
between the markers. If the *user* edited inside the markers, that is drift —
Toha refuses and names `--force`, exactly as whole-file conflicts do.

### Crate / CLI view

The public crate surface grows by one field and one pure function; callers that
already do `Plan::build(...).apply(...)` need no change.

```rust
use toha::{Plan, ApplyOptions, Applied};

let plan = Plan::build(&template, &completed, target)?;   // now also fills plan.edits
match plan.apply(target, ApplyOptions { force: false, trusted: true }, &runner)? {
    Applied::Written { files, .. } => { /* `files` includes changed injection targets */ }
    Applied::NeedsTrust(plan)      => { /* unchanged: hooks still gate on trust */ }
}
```

CLI behavior:

- `toha apply <PATH>` — whole-file writes happen first, then injections. A region
  that already matches produces no output line (idempotent). A changed or
  first-placed region prints its path (`apply` prints each written path,
  `command-line-interface.yml`).
- `toha apply --dry-run <PATH>` — extends the existing `create <path>` vocabulary
  with `inject <path> (<region>)` for first placement, `update <path> (<region>)`
  for a changed region; unchanged regions print nothing. A drifted region or a
  missing anchor prints as the error it would raise.
- `toha apply <PATH>` on a drifted region without `--force` — writes nothing,
  lists the drifted `path (region)` after `region drifted:`, and names the same
  command with `--force`. Exit 1, mirroring the conflict gate.
- Missing anchor → exit 1 (names the anchor text). Ambiguous anchor → exit 5, the
  existing "ambiguous" code.

## Shape

### Data structures (first)

Injection is a **third planned-mutation shape**, a sibling to `PlannedFile`, not a
third `Content` variant. Whole-file writes carry byte content; an injection
carries *intent that is resolved against the target's current bytes at apply
time*. Folding it into `Content` would put "I need to read the file" behind a
type that today means "I already hold the bytes," leaking the read requirement
into every `Content` match. A sibling keeps each type honest (`per
boundary-discipline`).

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
/// (Template, Completed): it holds the *rendered* body and the marker/anchor
/// policy, but NOT the target's bytes. The bytes are read only when this edit is
/// resolved at apply/dry-run time (`resolve_edit`), never in the engine.
pub struct PlannedEdit {
    pub path: TargetPath,           // same newtype + `.git`/symlink guards as files
    pub region: RegionKey,          // stable key; keys partition one file's regions
    pub body: String,               // rendered content between the markers (no markers)
    pub marker: MarkerStyle,        // comment delimiters, resolved at build time
    pub anchor: Option<Anchor>,     // first-placement only; None => end of file
    pub create: bool,               // may apply create the file if absent?
    pub source: Option<PathBuf>,    // support file for diagnostics; None for inline
}

/// `^[a-z0-9_-]+$`. Validated at build; the marker lines embed it verbatim, so
/// it must be safe inside a single comment line. Two edits to one path with the
/// same key are a plan-build `Duplicate` error.
pub struct RegionKey(String);

/// Line and optional block comment delimiters. A line style has `close == ""`.
/// Resolved from the author's `comment:`/`marker:` or inferred from the target
/// path's extension; an unknown extension with no override is a build error.
pub struct MarkerStyle { pub open: String, pub close: String }   // "#","" | "//","" | "<!--","-->"

/// Where a region goes the first time, when the file has no markers for its key.
/// Once the markers exist, THEY are the selection; the anchor is bootstrap-only,
/// so anchor ambiguity/absence can only fail on first application.
pub struct Anchor { pub matcher: AnchorMatch, pub place: Place, pub occ: Occurrence }
pub enum AnchorMatch { Literal(String), Regex(regex::Regex) }  // compiled at build
pub enum Place { Before, After }
pub enum Occurrence { First, Last, Only }  // `Only`: >1 match => ambiguous error
```

The on-disk managed region — the single source of truth for ownership and drift:

```text
{open} toha:region {key} {close}\n     <- begin marker, its own line
{body}\n                                <- Toha's body, verbatim
{open} toha:end {key} {algo}:{hex} {close}\n   <- end marker records body checksum
```

`{hex}` is a digest of the body bytes Toha wrote (`{algo}` names it, e.g.
`sha256`, so the algorithm can evolve without ambiguity). The checksum is what
turns drift detection from a heuristic into a fact: it distinguishes "the body on
disk is the one Toha last wrote" (safe to replace) from "the user changed it"
(drift).

### The load-bearing pure function

Both apply and dry-run route through one function. It reads no file; the caller
passes the current bytes. This is where idempotency, drift, and placement are
decided, and it is trivially fixture-testable (`per make-operations-idempotent`).

```rust
/// The whole-file outcome of resolving one edit against the target's current
/// bytes. `current == None` means the file is absent.
///
/// Purity: `resolve_edit(read(path), edit)` depends on nothing else, so
/// replaying a staged interview against a once-applied file yields `Unchanged`.
pub enum Resolution { Unchanged, Write(Vec<u8>) }

pub fn resolve_edit(current: Option<&[u8]>, edit: &PlannedEdit)
    -> Result<Resolution, EditError>
{
    // 1. Absent file: create (with markers at EOF of empty content) iff `create`,
    //    else EditError::TargetMissing.
    // 2. Non-UTF-8 file: EditError::NotUtf8 (markers are text).
    // 3. Locate the region by its exact begin/end marker lines for `edit.region`:
    //      - begin without end, two begins, or an unparseable end line
    //        => EditError::MalformedRegion.
    //      - found: split into (before, body_on_disk, recorded_hash, after).
    //          if body_on_disk == edit.body            => Unchanged   // idempotent
    //          else if hash(body_on_disk) == recorded  => Write(rebuilt) // template changed
    //          else                                    => EditError::Drift // user edited
    //      - not found: FIRST PLACEMENT
    //          anchor None            => append region at end of file
    //          anchor Some, 1 match   => insert before/after the match
    //          anchor Some, 0 matches => EditError::AnchorMissing
    //          anchor Some, >1 & Only => EditError::AnchorAmbiguous
    // 4. `Write(bytes)` always carries the FULL new file bytes with a freshly
    //    computed end-marker checksum, so the next resolve sees Unchanged.
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

Key invariant, stated so a reader trusts idempotency: **`Write` always recomputes
the end-marker checksum over the body it emits.** Therefore
`resolve_edit(bytes_written_by_a_prior_Write, same_edit) == Unchanged`. The
double-apply acceptance fixture (apply injection twice, file changes once) is a
direct consequence, and it survives staging replay because `PlannedEdit` is a
pure function of `(Template, Completed)`.

### Build path (pure over template + listing, unchanged discipline)

`Plan::build` gains a loop mirroring the `files` loop (`plan.rs:200`). It renders
each `InjectionRule` into `PlannedEdit`s — resolving the target path through the
same `TargetPath::parse`, rendering the body from the support file
(`read_render`) or inline template, resolving the marker style, and compiling the
anchor. It reads **no target file bytes**; it records intent only. `Plan::add_edit`
checks `has_symlink_component` and rejects a duplicate `(path, region)` pair, the
same way `add` guards files (`plan.rs:272`).

```rust
// src/template.rs — new domain type built by a new Builder branch
pub struct InjectionRule {
    pub into: Tmpl,
    pub region: Tmpl,
    pub body: BodySource,          // Support(PathBuf) | Inline(Tmpl)
    pub marker: MarkerSpec,        // Infer | Family(CommentFamily) | Raw { open, close }
    pub anchor: Option<AnchorSpec>,
    pub create: Typed<bool>,
    pub each: Option<Each>,        // reuses `Each`; one entry can fan out to N regions
    pub when: Option<Expr>,
}
// Template gains: pub injections: Vec<InjectionRule>,

/// Resolve comment delimiters at build time (the path is known once rendered).
/// Table maps extensions to families: hash(`#`) rem sql/lua(`--`) slashes(`//`)
/// block-c(`/* */`) xml(`<!-- -->`) semicolon(`;`) percent(`%`). An unknown
/// extension with `MarkerSpec::Infer` is a load/build error naming `comment`.
fn resolve_marker_style(path: &TargetPath, spec: &MarkerSpec)
    -> Result<MarkerStyle, PlanError> { todo!("not implemented") }
```

### Apply path (read-modify-write, one write per file)

`apply_reporting` (`apply.rs:66`) grows a stage between the conflict gate and the
file writes. It preserves the "write nothing on conflict" all-or-nothing gate:

```rust
// after the existing whole-file conflict recompute (apply.rs:73):
let mut resolved: Vec<(TargetPath, Vec<u8>)> = Vec::new();  // only the ones that change
let mut drift: Vec<(TargetPath, RegionKey)> = Vec::new();
for edit in &self.edits {
    let current = read_optional_bytes(target, &edit.path)?;      // read once
    match resolve_edit(current.as_deref(), edit) {
        Ok(Resolution::Unchanged)    => {}                        // idempotent skip
        Ok(Resolution::Write(bytes)) => resolved.push((edit.path.clone(), bytes)),
        Err(EditError::Drift { path, region }) => drift.push((path, region)),
        Err(other) => return Err(other.into()),                   // hard error, nothing written
    }
}
if !options.force && (!conflicts.is_empty() || !drift.is_empty()) {
    return Err(ApplyError::from_conflicts_and_drift(conflicts, drift));  // writes nothing
}
// with force: fold drift back into `resolved` by re-resolving as forced Write
// trust gate, then symlink checks for files AND edit paths, unchanged in spirit
// write files (whole-file) first, then each `resolved` edit ...
```

Each `Write` is a single `fs::write` of the full new bytes — recommended via a
sibling temp file + atomic rename so a crash cannot corrupt a **user-owned** file
(see open questions). `on_written` fires once per changed path, preserving the
"print each written path before hooks" contract (`apply.rs:151`).

**Ordering and coexistence.** Whole-file `files` are written first, then `edits`,
so an injection can target a file a whole-file rule just created in the same
plan. A `PlannedFile` and a `PlannedEdit` to the same path are allowed and
ordered (write then inject). Two edits to one path with different keys are
allowed and applied in list order, each independently idempotent. Two edits to
one path with the same key are a build-time `Duplicate`. This is the explicit
answer to "how injection and ordinary writes coexist in one `Plan`."

### The mutation contract (for 1066/1031)

The plan is the single source of truth for what Toha owns in each target file.
1066 (git-based update) and 1031 (implementation) consume a derived view, not a
synced copy (`per single-source-of-truth`):

```rust
/// What Toha claims to own in the target tree. Derived from the plan.
pub enum FileMutation<'a> {
    /// Whole file: re-apply replaces it wholesale; owner is Toha.
    Whole  { path: &'a TargetPath },
    /// Marked region: re-apply replaces only the marked span; the rest is the
    /// file owner's. The end-marker checksum lets an update detect owner drift.
    Region { path: &'a TargetPath, region: &'a RegionKey },
}
impl Plan {
    pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>> { todo!() }
}
```

Stated for 1066: **Toha owns exactly the byte span between (and including) a
begin/end marker pair keyed by region id; everything else in that file belongs to
its owner. The end marker's checksum identifies the body Toha last wrote.** An
update or replay can therefore reason per-region: a region whose on-disk body
still matches its checksum is safe to re-apply; a drifted region is a conflict.

### Error / results contract

- `Applied::Written.files` includes injection targets that changed; unchanged
  regions never appear (idempotent).
- New `ApplyError` arms: `Drift(Vec<(TargetPath, RegionKey)>)` (message names
  `--force`, exit 1); `AnchorMissing`/`MalformedRegion`/`TargetMissing`/`NotUtf8`
  (exit 1, name the file/region); `AnchorAmbiguous` (exit 5, the "ambiguous"
  code). `--force` governs **drift overwrite only** — it never invents placement,
  so anchor/malformed/missing-target errors persist even with `--force`. This
  keeps `--force` scoped to overwriting, never to fabricating structure, and
  never silently rewrites a file Toha does not own outside its own markers.
- New `PlanError` context reuses `Duplicate` for `(path, region)` collisions and
  `Path`/`Render` for bad region keys and unresolvable marker styles.

### Interface depth

Public surface added: one `template.yml` list (`inject`), one `Plan` field
(`edits`), one `FileMutation` view, and one pure function (`resolve_edit`).
Behind that small surface sits every hard part — marker synthesis, per-extension
comment inference, anchor placement, checksum-based drift detection, byte-level
idempotency, and crash-safe writes. Callers that already `build().apply()` are
untouched; authors write four lines of YAML. The complexity is pulled into the
callee (`per boundary-discipline`), and wire/comment concerns never surface on
the public type — `MarkerStyle` is resolved before it reaches `PlannedEdit`.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- **We accept visible marker comments in the user's file in exchange for
  structural idempotency and exact ownership.** Markers are the on-disk record;
  removing them would force a heuristic re-match of the injected text, which the
  grounding forbids.
- **We accept a small digest dependency (e.g. `sha2`) in exchange for
  non-heuristic drift detection.** The alternative — no checksum — cannot tell a
  template-body change from a user edit, so it would either clobber user edits or
  refuse every legitimate template update.
- **We accept a per-extension comment table with an explicit override in exchange
  for working across arbitrary text files.** There is no universal comment; a
  table plus `comment:`/`marker:` covers the long tail and fails loudly on the
  unknown rather than guessing.
- **We accept `edits` as a separate `Plan` vector, not a `Content` variant, in
  exchange for keeping "I already hold the bytes" and "I must read the target"
  as distinct, non-leaky types.**
- **We accept anchors as bootstrap-only in exchange for a stable steady state.**
  Anchor ambiguity can only bite on first placement; every later apply selects by
  marker, so a template that placed once keeps working even if the surrounding
  text moves.

## Alternatives considered

- **Structured merge via jaq (JSON/TOML/YAML).** Deep for structured targets, but
  it reserializes and drops comments, key order, and formatting — a disguised
  whole-file rewrite of a user-owned file — and cannot touch arbitrary text
  (README, `.gitignore`, source). It exposes format-specific behavior to the
  author and a jq dependency on the critical path. Rejected as the core; viable
  later as an optional, clearly-bounded mode over a `MarkerStyle`-free region.
- **Anchor-only injection (no markers), re-matching the injected text on
  re-apply.** Smaller on-disk footprint (no comments), but idempotency becomes a
  heuristic: once the user edits around the block, the matcher misfires and Toha
  either duplicates or clobbers. Explicitly forbidden by the grounding. Rejected.
- **A third `Content` variant `Injected { region, body, .. }`.** Fewer new
  names, but it puts a "reads the target file" requirement behind a type whose
  other variants hold complete bytes, forcing every `apply` `Content` match and
  every consumer to special-case it. Shallower interface, more leakage. Rejected
  in favor of the `edits` sibling.
- **A separate `toha inject` subcommand / second apply pass.** Temporal
  decomposition of one apply into two, breaking the single-`Plan` contract and
  the "print each written path" ordering. Rejected.

## Open questions and risks

- **Should injection writes use temp-file-plus-rename while whole-file writes
  stay direct `fs::write`?** Injection mutates user-owned files, where a partial
  write is worse than for a generated file. Atomic rename within the target dir
  adds no new port/permission/timeout, but it diverges from today's direct-write
  behavior (`apply.rs:112`). Do we adopt it for edits only, for all writes, or
  keep "recovery is re-run"?
- **Which digest, and is a new dependency acceptable?** `sha256` via `sha2` is the
  recommendation; a non-crypto checksum would avoid a dependency but is weaker
  against accidental collision. The `{algo}:` prefix lets us change later —
  acceptable?
- **What is the default `create` behavior?** This design defaults `create: false`
  (injection targets a file that already exists, by definition). Is opt-in
  creation the right default, or should a region into an absent file always
  create it?
- **Should a body line that happens to equal a marker line be a build error?** It
  would corrupt region parsing. Cheap mitigation: reject at build if the rendered
  body contains a line matching this region's marker. Worth the check?
- **Does `--dry-run` read target bytes?** It must, to report `inject`/`update`/
  drift accurately. That is consistent with reading the listing today, but
  confirm dry-run may read file contents.

## Next implementation step

Write `resolve_edit` and its region-parser against fixtures — first-placement at
EOF, first-placement at anchor, byte-identical re-apply (`Unchanged`),
template-body change (`Write`), and user drift (`Drift`) — since that pure
function is the whole contract and every apply/dry-run/mutation path depends on
it.
