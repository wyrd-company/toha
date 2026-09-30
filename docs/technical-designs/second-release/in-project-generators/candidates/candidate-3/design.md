# In-project generators — template-declared generators (candidate 3)

A template **declares** one or more named **generators**. A generator is an
author-named alternate view of the template — its own source subdirectory, its
own interview, its own file rules — meant to be applied **repeatedly into
subpaths** of an existing project ("add a component", "add a module"). A new
verb, `toha generate TEMPLATE GENERATOR SUBPATH`, drives the existing per-target
apply spine with that view. Each application's answers optionally default from
the project's existing **snapshots** of the same template source.

> Read `rationale.md` first if you want the honest verdict on the schema change.
> Short version: the template-declared concept buys real **authoring
> ergonomics** (a scoped interview + a bundled source subdirectory) but it does
> **not** buy any snapshot-keying leverage, because the frozen snapshot contract
> cannot carry a generator name. The design below is the strongest
> declared-concept shape; the rationale states where a purely derived shape
> would match it for less cost.

---

## 1. Caller usage (the spec)

### 1.1 What a template author writes

A template gains an **optional** `generators` map. A template that omits it
behaves exactly as today.

```yaml
# template.yml  (a React-ish app template; illustrative, generic values)
name: web-app
source: template            # the base source, used by `toha apply`
interview:
  - id: app_name
    prompt: "Application name"
  - id: license
    prompt: "License"
    default: "MIT"

generators:
  component:
    description: "Add a UI component"
    source: generators/component        # a sibling source subdir in the template root
    suggested_target: "src/components"   # person-route prompt default (no answer interpolation)
    interview:                           # the generator's OWN interview list
      - id: component_name
        prompt: "Component name"
      - id: styled
        prompt: "Include a stylesheet?"
        kind: confirm
        default: true
  module:
    description: "Add a feature module"
    source: generators/module
    suggested_target: "src/modules"
    interview:
      - id: module_name
        prompt: "Module name"
```

`component` renders the files under `<template-root>/generators/component/`,
asks `component_name` and `styled`, and is applied into a subpath the caller
names. `module` is a second, independent generator in the same folder.

### 1.2 Person terminal — first application, then a second defaulting from the first

```console
$ toha generate web-app component ./src/components/Alpha
? Component name: Alpha
? Include a stylesheet? (Y/n) y
added  src/components/Alpha/Alpha.tsx
added  src/components/Alpha/Alpha.css
snapshot 01J9Z4K7QX (web-app 3f9a1c2)        # produced by the plain-apply capture (task 1029)
```

The first `generate` in a clean git repo produces a snapshot exactly as a plain
`apply` does — capture is owned by task 1029 and is unchanged by this design. Its
`source` is `web-app`'s source identity; its `target` is `src/components/Alpha`.

```console
$ toha generate web-app component ./src/components/Beta
using defaults from snapshot 01J9Z4K7QX (web-app, src/components/Alpha)
? Component name: (Alpha) Beta
? Include a stylesheet? (Y/n) y                # defaulted to the recorded `true`
added  src/components/Beta/Beta.tsx
added  src/components/Beta/Beta.css
snapshot 01JA2B8M4R (web-app 3f9a1c2)
```

The second application found one snapshot whose `source` equals this template's
source identity and seeded its answers as **defaults**. The person overrode
`component_name`; `styled` kept the recorded `true`. This is the mandated
twice-in-one-project behaviour (§6, B1).

### 1.3 Scripted `--answers` — deterministic selection, no interaction

```console
$ toha generate web-app component ./src/components/Gamma --answers gamma.json
```

`gamma.json`:

```json
{ "component_name": "Gamma" }
```

Result document (one document, as today):

```json
{
  "protocol": 1,
  "status": "applied",
  "target": "src/components/Gamma",
  "defaults_from": "01JA2B8M4R",
  "files": [ { "path": "src/components/Gamma/Gamma.tsx", "action": "added" },
             { "path": "src/components/Gamma/Gamma.css", "action": "added" } ],
  "snapshot": { "id": "01JB3C2D5E" }
}
```

