# Confirm-flow arena — parent's own scoring

Independent read of all four candidates end to end, scored criterion by criterion
against `rubric.md` (0–5). Done before reconciling with the cross-judge.

## Scores

| Criterion | C1 (Interview variant) | C2 (directive on Completed) | C3 (unified 4-arm value) | C4 (node-level effect) |
| --- | --- | --- | --- | --- |
| C1 declaration/validation | 5 | 5 | 4.5 | 5 |
| C2 stop/dry-run, no files/hooks, all drivers | 5 | 4 | 5 | 5 |
| C3 skip scope (whole/group, nested, defined values) | 5 | 5 | 5 | 5 |
| C4 composition/atomicity/replay determinism | 5 | 4.5 | 5 | 5 |
| C5 interface depth / seam placement | 4.5 | 5 | 4.5 | 5 |
| C6 falsifiable validation | 5 | 5 | 5 | 5 |
| **Total** | **29.5** | **28.5** | **29** | **30** |

## Per-candidate notes

### C4 — node-level effect (recommended base)

Strongest overall. Models the action as an effect the walk emits at the confirm's
node position — the most faithful fit to how the walk already emits messages,
hooks, skips, and recorded answers. Three decisive properties:

- **Stop is unplannable by construction.** `Interview::Ended` carries *no*
  `Completed` and no hooks, so there is no API path from a stop to `Plan::build`.
  "Stop writes nothing / runs no hooks" is a type invariant, not a driver rule —
  the strongest correctness guarantee of the four.
- **Most rigorous atomicity treatment.** Effects live only inside the discarded
  tentative `advance`; the `stands` early-failure classifier is extended to read
  the new outcomes via the `skipped` set (not the effect kind), so a rejected
  answer can never produce a stop and a proven-skipped early error is still
  dropped per 1072 policy 2. Behaviors 18–19 pin exactly this.
- **Skip scope is one bit.** `skip: group` flips the local loop skip; `skip: rest`
  additionally propagates one level per return. Nesting composes because
  propagation is one level per `walk` return; reuses `Advance::skip`/`warn_if_held`
  verbatim.

Cost: widens the walk's return `bool → Flow`. This is engine-private, so it has no
public blast radius; every recursive `walk` call updates but nothing outside the
engine sees it.

### C1 — third `Interview::Stopped` variant

Very close second, and it **converges with C4** on the load-bearing shape:
stop = terminal variant (unplannable), dry-run = derived field on `Completed`,
skip = resolved in the walk with the existing machinery. That convergence across
two independent candidates is a strong signal the shape is right. C1 expresses it
slightly less rigorously than C4 (no explicit walk-effect framing; the
`skip_group` frame-local latch is correct but less general than C4's
`Flow::SkipRest` propagation), and its dry-run/stop asymmetry is asserted rather
than derived from the walk's existing effect model.

### C3 — unified 4-arm `Interview`

Excellent and the most "unified" surface: `Asking | Complete | Stopped | DryRun`,
internal `directive_of(q, &Answer) -> Directive` pure funnel, `DryRun(Completed)`
reusing `Completed` for byte-equal plans. Loses marginally on interface economy:
a separate `DryRun` arm is heavier than carrying dry-run as a `Completed` field
(C1/C4), and it adds two wire statuses (`stopped` + `dry-run`) where a
`disposition` field on `complete` suffices for dry-run. The `true:`/`false:` YAML
mapping keys are slightly awkward vs `when_true`/`when_false`. Its pure
`directive_of` naming and its "`skip: group` at top == `skip: interview`, no
special case" clarity are worth grafting.

### C2 — directive field on `Completed`

Best blast-radius story (a field on `Completed` leaves every existing two-arm
`match interview` site compiling) and a genuinely nice crate-facing funnel —
`realize(...) -> Realized` plus `directive.writes_allowed()/plan_shown()`. But its
core correctness choice is the weakest: it models **stop as "skip the whole
remaining interview + set Stop directive"**, so a stopped run still walks the
remainder as skips (rendering defaults, doing extra work, and able to surface a
template-default fault a true stop should never reach) and surfaces on the wire as
`status: complete`. It also lets a driver that ignores `directive` apply anyway —
the misuse compiles, which C2 itself flags. The new-variant approach (C1/C3/C4)
makes "forgot to handle stop" a compile error, and C4 makes it structurally
impossible. C2's `realize()` funnel is the graft worth keeping.

## Screening for design red flags

- No candidate is a shallow module, pass-through, or temporal decomposition; all
  concentrate the decision in one engine funnel and keep drivers as thin adapters.
- **Information leakage watch:** C3 exposes the most (4 `Interview` arms + 2 wire
  statuses); C2 keeps `Directive` public but that is intended. None leak transport
  types onto the public surface (all keep `action` behind the template boundary).
- C2's stop-as-completion is not a red flag per se, but it is the one choice that
  trades a correctness/honesty property for a smaller blast radius.

## Recommendation (pending cross-judge reconciliation)

**Base: C4.** Graft the crate-facing `realize()`/directive-predicate effect gate
from C2 (as an ergonomic convenience over the base types, without adopting
stop-as-completion); graft C3's pure `directive_of` naming and its explicit
"group-at-top == interview" clarity; note the C1/C4 convergence on
(stop = terminal unplannable variant, dry-run = `Completed` field, skip = in-walk)
as the consensus shape. Reject C2's stop-as-skip-to-complete and C3's separate
`DryRun` arm in favor of the leaner `Completed { disposition }` + terminal `Ended`.
