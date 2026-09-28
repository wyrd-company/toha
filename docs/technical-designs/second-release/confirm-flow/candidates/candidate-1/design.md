# Confirm-controlled interview and apply flow — candidate design

**Structural direction:** a third terminal `Interview` variant, `Stopped`,
produced by `advance` — the sole result constructor. Stop is terminal; dry-run
and skip are *not* — dry-run is a plan-only decision derived onto `Completed`,
and skip resolves inside the walk with the existing skip machinery.

---

## 1. Usage first (caller's view)

### README excerpt

> A `confirm` question can map a specific answer to a control **action**.
> Declare it under `on`, keyed by the answer's truth value. A confirm with no
> `on` is an ordinary boolean and behaves exactly as before.
>
> ```yaml
> interview:
>   - id: proceed
>     type: confirm
>     prompt: Proceed with scaffolding?
>     on:
>       when_false: stop          # answering "no" ends the interview
>   - id: preview
>     type: confirm
>     prompt: Preview only, no writes?
>     on:
>       when_true: dry-run        # answering "yes" plans but writes nothing
>   - id: with_ci
>     type: confirm
>     prompt: Configure CI?
>     on:
>       when_false: skip-group    # "no" skips the rest of the enclosing group
> ```
>
> The four actions are `stop`, `dry-run`, `skip-rest`, `skip-group`. Actions are
> declared data — never inferred from the prompt text. Incompatible forms are
> rejected when the template loads.
>
> * **stop** ends the interview immediately: no plan, no files, no hooks.
> * **dry-run** lets the interview finish, then plans and shows everything but
>   writes nothing and runs no hooks. It is the *engine* dry-run, declared in
>   the template; it is distinct from the `apply --dry-run` CLI flag.
> * **skip-rest** / **skip-group** skip the remaining questions (of the whole
>   interview, or of the enclosing group), recording each skipped question's
>   default/empty answer exactly as a `when: false` skip does.

### Call site A — crate / library (the exhaustive match is the contract)

```rust
use toha::{Interview, Seed, Template, Plan, ApplyOptions};

let interview = Interview::start(&template, Seed { now, defaults })?;
// A confirm action can end the interview before it completes, so a library
// caller MUST handle three terminal states. The compiler enforces it.
match run_to_end(interview)? {                       // drives to a terminal state
    Interview::Asking(_) => unreachable!("run_to_end returns a terminal state"),
    Interview::Stopped(stopped) => {
        eprintln!("stopped at \"{}\": no files written", stopped.trigger());
        // No Plan, no apply. Terminal.
    }
    Interview::Complete(completed) => {
        let plan = Plan::build(&template, &completed, &target)?;
        if completed.plan_only().is_some() {
            print_plan(&plan);                        // dry-run: show, do not apply
        } else {
            plan.apply(&target, ApplyOptions::default(), &runner)?;
        }
    }
}
```

### Call site B — terminal (direct `apply TEMPLATE PATH`)

```rust
// terminal::drive loops until a terminal state and returns a Conclusion —
// never `Asking`. It carries stop out of the middle of an interactive session.
match terminal::drive(interview, &mut InquireAsk, save_each)? {
    Conclusion::Stopped(stopped) => {
        eprintln!("{}", guidance::stopped(&stopped));  // no plan, no apply
        Outcome::Stopped(0)
    }
    Conclusion::Complete(completed) => {
        let plan = Plan::build(&template, &completed, path)?;
        // Engine dry-run and the `--dry-run` flag funnel to the same plan-only
        // branch; either one suppresses writes and hooks.
        if flag_dry_run || completed.plan_only().is_some() {
            Outcome::Written(plan_lines(&plan, force))
        } else {
            /* plan.apply_reporting(..) as today */
        }
    }
}
```

### Call site C — headless (wire protocol)

```rust
match protocol::answer_headless(&template, interview, document)? {
    Headless::Pending { pending, rejections, .. } =>
        emit(protocol::batch_document(pending.batch(), &ctx, Some(&rejections))),
    Headless::Complete { completed, .. } =>
        emit(protocol::complete_document(&completed, &ctx)),   // may carry apply: dry-run
    Headless::Stopped { stopped, .. } =>
        emit(protocol::stopped_document(&stopped, &ctx)),      // status: stopped
}
```