The script route selects the **most recent snapshot of this source** with no
interaction (ULID order is total — §5), seeds `styled = true` from it, and takes
`component_name` from `--answers`. `defaults_from` reports which snapshot seeded
the run. If `gamma.json` omitted a question that had neither a seeded value nor a
template default, the run returns `questions` (exit 4) exactly as an ordinary
headless apply does.

To pin a specific seed regardless of recency:

```console
$ toha generate web-app component ./src/components/Gamma \
    --defaults-from 01J9Z4K7QX --answers gamma.json
```

`--defaults-from` names a snapshot explicitly. Unknown, ambiguous, absent, or a
snapshot of another source fails clearly (§5).

### 1.4 Agent staged route

```console
$ toha generate web-app component ./src/components/Delta --async
{ "protocol": 1, "status": "questions",
  "questions": [ { "id": "component_name", ... },
                 { "id": "styled", "default": true, "default_source": "seed" } ],
  "defaults_from": "01JB3C2D5E" }
$ toha continue ./src/components/Delta answers.json
$ toha apply ./src/components/Delta
```

`generate --async` stages an interview at the target. The staged record carries
the **generator name** and the **selected seed snapshot id** so `continue` and
the final `apply PATH` rebuild the same effective template and re-read the same
seed (nothing is frozen into the project; the snapshot is re-read from git).

### 1.5 Snapshot-absent fallback (non-git, dirty, or unfetched)

```console
$ toha generate web-app component /tmp/scratch/Epsilon
? Component name: Epsilon
? Include a stylesheet? (Y/n) y                # template default; no snapshot exists
added  src/.../Epsilon.tsx
no snapshot: /tmp/scratch is not a git repository
```

When `Project::open` returns `None` (not in git), or the target is dirty, or a
clone has not fetched snapshot refs, there are no snapshots. `generate` still
applies: it falls back to **configured presets / template-defaults**, then to the
generator interview's own defaults, then to normal questions (fixed policy). It
never pretends a snapshot existed. `--defaults-from` in this situation fails
clearly (§5), because an explicitly required snapshot cannot be satisfied.

---

## 2. Data structures expected

### 2.1 The template-declared surface (new, optional)

```rust
// src/template.rs — one new OPTIONAL field on the existing struct.
// A template that omits `generators` deserializes to an empty map: today's
// behaviour, unchanged.
pub struct Template {
    // ... the existing 11 keys: name, description, source, data, interview,
    //     files, inject, ignore, static, hooks, messages ...
    pub generators: IndexMap<GeneratorName, Generator>,
}

/// An author-chosen generator name. Matches the existing `identifier` pattern
/// `^[a-z_][a-z0-9_]*$`. A distinct newtype so a generator name can never be
/// confused with a question `Id`, a template short name, or a formal name.
pub struct GeneratorName(String);

/// A named alternate view of its template. Every field the generator omits is
/// inherited from the base template when the effective template is built
/// (§3.1). Reuses the existing authoring types wholesale — a generator is not a
/// second interview language, it is a second *instance* of the one we have.
pub struct Generator {
    pub description: Option<String>,
    /// The generator's source subdirectory, relative to the template root.
    /// `None` reuses the template's own `source`.
    pub source: Option<PathBuf>,
    /// The generator's own interview list. Reuses `Question` unchanged.
    pub interview: Vec<Question>,
    pub files: Vec<FileRule>,       // reused type; defaults to the base's rules when empty
    pub inject: Vec<InjectRule>,    // reused type
    pub statics: Vec<StaticRule>,   // reused type (`static` key)
    pub ignore: Vec<IgnoreRule>,    // reused type
    pub messages: Messages,         // reused type
    /// A person-route prompt default for the subpath. A plain path fragment:
    /// it is NOT interpolated with answers, because the spine fixes the target
    /// before the interview runs (§3.4). `None` means no suggestion.
    pub suggested_target: Option<String>,
    // NOTE: a generator does NOT declare `hooks` in this slice. Hooks and trust
    // are keyed by the template's formal name; a generator inherits the base
    // template's hooks unchanged. Per-generator hooks are out of scope (§7).
}
```

