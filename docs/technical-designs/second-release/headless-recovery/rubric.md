---
relationships:
  depends-on: headless-recovery
  informs: headless-recovery
---

# Headless recovery arena rubric

This rubric was authored after both blind candidate processes were live. Candidates did not receive it. Each criterion is scored from 0 to 5 for a total of 25.

## R1 — Product answers and output compatibility

The candidate gives an unambiguous recommendation for each of the three questions and shows concrete commands. It preserves exit 4 and byte-valid JSON for non-terminal consumers, scopes any terminal summary so explicit machine-output routes are not changed, and keeps wrong-template and complete-state behavior intelligible.

## R2 — Deep ownership seam

The candidate puts recovery policy at one real CLI seam with a small interface. It removes semantic drift between path-only and matching-template apply without adding a public library abstraction, pass-through layer, second parser/walker, target normalizer, provenance source, or persisted derived state. Call chains and module knowledge remain local.

## R3 — Transaction, persistence, and effect ordering

The candidate carries the opaque `CanonicalTarget`, origin-bearing `Resolution`, accepted submissions, and answer results without reconstruction. It specifies state for accepted, rejected, partial, complete, dry-run, trust, conflict, plan, apply, hook, and removal failures. No record mutation occurs before the answer transaction commits; successful staged apply removes state only after apply succeeds.

## R4 — Accepted flow composition and route equivalence

The design handles `Pending`, `Completed(Proceed)`, `Completed(DryRun)`, `Ended(Stop)`, and `Ended(Abort)` across direct, staged, headless, and terminal routes. It treats confirm answers as ordinary values that can trigger later flow nodes, not actions. It proves both accepted apply spellings have equivalent observable results for the same staged identity and answers.

## R5 — Implementability and proof quality

Usage, types, signatures, module map, and behavior matrix agree. Error/results contracts are exhaustive, canonical document and guide impact is explicit, and each load-bearing claim has a falsifiable command-level test or sole-kill guard. The design identifies real tradeoffs and avoids hypothetical seams, new permissions/access, timeouts, pinned checks, and application subprocesses.