The same `stopped` document is produced by the headless and staged drivers, so
their byte output is identical for identical submissions (driver parity).

---

## 2. Shape — data structures first

### 2.1 The terminal state machine (the load-bearing decision)

```rust
/// The interview state machine. `advance` is the sole constructor of every
/// variant. A caller that reaches a terminal variant is done: `Complete`
/// carries answers to plan/apply, `Stopped` carries an early exit that plans
/// and applies nothing.
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
    /// A confirm answer mapped to `stop`, ending the interview before it
    /// completed. Invariant: no `Plan` is built and `apply` never runs for a
    /// `Stopped` interview.
    Stopped(Stopped),
}
```

Why a variant and not a field on `Completed`: a stop and a completion are
mutually exclusive terminal outcomes with *different downstream capabilities*
(one can be planned, one cannot). Encoding them as one type with a "did we
really finish?" flag would let a caller build a plan from a stopped interview.
The variant makes that unrepresentable — `per encode-lessons-in-structure`.

```rust
/// The result of a confirm `stop`. Mirrors `Completed`'s reporting fields but
/// deliberately carries no capability to plan or apply.
pub struct Stopped {
    /// Answers recorded up to the stop point, in interview order. Reported for
    /// visibility; never applied.
    answers: Answers,
    messages: Vec<String>,
    last_messages: Vec<String>,
    now: jiff::Zoned,
    /// The confirm question whose answer fired the stop.
    trigger: Id,
    skipped: Skipped,
    step_start: usize,
}
impl Stopped {
    /// The id of the confirm question that stopped the interview.
    pub fn trigger(&self) -> &Id { unimplemented!() }
    pub fn answers(&self) -> &Answers { unimplemented!() }
    pub fn messages(&self) -> &[String] { unimplemented!() }
    pub fn last_messages(&self) -> &[String] { unimplemented!() }
}
```

Dry-run is **not** a variant. It is a plan-only decision *derived* from a
recorded confirm answer and carried on `Completed`:

```rust
pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    skipped: Skipped,
    step_start: usize,
    /// The confirm question that forced plan-only mode (engine dry-run), if
    /// any. Derived during the walk from the recorded answer + declared action;
    /// recomputed on every replay; NEVER persisted to the staged record.
    /// `Some` ⇒ apply must plan and show but write nothing and run no hooks.
    dry_run: Option<Id>,
}
impl Completed {
    /// The confirm question that forced plan-only mode, or `None` to apply
    /// normally. Single source of truth for the engine dry-run decision.
    pub fn plan_only(&self) -> Option<&Id> { unimplemented!() }
}
```

Invariant, encoded in type + single-source: the dry-run decision is a pure
function of `(recorded confirm answer, template action mapping)`. It lives in
one place (`Completed.dry_run`, filled by the walk) and is re-derived on every
replay — `per boundary-discipline` and single-source-of-truth. Nothing new is
persisted; the staged record stays `{ submissions, identity, now }`.

### 2.2 Declaration types (template)

```rust
pub enum QuestionKind {
    // ...unchanged variants...
    Confirm {
        /// Unchanged: the boolean answer's default. A confirm still records
        /// `Answer::Bool` / `Answer::None` regardless of `on`.
        default: Option<Typed<bool>>,
        /// Control actions by truth value. `None` ⇒ pure boolean behavior,
        /// bit-for-bit as today. Populated and validated at load.
        on: Option<ConfirmActions>,
    },
}

/// Control actions a confirm answer triggers, keyed by truth value. Load
/// rejects an empty mapping, so at least one arm is `Some` when present.
pub struct ConfirmActions {
    pub when_true: Option<ConfirmAction>,
    pub when_false: Option<ConfirmAction>,
}
impl ConfirmActions {
    /// The action declared for the recorded boolean, if any.
    pub fn for_value(&self, answer: bool) -> Option<ConfirmAction> {
        if answer { self.when_true } else { self.when_false }
    }
}

/// A control action. Declared data; a closed set so an unknown action is a
/// load error, not a runtime surprise — `per encode-lessons-in-structure`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    /// End the interview now: no plan, files, or hooks. → `Interview::Stopped`.
    Stop,
    /// Plan normally, write nothing, run no hooks. → `Completed.dry_run`.
    DryRun,
    /// Skip every remaining question in the interview. Resolved in the walk.
    SkipRest,
    /// Skip every remaining question in the enclosing group. Resolved in the
    /// walk. Load rejects this on a top-level confirm (no enclosing group).
    SkipGroup,
}
```