### 2.2 The internal default bank (extended to carry an ordered fallback)

```rust
// src/interview.rs — the PRIVATE bank entry changes from an enum (one occupant
// per id) to a struct that can hold BOTH a snapshot seed AND a configured
// default for the same id, with a fixed precedence. The PUBLIC `Seed` contract
// (`defaults: IndexMap<Id, RawAnswer>`) is unchanged; only this private type
// changes.
struct DefaultBankEntry {
    /// From a snapshot's `submissions` (or a crate/replay `Seed`). Wins.
    seed: Option<RawAnswer>,
    /// From presets / template-defaults, origin-bearing for attribution.
    configured: Option<ResolvedDefault>,
    // INVARIANT: at least one field is `Some`. An id with neither is simply not
    // in the bank, so `render_default` falls through to the template's own
    // default expression — unchanged behaviour.
}

impl DefaultBankEntry {
    /// Snapshot seed beats a configured default beats the template's own
    /// default (which `render_default` reaches only when this returns `None`
    /// AND the id is absent from the bank). Single source of truth for
    /// generator precedence.
    fn resolve(&self) -> (&RawAnswer, PreparedDefaultSource) {
        unimplemented!("seed.is_some() → (seed, Seed); else (configured.raw, Configured(origin))")
    }
}
```

### 2.3 Generator selection and identity (new module `src/generator.rs`)

```rust
// src/generator.rs — the whole generator concept lives behind this module.
// Nothing downstream (engine, plan, apply, staging) learns that generators
// exist; they all see a `Template`.

/// A resolved generator request, before the spine runs.
pub struct GeneratorRequest {
    pub generator: GeneratorName,
    /// The base template as loaded, plus the selected generator's overlay
    /// already applied — the spine consumes this as an ordinary `&Template`.
    pub effective: Template,
    /// The base template's source identity (formal name WITHOUT `@reference`).
    /// Snapshots are matched against THIS, never the full formal name (§5).
    pub source_identity: String,
    /// The person-route suggested subpath, if the generator declared one.
    pub suggested_target: Option<String>,
}

/// How the default seed was chosen — recorded in the result and staged record.
pub enum SeedChoice {
    /// No snapshot seeded this run (non-git, dirty, unfetched, or none of this
    /// source). Fall back to configured / template defaults.
    None,
    /// The automatic choice: the most recent snapshot of this source.
    LatestOfSource(SnapshotId),
    /// `--defaults-from ID`, or a person's explicit pick from the list.
    Explicit(SnapshotId),
}
```

---

## 3. Public interfaces expected

### 3.1 Building the effective template (the whole generator concept, collapsed)

```rust
// src/generator.rs

/// Overlay a declared generator onto its base template, producing an owned
/// `Template` the existing spine runs unchanged. Every field the generator
/// omits is inherited from `base`; `source` resolves relative to the template
/// root. The result carries the generator's interview and source, so
/// `Plan::build` reads the generator's files and the engine asks the
/// generator's questions with ZERO change to either.
///
/// Errors:
///   - the base template declares no generators → `NoGenerators` (exit 2)
///   - `name` is not a declared generator → `UnknownGenerator { available }`
///     (exit 2)
pub fn effective_template(
    base: &Template,
    root: &Path,          // the template root, for resolving the generator source
    name: &GeneratorName,
) -> Result<GeneratorRequest, GeneratorError> {
    unimplemented!("clone base; override source/interview/files/inject/static/ignore/messages from the generator; keep name/data/hooks; compute source_identity from the resolved formal name")
}
```

### 3.2 Selecting the seed snapshot (deterministic per route)

