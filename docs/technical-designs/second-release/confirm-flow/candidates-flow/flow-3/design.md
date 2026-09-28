# Candidate design — `flow` node + unified `Disposition` outcome

A standalone `flow` interview node (peer of message/hook/computed) carries an
optional `label`, an optional `when`, and one action (`stop` | `dry-run` |
`{ skip: rest | group }`). The three control effects collapse into **one**
value — a `Disposition` (`Proceed` ⊑ `DryRun` ⊑ `Stop`) that rides on the single
terminal `Completed`, recomputed from submissions on every walk. Stop stays a
`Completed` disposition, **not** a new `Interview` variant; skip reuses the
existing skip flag verbatim and never touches `Disposition`. Every driver routes
the outcome through one method, `Completed::step()`, so no driver re-implements
stop/dry-run policy.

---

## 1. Usage (caller's view) — write this first

### 1.1 Template author README (proposed addition to `docs/template-flow.md`)

> A **flow** node steers the interview from a condition over earlier answers. It
> has no `id` and records no answer; it is a peer of `message`, `hook`, and
> `computed`. It fires at its position when its `when` is true (a flow node with
> no `when` always fires), and never fires under a skipped group or after a
> `skip: rest`. Give it a `label` to name it in stop/dry-run output.
>
> ```yaml
> interview:
>   - id: proceed
>     type: confirm          # an ORDINARY confirm — pure boolean, unchanged
>     prompt: "Ready to scaffold?"
>   - flow: stop             # the action IS the value of `flow:`
>     when: "not proceed"    # the existing `when` construct, as a sibling key
>     label: "declined at proceed gate"
> ```
>
> Actions:
>
> | `flow:` value        | effect                                                        |
> | -------------------- | ------------------------------------------------------------- |
> | `stop`               | interview ends; **no files, no hooks**; cannot be planned     |
> | `dry-run`            | interview completes; plan is built, **apply suppressed**      |
> | `{ skip: rest }`     | skip every remaining node of the whole interview              |
> | `{ skip: group }`    | skip the remaining nodes of the enclosing group only          |
>
> `skip: group` is a load error outside a group; use `skip: rest` at the top
> level. `dry-run` composes with the `apply --dry-run` flag (either suppresses
> apply). Skipped questions record their default/empty value and emit the
> existing `answer for "<id>" was not used` warning, exactly as a `when`-skip.

### 1.2 Call site A — direct `apply TEMPLATE PATH` (terminal driver, `main.rs::run`)

```rust
// after: let completed = terminal::drive(interview, &mut InquireAsk, ..)?;
// completed: toha::Completed, now carrying completed.disposition
match completed.step() {
    Step::Stop { label } => {                       // no plan, no apply
        eprintln!("{}", guidance::stopped(&invocation, path, label));
        return Outcome::Written(completed.last_messages);   // exit 0, wrote nothing
    }
    Step::Plan { apply, label } => {
        let plan = Plan::build(&template, &completed, path)?;   // pure; safe
        if !apply || cli_dry_run {                  // dry-run from EITHER source
            return Outcome::Written(plan_lines(&plan, force).collect());
        }
        plan.apply_reporting(path, options, &ProcessRunner, &mut on_written)?;
        // ...
    }
}
```

### 1.3 Call site B — headless `apply --answers` (wire driver, `main.rs::run`)

```rust
let result = protocol::answer_headless(&template, interview, raw)?;
match result {
    Headless::Completed { completed, .. } => completed,   // carries disposition
    Headless::Pending { pending, rejections, accepted } => { /* batch 4, unchanged */ }
};
// ...falls into the SAME completed.step() seam as 1.2; and the emitted
// document reports the disposition:
let doc = protocol::complete_document(&completed, &context(&saved));
// -> { "status":"complete", "disposition":"stop", "label":"…", "answers":{…}, … }
```

### 1.4 Call site C — library / crate consumer (`tests/fixtures.rs`, `lib.rs` users)

```rust
use toha::{Interview, Seed, Completed, Disposition, Step, Plan, ApplyOptions};

let completed = match Interview::start(&template, seed)?.answer(doc)? {
    Interview::Complete(c) => c,
    Interview::Asking(_)   => panic!("headless doc left questions"),
};
match completed.step() {
    Step::Stop { label } => { /* record "declined", write nothing */ }
    Step::Plan { apply, .. } => {
        let plan = Plan::build(&template, &completed, target)?;
        if apply { plan.apply(target, ApplyOptions::default(), &runner)?; }
    }
}
// or, when the caller wants the raw value:
assert_eq!(completed.disposition(), &Disposition::Stop { label: None });
```

