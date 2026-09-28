# Rationale — confirm action as a node-level walk effect

## Problem

Toha's engine has no surface today where an answer's *value* alters execution: a
confirm yields `Answer::Bool` that only feeds later `when`/template expressions
(`interview.rs:511`, `template.rs:114`). The feature must let a template map a
specific confirm answer to **stop**, **dry-run**, or **skip**, carry that
decision through one engine and five drivers, and stay replay-deterministic from
stored submissions only. The shape is non-obvious because the decision point is
buried: an active confirm's boolean is recorded in two different places (the
`Pending::answer` batch loop and the walk's held-accept branch), a confirm can
share a batch with the questions after it (`interview.rs:176`), and the outcome
must compose with an approved-but-unmerged predecessor (1072) whose atomic
answer-transaction and target-identity **contract** — not its runtime — we
depend on. The wire documents are closed (`additionalProperties: false`,
`status ∈ {questions, complete}`), so a new terminal outcome has no
representation today. Constraints honored: preserve ordinary confirm behavior
exactly; never infer actions from prompt text; reuse the existing skip/default
and "answer not used" semantics; add no second target normalizer; reconstruct no
configured origins; route configured-default consumption through the
origin-bearing consuming `Resolution`, never the flat projection.

## Usage (caller's view)

See `design.md §1`: a confirm gains an optional `action` block with `when_true` /
`when_false` arms whose values are `stop`, `dry-run`, or `{ skip: rest|group }`.
Three call sites — terminal direct-apply (a stop prints one line and exits 0
with nothing written), headless wire (`status: stopped` / `complete` +
`disposition: "dry-run"`), and a crate caller matching `Interview::Ended` — drive
the same engine outcome through thin adapters.

## Shape

Model the action as an **effect the walk emits at the confirm's node position**,
exactly as the walk already emits messages, hooks, skips, and recorded answers.
The load-bearing decisions:

- **One funnel, keyed off active-vs-skipped.** `confirm_act(q)` runs only on the
  active path after `when` and after the held-answer check, at the two points an
  active confirm's boolean becomes final. A skipped/inactive confirm records a
  default `Answer::Bool` but is never routed to the funnel, so it can never fire —
  the guard is the `skipped` set, not the recorded bool (per
  encode-lessons-in-structure). Ordinary confirms return `Act::None`.
- **Skip *is* the existing skip flag.** `skip: group` flips the local loop's skip
  bit; `skip: rest` also propagates one level per return. This reuses
  `Advance::skip`, `default_ready`, and `warn_if_held` verbatim — no parallel
  walker, no new skipped-default rule (grounding "Avoid"). Already-answered
  siblings keep their values because the walk's `answers.contains_key` branch
  precedes the skip branch, which answers "what happens to the other batch
  answers" with existing behavior.
- **Stop is walk-terminal; dry-run is a sticky disposition.** Stop returns
  `Flow::Stopped` → `Interview::Ended`, which deliberately carries **no
  `Completed` and no hooks**: there is no API path from `Ended` to `Plan::build`,
  so "stop writes nothing / runs no hooks" is a type invariant, not a driver
  rule (per encode-lessons-in-structure, boundary-discipline). Dry-run does not
  alter control flow — it rides to a normal `Completed { disposition: DryRun }`
  so the plan it shows is the full plan, matching the existing `--dry-run`
  precedent for no-write.
- **Nothing new persists.** `disposition` and the stop are re-derived every walk
  from recorded answers; `StagedRecord` is unchanged, so replay is deterministic
  (per make-operations-idempotent, single-source-of-truth).
- **Atomicity is inherited, not re-added.** Effects live only inside the
  tentative `advance`, which is discarded on rejection; the `stands` classifier
  reads the new outcomes via the `skipped` set so a rejected answer can never
  produce a stop (1072 contract).

**Interface depth:** the public surface grows by one `Interview` variant, one
`Completed` field, one `Disposition` enum, and one opaque `Ended` — behind which
sit skip-scope unwinding, dry-run stickiness, effect ordering, and the
tentative-advance interaction. Drivers route the outcome and re-implement no
policy (small, deep interface, per the runner discipline). Validation lives at
load (`Template::load`) and at the answer boundary (`check_answer`); the funnel
trusts types.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- **We accept the walk's `bool` return widening to `Flow` in exchange for a
  single, honest control point.** Every recursive `walk` call updates, but the
  third and fourth outcomes (stop, skip-rest) become expressible where they
  occur rather than inferred afterward.
- **We accept stop and dry-run being modeled differently (terminal variant vs.
  completion field) in exchange for each matching its true meaning** — stop ends
  interrogation; dry-run needs the full answer set to plan. Forcing symmetry
  would either make stop plannable (losing the type-level no-write guarantee) or
  make dry-run terminal (planning over an incomplete answer set).
- **We accept that `Ended` carries answers only for reporting, not a
  `Completed`,** so a stopped interview is structurally unplannable. A future
  need to plan a stopped result would require a deliberate type change — which is
  the point.
- **We accept adding a wire `status: stopped` and an optional `disposition`
  field** to closed documents; both are additive and omitted for the ordinary
  case, so existing documents stay byte-identical.

## Alternatives considered

- **Disposition-only (no new `Interview` variant): a `control` field on
  `Completed` for all three actions.** Shallower type surface, but it exposes to
  every driver the obligation to *remember* not to apply on stop — the no-write
  guarantee becomes prose, not type. It also muddies the wire (`complete` with a
  `stopped` flag is a contradiction). Lost on interface depth: it hides less and
  leaks a policy to callers.
- **A parallel "control walk" that scans answers for actions after each step.**
  Re-derives reachability outside the one walk that already owns it, duplicating
  skip/when/group logic and risking divergence from the real walk — precisely the
  "second walker" the grounding says to avoid. Lost on single-source-of-truth.
- **Encode the action on `Prompt` / the wire schema and let drivers act on it.**
  Puts a transport-shaped decision on the public surface and forces each driver
  to re-implement firing rules (when-order, active-vs-skipped, batch fate).
  Violates boundary-discipline and multiplies the five-driver policy surface.
- **Infer the action from prompt semantics or make stop a special `when`.**
  Rejected by the task and AGENTS.md: actions are declared data; `when` decides
  activeness *before* the answer, actions fire *on* the recorded answer — keeping
  them non-colliding.

## Open questions and risks

- Should a `stopped` interview exit `0` (deliberate success, as designed) or a
  distinct non-zero code so scripts can detect a template-driven abort?
- For dry-run, should the plan reflect only answers gathered before the gate, or
  should a dry-run confirm be constrained (lint) to appear after all questions
  its plan depends on? The design plans over the completed answer set (dry-run
  rides to completion), so this is only a concern if an author gates dry-run
  early and later answers feed the plan — worth an authoring guideline.
- Sequencing vs 1056: confirm-flow's impl (1062) touches neither the target
  normalizer nor configured origins, so it can land before or after 1056. Do we
  want it explicitly ordered after 1056 to consume the origin-bearing
  `Resolution` directly, or landed independently against the contract? (Phase C.)
- Is `by` (the stopping confirm's id) the right minimal wire payload for
  `stopped`, or do agents also need the gathered answers echoed (currently
  included) for audit?

## Next implementation step

Widen the walk's return from `Result<bool, EvalError>` to `Result<Flow,
EvalError>` and thread the two `confirm_act`/`act` calls into the active-confirm
branches of `Advance::walk` — the smallest change that makes the effect
expressible where it occurs; the outcome types and drivers follow from it.
