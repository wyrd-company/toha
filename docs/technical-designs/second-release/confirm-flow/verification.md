# Flow-node design — verification (round 2)

Phase F for the round‑2 `flow` node design (`design.md`). The round‑1
(confirm-action) verification is preserved in git history and its design as
`design-round1-confirm-action.md`. The synthesized flow design is checked against
the caller usage, every rubric criterion and inherited constraint, the failure
cases, and the approved predecessor contracts. Result: the design holds.

## Caller usage traced against the sketch

- Crate caller matches `Interview::{Asking, Complete, Ended}` and routes a
  `Completed` through `completed.step() -> Step::Plan { apply }` — both consume the
  types in Data structures/Interfaces; usage and sketch agree.
- Terminal `drive -> Session::{Completed, Ended}` and `main.rs` routing via
  `completed.step()` / `--dry-run` union match the signatures.
- Headless `answer_headless -> Headless::{Completed, Pending, Ended}` and the three
  wire builders match; staged `replay_with_resolution -> Interview` returns the
  widened outcomes. No usage references a type or field the sketch omits.

## Evidence-carrying contract

- **Stop label / messages.** Source: the flow node's `label` and reached messages →
  carrier `Ended { label, messages }` → signatures `Ended::label()/messages()`,
  `ended_document` → renderer: the `ended` wire document and the terminal notice.
  Survives end to end; nothing downstream must reconstruct it.
- **Dry-run disposition.** Source: the walk raising `Disposition::DryRun` → carrier
  `Completed.disposition` (re-derived each walk) → `Completed::disposition()`/`step()`,
  `complete_document` → renderer: optional wire `disposition` + the apply-gate branch.
  Omitted for `Proceed`, so ordinary completions are byte-identical.
- **Skip "answer not used" warning.** Reuses the existing `warn_if_held`/`warn_unused`
  path verbatim; the design adds no new provenance to carry.

## Dependency capability boundary

Not applicable: the flow node depends on no new parser/compiler/runtime feature of a
dependency. `when` is the existing conditional construct evaluated by the existing
`Expr`/`minijinja` path (`interview.rs:1213/1251`). The only external contracts are
the sibling toha designs (1058 integrated; 1072/1056 approved), addressed below.

## Rubric criteria checked

- **C1 declaration/validation.** `flow: <action>` + `when`/`label` siblings; closed
  `FlowAction`; load rejections (unknown action; `skip: group` needs a group; `flow`
  not combined with other node keys) naming the corrective form; additive to the
  closed `$defs/node` `oneOf`. Actions are declared data; `confirm` untouched. Met.
- **C2 stop/dry-run, no files/hooks, all drivers.** Stop = terminal `Ended` (no
  `Completed`/hooks → unplannable by type); dry-run = `Completed{DryRun}` suppressing
  the sole write/hook site; represented across all five drivers; the closed protocol
  extended additively (`ended` status, optional `disposition`); distinct from and
  composing with `apply --dry-run`. Met.
- **C3 skip scope.** `rest` (climb one frame per return) vs `group` (rest of the
  current group), nested; whole-group conditional skip is `group.when`; skipped
  values are the existing default/empty with the existing warning; a top-level
  `{ skip: group }` is a load error. No new rule. Met.
- **C4 composition/atomicity/replay.** Fires only when reached with `when` true and
  not under a skip; same-batch readiness gates the trigger (no race); a rejected
  document discards the tentative advance; `stands` reads outcomes via `skipped`;
  nothing persisted, re-derived on replay; composes with the five 1072 policies and
  the origin-bearing `Resolution`. Met.
- **C5 interface depth.** One walk arm holds the decision; public surface is one
  `Node` variant + `FlowNode`/two enums + one `Interview` variant + `Ended` + a
  `Completed` field + `step()` + one `Headless`/`Session` variant + `ended_document`;
  `Walk` is engine-private (no public blast radius); drivers route only. Met.
- **C6 falsifiable validation.** 19 numbered behaviors covering both branches of a
  condition, nesting, resume/replay, driver parity, and negative/atomicity, each
  stated as "how it could fail." Met.

## Inherited constraints checked

- Ordinary confirm keeps boolean behavior — held by construction (the `confirm` type
  is not modified; behavior 4 pins it).
- Actions never inferred from prompt text — held (declared data on the flow node).
- Pure engine, plan-before-apply, one engine per driver — held (decision in the
  engine; `Plan::build` pure; drivers are adapters).
- Supported terminal/headless/staged/direct/crate paths preserved — held.
- No production stubs / no premature shared schema edits — held (sketches only;
  canonical edits described, owned by 1062).
- Generic non-identifying examples — held.
- The existing outcomes are all preserved: stop, dry-run, skip whole-interview, skip
  current-group, nested, default/when/early-answer/replay — none cut (the cross-judge
  confirmed no outcome/route is removed).

