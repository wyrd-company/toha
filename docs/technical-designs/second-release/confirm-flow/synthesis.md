# Confirm-flow arena — synthesis

## Base

**C4 (confirm action as a node-level walk effect).** Final design in `design.md`.

## Dropouts and blindness

No dropouts: all four candidates produced both artifacts. All four ran in isolated
`/tmp/arena-confirm-flow/candidate-<n>/` capsules against a read-only `git archive`
export of `a62061f` that contains no confirm-flow package, so this arena's rubric,
cross-judge, scores, and other candidates' outputs were absent and unreachable
(evidence in `arena-setup.md`). Each candidate was seeded a distinct whole-shape
structural direction; all four are structurally distinct and viable, so the
"≥2 structurally distinct candidates" requirement is met with margin.

## Convergence signal

C1 and C4 independently converged on the same load-bearing shape:
**stop = a terminal `Interview` variant that cannot be planned; dry-run = a field on
`Completed`; skip = resolved inside the walk with the existing skip machinery.**
Two blind candidates reaching the same shape from different directions (a
state-machine-variant framing and a walk-effect framing) is strong evidence the
shape is right. C4 is the more rigorous expression of it, so it is the base; C1
corroborates and contributes wording.

## Reconciling the parent/judge disagreement

The parent scored **C4** first (30); the cross-judge scored **C2** first (30). Every
criterion is tied at 5 across all candidates except **C5 (interface depth)**, so the
entire disagreement is one axis, which the arena skill says to resolve by reading
both rationales rather than averaging.

- The judge favors C2 because its `realize()` wrapper hides all confirm-action
  policy behind one call. That is a real strength — but it is an **ergonomic
  convenience that is graftable onto any base**. Nothing about `realize()` requires
  C2's structural choice; it is a thin funnel over "build unless stopped; apply
  only when proceeding."
- The parent favors C4 for **non-graftable structural-correctness** properties:
  1. **Stop is unplannable by construction.** `Interview::Ended` carries no
     `Completed` and no hooks, so no code path can build a plan or run a hook from
     a stop. C2 makes stop a `directive` on a full `Completed`, so a driver that
     ignores the field can still apply — the misuse compiles (C2's own rationale
     admits this and relies on the `realize()` convention to prevent it).
  2. **Stop does no downstream work and cannot fault.** C2 models stop as "skip the
     whole remaining interview + set the directive", so a stop walks the remainder
     and renders each skipped question's default. Per the approved error-attribution
     policy a skipped question validates its template default against ready
     constraints and a failure is a template fault — so under C2 a user answering
     "no, stop" can fail with a template-default error from a question never
     reached and irrelevant to the stop. C4's terminal `Ended` never touches those
     defaults, so a stop is always a clean exit.
  3. **Most rigorous atomicity.** C4 extends the `stands` early-failure classifier
     to read the new outcomes via the `skipped` set, keeping 1072 policy 2 exact
     when a stop short-circuits before a probe's question.
- On the architect's own **encode-lessons-in-structure** priority, C4's
  compile-forced stop handling (a new variant every driver must match) is a
  *stronger* interface than C2's ignorable field, not a weaker one — the judge
  under-weighted the safety dimension of interface depth.

Resolution: take **C4 as the base** and **graft C2's `realize()` funnel**, which
captures the only axis the judge favored without adopting C2's stop-as-completion.
The disagreement therefore resolves to a synthesis that is best on every criterion.

## Grafts (folded into `design.md`)

- **From C2 — the crate/direct-caller effect gate.** Add a convenience funnel
  `realize(template, outcome, target, options, runner) -> Realized{Stopped, Previewed, Applied}`
  plus `Completed::disposition()` and a `writes_allowed()`/`plan_shown()` predicate
  pair, so crate and direct callers route every effect through one deep call
  instead of re-implementing "build unless stopped; apply only when proceeding".
  This is layered *over* C4's `Interview::{Ended, Complete{disposition}}` types; it
  does not reintroduce stop-as-completion.
- **From C1 — corroboration + load-rule wording.** Adopt C1's crisp load-rejection
  table (on-only-on-confirm, non-empty, closed set, skip-group-needs-a-group) and
  record the C1/C4 shape convergence as the consensus.
