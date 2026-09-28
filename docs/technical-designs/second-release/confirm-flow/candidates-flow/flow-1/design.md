# Candidate design — a standalone `flow` node

A minimal control node, peer to `message`/`hook`/`computed`, that reads any
`when` expression over committed answers and carries one declared control
action. The `confirm` type is not touched. The `flow` key itself carries the
action, mirroring `message:` carrying its text.

---

## Usage (caller's view)

### README section — "Flow nodes"

> A **flow node** steers the interview from a condition, without asking a
> question. Write it in the `interview` list with the `flow` key carrying the
> action, and the ordinary `when` sibling gating it:
>
> ```yaml
> interview:
>   - id: proceed
>     type: confirm            # an ORDINARY confirm — records a plain boolean
>     prompt: "Ready to scaffold?"
>   - flow: stop               # end the interview; write nothing, run no hooks
>     when: "not proceed"
> ```
>
> The action is one of:
>
> | Authored form            | Effect |
> | ------------------------ | ------ |
> | `flow: stop`             | End now. No files, no hooks, cannot be planned. |
> | `flow: dry-run`          | Plan as normal, then suppress apply. Composes with the `--dry-run` flag. |
> | `flow: { skip: rest }`   | Skip every remaining question in the interview, recording each default. |
> | `flow: { skip: group }`  | Skip the rest of the enclosing group (must be inside a group). |
>
> `when` is optional; a `flow` with no `when` always fires at its position.
> Add an optional `label:` sibling for diagnostics and result documents. A flow
> node has no `id` and records no answer, so it never appears in the answers
> object and never collides with a question id.
>
> The action fires **at the node's position**, after every earlier question its
> `when` reads has been committed. Because it reuses `when`, the trigger can be
> any expression over prior answers or computed values — not only one confirm:
>
> ```yaml
>   - flow: { skip: rest }
>     when: "mode == 'minimal'"          # several answers, a computed, anything
> ```

### Call site 1 — library / crate driver (the type is the contract)

A crate consumer drives the engine directly and observes the new outcome as a
third `Interview` variant; there is no `main.rs` glue to route around.

```rust
use toha::{Interview, Seed, Template, Plan, ApplyOptions, Disposition};

let mut interview = Interview::start(&template, Seed { now, defaults })?;
loop {
    interview = match interview {
        Interview::Asking(pending) => pending.answer(next_document(&pending))?,
        // A flow `stop` fired: terminal, carries no answers, cannot be planned.
        Interview::Ended(ended) => {
            for line in &ended.messages { println!("{line}"); }
            return Ok(());                       // no Plan::build is even callable
        }
        Interview::Complete(completed) => {
            let plan = Plan::build(&template, &completed, target)?;
            // A flow `dry-run` rides on the Completed; apply only when asked to.
            if completed.disposition == Disposition::DryRun {
                print_plan(&plan);
            } else {
                plan.apply(target, ApplyOptions::default(), &runner)?;
            }
            return Ok(());
        }
    };
}
```

### Call site 2 — answers-file (headless / wire protocol) driver

```rust
use toha::protocol::{answer_headless, Headless, ended_document, complete_document};

match answer_headless(&template, interview, document)? {
    Headless::Pending { pending, rejections, .. } =>
        emit(batch_document(pending.batch(), &ctx, Some(&rejections))), // status: questions
    Headless::Completed { completed, .. } =>
        emit(complete_document(&completed, &ctx)),   // status: complete (+ disposition)
    Headless::Ended { ended, .. } =>
        emit(ended_document(&ended, &ctx)),          // status: ended  (NEW, additive)
}
```

Wire result of `flow: stop` firing on a rejected document — never, because a
rejected document takes no step (atomicity); the stop is computed only from
committed answers. Wire result once `proceed=false` is committed:

```json
{ "protocol": 1, "status": "ended", "context": { "...": "..." },
  "messages": ["Stopping: nothing was written."], "label": "abort on decline" }
```

### Call site 3 — terminal (interactive) driver

`drive` gains a two-variant return so the interactive shell can route a stop; a
dry-run needs no new channel because it rides inside `Completed`.

