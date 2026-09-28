# Rationale — unified control-flow value

## Problem

A confirm answer must be able to map to a control action — stop, dry-run, or skip —
and carry that decision through the interview, the plan/apply pipeline, and all five
drivers, while ordinary confirms keep pure boolean behavior and actions stay declared
data. The shape is non-obvious because the engine has **no** control-flow surface
today where an answer's *value* alters execution (`grounding: Scope`); the only
result surface is `enum Interview { Asking, Complete }` produced by the sole
constructor `advance` (`interview.rs:1276`), and every driver, the closed wire
protocol (`status ∈ {questions, complete}`, `additionalProperties: false`), and the
closed `question` schema depend on that shape. The design must also compose with the
approved error-attribution predecessor (1072): the atomic answer transaction (a
rejected answer never enters probe state, a rejected document never persists), the
`SkipDisposition` semantics, and the **origin-bearing consuming `Resolution`** for
configured defaults — depending on the approved *contract*, not the unmerged 1056
runtime, and never flattening or reconstructing configured origins. Everything must
stay replay-deterministic from stored submissions with no extra persisted control
state (`staging.rs`).

## Usage (caller's view)

A template opts a confirm into an action with an additive `on` map
(`false: stop`, `true: dry-run`, `false: { skip: group }`); a confirm without `on`
is unchanged. The action lives on the value the driver returns, so every caller sees
it the same way — four arms to match: `Asking`, `Complete`, `Stopped`, `DryRun`. The
crate caller matches the return of `drive`; the headless caller matches `Headless`
and emits a `complete` / `dry-run` / `stopped` wire document; the staged caller
replays submissions and gets the terminal arm derived, never stored; the CLI routes
the same four arms and joins the confirm dry-run to the existing `--dry-run` flag at
one apply gate. Full call sites for drivers A–D are in `design.md §1`.

## Shape

**One derived value at one funnel.** `directive_of(q, &Answer::Bool) -> Directive`
is a pure function computed at the single walk branch that has both the confirm node
and its committed boolean (`interview.rs:1058`, and the held-accept branch on the
first walk that records an early answer). `Directive` is internal and never
persisted; `Advance::fire` applies it — `Stop` sets `stopped` and unwinds, `DryRun`
sets `mode`, `Skip` sets the frame's skip. `advance` classifies the terminal arm from
those flags. Load-bearing decisions:

- **The outcome enum is the interface (deep, small surface).** Widening `Interview`
  to four arms hides the entire mechanism — the funnel, directive folding, scope
  skipping, atomic composition — behind a value every driver already matches.
  Drivers, protocol, and staging become thin adapters that route arms and
  re-implement no policy (`per boundary-discipline`, task discipline). No wire or
  transport type reaches the public surface; the protocol documents are produced by
  mapping the arms.
- **Skip reuses the existing skip machinery, verbatim.** `skip: group` is the local
  `skip` of the current `walk` frame (which, by recursion, *is* the enclosing group;
  at top level it is the interview); `skip: interview` is a single non-clearing
  field. Both drive the existing `Advance::skip` path, so a confirm-skipped question
  records the same default/empty answer and the same "answer was not used" warning as
  a `when`-skip — no new rule (`per encode-lessons-in-structure`; grounding Avoid).
- **Determinism by derivation, not storage.** The directive is recomputed from
  committed answers on every walk, so replay reproduces the terminal arm with zero
  persisted control state (`per make-operations-idempotent`; single source of truth).
- **Atomicity by structure.** `fire` reads only `self.answers`, which holds only
  committed values; a rejected batch returns before the advance runs; a rejected
  document discards the tentative advance. The invariant is enforced by placement,
  not a runtime guard (`per boundary-discipline`).

Validation lives at load time (schema + `problem`s naming the corrective form).
Invariants encoded in types: an action is reachable only via `ConfirmActions` on
`Confirm`; `SkipScope` makes the two scopes exhaustive; `Stopped`/`DryRun` make the
terminal control outcomes unignorable at every match site. What the system
deliberately does **not** do: infer actions from prompt text; persist control state;
add a second target normalizer; reconstruct configured origins; invent a skip/default
rule; or change the atomic transaction.

**Interface depth.** Public surface added: one declaration struct pair
(`ConfirmActions`/`ConfirmAction`/`SkipScope`), two enum arms, one `Stopped` struct,
two protocol document builders. Hidden behind it: the funnel, directive derivation
and folding, scope-skip via frame recursion, terminal classification, and the whole
atomic-composition argument. The surface is no larger than "declare an action" +
"observe the outcome"; all policy is in the callee.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept **widening `Interview` to four arms** (touching every `Asking | Complete`
  match site) in exchange for a single value that carries the decision to every
  driver with no side channel and no `main.rs`-only glue.
- We accept **`DryRun(Completed)` looking near-identical to `Complete(Completed)`** in
  exchange for byte-equal plan output and a shared plan-build path; the difference is
  exactly apply suppression, gated once.
- We accept **recomputing the directive on every walk/replay** (a tiny per-node
  match) in exchange for zero persisted control state and free replay determinism.
- We accept **firing on a confirm answered by a configured default**, because the
  action derives from the committed value regardless of source; this is consistent,
  not an oversight.
- We accept **two new wire statuses on a closed schema** (additive, documented) in
  exchange for first-class, machine-readable stop/dry-run outcomes rather than an
  overloaded `complete` document.

## Alternatives considered

- **`Directive` returned *beside* the interview (a tuple/`(Interview, Directive)`).**
  Rejected: it splits one fact across two values, so every driver must remember to
  read the second, and the "no rejected answer fires" invariant would need a runtime
  guard rather than falling out of placement. Exposes more to callers, hides less.
- **A flag on `Completed` (`disposition: Apply | DryRun | Stopped`) with the enum
  unchanged.** Shallower surface, but `Stopped` is not a completion (no full answers,
  no plan), so it would need a sentinel `Completed`, and drivers could ignore the
  flag and apply anyway. The type would permit the misuse the feature must forbid;
  loses on interface depth.
- **Inferring the action in each driver from the boolean answer + template lookup.**
  Rejected outright: re-implements policy in five places, invites drift, and violates
  "put the decision where it is seen once." Maximal exposed complexity.
- **A parallel control-flow walker separate from the interview walk.** Rejected: a
  second reachability/skip authority (grounding Avoid) that would duplicate
  `when`/default/skip semantics and desynchronize from the atomic transaction.

## Open questions and risks

- Should a confirm answered by a **configured default** be allowed to fire a
  terminal action (stop/dry-run), or should defaulted values be inert for actions so
  a preset cannot silently stop an interview?
- For the `stopped` wire document, is a **partial `answers`** map (answers up to the
  stop) the right contract for agents, or should it carry no answers to avoid
  implying a usable result?
- Should `skip: group` fired at **interview top level** be a load-time warning (since
  it equals `skip: interview` there and may signal author confusion), or silently
  equivalent as designed?
- Sequencing risk vs 1056: confirm-flow references approved-but-unmerged 1072 types.
  The design couples only to the atomicity invariant and the target-identity type so
  it is order-independent — is landing 1062 before 1056 acceptable to confirm at
  Phase C?
- Wire-contract risk: adding `stopped`/`dry-run` statuses to an `additionalProperties:
  false` protocol is a change agents depend on; confirm the additive rollout and
  documentation are sufficient.

## Next implementation step

Add `ConfirmActions`/`ConfirmAction`/`SkipScope` to `template.rs`, the `on` parser
and the "on only on confirm" load rule, then `directive_of` + the widened
`Interview` arms in `interview.rs` — proving behaviors 1–9 before touching any driver.
