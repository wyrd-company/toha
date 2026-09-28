---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Arena scoring — architect

The architect read every candidate design and rationale end to end before
reading the cross-judge verdict. Scores use the withheld rubric, from 1 (does
not satisfy) to 5 (strong and falsifiable).

Candidate 2 is preserved as supplemental evidence. It is ineligible as one of
the two required blind candidates because its runner opened `rubric.md` while
exploring the worktree. Candidate 1 and replacement Candidate 3 are the two
blind candidates.

| Criterion                                     | Candidate 1 | Candidate 3 | Candidate 2, contaminated |
| --------------------------------------------- | ----------: | ----------: | ------------------------: |
| C1 Exact, local fault attribution             |           4 |           4 |                         5 |
| C2 Configured-default provenance and recovery |           2 |           4 |                         2 |
| C3 Early-answer policy and atomicity          |           4 |           5 |                         5 |
| C4 One canonical-target authority             |           5 |           3 |                         5 |
| C5 Parity, replay, and compatibility proofs   |           5 |           5 |                         5 |
| C6 Deep modules and bounded change            |           3 |           4 |                         3 |
| **Total**                                     |   **23/30** |   **25/30** |     **25/30, ineligible** |

## Candidate 1

- **C1 — 4.** One `TemplateFault` gives file rules and defaults an exact field,
  optional source, and message. The plan retains the support path. The proposed
  shared public value is broader than the text contract needs.
- **C2 — 2.** The candidate requires the prerequisite to replace the approved
  flat seed map with an origin-bearing map. Final prerequisite revision
  `4738042` does not do that. The design correctly rejects path reconstruction
  and parallel caller-managed maps, but its required seam does not exist.
- **C3 — 4.** The decision table, rejection ordering, warning timing, and
  discarded transition are explicit. Its probe says invalid current values
  remain held, which needs a stronger guard so rejected data cannot prove a
  skip.
- **C4 — 5.** An opaque `CanonicalTarget` makes unchecked construction
  impossible, keeps one filesystem normalization operation, and specifies a
  bounded legacy-key lookup.
- **C5 — 5.** The direct/staged/replay matrix, compile-fail checks, legacy
  identity, and exact error snapshots are falsifiable.
- **C6 — 3.** Replacing public target parameters, replacing `Seed.defaults`,
  moving hook faults, and adding a shared public fault type create more
  migration than the six behavior changes require.

Red-flag screen: the canonical-target wrapper is deep, but the
configured-default assumption blocks implementation. The shared fault type risks
information leakage between interview and planning. The full-engine probe is
viable only if unavailable rejected answers are excluded from its evaluation
context.

## Candidate 3

- **C1 — 4.** Module-local fault values share one crate-private formatter. This
  retains locality and exact text without exposing plan paths to interview code.
  The proposed `target path` field must be reconciled with the final canonical
  wording.
- **C2 — 4.** `ConfigEntry` remains the only origin source, and an opaque
  resolved bank prevents value/origin drift. The candidate replaces the
  prerequisite's public `Resolution.defaults` with a consuming `Resolution`, so
  synthesis must preserve the approved flat field and add the provenance
  capability explicitly.
- **C3 — 5.** `SubmissionTxn` distinguishes valid and unavailable current
  answers, shares the walker rather than adding a skip oracle, and has one
  commit point. The two product policies and warning timing are exact.
- **C4 — 3.** The existing `PathBuf` return minimizes public churn and fixes the
  empty suffix at the authority. It relies on an audited command boundary and
  does not prevent a crate caller from passing an unnormalized path to planning.
- **C5 — 5.** Eighteen scenarios cover exact faults, configured recovery, early
  classification, apply/continue equivalence, replay, both target shapes, legacy
  state, and a duplicate-normalizer audit.
- **C6 — 4.** Private `DefaultBank`, `PreparedDefault`, module-local faults, and
  the submission transaction are cohesive. `Resolution::start`,
  `replay_with_resolution`, and `check_default` need simplification so they do
  not become parallel entry paths or pass-through methods.

Red-flag screen: this is the best bounded base. The resolved bank is deep, but
changing the prerequisite's public `Resolution` would leak this design backward.
The terminal-only `check_default` seam is avoidable if invalid configured
defaults are removed from `Prompt.default` and carried as batch errors before
prompting.

## Candidate 2 — supplemental only

Candidate 2 is close to Candidate 1 and benefited from seeing the rubric. Its
strongest material is the rule that a probe uses only accepted state and
successfully checked current answers, plus its clear terminal/headless recovery
examples. It has the same unavailable prerequisite carrier as Candidate 1 and
adds wide public type changes. Its proposal to validate defaults for skipped
questions expands behavior beyond the observed active-prompt defect and is not
grafted.

## Architect base choice

Candidate 3 is the base because it keeps policy local and distinguishes rejected
current values during early classification. Synthesis must graft Candidate 1's
invalid-configured-default removal from `Prompt.default`, exact legacy-key
compatibility, and stronger canonical-target construction only where the public
surface earns it. Candidate 2 contributes the successful-current-only probe rule
and exact two-file provenance examples. The final design must preserve the final
prerequisite's flat public seed contract and present its provenance extension as
a separate approval item.