### 2.3 The single decision funnel (engine walk)

The action is evaluated in exactly one place: the walk's `Node::Question` arm,
when a confirm is **active** (passed `when`) and its **answer is recorded**,
reached in interview order. This is the funnel the task asks for — the one site
that sees both the question node and the final boolean. `per minimize-reader-load`
and short-call-chains: drivers, protocol, and staging never re-evaluate policy.

```rust
struct Advance<'a> {
    // ...existing fields...
    /// Set when a confirm mapped `stop`: ends the walk, yields `Stopped`.
    stop: Option<Id>,
    /// Set when a confirm mapped `dry-run`: carried onto `Completed`. Derived,
    /// never persisted. First trigger in interview order wins (idempotent).
    dry_run: Option<Id>,
    /// Set when a confirm mapped `skip-rest`: forces `skip` for all remaining
    /// nodes at every level, reusing the ordinary skip path.
    skip_rest: bool,
}

impl<'a> Advance<'a> {
    /// The action for an active, answered confirm, or `None`. Pure in
    /// `(recorded bool, declared mapping)`; produces the same result on every
    /// re-walk — `per make-operations-idempotent`.
    fn confirm_action(&self, q: &Question) -> Option<ConfirmAction> {
        let QuestionKind::Confirm { on: Some(actions), .. } = &q.kind else { return None };
        let Some(Answer::Bool(value)) = self.answers.get(&q.id) else { return None };
        actions.for_value(*value)
    }

    /// Applies a confirm action's flow effect. Returns `true` when the walk
    /// must stop (a `stop`), so the caller returns `Ok(false)`. `skip_group`
    /// is the frame-local skip flag for the enclosing group.
    fn fire(&mut self, q: &Question, action: ConfirmAction, skip_group: &mut bool) -> bool {
        match action {
            ConfirmAction::Stop     => { self.stop = Some(q.id.clone()); true }
            ConfirmAction::DryRun   => { self.dry_run.get_or_insert_with(|| q.id.clone()); false }
            ConfirmAction::SkipRest => { self.skip_rest = true; false }
            ConfirmAction::SkipGroup=> { *skip_group = true; false }
        }
    }
}
```

Placement inside `walk` (sketch; unchanged lines elided). `skip_group` is a
frame-local `let mut` so it resets when the frame returns to the parent — that
is what scopes `skip-group` to the enclosing group with zero extra bookkeeping:

```rust
fn walk(&mut self, nodes: &'a [Node], prefix: &str, skip: bool) -> Result<bool, EvalError> {
    let mut skip_group = false;                 // frame-local: enclosing-group skip
    for (i, node) in nodes.iter().enumerate() {
        let skip = skip || self.skip_rest || skip_group;   // reuse the existing skip path
        match node {
            Node::Question(q) => {
                if self.answers.contains_key(&q.id) {
                    if self.skipped.contains_key(&q.id) { self.warn_if_held(&q.id); continue; }
                    // Already-answered, active confirm: fire in interview order,
                    // idempotently, on every re-walk.
                    if let Some(a) = self.confirm_action(q) {
                        if self.fire(q, a, &mut skip_group) { return Ok(false); }
                    }
                    continue;
                }
                if skip { /* existing: default_ready → render_default → self.skip(q,..) */ continue; }
                // ...existing readiness + `when` evaluation → `active`...
                if active {
                    let prompt = make_prompt(..)?;
                    if let Some(raw) = self.held.shift_remove(&q.id) {
                        match check_answer(.., raw) {
                            Ok(answer) => {
                                self.answers.insert(q.id.clone(), answer);
                                // Held confirm answered now: fire after the held check.
                                if let Some(a) = self.confirm_action(q) {
                                    if self.fire(q, a, &mut skip_group) { return Ok(false); }
                                }
                                continue;
                            }
                            Err(..) => { /* existing carried-error path */ }
                        }
                    }
                    self.batch.items.push(Item::Prompt(prompt)); // fresh confirm: fires next step
                } else {
                    /* existing inactive: default_ready → render_default → self.skip(q,..) */
                }
            }
            // ...Computed / Group / Hook / Message arms unchanged...
        }
    }
    Ok(true)
}
```

