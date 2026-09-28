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

The JSON family (JSON/JSONC/JSON5) is the one place this mechanism meets a
boundary, and how it is handled — including whether an embedded `jaq` or other
structured mutator belongs here — is presented as a set of recommendations for
Bob's ruling in its own section below (**JSON-family injection and the
structured mode**), grounded in source-cited research
(`01a-grounding-addendum-json-family.md`) and a focused arena (`02a`/`04a`).
Nothing in that section is settled scope until Bob rules D6a/D6b/D9.

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

## JSON-family injection and the structured mode

This section presents recommendations on the JSON family (JSON/JSONC/JSON5) and
on a structured "set a value at a path" mutator, including the embedded `jaq`
candidate, for Bob to rule on (D6a/D6b/D9). **Nothing here is settled scope until
he rules.** It replaces the earlier *assumption* that structured merge is out of
scope with an explicit, evidence-backed choice Bob makes: which mechanism, and
whether to implement a structured mode in 1031 now or sequence it later. Both the
defer recommendation and the substantive implement-now option are on the table
(the arena developed both). Evidence and the focused arena are in
`01a-grounding-addendum-json-family.md`, `02a-arena-json-family.md`,
`04a-synthesis-json-family.md`, and the preserved candidate packages under
`03-candidates/json-family-revision/`.

### Recommended for 0.2.0 (D6a, pending approval): the marker mechanism covers JSONC and JSON5

JSONC and JSON5 permit `//` comments, so Toha's begin/end markers are ordinary
comment lines and inject exactly as they do in `.rs`/`.gitignore`/`.yaml`, with
the same `sha2` checksum idempotency and drift refusal. No new mechanism, no new
dependency. This is the same locate-excise-splice-write-if-differs cycle proven
for fifteen years by Ansible `blockinfile`
(`ansible/lib/ansible/modules/blockinfile.py:334-367`, rev `7ec731b`).

**Generic before / after — a `.jsonc` target.**

Template config:

```yaml
inject:
  - into: "config/app.jsonc"
    region: "features"
    content: |
      "analytics_enabled": true,
      "analytics_sample_rate": 0.1
```

Target BEFORE:

```jsonc
{
  "app_name": "sample-service",
  "port": 8080,
  // operator-owned settings
  "log_level": "info"
}
```

Target AFTER first apply (markers are valid JSONC `//` comments; the operator's
comment and keys are untouched; the region is appended at end of file because no
anchor was given):

```jsonc
{
  "app_name": "sample-service",
  "port": 8080,
  // operator-owned settings
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d… <<<
}
```

Second apply, same answers → **no bytes change** (the on-disk body hashes to the
recorded checksum ⇒ `Unchanged`). If the operator edits inside the markers,
re-apply refuses and names `--force` (drift), identical to any text target. This
is the twice-apply-changes-once acceptance, holding for the JSON family.

### The strict-`.json` boundary (proposed boundary of marker injection, with guidance)

Strict JSON has no comment syntax, so a `//` marker would make the file invalid
JSON. The recommendation (part of D6a) is that a *marker* injection whose
resolved target is a strict `.json` file (a line-comment marker with no
comment-capable extension and no override) be **refused at build time** —
`PlanError` naming the file, the region, and three author alternatives:

1. target a `.jsonc`/`.json5` file (any JSON parser accepts JSONC), or
2. manage the whole file with a `files:` rule, or
3. use the structured mode below (if D6b adopts it).

This is a **proposed boundary of the new injection feature, not an approved
operator restriction** and not a settled scope cut — it applies only to the new
marker-injection path and takes effect only if D6a is adopted. It **does not
change how Toha renders whole JSON files today**: whole-file `files:` rules that
generate or overwrite a `.json` target are entirely unaffected. It is not a
regression — today Toha cannot inject at all — and it matches the ecosystem: even
npm's own package.json editor reserializes the whole document rather than
surgically editing strict JSON (`npm-package-json/lib/index.js:239-264`, rev
`a7dafdb`).

### Why `jaq` (or any reserialize mechanism) is recommended against for the structured path (D9, pending approval)

