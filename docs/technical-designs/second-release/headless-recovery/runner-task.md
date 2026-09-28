---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Candidate task: recover from incomplete headless answers

Design a coherent CLI recovery shape for three product questions:

1. Whether `toha apply PATH --answers FILE` continues the staged interview at `PATH` and applies when the supplied answers complete it.
2. Whether an exit-4 question batch shown directly on a terminal becomes a short missing-answer and next-command summary, while scripts and agents retain the current JSON contract.
3. Whether `toha apply TEMPLATE PATH --answers FILE` resumes an incomplete staged interview when `TEMPLATE` has the staged formal identity.

The present implementation already does item 3, refuses item 1, always serializes exit-4 documents as JSON, and prints next-command guidance on stderr. The design may preserve or change each behavior, but it must recommend concrete answers and specify the consequences.

Produce one candidate package with caller usage first, two or three concrete command examples, core data and result types, function signatures, a module/seam diagram, persistence and error behavior, compatibility, a behavior matrix, falsifiable tests, tradeoffs, alternatives, risks, and the first implementation step. Keep pseudocode and `unimplemented!()` bodies in the design only.

Honor the supplied grounding and these constraints:

- Exit 4 and non-terminal JSON remain available to scripts and agents.
- Use the existing answers parser, pure interview engine, protocol answer transaction, origin-bearing `Resolution`, opaque `CanonicalTarget`, and sole `canonical_target` factory.
- Accepted submissions persist only after the engine transaction commits. Rejected documents and CLI dry-runs do not mutate staged state.
- A successful staged apply removes the record only after apply succeeds.
- Compose with the accepted future flow outcomes: `Pending`; `Completed(Proceed)`; `Completed(DryRun)`; `Ended(Stop)`; `Ended(Abort)`. An ordinary confirm remains a boolean term used by a flow node's `when`; it is not an action.
- Stop writes nothing and preserves staged state. Abort writes nothing and removes the current staged record through `Store::remove(&CanonicalTarget)`, surfacing removal failure. Dry-run plans and writes nothing. Skip follows the existing interview rules.
- Preserve wrong-template, stale-source, replay, trust, conflict, parse, evaluation, and staging I/O failures.
- Do not introduce a new public crate interface for a CLI-only concern, a second normalizer, a second provenance source, a second walker/parser, persisted derived state, permissions/access rules, timeouts, pinned-version checks, or application subprocesses.
- Proposed canonical specification and guide changes belong to the paired implementation. Do not edit runtime or shared specification files.

Choose a whole architecture independently. Prefer a deep policy-owning module or function with a small interface and short caller chains. Do not add a hypothetical seam where behavior does not vary. Your output is evaluated against undisclosed criteria, so derive priorities only from this task and grounding.
