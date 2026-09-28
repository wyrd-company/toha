# Cross-judge report (read-only, different model family)

A read-only judge from a different model family scored every candidate against
every rubric criterion after the candidates finished writing. It made no repo
changes. Its scores and verdict are carried into `../04-synthesis.md` (score set
A) and reconciled there against the architect's independent read (score set B).

## Scores

| Criterion | C1 | C2 | C3 |
|-----------|----|----|----|
| 1. Invalidation precision | 5 | 5 | 5 |
| 2. Capability preservation | 4 | 5 | 5 |
| 3. Engine purity / UI-free | 5 | 5 | 5 |
| 4. Forward-compat (1078 fields) | 5 | 5 | 3 |
| 5. Interface depth & locality | 4 | 5 | 4 |
| 6. Contract & migration surface | 4 | 5 | 2 |
| **Total** | **27** | **29** | **23** |

## Verdict

Recommended base: Candidate 2 (decoupled ledger), for cleanest module
boundaries, an explicit narrowing decision, and a complete legacy migration
path. It flagged that all three enforce the same load-bearing narrowing — trust
no longer forwards through an update that changes the executable surface — and
noted only Candidates 2 and 3 ask the human to approve it while Candidate 1
assumes it.

## Grounding-alignment flags raised

- Candidate 1: trust stored on the registry entry (co-location) and the narrowing
  not surfaced as an explicit decision.
- Candidate 3: temporal decomposition in `update`; a future hook field that
  references a new file is not auto-covered; no explicit legacy migration path.

The reconciliation in `../04-synthesis.md` accepts the locality critique of
Candidate 1 (fixed by grafting a single trust chokepoint), accepts the narrowing
critique (surfaced as a Bob decision), and down-weights the migration critique
for a pre-v1 tool with no users.
