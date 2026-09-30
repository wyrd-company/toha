# In-project generators — design (candidate: first-class `toha generate` verb)

A generator is the ordinary apply spine driven **again into a subpath** of an
existing project, with the prior application's recorded answers offered as
defaults. This candidate makes that second axis a first-class verb,
`toha generate TEMPLATE PATH`, distinct from the update axis (`apply --from`,
same target). The verb's whole purpose is repeated in-project application, so it
treats *available snapshots of the same template source as its default seed*.

Nothing new is persisted into the project beyond the snapshot that the shared
apply spine already captures. No engine fork, no `template.yml` change, no new
answers lifecycle.

---

## 1. Caller usage (the spec)

### 1.1 Person route — first application into a subpath

```console
$ toha generate forge:catalog/widget ./src/widgets/button
? Component label: Button
? Emit stories? (y/N) y
src/widgets/button/button.txt
src/widgets/button/button.stories.txt
snapshot 01J9Z4K7QX (widget 3f9a1c2)
share it: git push origin refs/toha/snapshots/01J9Z4K7QX6M2V8R0T5B3N1P9D
```

`PATH` is the subpath. `src/widgets` already exists; `button` does not — the
same target rule the plain apply uses (`canonical_target` walks to the nearest
existing ancestor and re-appends the new suffix). The apply captures a snapshot
because the target is a clean git target — the capture rule and output line are
the shared apply spine's, unchanged.

There are **no** snapshots of `forge:catalog/widget` in this project yet, so the
interview runs with the template's own defaults (plus any configured
`template-defaults`), exactly like `apply`.

### 1.2 Person route — second application, defaulting from the first

```console
$ toha generate forge:catalog/widget ./src/widgets/card
using snapshot 01J9Z4K7QX (widget, ./src/widgets/button) as answer defaults
? Component label: (Button) Card
? Emit stories? (Y/n) y
src/widgets/card/card.txt
src/widgets/card/card.stories.txt
snapshot 01JA2B8M4R (widget 3f9a1c2)
```

Same `source` (`forge:catalog/widget`), a **different** target subpath. The verb
finds the prior application's snapshot by source — never by target — and seeds
every recorded answer as the default. The person accepts or edits each one;
`Component label` is pre-filled `Button` and edited to `Card`, `Emit stories?`
defaults to the recorded `yes`. The subpath's `toha_target_name` (`card`) is the
only built-in per-application distinction and needs no answer.

When several snapshots of this source exist the person is shown the list and
picks one (the likely base preselected) — the same prompt shape the update axis
uses when `--from` is absent.

### 1.3 Scripted route — `--answers`, snapshot fills the gaps

```console
$ toha generate forge:catalog/widget ./src/widgets/badge --answers answers.json
```

`answers.json` (identity-bearing, as every scripted document):

```json
{ "template": "forge:catalog/widget", "answers": { "label": "Badge" } }
```

Result document (one shot, to stdout):

```json
{
  "protocol": 1,
  "status": "applied",
  "seeded_from": { "snapshot": "01J9Z4K7QX6M2V8R0T5B3N1P9D" },
  "files": ["src/widgets/badge/badge.txt", "src/widgets/badge/badge.stories.txt"],
  "snapshot": { "id": "01JB3C1D2E5G7H9K1M3P5R7T9W" }
}
```

The document answers `label`; every other question the snapshot recorded
(`emit_stories: true`) is taken as its default; nothing is prompted. Selection
is deterministic without interaction — the likely base of the source — so the
script is reproducible. `seeded_from` names the snapshot that supplied the
defaults, or is `null` when none did. `snapshot` is the apply spine's own
capture outcome (from the update-projects contract), unchanged.

A required question the snapshot did not record and the document does not answer
returns `questions` with the batch, exit 4 — the ordinary scripted contract.

### 1.4 Scripted route — snapshot absent, still applies

```console
$ toha generate forge:catalog/widget /tmp/scratch/widgets/box --answers answers.json
{ "protocol": 1, "status": "applied", "seeded_from": null,
  "files": ["..."], "snapshot": { "skipped": "not_git" } }
```

`/tmp/scratch` is not a git repository, so `Project::open` returns `None`: no
snapshot layer. The document's answers apply; unanswered questions fall to
configured `template-defaults` then to the template's own defaults then, if
required and still empty, to `questions`/exit 4. `seeded_from` is `null`. The
same fallback holds for a dirty target (snapshots are still *read* for defaults;
only the new snapshot *capture* is skipped) and for a fresh clone before
`toha init` + fetch (no refs → no same-source snapshot → `null`).

