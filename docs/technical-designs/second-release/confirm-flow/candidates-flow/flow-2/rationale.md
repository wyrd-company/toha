# Rationale — `flow` as an attachable guard

## Problem

Toha has no control-flow surface where an answer's *value* alters execution; the
feature adds one — stop, dry-run, and scoped skip — driven by a `when` expression,
without touching the `confirm` type. The engine constraints make the shape
non-obvious: the walk returns a bare `bool` (`false` == "stopped early",
disambiguated only by a `blocked` field), independent questions coalesce into one
batch so a control point cannot assume it runs before its batch-siblings are
answered, the wire protocol is a closed two-status schema (`additionalProperties:
false`), and nothing beyond raw submissions may persist — every outcome must be
re-derivable on replay. The design must also compose with the approved 1072 contract
(the answer-transaction atomicity invariant, the opaque target-identity type, and the
origin-bearing consuming `Resolution`) while depending on none of the unmerged 1056
runtime, and must reuse the existing skip machinery rather than fork a second walker.

## Usage (caller's view)

See `design.md` — the README section and three call sites (library, headless/wire,
terminal) are written first; the types below are derived from them. The load-bearing
caller fact: every driver reduces to a **three-way match** on `Interview` /
`Outcome` / `Headless` — `Asking` | `Complete` | `Stopped` — plus a `disposition()`
check on `Complete` that OR's with the CLI `--dry-run` flag. That is the whole cost
the feature imposes on callers.

## Shape

A `flow` guard is one shape — `{ when?, action, label? }` — that attaches in two
positions sharing one decision engine: a standalone `Node::Flow(Guard)` peer of
message/hook/computed, and an optional `guard: Option<Guard>` field on `Group`. The
guard carries no id and records no answer, so `confirm` and the flat `Answers` map
are untouched (*per encode-lessons-in-structure*: "ordinary confirms stay pure
boolean" is a structural fact, not a runtime guard on the confirm type). One pure
function `fire(&self, guard, skip) -> Fired` makes the decision from committed
answers and mutates nothing; the two placements are two call sites of it, and the
walk applies the effect (*per boundary-discipline*). The walk's return type becomes a
four-arm `Flow` enum (`Ran`/`Blocked`/`Stopped`/`SkipRest`) that replaces the
overloaded `bool`; `skip: rest` propagates by flipping the loop's `mut skip`
parameter for later siblings and returning `SkipRest` so each ancestor flips too,
reusing `Advance::skip`/`Skipped`/group recursion verbatim — no parallel walker.
`skip: group` scope is the design's centerpiece: **structural** when the guard sits
on the group it governs (move-safe, unambiguous), **positional** for a standalone
node, and a **load error** for a standalone `skip: group` at top level (validate at
the boundary; the corrective form is named). Terminal outcomes are encoded in types:
`Interview::Stopped` cannot reach `Plan::build`, so "no files, no hooks, cannot be
planned" is a type fact; `Completed.disposition` carries dry-run. No new state
persists — `dry_run` and `stop_reason` are working fields of one `advance`,
re-derived every walk and every replay (*per make-operations-idempotent* and single
source of truth: the committed submissions). Interface depth is high: one small
surface (one node variant, one group field, three enums, one `Interview` variant, one
wire status + one optional field) hides the whole engine — readiness gating,
cross-frame skip propagation, dry-run stickiness, terminal unwinding, replay
re-derivation.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept **two attach points for one guard** (larger surface, one genuine overlap
  where `group.flow {skip: group}` and `group.when: false` both skip a subtree) in
  exchange for making `skip: group` scope *structural* and giving stop/dry-run a
  boundary home — the reason this candidate diverges from a bare standalone-node
  design.
- We accept **one indirection between the question and its control point** in
  exchange for leaving `confirm` completely untouched and reusing `when` — pure
  confirms carry zero regression risk.
- We accept **re-deriving stop/dry-run/skip on every walk** (recomputation over
  caching) in exchange for zero new persisted state and trivially correct replay;
  the guard is idempotent, so recomputation is free of side effects.
- We accept an **additive wire-contract change** (a new `stopped` status and an
  optional `disposition` field) that agents pinning the old schema must refresh; it
  removes nothing.
- We accept that a **stop discards prompts already queued** in the same batch that
  sort before the guard; the outcome stays deterministic and the stop is terminal by
  intent.

## Alternatives considered

- **Standalone `flow` node only (no group attachment).** Smaller surface, one attach
  point. Lost because `skip: group` scope stays positional and implicit — moving the
  node silently changes what it skips — which is precisely the fragility the manager
  asked to remove; it hides less and exposes a foot-gun to authors.
- **Group guard only (no standalone node).** Every stop/dry-run/skip-rest would need
  a wrapping group, adding structural noise for position-anchored intent and having no
  natural home for a top-level stop. Lost on `minimize-reader-load`.
- **Action on the `confirm` type (round 1).** More locally obvious for "this no →
  stop", but changes the confirm type (regression surface), binds the trigger to one
  boolean (less expressive), and adds a bespoke confirm-answer decision funnel instead
  of reusing `when` + the node walk. Lost on interface depth and the "confirm
  untouched" constraint.
- **A new persisted control field on the staged record.** Rejected outright: it
  violates replay-from-submissions and adds state that can drift from the answers that
  imply it.

## Open questions and risks

- Should a group-attached guard whose action is exactly `{ skip: group }` be linted
  toward `group.when: false` (since they coincide), reserving the guard for
  stop/dry-run/skip-rest, or is the uniform vocabulary (all four actions attach the
  same way) worth the overlap?
- On a `stop`, should an unused early answer for a not-reached question emit the
  "answer not used" warning (as skip does), stay silent, or surface a distinct "stop"
  message? The design currently drops it silently, like a proven-skip; is a stop
  warning surface wanted?
- Should a standalone `skip: group` at top level be a hard load error (current
  choice, naming the fix) or silently mean `skip: rest`? The round-2 re-ground leaned
  "group at top level equals rest"; this design chose the stricter, more explicit
  rule per the runner discipline — confirm the preference.
- Should `stop`/`dry-run` be allowed on a group-attached guard at all, or restricted
  to `skip` there (keeping the boundary strictly about the group)? Current design
  allows all four for uniformity.
- Risk: sequencing vs 1056 — the design depends on the approved 1072 *contract*, not
  the unmerged runtime; runtime integration order (after 1056) is fixed coordination.
  The paired implementation must not land runtime integration ahead of 1056.

## Next implementation step

Add `Node::Flow(Guard)`, the `Group.guard` field, and the `Guard`/`FlowAction`/
`SkipScope` types with their builder arm and load rules in `template.rs`, then change
`walk` to return `Flow` and introduce the pure `fire` decision function — the two
seams every driver and the protocol adapt to.