`advance` inspects `stop` before its existing classification, so a short-circuit
does not fall into the "unresolved dependencies" error path:

```rust
fn advance(mut state: Advance<'_>) -> Result<Interview<'_>, EvalError> {
    let complete = state.walk(&state.template.interview, "", false)?;
    if let Some(trigger) = state.stop.take() {
        return Ok(Interview::Stopped(Stopped {
            answers: state.answers, messages: state.messages.clone(),
            last_messages: state.messages[state.step_start..].to_vec(),
            now: state.seed.now, trigger, skipped: state.skipped,
            step_start: state.step_start,
        }));
    }
    // ...existing !complete && !has_prompt error, Asking, Complete branches...
    // Complete now also carries `dry_run: state.dry_run`.
}
```

### 2.4 The terminal-driver conclusion

```rust
/// A terminal outcome of driving an interview to its end. `drive` never
/// returns `Asking`; the two owned variants are the only ways an interactive
/// session ends.
pub enum Conclusion {
    Complete(Completed),
    Stopped(Stopped),
}

pub fn drive<'a>(
    interview: Interview<'a>,
    ask: &mut impl Ask,
    accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>,
) -> Result<Conclusion, String> { unimplemented!() }
```

### 2.5 Headless outcome

```rust
pub enum Headless<'a> {
    Complete { completed: Completed, accepted: Vec<IndexMap<String, Value>> },
    Pending  { pending: Box<Pending<'a>>, rejections: Rejections,
               accepted: Vec<IndexMap<String, Value>> },
    /// A confirm stopped the interview mid-document. `accepted` are the
    /// submissions applied before the stop.
    Stopped  { stopped: Stopped, accepted: Vec<IndexMap<String, Value>> },
}
```

`answer_headless` gains one arm: when `pending.answer(..)` yields
`Interview::Stopped`, return `Headless::Stopped`. Remaining unsubmitted answers
in the document after a stop are dropped (the interview is over) — the same
"not reached ⇒ not used" rule the walk already applies.

### 2.6 Wire serialization (thin adapter, single source)

```rust
/// The canonical `stopped` protocol document. The only serializer of a stop;
/// used by every headless/staged path, so results are byte-equal across
/// drivers for identical submissions.
pub fn stopped_document(stopped: &Stopped, context: &Context) -> Value { unimplemented!() }
```

---

## 3. Module / seam diagram

```
template.yml ──load──▶ template.rs
                        QuestionKind::Confirm { default, on: Option<ConfirmActions> }
                        └─ load rules: on-only-on-confirm · non-empty · closed enum
                                       · skip-group ⇒ inside a group           (LOAD TIME)
                                              │
                                              ▼
interview.rs  Advance::walk ── confirm_action(q) ──▶ fire(q, action, &mut skip_group)
   (THE FUNNEL, seen once)      │        │        │            │
                     Stop ──────┘   DryRun│   SkipRest/SkipGroup┘
                        │               │        └─ reuse existing skip path
                        ▼               ▼           (render_default · warn_if_held)
             advance ─▶ Interview::Stopped   Completed.dry_run = Some(id)
                        │                        │
        ┌───────────────┼────────────────────────┼───────────────────────────┐
        ▼               ▼                         ▼                            ▼
   terminal.rs      protocol.rs              staging.rs (replay)          crate callers
   drive ─▶         answer_headless ─▶       replay_with_resolution ─▶    match Interview {
   Conclusion       Headless::Stopped        Interview::Stopped =>        Asking|Complete|Stopped }
   {Complete,       │                        terminal (no more subs)      (exhaustive)
    Stopped}        stopped_document ────────┘
        │           complete_document (may carry `apply: dry-run`)
        ▼
   main.rs run: Stopped ⇒ Outcome::Stopped (no Plan)  ·  Complete+plan_only ⇒ show plan, no apply
                                              │
                                              ▼
                            plan.rs / apply.rs  UNCHANGED — never entered on stop;
                            plan built but apply skipped on dry-run (the sole write/hook site)
```

Three files to trace input→output: `template.rs` (declare) → `interview.rs`
(decide, the funnel) → the driver that observes the terminal variant. `plan.rs`
and `apply.rs` are untouched — "no files, no hooks" is achieved by *not entering*
the apply effect loop, which is exactly the existing `--dry-run` and `NeedsTrust`
precedent.

---