### 1.5 Agent route — staged, pinned seed

```console
$ toha generate forge:catalog/widget ./src/widgets/chip --async
{ "protocol": 1, "status": "questions", "seeded_from": { "snapshot": "01J9Z4K7QX..." },
  "questions": [ { "id": "label", "default": "Button" }, { "id": "emit_stories", "default": true } ] }
$ toha continue ./src/widgets/chip answers.json
$ toha apply ./src/widgets/chip
```

`--async` stages the interview and pins the selected seed snapshot id into the
staged record (a pointer, not a copy of answers). `continue` and `apply PATH`
re-open the project, re-`find` that exact snapshot, and rebuild the same seed
from live config, so the async batches resolve deterministically and identically
to a one-shot run. A person driving `continue` without a file is prompted with
the same defaults.

### 1.6 Crate route

```rust
let project = toha::snapshot::Project::open(&target)?;          // None when not in git
let seed = project.as_ref()
    .map(|p| toha::generate::select_seed(p, resolved.source(), Selector::Auto))
    .transpose()?                                                // Option<SeedChoice>
    .flatten();
let Resolution { .. } = resolution;                              // configured defaults, as today
let (interview, dropped) = resolution.start_generated(
    &template, now, invocation,
    seed.map(|s| s.into_snapshot_defaults()),                    // Option<SnapshotDefaults>
)?;
dropped.iter().for_each(|w| eprintln!("warning: {w}"));
// drive `interview` exactly as apply does → Plan::build → apply
```

A crate caller gets the identical seam: it never touches gitoxide, a snapshot
tree, or the default bank; it hands the engine one optional `SnapshotDefaults`
and receives an `Interview`.

---

## 2. Data structures expected

```rust
// src/generate.rs — NEW crate module. The generate axis lives here; it owns
// selection and the boundary that turns a snapshot into seed defaults. No
// gitoxide, wire, or engine-internal type crosses its public surface.

/// The identity a generator application is keyed by: the template *source*
/// (formal name without @reference) plus the canonical target subpath. Derived,
/// never declared; sufficient to distinguish repeated applications and to key
/// snapshot lookup. It persists nothing itself — the apply spine's snapshot
/// already records (source, target).
pub struct GeneratorApplication<'a> {
    source: &'a str,                 // Snapshot::source() identity
    target: &'a CanonicalTarget,     // where this application writes
}
impl<'a> GeneratorApplication<'a> {
    pub fn source(&self) -> &str { unimplemented!() }
    pub fn target(&self) -> &CanonicalTarget { unimplemented!() }
}

/// How the caller chose the seed snapshot. Parsed once from the CLI at the
/// boundary; the reference/keyword split is a type, not a re-parsed string.
pub enum Selector {
    /// No flag: seed from the likely base of the source, prompt on the person
    /// route when several exist, take the likely base without interaction on the
    /// script and agent routes. A source with no snapshot yields no seed.
    Auto,
    /// `--defaults-from <ID>`: a specific snapshot, required-satisfy. Unknown,
    /// ambiguous, absent, or wrong-source fails clearly (exit 1).
    Required(String),
    /// `--no-defaults`: never read snapshots; configured + template defaults only.
    None,
}

/// The snapshot chosen to seed defaults, with the label shown to callers.
/// Private fields — a snapshot type never leaves this module.
pub struct SeedChoice { /* snapshot id, short label, flattened submissions */ }
impl SeedChoice {
    pub fn label(&self) -> &str { unimplemented!() }            // "01J9Z4K7QX (widget, ./src/widgets/button)"
    pub fn id(&self) -> &toha::snapshot::SnapshotId { unimplemented!() }
    /// Flatten `Snapshot::submissions()` (Vec<IndexMap<Id, Value>>) into the flat
    /// per-id shape the engine seeds from: fold batches in order, a later batch
    /// overrides an earlier id; a looped question's one JSON array under its id
    /// carries through whole.
    pub fn into_snapshot_defaults(self) -> toha::interview::SnapshotDefaults { unimplemented!() }
}
```

