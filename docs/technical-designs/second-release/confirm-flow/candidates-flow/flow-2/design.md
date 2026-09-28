# `flow` as an attachable guard — candidate design

A `flow` guard is one shape: `{ when?, action, label? }`. It attaches in **two
positions** that share one decision engine:

1. **Standalone** — a `flow:` entry in any `interview`/`nodes` list, a peer of
   `message`/`hook`/`computed`. Scope of `skip: group` is its **enclosing frame**
   (positional).
2. **Group-attached** — a `flow:` key on a `group:`. Scope of `skip: group` is the
   **annotated group** (structural). Fires at the group boundary, after the group's
   `when` gate, before the group's children.

`confirm` is untouched. The guard reads committed answers through the existing
`when` construct. No new persisted state: every outcome (stop / dry-run / skip) is
re-derived from stored submissions on every walk and replay.

---

## Usage (caller's view)

### README section — "Steering the interview with `flow`"

> A `flow` guard evaluates a `when` expression over answers already given and
> performs one control **action**. It never asks a question and never records an
> answer. Actions:
>
> | action | effect |
> | --- | --- |
> | `stop` | End the interview now. No files are written, no hooks run, nothing can be planned. |
> | `dry-run` | Let the interview finish and plan normally, but suppress apply — the same effect as `apply --dry-run`, composed with it. |
> | `{ skip: rest }` | Record defaults for every remaining question in the whole interview (the usual skip warning for any unused answer), then finish. |
> | `{ skip: group }` | Record defaults for the remaining questions in one group, then continue after it. |
>
> `when` is optional. A `when`-less guard always fires at its position.
>
> **Attach it where the scope reads naturally.** Put a standalone `flow:` node in the
> list to stop or dry-run at a point. Put `flow:` **on a group** when the action
> governs that group — then `skip: group` means *this* group, not "wherever the node
> happens to sit".

```yaml
# Standalone: stop the whole interview when the user declines.
interview:
  - id: proceed
    type: confirm
    prompt: "Ready to scaffold?"          # ORDINARY confirm — pure boolean
  - flow:
      when: "not proceed"
      action: stop

# Group-attached: skip an optional feature block structurally.
interview:
  - id: want_ci
    type: confirm
    prompt: "Add CI config?"
  - group: ci
    flow: { when: "not want_ci", action: { skip: group } }   # scope = the `ci` group
    nodes:
      - id: ci_provider
        type: select
        options: [github, gitlab]
      - id: ci_matrix
        type: confirm
        prompt: "Test on multiple versions?"

# Dry-run when a preview flag is set (any expression, not one confirm).
interview:
  - id: mode
    type: select
    options: [apply, preview]
  - flow:
      when: "mode == 'preview'"
      action: dry-run
      label: "preview mode"
```

### Call site 1 — library / crate driver (`lib.rs` consumer)

The three-variant match is the whole surface a library caller sees.

```rust
use toha::{Interview, Seed, Plan, ApplyOptions};

let mut interview = Interview::start(&template, Seed { now, defaults })?;
// ... drive to a terminal state via answer()/answer_headless ...
match interview {
    Interview::Asking(_) => unreachable!("driven to completion"),
    Interview::Stopped(stopped) => {
        // Terminal. No plan, no apply. Report why.
        for m in &stopped.last_messages { eprintln!("{m}"); }
        if let Some(reason) = &stopped.reason { eprintln!("stopped: {reason}"); }
        // exit 0 — a stop is a normal, chosen outcome.
    }
    Interview::Complete(completed) => {
        let plan = Plan::build(&template, &completed, target)?;
        if completed.disposition() == Disposition::DryRun {
            print!("{}", plan.lines());          // planned, not applied
        } else {
            plan.apply(target, ApplyOptions::default(), &runner)?;
        }
    }
}
```

### Call site 2 — headless answers document (`toha apply --answers` / wire protocol)

