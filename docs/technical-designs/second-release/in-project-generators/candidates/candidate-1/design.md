# In-project generators — design (candidate: minimal CLI surface)

A generator is not a new concept. It is an **invocation pattern** over a plain
template: apply the same template repeatedly into subpaths of one project, each
application carrying its own answers, and — optionally — seeding the new
application's answer **defaults** from a snapshot of an earlier application.

The whole feature is one new flag on `apply`/`stage`, `--like`, a **sibling of
`--from`** and never an overload of it. `--from` re-renders into the *same*
target and merges in place (the update axis, owned by task 1029). `--like`
writes a *fresh* application into the given subpath and only pre-fills its answer
defaults from a selected snapshot (the generate axis). Per-application identity
is **derived** from the snapshot `source` plus the target subpath — the same
two facts a snapshot is already keyed by. No new `template.yml` surface, no new
verb, no new persisted-answers lifecycle.

---

## 1. Caller usage (the spec)

### Route A — person terminal: first application, then a second seeded from it

```console
$ toha apply gh:example/widget ./src/widgets/alpha
? Label: Alpha
? Style: › card
? With tests: (Y/n) y
src/widgets/alpha/mod.rs
src/widgets/alpha/alpha.test.txt
snapshot 01J9Z4K7QX (widget 3f9a1c2)
```

A second widget, defaulting from the first. `--like latest` selects the newest
snapshot of the **same source** in this project; each seeded default is shown in
parentheses and the person accepts or overrides it:

```console
$ toha apply gh:example/widget ./src/widgets/beta --like latest
seed defaults from snapshot 01J9Z4K7QX (widget)
? Label: (Alpha) Beta
? Style: (card) › card
? With tests: (Y) y
src/widgets/beta/mod.rs
src/widgets/beta/beta.test.txt
snapshot 01JA2B8M4R (widget 3f9a1c2)
```

Only `Label` was overridden; `Style` and `With tests` took the seeded defaults.
Two applications now exist: two snapshots with equal `source`
(`gh:example/widget`) and different `target` (`src/widgets/alpha`,
`src/widgets/beta`). That pair **is** the per-application identity; nothing new
was recorded to make it.

### Route B — scripted `--answers`: seed from an explicit snapshot, override one id

The scripted machine route is selected by `--answers`, not `--json`. The answers
document supplies only the ids the caller wants to **change**; every other
question takes its seeded default (auto-applied in the headless batch walk):

```console
$ toha apply gh:example/widget ./src/widgets/beta --like 01J9Z4 --answers beta.json
```

`beta.json`:

```json
{ "protocol": 1, "template": "gh:example/widget", "answers": { "label": "Beta" } }
```

Result document (one document, as today; `applied` gains a `seed` member):

```json
{ "protocol": 1, "status": "applied", "target": "src/widgets/beta",
  "seed": { "from": "01J9Z4K7QX6M2V8R0T5B3N1P9D" },
  "files": ["src/widgets/beta/mod.rs", "src/widgets/beta/beta.test.txt"],
  "snapshot": { "id": "01JA2B8M4R0C7W1Y5F3H9K2S6E" } }
```

`style` and `with_tests` were not in `beta.json`; the walk applied their seeded
values from snapshot `01J9Z4…`. `label` was replaced by the document. A seeded
default the **new** template version rejects, and that the document does not
override, is returned as a remaining question (`status: "questions"`, exit 4) —
the seed never forces an invalid value through.

### Route C — agent staged, and the snapshot-absent fallback

```console
$ toha stage gh:example/widget ./src/widgets/gamma --like latest --async
{ "protocol":1, "status":"questions", "seed":{"from":"01JA2B8M4R..."}, "context":{...},
  "questions":[ {"id":"label","default":"Beta",...}, ... ] }
$ toha continue ./src/widgets/gamma answers.json
$ toha apply ./src/widgets/gamma
```

The staged interview records **which** snapshot it seeded from (a selector, not
answers), so `continue`/`apply PATH` rebuild the identical seeded defaults.