```rust
// src/interview.rs — additive. Snapshot defaults enter ONLY through the engine's
// existing seed/default-bank boundary.

/// Raw answers recovered from a prior application, tagged with a display-only
/// label (the short snapshot id + its subpath). The engine treats `from` as an
/// opaque provenance string and never learns what a snapshot is — purity holds.
pub struct SnapshotDefaults {
    pub from: String,                        // e.g. "01J9Z4K7QX (./src/widgets/button)"
    pub answers: IndexMap<Id, RawAnswer>,    // flattened, pre-format, pre-validate
}

/// One new bank occupant, alongside Seed and Configured. Carries only raw JSON
/// and a provenance label — no snapshot/gitoxide type. (Private, as today.)
enum DefaultBankEntry {
    Seed(RawAnswer),
    Configured(ResolvedDefault),
    Snapshot { raw: RawAnswer, from: String },   // NEW
}

/// One new prepared-default provenance, mirroring the bank variant, so a prompt
/// and a rejection can name the snapshot a default came from. (Private.)
enum PreparedDefaultSource {
    Template { expression: Option<String> },
    Configured(ConfiguredDefaultOrigin),
    Seed,
    Snapshot { from: String },                   // NEW
}
```

`StagedRecord` (`src/staging.rs`) gains one optional member, `seed_snapshot:
Option<SnapshotId>` — a **pointer** into the existing snapshot lifecycle that
pins which snapshot a staged generate seeded from, so `continue`/`apply PATH`
resolve the identical seed. It is `None` for a non-generate stage and for a
generate that selected no snapshot. It holds no answers, so it is not a second
persisted-answers lifecycle. It is distinct from the update axis's proposed
`base` member (`base` drives replay-in-place; `seed_snapshot` only seeds
defaults).

---

## 3. Public interfaces expected

### 3.1 The selection surface (new, `toha::generate`)

```rust
/// Choose the seed snapshot for an application, honoring the selector and the
/// route's interaction budget. Reads the project's snapshots through the
/// approved reader contract; matches on `source` equality ONLY (never target).
///
/// - Auto + no same-source snapshot            -> Ok(None)
/// - Auto + exactly one                        -> Ok(Some(that))
/// - Auto + several, person route              -> prompt (likely base preselected)
/// - Auto + several, script/agent route        -> Ok(Some(likely base)); deterministic
/// - Required(id): find + require source match -> Ok(Some) or Err(unsatisfiable)
/// - None                                       -> Ok(None)
pub fn select_seed(
    project: &toha::snapshot::Project,
    source: &str,
    selector: Selector,
    route: Route,           // Person | Script | Agent — sets whether a prompt is allowed
) -> Result<Option<SeedChoice>, SelectError>;

/// Named the same way the update axis names its snapshot failures: every message
/// points at `toha snapshots list PATH`. Maps to exit 1.
pub enum SelectError {
    Unknown(String),                 // no snapshot matches the prefix
    Ambiguous { prefix: String, matches: Vec<SnapshotId> },
    WrongSource { id: SnapshotId, wanted: String, found: String },
    Read(toha::snapshot::ProjectError),
}
```

`likely base` is **not reinvented**: `select_seed` calls
`Project::likely_bases(&same_source_snapshots)` and takes the source's likely
base. Selection never unions answers across snapshots — exactly one snapshot
seeds, so cross-snapshot precedence is "pick one, deterministically," not a
merge.

### 3.2 The seam where submissions become defaults (the load-bearing edit)

```rust
// src/interview.rs — sibling of Resolution::start_with_context. The ONE place
// snapshot answers and configured answers are layered into the single-occupant
// bank. Precedence is resolved HERE, so the bank still holds one entry per id.
impl Resolution {
    /// Start a generator interview. For each question id:
    ///   1. a snapshot answer that parses to the question's kind  -> Snapshot entry
    ///   2. else a configured default (this Resolution)           -> Configured entry
    ///   3. else the template's own `default` expression          -> no bank entry
    ///   4. else the question is asked
    /// A snapshot answer for an id the template no longer defines, or whose kind
    /// no longer matches, is DROPPED (returned as a warning), never a hard error:
    /// snapshots are optional defaults, so a stale one falls through to (2)/(3).
    /// The single-occupant `DefaultBankEntry` is preserved: snapshot vs configured
    /// never coexist for one id; the winner is chosen here.
    pub fn start_generated<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
        snapshot: Option<SnapshotDefaults>,
    ) -> Result<(Interview<'a>, Vec<String>), EvalError> { unimplemented!() }
}
```