```rust
match protocol::answer_headless(&template, interview, document)? {
    Headless::Completed { completed, .. } => {
        // complete_document(&completed, &ctx) now carries "disposition"
        println!("{}", complete_document(&completed, &ctx));
    }
    Headless::Stopped { stopped, .. } => {
        // NEW terminal document, status: "stopped"
        println!("{}", stopped_document(&stopped, &ctx));
    }
    Headless::Pending { pending, rejections, .. } => {
        println!("{}", batch_document(pending.batch(), &ctx, Some(&rejections)));
    }
}
```

Wire documents an agent reads:

```jsonc
// A flow stopped the interview
{ "protocol": 1, "status": "stopped", "context": {…},
  "messages": ["…"], "reason": "preview mode" }

// A flow set dry-run; still a normal completion, apply is suppressed downstream
{ "protocol": 1, "status": "complete", "context": {…},
  "answers": {…}, "messages": [], "disposition": "dry-run" }
```

### Call site 3 — terminal (interactive) driver

`drive` returns an `Outcome`, not a bare `Completed`, so "the user's answer steered
the flow" has a channel.

```rust
match terminal::drive(interview, &mut InquireAsk, record_submission)? {
    Outcome::Stopped(stopped) => {
        for m in stopped.last_messages { println!("{m}"); }
        // no plan, no apply
    }
    Outcome::Complete(completed) => {
        let dry = cli.dry_run || completed.disposition() == Disposition::DryRun;
        let plan = Plan::build(&template, &completed, &target)?;
        if dry { print!("{}", plan.lines()); }
        else   { plan.apply(&target, opts, &runner)?; }
    }
}
```

---

## Shape

### Data structures (parsed domain types, `template.rs`)

```rust
pub enum Node {
    Question(Question),
    Computed(Computed),
    Group(Group),
    Message(Message),
    Hook(HookNode),
    Flow(Guard),                       // standalone guard — no id, no answer
}

pub struct Group {
    pub name: Id,
    pub nodes: Vec<Node>,
    pub when: Option<Expr>,
    pub guard: Option<Guard>,          // group-attached guard (same shape)
}

/// A control guard. Carries no id and records no answer, exactly like
/// `Message`/`Hook`. Both placements parse to this.
pub struct Guard {
    pub when: Option<Expr>,            // reused `when` construct; None => always fires
    pub action: FlowAction,
    pub label: Option<String>,         // optional diagnostic label (not an id)
}

pub enum FlowAction {
    Stop,
    DryRun,
    Skip(SkipScope),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SkipScope {
    Rest,                              // whole interview from here
    Group,                             // one group's subtree
}
```

`collect_answers` ignores `Node::Flow` (it joins `Message`/`Hook`); the guard never
enters the flat `Answers` map, never collides with the id namespace, needs no
`default`/`empty_answer`. **`QuestionKind::Confirm` is not modified** — "ordinary
confirms stay pure boolean" holds by construction, not by a guard on the type
(*per encode-lessons-in-structure*: the invariant is that the confirm type has no
control surface at all).

### Result / state types (`interview.rs`)

```rust
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
    Stopped(Stopped),                  // NEW terminal variant
}

pub struct Completed {
    // …existing fields…
    disposition: Disposition,          // NEW; re-derived each walk, not persisted
}
impl Completed { pub fn disposition(&self) -> Disposition { self.disposition } }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disposition { Proceed, DryRun }

pub struct Stopped {
    pub answers: Answers,              // committed answers up to the stop
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub now: jiff::Zoned,
    pub reason: Option<String>,        // the guard's `label`, for diagnostics
}
```

`Plan`/`apply` are unchanged and still consume only `Completed`. `Stopped` can never
reach `Plan::build` — the "no files, no hooks, cannot be planned" invariant is
encoded by the type, not a runtime flag (*per encode-lessons-in-structure*).

### The single decision site

The walk's return type changes from `Result<bool, EvalError>` to `Result<Flow, _>`;
`bool` was already overloaded (`false` == "stopped early", disambiguated by the
`blocked` field). `Flow` names the four ways a frame ends:

```rust
enum Flow {
    Ran,        // reached the end of these nodes; keep walking the parent
    Blocked,    // a node waits for answers; `self.blocked` names it
    Stopped,    // a guard requested a terminal stop
    SkipRest,   // whole-interview skip: caller skips its remaining siblings too
}
```