`jaq` was evaluated as an embedded crate against its actual source; the
recommendation is to **reject it as the structured mechanism** (D9, Bob rules).
It discards `#` comments at lex time
(`jaq-json/src/read.rs:10-19`, rev `f167ad4`) and regenerates all output from a
pretty printer (`jaq-json/src/write.rs:202-260`); its object `IndexMap`
(`lib.rs:117`) keeps order in memory but the printed bytes are fresh. `jaq` is a
jq-language filter/query engine, not an editor: using it to change one value in a
user-owned JSON-family file is a disguised whole-file rewrite that destroys the
owner's comments and normalizes their formatting/order — the no-silent-overwrite
hazard the grounding forbids — and it imposes a query language for what is
almost always a single-value set. The same disqualifier applies to every
reserialize approach (`serde_json`, `json5-rs`; npm mitigates only indent/newline,
never comments). If D9 is adopted, `jaq` is retained only as the not-recommended
alternative; the implement-now option (D6b) does not depend on jaq.

### The structured mode: recommended mechanism and sequencing (Bob rules D6b)

If Toha adds a structured "owns the value at a path" mode for the JSON family,
the **recommended** mechanism is **`jsonc-parser`'s `cst` module** (crate
`0.34.0`, feature `cst`, rev `c7d4cf5`) — the format-preserving JSON-family
analogue of `toml_edit`, and the same pattern VS Code uses to edit
`settings.json`. Its CST retains every comment and whitespace token and re-emits
them verbatim; only the touched node changes
(`jsonc-parser/src/cst/mod.rs:1-5,1356`). The arena and a readonly cross-judge
both scored this the superior structured design (26/30 vs jaq's 17/30). On
sequencing, the arena's **recommendation** is to defer the implementation out of
the 0.2.0 slice — it adds a second mechanism, a new dependency, a distinct
ownership contract, and new dry-run/error semantics to the first-ever injection
slice, for a gain over markers that JSONC/JSON5 already deliver. **Bob may
instead choose to implement the structured mode in 1031 now**; that
implement-now option is a co-equal choice on D6b, is fully sketched here, and is
not foreclosed. Neither the mechanism choice nor the sequencing is settled until
Bob rules; 1031's scope is not narrowed by this recommendation.

Proposed interface (a sibling discriminator inside the same `inject:` list, so
adopting it — now or later — is non-breaking):

```yaml
inject:
  - into: "package.json"        # strict JSON is fine for the STRUCTURED mode
    struct:                      # discriminator; mutually exclusive with `region`
      path: "scripts.build"      # owns the VALUE at this path, not a byte span
      value: "tsc"               # rendered, then parsed as a JSON value
    # provenance: true           # JSONC-only, optional drift-awareness comment
```

```rust
// reserved sibling of resolve_edit; pure over (current_bytes, edit)
pub fn resolve_struct_edit(current: Option<&[u8]>, edit: &PlannedStructEdit)
    -> Result<Resolution, StructError>;   // Unchanged when value already equals desired
```

**Ownership / idempotency / drift for the structured mode (the contract Bob
ratifies before it is built).** Toha owns *the value at the named path*;
everything else — comments, key order, whitespace — is preserved byte-for-byte by
the CST. Re-apply sets the value and is a byte no-op when it already equals the
desired value (idempotent, replay-safe). Because strict JSON has no comment slot
for a checksum, a re-apply cannot distinguish "the operator edited the managed
value" from "first set": the honest model is **declarative convergence** — Toha
re-sets exactly the path it owns and touches nothing else (the structured
analogue of the marker span). JSONC can optionally carry an inline `// toha:struct
<path>` provenance comment for awareness. This is a genuinely different ownership
contract from marker drift-refusal, which is why it is a deliberate, deferred
decision rather than a silent addition.

**Generic before / after — the proposed structured mode (D6b).** A strict `.json`
target, `struct: { path: "scripts.build", value: "tsc" }`:

BEFORE:

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js"
  }
}
```

AFTER first apply (surgical CST edit — only the touched node changes; key order
and 2-space indent preserved):

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js",
    "build": "tsc"
  }
}
```

Second apply → **no bytes change** (`scripts.build` already equals `"tsc"` ⇒
`Unchanged`). For a `.jsonc` target the same edit preserves the operator's
comments and trailing commas verbatim around the changed value — the property
markers `jaq` would have destroyed. If D6b defers the structured mode, these
fixtures belong to the fast-follow design; if D6b adopts implement-now, they are
1031 fixtures. Bob's ruling decides which.
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

