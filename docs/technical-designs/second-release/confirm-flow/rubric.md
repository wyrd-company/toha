# Confirm-flow arena rubric

Derived from task 1077's observable outcomes and invariants. This is the
picker's tool for cross-judge and own-scoring; **candidates do not receive it**
(blindness audit in the synthesis record). Six concrete, gradeable criteria.

## C1 — Confirm-action declaration syntax and validation

- A confirm question binds an action (**stop**, **dry-run**, **skip**) to a
  specific boolean answer; a confirm with no action stays ordinary boolean.
- Actions are declared data, never inferred from prompt text.
- Incompatible/ambiguous combinations are rejected at **load** with a message
  that names the corrective form: action on a non-confirm type; the same answer
  bound to two actions; a skip action lacking a resolvable scope; a stop and a
  continue-action on the same truth value.
- The schema addition is additive to the closed `question` object and adds an
  "action only on confirm" load rule beside the existing `loop`/`options` rules.
- Grade down: free-form action strings; prompt-text heuristics; a mapping that
  changes ordinary confirm answers; a schema change that is not additive.

## C2 — Stop / dry-run result model across all five drivers, no files or hooks

- Stop yields a terminal result that writes **no files** and runs **no hooks**,
  representable through terminal, headless (wire protocol), staged, direct, and
  crate callers.
- Dry-run yields the computed plan (files + hooks listed) with no writes/hooks,
  and is explicitly distinguished from (a) the existing `apply --dry-run` CLI
  flag and (b) normal planning.
- The new outcome is representable in the `additionalProperties: false` protocol
  documents (a new `status` value or a declared field, added to the schema) and
  in `Applied`/driver outcomes — not left to "the caller happens not to call
  apply".
- Grade down: a stop/dry-run that can leak a write or hook on any driver; an
  outcome only expressible in the CLI and not to headless/crate callers; reusing
  `apply --dry-run` semantics without disambiguation.

## C3 — Skip scope: whole-interview vs current-group, nested, defined values

- The skip action declares its scope: the entire remaining interview or the
  current/enclosing group; composes with arbitrarily nested groups.
- Defines exactly what answer each skipped question records, reusing the existing
  default/empty semantics and the `warn_unused` "answer was not used" contract —
  not a new skipped-default rule.
- The interaction with the existing `when`-based skip and its warning contract is
  explicit and non-colliding.
- Grade down: undefined skipped values; a skip that silently changes generated
  output; a new reachability rule that diverges from `skipped_descendants_ready`.

## C4 — Composition with predecessor policies, atomicity, replay determinism

- The action fires only after `when` (a skipped/inactive confirm cannot fire) and
  after the held-answer check.
- Respects the atomic answer transaction: a rejected answer never enters
  probe/skip classification; a rejected document never persists. Reuses the
  `unless_skipped`/`SkipDisposition` classification, not a parallel walker.
- Composes with the five fixed 1072 policies and their four "do not" constraints.
- Fully replay-deterministic from stored submissions; persists **no** extra
  runtime control state. Resume/replay of an accepted prefix equals uninterrupted
  execution.
- Depends on the approved 1072 **contract** (atomic transaction, `SkipDisposition`,
  opaque `CanonicalTarget` at identity consumers), not on 1056 runtime, and states
  the sequencing explicitly.
- Grade down: firing before `when`; persisting a control flag; a rejected answer
  influencing skip; assuming 1056 is merged; reconstructing configured origins.

## C5 — Interface depth and seam placement

- The action decision lives once in the interview engine — the sole funnel that
  sees the question node and the final `Answer::Bool` — surfaced through a small
  result-surface addition.
- Drivers, protocol, and staging adapters route/serialize the outcome without
  re-implementing policy. Short call chains; a small public surface hiding the
  control-flow complexity.
- Prefer dependency acceptance and returned results over hidden construction or
  side effects. No hypothetical seams without real variation.
- Grade down: action handling scattered across every driver; pass-through layers;
  policy duplicated in adapters; a wide public surface exposing internal stages.

## C6 — Falsifiable validation across both answers, nesting, resume, drivers

- Names concrete, failable scenarios: ordinary confirm unchanged; each action
  fires on its bound answer and not the other; stop/dry-run leave no files/hooks;
  skip scope (whole vs group, nested) records defined values with the correct
  warnings; byte-equal results across terminal / headless / staged-continue /
  direct / crate; replay of an accepted prefix equals an uninterrupted run;
  incompatible-combination load rejections; every emitted document still
  validates the protocol schema.
- Each behavior is stated as "how it could fail," not just "it works".
- Grade down: vague validation; missing driver-parity or resume scenarios; no
  negative (rejection) cases.