One function decides; it mutates nothing, so the decision has a single source of
truth (*per boundary-discipline*: decide in the pure core, apply at the call site):

```rust
enum Fired { None, Block, Stop, DryRun, Skip(SkipScope) }

impl<'a> Advance<'a> {
    /// Decide a guard's effect from committed answers. Pure: no state change.
    fn fire(&self, g: &Guard, skip: bool) -> Result<Fired, EvalError> {
        if skip { return Ok(Fired::None); }                 // under ancestor skip: never fires
        if let Some(w) = &g.when {
            if !expr_ready(w, &self.answers, self.template) {
                return Ok(Fired::Block);                     // gate on when refs (like message/hook)
            }
        }
        let ctx = context(self.template, &self.answers, &self.seed);
        let active = g.when.as_ref()
            .map(|w| w.eval(&ctx).map(|v| v.is_true())
                 .map_err(|e| fault(&flow_id(g), "when", Some(w.source()), e)))
            .transpose()?
            .unwrap_or(true);                                // when-less => always fires
        Ok(if !active { Fired::None } else { match g.action {
            FlowAction::Stop        => Fired::Stop,
            FlowAction::DryRun      => Fired::DryRun,
            FlowAction::Skip(scope) => Fired::Skip(scope),
        }})
    }
}
```

The two placements are two call sites of `fire`; the effect is applied by the walk:

```rust
// standalone node arm
Node::Flow(g) => match self.fire(g, skip)? {
    Fired::None                  => {}
    Fired::Block                 => return self.block(node),
    Fired::Stop                  => return Ok(Flow::Stopped),
    Fired::DryRun                => self.dry_run = true,     // sticky within this walk
    Fired::Skip(SkipScope::Group)=> skip = true,             // skip rest of THIS frame
    Fired::Skip(SkipScope::Rest) => { skip = true; rest = true; }
},

// group arm (after computing `active` from g.when, as today)
Node::Group(g) => {
    let mut child_skip = !active;
    let mut skip_after = false;
    if active {
        if let Some(guard) = &g.guard {
            match self.fire(guard, skip)? {
                Fired::None                   => {}
                Fired::Block                  => return self.block(node),
                Fired::Stop                   => return Ok(Flow::Stopped),
                Fired::DryRun                 => self.dry_run = true,
                Fired::Skip(SkipScope::Group) => child_skip = true,           // skip THIS group
                Fired::Skip(SkipScope::Rest)  => { child_skip = true; skip_after = true; }
            }
        }
    }
    // existing skipped_descendants_ready gate when child_skip …
    match self.walk(&g.nodes, &key, child_skip)? {
        Flow::Blocked  => return Ok(Flow::Blocked),
        Flow::Stopped  => return Ok(Flow::Stopped),
        Flow::SkipRest => { skip = true; rest = true; }
        Flow::Ran      => {}
    }
    if skip_after { skip = true; rest = true; }
}
```

`skip` is the loop's `mut` parameter; flipping it mid-loop makes every later sibling
take the existing skip branch (record its default, mark `Skipped`, emit the unused
warning). `rest` is a frame-local `bool`; the frame returns `Flow::SkipRest` when
set, so each ancestor flips too — whole-interview skip. **This reuses the existing
skip machinery verbatim** (`Advance::skip`, `Skipped`, group recursion); no parallel
walker.

### How the scope decision becomes structural

For a **standalone** guard, `skip: group` == "skip the rest of the frame I sit in".
At top level that frame is the whole interview, which is *not* what `group` should
mean — so a standalone `skip: group` **outside any group is a load error** naming the
fix (below). For a **group-attached** guard, `skip: group` == the annotated group,
full stop — the scope is the structure, not the position. This is the reason to keep
both placements: authors who want group scope attach to the group and get an
unambiguous, move-safe scope; authors who want a positional stop/dry-run/skip-rest
use the node without inventing a wrapper group.

### Is the standalone node still needed?

Yes. `stop`, `dry-run`, and `skip: rest` are position-anchored, not group-anchored;
a top-level "stop when they decline" has no group to attach to, and forcing a
wrapping group would add structural noise for a positional intent (*per
minimize-reader-load*). The group-attached guard's distinctive value is narrow and
real: it makes `skip: group` scope structural and offers a boundary home for
stop/dry-run. Both compile to `Guard`/`FlowAction` and route through `fire`; there is
one engine, two attach points.