The caller imports two new names (`Disposition`, `Step`) and calls one new
method (`Completed::step`). Everything else — `Interview`, `Pending::answer`,
`Plan::build`, `Plan::apply` — is unchanged.

---

## 2. Shape — data structures first

### 2.1 Template types (`template.rs`)

```rust
pub enum Node {
    Question(Question),
    Computed(Computed),
    Group(Group),
    Message(Message),
    Hook(HookNode),
    Flow(FlowNode),          // NEW — peer of Message/Hook; no id, no answer
}

/// A control-flow node. Fires once at its position when `when` is true (or
/// always, when `when` is absent); never fires under an ancestor skip. Carries
/// no id and records no answer, so it never enters the `Answers` map or the id
/// namespace and needs no default/empty handling.
pub struct FlowNode {
    pub action: FlowAction,
    pub label: Option<String>,   // diagnostic only; surfaces in stop/dry-run results
    pub when: Option<Expr>,      // the existing `when`, parsed as a sibling key
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlowAction { Stop, DryRun, Skip(SkipScope) }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SkipScope { Rest, Group }
```

`QuestionKind::Confirm { default }` (`template.rs:114`) is **not touched**.
Ordinary-confirm behavior is preserved by construction.

Access-pattern check: the walk needs, per position, (a) the action, (b) the
label, (c) the `when` refs. All three are on `FlowNode`; no map/index/cache is
needed later. `answer_ids`/`collect_answers`/`has_question_id`/
`collect_hook_nodes` ignore `Flow` (no `id`, no `nodes`, no `hook` key), so the
id namespace and the reviewable executable surface are unchanged.

### 2.2 The unified outcome (`interview.rs`)

```rust
/// How a completed interview's plan/apply step must be treated — the single
/// value that unifies normal completion, dry-run, and stop. Computed inside the
/// walk from the flow node(s) that fired; the sole source of truth for
/// stop/dry-run. Ordered: Proceed ⊑ DryRun ⊑ Stop. Never persisted; recomputed
/// from submissions on every walk and every replay.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Disposition {
    #[default]
    Proceed,
    DryRun { label: Option<String> },
    Stop   { label: Option<String> },
}

impl Disposition {
    fn rank(&self) -> u8 {
        match self { Self::Proceed => 0, Self::DryRun { .. } => 1, Self::Stop { .. } => 2 }
    }
    /// Monotone raise toward the more restrictive outcome. First DryRun wins
    /// over a later DryRun (equal rank, no replace); Stop replaces DryRun.
    /// Idempotent, so a re-walk / replay reaches the same value.
    fn raise(&mut self, other: Disposition) {
        if other.rank() > self.rank() { *self = other; }
    }
}
```

```rust
pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    pub disposition: Disposition,   // NEW — Proceed for an ordinary interview
    skipped: Skipped,
    step_start: usize,
}
```

```rust
/// The one interpretation of a completed interview. Every driver calls this
/// instead of matching `Disposition`, so the stop/dry-run/apply policy has a
/// single home (per boundary-discipline: policy behind the interface, not in
/// each shell).
pub enum Step<'c> {
    /// Terminal: build no plan, run no hook. Carries the stop label.
    Stop { label: Option<&'c str> },
    /// Build the plan; apply only when `apply` is true. `dry-run` sets it false.
    Plan { apply: bool, label: Option<&'c str> },
}

impl Completed {
    pub fn disposition(&self) -> &Disposition { &self.disposition }
    pub fn step(&self) -> Step<'_> {
        match &self.disposition {
            Disposition::Proceed          => Step::Plan { apply: true,  label: None },
            Disposition::DryRun { label }  => Step::Plan { apply: false, label: label.as_deref() },
            Disposition::Stop   { label }  => Step::Stop { label: label.as_deref() },
        }
    }
}
```

**`Interview` keeps its two variants** (`Asking(Pending)`, `Complete(Completed)`
— `interview.rs:44`). Stop is a `Completed` with `disposition: Stop`, not a third
variant. Rationale in §7.1.

