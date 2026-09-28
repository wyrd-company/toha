---
relationships:
  depends-on:
    - headless-recovery
    - candidate-1
    - candidate-2
  informs: headless-recovery
---

# Claude cross-judge: headless recovery arena

The read-only cross-judge ran as Claude session `84fdcd73-606e-4993-9181-6014e60caf9c`. Its working directory contained copies of the grounding, rubric, and both terminal candidate artifacts after both candidates completed. The process had normal host filesystem access rather than enforced read isolation; its terminal report states that it read those four inputs end to end and reports no outside evidence. It found no rubric leakage, candidate contamination, invented product history, or identifying sample values.

## Scores

| Criterion | Candidate 1 | Candidate 2 |
|---|---:|---:|
| R1 — Product answers and output compatibility | 5 | 4 |
| R2 — Deep ownership seam | 4 | 5 |
| R3 — Transaction, persistence, and effect ordering | 5 | 4 |
| R4 — Accepted flow composition and route equivalence | 4 | 5 |
| R5 — Implementability and proof quality | 5 | 5 |
| **Total** | **23** | **23** |

The judge broke the tie for candidate 2. It valued candidate 2's single command interface, shared replay and commit policy for continue plus both apply spellings, and `StagedOutcome` type that exposes a plannable value only for completion. It judged those structural strengths harder to retrofit than candidate 2's point defects.

The judge required two corrections to candidate 2:

1. Use candidate 1's preview semantics so CLI `--dry-run` performs no staged save or removal, including a committed abort result.
2. Consider candidate 1's proposed `--json` override for pseudoterminal machine consumers.

It rejected candidate 1's separate persistence policy for continue, candidate 2's abort mutation during CLI dry-run, continue-then-apply sequencing, always-JSON terminal output, `--output human|json`, and shallow match-arm helpers.

## Red-flag screen

The judge passed both candidates for information leakage and temporal decomposition. It considered candidate 1's recovery and presentation modules deep, with a minor split in post-apply cleanup ownership. It considered candidate 2's one-method command module deeper and its `commit_staged` a real policy owner rather than a pass-through.

## Evidence gap identified by the judge

Both candidates correctly preserved the grounding's two source gaps: repository history gives no product reason for the path-only refusal, and no prior decision governs terminal exit-4 presentation. The judge treated both as decisions for the checkpoint.