```rust
// src/generator.rs — consumes ONLY the `toha::snapshot` reader contract.
// No gitoxide, wire, or snapshot-internal type crosses this signature.

/// The snapshots of this source, newest first, for selection and person-route
/// listing. Filters `project.snapshots()` to `Valid` entries whose `source()`
/// equals `source_identity`. Invalid snapshots are dropped for seeding (they
/// are surfaced by `snapshots list`, owned by 1029).
pub fn candidates<'p>(
    project: &'p Project,
    source_identity: &str,
) -> Result<Vec<Snapshot>, ProjectError> {
    unimplemented!("project.snapshots() → keep Valid where source()==source_identity → sort by id() desc (ULID = creation order)")
}

/// The automatic, non-interactive choice used by the script and agent routes
/// and preselected for the person: the most recent snapshot of this source, or
/// `None` when there are no candidates. Total and deterministic because ULIDs
/// are a total creation order.
pub fn latest(candidates: &[Snapshot]) -> Option<&Snapshot> {
    unimplemented!("candidates.first() — already sorted newest-first")
}

/// Resolve an explicit `--defaults-from PREFIX` (or a person's pick) against the
/// project, then verify the source. Fails clearly per the fixed policy.
///
/// Errors (all exit 1, each naming `toha snapshots list SUBPATH`):
///   - `project` is `None` (not in git) → `SnapshotUnavailable`
///   - prefix unknown or ambiguous, or the snapshot invalid → `SnapshotError`
///   - `snapshot.source() != source_identity` → `SourceMismatch { wanted, found }`
pub fn require(
    project: Option<&Project>,
    source_identity: &str,
    prefix: &str,
) -> Result<Snapshot, GeneratorError> {
    unimplemented!("project.ok_or(SnapshotUnavailable)?.find(prefix)?; check source(); return")
}
```

### 3.3 The submissions → defaults seam (load-bearing)

```rust
// src/generator.rs — the ONLY place a snapshot's data reaches the interview.
// A `Snapshot` (domain type from the consumed contract) enters; an
// `IndexMap<Id, RawAnswer>` (the exact `Seed.defaults` shape) leaves. The
// snapshot type never crosses into the engine.

/// Flatten a snapshot's accepted raw submissions into per-id defaults. Folds
/// all batches into one map; a looped question's recorded JSON array stays one
/// value under its id (a valid `RawAnswer` for a loop default). These are
/// pre-`format`, pre-`validate` values — the new application's own
/// `format`/`validate`/`when` re-run on them through the unchanged engine.
///
/// This uses question ids only WITHIN one template source identity (the
/// snapshot was already filtered to `source_identity`). It is NOT cross-template
/// id inference: the forbidden case is seeding from a snapshot of a different
/// source, which §3.2 refuses.
pub fn seed_from(snapshot: &Snapshot) -> IndexMap<Id, RawAnswer> {
    unimplemented!("for batch in snapshot.submissions() { for (id, v) in batch { out.insert(id.clone(), RawAnswer(v.clone())) } }")
}
```

### 3.4 Starting the interview with both seed and configured defaults (the precedence seam)

```rust
// src/interview.rs — a new constructor beside `Resolution::start_with_context`.
// It is the single seam where a snapshot seed and the configured resolution
// coexist. Precedence (snapshot > configured) is realized HERE, in the boundary
// layer, encoded in `DefaultBankEntry`; the pure engine below stays unaware of
// snapshots, presets, and identity.
impl Resolution {
    /// Build the internal bank from BOTH the origin-bearing configured defaults
    /// (`self`) and a snapshot seed. For each id: the seed occupies
    /// `DefaultBankEntry.seed`; a configured default occupies
    /// `.configured`; ids present in only one source get only that field. The
    /// public `Seed { defaults: IndexMap<Id, RawAnswer> }` contract is
    /// untouched — this path does not build a `Seed`.
    pub fn start_with_seed<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
        seed: IndexMap<Id, RawAnswer>,
    ) -> Result<Interview<'a>, EvalError> {
        unimplemented!("merge self.defaults (Configured) with `seed` (Seed) into IndexMap<Id, DefaultBankEntry>; start_with_bank")
    }
}
```

`render_default` (unchanged in shape) consults the bank first and now calls
`DefaultBankEntry::resolve()` instead of matching the old two-variant enum. A
seeded value renders with `PreparedDefaultSource::Seed`; a surviving configured
value keeps `PreparedDefaultSource::Configured(origin)`, so configured-default
attribution is preserved in generator runs.

