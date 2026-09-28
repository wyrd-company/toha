# Confirm-flow design — verification

Phase F. The synthesized design is checked against the caller usage, every rubric
criterion and inherited constraint, the failure cases, and the approved predecessor
contracts. Result: the design holds.

## Caller usage traced against the sketch

- Crate caller matches `Interview::{Asking, Complete, Ended}` or routes through
  `realize(...) -> Realized{Stopped, Previewed, Applied}` — both consume the types
  in `Data structures`/`Public interfaces`; the usage and the sketch agree.
- Terminal `drive -> DriveResult{Completed, Stopped}` and `main.rs` routing on
  `Completed::disposition().writes_allowed()` match the sketched signatures.
- Headless `answer_headless -> Headless{Completed, Pending, Stopped}` and the three
  wire builders (`batch_document`, `complete_document` + optional `disposition`,
  `stopped_document`) match. Staged `replay_with_resolution -> Interview` returns
  the widened outcomes. No usage references a type or field the sketch omits.

## Evidence-carrying contract (per architect "verify the contract can carry its evidence")

Every field the design promises is traced source → carrier → signature → renderer:

- **Stop trigger `by`.** Source: the confirm `Id` at the funnel (`stopped_by` on
  `Advance`). Carrier: `Ended.by`. Signatures: `Ended::by()`, `stopped_document`.
  Renderer: the `stopped` wire document `by` field and the terminal stderr notice.
  Survives end to end.
- **Dry-run disposition.** Source: the funnel raising `disposition = DryRun`.
  Carrier: `Completed.disposition` (re-derived each walk). Signatures:
  `Completed::disposition()`, `complete_document`. Renderer: the optional
  `disposition: dry-run` wire field and the apply-gate branch. Survives; omitted for
  `Proceed` so ordinary documents are byte-identical.
- **Skip "answer not used" warning.** Source/carrier/renderer are the *existing*
  `warn_if_held`/`warn_unused`/`skipped_warning` path, reused verbatim; the design
  adds no new provenance to carry, so no hop can drop it.
- **No promised field requires a caller to reconstruct it from an identifier or a
  final value** — the one risk the architect skill names. `by` and `disposition`
  are produced at the funnel and carried on the result types; nothing downstream
  rebuilds them.

## Dependency capability boundary (per architect "verify dependency capability boundaries")

Not applicable: the design depends on no parser/compiler/runtime feature of a
dependency (no new Jinja construct, no `minijinja` feature gate). The only external
contracts are the sibling toha designs (1058 presets integrated; 1072/1056
error-attribution approved), addressed under Predecessor composition below.

## Rubric criteria checked against the design

- **C1 declaration/validation.** `action` on confirm with `when_true`/`when_false`,
  values `stop | dry-run | {skip: rest|group}`; closed enum; four load rejections
  naming the corrective form; additive to the closed `question` schema with an
  "action only on confirm" rule. Ordinary confirms unaffected; actions declared
  data. Met.
- **C2 stop/dry-run, no files/hooks, all drivers.** Stop = terminal `Ended`
  carrying no `Completed`/hooks (unplannable by construction); dry-run = `Completed`
  with `Disposition::DryRun` suppressing apply at the sole write/hook site;
  represented across terminal/headless(wire)/staged/direct/crate; the closed
  protocol extended additively (new `stopped` status, optional `disposition`).
  Distinct from the `apply --dry-run` flag (composes by union) and from ordinary
  planning. Met.
- **C3 skip scope.** `rest` vs `group`, nested via one-level `Flow::SkipRest`
  propagation; group-at-top == rest; skipped questions record the existing
  default/empty values with the existing "answer not used" warning; no new rule.
  Met.
- **C4 composition/atomicity/replay.** Fires after `when` and the held check, keyed
  off active-vs-skipped; effects live only in the discarded tentative advance;
  `stands` reads outcomes via the `skipped` set; nothing new persisted, outcome
  re-derived on replay; composes with the five 1072 policies and origin-bearing
  `Resolution`. Met.
- **C5 interface depth.** One funnel in the engine; one terminal variant + one
  completion field on the public surface; drivers thin; the `realize` gate hides the
  effect policy for crate/direct callers; `Flow`/`Act` engine-private (no public
  blast radius). Met.
- **C6 falsifiable validation.** 22 numbered behaviors covering both truth values,
  nesting, resume/replay, driver parity, and negative/atomicity cases, each stated
  as "how it could fail." Met.

## Inherited constraints checked

- Ordinary confirm keeps boolean behavior unless an action is configured — held
  (behavior 5, pure-boolean regression pin).
- Actions never inferred from prompt text — held (declared data only; behavior list
  and Out of scope).
- Pure interview engine, plan-before-apply, one engine per driver — held (funnel in
  the engine; `Plan::build` pure; drivers are adapters).
- Supported terminal/headless/staged/direct/crate paths preserved — held (each
  observes the outcome; no path removed).
- No production stubs / no premature shared schema edits — held (all sketches in
  this package; canonical edits only described, owned by 1062).
- Generic non-identifying examples — held (`example.invalid`-style, generic
  templates; no user/domain names).

## Failure cases and negative scenarios

- Rejected document sharing a batch with a firing confirm ⇒ rejected, no effect,
  nothing persists (behavior 20; atomicity by placement).
- Stop before a proven-skipped early answer ⇒ early error dropped, stop stands
  (behavior 21; `stands` via `skipped`).
- `when`-inactive confirm ⇒ never fires (behavior 16).
- Invalid downstream default ⇒ a stop still exits cleanly because it renders no
  later default (behavior 8) — the property C2's stop-as-completion lacked.
- Submission after a terminal interview ⇒ terminal-replay error (behavior 22).

## Compatibility with approved predecessor revisions

- **Presets 1058 (integrated).** Uses the real `Seed`/`Resolution`/
  `configured_defaults`/`Interview::start` shapes; adds no configured-default
  consumption; provenance-neutral.
- **Error-attribution 1072 (approved, unmerged 1056).** Depends on the approved
  contract only: atomicity invariant + target-identity type + origin-bearing
  consuming `Resolution`; adds no second normalizer, reconstructs no origins,
  accepts no rejected answer into probe state, infers no different skip/default
  policy. The design needs no 1056 code; per the effort's sequencing ruling,
  1062's runtime integration lands after 1056 (a coordination fact, not a
  decision — design preparation is independent, runtime integration is not). No
  approved interface is changed by this design, so no dependent returns to review
  on its account.

## Residual risks (carried to the checkpoint)

- The four product decisions D1, D2, D3, D5 (`design.md`) are genuine choices with
  recommendations; D2 (a preset silently firing a terminal action) is the one worth
  Bob's explicit attention. Sequencing (former D4) is no longer a Bob decision: the
  effort's sequencing ruling fixes 1062's runtime integration after 1056.
- Adding a wire `status: stopped` and a `disposition` field to `additionalProperties:
  false` documents is an additive contract change agents depend on; documented and
  omitted-on-ordinary, but it is a contract addition, not a no-op.

## Verdict

Caller usage agrees with the sketch; every criterion and inherited constraint is
met; failure cases are covered by falsifiable behaviors; predecessor composition
holds. The synthesized design is sound and ready for the Phase C human checkpoint.
No re-frame or re-run is required.
