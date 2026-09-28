# Rationale — standalone `flow` node

## Problem

Toha maps an interview to a scaffold with no surface where an answer's *value*
alters execution. Bob asked for stop / dry-run / skip control decoupled from the
`confirm` type: "instead of adding an action to `confirm`, use a new `flow` block
with a `when` expression and an action enum." The shape is non-obvious because
the outcomes must survive five drivers and a closed wire protocol
(`status ∈ {questions, complete}`, `additionalProperties: false`), stay
replay-deterministic with no new persisted state, and compose with the approved
1072 contract (answer-transaction atomicity, target-identity type, origin-bearing
`Resolution`) without depending on unmerged 1056 runtime. The engine already owns
the pieces — `when`, the node walk, `visited`, the skip flag/`Skipped` map, the
tentative-advance atomicity — so the real design question is how small a surface
can carry the new control without duplicating any of them.

## Usage (caller's view)

See `design.md` — README "Flow nodes" section plus three call sites (library,
headless/wire, terminal). The authored form is `flow: <action>` with an optional
`when` sibling and optional `label`, mirroring `message: <text>` + `when`.
Callers observe the outcome as a third `Interview` variant (`Ended`) and a
`Disposition` field on `Completed`; drivers are thin routers over those two.

## Shape

A `Node::Flow(FlowNode { when, label, action })` peer to message/hook/computed;
`FlowAction = Stop | DryRun | Skip(SkipScope)`, `SkipScope = Rest | Group`. The
single decision is one arm of `Advance::walk` that mirrors the message/hook arm:
fire once via `visited`, readiness-gate on `when` refs, inert under an ancestor
skip, evaluate `when` (absent ⇒ fires), then apply the effect. `stop` returns a
new `Walk::Ended` that unwinds to `advance`, producing `Interview::Ended` (which
`Plan::build` cannot consume — "cannot be planned" is a type fact, `per
encode-lessons-in-structure`). `dry-run` sets a `Disposition` threaded like
`messages`/`hooks` into `Completed`, OR-composed with `--dry-run` at the apply
boundary. `skip` flips a mutable local `skip` for the rest of the frame
(`group`) and, for `rest`, returns `Walk::SkippedRest` that climbs one frame per
return — reusing `Advance::skip`, `Skipped`, and `warn_if_held` verbatim.
Validation lives at the load boundary (`per boundary-discipline`): unknown action
and top-level `skip: group` are load errors naming the corrective form; the
`FlowAction`/`SkipScope` enums make invalid actions unrepresentable inside the
engine. No new persisted state: the decision is recomputed from committed answers
on every walk and every replay (`per make-operations-idempotent`). Interface
depth is high — a handful of variants/fields hide readiness sequencing, nested
skip propagation, atomicity, and replay recomputation; no wire type reaches the
public surface.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept a one-node indirection between a confirm and its control (two nodes
  for "no → stop") in exchange for leaving `QuestionKind::Confirm` completely
  untouched and gaining triggers over any expression.
- We accept widening the walk's control return from `bool` to a four-variant
  `Walk` enum in exchange for making stop / block / skip-rest / complete explicit
  and correctly unwound, rather than overloading `bool` with a side flag.
- We accept `stop` as a new `Interview` variant (touching every `Asking |
  Complete` match site) in exchange for making "cannot be planned" unrepresentable
  rather than a runtime guard.
- We accept parsing `when` as a node-level sibling of `flow:` (not nested under
  it, as the round-2 sketch showed) in exchange for reusing the existing
  node-level `when` extraction and walk gating verbatim — the tightest mirror of
  the message/hook arm.
- We accept `dry-run` as a `Completed` field rather than a variant, because a
  dry-run is a normal completion that still plans; a variant would fork every
  plan call site for no gain.

## Alternatives considered

- **Action on the `confirm` type (round-1 shape).** Co-locates intent on the
  question and is marginally more obvious for the simplest case, but changes the
  confirm type (regression surface, guarded field), adds an answer→decision
  funnel at the boolean choke point, and binds the trigger to one boolean. Lost
  on interface cleanliness and expressiveness; Bob's "instead" rules it out as
  the mechanism.
- **`flow` with a nested `action:` and nested `when:` object** (the round-2
  sketch's exact spelling). Reads as one self-contained object, but does not
  reuse the node-level `when` extraction and adds a redundant `action:` key when
  the `flow:` key can carry the action directly (as `message:` carries text).
  Lost on minimality.
- **A `stop` flag on `Completed` instead of an `Ended` variant.** Smaller diff,
  but `Plan::build` would then accept a "completed" that must not be planned,
  pushing the invariant into a runtime check at every call site. Lost on
  encode-in-types.

## Open questions and risks

- Should a top-level `skip: group` be a **load error** (this design, safer,
  encodes the invariant) or silently mean `rest` (the grounding-flow note's
  "group at top level == rest")? The two inputs point different ways; the load
  error loses nothing because the runtime equivalence is exactly why authoring it
  is redundant — is that the intended reading?
- What **exit code** should a `stop` produce on the CLI — `0` (a deliberate stop
  is success, this design's lean) or a distinct code so scripts can detect it?
- On a `stop` during interactive `stage`, should the staged record be **removed**
  (nothing to continue) or **kept** (replay reproduces `Ended`)? This design
  keeps it for determinism; is that the wanted lifecycle?
- Should the `ended` wire document carry the reached `messages` only, or also a
  machine-readable reason beyond the optional `label`?
- A `skip: rest` whose skipped questions have a `default` referencing a
  *later* still-unanswered question will block (the pre-existing skip readiness
  edge). Acceptable to inherit, or worth a load-time lint?

## Next implementation step

Add `Node::Flow(FlowNode)`, `FlowAction`, `SkipScope` to `template.rs` with the
builder arm and the two load rules, so the parser and `collect_answers`/
`expressions` compile before the walk arm and `Interview::Ended` are wired.