### 3.5 Staging record additions (agent route only)

```rust
// src/staging.rs — two OPTIONAL members, absent for a plain apply/stage.
pub struct StagedRecord {
    // ... existing members: target, template, commit, named, now, submissions, context ...
    /// The generator this staged interview belongs to. Absent = plain apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<GeneratorName>,
    /// The snapshot chosen to seed defaults, re-read (not frozen) on resume so
    /// `continue`/`apply PATH` reproduce the same defaults deterministically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults_from: Option<SnapshotId>,
}
```

These are Toha's transient staging state, not a project file — they are **not** a
second persisted-answers lifecycle. No answers are persisted into the project;
prior answers still come only from snapshots or configured defaults.

---

## 4. Module and seam diagram

```text
CLI  generate TEMPLATE GENERATOR SUBPATH [--defaults-from ID] [--answers FILE] [--force] [--async]
  │                                                             [NEW verb: main.rs + CLI spec]
  ├─ cli::resolve::resolve_template(TEMPLATE)  → formal name, source identity, folder   [REUSED]
  ├─ Template::load(folder)                                                             [REUSED]
  ├─ generator::effective_template(base, root, GENERATOR)  → GeneratorRequest           [NEW]
  │        (owns the effective Template the rest of the spine consumes)
  ├─ staging::canonical_target(SUBPATH)  → CanonicalTarget                              [REUSED]
  ├─ toha::snapshot::Project::open(target)  → Option<Project>                           [CONSUMED]
  ├─ generator::{candidates, latest, require}  → SeedChoice                             [NEW]
  ├─ generator::seed_from(snapshot)  → IndexMap<Id, RawAnswer>                          [NEW seam]
  ├─ interview::configured_defaults(source_identity, effective, presets, mappings)      [REUSED]
  │        → Resolution
  └─ Resolution::start_with_seed(effective, now, context, seed)                         [NEW ctor]
        │
        ▼   from here the EXISTING per-target spine is byte-for-byte unchanged:
  build_context → start_with_context(bank) → Pending::answer* → Plan::build → apply_reporting
        │
        ▼
  PURE INTERVIEW ENGINE + Plan + apply    [UNCHANGED — unaware of generators/snapshots]

Consumed, never edited:  toha::snapshot  { Project, Snapshot, SnapshotId }  (task 1029)
Reused unchanged:        interview engine, Plan::build/apply, canonical_target, staging spine,
                         configured_defaults, exit-code mapping
New:                     `generators` field on Template + loader; src/generator.rs;
                         DefaultBankEntry struct + start_with_seed; `generate` verb;
                         StagedRecord.{generator, defaults_from}
```

Only `src/generator.rs` names the `toha::snapshot` reader types, and only
`toha::snapshot` names gitoxide. A `Snapshot` is reduced to
`IndexMap<Id, RawAnswer>` at §3.3 before anything interview-facing sees it.

---

## 5. Selection, precedence, collision, and the error/results contract

### 5.1 Selection matrix (snapshot count × route)

| Snapshots of this source | Person | Script (`--answers`) | Agent (`--async`) |
|---|---|---|---|
| 0 (or non-git / dirty / unfetched) | no seed; configured → template → ask | no seed; configured → template → ask | no seed; configured → template → ask |
| 1 | seed from it (shown) | seed from it | seed from it; record its id |
| N | list, preselect **latest of source**, person may pick another | seed from **latest of source** (ULID total order — no interaction) | seed from **latest of source**; record chosen id |
| any, with `--defaults-from ID` | seed from that snapshot (overrides the pick) | seed from that snapshot | seed from that snapshot; record it |

Automatic selection is **latest snapshot of this source** — total and
deterministic because ULIDs order by creation time. No content heuristic, no
sibling-subpath affinity (rejected in `rationale.md` as non-total).

### 5.2 Precedence for one question id (total)

```
snapshot seed  >  configured (template-defaults / preset)  >  generator's own `default`  >  unanswered
```