- **From C3 — naming and clarity.** Name the funnel helper `directive_of(question,
  &Answer) -> Directive` (pure, one place), and state explicitly that
  `skip: group` at interview top level equals `skip: rest` with no special case
  (top frame == whole interview). Adopt C3's emphasis that a dry-run reuses the
  same `Completed`/plan so the previewed plan is byte-equal to a normal completion,
  and its behavior pinning that a confirm answered by a configured default fires
  through the origin-bearing `Resolution` without provenance reconstruction.

## Rejections (with reason)

- **C2's stop-as-skip-to-complete** — rejected: turns a stop into a full completion
  that renders downstream defaults (can fault on an unreached question) and reports
  `status: complete` for a run that wrote nothing. C4's terminal `Ended` is safer
  and more honest.
- **C3's separate `DryRun` `Interview` arm** — rejected in favor of the leaner
  `Completed { disposition }` (C1/C4): dry-run is a completion that suppresses
  apply, not a distinct state; a field avoids a fourth arm and keeps the dry-run
  plan on the same `Completed` type.
- **Inferring actions from prompt text or a magic answer value** — rejected by the
  inherited constraint; actions are declared data.
- **A parallel skip/reachability walker** — rejected (grounding "Avoid"); skip
  reuses the existing `Advance::skip`/`default_ready`/`warn_if_held` machinery.

## Verification

See `verification.md`: caller usage traced against the sketch; every rubric
criterion and inherited constraint checked against the design; failure cases and
falsifiable scenarios enumerated; composition with the approved 1072 contract and
the origin-bearing `Resolution` confirmed; the design holds.

---

## Round 2 — `flow` node vs action-bearing confirm (Bob's 19:06 request)

Bob asked, at the Phase C checkpoint, for a structural alternative: *"instead of
adding an action to `confirm` types, perhaps a new `flow` block with a when
expression and action enum should be used instead."* This is a change request, not
approval. Per the architect workflow, human pushback on the shape is Phase A
evidence: `grounding-flow.md` re-grounds the flow-node semantics, and a fresh blind
arena (`runner-task-flow.md`, capsules `flow-1/2/3`, `own-score-flow.md`,
`cross-judge-flow.md`) developed the flow shape. Round-1 evidence (candidates 1–4,
its cross-judge, own-score, and the synthesized confirm-action design preserved as
`design-round1-confirm-action.md`) is retained intact.

### Base (round 2)

**flow-1 — the minimal standalone `flow` node.** Final design in `design.md`.
Stop is a terminal `Interview::Ended` variant that `Plan::build` cannot consume
(type-safe unplannability); dry-run is a `Disposition` on `Completed`; skip reuses
the existing skip machinery; `skip: group` skips the *rest of the current group*
from the node's position (`rest` skips the rest of the interview). The `confirm`
type is not modified at all — ordinary confirms stay pure boolean by construction.

### Reconciling the parent/judge disagreement

Both the parent and the cross-judge **recommend the flow node over the round‑1
action-bearing confirm**, and both **reject flow-3's stop-as-`Completed`-disposition**
in favour of a type-safe terminal stop variant. The only disagreement is the base:
the judge picked **flow-2** (28) for its group-attached guard; the parent scored
flow-1 and flow-3 tied first (29.5) and flow-2 lower (28.5).

The judge's case for flow-2 rests almost entirely on one claim: a group-attached
`{ skip: group }` makes group-skip *structural and move-safe*, addressing the
grounding Risk about scope silently changing output. Examining the semantics
dissolves that advantage:

- A `group:` node **already carries its own `when`**, which skips the entire group
  when false. "Skip this whole group on a condition" is therefore already
  expressible today with `group.when` — flow-2's group-attached `{ skip: group }`
  largely **duplicates** it (flow-2's own rationale flags the
  `group.flow {skip: group}` vs `group.when: false` overlap as an open question).
- The task's `skip: group` outcome is "skip the **rest of the current** group from
  here" — a mid-group early-out. That is *inherently positional* and is exactly
  flow-1's primitive; a positional node that skips the rest of its enclosing group
  is doing precisely what it says, not silently changing meaning.