- **Structured merge via `jaq` / serde (reserialize family).** Evaluated against
  actual source; **recommended against** as the structured mechanism (D9, Bob
  rules): `jaq` discards comments at lex time and regenerates all output
  (`jaq-json/src/read.rs:10-19`, `write.rs:202-260`), so editing a user-owned file
  is a disguised whole-file rewrite; `serde_json`/`json5-rs` share the same
  trivia-free data model. See **JSON-family injection and the structured mode**
  above.
- **Structured, format-preserving CST via `jsonc-parser` (the recommended
  structured mechanism; sequencing is D6b).** Preserves comments/order/whitespace
  (the JSON analogue of `toml_edit`); owns the value at a path with declarative
  convergence. Superior structured design in the arena. The recommendation is to
  sequence it after 0.2.0 with its interface reserved, but implementing it in 1031
  now remains a co-equal option on D6b; Bob's ruling decides, and 1031's scope is
  not narrowed until then.
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
- **D6 (revised) — JSON family & the structured mode.** Two parts, both open for
  Bob's ruling:
  - **D6a — JSON family via markers.** JSONC/JSON5 inject via comment markers like
    any text file; a strict `.json` marker injection is *proposed* to be refused
    at build time (a boundary of the new feature, not an operator restriction and
    not a change to existing whole-file JSON rendering), with named author
    alternatives. *Recommend: adopt.*
  - **D6b — Structured "owns-the-value-at-a-path" mode: mechanism and sequencing.**
    *Recommended* mechanism is `jsonc-parser`'s format-preserving CST (not jaq),
    with a `struct:` interface seam that lands non-breaking whenever it is built.
    Two co-equal sequencing options: **(i) sequence after 0.2.0** as a fast-follow
    *(recommended — keeps the first injection slice small for a gain markers
    already deliver on JSONC/JSON5)*, or **(ii) implement in 1031 now** (fully
    sketched, not foreclosed). Adopting D6b also ratifies its
    *declarative-convergence* ownership contract (Toha re-sets exactly the owned
    path; strict JSON carries no drift marker). *Recommend: option (i); Bob rules.*
    Until he rules, neither the mechanism nor the sequencing is settled and 1031's
    scope is not narrowed.
- **D9 — `jaq` as the structured mechanism.** Reserialize destroys a user-owned
  file's comments/order/formatting (cited source); if a structured mode ships, the
  recommendation is `jsonc-parser`'s CST instead. *Recommend: reject jaq; Bob
  rules.*
- **D7 — Atomic temp-file+rename for injection writes.** *Recommend: yes
  (injection only; whole-file writes unchanged).*
- **D8 — Reuse existing `sha2`** for the region checksum. *Recommend: yes (no new
  dependency).* A structured mode, if adopted (D6b), would add `jsonc-parser` when
  built — an embedded, well-maintained crate (dprint/deno), no subprocess —
  surfaced now so that adoption is evaluated as part of D6b rather than later.

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
- `docs/specifications/template-format.yml` (contingent on the D6 rulings):
  document that JSONC/JSON5 inject via comment markers like any text file, that a
  strict `.json` marker injection is refused at build time with the named
  alternatives (D6a), and — if D6b adopts it — that a `struct:` discriminator
  selects the format-preserving structured mode. Existing whole-file JSON
  rendering is unchanged.
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
9. (If D6a adopted) JSONC/JSON5 injection places valid `//` markers and is
   twice-apply idempotent: fails if the markers invalidate the file or the second
   apply changes bytes.
10. (If D6a adopted) Strict `.json` marker injection is refused at build time,
    naming the boundary and alternatives: fails if Toha writes a `//` marker into
    strict JSON or refuses silently. (Structured-mode fixtures attach to whichever
    D6b sequencing Bob picks — fast-follow or 1031 — not to 1031 by assumption.)

## Next implementation step

Write `resolve_edit` and its byte-range region parser against fixtures —
first-placement at EOF, first-placement at anchor, byte-identical re-apply
(`Unchanged`), template-body change (`Write`), user drift (`Drift`) — since that
pure function is the whole contract and every apply / dry-run / mutation path
depends on it.