### 2.3 Signatures that change (everything else is unchanged)

```rust
// template.rs — builder: dispatch a new node kind, exactly parallel to `message`
impl Builder {
    fn nodes(&mut self, values: &[Value], prefix: &str, in_group: bool) -> Vec<Node>;
    //                                                    ^^^^^^^^^^^^^ NEW: depth flag
    //   for the `skip: group` load rule; top-level call passes `false`.
    fn flow(&mut self, map: &Map<String, Value>, path: &str,
            when: Option<Expr>, in_group: bool) -> Option<FlowNode>;
    //   parses `flow:` action + optional `label`; validates skip-scope-in-group.
}

// interview.rs — Advance gains the outcome + two skip-propagation flags
struct Advance<'a> {
    // …existing fields…
    disposition: Disposition,   // NEW — raised by fired flow nodes
    skip_rest: bool,            // NEW — a `skip: rest` fired; propagates to all frames
    halt: bool,                 // NEW — a `stop` fired; walk unwinds, result is Complete
}

fn advance(state: Advance<'_>) -> Result<Interview<'_>, EvalError>;   // signature unchanged
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError>;  // unchanged
}
impl<'a> Pending<'a> {
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>>;  // unchanged
}

// protocol.rs — complete document reports the disposition; parse unchanged
pub fn complete_document(completed: &Completed, context: &Context) -> Value;   // adds fields

// lib.rs — new public names
pub use interview::{Disposition, Step /* + existing */};
pub use template::{FlowAction, FlowNode, SkipScope /* + existing */};
```

`terminal::drive` (`terminal.rs:195`) — **signature unchanged**: it still returns
`Completed`; stop just makes `Interview::Complete` arrive sooner. `Headless`
(`protocol.rs:191`) — **unchanged**: `Headless::Completed { completed, .. }`
already carries the disposition. `StagedRecord` (`staging.rs:20`) — **unchanged**:
no new persisted field. `Plan`/`Applied`/`ApplyOptions` — **unchanged**.

---

## 3. The single decision point — the walk (`interview.rs::Advance::walk`)

Add one arm to the `for` loop, mirroring `Node::Message` (`interview.rs:1230`)
for readiness/`visited`/`when`, and fold the two new skip flags into the
per-iteration `skip`:

```rust
fn walk(&mut self, nodes: &'a [Node], prefix: &str, ancestor_skip: bool)
    -> Result<bool, EvalError>
{
    let mut group_skip = false;                 // raised by `skip: group` in THIS frame
    for (i, node) in nodes.iter().enumerate() {
        let key = format!("{prefix}/{i}");
        let skip = ancestor_skip || self.skip_rest || group_skip;   // one folded value
        match node {
            // …Question / Computed / Group / Hook / Message arms read `skip` unchanged…
            Node::Flow(f) => {
                if self.visited.contains(&key) { continue; }        // fire once per position
                if !skip
                    && f.when.as_ref()
                        .is_some_and(|w| !expr_ready(w, &self.answers, self.template))
                {
                    return self.block(node);        // never held in a batch; ends it here
                }
                self.visited.insert(key);
                if skip { continue; }               // under ancestor/rest/group skip: inert
                let ctx = context(self.template, &self.answers, &self.seed);
                let active = f.when.as_ref()
                    .map(|w| w.eval(&ctx).map(|v| v.is_true())
                        .map_err(|e| fault(&Id::parse("flow").unwrap(), "when", Some(w.source()), e)))
                    .transpose()?
                    .unwrap_or(true);               // absent `when` ⇒ always fires
                if active {
                    match f.action {
                        FlowAction::Stop =>
                            { self.disposition.raise(Disposition::Stop { label: f.label.clone() });
                              self.halt = true; return Ok(false); }
                        FlowAction::DryRun =>
                            self.disposition.raise(Disposition::DryRun { label: f.label.clone() }),
                        FlowAction::Skip(SkipScope::Group) => group_skip = true,
                        FlowAction::Skip(SkipScope::Rest)  => self.skip_rest = true,
                    }
                }
            }
        }
    }
    Ok(true)
}
```