Fallback when no snapshot exists — a non-git target, a dirty clone that has not
fetched snapshot refs, or a source with no prior application. Omitting `--like`
always applies normally; it never pretends a snapshot existed:

```console
$ toha apply gh:example/widget ./src/widgets/delta
? Label: Delta
...                                  # normal questions (+ any configured template-defaults)
```

An **explicitly required** selector that cannot be satisfied fails clearly
instead of silently falling back:

```console
$ toha apply gh:example/widget ./src/widgets/delta --like latest
error: no snapshot of gh:example/widget to seed from
       run: toha snapshots list .                                # exit 1
```

---

## 2. Data structures expected

All snapshot parsing happens at the CLI boundary and produces domain types; no
`toha::snapshot`, gitoxide, ULID, or wire type reaches the interview engine.

```rust
// src/cli/like.rs — NEW, crate-private CLI-side boundary. The ONLY code outside
// toha::snapshot that names Snapshot/Project.

/// How `--like` names the snapshot to seed from. Parsed once from the CLI.
pub enum LikeSelector {
    /// A full id or a >= 6-char prefix. Must resolve to exactly one snapshot of
    /// the template's source identity. Unsatisfiable → clear failure (exit 1).
    Reference(String),
    /// The newest snapshot of the template's source in this project.
    Latest,
    /// Person route only: pick from the source's snapshots (likely base
    /// preselected), or choose "none".
    Prompt,
}

/// The resolved seed: a snapshot's raw answers folded to one default per id,
/// plus the id it came from (for reporting and the staged record).
pub struct SnapshotSeed {
    /// Snapshot `submissions` folded left-to-right, later batch wins per id.
    /// Exactly the `Seed.defaults` shape the engine already consumes.
    pub defaults: IndexMap<Id, RawAnswer>,
    pub from: SnapshotId,
}

/// Why a `--like` request could not be honoured. Each variant names the
/// snapshot or the reason; none carries answer values or file contents.
pub enum LikeError {
    NoProject,                       // target not in git
    None { source: String },         // required selector, no matching-source snapshot
    Unknown { reference: String },   // prefix matched nothing
    Ambiguous { reference: String }, // prefix matched several
    Invalid { id: SnapshotId, reason: String },
    WrongSource { want: String, got: String, id: SnapshotId },
}
```

The route enum already exists conceptually (person / scripted / agent); the
boundary only needs to know whether interaction is allowed.

### The seam extension on the interview library

`Seed`, `RawAnswer`, and `DefaultBankEntry` are **unchanged**:

```rust
// src/interview.rs — UNCHANGED, shown for the seam.
pub struct Seed { pub now: jiff::Zoned, pub defaults: IndexMap<Id, RawAnswer>, pub context: InvocationContext }
enum DefaultBankEntry { Seed(RawAnswer), Configured(ResolvedDefault) } // still ONE occupant per id
```

One new method on `Resolution` layers snapshot defaults over configured ones,
snapshot winning **per id**, so no id ever needs two occupants:

```rust
impl Resolution {
    /// Start the interview with snapshot-sourced defaults layered over the
    /// configured defaults. Precedence is resolved here by subtraction:
    ///   * every id in `snapshot_defaults` becomes a `DefaultBankEntry::Seed`
    ///     and SHADOWS any configured entry for that id;
    ///   * every remaining configured id keeps its `DefaultBankEntry::Configured`
    ///     provenance and behaviour exactly as `start_with_context` gives it.
    /// When `snapshot_defaults` is empty this is byte-for-byte identical to
    /// `start_with_context` — the no-`--like` path is untouched.
    /// Invariant: for any id, the resulting bank holds exactly one entry.
    pub fn start_with_seed<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
        snapshot_defaults: IndexMap<Id, RawAnswer>,
    ) -> Result<Interview<'a>, EvalError> { unimplemented!() }
}
```

### The staged record gains one optional selector (not answers)

