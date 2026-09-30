# Synthesis — in-project generators

How the final design (`design.md`) was chosen and assembled from the three arena
candidates, the readonly cross-judge (`cross-judge.md`), and the parent's own
end-to-end reading. Base, grafts, rejections, dropouts, and verification are
recorded here.

## Runner record and blindness audit

- **Candidate runners:** three Claude `inherit-parent` (Opus) Agent subagents,
  synchronous, completing in the parent's turn. The arena's default runner slots
  are four `inherit-parent`; three structurally distinct seeded directions were
  used (matching the sibling `template-defaults` / `error-attribution` /
  `bundled-demo` packages). Each candidate received the common task, the
  grounding, and the two consumed contracts, plus a distinct seed direction, and
  was told it might deviate with justification.
- **Cross-judge runner:** Codex (GPT) — a different model family — via
  `async-codex-mcp`, giving genuine cross-family judgment. (Standing decision:
  GPT models are reached through the codex tool, never Agent `ocx-gpt-*`; and the
  harness cannot reach `ocx-self`. Recorded in memory.)
- **Blindness:** candidate output paths were isolated under
  `/tmp/arena-generators/candidate-<n>/`, outside product git. Candidates ran as
  Agent subagents with filesystem read access to the worktree, where `rubric.md`,
  this file, and the cross-judge existed only after the candidates finished.
  Blindness therefore rests on **input-withholding and runner discipline** (each
  candidate was instructed not to read `rubric.md`, `cross-judge*`, `synthesis*`,
  or a sibling candidate directory), **not** on enforced isolation. No candidate's
  returned summary showed rubric-derived language; none is recorded as
  contaminated. No dropouts: all three produced whole `design.md` + `rationale.md`
  (candidate 2's `design.md` carried a stray trailing tool wrapper, cosmetic, not
  grafted).

## Own scores (parent, end-to-end) and reconciliation with the cross-judge

| Candidate | C1 | C2 | C3 | C4 | C5 | C6 | Parent total | Judge total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 — `apply --like` | 4 | 4 | 4 | 4 | 5 | 3* | **24** | 22 |
| 2 — `generate` verb | 5 | 4 | 3 | 5 | 3 | 3 | **23** | 21 |
| 3 — declared generators | 3 | 2 | 3 | 3 | 2 | 2 | **15** | 13 |

\* C6 rises to 4 after the corrections below are applied.

Parent and cross-judge **agree on the base (Candidate 1)** and on the ordering
(1 ≳ 2 ≫ 3). The only material divergence: the parent rates C1's interface depth
(C5) a 5 where the judge gives 4 — the judge notes C1 kept snapshot selection
CLI-private, unavailable to crate callers. That is a real point and is resolved in
the final design by exposing the selection as a small pure library surface the
crate path can call (the graft below). The judge rates C1's C6 a 2 for three
contract-accuracy defects; the parent agrees these are real and fixes them, which
is why C1 is the base **with revisions**, not as-written.

## Base

**Candidate 1 — the generate axis is a flag on `apply` (`--like`), not a new
concept.** Chosen because:

- **Consistency with the approved sibling design.** Project-updates (1066)
  deliberately made the *update* axis a flag on `apply` (`--from`) and explicitly
  rejected a separate `update` verb ("no-update-command"). The generate axis is
  even more clearly "apply with a modifier": `toha apply TEMPLATE SUBPATH` already
  creates an application into a subpath (grounding: subpaths already work as
  targets), so a generator needs only the *seed-defaults-from-a-snapshot* delta.
  A flag adds that delta; a verb would duplicate the whole apply spine and
  introduce a second identity mental model.
- **Interface depth.** The feature collapses to one CLI boundary that turns a
  selected `Snapshot` into `IndexMap<Id, RawAnswer>` and one `Resolution`
  constructor that layers those over configured defaults. The pure engine — shared
  by every route and crate caller — sees nothing new but one additive private
  default-bank variant. Complexity in, a flag out.