Renaming the parameter to `ancestor_skip` and adding one `let skip = …` line
means **every existing arm reads the folded `skip` with no other edit**. The
group recursion (`interview.rs:1187`) is unchanged: `skip: rest` propagates
upward because each ancestor frame re-reads `self.skip_rest` for its remaining
siblings; `skip: group` dies with the local when the frame returns.

`advance` (`interview.rs:1276`) checks `halt` **before** the batch logic:

```rust
let complete = state.walk(&template.interview, "", false)?;
if state.halt {                                   // a `stop` fired: terminal, discard batch
    return Ok(Interview::Complete(Completed {
        disposition: state.disposition,           // Stop { label }
        last_messages: state.messages[state.step_start..].to_vec(),
        answers: state.answers, messages: state.messages, hooks: state.hooks,
        now: state.seed.now, skipped: state.skipped, step_start: state.step_start,
    }));
}
// …existing !complete/has_prompt error, complete/!has_prompt Complete (carry
//    state.disposition), else Asking — unchanged except Completed gains the field…
```

Skip records defaults through the **existing** `Advance::skip` (`interview.rs:1039`)
and `Skipped` map: skipped siblings walk with `skip=true`, so each records its
default/empty and its held-answer warning exactly as a `when`-skip does today.

---

## 4. Composition, determinism, and the 1072 contract

- **Same-batch confirm.** A flow node whose `when` reads a question still pending
  in the current batch is **not** ready (`expr_ready` false) and blocks, ending
  the batch at the flow node. It fires only on the next step, after that answer
  commits — so it never races its batch-siblings (the round-1 hazard at
  `grounding.md:190`). Readiness gating gives this for free.
- **Committed answers only.** The decision runs inside the tentative `advance`
  in `Pending::answer` (`interview.rs:1503`). On rejection, `AnswerError::Rejected`
  returns the **original** `Pending` (`interview.rs:1496`); the raised
  `disposition` lives only in the discarded `Advance`. A rejected document fires
  nothing and persists nothing — the **1072 atomicity invariant**, unchanged.
- **Early / held answers.** `unless_skipped`/`dropped` (`interview.rs:1519-1527`)
  is unchanged. On a `stop` result, `advance` returns `Complete`, so `stands(id)`
  is `false` and a held early-failure for an unreached question is dropped (the
  document that stopped is accepted; its later answers are simply unused). No new
  policy, no parallel walker.
- **1072 target-identity & Resolution.** Stop/dry-run/skip never touch the target
  path, so `canonical_target` / the opaque `CanonicalTarget` is untouched — no
  second normalizer. Flow reads answers only through the existing `when` context,
  so configured-default consumption still flows through the origin-bearing
  `Resolution` unchanged; flow reconstructs no origins and re-derives no id-only
  key. The design depends on the 1072 **contract** (atomicity + target-identity
  type), not on 1056 runtime.
- **Replay determinism.** `StagedRecord { submissions, identity, now }` is
  unchanged. `Disposition` is recomputed by `raise` on every walk; `visited`
  keeps each flow node firing once; `raise` is idempotent — so replay reproduces
  the identical outcome with **no new persisted state** (`staging.rs:127`).

---

## 5. Driver routing — five surfaces, one seam

| Driver | Where the outcome is routed | Behavior |
| --- | --- | --- |
| **Terminal** (`terminal::drive`, `terminal.rs:195`) | `drive` returns `Completed`; caller calls `completed.step()` at the plan/apply seam (`main.rs:1065`). | `drive` loop unchanged — `stop` just yields `Complete` sooner. |
| **Headless / wire** (`answer_headless`, `protocol.rs:203`) | `Headless::Completed { completed }` → same `step()` seam; `complete_document` reports the disposition. | No new `Headless` variant. |
| **Staged** (`stage`/`continue`/`abort`, `main.rs:543/635/744`) | Never plans; ignores disposition at record time; **replay recomputes** it at `apply`. | `abort` unchanged; no new persisted field. |
| **Direct** (`apply TEMPLATE PATH`, `main.rs:818`) | `completed.step()` before `Plan::build`. | Stop → write nothing; DryRun → plan, suppress apply; composes with `--dry-run`. |
| **Library** (`lib.rs`, `tests/fixtures.rs`) | Caller matches `completed.step()` (call site C). | Policy hidden in `step()`; caller re-implements nothing. |

The CLI `--dry-run` composes at the seam: `let apply = matches!(step, Step::Plan { apply: true, .. }) && !cli_dry_run;` — either source suppresses apply, matching `grounding.md:38`.

