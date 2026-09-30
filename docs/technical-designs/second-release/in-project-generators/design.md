---
relationships:
  depends-on:
    - project-updates
    - template-defaults
  informs: in-project-generators
---

# In-project generators

## Purpose

A generator is not a new kind of template. It is a way to **apply one template
repeatedly into subpaths of an existing project** — "add a widget", "add a
module" — where each application carries its own answers and its own identity,
and where a new application can **seed its answer defaults from a snapshot of an
earlier application**.

Toha already applies a template into a subpath: `toha apply TEMPLATE SUBPATH`
creates the files, and — in a clean git target — saves a snapshot of that
application under a Toha-owned ref. The only thing missing to "make another one
like the last" is a way to pre-fill the new application's answers from a prior
one. That is a *defaults* problem, and Toha already has a defaults boundary
(`Seed.defaults` / the interview default bank). So the whole feature is one new
flag on `apply` and `stage`, **`--like`**, that feeds a selected snapshot's
recorded answers into that boundary as defaults.

`--like` is a **sibling of `--from`, never an overload of it**. `--from` (the
update axis, task 1029) re-renders into the *same* target and three-way merges in
place. `--like` (the generate axis) writes a *fresh* application into the given
subpath and only pre-fills its answer defaults; it never merges. The two are
mutually exclusive.

Consuming: project-updates design **revision 3** (Bob-approved) with completeness
correction **1110**, integrated at the epic base — the reader contract
`toha::snapshot::{Project, Snapshot}` only. Snapshot capture, the `--from` update
merge, the update replay adapter, and the `snapshots`/`init` commands are owned
by the paired producer implementation (task 1029); this design consumes, and does
not edit, any of them.

## Caller usage

### First application, then a second seeded from it (person route)

```console
$ toha apply gh:example/widget ./src/widgets/alpha
? Label: Alpha
? Style: › card
? With tests: (Y/n) y
src/widgets/alpha/mod.txt
src/widgets/alpha/alpha.test.txt
snapshot 01J9Z4K7QX6M2V8R0T5B3N1P9D (widget 3f9a1c2)
```

The apply saved a snapshot because the target was a clean git target — that is
the producer's own capture rule and output, unchanged. Now a second widget,
defaulting from the first. `--like latest` selects the newest snapshot of the
**same source** in this project; each seeded default is shown in parentheses and
the person accepts or overrides it:

```console
$ toha apply gh:example/widget ./src/widgets/beta --like latest
seed defaults from snapshot 01J9Z4K7QX6M2V8R0T5B3N1P9D (widget, src/widgets/alpha)
? Label: (Alpha) Beta
? Style: (card) › card
? With tests: (Y) y
src/widgets/beta/mod.txt
src/widgets/beta/beta.test.txt
snapshot 01JA2B8M4R0C7W1Y5F3H9K2S6E (widget 3f9a1c2)
```

Only `Label` was overridden; `Style` and `With tests` took the seeded defaults.
Two applications now exist: two snapshots with equal `source`
(`gh:example/widget`) and different `target` (`src/widgets/alpha`,
`src/widgets/beta`). **That pair is the per-application identity; nothing new was
recorded to make it.** The second application saved its own snapshot, so a third
could seed from it in turn.

### Scripted `--answers`: seed from an explicit snapshot, override one id

The scripted machine route is selected by `--answers` (not `--json`). The answers
document supplies only the ids the caller wants to **change**; every other
question takes its seeded default:

```console
$ toha apply gh:example/widget ./src/widgets/beta \
    --like 01J9Z4K7QX6M2V --answers beta.json
```

`beta.json`:

```json
{ "protocol": 1, "template": "gh:example/widget", "answers": { "label": "Beta" } }
```

Result document (one document, as today; `applied` gains an optional `seed`
member):

```json
{ "protocol": 1, "status": "applied", "target": "src/widgets/beta",
  "seed": { "from": "01J9Z4K7QX6M2V8R0T5B3N1P9D" },
  "files": ["src/widgets/beta/mod.txt", "src/widgets/beta/beta.test.txt"],
  "snapshot": { "id": "01JA2B8M4R0C7W1Y5F3H9K2S6E" } }
```

`style` and `with_tests` were not in `beta.json`; the walk applied their seeded
values. `label` was replaced by the document. `--like` takes a full 26-character
ULID or any prefix of **at least six characters** that matches exactly one
snapshot of the template's source; a shorter or ambiguous prefix fails clearly
(below).