Realized in `DefaultBankEntry` at the §3.4 seam: an id present in both the seed
and the configured resolution keeps both occupants; `resolve()` returns the seed.
`--answers` (script) and a typed answer (person/agent) are **answers**, not
defaults, and override any default through the unchanged route.

### 5.3 Failure cases → exit codes

| Condition | Result | Exit |
|---|---|---|
| Template declares no `generators` | `error`; names `toha apply TEMPLATE PATH` | 2 |
| Generator name not declared | `error`; lists the available generator names | 2 |
| Ambiguous `TEMPLATE` short name | `error` (unchanged resolve path) | 5 |
| `--defaults-from` and target not in git | `error`; names why (no snapshots here) | 1 |
| `--defaults-from` unknown / ambiguous / invalid snapshot | `error`; names `toha snapshots list SUBPATH` | 1 |
| `--defaults-from` snapshot of another source | `error`; names the wanted and found source | 1 |
| Questions remain (script/agent) | `questions` (the batch) | 4 |
| Untrusted hooks (inherited from base) | `planned`, `trusted: false` | 3 |
| Subpath occupied by an existing file, no `--force` | `error` `conflicts` (unchanged `Plan`/apply) | 1 |
| Render / hook failure | `error` of that kind | 1 |
| Applied (with or without a captured snapshot) | `applied` | 0 |

No new exit code. No new permission, timeout, pinned-version check, or
subprocess. `generate` does **not** accept `--from` or `--baseline` — those are
the update axis (task 1029). Combining `generate` with `--from` is a usage error
(exit 2).

---

## 6. Behaviors to prove (falsifiable)

**B1 — twice in one project (mandated fixture).** In a clean git repo, `generate
web-app component ./src/components/Alpha` with `component_name=Alpha,
styled=true` produces a snapshot of source `web-app` at target
`src/components/Alpha`. Then `generate web-app component ./src/components/Beta`
with `--answers {component_name: Beta}` seeds `styled=true` from the Alpha
snapshot, writes Beta's files, and its own snapshot records
`styled=true, component_name=Beta`. The two snapshots share `source`, differ in
`target`. *Sole-kill:* if the second run ignores the first snapshot, `styled`
falls to the generator's own default and `defaults_from` is absent — the fixture
asserts `defaults_from` equals the Alpha snapshot id and `styled` came from
`seed`.

**B2 — snapshot beats configured.** With a `template-defaults` entry setting
`styled=false` for `web-app` and an Alpha snapshot recording `styled=true`, the
second application defaults `styled=true`. *Sole-kill:* if `DefaultBankEntry`
kept a single occupant and configured overwrote seed, `styled` would default
`false`.

**B3 — configured fallback when no snapshot value.** A generator question the
snapshot never recorded (the snapshot predates the question) defaults from
`template-defaults` when present, else from the template's own `default`.
*Sole-kill:* if the seam dropped configured defaults whenever a seed existed for
any id, this question would lose its configured default.

**B4 — source identity, not formal name.** A snapshot captured as
`web-app@v1` (formal name with a reference) seeds a `generate web-app@v2` run of
the same source. A snapshot of a different source is never used and, when named
by `--defaults-from`, fails exit 1 naming both sources. *Sole-kill:* comparing
the full formal name (`@reference` included) instead of `source()` rejects the
legitimate cross-version seed.

**B5 — deterministic script selection.** With three snapshots of the source, the
script route with no `--defaults-from` seeds from the highest ULID, with no
prompt, identically on repeated runs. *Sole-kill:* a content heuristic or
insertion-order pick makes the choice non-total or route-dependent.

**B6 — explicit require fails clearly.** `--defaults-from` with an unknown
prefix, an ambiguous prefix, a non-git target, or a wrong-source snapshot each
fails exit 1 and writes nothing. *Sole-kill:* falling back to normal questions
when an explicitly required snapshot is unsatisfiable violates the fixed policy.

**B7 — snapshot-absent still applies.** `generate` into a non-git path, and into
a dirty git target, both apply using configured/template/normal defaults and
save no snapshot, without error. *Sole-kill:* requiring a snapshot for every
`generate` breaks non-git and dirty targets.