Enforcement note (per boundary-discipline): the wire `disposition` is a *report*;
the *enforcement* is `toha apply` recomputing the disposition from the staged
submissions and refusing to write on `Stop`. A consumer that ignores the wire
field cannot cause a stopped template to be applied, because `apply` recomputes.

---

## 6. Error / results contract

### 6.1 Load-boundary validation (`template.rs::Builder`, per encode-lessons-in-structure)

- Unknown `flow:` action (not `stop`/`dry-run`/`{skip: rest|group}`) → load error
  at `interview[i].flow`, naming the allowed set.
- `flow: { skip: group }` at top level (`in_group == false`) → load error:
  `skip: group is only valid inside a group; use skip: rest to skip the rest of the interview`.
- `label` non-string → schema error (declared `type: string`).
- A flow node with neither `id`/`type` nor a recognized kind key already yields
  no node; adding the `flow` key arm before the `type` fallthrough keeps the
  closed dispatch.
- `when` is parsed by the existing sibling handler (`template.rs:573`), so its
  reference-checking (`Builder::refs`) and forward-reference rule apply unchanged.

### 6.2 Runtime results

- **Stop**: `Completed { disposition: Stop { label }, .. }`. `Plan::build` is
  never called (driver checks `step()` first). No files, no hooks. Exit 0 (the
  interview succeeded; it chose not to scaffold).
- **DryRun**: `Completed { disposition: DryRun { label }, .. }`. `Plan::build`
  runs (pure, `plan.rs:179`); apply is suppressed; plan preview printed.
- **Skip**: no disposition change; skipped ids recorded in `Skipped` with
  default/empty answers and the existing unused-answer warning.
- A flow `when` evaluation error is a `fault(id="flow", field="when", …)` —
  identical to a message/group `when` fault.

### 6.3 Wire protocol (proposed additive edits — owned by impl 1062)

`interview-protocol.schema.yml` `$defs/complete` (currently
`additionalProperties: false`, `:76`) gains two optional, declared properties, so
closure holds:

```yaml
complete:
  properties:
    # …existing protocol/status/context/answers/messages…
    disposition:
      description: Control outcome of the interview. Absent ⇒ proceed.
      enum: [ proceed, dry-run, stop ]
    label:
      description: Optional diagnostic label of the flow node that set a non-proceed disposition.
      type: string
```

`status` stays `∈ {questions, complete}` — a stopped interview is a *complete*
document with `disposition: stop`, not a new status (this is the wire face of the
unified outcome; §7.2). `batch` is unchanged: flow nodes are never batch items.
`complete_document` (`protocol.rs:144`) emits `disposition` only when not
`Proceed`, and `label` only when present — old consumers ignoring the field see a
normal complete, and `apply` recomputes enforcement engine-side.

### 6.4 Guide edits (proposed)

- `docs/template-flow.md`: the flow-node section (§1.1), the action table, the
  `skip: group`/`skip: rest` rule, and dry-run/CLI composition.
- `docs/specifications/interview-protocol.yml`: document `disposition`/`label` on
  the complete document and the "report, not enforcement" rule.
- `docs/specifications/template-format.yml`: the flow-node grammar and the two
  load rules.

---

## 7. Load-bearing decisions

### 7.1 Stop is a `Completed` disposition, not a new `Interview` variant

The direction demands an explicit argument. **For this shape, stop is a
`Completed` disposition.**

- Stop, dry-run, and normal completion are all "the interview is over; here is
  how to treat the plan/apply step." They differ only at the *one* apply seam.
  Unifying them on `Completed.disposition` puts that difference in one place;
  `Completed::step()` is the only code that reads it.
- A third `Interview::Stopped(Completed)` variant would force a new arm on every
  `Asking | Complete` match — `main.rs:631/706/953/1015`, `staging.rs:143`,
  `terminal.rs:211`, `protocol.rs:214` — even though staging replay, the terminal
  drive loop, and the headless loop all treat "stopped" identically to
  "complete" (stop looping; the interview is done). That is seven third arms to
  say "same as Complete," which is exactly the split the unified outcome avoids.
- The answer transaction returns the outcome as a first-class arm without a new
  error or a new variant: `Pending::answer` already returns
  `Interview::Complete(Completed)`; the disposition rides inside it.

