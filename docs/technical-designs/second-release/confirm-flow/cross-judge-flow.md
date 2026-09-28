# Confirm-flow arena round 2 — read-only cross-judge report

Judge: other-family model (`ocx-gpt-5-6-sol`), read-only, given the rubric, both
groundings, the round-1 incumbent design, and the three flow candidates by path.
Recorded here as arena evidence; the parent's reconciliation is in `synthesis.md`.

## Judge scores (0–5)

| Criterion | flow-1 | flow-2 | flow-3 |
| --- | --- | --- | --- |
| C1 declaration/validation | 4 | 4 | 4 |
| C2 stop/dry-run, no files/hooks | 5 | 5 | 4 |
| C3 skip scope | 4 | 5 | 4 |
| C4 composition/atomicity/replay | 5 | 5 | 5 |
| C5 interface depth | 4 | 4 | 3 |
| C6 falsifiable validation | 4 | 5 | 4 |
| **Total** | **26** | **28** | **24** |

## Judge verdict

**Base: flow-2** (standalone + group-attached guard). Reasons: highest C3 (a
group-attached guard makes `skip: group` scope structural/move-safe, which the
judge reads as addressing the grounding Risk about scope silently changing output);
type-safe terminal `Interview::Stopped`; one pure `fire()` decision at two call
sites. Graft flow-3's monotone `Disposition` lattice as a routing helper but
**reject** putting the disposition on `Completed`; keep a terminal stop variant.
Reject flow-1's positional-only `skip: group`.

## Judge correctness screen

- flow-1, flow-2: type-safe terminal stop (`Ended`/`Stopped` cannot reach
  `Plan::build`); no leaks; additive protocol; no outcome/route cut.
- flow-3: flagged an **information-leakage / refactor hazard** — stop is a
  `Completed.disposition`, so "cannot be planned" is runtime discipline (`step()`
  before `Plan::build`) across ~6 call sites, not a type fact.
- All three: replay-deterministic, atomicity honored, origin-bearing `Resolution`
  respected, ordinary confirms untouched.

## Judge shape comparison (flow node vs round-1 action-bearing confirm)

Recommends the **flow node** decisively: confirm untouched (zero regression),
dramatically higher expressiveness (any `when` vs one boolean), type-safe stop, no
same-batch race (readiness gating), one pure decision engine. The only round-1
advantage is locality for the simplest "no → stop"; the judge judges the flow
node's one indirection worth it and aligned with Bob's "instead" direction.

## Parent note on this report

The parent adopts the judge's shape recommendation (flow node over confirm-action)
and its rejection of flow-3's disposition-stop, but does **not** adopt flow-2 as the
base — see `synthesis.md` for the reasoning (the group-attached guard's structural
`skip: group` largely duplicates the existing `group.when`; the task's `skip: group`
is "skip the rest of the current group", which is inherently positional).