```rust
use toha::cli::terminal::{self, Session};

match terminal::drive(interview, &mut InquireAsk, record_each_submission)? {
    Session::Completed(completed) => run_plan(completed),  // disposition honored here
    Session::Ended(ended) => {                             // stop: print, write nothing
        for line in ended.messages { println!("{line}"); }
        Outcome::Ended
    }
}
```

Staged (`stage`/`continue`) and direct (`apply PATH`) drivers reach the same
three outcomes through `StagedRecord::replay` and route them identically: an
`Ended` replay prints its messages and builds no plan; a `Complete` whose
`disposition` is `DryRun` builds the plan and suppresses apply.

---

## Shape

### Data structures (first)

```rust
// template.rs — a new peer variant; Question/QuestionKind untouched.
pub enum Node {
    Question(Question),
    Computed(Computed),
    Group(Group),
    Message(Message),
    Hook(HookNode),
    Flow(FlowNode),                 // NEW
}

/// A control node: no id, records no answer, fires once at its position.
#[derive(Debug)]
pub struct FlowNode {
    /// Gating condition over committed answers. `None` always fires.
    pub when: Option<Expr>,
    /// Optional diagnostic name (the node has no id).
    pub label: Option<String>,
    /// The control effect, declared as data — never inferred from text.
    pub action: FlowAction,
}

/// Invalid actions are unrepresentable once parsed (encode-lessons-in-structure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAction {
    Stop,
    DryRun,
    Skip(SkipScope),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipScope {
    /// Skip the rest of the whole interview.
    Rest,
    /// Skip the rest of the enclosing group. A load error at top level.
    Group,
}
```

```rust
// interview.rs — the third outcome and the dry-run carrier.
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
    Ended(Ended),                   // NEW — terminal, cannot be planned
}

/// A stop. Carries only what a driver prints; nothing a plan consumes.
#[derive(Debug)]
pub struct Ended {
    /// Messages reached in interview order, up to the stop.
    pub messages: Vec<String>,
    /// Messages reached by the step that ended the interview.
    pub last_messages: Vec<String>,
    /// The stopping flow node's `label`, if any.
    pub label: Option<String>,
}

/// How a completed interview asks the pipeline to treat its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition {
    #[default]
    Apply,
    DryRun,
}

pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    pub disposition: Disposition,   // NEW — default Apply; single source of truth
    skipped: Skipped,
    step_start: usize,
}
```