### Agent staged route, and the snapshot-absent fallback

```console
$ toha stage gh:example/widget ./src/widgets/gamma --like latest --async
{ "protocol":1, "status":"questions", "seed":{"from":"01JA2B8M4R0C7W1Y5F3H9K2S6E"},
  "context":{...}, "questions":[ {"id":"label","default":"Beta",...}, ... ] }
$ toha continue ./src/widgets/gamma answers.json
$ toha apply ./src/widgets/gamma
```

The staged interview records **which** snapshot it seeded from (a pinned id, not
answers), so `continue`/`apply PATH` rebuild the identical seeded defaults by
re-reading that exact snapshot from git. A snapshot added between `stage` and
`apply PATH` does not change the pinned choice.

Fallback when no snapshot is available — a non-git target, a clone that has not
fetched snapshot refs, or a source with no prior application. Omitting `--like`
always applies normally and never pretends a snapshot existed:

```console
$ toha apply gh:example/widget ./src/widgets/delta
? Label: Delta
...                       # normal questions (plus any configured template-defaults)
```

A **dirty** git target still reads existing snapshots to seed defaults; only the
new application's snapshot *capture* is skipped (the producer's rule). An
explicitly **required** selector that cannot be satisfied fails clearly instead
of silently falling back:

```console
$ toha apply gh:example/widget ./src/widgets/delta --like latest
error: no snapshot of gh:example/widget to seed from        # exit 1
```

## Data structures

All snapshot reading and parsing happen at the CLI-side boundary and produce
domain types; no `toha::snapshot`, gitoxide, ULID, or wire type reaches the
interview engine.

```rust
// src/cli/like.rs — NEW, crate-private CLI-side boundary. The only code outside
// toha::snapshot that names Snapshot / Project on the generate axis.

/// How `--like` names the snapshot to seed from. Parsed once from the CLI.
pub enum LikeSelector {
    /// A full id or a >= 6-char prefix; must resolve to exactly one snapshot of
    /// the template's source identity.
    Reference(String),
    /// The newest snapshot of the template's source in this project (max ULID).
    Latest,
    // A bare `--like` (no value) is a person-route picker, resolved in the CLI
    // adapter to one of the above or to "none"; it is a usage error on the
    // script and agent routes.
}

/// The resolved seed: a snapshot's raw answers folded to one default per id,
/// plus the id and label it came from (for reporting and the staged record).
pub struct SnapshotSeed {
    /// `submissions` folded left-to-right, a later batch winning per id;
    /// exactly the `Seed.defaults` shape the engine already consumes.
    pub defaults: IndexMap<Id, RawAnswer>,
    pub from: SnapshotId,
    pub label: String,               // "01J9Z4K7QX6M2V8R0T5B3N1P9D (widget, src/widgets/alpha)"
}

/// Why a `--like` request could not be honoured. Each variant names the snapshot
/// or the reason; none carries answer values or file contents.
pub enum LikeError {
    NoProject,                        // target not in git, explicit selector given
    None { source: String },          // required selector, no matching-source snapshot
    Unknown { reference: String },    // prefix matched nothing
    Ambiguous { reference: String },  // prefix matched several
    Invalid { id: SnapshotId, reason: String },
    WrongSource { want: String, got: String, id: SnapshotId },
}
```

### The default bank gains one additive variant (engine-internal, private)

```rust
// src/interview.rs — one additive private variant. The public `Seed`
// (`defaults: IndexMap<Id, RawAnswer>`) is UNCHANGED.
enum DefaultBankEntry {
    Seed(RawAnswer),                     // crate/replay seed, as today
    Configured(ResolvedDefault),         // presets / template-defaults, origin-bearing, as today
    Snapshot { raw: RawAnswer, from: String },   // NEW: a snapshot-seeded default, provenance-bearing
}

/// Mirrors the bank variant so a prompt and a rejection can name the snapshot.
enum PreparedDefaultSource {
    Template { expression: Option<String> },
    Configured(ConfiguredDefaultOrigin),
    Seed,
    Snapshot { from: String },           // NEW
}
```