The cost: the "stop cannot be planned" invariant is enforced by the driver
calling `step()` rather than by the type system forbidding `Plan::build`. Judged
worth it — `Plan::build` stays pure and directly callable (tests and the library
use it), and `step()` is the single, small, deep gate. See §8 tradeoffs.

### 7.2 Interface depth

Public surface added: one `Node` variant, `FlowNode`/`FlowAction`/`SkipScope`
(declared data), `Disposition`, `Step`, and `Completed::{disposition, step}`.
Behind it: readiness-gated once-per-position evaluation, monotone
stop/dry-run/skip resolution, batch discard on stop, skip-scope propagation
across frames, atomic recomputation under the 1072 transaction, and
replay-determinism with zero new persisted state. Callers touch two types and
one method; the walk and the answer transaction hold the complexity. No
transport type is on the public surface — the wire `disposition` string is parsed
to/from the `Disposition` domain enum inside `protocol.rs`.

### 7.3 `flow:` holds the action; `when`/`label` are siblings

`flow: stop` mirrors `message: <text>` and `computed: <expr>`: the node key holds
the node's payload, and `when` is the same sibling key every other node uses
(`template.rs:573`). This reuses the builder's uniform `when` parsing and the
engine's uniform readiness/`expressions` machinery verbatim, rather than nesting
`when` inside `flow:` (which would need a bespoke parse and break the uniform
sibling handling). `label` is a sibling string, diagnostic only.

---

## 8. Tradeoffs accepted

- **Stop enforcement is a driver call, not a type bound.** We accept a runtime
  discipline (`step()` before `Plan::build`) in exchange for keeping `Interview`
  at two variants, `Plan::build` pure and directly callable, and the outcome
  unified on one value. A witness type gating `Plan::build` was judged heavier
  than the risk (one seam, five callers).
- **Trigger is one indirection from the question.** We accept that
  `flow: stop / when: "not proceed"` separates the control from the `proceed`
  confirm, in exchange for leaving `confirm` a pure boolean by construction and
  gaining triggers over any expression.
- **`skip: group` at top level is a load error, not an alias for `rest`.** We
  accept rejecting a plausibly-meant document in exchange for a corrective
  message and no silent scope coercion (`grounding.md:344` warns scope mistakes
  silently change output).
- **The wire `disposition` is advisory.** We accept that a naive consumer could
  ignore it, because enforcement is recomputed by `apply` engine-side; the field
  is a report, and the single source of truth is the submissions.
- **First `dry-run` label wins; `stop` overrides.** We accept a fixed
  precedence (a monotone lattice) so that multiple flow nodes are deterministic
  and idempotent under replay, rather than "last writer wins."

---

## 9. Falsifiable behaviors / test scenarios

Both branches of a condition, nesting, resume/replay, every driver, driver
parity, and negative cases:

1. **Stop, both branches.** `flow: stop / when: "not proceed"`; answer
   `proceed=false` → `Completed.disposition == Stop`, `Plan::build` not called,
   no files, no hooks, exit 0. Answer `proceed=true` → `Proceed`, normal apply.
2. **Dry-run composition.** `flow: dry-run / when: preview` fires → plan built,
   no writes; and `apply --dry-run` with `Proceed` → same suppression; both
   together → still suppressed (idempotent).
3. **Skip rest vs group.** `flow: { skip: rest }` mid-interview → all later
   questions record defaults + unused warnings; `flow: { skip: group }` inside a
   group → only the group's remaining siblings skip; a question after the group
   is still asked.
4. **Nested groups.** `skip: rest` inside a nested group skips the inner
   remainder, the outer remainder, and the top-level remainder; `skip: group`
   inside the inner group leaves the outer group's later nodes active.
5. **Absent `when`.** `flow: stop` with no `when` at a position fires
   unconditionally there; under a `when:false` ancestor group it does **not**
   fire.
6. **Same-batch readiness.** A `flow` whose `when` reads a confirm pending in the
   same batch blocks (ends the batch) and fires only after that confirm is
   answered — the flow never appears as a batch item.
7. **Replay determinism.** A staged interview whose submissions drive a stop:
   `replay` reproduces `disposition == Stop` with identical `answers`/`messages`;
   the staged record has no disposition field.
