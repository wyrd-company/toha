# Confirm-flow arena round 2 — common runner task (the `flow` node shape)

You are producing **one candidate design** for a Toha (Rust scaffolding tool)
feature. Read `grounding.md` (shared engine facts) and `grounding-flow.md` (the
round‑2 re‑ground for this shape) in full first. Follow the architect runner
discipline in `runner-prompt.md` and shape your package per `rationale-template.md`.
Do not edit production code or specs; keep every sketch inside your own output
directory.

## The direction (fixed for this round)

The manager asked: *instead of adding an action to `confirm` types, use a new
`flow` block with a `when` expression and an action enum.* Your candidate develops
that shape: a standalone interview node that carries a `when` condition and a
control action, decoupled from any confirm answer. The `confirm` question type must
stay **completely unchanged** (ordinary confirms are pure boolean by construction).

## The problem to solve

Design the `flow` node end to end:

1. **Declaration syntax.** How a `flow` node is written in the `interview` list —
   its `when` expression (reusing the existing `when` construct) and its action
   enum. Actions must cover **stop**, **dry-run**, and **skip** with scope **whole
   interview** vs **current group**. Decide whether `when` is optional (a `when`‑less
   flow always fires), whether the node needs an optional label (it has no id), and
   the exact spelling of the action enum (including how `skip` carries its scope).
   Actions are declared data; nothing is inferred from prompt text.

2. **Ownership and evaluation.** The `flow` node is a peer of message/hook/computed
   nodes — no id, records no answer. Specify how it is parsed (a new `Node` kind),
   how the walk evaluates it (fire once per position via `visited`; readiness‑gate on
   its `when` references; never held in a batch; skipped under an ancestor skip), and
   where the single decision is made.

3. **Outcomes (must be preserved exactly).** Stop = a terminal result that writes no
   files and runs no hooks and cannot be planned. Dry-run = normal planning with
   apply suppressed, distinct from and composing with the `apply --dry-run` CLI flag.
   Skip = whole interview or current group, nested, recording the existing
   default/empty values and the existing "answer not used" warning. Show these across
   the terminal, answers‑file (wire protocol), staged, direct, and library drivers,
   and how the closed protocol (`status ∈ {questions, complete}`,
   `additionalProperties:false`) is extended additively.

4. **Composition and determinism.** Compose with `when`, defaults, early/held
   answers, and nested groups; a flow node under a false `when` or an ancestor skip
   does not fire; the decision is computed from committed answers only (a rejected
   document fires nothing and never persists); everything is replay‑deterministic
   from stored submissions with no new persisted state. Compose with the approved
   error‑attribution (1072) contract (atomicity invariant, target‑identity type, and
   the origin‑bearing consuming `Resolution`; see grounding) — depend on the
   contract, not the unmerged 1056 runtime. Runtime integration order (after 1056) is
   fixed coordination, not a design choice.

5. **Interfaces, state, tests, and the comparison.** Specify the result/state/
   interface changes, the proposed schema/protocol/guide edits (described, owned by
   the paired implementation), and falsifiable test scenarios for both branches of a
   condition, nesting, resume/replay, and every driver, including driver‑parity and
   negative (rejection) cases. Include a short, honest **comparison** of your `flow`
   shape against the action‑bearing‑confirm alternative summarized in
   `grounding-flow.md`: where the flow node is better (confirm untouched, reuses
   `when`, more expressive) and where it is worse (the trigger is one indirection
   from the question).

## Discipline reminders

- Caller usage first (README + two or three real call sites across different
  drivers); derive types from it. Data structures first.
- Prefer a small, deep interface: put the decision where the flow node is evaluated
  once; keep drivers, protocol, and staging thin adapters that route the outcome.
- Encode invariants in types; validate at the load boundary (a `skip: group` outside
  a group is a load error naming the corrective form; an unknown action is a load
  error).
- Do not add a second target normalizer, reconstruct configured origins, accept a
  rejected answer into probe state, infer a different skip/default policy, or cut any
  existing outcome/route.
- Produce the strongest version of this shape your model can make; do not hedge
  toward the round‑1 shape. Divergence from the other round‑2 runners is the signal.