Why kind-check (not full-constraint) at the seam: the engine already validates a
bank default's kind in `render_default` and its constraints in `Pending::answer`
batch-fill. The seam drops only what the engine would hard-fault on for a seed
(a wrong-kind value), so the "optional" contract holds; a value that passes kind
but fails a constraint behaves exactly as a configured default does today (the
person edits it; the script route reports it rejected, exit 4). Snapshot answers
therefore never make the engine behave differently from a hand-written seed.

`Plan::build`, `Plan::apply`, `Plan::mutations`, `Pending::answer`, `check`,
`batch`, the staging replay walk, `configured_defaults`, and every route adapter
keep their signatures. The only engine change is the additive `DefaultBankEntry`
/ `PreparedDefaultSource` variant and the `start_generated` entry; the
`render_default` and `Pending::answer` match arms gain a `Snapshot { from }` arm
that behaves like the `Seed` arm but names the snapshot in its message.

---

## 4. Module and seam diagram

```text
CLI  generate TEMPLATE PATH [--defaults-from ID | --no-defaults]
     [--answers FILE] [--async FILE] [--force] [--trust] [--dry-run]
  │  reuse: setup · canonical_target · resolve_template · load_template ·
  │         build_context · report_config_warnings · Plan::build · apply_reporting
  ▼
src/generate.rs                                   NEW (generate axis)
  Selector · Route · select_seed · SeedChoice · GeneratorApplication
  │ matches snapshots on SOURCE, takes Project::likely_bases for "auto"
  │ flattens Snapshot::submissions() -> SnapshotDefaults
  ▼                                        ▲ consumes (read-only)
src/interview.rs                           │  toha::snapshot  (owned by 1029)
  Resolution::start_generated  <-- SEAM    │    Project::open / snapshots /
  DefaultBankEntry::Snapshot (additive)    │    find / likely_bases
  PreparedDefaultSource::Snapshot          │    Snapshot::source/target/submissions
  │ single-occupant bank, precedence resolved at the seam
  ▼
PURE ENGINE (Asking/Complete/Ended)   unchanged below the bank
  │
  ▼
src/staging.rs   StagedRecord + seed_snapshot: Option<SnapshotId>  (pointer)
```

New: `src/generate.rs`, the `generate` CLI arm, the `start_generated` seam, the
two additive engine variants, the staged-record pointer. Reused whole: the
five-step apply spine, the interview engine, `configured_defaults`, the conflict
/ trust / plan / apply machinery, `canonical_target`, `toha_target_name`.
Consumed read-only: the `toha::snapshot` reader contract. Not touched: snapshot
capture, the `--from` merge, the replay adapter, the `snapshots` commands.

---

## 5. Selection, precedence, collision, and the result/exit contract

### 5.1 Snapshot selection — counts × routes

| Same-source snapshots | `Selector` | Person | Script | Agent (`--async`) |
|---|---|---|---|---|
| 0 | Auto | no seed; run normally | no seed; `seeded_from: null` | no seed |
| 1 | Auto | seed from it | seed from it | seed + pin its id |
| ≥2 | Auto | list + prompt (likely base preselected) | seed from **likely base**, no prompt | seed from likely base + pin |
| any | `--defaults-from ID` | seed from ID (required) | seed from ID (required) | seed from ID + pin |
| any | `--no-defaults` | no seed | no seed | no seed |
| target not in git / fresh clone | any non-Required | no seed; apply proceeds | no seed; apply proceeds | no seed |
| target not in git | `--defaults-from ID` | error (no project to read) | error | error |

Snapshots are matched on `Snapshot::source()` equality. The selected snapshot's
`target` is irrelevant and normally differs (it is the prior subpath). Dirty
target: snapshots are still read for defaults; only the new capture is skipped.

### 5.2 Per-id precedence into the single-occupant bank

For each question id, exactly one wins (resolved in `start_generated`):

1. the selected snapshot's answer, **if** it parses to the question's kind;
2. else the configured `template-defaults` value for the formal name;
3. else the template's own `default` expression;
4. else the question is asked.

A `--answers` document value or a person's typed value overrides whatever the
bank offered, per the normal interview. A snapshot answer for an unknown or
kind-changed id is dropped with a warning and falls to (2)/(3). Answers are never
unioned across snapshots, and configured defaults are keyed on the formal name so
every subpath of one generator gets the same configured defaults — per-subpath
variation comes from answers and `toha_target_name`, never from config.

### 5.3 Failures and results