```rust
// src/staging.rs — StagedRecord gains ONE optional member.
pub struct StagedRecord {
    // target, template (formal name), commit, named, now, submissions, context — as today
    /// The snapshot `--like` seeded from, if any. A SELECTOR, re-read from git
    /// on resume; the record stores no prior-application answers. Orthogonal to
    /// task 1029's `base`: `--like` (generate) and `--from` (update) never combine.
    pub seed: Option<SnapshotId>,   // NEW, optional, additive
}
```

---

## 3. Public interfaces expected

```rust
// src/cli/like.rs — resolve `--like` to seed defaults, consuming ONLY the
// snapshot reader contract. This is the exact submissions → defaults seam.
//
// `template_source` is the formal name WITHOUT its @reference (Snapshot::source
// identity). `project` is None when the target is not in git.
//
// Returns Ok(None) when no seeding applies (flag absent, or Prompt with the
// person declining / nothing to offer). Returns Ok(Some(..)) with the folded
// defaults and the source snapshot id. Errors are the required-but-unsatisfiable
// cases, each mapped to an exit code by the CLI.
pub fn resolve_like(
    project: Option<&toha::snapshot::Project>,
    template_source: &str,
    selector: Option<LikeSelector>,
    interactive: bool,
) -> Result<Option<SnapshotSeed>, LikeError> { unimplemented!() }

/// Fold a snapshot's raw submissions into one default per id. Later batches win;
/// a looped question's JSON array is carried as that id's array default. Pure,
/// total, no validation (the new interview re-validates on submit).
fn fold_submissions(submissions: &[IndexMap<Id, serde_json::Value>]) -> IndexMap<Id, RawAnswer> {
    let mut out = IndexMap::new();
    for batch in submissions {
        for (id, value) in batch { out.insert(id.clone(), RawAnswer(value.clone())); }
    }
    out
}
```

The engine-facing signatures used by the spine are the existing ones:
`configured_defaults(formal_name, template, presets, mappings) -> Result<Resolution, EvalError>`
and the new `Resolution::start_with_seed`. No snapshot or gitoxide type appears
in any interview-engine signature; the engine still receives only
`IndexMap<Id, RawAnswer>` through `Seed.defaults` / the default bank.

### How a snapshot default becomes an engine default (the realized path)

1. The CLI resolves `--like` to `Option<SnapshotSeed>` at the boundary
   (`resolve_like`), parsing the `Snapshot` into `IndexMap<Id, RawAnswer>` via
   `fold_submissions`. Snapshot types stop here.
2. `configured_defaults(...)` runs exactly as today, yielding a `Resolution`
   (configured presets/template-defaults, with its warnings and attribution).
3. `Resolution::start_with_seed(template, now, context, seed.defaults)` builds
   the bank: snapshot ids → `Seed` entries (shadowing configured), other
   configured ids → `Configured` entries. When there is no seed, the caller
   passes an empty map and the call is identical to `start_with_context`.
4. `render_default` consults the bank first (as today); a `Seed`-sourced default
   is shown to the person and, in the headless walk, auto-applied per batch. A
   `Seed` default the new template rejects yields the engine's existing
   "default … is not allowed" rejection — so it re-asks (person) or returns as a
   remaining question (exit 4), never a silent wrong value.

---

## 4. Module and seam diagram

```text
CLI  apply / stage  (NEW flag: --like [SELECTOR])
  │  setup · canonical_target · resolve_template · load_template      [reused spine]
  │  resolution(configured_defaults)                                   [reused]
  │  build_context                                                     [reused]
  │
  ├── src/cli/like.rs  ── resolve_like(project, source, selector, interactive)   [NEW]
  │        │  consumes the snapshot READER contract only:
  │        ▼
  │   toha::snapshot::{ Project::open · snapshots · find · likely_bases,          [CONSUMED, task 1029]
  │                     Snapshot::source · target · submissions · id · created }
  │        │  fold_submissions → IndexMap<Id, RawAnswer>   (snapshot types stop here)
  │        ▼
  └── Resolution::start_with_seed(template, now, context, snapshot_defaults)      [NEW method]
           │  snapshot ids → DefaultBankEntry::Seed (shadow configured);
           │  empty seed ≡ start_with_context (no---like path byte-identical)
           ▼
       PURE INTERVIEW ENGINE — Seed.defaults / default bank / render_default      [UNCHANGED]
           ▼
       Plan::build (single target = the subpath) · apply_reporting                [reused]

staging: StagedRecord.seed: Option<SnapshotId>   [NEW optional selector; re-read on resume]
```