- **The cross-judge independently reached the same base**, over a structurally
  cleaner-scoring verb candidate, on interface depth and least-new-machinery.

## Grafts (folded in by hand, kept coherent under one model)

- **G1 — snapshot-default provenance (from Candidate 2, D3).** Add a private,
  additive `DefaultBankEntry::Snapshot { raw, from }` and a matching
  `PreparedDefaultSource::Snapshot { from }`, so a prompt and a rejection can name
  the snapshot a default came from ("default from snapshot 01J9Z4K7QX6M2V…,
  src/widgets/alpha"). This *replaces* Candidate 1's provenance-less reuse of the
  `Seed` bank entry for snapshot defaults. It keeps one occupant per id
  (precedence still resolved at the boundary) and never appears on the no-`--like`
  path, so template-defaults' driver-parity/provenance (its B12) is untouched.
- **G2 — three distinct seed dispositions (from Candidate 2, D4; cross-judge
  required).** At the seam, kind-check each snapshot value against the live
  question: a **wrong-kind** value (the template evolved) is **dropped with a
  warning** and falls through to configured/template — snapshots are *optional*
  defaults, a cross-version drift must not brick a generate; a **kind-valid**
  value becomes the default and, if it later fails a constraint/`when`, is
  **re-asked** (person) or **returned** (`questions`, exit 4) exactly as any
  default; a **missing** value simply falls through. Wrong-kind ≠ rejected ≠
  missing, stated separately.
- **G3 — pinned seed pointer with a resume guard (from Candidate 2 D5 /
  Candidate 3 #4).** The staged record pins the *resolved* snapshot **id** (a
  pointer, never answers), so `continue`/`apply PATH` reproduce the identical
  defaults and a newer snapshot added mid-interview cannot change the pinned
  choice; a pinned snapshot that vanished fails clearly (exit 1).
- **G4 — pure selection surface for crate callers (from the cross-judge; fixes
  Candidate 2's leakage red flag).** Snapshot selection is a small set of pure,
  reader-only functions (`candidates`, `latest`, `find_required`) that a crate
  caller can drive; the person picker lives in the CLI adapter and calls them, so
  no prompting or `Project`/`SnapshotId` type sits on a selection signature except
  the consumed reader contract.
- **G5 — correct dirty-target reading (from Candidate 2; cross-judge flag 6).**
  A dirty git target still *reads* existing snapshots for seeding; only the new
  application's *capture* is skipped. Snapshot absence means non-git, unfetched,
  or genuinely none of this source — not dirty.
- **G6 — explicit loop-array preservation (from Candidate 3, B10).** The
  submissions→defaults fold keeps a looped question's recorded JSON array as one
  array value under its id (a valid loop default), with a dedicated guard.
- **G7 — the decisive rejection evidence (from Candidate 3's own finding).**
  Candidate 3 proved that a template-declared generator concept buys **zero**
  snapshot-keying leverage (the frozen snapshot records no generator name) and is
  not load-bearing for any of the five task demands. This is the recorded reason
  the declared concept is out of scope for 0.2.0.

## Corrections applied to the base (from the cross-judge)

- **Producer capture is preserved.** "Reader-only" is scoped to *seed resolution*:
  `--like` selection reads `refs/toha/snapshots/*` and never writes. The generate
  apply itself still saves its own snapshot through producer-owned capture (1029),
  which is exactly what lets the *next* application seed from it — and what the
  mandated twice-in-one-project fixture needs. Candidate 1's whole-run
  "never writes a ref" guard is corrected to a seed-resolution-scoped guard.
- **The update replay adapter is described correctly.** The reason a generator
  does not reuse it is ownership (it is crate-private to 1029) and semantics (it
  replays recorded answers to *update* an application in place); it is not that it
  is "fatal on rejects" (that is the older staged replay, which the update adapter
  fixes). Seed-as-default remains the right generate mechanism.
- **Discovery works across subpaths.** Selection consumes the repository-wide
  snapshot reader (`Project::snapshots()` over all `refs/toha/snapshots/*`),
  filtered by `source` — not the target-scoped CLI `snapshots list PATH`. The
  person picker shows the source's applications in-flow; a required-but-absent
  selector fails naming the source that has no snapshot, not a possibly-unhelpful
  target-scoped list command. (Coordination note recorded: if the producer scopes
  `Project::snapshots()` to the opened target rather than the repository, the
  generator selection needs a repository-wide enumeration on the reader contract —
  surfaced to 1029.)
- **Example snapshot prefixes respect the ≥6-character minimum** (full 26-char
  ULIDs or ≥6-char prefixes) throughout the design and fixtures.
- **The exact consumed revisions are named:** project-updates design 1066
  revision 3 (Bob-approved) with completeness correction 1110, integrated at the
  epic base; producer `design.md` / `design.yml` at the SHAs recorded on the task.

## Rejections (with reasons)

- **A `generate` verb (Candidate 2/3 base).** Rejected as the base for the
  consistency and interface-depth reasons above; *retained as the primary Phase C
  decision for Bob* (flag vs verb is a real, UX-visible choice with a
  discoverability upside for the verb).
- **Template-declared `generators:` (Candidate 3 base).** Rejected for 0.2.0 on
  Candidate 3's own finding (G7): no snapshot-keying leverage, not load-bearing,
  severable later. Recorded as a future direction (revisit only if generators and
  snapshots are designed together, which would need a generator field on the
  snapshot — a 1029-owned change).
- **A two-field `DefaultBankEntry` struct (Candidate 3, E) / an ordered-fallback
  `Vec` (grounding float).** Rejected in favour of G1's additive variant:
  precedence is total per id (the higher-priority present value wins), so two
  occupants never need to coexist; the additive variant is less invasive and keeps
  the single-occupant invariant every other caller relies on.
- **Flattening snapshot *and* configured defaults into plain `Seed` entries
  (Candidate 3, F).** Rejected: it discards configured-default attribution for
  surviving configured ids. The chosen design keeps configured ids as
  `Configured` entries (attribution intact) and only snapshot ids as `Snapshot`
  entries.
- **Overloading `--from` for generation (all three).** Rejected: `--from` is
  update-in-place; `--like` is create-and-seed; the two are mutually exclusive.
- **Reusing the update replay adapter to drive the interview (Candidate 1's
  correct instinct, corrected description).** Rejected: seed-as-default gives
  override and re-ask for free without coupling to a 1029-owned adapter.
- **Unioning answers across several same-source snapshots (Candidate 2 D2 /
  Candidate 3 G).** Rejected: it invents answer identity by shared id across
  applications (an Avoid constraint). Exactly one snapshot seeds; `latest` = the
  newest snapshot of the source by ULID.
- **Auto-seeding on plain `apply` (Candidate 1's rejection).** Rejected under the
  flag model: seeding is opt-in via `--like`, so plain `apply` is never hijacked.
  (Auto-seed becomes defensible only under a `generate` verb — noted with the
  Phase C decision.)
- **`--reanswer` for generate (Candidate 2 D7).** Rejected: a generator never
  replays; every question is answered, defaulted, or asked. `--no-like` intent is
  simply omitting the flag.
- **Answer-interpolated target patterns (Candidate 3 H).** Rejected: the spine
  fixes the target before the interview and `Plan` is single-target; the subpath
  is an explicit argument.

## Verification

Recorded in `verification.md`: caller usage reconciled against the sketch;
every task decision and constraint checked against the design; the failure cases
and the mandated twice-in-one-project fixture traced; compatibility confirmed
against the named consumed producer revisions; and the cross-judge's seven
contract flags each confirmed resolved or rejected with reason.
