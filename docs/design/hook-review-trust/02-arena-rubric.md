# Arena: runners, directions, and scoring rubric

Phase B of the architect workflow for task 1069. Candidates receive the common
task and the grounding, **not** this rubric (per the arena method). The rubric
drives cross-judgment and synthesis.

## Runners and structural directions

Default slots are four `inherit-parent`. Three viable, structurally distinct
directions were dispatched (opus subagents), which exceeds the "at least two
structurally distinct" bar and leaves margin for a dropout. Isolated output
paths: `/tmp/arena-hook-review/candidate-<n>/design.md` (no shared writable
path).

| # | Direction | Distinct on the "identity bound to approval" axis |
|---|-----------|---------------------------------------------------|
| 1 | Content-addressed digest **on the registry entry** | digest of executable surface, co-located with install metadata |
| 2 | Approved-digest **ledger decoupled** from the entry | set of approved digests per identity, trust state separated from install state |
| 3 | **Commit-bound** trust + executable-change fallback | leans on the git commit Toha already records; content signature only for non-git |

Cross-judge: a read-only judge (prefer a different model family where the pool
allows) scores every candidate against every criterion after candidates finish;
the architect also reads and scores each candidate independently and reconciles.

## Scoring criteria (derived from this task's observable outcomes and invariants)

Each scored 1-5 with evidence. Derived from the DoD and the grounding's
Preserve/Change constraints.

1. **Invalidation precision.** Trust survives a doc-only or unrelated update and
   breaks exactly when the executable input changes. Penalize over-invalidation
   (re-review on every commit) and under-invalidation (a changed `script:` body
   or hook line that keeps trust). Falsifiable: enumerate {doc-only change,
   hook-line change, script-byte change, no change} and check the outcome.

2. **Capability preservation.** All existing adapters (interactive, headless
   `--answers`, staged resume, crate) and the headless `add --trust`/`--trust`
   grant flows keep working with no new interactive gate a script cannot pass.
   Any narrowing is surfaced as a Bob decision, not silently baked in.

3. **Engine purity / UI-free.** The reviewable surface is a pure library-
   computed value; adapters present. Nothing pushes UI, I/O, or prompting into
   the interview engine; `HookRunner` remains the only port.

4. **Forward-compatibility with hook-result expansion (1078).** New hook-node
   fields are covered by review and by the bound identity automatically (e.g. a
   canonical serialization of the whole node), rather than an allow-list of
   fields the expansion must remember to update.

5. **Interface depth & locality.** Trust/review is a deep module with a small
   surface; trust logic does not leak across registry/plan/apply/cli. No wire/
   storage types on the public surface. Judge per design-red-flags (shallow
   module, information leakage, temporal decomposition, pass-through).

6. **Contract & migration surface.** Minimal, well-defined registry-schema and
   CLI-spec edits; backward-compatible migration of existing `trusted: bool`
   entries; the `templates update` contract change is precise. Trust-management
   *command syntax* is left to design 1071 (not settled here).

## Screening (design red flags), applied before scoring

Each candidate is screened for: shallow module, information leakage (trust
representation appearing in registry + apply + cli), temporal decomposition
(load/verify/apply stages each re-deriving the surface), and pass-through
methods. Offenders are revised or rejected before scoring.