## Failure cases and negative scenarios

- Rejected document with a firing flow action ⇒ rejected, no effect, nothing persists
  (behavior 17; atomicity by placement).
- Stop before a proven-skipped early answer ⇒ early error dropped, stop stands
  (behavior 18).
- Flow node under a false `when`/ancestor skip ⇒ never fires (behavior 13).
- Same-batch pending reference ⇒ blocks, fires only after commit (behavior 14).
- Invalid downstream default ⇒ a stop still exits cleanly (behavior 7).
- Submission after a terminal interview ⇒ terminal-replay error (behavior 19).

## Compatibility with approved predecessor revisions

- **Presets 1058 (integrated):** uses the real `Seed`/`Resolution`/`configured_defaults`/
  `Interview::start` shapes; the flow node adds no configured-default consumption;
  provenance-neutral.
- **Error-attribution 1072 (integrated via 1056):** the error-attribution
  implementation (1056) is integrated on the epic (`8a19269`). Verified against the
  integrated code — the opaque `CanonicalTarget` + sole
  `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>` factory,
  `Store::{path_for,load,remove}(&CanonicalTarget)`,
  `Plan::build`/`apply(&CanonicalTarget)`, and the origin-bearing consuming `Resolution`
  (`warnings()`/`start`/`into_flat_defaults`) — match the contract the design targeted,
  with no drift. The design adds no second normalizer, reconstructs no origins, accepts
  no rejected answer into probe state, and infers no different skip/default policy. No
  approved interface was narrowed.

## Residual risks (carried to the checkpoint)

- D‑shape is the genuine structural decision for Bob (flow node vs confirm-action);
  both reviewers recommend the flow node, but it is Bob's call.
- D‑attach (standalone-only vs a group-attached guard) and D2 (a preset silently
  satisfying a flow `when`) are the substantive product choices.
- Adding an `ended` status and a `disposition` field to `additionalProperties: false`
  documents is an additive contract change agents depend on — documented, omitted on
  ordinary completions, but a contract addition.

## Abort action (adopted at approval) — verification

Bob pre-approved the `abort` disposition either way (verbatim on the task); the author
adopted it. `abort` = `stop` + remove the target's staged record via the existing
`Store::remove(&CanonicalTarget)`. Checks:

- **All routes / staged cleanup.** Abort removes the target's staged record where one
  exists (staged/continue, headless, terminal/direct resuming a staged interview) and
  is a no-op where none exists (fresh direct apply, crate) — behaviors 8–9. It reuses
  the sole `canonical_target` factory for the identity — behavior 11 (sole-kill:
  divert to a raw path → the removal-key test fails).
- **Error semantics.** A failed `Store::remove` surfaces the underlying `StagingError`
  (nonzero); the stop portion — no files, no hooks — has already held — behavior 10
  (sole-kill: swallow the error → the failure-surface test fails).
- **Atomicity — abort never runs from a rejected document.** The removal is a
  driver-layer effect applied only after a committed `Ended{Abort}`, never from the
  discarded tentative advance — behavior 21 (sole-kill: move the removal before the
  rejection gate → the test fails). This is the load-bearing abort guard.
- **No new capability / no interface narrowing.** Abort reuses the existing supported
  `Store::remove`; it adds no second normalizer, no trust/permission/timeout/subprocess
  surface, and removes only the current target's record (Out of scope).
- **Engine stays pure.** The engine yields `Ended{kind: Abort}`; the removal is
  performed by the driver like any effect (mirrors how apply performs writes).

Type-safety: `abort` is the `Ended` terminal variant (differing only by `kind`), so
"stop/abort writes nothing and cannot be planned" remains a type fact.

## Approval-closure status

Bob approved the flow-node shape and pre-approved abort (2026-09-28 19:37); the
recommended choices are resolved (Decisions section of `design.md`). The
error-attribution producer (1056) is now integrated on the epic (`8a19269`); the
branch was rebased onto that head, with merge-base == current epic head proved before
these edits, and the design re-verified against the integrated producer interfaces
(`CanonicalTarget`/`canonical_target`, `Store::remove(&CanonicalTarget)`,
`Plan`/`apply(&CanonicalTarget)`, origin-bearing `Resolution`) with no drift. The
approved design shape (stop/dry-run/skip/abort) is unchanged. Independent design review
is no longer held: the task is handed off for cold review at the refreshed clean basis
with the approved evidence recorded.

## Verdict

Caller usage agrees with the sketch; every criterion and inherited constraint is met;
failure cases (including abort routes, cleanup, error semantics, and the
rejected-document guard) are covered by falsifiable behaviors with named sole-kills;
predecessor composition holds; no existing outcome or route is cut. The flow-node
design (with the adopted `abort` action) is sound and approved. No re-frame or re-run
is required.
