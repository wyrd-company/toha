# Rationale — directive field on `Completed`

## Problem

Toha must let a template map a `confirm` answer to a control action — `stop`,
`dry-run`, or `skip` — and carry that decision through the interview walk, the
plan/apply pipeline, and all five drivers, while ordinary confirms keep pure
boolean behavior and actions stay declared data. The shape is non-obvious
because the engine has **no** control-flow surface today where an answer's
*value* alters execution (grounding, "Scope"), and the change must honor several
constraints that crossed our boundary: the two-variant `Interview` state machine
whose `advance` is the sole result constructor; a flat, insertion-ordered
`Answers` map; replay determinism from raw submissions only; the closed wire
protocol (`status` in {questions, complete}, `additionalProperties: false`); the
closed `question` schema; the five fixed error-attribution (1072) policies plus
their four "do not" constraints; and the origin-bearing consuming `Resolution`
as the final configured-defaults contract (the flat a62061f projection is
grounding-only). We depend on the approved 1072 *contract*, not on unmerged 1056
runtime.

## Usage (caller's view)

Templates add an optional `action` map on a confirm, keyed by `when-true` /
`when-false`, with verbs `stop`, `dry-run`, and `{ skip: interview|group }`
(design §1.1). Crate callers drive to a `Completed` and either call one
funnel — `realize(template, completed, target, options, runner)` returning
`Realized::{Stopped, Previewed, Applied}` — or branch manually on
`completed.directive.writes_allowed()` / `plan_shown()` around the always-safe
`Plan::build` and the effectful `Plan::apply` (§1.2). Terminal `apply` reads the
directive and routes a stop to a one-line exit-`0` notice and a dry-run into the
existing `--dry-run` plan-print path (§1.3). Headless completion carries the
decision as an additive `outcome`/`outcome-by` on the still-`complete` wire
document; a proceed omits it, so ordinary documents are byte-identical to today
(§1.4).

## Shape