**B8 — no `generators` is inert.** A template with no `generators` key loads and
plain `apply` behaves exactly as today; `generate` on it fails exit 2 naming
`apply`. *Sole-kill:* the loader rejecting the absent key (or `generate`
succeeding with an empty generator set) breaks backward compatibility.

**B9 — effective template overlay.** `generate web-app component` renders the
files under `generators/component/` and asks `component_name`/`styled`, never the
base `app_name`/`license`. *Sole-kill:* running the base source or base interview
means the overlay was not applied.

**B10 — looped values survive flattening.** A generator whose snapshot recorded a
`TextLoop` question (a JSON array under one id) seeds that array as the loop's
default. *Sole-kill:* flattening that splits or drops the array corrupts the loop
default.

**B11 — route parity.** Person, scripted, and agent routes produce identical
files and the identical captured snapshot for identical inputs and the identical
seed. The engine never receives a `Snapshot`, a generator name, or the source
identity. *Sole-kill:* any route diverging, or a snapshot/gitoxide type reaching
the engine, breaks the purity guarantee.

**B12 — agent resume re-reads the seed.** `generate --async`, then `continue`,
then `apply PATH` reproduces the seeded defaults from the recorded
`defaults_from`; a snapshot added between `stage` and `continue` does not change
the recorded choice. *Sole-kill:* re-selecting `latest` at resume (instead of the
recorded id) lets a later snapshot change an in-flight interview.

---

## 7. Compatibility, canonical documents, and out of scope

### 7.1 Backward compatibility

- `generators` is optional; absent = empty map = today's behaviour. Plain `apply
  TEMPLATE PATH` is untouched for every template, with and without generators.
- No dependent users exist (pre-1.0), so the closed-schema addition has no blast
  radius beyond the loader.
- Snapshot capture, `--from`, `--baseline`, replay, and the `snapshots` commands
  are unchanged — consumed read-only.

### 7.2 Canonical-document impact (described; applied by the paired implementation)

- `docs/specifications/template-format.schema.yml`: add an optional `generators`
  property (the 12th key; `additionalProperties: false` stays). Each generator:
  optional `description`, `source`, `suggested_target`, `messages`, and reused
  `interview`/`files`/`inject`/`static`/`ignore` sub-schemas.
- `docs/specifications/command-line-interface.yml` and `.spec.yml`: the
  `generate` verb, its positionals (`TEMPLATE GENERATOR SUBPATH`),
  `--defaults-from`, `--answers`, `--force`, `--async`, its results, and the
  exit mapping in §5.3.
- `docs/specifications/interview-protocol.yml` and schema: the `defaults_from`
  member on the `generate` result and the `generator`/`defaults_from` members on
  the staged record; the `default_source: "seed"` value on a seeded question.
- `docs/concepts/toha.yml`: the in-project generator concept.
- The glossary (`CLAUDE.md`/`AGENTS.md`): **Generator** (a template-declared,
  named alternate view applied repeatedly into subpaths) and **Effective
  template** (a base template with one generator's overlay applied).
- A user-guide page on generators and a generic fixture under
  `tests/fixtures/generate-*` (a template declaring a `component` generator, a
  first and second application, an `--answers` document, and `expect.yml`
  asserting B1–B2's `defaults_from` and seeded values).

### 7.3 Out of scope

- Snapshot capture, the `--from`/`--baseline` update merge, replay, and the
  `snapshots list|clean`/`init` commands (task 1029; consumed only).
- Answer-interpolated target patterns. The spine fixes the target before the
  interview, so `suggested_target` is a static person-route default and SUBPATH
  is an explicit argument on the script and agent routes.
- Per-generator hooks and per-generator trust. A generator inherits the base
  template's hooks and the base template's formal-name trust.
- A generator-name field on the snapshot (impossible without editing the frozen
  contract; see `rationale.md`). Snapshot selection is by `source` + recency.
- A second persisted-answers lifecycle; content-based or sibling-subpath seed
  selection; new permissions, timeouts, pinned-version checks, or subprocesses.
