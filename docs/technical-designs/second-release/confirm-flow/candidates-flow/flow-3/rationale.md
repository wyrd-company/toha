# Rationale — `flow` node + unified `Disposition`

## Problem

Toha must let a template steer the interview — stop before writing, plan without
applying (dry-run), or skip the rest of the interview or the current group — from
a condition over the answers, without inferring anything from prompt text. The
engine has no surface today where control depends on an answer's value, and the
shape is non-obvious because of hard constraints that crossed our boundary:
`confirm` must stay a pure boolean (`template.rs:114`); the interview must reach
every driver identically (terminal, headless/wire, staged, direct, library);
resume is full replay from `{submissions, identity, now}` with no new persisted
state (`staging.rs:20`); the wire protocol is closed (`status ∈ {questions,
complete}`, `additionalProperties:false`); a batch can hold a confirm and its
successors together (`grounding.md:190`); and the decision must compose with the
approved 1072 contract — atomic answer transaction, opaque target identity,
origin-bearing `Resolution` — depending on the contract, not on unmerged 1056
code.

## Usage (caller's view)

Authors write a `flow` node in the `interview` list: `flow:` holds the action
(`stop`, `dry-run`, or `{ skip: rest | group }`), with the existing `when` and an
optional `label` as sibling keys. Drivers and library callers read one new value
off the completed interview through one method — `completed.step()` returns
`Step::Stop { label }` or `Step::Plan { apply, label }` — and route accordingly;
the CLI `--dry-run` ANDs into `apply`. The wire `complete` document gains an
optional `disposition` (+`label`). Three real call sites (direct terminal apply,
headless `--answers`, and a library consumer) all funnel through the same
`step()` seam. Full README, call sites, types, signatures, seam diagram, and
contracts are in `design.md`.

## Shape

A `Node::Flow(FlowNode { action, label, when })` peer of message/hook/computed:
no id, no answer, so it never enters the `Answers` map or the id namespace and
needs no default/empty handling — `confirm` is untouched by construction. The
three effects collapse into **one** value, `Disposition` (`Proceed ⊑ DryRun ⊑
Stop`, carrying the label), raised monotonically inside the walk and carried on
the single `Completed`. Skip is orthogonal: it reuses the existing skip flag and
`Advance::skip` verbatim and never touches `Disposition`. The **decision has one
home** — a `Node::Flow` arm in `Advance::walk` that mirrors the message arm for
`visited`/readiness/`when`, folds two new skip flags (`skip_rest`, per-frame
`group_skip`) into the existing `skip`, and sets `halt` for stop. Invariants
encoded in types: declared actions (`FlowAction`/`SkipScope`), the ordered
`Disposition`, and `Step` as the sole policy interpreter (per
encode-lessons-in-structure). Validation lives at the load boundary (unknown
action, `skip: group` outside a group) per boundary-discipline; the walk trusts
the parsed types. Single source of truth: the disposition is recomputed from
submissions every walk, never persisted, so replay is deterministic and
idempotent (per make-operations-idempotent). What it deliberately does **not**
do: it does not add an `Interview::Stopped` variant (stop is a `Completed`
disposition — §7.1 of the design), does not add a wire status, does not persist
control state, does not touch the target normalizer or `Resolution`, and does not
enforce stop with a type bound — `step()` is the one runtime gate, keeping
`Plan::build` pure. Interface depth: callers touch two types and one method; the
walk and answer transaction hold all the complexity, and no transport type sits
on the public surface.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept runtime discipline (`step()` before `Plan::build`) instead of a type
  bound, in exchange for a two-variant `Interview`, a pure directly-callable
  `Plan::build`, and one unified outcome value.
- We accept the trigger being one indirection from the question, in exchange for
  a pure-boolean `confirm` and triggers over any expression.
- We accept `skip: group` at top level as a load error (not an alias for `rest`),
  in exchange for a corrective message and no silent scope coercion.
- We accept the wire `disposition` being advisory, because `apply` recomputes
  enforcement engine-side from the submissions.
- We accept fixed precedence (first dry-run label wins, stop overrides) for
  deterministic, idempotent multi-flow-node behavior under replay.

## Alternatives considered

- **Action-bearing confirm (round 1).** Deeper *locality* for "this no → stop,"
  but it modifies the `confirm` type, binds the trigger to one boolean, must
  define batch-sibling fate at the confirm, and adds a decision funnel at
  `parse_kind`/`check_answer`. Lost on confirm-purity and expressiveness.
- **`Interview::Stopped(Completed)` third variant.** Makes "cannot be planned"
  structural, but forces a third arm on seven `Asking | Complete` sites that all
  mean "same as Complete," re-splitting the outcome the design unifies. Lost on
  interface size for no real safety (the driver still chooses not to plan).
- **New wire status `stopped`.** Splits "the interview is complete" into two
  statuses on the wire, diverging from the in-memory unification and making a
  proceed/stop pair look like different document kinds. Lost; a `disposition`
  field on `complete` is additive and keeps one completion shape.
- **`when` nested inside `flow:`.** Needs a bespoke parse and breaks the uniform
  sibling `when` handling every other node shares. Lost on engine reuse.

## Open questions and risks

- Should a `stop` result auto-remove the staged record (finality, like `abort`),
  or leave it so the decision stays replayable? The design leaves it; is that the
  behavior you want?
- Should the terminal and staged drivers print a distinct notice when a template
  stops or requests dry-run during `stage` (which never plans), or is the wire
  `disposition` enough?
- Is `label` free text, or should it be constrained (e.g. an `Id`) so it reads
  consistently in results?
- Sequencing risk: the paired implementation (1062) integrates runtime after
  1056; independent design preparation is permitted, independent runtime
  integration is not — a coordination fact, not a shape decision.

## Next implementation step

Add `Node::Flow(FlowNode)` with `FlowAction`/`SkipScope`, the `Builder::flow` arm
and its two load rules, the `Disposition` type, and the `Node::Flow` walk arm, so
`Interview::start` on a `flow: stop / when:false` template returns
`Completed { disposition: Proceed }` — proving confirm-untouched and
default-Proceed before any driver routing lands.
