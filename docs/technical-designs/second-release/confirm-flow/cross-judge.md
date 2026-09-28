# Confirm-flow arena — read-only cross-judge report

Judge: other-family model (`ocx-gpt-5-6-sol`), read-only, given the rubric,
grounding, and the four candidates by path. Preferred a different family from the
parent per the arena skill. Recorded here verbatim in substance as arena evidence;
the parent's reconciliation is in `synthesis.md`.

## Judge scores (candidate × criterion, 0–5)

| Candidate | C1 | C2 | C3 | C4 | C5 | C6 | Total |
| --- | --- | --- | --- | --- | --- | --- | --- |
| C1 (Stopped variant) | 5 | 5 | 5 | 5 | 4.5 | 5 | 29.5 |
| C2 (directive on Completed) | 5 | 5 | 5 | 5 | **5** | 5 | 30 |
| C3 (four-arm Interview) | 5 | 5 | 5 | 5 | 4 | 5 | 29 |
| C4 (node-level walk effect) | 5 | 5 | 5 | 5 | 4.5 | 5 | 29.5 |

## Judge verdict

**Base: C2.** Every criterion tied at 5 except **C5 (interface depth)**, where the
judge scored C2 highest for its `realize()` wrapper that "completely hides all
confirm-action policy — drivers call one function and match the result." The judge
recorded the stop-as-skip-to-complete tradeoff ("the interview runs to full
completion even for stop") and judged it "not a correctness issue; the grounding
does not forbid it."

## Judge red-flag screen

No shallow module, information leakage, temporal decomposition, or pass-through
method in any candidate.

## Judge correctness screen (all four judged safe)

- File/hook leak on stop: all four safe (C1/C3/C4 type-safe terminal; C2 safe via
  `realize()`/`stopped_by()` gate).
- Dry-run leak: all four gated at the apply boundary.
- Ordinary confirm boolean behavior: all four preserve `Answer::Bool` end to end.
- Replay determinism: all four re-derive the outcome from stored submissions.
- Atomic answer transaction: all four honor it (rejected answer never fires).
- Configured origins: all four route through the origin-bearing consuming
  `Resolution`; none reconstruct origins or use the flat projection.
- Protocol schema closure: all four additive and documented.

## Judge graft recommendations

- From C1: frame-local `skip-group` via a `let mut` that resets on frame return
  (no depth bookkeeping) — simpler, more idiomatic.
- From C3: consider explicit wire statuses (`stopped`, `dry-run`) over fields on
  `complete`, so an agent need not peek at a field to know the interview stopped.
- Do not graft: C1's split interface, C3's four-arm enum, or C4's internal
  `Flow`/`Act` enums (the judge preferred C2's single `Directive`).

## Judge ranking on interface depth

`C2 >> C1 ≈ C4 > C3`.
