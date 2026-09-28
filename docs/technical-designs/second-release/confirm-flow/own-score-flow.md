# Confirm-flow arena round 2 — parent's own scoring (flow-node shape)

Independent read of the three flow candidates end to end, scored against
`rubric.md` (the six criteria are shape-agnostic and reused unchanged). Done
before reconciling with the round-2 cross-judge.

## Scores

| Criterion | flow-1 (minimal node, stop=Ended variant) | flow-2 (attachable guard: node + group) | flow-3 (unified Disposition + step()) |
| --- | --- | --- | --- |
| C1 declaration/validation | 5 | 4.5 | 5 |
| C2 stop/dry-run, no files/hooks, all drivers | 5 | 5 | 4.5 |
| C3 skip scope (whole/group, nested, defined values) | 5 | 5 | 5 |
| C4 composition/atomicity/replay determinism | 5 | 5 | 5 |
| C5 interface depth / seam placement | 4.5 | 4 | 5 |
| C6 falsifiable validation | 5 | 5 | 5 |
| **Total** | **29.5** | **28.5** | **29.5** |

## Notes

All three share the winning structural core Bob asked for: a `flow` node as a peer
of message/hook/computed, no id, records no answer, reusing `when` + the node walk +
the skip machinery verbatim, leaving `QuestionKind::Confirm` **completely untouched**
(ordinary confirms pure boolean by construction — the strongest form of that
invariant, better than round‑1's guarded field). None reconstructs origins or uses
the flat `Resolution`; all are replay-deterministic with no new persisted state; all
compose with the 1072 atomicity invariant.

### flow-1 — minimal standalone node (recommended base)

Tightest realization: `flow: <action>` key + `when`/`label` siblings, one walk arm
mirroring the message/hook arm. Stop is a new terminal `Interview::Ended` variant
that **`Plan::build` cannot consume** — "stop writes nothing / cannot be planned" is
a type fact. Dry-run is a `Disposition` field on `Completed`; skip reuses the flag
with one-frame-per-return `SkipRest` propagation. Cost: the `Ended` variant forces a
third arm on the `Asking | Complete` match sites (compile-forced, and uniformly
"print + no plan").

### flow-3 — unified `Disposition` + `Completed::step()` (closest runner-up)

Smallest surface and deepest single seam: keeps `Interview` at two variants; stop is
`Completed { disposition: Stop }` reached by halting the walk (`halt`), and every
driver routes through one method `Completed::step() -> Step::{Stop, Plan{apply}}`.
Crucially its stop **halts** the walk (it does not skip-to-complete), so — unlike
round‑1's directive-on-Completed candidate — it renders no downstream defaults and
cannot fault on an unreached question. Its one cost vs flow-1 is that
"cannot be planned" is enforced by driver discipline (`step()` before `Plan::build`),
not by the type. Its same-batch readiness argument and monotone/idempotent dry-run
`raise` are the sharpest in the set.

### flow-2 — attachable guard (node + group)

Same guard shape in two placements; its distinctive value is making `skip: group`
scope **structural** (a `guard` on the group it governs, move-safe) rather than
positional. Genuinely useful, but it is the largest surface (a `Group.guard` field
plus the standalone node) and carries one honest overlap (`group.flow {skip: group}`
vs `group.when: false`). Best treated as an optional affordance/decision layered over
a standalone-node base, not the base itself.

## Recommendation (pending cross-judge reconciliation)

**Base: flow-1.** The flow-1 vs flow-3 tie is the round-2 echo of round-1's
type-safety-vs-smallest-surface split; consistent with the round-1 decision and the
`encode-lessons-in-structure` priority, take the shape where applying a stopped
interview is **unrepresentable** (flow-1's `Ended`). Then **graft flow-3's
`Completed::step()` seam** for uniform dry-run/apply routing (drivers match `Ended`
for stop, then call `step()` on a `Completed` for proceed-vs-dry-run), flow-3's
same-batch readiness framing, and its idempotent dry-run raise; and **surface flow-2's
group-attached guard** as an optional structural-`skip: group` affordance for Bob to
decide, not baked in. Reject stop-as-advisory-disposition as the base (keep type
safety) and reject baking in the second attach point (scope/ergonomics decision).

The larger question Bob posed — the `flow` node shape vs the round‑1 action-bearing
confirm — is addressed in `synthesis.md` with a recommendation and laid out as the
headline product decision at the checkpoint.