New: one flag, one boundary module (`resolve_like` + `fold_submissions`), one
`Resolution` method, one optional staged member, one `applied`/`questions`
result member. Reused unchanged: the whole five-step apply spine, the pure
engine, `Plan` (single-target), `configured_defaults`, every route adapter.
Consumed, never edited: the `toha::snapshot` reader. Untouched: snapshot
capture, `--from`, the replay/merge, and the `snapshots` commands (all task
1029); `template.yml` and its closed 11-key schema.

---

## 5. Selection, precedence, collisions, and the error / results contract

### Total precedence, per question id

For each question the template defines and asks (after `when`/batching):

1. an **answer submitted by the route** (person typed, `--answers` value, agent
   `continue`) — always wins over any default;
2. else the **selected snapshot's** value for that id (a `Seed` bank entry);
3. else the **configured** default for that id (`template-defaults` / presets,
   a `Configured` bank entry, keyed on the formal name);
4. else the **template's own `default`** expression;
5. else no default (required → asked / rejected; optional → empty).

This is total and deterministic on every route. Snapshot-vs-configured never
collide per id because step 2 removes the id from step 3's set at the boundary;
`DefaultBankEntry` keeps its single occupant.

### Snapshot selection by route and snapshot count (matching `source`)

| matching snapshots | person, no `--like` | person, `--like` (bare) | `--like latest` (any route) | `--like <id/prefix>` (any route) | script/agent, no `--like` |
|---|---|---|---|---|---|
| 0 | plain apply, normal questions | picker offers nothing → proceed normal | required-absent → **exit 1** | unknown → **exit 1** | plain apply, normal questions |
| 1 | plain apply, normal questions | picker preselects it; seed or "none" | seed from it | seed if prefix matches & source ok | plain apply, normal questions |
| N (same source) | plain apply, normal questions | list N, preselect likely base | seed from the **newest** | seed the unique match; multi-match prefix → ambiguous **exit 1** | plain apply, normal questions |

Plain `apply` never starts prompting about generators; seeding is opt-in via
`--like`. Bare `--like` (no selector) is a **person-only** convenience; on the
script/agent routes it is a usage error so selection is always non-interactive
and deterministic there.

### Failure cases → exit codes

| Condition | Route | Result document | Exit |
|---|---|---|---|
| `--like <id/prefix>` unknown, ambiguous, or invalid snapshot | all | `error` `snapshot` naming the reference | 1 |
| `--like latest` and no snapshot of the template's source | all | `error` naming `toha snapshots list` | 1 |
| selected snapshot's `source` ≠ template's source identity | all | `error` naming both sources | 1 |
| target not in git and `--like <selector>` is explicit | all | `error` (no project) | 1 |
| `--like` bare (no selector) | script / agent | `error` usage (name a selector or `latest`) | 2 |
| `--like` combined with `--from` or `--baseline` | all | `error` usage | 2 |
| a seeded default the new template rejects, not overridden | script / agent | `questions` (the batch, the rejection) | 4 |
| a seeded default the new template rejects, not overridden | person | re-prompted with the default shown | 0 / 4 |
| subpath already occupied, no `--force` | all | `error` conflicts (unchanged) | 1 |
| template has untrusted hooks | all | `planned`, `trusted: false` (unchanged) | 3 |
| template name ambiguous (short-name collision) | all | `ambiguous` (unchanged) | 5 |
| resume finds the recorded seed snapshot missing/invalid | agent | `error` `snapshot` | 1 |