8. **Atomic rejection.** A document that both answers invalidly and would trip a
   `flow: stop` → `AnswerError::Rejected`, original `Pending` returned,
   disposition never observed, nothing persisted.
9. **Driver parity.** For one stop template and one dry-run template, assert
   `terminal::drive`, `answer_headless`, and `replay` reach the same
   `Completed.disposition` and the same `answers` (extends the existing parity
   test at `terminal.rs:541`).
10. **Wire additivity.** `complete_document` for a stop emits
    `status: complete`, `disposition: stop`, optional `label`; validates against
    the amended `additionalProperties:false` complete schema; a proceed emits no
    `disposition` key.
11. **Load errors.** `flow: { skip: group }` at top level → load error naming
    `skip: rest`; `flow: teleport` → unknown-action load error; both cite
    `interview[i].flow`.
12. **Confirm untouched.** An ordinary `confirm` with no flow node yields
    `Answer::Bool`/`Answer::None` and `disposition == Proceed` end to end (guards
    the "pure boolean" invariant).

---

## 10. Comparison — `flow` node vs action-bearing confirm

| Axis | `flow` node (this shape) | action on `confirm` (round 1) |
| --- | --- | --- |
| Touches `confirm` type | No — pure boolean by construction | Yes — adds a guarded `action` field + load guard |
| Trigger | Any `when` expression over prior answers/computed | One confirm's truth value |
| New author concepts | A node kind (like message/hook) + action enum + label | A per-confirm `on:{when_true/when_false}` map |
| Expressiveness | Fires on any condition (several answers, a computed) | Bound to a single confirm answer |
| Locality | Trigger separated from the question (one indirection) | Action co-located on the question |
| Engine reuse | Reuses `when` + node-walk + skip + readiness verbatim | Reuses skip; adds a confirm-answer decision funnel at `parse_kind`/`check_answer` |
| Same-batch race | None — readiness gates the flow after its refs | Must define fate of batch-siblings around the confirm |
| Regression risk to ordinary confirm | None (type untouched) | Guarded, but the type changed |

**Where flow is better**: confirm is untouched, `when` is reused verbatim, the
trigger is strictly more expressive, and the same-batch ordering hazard
disappears because readiness gates the node. **Where flow is worse**: the trigger
is one indirection from the question — `flow: stop / when: "not proceed"` reads
less locally than `stop` written on the `proceed` confirm, so the simplest
"this no → stop" case is marginally less obvious. Net: flow is the cleaner engine
fit and more expressive; the confirm-action shape is more locally obvious for the
single simplest case only.

---

## 11. Module / seam diagram

```
template.yml
   │  Template::load  (template.rs:727)
   ▼
Builder::nodes(in_group)  ──►  Node::Flow(FlowNode{action,label,when})   [load-boundary validation]
   │
   ▼
Interview::start / Pending::answer  (interview.rs:1322 / 1392)
   │        └─ advance → Advance::walk  (interview.rs:1276 / 1054)
   │               └─ Node::Flow arm  ── THE SINGLE DECISION ──►  Disposition::raise / halt / skip flags
   ▼
Interview::Complete(Completed{ disposition, .. })      ← stop is here, NOT a new variant
   │
   ▼  completed.step()   ── the one policy seam ──
   ├── Step::Stop  ─────────────────────────────►  write nothing, run no hook   (terminal/direct/library)
   └── Step::Plan{apply} ─► Plan::build (plan.rs:179, pure) ─► apply? Plan::apply (apply.rs:56)
                                                              └─ apply &&= !cli_dry_run
   │
   ├── protocol::complete_document  ──►  { status:complete, disposition, label }   [wire report]
   └── StagedRecord{submissions}  ──►  replay recomputes disposition               [enforcement]
```

Tracing input→output touches `template.rs` (parse), `interview.rs` (decide +
carry), and the driver seam (route) — three files, short chain, per
minimize-reader-load.

---

## 12. Next implementation step

Add `Node::Flow(FlowNode)` + `FlowAction`/`SkipScope` and the `Builder::flow`
arm with the two load rules, then the `Disposition` type and the `Node::Flow`
walk arm — so `Interview::start` on a `flow: stop / when:false` template compiles
and returns `Completed { disposition: Proceed }`, proving the confirm-untouched
and default-Proceed invariants before any driver routing lands.