**Why these types.** The stop outcome is a *variant*, not a flag, so that
`Plan::build(&Completed, ...)` is literally uncallable on a stop — "cannot be
planned" is enforced by the type, not a runtime check (`per
encode-lessons-in-structure`). Dry-run is a *field on `Completed`*, not a
variant, because a dry-run **is** a normal completion that still produces a plan;
making it a variant would fork every plan-building call site for no gain. The
`FlowAction`/`SkipScope` pair makes every action the walk can meet a closed,
already-validated value (`per boundary-discipline`: parse at load, trust inside).

### The single decision — one walk arm

The whole feature's decision lives in one new arm of `Advance::walk`
(`interview.rs:1054`), mirroring the `Node::Message`/`Node::Hook` arms verbatim
in structure:

```rust
Node::Flow(f) => {
    if self.visited.contains(&key) { continue; }          // fire once per position
    if !skip && f.when.as_ref()
        .is_some_and(|w| !expr_ready(w, &self.answers, self.template)) {
        return Ok(self.block(node));                       // readiness-gate on when refs
    }
    self.visited.insert(key);
    if skip { continue; }                                  // under an ancestor skip: inert
    let ctx = context(self.template, &self.answers, &self.seed);
    let active = f.when.as_ref()
        .map(|w| w.eval(&ctx).map(|v| v.is_true())
            .map_err(|e| fault(&flow_id(), "when", Some(w.source()), e)))
        .transpose()?.unwrap_or(true);                     // when-less ⇒ always fires
    if !active { continue; }
    match f.action {
        FlowAction::Stop        => return Ok(Walk::Ended),
        FlowAction::DryRun      => self.disposition = Disposition::DryRun, // idempotent
        FlowAction::Skip(Rest)  => { skip = true; climbing = true; }
        FlowAction::Skip(Group) => { skip = true; }        // rest-of-frame only
    }
}
```

The walk's control return widens from `bool` to a small enum so the four
outcomes are explicit and each caller unwinds correctly:

```rust
enum Walk {
    Complete,       // this frame's nodes resolved; parent resumes normally  (was `true`)
    Blocked,        // a node waits for an answer; the step ends the batch    (was `false`)
    Ended,          // a flow `stop` fired; unwind every frame to advance()
    SkippedRest,    // a flow `skip: rest`; each ancestor skips its remainder, then completes
}
```

`skip` becomes a mutable local seeded from the parameter; a `skip: group`
flips it on for the rest of the current frame and returns `Complete` (the parent
continues normally). A `skip: rest` also sets `climbing`, so the frame returns
`SkippedRest`; each parent frame, on seeing it, flips its own `skip` on for its
remaining siblings and re-propagates `SkippedRest` — "one frame per return"
until the root, where the interview completes with every remaining question
recorded as skipped. This reuses the **existing** skip machinery exactly:
`Advance::skip` records the default/empty and marks `Skipped`; `warn_if_held`
emits the unchanged "answer not used" warning. No parallel walker, no second
skip policy (`per` the Avoid list).

`advance` (`interview.rs:1276`) classifies:

```rust
match state.walk(&template.interview, "", false)? {
    Walk::Ended => Ok(Interview::Ended(Ended { messages, last_messages, label })),
    outcome => {
        let complete = matches!(outcome, Walk::Complete | Walk::SkippedRest);
        // ...existing has_prompt logic → Complete { disposition, .. } | Asking | error
    }
}
```

### State ownership and single source of truth

- **`disposition`** is threaded like `messages`/`hooks`: `Advance.disposition`
  → carried into `Pending` across steps → into `Completed`. It is never
  persisted; on replay the flow node re-fires during the step whose committed
  answers first make its `when` true and re-sets it (`per`
  make-operations-idempotent — re-setting `DryRun` is a no-op). Single source of
  truth: the flow arm, derived on every walk, never stored to disk.
- **skip effects** persist through the *existing* carried `answers` + `skipped`
  maps, not through re-firing the flow; `visited` marks the flow done so it does
  not re-fire, exactly as message/hook.
- **`Ended`** is produced only by `advance`; it owns nothing between steps
  because it is terminal.

### Where validation lives (the load boundary)

In the `template.rs` builder (`nodes`, `template.rs:566`), a new arm parallel to
the `message` arm (`template.rs:600`):

```rust
if map.contains_key("flow") {
    // `flow:` value IS the action, mirroring `message:` carrying its text.
    let action = self.flow_action(map.get("flow"), &format!("{path}.flow"), in_group);
    let label  = self.text_label(map.get("label"), &format!("{path}.label"));
    if let Some(action) = action {
        result.push(Node::Flow(FlowNode { when, label, action })); // `when` reused verbatim
    }
    continue;
}
```

`flow_action` records a load `problem` (`per` the existing `problem` channel) for:

1. **Unknown action** — e.g. `flow: halt` → *"flow action must be `stop`,
   `dry-run`, `{ skip: rest }`, or `{ skip: group }`"*.
2. **`skip: group` outside any group** — the builder passes an `in_group: bool`
   down its `nodes` recursion; a top-level `{ skip: group }` is rejected naming
   the corrective form: *"`skip: group` needs an enclosing group; use `{ skip:
   rest }` to skip the rest of the interview"*. (Encodes the invariant the walk's
   "group at top level == rest" note describes, so no ambiguous authoring
   reaches the walk.)

`collect_answers` (`template.rs:805`) gains `Node::Flow(_) => {}`; `expressions`
(`interview.rs:945`) gains a `Node::Flow` arm emitting the `when` refs so a
blocked flow node yields a precise unresolved-dependency error.

### Interface depth

The public surface grows by exactly: one `Node` variant, one `FlowNode`
struct + two small enums, one `Interview` variant + one `Ended` struct, one
`Disposition` field, one `Headless`/`Session` variant each, one `ended_document`
function. Behind that surface sits: readiness sequencing of the trigger after
its referenced answers, mid-frame skip propagation across nested frames, the
tentative-advance atomicity that keeps a rejected document from firing anything,
and replay recomputation with zero new persisted state. No wire or transport
type reaches the surface — `FlowAction` is a parsed domain enum, the wire sees
only additive JSON produced behind `protocol.rs` (`per boundary-discipline`).

### Module / seam diagram

```
 template.yml  ──parse──►  template.rs
   flow: <action>            Node::Flow(FlowNode{ when, label, action })
   when: <expr>              └─ load rules: unknown-action, skip:group-needs-group
        │
        ▼
 interview.rs  ── Advance::walk ──►  ONE Node::Flow arm  ◄── the single decision
   (readiness-gate when · fire once via visited · skip⇒inert)
        │  Walk::{Complete|Blocked|Ended|SkippedRest}
        ▼
   advance()  ─────────────►  Interview::{ Asking | Complete{disposition} | Ended }
        │                                   │              │
        │ (no new persisted state)          │              │
        ▼                                   ▼              ▼
 staging.rs replay          plan.rs Plan::build      (uncallable on Ended)
 (Ended/Complete = no more     apply.rs suppressed
  submissions)                 when disposition=DryRun || --dry-run
        │
        ▼
 drivers (thin adapters, route only):
   terminal.rs  Session::{Completed|Ended}
   protocol.rs  Headless::{Pending|Completed|Ended}  →  status questions|complete|ended
   main.rs      stage · continue · apply · direct   →  Outcome::{…|Ended}
   lib.rs       matches Interview directly
