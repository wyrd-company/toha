# Confirm-controlled interview and apply flow — candidate design (unified control-flow value)

**Structural direction:** a single derived control-flow value — `Directive` —
computed once at the engine funnel that has both the confirm question node and its
final `Answer::Bool`. Its terminal effects surface as first-class arms on the
result the answer transaction already returns. Ordinary confirms keep pure boolean
behavior; actions are declared data.

---

## 1. Caller's usage first (README view)

A template maps a **confirm** answer to a control **action** with an additive `on`
map on the question. The action is `stop`, `dry-run`, or `skip` (with a scope).
Ordinary confirms omit `on` and are unchanged.

```yaml
# template.yml  (interview excerpt)
interview:
  - id: proceed
    type: confirm
    prompt: "Scaffold into {{ target }}?"
    on:
      false: stop            # answering "no" ends the interview: no files, no hooks

  - id: preview
    type: confirm
    prompt: "Preview the plan without writing?"
    on:
      true: dry-run          # "yes" completes the interview, shows the plan, writes nothing

  - group: ci
    nodes:
      - id: want_ci
        type: confirm
        prompt: "Configure CI?"
        on:
          false: { skip: group }   # "no" skips the rest of THIS group only
      - id: ci_provider
        type: select
        prompt: "CI provider?"
        options: [github, gitlab]
```

The action is **data**, never inferred from the prompt text. `false: stop` fires
only because it is declared. A confirm with no `on` (or whose recorded value has no
mapped action) behaves exactly as today.

### Call site A — crate / library caller

The control action now lives on the value the driver returns, so a library
embedder observes it without any `main.rs` glue.

```rust
use toha::{Interview, Plan, Seed};
use toha::cli::terminal;

let interview = Interview::start(&template, seed)?;          // Seed OR Resolution route, unchanged
let outcome = terminal::drive(interview, &mut ask, record)?; // returns a terminal `Interview` arm

match outcome {
    Interview::Complete(done) => {
        let plan = Plan::build(&template, &done, &target)?;
        plan.apply(&target, opts, &runner)?;                 // writes + hooks
    }
    Interview::DryRun(done) => {
        let plan = Plan::build(&template, &done, &target)?;   // SAME plan, byte-for-byte
        show(&plan);                                          // no apply — no writes, no hooks
    }
    Interview::Stopped(stop) => {
        report_stop(&stop);                                   // no plan built at all
    }
    Interview::Asking(_) => unreachable!("drive runs to a terminal outcome"),
}
```

### Call site B — headless (wire protocol) caller

```rust
use toha::protocol::{answer_headless, Headless, complete_document, dry_run_document, stopped_document, batch_document};

let document = protocol::parse_answers(&stdin_text)?;
match answer_headless(&template, interview, document)? {
    Headless::Complete { completed, .. } => emit(complete_document(&completed, &ctx)),   // status: complete
    Headless::DryRun   { completed, .. } => emit(dry_run_document(&completed, &ctx)),    // status: dry-run
    Headless::Stopped  { stopped, .. }   => emit(stopped_document(&stopped, &ctx)),      // status: stopped
    Headless::Pending  { pending, rejections, .. } =>
        emit(batch_document(pending.batch(), &ctx, Some(&rejections))),                  // status: questions
}
```

Wire documents are the ONLY caller-visible surface for stop/dry-run in the headless
driver. `status` gains `stopped` and `dry-run` beside `questions`/`complete`.

### Call site C — staged driver (`stage` / `continue` / resume)

```rust
// `stage`/`continue` record raw submissions exactly as today — no new persisted field.
store.save(&record)?;

// Resume replays stored submissions; the terminal control outcome is DERIVED, never stored.
match record.replay_with_resolution(&template, resolution)? {   // origin-bearing Resolution route
    Interview::Asking(p)       => emit(batch_document(p.batch(), &ctx, None)),
    Interview::Complete(done)  => { /* apply path */ }
    Interview::DryRun(done)    => { /* plan-only path; single apply gate suppresses effects */ }
    Interview::Stopped(stop)   => emit(stopped_document(&stop, &ctx)),
}
```