| Condition | Route result | Exit |
|---|---|---|
| Applied cleanly | `applied` (+ `seeded_from`, + spine's `snapshot`) | 0 |
| Dry-run plan | `planned` (+ `seeded_from`) | 0 |
| Flow stop/abort | `ended` | 0 |
| Template load / render / hook failure | `error` of that kind | 1 |
| File conflict at the subpath, no `--force` | `error` conflict, nothing written | 1 |
| `--defaults-from` unknown / ambiguous / absent | `error` naming `snapshots list` | 1 |
| `--defaults-from` snapshot is another source | `error` naming both sources | 1 |
| Project read error (corrupt repo) | `error` | 1 |
| Staged-record mismatch (`continue`/`apply PATH`) | `error` | 1 |
| Invalid CLI (`--defaults-from` with `--no-defaults`; `--answers` with `--async`) | usage error | 2 |
| Untrusted hooks | `planned`, `trusted: false` | 3 |
| Questions remain / answer rejected | `questions` (batch returned) | 4 |
| Ambiguous **template name** (short-name collision) | `error` | 5 |

Two ambiguities, two exits: an ambiguous **template name** is exit 5 (unchanged,
from `resolve_template`); an ambiguous **snapshot selector** is exit 1 (a
snapshot error, matching the update axis). They never collapse.

---

## 6. Behaviors to prove (falsifiable)

Fixtures live under `tests/fixtures/generate-*`: a `widget/` template (one
`source/`, an interview with `label` (text, required) and `emit_stories`
(confirm, default false)), pre-built project fixtures, one or more snapshot
fixtures supplied through the reader contract, `answers*.json`, and `expect.yml`
with the worktree tree and the result document.

1. **Twice in one project (mandated).** In a clean git project, `generate widget
   ./a` with `label=Alpha, emit_stories=true`; then `generate widget ./b`. The
   second seeds from the first: its `label` default is `Alpha` and its
   `emit_stories` default is `true`; answering `label=Beta` yields `./b` files
   for Beta with stories, and the second `seeded_from` names the first's
   snapshot. *Sole-kill:* select by **target** instead of source — `./b` finds no
   snapshot (targets differ), no defaults, `label` has no default → exit 4; fails.
2. **Source, not formal name.** A snapshot of `widget@1.0` seeds a `generate
   widget@2.0` (equal source). *Sole-kill:* compare the formal name (with
   `@reference`) instead of `source` — the snapshot is skipped; fails.
3. **Snapshot beats configured on overlap.** A `template-defaults` entry sets
   `label=Configured`; a snapshot recorded `label=Snap`. The offered default is
   `Snap`. *Sole-kill:* let configured win — default is `Configured`; fails.
4. **Configured fills a snapshot gap.** The snapshot recorded no `emit_stories`
   (an older version lacked it); a `template-defaults` sets it true. The offered
   default is `true`. *Sole-kill:* have the snapshot layer clear configured ids —
   default falls to the template's `false`; fails.
5. **Stale snapshot answer is dropped, not fatal.** A snapshot recorded
   `emit_stories="maybe"` (string) but the question is a Confirm now. The apply
   proceeds with `emit_stories` falling to configured/template; a warning names
   the dropped id. *Sole-kill:* hard-error like a configured default — exit 1
   instead of proceeding; fails.
6. **Snapshot-absent fallback.** `generate widget /tmp/x` (non-git) applies with
   template/configured defaults and `seeded_from: null`. *Sole-kill:* require a
   project/snapshot — refuses on a non-git target; fails.
7. **Dirty target still seeds.** With a dirty git target and a same-source
   snapshot, defaults are still seeded; the spine reports the new capture skipped
   `dirty`. *Sole-kill:* gate the snapshot *read* on cleanliness — no defaults;
   fails.
8. **Required selector satisfied and refused.** `--defaults-from 01J9` seeds that
   snapshot; `--defaults-from ZZZZ` (unknown) → exit 1 naming `snapshots list`;
   `--defaults-from 01` (matches two) → exit 1 ambiguous. *Sole-kill:* fall back
   to Auto when a Required selector is unsatisfiable — exit 0; fails.
9. **Required selector wrong source refused.** `--defaults-from ID` where ID is a
   `gadget` snapshot and TEMPLATE is `widget` → exit 1 naming both sources.
   *Sole-kill:* seed it anyway — wrong defaults apply; fails.
10. **Cross-snapshot single pick.** Three same-source snapshots; scripted Auto
    seeds exactly the likely base; an id only present in a non-selected snapshot
    is **not** offered. *Sole-kill:* union submissions across snapshots — the
    stray id gets a default; fails.
11. **Route interaction budget.** With ≥2 snapshots, the person route prompts for
    the pick; the script and agent routes never prompt and resolve to the likely
    base identically. *Sole-kill:* let the script route prompt/block — it hangs or
    diverges; fails.
12. **`--no-defaults`.** With a same-source snapshot present, `--no-defaults`
    offers only configured/template defaults; `seeded_from: null`. *Sole-kill:*
    still seed; fails.
13. **CLI usage guards.** `--defaults-from ID --no-defaults` → exit 2; `--answers
    FILE --async` → exit 2. *Sole-kill:* accept either combination; fails.
14. **Exit 5 stays template-name only.** An ambiguous short TEMPLATE name → exit
    5; an ambiguous `--defaults-from` prefix → exit 1. *Sole-kill:* route snapshot
    ambiguity to exit 5; fails.
15. **Engine parity / determinism.** For identical (template, context, snapshot,
    config, answers), the terminal, headless, staged, and crate routes accept the
    identical raw submissions and write the identical tree. *Sole-kill:* give
    generate its own interview walk — a route diverges; fails.
16. **Staged seed is pinned.** `generate widget ./c --async` pins the selected
    snapshot; deleting that snapshot before `continue` → exit 1 (unknown); adding
    a newer same-source snapshot before `continue` does **not** change the seed.
    *Sole-kill:* re-derive the likely base at resume instead of honoring the pin —
    the seed drifts to the newer snapshot; fails.
17. **Per-subpath identity.** `./src/widgets/button` and `./src/widgets/card` see
    `toha_target_name` `button` and `card`; two distinct snapshots result (same
    source, different target). *Sole-kill:* reuse the first target's context — the
    second renders `button` paths; fails.

---

## 7. Compatibility and canonical-document impact

Toha is before 1.0.0 with no dependent users. `apply`, `stage`, `continue`, and
their exit contract are unchanged; `generate` is additive. The paired
implementation edits, in one change:

- `docs/specifications/command-line-interface.yml` and `.spec.yml`: the
  `generate` command, its `--defaults-from`, `--no-defaults`, `--answers`,
  `--async`, `--force`, `--trust`, `--dry-run` options, and its reuse of exits
  0–5.
- `docs/specifications/interview-protocol.yml` and schema: the optional
  `seeded_from` member of `applied`/`planned`/`questions`, and the staged
  record's optional `seed_snapshot` pointer.
- `docs/concepts/toha.yml`: the in-project generator concept — the generate axis
  distinct from the update axis, snapshots-as-optional-defaults.
- `docs/technical-designs/architecture.yml`: the `generate` module and the
  `start_generated` seam.
- The repository glossary (`AGENTS.md`): **Generator** (a template applied
  repeatedly into project subpaths, each application defaulting its answers from a
  prior application's snapshot), **Seed snapshot** (the snapshot whose submissions
  seed a generator application's defaults).
- A user guide page on in-project generators, with a generic example, and the
  `tests/fixtures/generate-*` harness.

The `template-format` schema and the `template.yml` struct are **not** touched
(see §8). `Snapshot`/`Project` are consumed only.

---

## 8. Out of scope

- Any `template.yml` change — no `kind`, no `generator` key, no "repeatable"
  marker. The schema is a closed 11-key set with `additionalProperties: false`,
  and a template is already "one source + one interview"; subpaths already work
  as targets. A generator needs nothing declared. (Rationale in `rationale.md`.)
- Snapshot capture, the `--from` update merge, the replay adapter, `--baseline`,
  and the `snapshots`/`init` commands — owned by the update-projects task;
  consumed read-only here.
- A second persisted-answers lifecycle. Only a snapshot-id *pointer* is added to
  the staged record; answers stay in snapshots.
- Merging answers across multiple snapshots; inferring answer identity by shared
  question id across templates or applications.
- A new reserved context name for the subpath beyond `toha_target_name`.
- New permissions, timeouts, pinned-version checks, or application subprocesses.
  This design introduces none; the only process ever started is a trusted
  template's hooks, through the existing trust gate.
- `--reanswer`. A generator never auto-replays answers (unlike the update axis),
  so every question is answered, defaulted, or asked; there is no replay to
  suppress. (Rationale in `rationale.md`.)
