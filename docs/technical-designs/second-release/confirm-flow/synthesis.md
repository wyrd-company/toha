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