### Call site D — terminal / direct `apply TEMPLATE PATH` (CLI)

The confirm-driven dry-run and the existing `apply --dry-run` **flag** meet at one
apply gate; either being set suppresses writes and hooks. Stop never reaches
plan-build.

```rust
let dry = dry_run_flag || matches!(outcome, Interview::DryRun(_));  // single source of truth
match outcome {
    Interview::Stopped(stop) => return Outcome::Stopped(stop.line()),     // no plan, no apply
    Interview::Complete(done) | Interview::DryRun(done) => {
        let plan = Plan::build(&template, &done, path)?;
        if dry { return Outcome::Written(plan_lines(&plan, force).collect()); }  // no apply
        plan.apply_reporting(path, opts, &runner, &mut report)?;                 // writes + hooks
    }
    Interview::Asking(_) => { /* emit batch, exit 4 — unchanged */ }
}
```

---

## 2. Data / type sketch (derived from the usage)

### 2.1 Declaration (public, `template.rs`)

```rust
/// A confirm question. `default` and the boolean answer are unchanged; `on`
/// is the additive, opt-in action map. `on == ConfirmActions::NONE` is an
/// ordinary confirm with pure boolean behavior.
QuestionKind::Confirm {
    default: Option<Typed<bool>>,
    on: ConfirmActions,
}

/// The control action mapped to each truth value of a confirm answer.
/// Absent entries mean "no action; behave as a plain boolean for that value".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConfirmActions {
    pub when_true: Option<ConfirmAction>,
    pub when_false: Option<ConfirmAction>,
}
impl ConfirmActions {
    pub const NONE: Self = ConfirmActions { when_true: None, when_false: None };
    /// The action declared for `value`, if any. Single source of truth for
    /// "which action does this confirm answer map to".
    pub fn action_for(&self, value: bool) -> Option<&ConfirmAction> {
        if value { self.when_true.as_ref() } else { self.when_false.as_ref() }
    }
}

/// A control action a confirm answer may fire. Declared data only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// End the interview immediately: build no plan, write no files, run no hooks.
    Stop,
    /// Finish the interview, build and show the plan, but suppress apply.
    DryRun,
    /// Skip questions in `scope`, recording each with its ordinary
    /// default/empty answer and the existing "answer was not used" warning.
    Skip(SkipScope),
}

/// How far a `skip` reaches. At interview top level, `Group` equals `Interview`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipScope {
    /// The entire remaining interview.
    Interview,
    /// The rest of the nearest enclosing group (the walk frame the confirm is in).
    Group,
}
```

Invariant encoded in types: an action is reachable **only** through `ConfirmActions`
on `QuestionKind::Confirm`. No other kind can carry one — the field does not exist
elsewhere (`encode-lessons-in-structure`).

### 2.2 The unified outcome (public, `interview.rs`)

```rust
/// The result the sole constructor (`advance`) produces and the answer
/// transaction returns. `Asking`/`Complete` are unchanged states; `Stopped`
/// and `DryRun` are the first-class terminal control outcomes a confirm
/// action fires. Every driver matches these four arms and routes them.
#[derive(Debug)]
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),
    /// A confirm answer fired `stop`. Terminal. No plan, no files, no hooks.
    Stopped(Stopped),
    /// A confirm answer fired `dry-run`. The interview reached completion
    /// (all reachable questions answered); the carried `Completed` is
    /// byte-identical to what `Complete` would carry. Apply is suppressed.
    DryRun(Completed),
}

/// The record of an interview ended early by a confirm `stop`. Carries only
/// what a caller reports or replays; it is fully re-derivable from the stored
/// submissions, so nothing here is persisted as control state.
#[derive(Debug)]
pub struct Stopped {
    /// The confirm question whose answer stopped the interview.
    pub by: Id,
    /// The truth value that fired the stop.
    pub on: bool,
    /// Answers recorded up to the stop, in interview order.
    pub answers: Answers,
    /// Every message reached before the stop, in interview order.
    pub messages: Vec<String>,
    /// The messages reached by the step that stopped.
    pub last_messages: Vec<String>,
    pub now: jiff::Zoned,
}
```