Snapshot ambiguity is **exit 1**, not exit 5: exit 5 is reserved for template
short-name collisions in `registry.resolve`. `--like` never re-enters template
resolution.

The `applied` and `questions` result documents gain one optional member,
`seed: { "from": "<snapshot id>" }`, present only when `--like` seeded. It
carries no answer values.

---

## 6. Behaviors to prove (falsifiable), with a sole-kill per load-bearing rule

**Mandated fixture — same generator twice in one project, second seeded from the
first.** `tests/fixtures/generator-twice/`: a `widget` template
(`label`, `style` select, `with_tests` confirm); a clean git repo. Apply into
`src/widgets/alpha` with `{label:"Alpha", style:"card", with_tests:true}`;
commit. Apply into `src/widgets/beta` with `--like <alpha id>` and
`--answers {label:"Beta"}`. Assert: `beta/` renders `label=Beta`,
`style=card`, `with_tests=true` (the two unspecified ids took the snapshot
defaults); `alpha/` is unchanged; two snapshots exist with equal `source` and
`target ∈ {alpha, beta}`; person, scripted, and agent routes produce identical
`beta/` trees for the same inputs. *Fails if the second application does not
inherit `style`/`with_tests`, or if the routes diverge.*

1. **Seed-not-replay.** A seeded default is a default, not a submitted answer:
   the person can change it, and `--answers` overrides it. *Sole-kill:* drive
   `--like` through the `--from` replay adapter (recorded answers submitted
   whole, a rejected value fatal) → `beta` cannot override `label` and a
   template that tightened a rule aborts instead of re-asking; fails the fixture
   and B7.
2. **Precedence snapshot > configured > template.** With a configured
   `template-defaults` for `style` and a snapshot value for `style`, the
   snapshot value wins; with only a configured value, it is used; with neither,
   the template default. *Sole-kill:* let the configured entry shadow the
   snapshot → `style` shows the configured value; fails.
3. **Answer beats seed.** `--answers {label:"Beta"}` yields `Beta` though the
   snapshot has `Alpha`. *Sole-kill:* apply seed after route answers → `beta`
   gets `Alpha`; fails.
4. **Source identity, not formal name.** A snapshot of `gh:example/widget@v1`
   seeds an apply of `gh:example/widget@v2` (equal `source`); a snapshot of
   `gh:example/other` is refused (exit 1). *Sole-kill:* compare `template`
   (with `@reference`) instead of `source` → the same-source/different-ref seed
   is refused; fails.
5. **Generate axis, not update axis.** `--like` seeds from a snapshot whose
   `target` differs from the new subpath, and writes a fresh application there
   (no merge, no in-place re-render). *Sole-kill:* require `target` to match
   (as `--from` does) → seeding `beta` from `alpha`'s snapshot is refused; fails
   — proves `--from` is not overloaded.
6. **Fresh instant.** The new application uses the normal `now` and gets its own
   `generated`; it does **not** inherit the seed snapshot's frozen instant.
   *Sole-kill:* carry `generated` from the seed snapshot → a date-rendering
   widget freezes `beta` to `alpha`'s instant; fails.
7. **Rejected seed fails clearly.** A snapshot value invalid for the new
   template version, not overridden, re-asks (person) or returns `questions`
   (exit 4); it is never written. *Sole-kill:* auto-accept the invalid seed →
   an invalid file is rendered; fails.
8. **No-`--like` path is byte-identical.** With `--like` absent, apply,
   configured-default provenance, and every route result are exactly today's.
   *Sole-kill:* always flatten configured defaults through the seed path → a
   rejected configured default's message/provenance changes and driver parity
   breaks; fails B12 of template-defaults.
9. **Required-absent vs optional-absent.** `--like latest` with no matching
   snapshot → exit 1; `--like` bare on script/agent → exit 2; `--like` absent
   entirely → plain apply (exit 0 path). *Sole-kill:* silently proceed when an
   explicit selector is unsatisfiable → a required seed is dropped; fails.