## 4. Error / results contract

**Load time (preferred, `per boundary-discipline`).** In `template.rs`, beside
the existing `loop`/`options` rejections:

| Condition | Message (names the corrective form) |
|---|---|
| `on` on a non-confirm question | `on only allowed on confirm` |
| `on` present but empty | `on requires when_true or when_false` |
| unknown key under `on` | `on only allows when_true and when_false` |
| value not in the closed set | `on.<arm> must be one of: stop, dry-run, skip-rest, skip-group` |
| `skip-group` on a top-level confirm | `skip-group requires the question inside a group; use skip-rest at the top level` |

The closed `type` enum is unchanged. `on` is an additive optional key on the
closed `$defs/question` object.

**Run time.** The funnel performs no evaluation (the mapping is static, the
answer is recorded), so it introduces no new `EvalError`. Existing errors are
unchanged.

**Atomicity (composes with the 1072 contract).** Actions fire only inside the
walk, on already-accepted answers. In `Pending::answer`, a document with any
rejection returns `AnswerError::Rejected` *regardless* of what the tentative
`advance` produced — so a rejected document can never emit `Stopped` or set
`dry_run`. A rejected answer never enters skip/reachability classification;
`stands(id)` treats a `Stopped`/`Complete` tentative result as "not standing"
(the question was not reached), dropping its early-failure exactly as for a
completed interview. No rejected document persists.

**Determinism / replay.** Nothing new is persisted. `stop`, `dry_run`, and
`skip_rest` are `Advance` fields, rebuilt from scratch on every `advance`; the
staged record stays `{ submissions, identity, now }`. `StagedRecord::replay*`
returns `Interview`, now possibly `Stopped`; a submission after a terminal
(`Complete` **or** `Stopped`) interview is the existing replay error, message
broadened to "submission after a terminal interview".

**Exit codes / `Outcome`.** New `Outcome::Stopped(u8)` (exit `0` — a stop is a
deliberate, successful control outcome, not a failure) with a stderr message
naming the trigger. `apply --dry-run` (flag) and engine dry-run
(`completed.plan_only()`) funnel to the same plan-only branch and the same
exit `0`.

---

## 5. Proposed canonical-document edits (described, owned by the impl)

**`template-format.schema.yml`** — additive to `$defs/question`
(`additionalProperties:false` retained; `type` enum unchanged):

```yaml
on:
  description: Control actions triggered by a confirm answer, by truth value.
  type: object
  additionalProperties: false
  minProperties: 1
  properties:
    when_true:  { $ref: "#/$defs/confirm-action" }
    when_false: { $ref: "#/$defs/confirm-action" }
# new $def:
confirm-action:
  enum: [ stop, dry-run, skip-rest, skip-group ]
```

**`template-format.yml`** — document `on`, the four actions, the load rules
above, and that ordinary confirms are unaffected. State that `skip-rest` /
`skip-group` reuse the `when`-skip contract: each skipped question records its
default/empty answer and a held answer for it emits the existing
`answer for "<id>" was not used: the question was skipped` warning.

**`interview-protocol.schema.yml`** — add a third top-level document and status:

```yaml
# add to the top-level oneOf:
- { $ref: "#/$defs/stopped" }
# new $def:
stopped:
  type: object
  additionalProperties: false
  required: [ protocol, status, context, trigger, answers, messages ]
  properties:
    protocol: { const: 1 }
    status:   { const: stopped }
    context:  { $ref: "#/$defs/context" }
    trigger:  { $ref: "#/$defs/identifier" }   # the confirm that stopped
    answers:  { $ref: "#/$defs/answers" }       # recorded so far; not applied
    messages: { $ref: "#/$defs/messages" }
# additive optional field on `complete` (derived; present only in dry-run):
complete.properties.apply:
  description: >-
    Present as `dry-run` when a confirm action forced plan-only mode. Absent
    means apply normally. Derived from answers + template; not persisted.
  enum: [ dry-run ]
```

**`interview-protocol.yml`** — document the `stopped` status (writes no files,
runs no hooks), the `trigger` field, and the `complete.apply: dry-run` field.
Explicitly distinguish the *engine* dry-run (declared in the template, rides
through completion, surfaced as `apply: dry-run`) from the `apply --dry-run` CLI
flag and from ordinary planning.