So the judge's headline benefit is mostly redundant surface, and flow-1's
positional `skip: group` is the correct, task-specified primitive — not the
fragility the judge scored it as. The parent therefore keeps **flow-1 as the base**
and defuses the scope concern by defining `skip: group` precisely in the design
(and spelling whole-group conditional skip as `group.when`). Group-attachment is
surfaced as an **optional decision** (D-attach), not baked in.

### Grafts (folded into `design.md`)

- **From flow-3:** the `Completed::step() -> Step::{Plan{apply}, …}` routing seam so
  every driver routes proceed-vs-dry-run through one method (adapted: stop stays the
  `Ended` variant, handled before a `Completed` exists); the explicit same-batch
  **readiness** argument (a flow node whose `when` reads a still-pending answer
  blocks and fires only after it commits — no batch race); and the idempotent
  monotone dry-run raise for deterministic multi-flow-node replay.
- **From flow-2:** the group-attached guard, offered as an **optional** affordance
  (decision D-attach) with the explicit note that conditional whole-group skip is
  already `group.when`; and its scenario proving positional-vs-structural scope,
  reused to document the `skip: group` semantics unambiguously.
- **From flow-1 (base):** the minimal `flow: <action>` + sibling `when`/`label`
  spelling that mirrors `message: <text>`, and the type-safe `Ended` stop.

### Rejections (with reason)

- **flow-3's stop-as-`Completed`-disposition** — rejected (parent and judge agree):
  it makes "cannot be planned" a runtime discipline across ~6 call sites rather than
  a type fact. Keep the terminal `Ended` variant.
- **flow-2 as the base / baking in group-attachment** — rejected: the group-attached
  `{ skip: group }` largely duplicates the existing `group.when`; it adds surface and
  a real overlap for little unique value. Offered as an optional decision instead.
- **The round‑1 action-bearing-confirm shape as the mechanism** — set aside per
  Bob's "instead" and the merits below; preserved as `design-round1-confirm-action.md`
  and summarized in the design's Alternatives.

### Shape recommendation to Bob (headline decision)

Adopt the **`flow` node**. It is the cleaner engine fit (reuses `when`, the node
walk, `visited`, and the skip machinery verbatim; leaves `confirm` untouched so
ordinary confirms carry zero regression risk), strictly more expressive (the trigger
is any expression over prior answers/computed values, not one confirm's boolean),
and free of the same-batch race (readiness gating). Its one cost is a single
indirection for the simplest "no → stop" (the trigger reads one node away from the
confirm), which both reviewers judge worth it. The round‑1 confirm-action shape
remains the more *locally obvious* form for that simplest case only.

### Verification

See `verification.md` (rewritten for the flow-node design): caller usage traced
against the sketch, every criterion and inherited constraint checked, failure cases
enumerated, predecessor composition confirmed. The design holds.

---

## Approval closure (2026-09-28)

Bob approved the flow-node design and pre-approved the `abort` disposition either way
(verbatim recorded on the task, kept separate from interpretation). Closure actions:

- **Decisions resolved:** D-shape = the `flow` node; D-attach = standalone-only; D1 =
  stop/abort exit 0 + `ended` status; D2 = a `when` satisfied by a configured/preset/
  computed answer may fire; D3 = `{ skip: rest | group }`; D5 = `ended` carries
  `kind` + label + messages. Folded into `design.md`.
- **Abort: adopted** (pre-approved; not reported to Bob; not a blocker). A fourth
  opt-in action `flow: abort` = stop + remove the target's staged record via the
  existing `Store::remove(&CanonicalTarget)`. Rationale: "cancel and discard" is a
  genuine intent distinct from `stop` (resumable), it reuses an existing supported
  operation, adds no new capability, and keeps the engine pure (the driver performs
  the removal). Full routes/cleanup/error-semantics/proof-guards in `design.md` and
  `verification.md`. The alternative (implicit abort on every stop) was rejected so a
  template author cannot silently delete a user's staged state.
- **Base refresh:** rebased `dfe7ba0 → a62061f` (current epic head); merge-base ==
  current epic head proved before edits. The approved shape is unchanged; the closure
  diff adds abort + resolves decisions + refines titles.
- **Independent review** remains required and is **held** until the error-attribution
  producer (1056) publishes; the handoff records a clean exact basis and approved
  evidence. No merge/cleanup/DONE.
