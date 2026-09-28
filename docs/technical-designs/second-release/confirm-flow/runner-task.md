# Confirm-flow arena — common runner task

You are producing **one candidate design** for a Toha feature. Read the
grounding artifact in full first: `../confirm-flow/grounding.md` (provided to you
by path). Follow the architect runner discipline you were given
(`runner-prompt.md`) and shape your package per `rationale-template.md`.

Toha is a Rust scaffolding tool and crate: it interviews a caller and renders
files from a template. Work against the grounding evidence and the approved
predecessor contracts it records. Do **not** edit production code or specs; keep
every sketch, pseudocode, and `not implemented` body inside your candidate design
artifacts in your own output directory.

## The artifact you produce

Two files in your assigned output directory:

- `design.md` — the caller's usage first (a README-style view plus two or three
  concrete call sites across different drivers), then data/type sketches,
  function signatures, a module/seam diagram, the error/results contract, the
  proposed canonical-document (spec/schema/guide) edits described (not applied),
  and the behaviors to prove.
- `rationale.md` — shaped per `rationale-template.md` (Problem, Usage, Shape,
  Tradeoffs, Alternatives considered, Open questions and risks, Next step). Name
  the alternatives you rejected and why.

## The problem to solve

Let a template map a **confirm** question's answer to a control **action**, and
carry that decision through the interview and apply pipeline and every driver:

1. **Declaration syntax.** Define how a template declares that a specific confirm
   answer (true or false) triggers **stop**, **dry-run**, or **skip**. Ordinary
   confirm questions must keep pure boolean behavior and are unaffected unless an
   action is explicitly configured. Actions are declared data — never inferred
   from prompt text. Define which incompatible or ambiguous combinations are
   rejected, and when (prefer load time), with messages that name the corrective
   form. Keep additions additive to the closed `question` schema object.

2. **Stop and dry-run results.** Define the result of a **stop**: it must write
   no files and run no hooks, and be expressible through the terminal, headless
   (wire protocol), staged, direct, and crate callers. Define the **dry-run**
   result: normal planning (files and hooks computed and shown) with no writes
   and no hooks — and distinguish it from the existing `apply --dry-run` CLI flag
   and from ordinary planning. The wire protocol documents are closed
   (`additionalProperties: false`, `status` ∈ {questions, complete}); say how a
   new terminal outcome is represented.

3. **Skip scope.** Define **skip** scope: the entire remaining interview versus
   the current/enclosing group, composing with arbitrarily nested groups. Define
   exactly what answer each skipped question records, reusing the engine's
   existing default/empty and "answer was not used" warning semantics rather than
   inventing a new rule. Make the relationship to the existing `when`-based skip
   explicit and non-colliding.

4. **Composition and determinism.** Compose the action decision with `when`,
   per-question and configured `default`, early/held answers, and nested groups.
   Fire an action only after `when` (a skipped confirm cannot fire) and after the
   held-answer check. Respect the atomic answer transaction: a rejected answer
   never influences skip/reachability classification and a rejected document never
   persists. Keep everything replay-deterministic from stored submissions — persist
   no extra runtime control state. Compose with the approved error-attribution
   predecessor's five fixed policies (see grounding) and depend on that approved
   **contract**, not on its unmerged implementation; state the sequencing.

5. **Interfaces, state, tests.** Specify the result/state/interface changes, the
   proposed schema/protocol/guide edits (described, owned by the paired
   implementation), and falsifiable test scenarios for both confirm answers,
   nesting, resume/replay, and every driver, including driver-parity (byte-equal
   results) and negative (rejection) cases.

## Discipline reminders

- Caller usage first; derive types from it. Data structures first.
- Prefer a small, deep interface: put the decision where it is seen once (the
  engine funnel that has the question and the final boolean), and keep drivers,
  protocol, and staging as thin adapters that route the outcome without
  re-implementing policy.
- Encode invariants in types; validate at boundaries; single source of truth per
  invariant; idempotent/replayable transitions.
- Do not add a second target normalizer, reconstruct configured origins, accept a
  rejected answer into probe state, or infer a different skip/default policy.
- Produce the strongest design your model can make. Divergence from other
  candidates is the signal; do not hedge toward a safe middle.