**`command-line-interface.yml` / `.spec.yml`** — document that direct `apply`,
`continue`, and terminal runs honor a confirm stop (no plan/apply, exit `0`,
message) and a confirm dry-run (plan shown, no writes/hooks), the latter
equivalent to the `--dry-run` flag but template-driven.

---

## 6. Composition with the approved 1072 contract & sequencing

- Confirm-flow depends on the approved 1072 **contract**, not the unmerged 1056
  runtime: (a) the **answer-transaction atomicity invariant** — a rejected
  answer never enters probe/skip state and a rejected document never persists —
  which holds under today's emergent `Pending::answer` loop and under a future
  `SubmissionTxn`; (b) the **target-identity type** at identity consumers,
  which confirm-flow does not widen or bypass (it builds no plan on stop and
  reuses the single `canonical_target`).
- **Producer composition (origin-bearing `Resolution`).** Wherever confirm-flow
  threads configured-default consumption — `Interview::start`, replay, headless —
  it routes through the origin-bearing consuming `Resolution`
  (`Resolution::warnings()`, `Resolution::start(template, now)`,
  `StagedRecord::replay_with_resolution(template, resolution)`), carrying origins
  to the boundary. It does **not** reconstruct configured origins, does **not**
  re-derive an id-only key from the flat projection, and does **not** call
  `into_flat_defaults` (the sole sanctioned provenance-shedding escape). The
  ordinary flat `Seed` route is unchanged and independent. In practice
  confirm-flow adds *no new* configured-default consumption; it only preserves
  the existing route while the result type it returns gains the `Stopped` arm.
- **Sequencing.** The engine change (the `Stopped` variant + the walk funnel) is
  independent of 1056 and can land first. The paired implementation (1062)
  states this in the design and, at merge, consumes whichever `Resolution`
  surface exists — targeting the origin-bearing form, never baking in the flat
  projection. Surface at the Phase C checkpoint.
- Honors all four 1072 "do not" constraints: no second target normalizer, no
  reconstructed origins, no rejected answer in probe state, no different
  skip/default policy (skip-rest/skip-group reuse the existing skip path).

---

## 7. Behaviors to prove (falsifiable)

**Declaration / load**
1. `on` on a `text` question fails to load with `on only allowed on confirm`.
2. `skip-group` on a top-level confirm fails to load with the corrective message.
3. An unknown action value fails to load naming the four allowed values.
4. A confirm with no `on` produces byte-identical output to the pre-feature build
   for the same answers (pure-boolean preservation).

**Stop**
5. `when_false: stop`, answered `false` ⇒ `Interview::Stopped`; target directory
   is unchanged and no hook ran (assert filesystem + recording runner empty).
6. Stop across all five drivers (terminal, headless, staged, direct, crate)
   yields the same decision; headless and staged emit byte-equal `stopped`
   documents (driver parity).
7. A confirm mapped to stop but answered the *other* value completes normally.
8. A confirm sharing a batch with a later question: stop fires on the step that
   submits the confirm; the later question's answer is recorded but never
   applied; no files/hooks.

**Dry-run**
9. `when_true: dry-run`, answered `true` ⇒ interview `Complete`,
   `plan_only() == Some(trigger)`; plan is built and shown, no writes, no hooks.
10. Direct `apply` with the flag `--dry-run` and with the template dry-run
    produce the same plan output.
11. Answering the dry-run confirm `false` applies normally (writes + hooks).

**Skip**
12. `skip-rest` on a confirm skips every later question; each records its
    default/empty; a held answer for a skipped question emits the existing
    unused warning; interview completes.
13. `skip-group` inside a nested group skips only the enclosing group's
    remaining questions; questions after the group are asked normally.
14. Skip composes with `when`: an inactive confirm (its `when` is false) never
    fires its action.

**Resume / replay / determinism**
15. Stage → continue across a stop reproduces `Stopped` from stored submissions
    alone; the staged record contains no action/mode/control field.
16. Replaying a dry-run interview re-derives `plan_only()` identically; a
    submission after a stopped interview is the terminal-replay error.

**Negative / atomicity**
17. A document that both fires a confirm stop and carries a rejected answer
    returns `AnswerError::Rejected` and produces no `Stopped` and no `dry_run`;
    no document persists.
18. An early-answer failure for a question the interview stopped before reaching
    is dropped (does not stand), mirroring the completed-interview rule.
