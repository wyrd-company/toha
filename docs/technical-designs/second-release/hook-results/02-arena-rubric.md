# Arena rubric — opt-in Jinja hook results

**Withheld from candidate runners.** Used only for cross-judge and own scoring
after candidates finish writing. Six criteria, derived from the task's
observable outcomes and invariants. Score each 1–5.

## Criteria

1. **Composition fidelity (weight 3).** Honors every load-bearing predecessor
   contract without violating one: 1075 — static reference analysis stays
   pre-admission over the complete retained program, engine performs no ambient
   read, no new environment surface, seventeen reserved names + collision rule
   untouched, `CanonicalTarget`/`StagingError` producer seam, configured
   `Resolution` vs flat `Seed`, record-owned replay with no live read, staged
   `apply --trust` stays hook-only; 1076 — includes inert and file-body-only,
   hook fields not an include surface, capability-by-type; 1077 — hooks only at
   the write/hook site, stop/abort unplannable, dry-run suppresses the site;
   1069 — parsed-node digest coverage. A single violated load-bearing sentence
   caps this at 2.

2. **Phase model + availability guarantee (weight 3).** The phase/ordering model
   is explicit and **statically guarantees a referenced result exists before it
   is read** (a later surface may reference only results of hooks ordered
   earlier), is deterministic and replay-stable, and bounds readable surfaces
   precisely to later hooks' fields + after-apply. Files, before-apply, and
   interview fields are excluded by construction.

3. **Opt-in + trust preservation (weight 3).** Exposure is strictly opt-in;
   existing hooks (no id, no capture) behave byte-identically; the default
   stop-and-fail is preserved and any tolerance of nonzero is explicit and
   opt-in with no silent weakening; stdout/stderr never reach generated files or
   public output silently; the 1069 coverage decision is stated and a guard test
   is required; introduces no new trust/permission/timeout/pinned/subprocess
   policy.

4. **Failure + edge completeness (weight 2).** Falsifiable behavior for: nonzero
   interaction, absent/unrun/skipped result (`when` false, empty `each`, prior
   stop), duplicate ids (load error), non-UTF-8 decode rule, trailing newline,
   dry-run (no results), stop/abort suppression.

5. **Interface depth (weight 2).** Ousterhout depth: a small public surface hides
   the interleave/defer complexity; no shallow module, information leakage,
   temporal decomposition, or pass-through methods; error modes, invariants,
   ordering, and configuration are in the interface; tested through the same
   interface callers use; prefers returned results / dependency acceptance over
   hidden side effects; no hypothetical seam without real variation.

6. **Compatibility + canonical impact (weight 1).** Existing templates and crate
   callers unchanged; supported terminal/headless/staged/direct/crate paths
   preserved; canonical specification/schema/guide/example edits are described
   (owned by impl 1063), not applied here.

## Red-flag screen (reject/revise before scoring)

- Shallow module (thin wrapper over the runner or the plan).
- Information leakage (the same ordering rule duplicated across engine, plan,
  and apply).
- Temporal decomposition (modules named for phases with no encapsulated state).
- Pass-through methods (a method that only forwards to the runner or map).
- A hypothetical seam (a phase or capture mode with no real variation).