```

### Error / results contract

- **Load errors** (at parse): unknown action; `skip: group` at top level. Both
  are `problem`s that fail template load with a corrective message. No runtime
  path can meet an invalid action.
- **`EvalError`** unchanged: a flow `when` that fails to evaluate faults at
  `flow.when` with its source; a flow blocked as the sole unresolved node yields
  the existing unresolved-dependency error naming the missing id.
- **`Interview::Ended`**: terminal success. No `Plan`, no `Applied`, no hooks.
- **`Completed.disposition == DryRun`**: `Plan::build` runs (pure); apply is
  suppressed; distinct from and OR-composed with the `--dry-run` CLI flag at
  `main.rs:1069`.
- **Atomicity (1072 contract)**: the flow arm runs only inside the tentative
  `advance` in `Pending::answer`; a rejected document returns before that
  advance, so a stop/dry-run/skip on a rejected document never happens and
  nothing persists. The existing `stands`/`dropped` classifier
  (`interview.rs:1519`) reads flow-driven skips through the same `skipped` set —
  an early-invalid answer for a question a stop or skip prevents reaching is
  dropped, matching 1072 policy 2. Depends on the **atomicity invariant**, not on
  1056 code.
- **Origin-bearing `Resolution` (1072/1056 contract)**: flow is
  provenance-neutral. It has no default and takes no answer, so it never touches
  configured-default consumption; the new `Interview::Ended` variant is simply
  threaded through `Resolution::start`/`replay_with_resolution` alongside the
  existing variants. It neither reconstructs configured origins nor reintroduces
  a flat projection (`per` the producer-composition correction).

### What this deliberately does not do

- Does not touch `QuestionKind::Confirm`, `parse_kind`, or `empty_answer`.
  Ordinary confirms stay pure boolean **by construction**.
- Does not persist any new state; no new field on `StagedRecord`.
- Does not add a `when_true`/`when_false` mini-language; the trigger is the
  existing `when`.
- Does not add a second target normalizer or reconstruct origins.

---

## Proposed spec / schema / guide edits (owned by the paired implementation)

**`interview-protocol.schema.yml`** (additive; closure preserved):

- New `$defs/ended`, `additionalProperties: false`, `required: [protocol,
  status, context, messages]`, `status: const ended`, optional `label: string`.
- Add `#/$defs/ended` to the top-level `oneOf` (the union widens to three; the
  two existing statuses are unchanged, so existing agents still validate).
- `$defs/complete`: add optional `disposition: { enum: [apply, dry-run] }`
  (absent ⇒ `apply`). Still `additionalProperties: false`, one more allowed key.

**`template-format.schema.yml`** (additive):

- New `$defs/flow-node`, `additionalProperties: false`, `required: [flow]`,
  `properties: { flow: <FlowAction>, when: {$ref: when}, label: {type: string} }`
  where `<FlowAction>` is `oneOf: [ {const: stop}, {const: dry-run}, {type:
  object, additionalProperties:false, required:[skip], properties:{skip:{enum:
  [rest, group]}}} ]`.