The bank still holds **exactly one occupant per id** — precedence is resolved at
the seam before the bank is built, so a `Snapshot` entry and a `Configured` entry
never coexist for one id. The `Snapshot` variant exists for provenance, not
coexistence, and never appears on the no-`--like` path, so the configured-default
behaviour and attribution the template-defaults design pins down are untouched.

### The staged record gains one optional pinned selector (not answers)

```rust
// src/staging.rs — StagedRecord gains ONE optional member.
pub struct StagedRecord {
    // target, template (formal name), commit, named, now, submissions, context — as today
    /// The snapshot `--like` seeded from, if any. A pinned id, re-read from git on
    /// resume; the record stores no prior-application answers. Orthogonal to the
    /// update axis's `base` (task 1029): `--like` (generate) and `--from` (update)
    /// never combine, so the two optional members never both apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SnapshotId>,        // NEW, optional, additive
}
```

## Public interfaces

```rust
// src/cli/like.rs — pure, reader-only selection. Consumes ONLY the snapshot
// reader contract; NO prompting (the person picker lives in the CLI adapter and
// calls these), so the surface is crate-callable and free of UI. A crate caller
// drives the same selection a command does.

/// Snapshots of `source` in this repository, newest first (ULID descending).
/// Reads the repository-wide snapshot set (all `refs/toha/snapshots/*`) through
/// the producer reader and keeps the Valid ones whose `source()` equals `source`.
/// Repository-wide, not target-scoped: a prior application lives at a DIFFERENT
/// subpath, so its snapshot's `target` differs from the new one.
pub fn candidates(project: &toha::snapshot::Project, source: &str)
    -> Result<Vec<toha::snapshot::Snapshot>, toha::snapshot::ProjectError> { unimplemented!() }

/// The automatic, non-interactive choice (script/agent routes; preselected for
/// the person): the newest snapshot of the source, or None when there are none.
/// Total and deterministic because ULIDs are a total creation order.
pub fn latest(candidates: &[toha::snapshot::Snapshot]) -> Option<&toha::snapshot::Snapshot> { unimplemented!() }

/// Resolve an explicit `--like <id/prefix>` against the repository and verify the
/// source. Fails clearly per each `LikeError`. Never prompts.
pub fn find_required(project: Option<&toha::snapshot::Project>, source: &str, reference: &str)
    -> Result<toha::snapshot::Snapshot, LikeError> { unimplemented!() }

/// Fold a snapshot's raw submissions into one default per id. Later batches win;
/// a looped question's recorded JSON array stays one array value under its id (a
/// valid loop default). Pure, total, no validation — the new interview
/// re-validates and re-formats on submit through the unchanged engine.
fn fold_submissions(submissions: &[IndexMap<Id, serde_json::Value>]) -> IndexMap<Id, RawAnswer> {
    // for batch in submissions { for (id, value) in batch { out.insert(id, RawAnswer(value)) } }
    unimplemented!()
}
```

```rust
// src/interview.rs — the ONE engine seam, a sibling of Resolution::start_with_context.
// It is the single place a snapshot seed and the configured resolution meet, and
// the single place generator precedence is realized. The pure engine below stays
// snapshot-, preset-, and identity-unaware.
impl Resolution {
    /// Start an interview with snapshot-sourced defaults layered over the
    /// configured defaults. For each question the template defines:
    ///   1. a snapshot value that parses to the question's kind  -> Snapshot entry
    ///      (shadows any configured entry for that id);
    ///   2. else the configured default (this Resolution)        -> Configured entry;
    ///   3. else the template's own `default` expression         -> no bank entry;
    ///   4. else the question is asked.
    /// A snapshot value whose kind no longer matches the question is DROPPED and
    /// returned as a warning (snapshots are optional defaults), falling through to
    /// (2)/(3) — it is never a hard error and never forces an invalid value. The
    /// single-occupant bank invariant is preserved: the winner is chosen here.
    /// With `seed == None` this is byte-for-byte `start_with_context`, so the
    /// no-`--like` path and driver parity are unchanged.
    pub fn start_with_seed<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
        seed: Option<SnapshotSeed>,
    ) -> Result<(Interview<'a>, Vec<String>), EvalError> { unimplemented!() }  // Vec: dropped-id warnings
}
```

`configured_defaults`, `Interview::start`, `Pending::answer`, `Plan::build`,
`Plan::apply`, every route adapter, and the `Seed` type keep their signatures.
`render_default` and `Pending::answer` gain a `Snapshot { from }` match arm that
behaves like the `Seed` arm but names the snapshot in its message.

### How a snapshot default becomes an engine default (the realized path)

1. The CLI resolves `--like` at the boundary: `find_required` for a reference,
   `latest(candidates(...))` for `latest`, the picker for a bare `--like`. The
   selected `Snapshot` is folded by `fold_submissions` into
   `IndexMap<Id, RawAnswer>`. **Snapshot types stop here.**
2. `configured_defaults(formal_name, template, presets, mappings)` runs exactly as
   today, yielding a `Resolution` with its warnings and attribution.
3. `Resolution::start_with_seed(template, now, context, seed)` builds the bank:
   snapshot ids that kind-match → `Snapshot` entries (shadowing configured);
   remaining configured ids → `Configured` entries; wrong-kind snapshot ids
   dropped with a warning. With no seed the call equals `start_with_context`.
4. The pure engine drives as today. A `Snapshot`-sourced default is shown to the
   person and auto-applied per batch on the headless walk; if the new template
   version rejects it on a constraint, the engine re-asks (person) or returns it
   as a remaining question (exit 4) — never a silent wrong value.

## Module and seam

```text
CLI  apply / stage   (NEW flag: --like [SELECTOR]; mutually exclusive with --from/--baseline)
  │  setup · canonical_target · resolve_template · load_template      [reused spine]
  │  resolution(configured_defaults)                                  [reused]
  │  build_context                                                    [reused]
  │
  ├── src/cli/like.rs   candidates · latest · find_required · fold_submissions   [NEW]
  │        │  pure, reader-only; person picker lives in the CLI adapter, calls these
  │        ▼  consumes the snapshot READER contract only (task 1029):
  │   toha::snapshot::{ Project::open · snapshots (repo-wide) · find · likely_bases ,
  │                     Snapshot::source · target · submissions · id · created }
  │        │  fold_submissions → IndexMap<Id, RawAnswer>   (snapshot types stop here)
  │        ▼
  └── Resolution::start_with_seed(template, now, context, Option<SnapshotSeed>)   [NEW seam]
           │  precedence resolved here; DefaultBankEntry::Snapshot (additive);
           │  seed == None ≡ start_with_context (no-`--like` path byte-identical)
           ▼
       PURE INTERVIEW ENGINE — Seed.defaults / default bank / render_default      [+1 match arm]
           ▼
       Plan::build (single target = the subpath) · apply_reporting · producer CAPTURE [reused]

staging: StagedRecord.seed: Option<SnapshotId>   [NEW optional pinned id; re-read on resume]
```

Only `src/cli/like.rs` names the `toha::snapshot` reader; only `toha::snapshot`
names gitoxide. The generate apply runs the ordinary spine, including the
producer-owned snapshot **capture** — the only write is the producer's, of the
new application's own snapshot; the `--like` selection path itself writes nothing.

## Selection, precedence, collisions, and the error/results contract

### Total precedence, per question id

For each question the template defines and asks (after `when`/batching):

1. an **answer submitted by the route** (person typed, `--answers` value, agent
   `continue`) — always wins;
2. else the selected **snapshot's** value for that id, if it kind-matches — a
   `Snapshot` bank entry;
3. else the **configured** default (`template-defaults` / preset) for that id — a
   `Configured` bank entry, keyed on the formal name;
4. else the template's own `default` expression;
5. else no default (required → asked; optional → empty).

A snapshot value that does **not** kind-match is dropped (warned) and the id falls
to 3/4. This is total and deterministic on every route; snapshot-vs-configured
never collide per id because step 2 removes the id from step 3's set at the seam.

### Snapshot selection by route and snapshot count (matched on `source`)

| Matching snapshots | person, no `--like` | person, bare `--like` | `--like latest` (any route) | `--like <id/prefix>` (any route) | script/agent, no `--like` |
|---|---|---|---|---|---|
| 0 | plain apply, normal questions | picker offers nothing → proceed normal | required-absent → **exit 1** | unknown → **exit 1** | plain apply, normal questions |
| 1 | plain apply, normal questions | picker preselects it; seed or "none" | seed from it | seed if prefix matches & source ok | plain apply, normal questions |
| N (same source) | plain apply, normal questions | list N, preselect newest | seed from the **newest** (max ULID) | seed the unique match; multi-match prefix → ambiguous **exit 1** | plain apply, normal questions |

Exactly one snapshot ever seeds — never a union across snapshots, so cross-snapshot
precedence is "pick one deterministically" (newest of the source), not a merge.
A bare `--like` is a **person-only** convenience; on the script and agent routes it
is a usage error (exit 2), so selection is always non-interactive and deterministic
there. The person picker shows the source's applications in-flow (from the repo-wide
reader), so discovery needs no separate command.

### Failure cases → exit codes

| Condition | Route | Result document | Exit |
|---|---|---|---|
| `--like <id/prefix>` unknown, ambiguous, or an invalid snapshot | all | `error` `snapshot` naming the reference | 1 |
| `--like latest` and no snapshot of the template's source | all | `error` naming the source that has none | 1 |
| selected snapshot's `source` ≠ the template's source identity | all | `error` naming both sources | 1 |
| target not in git and `--like <selector>` is explicit | all | `error` (no project) | 1 |
| bare `--like` (no selector) | script / agent | `error` usage (name a selector or `latest`) | 2 |
| `--like` combined with `--from` or `--baseline` | all | `error` usage | 2 |
| a seeded default the new template rejects on a constraint, not overridden | script / agent | `questions` (the batch, the rejection) | 4 |
| a seeded default the new template rejects on a constraint, not overridden | person | re-prompted with the default shown | 0 / 4 |
| subpath already occupied, no `--force` | all | `error` conflicts (unchanged) | 1 |
| template has untrusted hooks | all | `planned`, `trusted: false` (unchanged) | 3 |
| template name ambiguous (short-name collision) | all | `ambiguous` (unchanged) | 5 |
| resume finds the pinned seed snapshot missing or invalid | agent | `error` `snapshot` | 1 |

Snapshot ambiguity is **exit 1**, not exit 5: exit 5 is reserved for template
short-name collisions in `registry.resolve`; `--like` never re-enters template
resolution. The `applied` and `questions` documents gain one optional member,
`seed: { "from": "<snapshot id>" }`, present only when `--like` seeded; it carries
no answer values.

## Behaviors to prove (falsifiable)

**Mandated fixture — same generator twice in one project, the second seeded from
the first.** `tests/fixtures/generator-twice/`: a `widget` template (`label`
text; `style` select; `with_tests` confirm) and a clean git repo. Apply into
`src/widgets/alpha` with `{label:"Alpha", style:"card", with_tests:true}` and
commit; the apply saves snapshot `SA`. Apply into `src/widgets/beta` with
`--like <SA full id or >=6-char prefix>` and `--answers {label:"Beta"}`. Assert:
`beta/` renders `label=Beta`, `style=card`, `with_tests=true` (the two unspecified
ids took the snapshot defaults); `alpha/` is unchanged; two snapshots exist with
equal `source` and `target ∈ {alpha, beta}`; person, scripted, and agent routes
produce identical `beta/` trees for the same inputs. *Fails if the second
application does not inherit `style`/`with_tests`, or if the routes diverge.*

1. **Seed-not-replay.** A seeded default is a default, not a submitted answer: the
   person can change it and `--answers` overrides it. *Sole-kill:* route `--like`
   through a replay-style "submit recorded answers" path → `beta` cannot override
   `label`; fails the fixture.
2. **Precedence snapshot > configured > template.** With a `template-defaults`
   entry for `style` and a snapshot value for `style`, the snapshot value wins;
   with only a configured value, it is used; with neither, the template default.
   *Sole-kill:* let the configured entry shadow the snapshot → `style` shows the
   configured value; fails.
3. **Answer beats seed.** `--answers {label:"Beta"}` yields `Beta` though the
   snapshot has `Alpha`. *Sole-kill:* apply the seed after route answers → `beta`
   gets `Alpha`; fails.
4. **Source identity, not formal name.** A snapshot of `gh:example/widget@v1`
   seeds an apply of `gh:example/widget@v2` (equal `source`); a snapshot of
   `gh:example/other` is refused (exit 1). *Sole-kill:* compare `template` (with
   `@reference`) instead of `source` → the same-source/different-ref seed is
   refused; fails.
5. **Generate axis, not update axis.** `--like` seeds from a snapshot whose
   `target` differs from the new subpath and writes a fresh application there, no
   merge. *Sole-kill:* require the snapshot `target` to match the new target (as
   `--from` does) → seeding `beta` from `alpha`'s snapshot is refused; fails —
   proving `--from` is not overloaded.
6. **Producer capture preserved.** The `--like` apply into a clean git subpath
   still saves its own snapshot, so a third application can seed from the second.
   *Sole-kill:* make the generate path skip or block capture ("reader-only" over
   the whole run) → only one snapshot exists after two applications and the
   third cannot seed; fails.
7. **Fresh instant.** The new application uses the normal `now` and its own
   `generated`; it does not inherit the seed snapshot's frozen instant.
   *Sole-kill:* carry `generated` from the seed snapshot → a date-rendering widget
   freezes `beta` to `alpha`'s instant; fails.
8. **Wrong-kind seed dropped, not fatal.** A snapshot recorded
   `with_tests="maybe"` (string) but the question is now a Confirm: the id is
   dropped with a warning and falls to configured/template; the apply proceeds.
   *Sole-kill:* hard-error on the wrong-kind seed → exit 1 instead of proceeding;
   fails.
9. **Constraint-invalid seed re-asks.** A kind-valid seed that fails a tightened
   pattern in the new version, not overridden, re-asks (person) or returns
   `questions` (exit 4); it is never written. *Sole-kill:* auto-accept the invalid
   seed → an invalid file is rendered; fails.
10. **No-`--like` path is byte-identical.** With `--like` absent, apply,
    configured-default provenance, and every route result are exactly today's.
    *Sole-kill:* always route configured defaults through the seed path → a
    rejected configured default's message/provenance changes and driver parity
    breaks; fails the template-defaults driver-parity behavior.
11. **Single-occupant bank + provenance.** For an id with both a snapshot and a
    configured default, the bank holds exactly one entry, a `Snapshot` whose prompt
    names the snapshot. *Sole-kill:* keep both occupants, or drop the provenance
    label → the id is prepared twice, or the prompt cannot name the source; fails.
12. **Extra seed ids ignored; unknown configured ids still warn.** A snapshot
    carrying an id the new template dropped is silently ignored; a configured
    mapping for an id the template lacks still warns. *Sole-kill:* error on the
    extra snapshot id → ordinary template evolution breaks.
13. **Looped values survive the fold.** A snapshot that recorded a `TextLoop`
    question (one JSON array under its id) seeds that whole array as the loop's
    default. *Sole-kill:* a fold that splits or drops the array corrupts the loop
    default; fails.
14. **Required-absent vs optional-absent.** `--like latest` with no matching
    snapshot → exit 1; bare `--like` on script/agent → exit 2; `--like` absent →
    plain apply. *Sole-kill:* silently proceed when an explicit selector is
    unsatisfiable → a required seed is dropped; fails.
15. **Dirty target still seeds.** With a dirty git target and a same-source
    snapshot, defaults are still seeded; only the new capture is skipped (`dirty`).
    *Sole-kill:* gate the snapshot *read* on cleanliness → no defaults on a dirty
    target; fails.
16. **≥6-char prefix, exact-one.** A 6-char prefix matching one snapshot seeds; a
    shorter prefix or one matching several fails (exit 1). *Sole-kill:* accept a
    <6-char or ambiguous prefix → the wrong or an arbitrary snapshot seeds; fails.
17. **Repository-wide, source-filtered selection.** With snapshots of the source
    at two sibling subpaths and a snapshot of a different source, `candidates`
    returns exactly the two same-source snapshots newest-first, regardless of the
    new target's own (empty) snapshot set. *Sole-kill:* scope selection to the new
    target's snapshots → zero candidates, no seed; fails.
18. **Plan stays single-target.** One `--like` apply writes one subpath; the
    `Plan`'s context target equals the subpath. *Sole-kill:* fan `--like` out to
    several targets in one invocation → `PlanError::ContextTarget`; fails.
19. **Resume re-seeds from the pinned id.** Agent `stage --like latest` pins the
    snapshot id; `continue`/`apply PATH` rebuild the identical seeded defaults; a
    newer same-source snapshot added before `continue` does not change the pin; a
    pinned snapshot deleted before `apply PATH` fails clearly (exit 1).
    *Sole-kill:* re-derive `latest` at resume instead of honouring the pin → the
    seed drifts to the newer snapshot; fails.
20. **Reader-only selection.** The `--like` selection path reads snapshot refs and
    writes none; the only ref written during the run is the producer's capture of
    the new application. *Sole-kill:* any write to `refs/toha/snapshots/*` from the
    selection path; fails.
21. **Engine purity / route parity.** For identical (template, context, snapshot,
    config, answers), the terminal, headless, staged, and crate routes accept the
    identical raw submissions and write the identical tree; the engine never
    receives a `Snapshot`, a `Project`, or a gitoxide type. *Sole-kill:* give the
    generate path its own interview walk, or leak a snapshot type past the seam →
    a route diverges; fails.

## Compatibility and canonical documents

Toha is before 1.0.0 with no dependent users. Plain `apply` and `stage` keep their
behaviour exactly; `--like` is additive and opt-in. The consumed producer contract
is project-updates **revision 3** (Bob-approved) with completeness correction
**1110**, integrated at the epic base; only its `toha::snapshot::{Project,
Snapshot}` reader is used. The paired implementation (task 1030) updates, in one
change:

- `docs/specifications/command-line-interface.yml` and `.spec.yml`: `--like
  [SELECTOR]` on `apply` and `stage`, its route rules, the selection/precedence
  table, its mutual exclusion with `--from`/`--baseline`, and the new exit rows
  (all within 0–5).
- `docs/specifications/interview-protocol.yml` and schema: the optional `seed`
  member on `applied` and `questions`, and the optional `seed` member on the
  staged record.
- `docs/technical-designs/architecture.yml`: the `src/cli/like.rs` reader boundary
  and the `Resolution::start_with_seed` seam with the additive `DefaultBankEntry`
  variant.
- The repository glossary (`AGENTS.md`): **Generator** (an apply pattern that
  writes one template repeatedly into project subpaths; not a `template.yml`
  object), **Seed selector** (`--like`, names a snapshot whose answers pre-fill a
  new application's defaults), **Snapshot-seeded default** (a default that enters
  the engine as a `Snapshot` bank entry from a snapshot's submissions).
- `docs/concepts/toha.yml`: the in-project generator usage pattern.
- A user-guide page on generating into subpaths, with a generic example, and the
  `tests/fixtures/generator-*` harness (`template/`, `answers-*.json`,
  `expected/`, `expect.yml`).

`docs/specifications/template-format.schema.yml` is **unchanged**: no `kind`, no
`generator` key, no second entrypoint. The closed key set and the "one source, one
interview" load assumption are preserved.

**Coordination with task 1029.** `--like` consumes the reader contract only and
adds `StagedRecord.seed`, orthogonal to 1029's `StagedRecord.base`; the flags are
mutually exclusive (exit 2 if combined), so no field or code path conflicts. One
open reader point is surfaced to 1029: the generator selection needs
`Project::snapshots()` to enumerate the **repository-wide** snapshot set (all
`refs/toha/snapshots/*`), which the consumer filters by `source`. If the producer
instead scopes that reader to the opened target, a repository-wide enumeration
must be added to the reader contract; it is a read-only addition and edits no
1029-owned write path.

## Out of scope

- Snapshot capture, `--from`/`--baseline`, the update replay/merge adapter, and
  the `snapshots`/`init` commands (task 1029); `--like` consumes the reader only.
- Any `template.yml` generator/`kind` concept, sub-templates, or multiple
  entrypoints. A template-declared generator was evaluated (candidate 3) and found
  to buy only authoring ergonomics and **no** snapshot leverage — the frozen
  snapshot records no generator name, so a generator's defaulting identity is
  `(source, target)` regardless of declaration. It is severable and deferred; it
  would become worthwhile only alongside a generator field on the snapshot, a
  1029-owned change.
- A second persisted-answers lifecycle; `--like` persists only a snapshot **id**
  in the staged record and re-reads answers from git.
- Unioning answers across several snapshots; inferring answer identity by matching
  question ids across templates or applications.
- Multi-target fan-out in one invocation; a `Plan` is single-target.
- Combining `--like` with `--from`/`--baseline`; a new reserved context name for
  the subpath beyond `toha_target_name`; a `--reanswer`-style replay (a generator
  never replays).
- New permissions, timeouts, pinned-version checks, or application subprocesses —
  this design introduces none, and removes no supported capability.

## Decisions

The product decisions and the decisions still needed are in `design.yml`.