### `advance` — mapping `Flow` to a result

```rust
let outcome = state.walk(&template.interview, "", false)?;
let has_prompt = /* any Item::Prompt in batch */;
match outcome {
    Flow::Stopped => Ok(Interview::Stopped(Stopped { answers: state.answers,
        last_messages: state.messages[state.step_start..].to_vec(),
        messages: state.messages, now: state.seed.now, reason: state.stop_reason })),
    Flow::Blocked if !has_prompt => Err(/* unresolved deps, unchanged */),
    Flow::Blocked                => Ok(Interview::Asking(Pending { /* unchanged */ })),
    Flow::Ran | Flow::SkipRest if has_prompt => Ok(Interview::Asking(Pending { … })),
    Flow::Ran | Flow::SkipRest => Ok(Interview::Complete(Completed {
        /* … */ disposition: if state.dry_run { Disposition::DryRun } else { Disposition::Proceed } })),
}
```

`Advance` gains `dry_run: bool` and `stop_reason: Option<String>`, both **re-derived
every walk** — they are working state of one `advance`, never stored in `Pending` or
the staged record.

### Interface depth

The public surface grows by exactly: one `Node` variant, one `Group` field, the
`Guard`/`FlowAction`/`SkipScope` triple, a third `Interview` variant, a `Disposition`
accessor on `Completed`, a `Stopped` struct, and one wire status + one optional wire
field. Behind that surface sits the entire control engine: readiness gating, the
`skip=true`/`rest` propagation across nested frames, dry-run stickiness, terminal
unwinding, and re-derivation on replay — all in `walk`/`fire`, which no driver sees.
Drivers, protocol, and staging are thin adapters that route one of three terminal
states (*per boundary-discipline*: business logic in pure `walk`/`fire`; the shells
stay thin). No transport type reaches the engine; the wire `status: stopped` /
`disposition` strings are produced only at the `protocol.rs` boundary from
`Stopped`/`Disposition` domain types.

### What it deliberately does not do

- No new persisted state. The staged record stays `{ target, template, commit, named,
  now, submissions }`. Stop/dry-run/skip are recomputed from submissions on every
  replay (*per make-operations-idempotent*: replaying the same submissions twice
  yields the identical terminal state).
- No second target normalizer, no `Path::canonicalize` at consumers. The guard fires
  inside the engine, upstream of the target entirely; dry-run reaches `Plan::build`
  with whatever target type the consumer holds (`&Path` now, `&CanonicalTarget`
  post-1056) — the design is agnostic to it.
- No change to `confirm`, to the skip/default readiness owners
  (`default_ready`/`skipped_descendants_ready`), or to the flat `Answers` map.

---

## Error / results contract

**Load-time (validate at the boundary, `template.rs` builder + schema):**

- `flow` with an unknown or absent `action` → load error at
  `interview[i].flow.action`: *"unknown flow action `X`; expected stop, dry-run, or
  {skip: rest|group}"*.
- Standalone `flow` with `action: { skip: group }` that has **no enclosing group** →
  load error: *"skip: group requires an enclosing group; use `skip: rest` for the
  whole interview, or attach `flow:` to a group"*. (Nesting is known at build time.)
- `group.flow` with `action: { skip: group }` is always valid (its scope is the
  group). `skip: rest` is valid in either placement.
- A `flow` key combined with `id`/`type`/`prompt` on the same node, or `flow` on a
  question node → load error (`flow` is a node kind, mutually exclusive with
  `question`/`group`/etc.), parallel to the existing `loop`/`options` guards.

**Run-time (`EvalError`, unchanged shape):** a guard whose `when` raises during
`eval` produces the existing `fault(&flow_id(g), "when", …)` — identical to a
message/hook `when` fault. `flow_id(g)` is the `label` if present, else a synthetic
`flow` id, purely for the diagnostic.

**Terminal results:**