- Add `#/$defs/flow-node` to the `$defs/node` `oneOf` (`:93`).
- Prose load rule beside the `loop`/`options` rules: `skip: group` requires an
  enclosing group.

**Guides**: a "Flow nodes" section in `docs/template-flow.md` and
`template-interviews.md`; `interview-protocol.yml` documents the `ended` status
and the `disposition` field and states both are additive; `configuration.md`
notes dry-run composition with `--dry-run`.

---

## Falsifiable behaviors (test scenarios)

Both branches of a condition, nesting, replay, every driver, driver-parity,
negatives:

1. **when true → stop.** `proceed` confirm then `flow: stop when: "not
   proceed"`. Answer `false`: `Interview::Ended`; `Plan::build` never called; no
   file exists in target; no hook ran. Wire: `status: ended`.
2. **when false → no fire.** Same template, answer `true`: interview completes
   normally, files written, hooks run. (Falsifies "flow always fires".)
3. **dry-run composition.** `flow: dry-run when: "preview"`. With `preview=true`
   and no CLI flag: `Completed.disposition == DryRun`, plan printed, nothing
   written. With `--dry-run` and `preview=false`: same suppression (OR-compose).
   With neither: apply runs.
4. **skip: rest.** `flow: { skip: rest } when: "minimal"` before questions
   `a`,`b`. `minimal=true`: `a`,`b` recorded at defaults, marked skipped; any
   held answers for them produce the unchanged "answer not used" warning;
   interview completes.
5. **skip: group + nesting.** A `flow: { skip: group }` inside group `G` (nested
   in `H`): questions after it in `G` skip; questions in `H` after `G` still ask.
   A sibling `flow` after `G` still fires. (Falsifies rest-vs-group scope.)
6. **skip: group at top level → load error** naming `{ skip: rest }`. Unknown
   action `flow: halt` → load error. (Negative, boundary.)
7. **readiness sequencing.** `flow: stop when: "not proceed"` where `proceed`
   and the flow are otherwise batchable: first batch offers only `proceed`; the
   flow blocks until `proceed` is committed, then fires. (Falsifies "action runs
   before its trigger is answered".)
8. **replay determinism.** Stage → answer `proceed=false` → `continue`/`apply`:
   replay of the stored submissions reproduces `Interview::Ended`; no new state
   in `StagedRecord`. Re-running yields the identical outcome.
9. **rejection atomicity (negative).** A document that both answers a later
   question invalidly and would trigger a stop: the document is rejected, no
   step is taken, no stop fires, nothing persists. A rejected document never ends
   the interview.
10. **driver parity.** Scenarios 1, 3, 4 asserted identically across terminal
    (`Session`), headless (`Headless`/wire status), staged, direct, and library
    (`Interview` match) — one engine outcome, five thin routers.

---

## Comparison — `flow` node vs action-bearing confirm

**Where the flow node is better.** `QuestionKind::Confirm` is untouched, so
"ordinary confirms stay pure boolean" is true by construction rather than by a
guarded field — there is no regression surface on the confirm type at all. It
reuses `when`, the node walk, `visited`, and the skip machinery verbatim, adding
one arm that mirrors `message`/`hook`; the confirm-action shape had to add a new
answer→decision funnel at the boolean choke point. It is strictly more
expressive: the trigger is any expression over prior answers or computed values,
not one confirm's truth. Readiness gating sequences the trigger after its
referenced answers for free, so the "same-batch" hazard resolves itself.

**Where it is worse.** The trigger sits one indirection from the question: for
the simplest "this no → stop", an author writes two nodes (`confirm` + `flow:
stop when: "not proceed"`) and must keep the `when` in step with the confirm's
id, whereas the confirm-action form co-locates the action on the question
(`on: { when_false: stop }`) so the intent reads in one place and cannot drift
from its answer. The flow node also introduces a new node *kind* for authors to
learn, versus a new key on a type they already use.

**Net.** The flow node is the cleaner engine fit and more expressive; the
confirm-action form is marginally more locally obvious for the single simplest
case. This design takes expressiveness and a zero-regression confirm over
co-location, per Bob's "instead" wording (flow is the sole mechanism).