10. **Single-occupant bank.** For an id with both a snapshot and a configured
    default, the bank holds exactly one entry (Seed). *Sole-kill:* keep both →
    the id is prepared twice / provenance is ambiguous; fails.
11. **Extra seed ids ignored, unknown configured ids still warn.** A snapshot
    carrying an id the new template dropped is silently ignored; a configured
    mapping for an id the template lacks still warns. *Sole-kill:* error on the
    extra snapshot id → ordinary template evolution breaks.
12. **Plan stays single-target.** One `--like` apply writes one subpath; the
    `Plan`'s context target equals the subpath. *Sole-kill:* fan `--like` out to
    several targets in one invocation → `PlanError::ContextTarget`; fails.
13. **Resume re-seeds.** Agent `stage --like latest` records the snapshot id;
    `continue`/`apply PATH` rebuild the identical seeded defaults for the still-
    pending questions. *Sole-kill:* do not persist the selector / do not re-seed
    on resume → pending questions lose their snapshot defaults on `continue`;
    fails.
14. **Reader-only.** The whole run reads snapshot refs and starts no process
    except a trusted template's hooks; it never writes a ref, never captures,
    never merges. *Sole-kill:* any write to `refs/toha/snapshots/*` during a
    `--like` apply; fails.

---

## 7. Compatibility and canonical-document impact

Toha is before 1.0.0 with no dependent users. Plain `apply` and `stage` keep
their behaviour exactly; the flag is additive and opt-in. The paired
implementation updates, in one change:

- `docs/specifications/command-line-interface.yml` and `.spec.yml`: `--like
  [SELECTOR]` on `apply` and `stage`, its route rules, the selection/precedence
  table, and the new exit-code rows (all within 0–5).
- `docs/specifications/interview-protocol.yml` and schema: the optional `seed`
  member on `applied` and `questions`; the optional `seed` member on the staged
  record.
- `docs/technical-designs/architecture.yml`: the `src/cli/like.rs` boundary that
  consumes the `toha::snapshot` reader and the `Resolution::start_with_seed`
  seam.
- The repository glossary (`AGENTS.md`): **Generator** (an apply pattern that
  writes one template repeatedly into project subpaths; not a `template.yml`
  object), **Seed selector** (`--like`, names a snapshot whose answers pre-fill
  the new application's defaults), **Snapshot-seeded default** (a default that
  enters the engine as a `Seed` bank entry from a snapshot's submissions).
- `docs/concepts/toha.yml`: the generator usage pattern.
- A user-guide page on generating into subpaths, with a generic example, and the
  `tests/fixtures/generator-*` harness (`template/`, `answers-*.json`,
  `expected/`, `expect.yml`).

`docs/specifications/template-format.schema.yml` is **unchanged**: no `kind`, no
`generator` key, no second entrypoint. The closed 11-key schema and the "one
source, one interview" load assumption are preserved.

Coordination with task 1029: `--like` consumes the reader contract only and adds
`StagedRecord.seed`, orthogonal to 1029's `StagedRecord.base`; the two flags are
mutually exclusive (exit 2 if combined), so no field or code path conflicts.

---

## 8. Out of scope

- Snapshot capture, `--from`/`--baseline`, the replay/merge adapter, and the
  `snapshots`/`init` commands (task 1029); `--like` consumes the reader only.
- Any `template.yml` generator/`kind` concept, sub-templates, or multiple
  entrypoints.
- A second persisted-answers lifecycle; `--like` persists only a snapshot *id*
  in the staged record and re-reads answers from git.
- Multi-target fan-out in one invocation; a `Plan` is single-target.
- Combining `--like` with `--from`/`--baseline`.
- Inferring identity or reusing answers by matching question ids across
  templates or applications.
- Cross-project seeding; `--like` reads snapshots of the target's own
  repository.
- New permissions, timeouts, pinned-version checks, or application subprocesses
  — this design introduces none.