Data first. `Completed` gains one field, `directive: Directive` (Proceed / Stop
{by} / DryRun {by}); the two `Interview` variants are untouched. `skip` is
deliberately **not** part of the directive — it is realized by the engine's
existing walk skip machinery (`Advance::skip`, the `Skipped` map, group
recursion), so a skipped region records defined default/empty answers under the
existing `warn_unused` semantics rather than a new rule (composes with the "do
not infer a different skip/default policy" constraint). `stop` is modeled as
"skip the whole remaining interview **and** set `Stop`", so the interview always
lands on a well-formed `Completed` with a full answers map; `dry-run` sets the
directive but does **not** skip, so the plan it previews is complete.

The confirm action is encoded on the `QuestionKind::Confirm` variant
(`on_true` / `on_false: Option<Action>`), making "actions are confirm-only" a
type invariant, not a runtime check (per encode-lessons-in-structure). The
mapping is decided in exactly one place — `Advance::fire_action`, reached only
when the walk hits an *active, answered, non-skipped* confirm and no skip scope
is engaged (design §3). That funnel sits inside `advance`, which
`Pending::answer` reaches only after its rejection gate, so a rejected answer
never fires an action and a rejected document never persists (per
boundary-discipline; composes with the 1072 atomicity invariant). Because
`advance` recomputes the walk from the recorded answers on every step and every
replay, the directive is **derived, single-sourced, and never persisted** — no
extra runtime control state enters `StagedRecord` (per single-source-of-truth
and the replay-determinism invariant).

Interface depth. The public surface added is small — one field, one enum with
three predicate methods, one optional `realize`/`Realized` — but it hides the
whole confirm->action machine: the funnel, the skip-scope latch, batch-sibling
suppression, and the effect gate all sit behind `completed.directive` and its
predicates. Drivers become thin routers that call `writes_allowed()` /
`plan_shown()`; none re-implement the directive->effect policy (per the task's
"route the outcome, don't re-implement policy"). `Plan::apply` stays the sole
write/hook site; stop/dry-run never enter it, so "no writes/hooks" is enforced
by *not calling* one function, in one place. Validation lives at the boundary:
load-time rules (beside `loop`/`options`) reject action-off-confirm,
`skip: group` outside a group, and empty actions, each naming the corrective
form. The system deliberately does **not** add a target normalizer, reconstruct
configured origins, add an `Interview` variant, or stash control state in the
staged record.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept modeling `stop` as "skip-to-complete + Stop directive" (recording
  throwaway defaults for un-asked questions) in exchange for always landing on a
  well-formed `Completed` with a full answers map and one code path, rather than
  a partial/halted state that would need a third `Interview` variant.
- We accept a public field on `Completed` (any in-crate constructor must set it)
  in exchange for leaving every existing `Asking | Complete` match site
  compiling untouched — a far smaller blast radius than a new enum variant.
- We accept an additive `outcome` key on the closed complete wire document (a
  documented, `protocol: 1`-compatible schema change agents depend on) in
  exchange for representing stop/dry-run without a new `status` value or a second
  document kind.
- We accept that a confirm answered by its configured/question default fires its
  action (the default is the answer), rather than restricting actions to
  user-typed answers — consistent with how defaults become answers elsewhere.
- We accept suppressing later same-batch confirm actions once a stop/skip
  engages, in exchange for a deterministic, order-defined batch outcome.

## Alternatives considered

- **Third `Interview` variant (`Stopped`).** Deeper-looking but shallower in
  practice: it forces every `match interview` site across `main.rs`,
  `staging.rs`, `terminal.rs`, `protocol.rs` to handle a new case (exposing
  complexity to every caller), splits "the interview finished" across two
  variants, and still needs a place to carry dry-run. Lost on interface depth
  and blast radius.
- **`skip`/`stop`/`dry-run` as a control `Answer` value or a magic sentinel
  answer.** Overloads the flat `Answers` map with non-answer semantics, breaks
  the "ordinary confirm records `Answer::Bool`" invariant, and leaks into every
  answer consumer and the wire schema. Rejected: it infers control from answer
  shape rather than declared data.
- **Gate effects only in `main.rs` (no crate surface).** Smallest change, but
  makes stop/dry-run "the caller happens not to call apply" — non-first-class,
  re-implemented per driver, invisible to crate/test callers. Rejected against
  the grounding's "extend outcomes so stop and dry-run are first-class."
- **A parallel skip/reachability walker for action-skip.** Rejected outright:
  duplicates the walk and risks a divergent skipped-default rule, violating a
  1072 "do not" constraint. Reusing `Advance::skip` is the whole point of the
  shape.

## Open questions and risks

- Is exit code `0` the right terminal/direct result for a user- or
  template-chosen `stop` (treating it as success, not error)? If a non-zero
  "stopped" code is wanted, which value, and does it collide with the existing
  `4` (incomplete) / `1` (error) codes?
- Additive-but-closed wire change: adding `outcome`/`outcome-by` to an
  `additionalProperties: false` document is a contract change consumers pin
  against. Is keeping `protocol: 1` and omitting the key on `proceed` sufficient,
  or should the protocol version bump?
- Should `stop` really record defaults for the skipped remainder (clean
  `Completed`, throwaway answers), or is a leaner "answers so far only" stop
  preferred despite needing a non-`Complete` shape?
- Sequencing vs 1056: should the paired implementation (1062) land after 1056,
  or express its coupling purely as the atomicity invariant + target-identity
  type so it is order-independent (this design assumes the latter)?
- When both branches of a confirm map to the same terminal action (always fires
  regardless of the boolean), should load emit a warning, or stay silent?

## Next implementation step

Add `Directive` and the `directive` field to `Completed`, thread a `directive`
accumulator through `advance`, and write the driver-parity regression test that
proves an ordinary confirm still produces a byte-identical complete document
before any action behavior exists.