`Completed` is **unchanged**; `DryRun(Completed)` reuses it exactly so plan-building
is shared and dry-run plan output is byte-equal to a normal completion
(`single source of truth per invariant`).

### 2.3 The directive — internal to `interview.rs` (never public, never persisted)

```rust
/// The control-flow decision derived once at the funnel. Pure function of the
/// question's declared `on` map and the committed answer. `Proceed` is the
/// pure-boolean path every ordinary confirm and every non-firing value takes.
enum Directive {
    Proceed,
    Skip(SkipScope),
    DryRun,
    Stop { by: Id, on: bool },
}

/// THE FUNNEL. Derives the directive for an active, answered question.
/// - Non-confirm, action-less confirm, `Answer::None`, non-bool  -> `Proceed`.
/// - A skipped confirm never reaches here (callers guard on `skipped`).
/// Depends on nothing but its two arguments -> replay-deterministic and
/// idempotent (`make-operations-idempotent`).
fn directive_of(q: &Question, answer: &Answer) -> Directive {
    unimplemented!("match Confirm{{ on }} + Answer::Bool -> action; else Proceed")
}

/// How the current apply disposition is carried while the walk proceeds.
/// Recomputed each walk from committed answers; not stored.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ApplyMode { #[default] Apply, DryRun }
```

### 2.4 Walk state additions (internal `Advance`, `interview.rs`)

```rust
struct Advance<'a> {
    // ...all existing fields unchanged...
    /// Set when a `dry-run` confirm has fired; folded to `Interview::DryRun`
    /// at completion. Recomputed each walk.
    mode: ApplyMode,
    /// Set when a `stop` confirm has fired; unwinds the walk and yields
    /// `Interview::Stopped`. Distinct from `blocked` (which waits for input).
    stopped: Option<Stopped>,
    /// Set by `skip: interview`; forces skip at every level of every frame
    /// for the rest of the walk. `skip: group` needs no field — it is the
    /// existing local `skip` of the current walk frame (see §3.2).
    skipping_all: bool,
}
```

---

## 3. How the decision is derived and composed (the seam)

### 3.1 One funnel, in `Advance::walk`

The single place that sees the question node and its final `Answer::Bool` is the
already-answered branch of the question arm (`interview.rs:1058`), reached for a
confirm answered in the committed batch, and — via the held-accept branch
(`interview.rs:1089`) on the first walk that records an early/held confirm. Both
route through **one** helper so the decision is expressed once
(`minimize-reader-load`):

```rust
Node::Question(q) => {
    if self.answers.contains_key(&q.id) {
        if self.skipped.contains_key(&q.id) {
            self.warn_if_held(&q.id);          // a skipped confirm NEVER fires
            continue;
        }
        match self.fire(q, prefix)? {          // THE FUNNEL, applied
            ControlFlow::Continue => continue,
            ControlFlow::StopWalk => return Ok(false), // stopped set; unwind
        }
    }
    // ...unanswered path unchanged; on held-accept it inserts then calls self.fire(...)
}

impl<'a> Advance<'a> {
    /// Applies the directive of an active, answered question. Sets `stopped` /
    /// `mode` / the frame skip, or does nothing for `Proceed`.
    fn fire(&mut self, q: &Question, prefix: &str) -> Result<ControlFlow, EvalError> {
        unimplemented!(
            "match directive_of(q, &self.answers[&q.id]):\n\
             Proceed  => Continue\n\
             DryRun   => {{ self.mode = ApplyMode::DryRun; Continue }}\n\
             Skip(Interview) => {{ self.skipping_all = true; Continue }}\n\
             Skip(Group)     => {{ set the current frame's local `skip` for the\n\
                                   remaining siblings; Continue }}\n\
             Stop{{by,on}}   => {{ self.stopped = Some(Stopped{{..}}); StopWalk }}"
        )
    }
}
```

