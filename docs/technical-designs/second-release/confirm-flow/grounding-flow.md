# Re-ground (round 2) — a `flow` node vs an action-bearing confirm

Phase A evidence for Bob's 19:06 structural request: *"instead of adding an action
to `confirm` types, perhaps a new `flow` block with a when expression and action
enum should be used instead."* This is a change request, not approval. It re-grounds
the ownership, evaluation, default, skip, and replay semantics of a standalone
`flow` node and compares it with the round‑1 action‑bearing‑confirm shape. The
shared engine facts, drivers, protocol closure, predecessor contracts, and
Preserve/Change/Avoid/Risk in `grounding.md` still hold; only the *trigger surface*
changes. All round‑1 artifacts (grounding, rubric, runner-task, candidates 1‑4,
cross‑judge, own‑score) are retained as historical evidence.

## The proposed shape

A new interview node, declared in the `interview` list, that carries a `when`
expression and an action:

```yaml
interview:
  - id: proceed
    type: confirm
    prompt: "Ready to scaffold?"        # an ORDINARY confirm — unchanged
  - flow:
      when: "not proceed"               # any expression over earlier answers
      action: stop                      # stop | dry-run | { skip: rest | group }
```

The control decision is decoupled from the confirm answer: the confirm records a
plain boolean, and a separate `flow` node reads it (or any expression) through the
existing `when` machinery.

## Ownership and node machinery (evidence at `a62061f`)

A `flow` node is a peer of `Message`, `Hook`, and `Computed`, not a change to
`Question`:

- `enum Node` — `template.rs:87` — variants `Question`/`Computed`/`Group`/`Hook`/
  `Message`. A `flow` node adds `Node::Flow(FlowNode { when: Option<Expr>, action:
  FlowAction })`.
- Node kind is chosen by key in the builder — `template.rs:574-611`
  (`if map.contains_key("hook"|"group"|"message"|"computed")`). A `flow` node is a
  new `if map.contains_key("flow")` arm producing `Node::Flow`, exactly parallel to
  `Node::Message { text, when }` (`template.rs:600-602`).
- It carries **no id** and **records no answer**: `collect_answers` ignores
  `Message`/`Hook` (`template.rs:815`); `Flow` joins them. So it never enters the
  flat `Answers` map, never collides with the id namespace, and needs no
  `default`/`empty_answer` handling.

Consequence: the `confirm` question type (`QuestionKind::Confirm { default }`,
`template.rs:114`) is **not modified at all**. "Ordinary confirms keep pure boolean
behavior" is true by construction — the round‑1 shape had to add and then guard an
`action` field on the confirm; the flow shape never touches it.

## Evaluation (walk)

A `Node::Flow` arm in `Advance::walk` mirrors the `Node::Message`/`Node::Hook` arms
(`interview.rs:1191-1258`):

1. **Fire once per position** via the `visited` set keyed by the node's positional
   `key` (`interview.rs:1192/1204/1231/1242`) — a flow node reached again on a later
   walk does not re-fire spuriously.
2. **Readiness gating**: block until the `when` expression's references are
   answerable, the same `*_ready`/`has_refs` gate the other nodes use
   (`interview.rs:299-343`). A flow node whose `when` references an unanswered id
   ends the batch just as a referencing question would; it is **never held in a
   batch** (interview-protocol.yml:14-18: computed/message/hook nodes are evaluated
   as soon as their references are answered, never held — `flow` joins that class).
3. **Evaluate `when`** with `w.eval(&ctx).map(|v| v.is_true())`
   (`interview.rs:1213/1251`). A node whose `when` is false is skipped with all its
   effect (template-format.yml:54); an absent `when` always fires at its position.
4. **Under an ancestor skip** (a `when:false` group, or a prior flow skip), the flow
   node is not evaluated — same rule as every other node.