| driver | stop | dry-run | skip (rest / group) |
| --- | --- | --- | --- |
| library | `Interview::Stopped` | `Completed { disposition: DryRun }` | `Completed` with skipped ids recorded + unused warnings |
| terminal | `Outcome::Stopped` → print, exit 0 | plan printed, apply suppressed (OR'd with `--dry-run`) | normal completion, defaults recorded |
| headless / wire | `status: stopped` document | `status: complete` + `disposition: dry-run` | `status: complete`, skip warnings in `messages` |
| staged (`stage`/`continue`) | replay → `Stopped` → stopped document, record kept, no apply | replay → `Complete(DryRun)` | replay → `Complete`, skip warnings |
| direct (`apply PATH`) | `Stopped` → no write, exit 0 | plan printed, no write | write with skipped defaults |

**Atomicity / rejection (1072 contract):** the guard fires only inside the committed
`advance` produced by `Pending::answer`. A rejected document returns
`AnswerError::Rejected { pending, .. }`; its tentative `advance` (and any guard fired
within it) is discarded, so **a rejected document fires nothing and persists
nothing**. The early-answer `stands` closure gains one arm: a probe error for a
question the terminal step **stopped** (rather than reached) is dropped, matching the
"proven-skipped early error is omitted" policy — a `Stopped` outcome is treated like
`Complete` there. (See open question on the stop-vs-skip warning surface.)

**1072 / configured-`Resolution` composition:** wherever confirm-flow touches
configured-default consumption it routes through the origin-bearing `Resolution`
(`Resolution::start`, `StagedRecord::replay_with_resolution`) unchanged; those
constructors now may yield `Interview::Stopped`, handled exactly like the existing
`Complete` guard (a submission after a stopped interview is a `Replay` error). No
configured origin is reconstructed and no id-only key is re-derived from the flat
projection.

---

## Proposed spec / schema / guide edits (described; owned by the paired implementation)

**`template-format.schema.yml`** — additive:
- Add `#/$defs/flow-node` and `#/$defs/guard` and list `flow-node` in
  `#/$defs/node.oneOf`.
  ```yaml
  guard:
    type: object
    additionalProperties: false
    required: [ action ]
    properties:
      when:  { $ref: "#/$defs/when" }
      label: { type: string }
      action:
        oneOf:
          - const: stop
          - const: dry-run
          - type: object
            additionalProperties: false
            required: [ skip ]
            properties: { skip: { enum: [ rest, group ] } }
  flow-node:
    type: object
    additionalProperties: false
    required: [ flow ]
    properties: { flow: { $ref: "#/$defs/guard" } }
  ```
- Add optional `flow: { $ref: "#/$defs/guard" }` to `#/$defs/group.properties`.
- Add the load rule "skip: group needs an enclosing group" (structural, checked in
  the builder; the schema cannot express nesting depth).

**`interview-protocol.schema.yml`** — additive:
- Add `#/$defs/stopped` to the top-level `oneOf`:
  ```yaml
  stopped:
    type: object
    additionalProperties: false
    required: [ protocol, status, context, messages ]
    properties:
      protocol: { const: 1 }
      status:   { const: stopped }
      context:  { $ref: "#/$defs/context" }
      messages: { $ref: "#/$defs/messages" }
      reason:   { type: [ string, "null" ] }
  ```
- Add optional `disposition: { enum: [ proceed, dry-run ] }` to `#/$defs/complete`
  (absent == `proceed`). This is a wire-contract change: it adds an enum member and a
  declared field, so agents pinning the old schema must refresh. It removes nothing.

**Guides:** a `flow` section in `docs/template-flow.md` (both placements, the four
actions, the `skip: group` scope rule and the structural-vs-positional guidance) and
the two new wire documents in `docs/template-interviews.md` / the protocol guide.

---

## Falsifiable behaviors (test scenarios)

Both branches of a condition, nesting, replay, and every driver:

1. **stop, true branch:** `confirm proceed=false`, `flow when "not proceed" stop` →
   `Interview::Stopped`; no file written, no hook run; `Plan::build` never called.
2. **stop, false branch:** `proceed=true` → guard `None`, interview completes and
   applies normally.
3. **dry-run:** `flow dry-run` fires → `Completed.disposition() == DryRun`; driver
   builds the plan, writes nothing; identical plan text whether triggered by the
   guard or by `--dry-run`; both together still dry-run (OR).
4. **skip: rest:** guard fires at top level → every later question recorded with its
   default/empty; an answer supplied for a skipped question yields exactly the
   existing `answer for "<id>" was not used: the question was skipped` warning.
5. **skip: group (standalone, inside a group):** remaining group siblings skipped;
   nodes after the group ask normally.
6. **skip: group (group-attached):** `group.flow skip:group` skips exactly the
   annotated group's subtree; a sibling group after it is unaffected — proving the
   scope is structural, not positional.
7. **nested skip: group vs skip: rest:** in `A[ B[ flow skip:group ], C ]`, group
   scope skips `B`'s remainder and continues into `C`; switching to `skip: rest`
   skips `B`'s remainder **and** `C` and everything after `A`.
8. **readiness gate:** `flow when "proceed"` reached while `proceed` is unanswered
   ends the batch at the guard (blocks); the guard fires only on the next walk after
   `proceed` commits — proving the decision is from committed answers only.
9. **under ancestor skip / false when:** a `flow stop` inside a `when:false` group,
   or after an earlier `skip:rest`, does **not** fire.
10. **rejection / atomicity:** a document that would trigger `stop` but also carries a
    rejected answer returns `AnswerError::Rejected`; re-inspecting the pending shows
    no stop, no persisted state.
11. **replay determinism:** a staged record whose submissions trigger stop/dry-run,
    replayed via `replay` and `replay_with_resolution`, reproduces the identical
    terminal state with no extra stored fields; `apply` from that record refuses to
    write (stop) or plans-only (dry-run).
12. **driver parity:** scenarios 1/3/4 asserted identical across library, terminal,
    headless, staged, and direct — same skipped ids, same warnings, same
    stop/dry-run outcome.
13. **negative wire:** a `stopped` document validates against the amended protocol
    schema and is rejected by the old one (documents the additive break); a
    `complete` document without `disposition` still validates.
14. **load errors:** standalone `skip: group` at top level; unknown `action`; `flow`
    beside `type`/`id` — each a load error naming the corrective form.

---

## Comparison — `flow` guard vs action-bearing confirm

| axis | `flow` attachable guard (this) | action on `confirm` (round 1) |
| --- | --- | --- |
| touches `confirm` | no — pure boolean by construction | yes — adds a guarded `action` field |
| trigger | any `when` over prior answers/computed | one confirm's truth value |
| expressiveness | fires on any condition; multi-answer, computed | bound to a single confirm |
| scope of `skip: group` | **structural** when attached to a group; positional as a node | positional (where the confirm sits) |
| new author concepts | a node kind + optional group attribute + action enum | a per-confirm `on: {when_true/when_false}` map |
| engine reuse | `when` + node walk + skip machinery verbatim | reuses skip; adds a confirm-answer decision funnel |
| regression risk to plain confirms | none (type untouched) | guarded, but the type changed |
| locality | trigger is one indirection from the question | action co-located on the question |

**Where this is better:** `confirm` is untouched (the invariant is structural, not
guarded); it reuses `when` and the existing node/skip machinery with no new
decision funnel; it is strictly more expressive; and — unique to the attachable
shape — `skip: group` scope becomes *structural* when the guard sits on the group,
which the round-1 shape cannot express.

**Where this is worse:** the trigger is one indirection from the question. For the
simplest "this **no** → stop", the action-bearing confirm is more locally obvious
because the action sits on the question the reader is already looking at. The
attachable guard answers this partway — attaching to a group re-couples action and
scope at the boundary — but a standalone stop still reads a line or two away from the
confirm it depends on.

**Second-order cost this shape adds over a bare standalone-node design:** two attach
points for one guard is a slightly larger surface and one genuine overlap —
`group.flow { skip: group }` and `group.when: false` both skip the group's subtree.
The guard earns its place by also carrying stop/dry-run/skip-rest at that boundary
and by making the *action* choice explicit; see the open question on whether to lint
the pure-`skip:group` guard toward `when`.