`fire` acts on `self.answers[&q.id]`, which holds **only committed** values: a
rejected answer never enters `answers` (`interview.rs:1476-1483` inserts only `Ok`
values into `next`, merged before the walk). So the funnel cannot act on a rejected
answer — the atomicity invariant is honored structurally, not by a check
(`boundary-discipline`).

### 3.2 Skip scope reuses the existing skip machinery

`skip: group` sets the **local** `skip` of the current `walk` frame for its
remaining siblings. Because groups recurse via `walk(&g.nodes, key, !active)`
(`interview.rs:1187`), "the rest of the current frame" **is** the rest of the
enclosing group; when the frame returns, the parent resumes normally. At interview
top level the current frame is the whole interview, so `Group` there equals
`Interview` — no special case.

`skip: interview` sets `self.skipping_all`, consulted at the top of every node
iteration in every frame (`if self.skipping_all { treat this node as skip }`); it
never clears, so the remaining interview at all depths is skipped.

Either way a skipped question flows through the **existing** skip path
(`interview.rs:1065-1072`): `default_ready` → `render_default` → `Advance::skip`,
which records the ordinary default/empty answer, marks it in `Skipped`, and warns
any held answer (`interview.rs:1039-1052`). Confirm-skip therefore records **exactly
what `when`-skip records** and reuses `warn_unused`/`skipped_warning` verbatim — no
new skipped-value or warning rule is invented (grounding Avoid).

Relationship to `when`: `when` gates **reachability** (evaluated before the answer
exists); `on` gates the **action** of a committed answer. A `when`-inactive confirm
is in `Skipped`, so the funnel's `skipped.contains_key` guard means it can never
fire — "fire an action only after `when`" is enforced by ordering, not by a flag.
The two skip mechanisms never collide: `when`-skip populates `Skipped` during the
inactive branch; confirm-skip drives the same `Advance::skip` for reached-but-
directed nodes.

### 3.3 Terminal classification in `advance`

```rust
fn advance(mut state: Advance<'_>) -> Result<Interview<'_>, EvalError> {
    let complete = state.walk(&state.template.interview, "", false)?;
    // Stop is checked FIRST: it unwinds via Ok(false) with `stopped` set, which
    // must not be mistaken for a blocked (waiting) walk.
    if let Some(mut stopped) = state.stopped.take() {
        stopped.answers = state.answers;
        stopped.last_messages = state.messages[state.step_start..].to_vec();
        stopped.messages = state.messages;
        stopped.now = state.seed.now;
        return Ok(Interview::Stopped(stopped));
    }
    let has_prompt = state.batch.items.iter().any(|i| matches!(i, Item::Prompt(_)));
    if !complete && !has_prompt { /* unchanged unresolved-dependency error */ }
    if complete && !has_prompt {
        let completed = Completed { /* unchanged fields */ };
        return Ok(match state.mode {
            ApplyMode::DryRun => Interview::DryRun(completed),
            ApplyMode::Apply  => Interview::Complete(completed),
        });
    }
    Ok(Interview::Asking(Pending { /* unchanged */ }))
}
```

`mode` and `stopped` are set only from committed answers each walk, so a resume that
replays the same submissions reproduces the same terminal arm — determinism with no
persisted control state (`make-operations-idempotent`).

### 3.4 The atomic transaction is untouched

`Pending::answer` keeps its exact shape (`interview.rs:1392-1536`). The tentative
`advance` at `:1503` now also folds directives, but:

- If any **batch** answer is rejected, `batch_failed` returns **before** `:1503`
  (`:1495`) — a firing confirm that shares a rejected batch never advances, so its
  directive is never computed. A rejected answer cannot influence skip/reachability.
- When only early-answer failures remain, the tentative advance runs; a `Stop`
  yields `Interview::Stopped`, and `stands` treats any non-`Asking` terminal as
  "not standing," dropping the unreached early failure — consistent with the
  approved policy that an unreached probe's error is omitted. `stands` updates to:
  `Ok(Interview::Asking(next)) => !next.skipped.contains_key(id), _ => false`.
- A rejected document still returns `rejected(self, rejections)` (`:1531/1533`); the
  tentative outcome — including any directive it computed — is discarded. No leak.

---