5. If `when` is true, apply the action effect using the same in‑walk mechanics
   round‑1's node‑effect candidate used: `stop → terminal Ended`, `dry-run → sticky
   Disposition on Completed`, `skip → the existing skip flag`
   (`Advance::skip`/`Skipped`/group recursion), with `skip: rest` propagating one
   frame per return and `skip: group` local to the enclosing frame. Group scope at
   interview top level equals rest.

## Default, early-answer, and `when` composition

- The flow node has **no default and takes no answer**, so the template-default and
  early-answer policies (the five fixed 1072 policies) do not apply to it directly.
  They continue to apply unchanged to the `confirm`/other questions the flow node's
  `when` reads.
- `when` is the *existing* conditional construct (`template-format.yml:54`,
  `docs/template-flow.md`), so composing "stop when they answered no" is
  `when: "not proceed"` — no new `when_true`/`when_false` mini‑language. The flow
  action can depend on **any** expression (several answers, a computed value), not
  only one confirm's boolean, which is strictly more expressive than round‑1.
- An early/held answer that the flow's `when` reads is applied before the flow node
  is reached, by the ordinary batch/readiness rules; the flow decision is therefore
  computed from committed answers only.

## Skip, stop, dry-run outcomes (unchanged from round 1)

The outcomes Bob requires are preserved exactly: **stop** (no files, no hooks, a
terminal result that cannot be applied), **dry-run** (normal planning, apply
suppressed, distinct from and composing with the `apply --dry-run` flag), and
**skip** of the **whole interview** or the **current group**, with nested groups,
defined skipped values (existing default/empty), and the existing "answer not used"
warning. Only the *trigger* moves from a confirm answer to a flow node's `when`.

## Replay determinism and atomicity (unchanged)

Nothing new is persisted: the staged record still holds only `{ submissions,
identity, now }`. A flow node is re‑evaluated from the recorded answers on every
walk and every replay, so resume reproduces the identical outcome. The action fires
only inside the walk reached by `advance`; a rejected document discards the
tentative advance (atomicity), and the `stands` early‑failure classifier reads the
new outcomes via the `skipped` set — identical to round 1, because the effect
mechanics are the same; only the trigger differs.

## Comparison axes (flow node vs action-bearing confirm)

| Axis | `flow` node (round 2) | action on `confirm` (round 1) |
| --- | --- | --- |
| Touches `confirm` type | No — ordinary confirms unchanged by construction | Yes — adds a guarded `action` field |
| Trigger | Any `when` expression (reuses existing construct) | One confirm's truth value via new `on:{when_true/when_false}` |
| New concepts for authors | A node kind (like message/hook) + action enum | A per‑confirm `on` map + truth‑value arms |
| Expressiveness | Fires on any condition over prior answers/computed | Bound to a single confirm answer |
| Locality | Trigger and question separated (one indirection) | Action co‑located on the question ("this no → stop") |
| Reuse of engine | Reuses `when` + node‑walk + skip verbatim | Reuses skip; adds a confirm‑answer decision funnel |
| Load rules | `flow` schema; `skip: group` needs a group; `when` optional | action‑only‑on‑confirm; non‑empty; skip‑group‑needs‑a‑group |
| Ordinary‑confirm regression risk | None (type untouched) | Guarded, but the type changed |

Net read: the flow node is the cleaner engine fit (it reuses `when` and the existing
node/skip machinery, and leaves `confirm` untouched) and is more expressive; the
confirm‑action shape is marginally more *locally* obvious for the simplest
"no → stop" case because the action sits on the question. The trade is
expressiveness/cleanliness (flow) vs co‑location (confirm‑action).

## New questions this shape raises (for the arena/decisions)

- Is `when` **optional** on a flow node (a `when`‑less flow always fires at its
  position — useful for an unconditional preview/stop marker)? (Lean: yes, optional.)
- Does a flow node need a **name/label** for diagnostics (like a group's `group:`
  name), since it has no id? (Lean: an optional `label` for messages/results.)
- Is the action enum spelled inline (`action: stop`) with `skip` as
  `{ skip: rest | group }`, or is the whole thing one object? (Arena to explore.)
- Whether to **also** keep an action-on-confirm affordance or make `flow` the sole
  mechanism (Bob's wording says "instead", so: flow is the sole mechanism; confirm
  stays pure boolean).

These feed the round‑2 rubric criteria (unchanged six) and the genuine product
decisions returned at the checkpoint.