## 4. Module / seam diagram

```
 template.rs ── declares ──►  QuestionKind::Confirm { default, on: ConfirmActions }
   parse: read `on`;  load rule: "on only on confirm" + vocabulary + scope checks
        │
        ▼
 interview.rs ─────────────  THE FUNNEL (seen once)  ─────────────
   directive_of(q, &Answer::Bool) -> Directive               (pure)
   Advance::fire  applies it:  Stop->stopped  DryRun->mode  Skip->frame/skipping_all
   advance():  stopped? -> Interview::Stopped
               complete & DryRun -> Interview::DryRun ; complete -> Complete ; else Asking
   Pending::answer:  atomic txn UNCHANGED; sees only committed answers; returns 4-arm Interview
        │            (rejected batch returns before the advance; rejected doc discards it)
        ▼
 DRIVERS (thin adapters — route the four arms, re-implement no policy)
   cli/terminal.rs  drive(..) -> Interview          (returns a terminal arm)
   protocol.rs      answer_headless -> Headless {Complete|DryRun|Stopped|Pending}
   staging.rs       replay_with_resolution -> Interview   (terminal arm DERIVED on replay)
   main.rs          run/apply: match arm; single apply gate `dry = flag || DryRun`
        │
        ▼
 plan.rs / apply.rs  UNCHANGED.  apply entered ONLY for Complete;
                     suppressed for DryRun (plan shown) and never reached for Stopped.
```

Call chain to trace a stop: `template.rs` (declare) → `interview.rs`
`directive_of`/`fire`/`advance` → driver match → apply gate. Three files, one seam
(`laziness-protocol`).

---

## 5. Function signatures (changed / added)

```rust
// template.rs — parse (extend existing confirm arm at template.rs:685)
"confirm" => QuestionKind::Confirm {
    default: self.typed(default, &format!("{path}.default")),
    on: self.confirm_actions(map.get("on"), &format!("{path}.on")),
},
/// Parses the `on` map. Records a `problem` (load-time) for an unknown action,
/// a bad skip scope, or an empty `on`. Absent -> `ConfirmActions::NONE`.
fn confirm_actions(&mut self, value: Option<&Value>, path: &str) -> ConfirmActions {
    unimplemented!("parse true/false -> ConfirmAction; push problems naming the corrective form")
}

// template.rs — load rule (beside the loop/options rules at template.rs:618-636)
if map.contains_key("on") && kind_name != "confirm" {
    problem(&mut self.problems, format!("{path}.on"), "on is only allowed on confirm");
}

// interview.rs — sole constructor, WIDENED return type (same signature)
fn advance(state: Advance<'_>) -> Result<Interview<'_>, EvalError> { unimplemented!() }
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError> { unimplemented!() }
}
impl<'a> Pending<'a> {
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>> { unimplemented!() }
}

// cli/terminal.rs — drive now returns the terminal Interview arm, not bare Completed
pub(crate) fn drive<'a>(
    interview: Interview<'a>,
    ask: &mut impl Ask,
    accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>,
) -> Result<Interview<'a>, String> { unimplemented!("loop until a terminal arm; return it") }

// protocol.rs — Headless gains the two terminal control arms
pub enum Headless<'a> {
    Complete { completed: Completed, accepted: Vec<IndexMap<String, Value>> },
    DryRun   { completed: Completed, accepted: Vec<IndexMap<String, Value>> },
    Stopped  { stopped: Stopped,     accepted: Vec<IndexMap<String, Value>> },
    Pending  { pending: Box<Pending<'a>>, rejections: Rejections, accepted: Vec<IndexMap<String, Value>> },
}
pub fn dry_run_document(completed: &Completed, context: &Context) -> Value { unimplemented!() }
pub fn stopped_document(stopped: &Stopped, context: &Context) -> Value { unimplemented!() }

// staging.rs — replay returns the widened Interview; a terminal control arm mid-record
// (a submission after stop) is a corrupt record -> StagingError::Replay.
impl StagedRecord {
    pub fn replay_with_resolution<'a>(
        &self, template: &'a Template, resolution: crate::Resolution,   // origin-bearing route
    ) -> Result<Interview<'a>, StagingError> { unimplemented!() }
}
```

---

## 6. Error / results contract

- **Load time (preferred).** New `problem`s, all additive, each naming the
  corrective form: `on is only allowed on confirm`; `action must be one of: stop,
  dry-run, skip`; `skip scope must be "interview" or "group"`; `on must map "true"
  or "false" to an action`. No incompatible terminal combination exists — `true`
  and `false` map independently — so no cross-value rejection is needed.
- **Runtime.** No new error variants. `Stopped`/`DryRun` are `Ok` outcomes.
  `AnswerError`, `EvalError`, `StagingError`, `ApplyError` are unchanged.
- **Atomicity.** A rejected document never persists and never fires a directive
  (§3.4). A rejected answer never enters probe/skip classification.
- **Determinism / persistence.** `StagedRecord.submissions` is unchanged; **no**
  control state is persisted. The terminal arm is re-derived from stored submissions
  on every replay. Running a step twice yields the identical arm (idempotent).
- **Predecessor (1072) coupling.** Confirm-flow depends on the approved 1072
  **contract**, not its unmerged runtime:
  - It reads only committed `Answer` values and never normalizes a target, so it
    adds **no** second normalizer and is order-independent w.r.t. 1056: the target
    passes through as `&Path` today or `&CanonicalTarget` post-1056 with no change
    to this design.
  - Configured-default consumption stays on the **origin-bearing consuming
    `Resolution`** (`Resolution::warnings()`, `Resolution::start(template, now)`,
    `StagedRecord::replay_with_resolution(template, resolution)`). Confirm-flow adds
    no default consumption of its own and never flattens: it does **not** reconstruct
    configured origins and does **not** re-derive an id-only key from the flat
    projection. `into_flat_defaults()` remains the only sanctioned provenance-shedding
    escape; the ordinary flat `Seed` route is unchanged and independent.
  - Sequencing: the paired implementation (1062) lands independent of 1056; its
    coupling is expressed as the answer-transaction atomicity invariant (holds in
    today's emergent loop and in the future `SubmissionTxn`) plus the target-identity
    type (holds for `&Path` and `&CanonicalTarget`).

---

## 7. Proposed canonical-document edits (described; owned by the paired implementation)

### 7.1 `template-format.schema.yml` — `$defs/question`

Add an `on` property (the object stays `additionalProperties: false`):

```yaml
on:
  description: >-
    Maps a confirm answer to a control action. Allowed only on `type: confirm`.
    Absent means the confirm is a plain boolean.
  type: object
  additionalProperties: false
  minProperties: 1
  properties:
    "true":  { $ref: "#/$defs/confirm-action" }
    "false": { $ref: "#/$defs/confirm-action" }
```

Add `$defs/confirm-action`:

```yaml
confirm-action:
  oneOf:
    - enum: [ stop, dry-run ]
    - type: object
      additionalProperties: false
      required: [ skip ]
      properties:
        skip: { enum: [ interview, group ] }
```

`template-format.yml` (guide): document `on`, the three actions, skip scope
(`interview` vs `group`, nested), that actions are declared data never inferred from
prompt text, and the non-collision with `when` (reachability vs action). Add the
load-rule prose beside the existing `loop`/`options` "only allowed on" rules.

### 7.2 `interview-protocol.schema.yml` — statuses

Extend the top `oneOf` to four and add two closed documents:

```yaml
oneOf:
  - $ref: "#/$defs/batch"
  - $ref: "#/$defs/complete"
  - $ref: "#/$defs/dry-run"
  - $ref: "#/$defs/stopped"
```

```yaml
dry-run:                     # interview finished; apply MUST be suppressed
  type: object
  additionalProperties: false
  required: [ protocol, status, context, answers, messages ]
  properties:
    protocol: { const: 1 }
    status:   { const: dry-run }
    context:  { $ref: "#/$defs/context" }
    answers:  { $ref: "#/$defs/answers" }
    messages: { $ref: "#/$defs/messages" }

stopped:                     # confirm ended the interview early; no apply at all
  type: object
  additionalProperties: false
  required: [ protocol, status, context, by, on, answers, messages ]
  properties:
    protocol: { const: 1 }
    status:   { const: stopped }
    context:  { $ref: "#/$defs/context" }
    by:       { $ref: "#/$defs/identifier" }   # the confirm that stopped
    on:       { type: boolean }                # the truth value that fired
    answers:  { $ref: "#/$defs/answers" }      # gathered so far
    messages: { $ref: "#/$defs/messages" }
```

`interview-protocol.yml` (guide): document `dry-run` and `stopped`, and that they
are terminal like `complete`; note that a confirm `dry-run` and the `apply
--dry-run` CLI flag both suppress apply and meet at one gate.

### 7.3 `command-line-interface.yml`

Note that `apply` may terminate via a confirm `stop` (exit reporting) or complete in
confirm-driven `dry-run` (plan shown, nothing written), distinct from the
`--dry-run` flag though sharing its suppression.

---

## 8. Behaviors to prove (falsifiable)

**Declaration / load (negative, at load time).**
1. `on` on a non-confirm question → load error `on is only allowed on confirm`.
2. `on.true: frobnicate` → load error naming `stop, dry-run, skip`.
3. `on.false: { skip: everything }` → load error naming `interview` or `group`.
4. `on: {}` → load error `on must map "true" or "false" to an action`.
5. A confirm with `on` absent renders byte-identical interview/plan/apply to the
   same template with the `on` key removed (ordinary behavior preserved).

**Stop (both truth values).**
6. `on: { false: stop }`, answer `false` → `Interview::Stopped { by, on:false }`;
   target directory unchanged (no files), hook runner never invoked.
7. `on: { true: stop }`, answer `true` → stop; answer `false` → normal completion +
   apply.
8. Stop mid-interview: later questions are never prompted; `Stopped.answers`
   contains only answers up to the stop.

**Dry-run (both truth values).**
9. `on: { true: dry-run }`, answer `true` → `Interview::DryRun(done)`; `Plan::build`
   over `done` equals the plan for `Complete(done)` with identical answers
   (byte-equal); no writes, no hooks.
10. Confirm dry-run and `apply --dry-run` flag together → still exactly one plan
    shown, no double suppression error.

**Skip scope + nesting.**
11. `on: { false: { skip: group } }` inside group G, answer `false` → the rest of G
    is skipped; questions after G proceed normally; each skipped question records its
    ordinary default/empty answer and emits the existing "answer was not used"
    warning for any held answer.
12. Same confirm at interview top level → skips the rest of the interview
    (Group==Interview at top).
13. `on: { false: { skip: interview } }` inside a deeply nested group → every
    remaining question at every level is skipped; interview reaches
    `Complete`/`DryRun`, not `Asking`.
14. A `when`-inactive confirm with `on: { true: stop }` → never fires (it is in
    `Skipped`); interview proceeds.

**Composition / determinism.**
15. A confirm answered by a **configured default** (origin-bearing `Resolution`
    route) that maps to an action fires that action; provenance is not reconstructed
    and warnings still flow through `Resolution::warnings()`.
16. Resume from a staged record whose submissions include a firing confirm →
    `replay_with_resolution` reproduces the identical terminal arm; a submission
    stored after a stop → `StagingError::Replay`.
17. Rejected batch sibling alongside a firing confirm → document rejected; no
    directive fires; interview state unchanged (atomicity).
18. An early/held answer that fails, in a document whose confirm fires stop → the
    unreached failure is dropped, the interview stops (no spurious rejection).

**Driver parity (byte-equal results).**
19. The same template + submissions driven through terminal, headless, staged-resume,
    and direct `apply` produce equivalent terminal outcomes: stop → `stopped`
    document / `Interview::Stopped` with equal `by`/`on`/`answers`; dry-run →
    `dry-run` document / `Interview::DryRun` with equal answers and byte-equal plan.
20. Headless multi-submission document where a middle submission's confirm fires stop
    → `Headless::Stopped` with `accepted` listing only submissions applied before the
    stop.
```
