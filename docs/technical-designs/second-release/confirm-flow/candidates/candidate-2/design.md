# Confirm-controlled interview and apply flow — candidate design (directive on `Completed`)

Structural shape: the interview keeps its two states, `Interview::{Asking, Complete}`.
It always runs to a `Completed`, which gains one control field — a **directive**
(`proceed` / `stop` / `dry-run`) — describing the outcome. **skip** is not a
directive: it is realized entirely through the engine's existing walk skip
machinery (`Advance::skip`, `Skipped`, group recursion), so a skipped region
records defined default/empty answers under the existing `warn_unused`
semantics. Drivers branch on the directive at the apply boundary; `Plan::build`
stays always-safe and a stop/dry-run simply never enters the apply effect loop.

---

## 1. Usage (caller's view, first)

### 1.1 README excerpt — declaring a confirm action

A `confirm` question stays a pure boolean unless you add the optional `action`
key. Actions are declared data, mapped per truth-value, never inferred from the
prompt text.

```yaml
# template.yml
name: service
interview:
  - id: preview_only
    type: confirm
    prompt: Preview the plan without writing?
    action:
      when-true: dry-run          # answer true -> build + show plan, write nothing

  - id: proceed
    type: confirm
    prompt: Continue?
    action:
      when-false: stop            # answer false -> end now, write nothing, run no hook

  - name: database                # a group
    when: "{{ use_db }}"
    nodes:
      - id: want_db_extras
        type: confirm
        prompt: Configure database extras?
        action:
          when-false: { skip: group }   # skip the rest of THIS group only
      - id: db_pool_size
        type: text
        prompt: Pool size?

  - id: want_more
    type: confirm
    prompt: Answer the remaining optional questions?
    action:
      when-false: { skip: interview }   # skip every remaining reachable question
```

Vocabulary (closed):

| Form | Meaning |
| --- | --- |
| `stop` | End the interview; write no files, run no hooks. |
| `dry-run` | Complete the interview; build and show the plan; write nothing, run no hooks. |
| `{ skip: group }` | Record defaults/empties for the rest of the **enclosing group**, then resume. |
| `{ skip: interview }` | Record defaults/empties for the **whole remaining interview**. |

Ordinary confirms (no `action`) behave exactly as today: `Typed<bool>` default
-> `Answer::Bool` -> JSON boolean.

### 1.2 Call site A — crate / library caller

```rust
use toha::{Interview, Seed, Plan, ApplyOptions, Directive, Realized, realize};

let interview = Interview::start(&template, Seed { now, defaults })?;
let completed  = /* drive via terminal::drive | protocol::answer_headless | replay */;

// Deep interface: one call gates every effect on the directive. This is the
// single place "no writes/hooks on stop or dry-run" is enforced.
match realize(&template, &completed, target, ApplyOptions::default(), &runner)? {
    Realized::Stopped { by }          => eprintln!("stopped at `{by}`: nothing written"),
    Realized::Previewed { plan, by }  => show(&plan),          // dry-run: built, not applied
    Realized::Applied(applied)        => report(applied),      // proceed: normal apply
}

// Or manual control, with the directive as the single source of truth:
if let Some(id) = completed.directive.stopped_by() {
    return Ok(());                                   // stop: never build, never apply
}
let plan = Plan::build(&template, &completed, target)?;   // always safe
if completed.directive.writes_allowed() {                // false for stop and dry-run
    plan.apply(target, ApplyOptions::default(), &runner)?;
}
```

The whole confirm->action machine is hidden behind `completed.directive`. A
caller that ignores the field applies exactly as before (default is `Proceed`).

### 1.3 Call site B — terminal (interactive `apply PATH`)

The user answers `Continue? = no`. `terminal::drive` returns a `Completed`
whose `directive == Stop { by: "proceed" }`. `run` in `main.rs` reads the
directive and, instead of entering the apply/plan-print path, prints a one-line
notice and exits `0`:

```text
$ toha apply ./out
Continue? no
stopped at `proceed`: no files written, no hooks run
$ echo $?
0
```

`Preview the plan without writing? = yes` yields `directive == DryRun`, which
routes into the **existing** `--dry-run` plan-print path (build + show, no
apply). It composes with the CLI `--dry-run` flag: either source previews.

### 1.4 Call site C — headless wire protocol (`stage --async` / `continue --answers`)

The interview completes, so the wire status stays `complete`. The directive is
carried as an **additive** field on the closed complete document:

```jsonc
// continue emits, after answering { "proceed": false }:
{
  "protocol": 1,
  "status": "complete",
  "context": { "target": "/abs/out", "template": "service", "commit": null },
  "answers": { "preview_only": false, "proceed": false, "want_more": null },
  "messages": [],
  "outcome": "stop",          // absent when proceed -> byte-identical to today
  "outcome-by": "proceed"
}
```

An agent reads `outcome`: `stop` -> do not run `apply`; `dry-run` -> run `apply
--dry-run` (or expect no writes); absent/`proceed` -> `apply`. `apply --answers`
on such a completion refuses to write when `outcome == stop` and previews when
`outcome == dry-run`.

---

## 2. Data and type sketches

### 2.1 The directive (new) — the one carried decision

```rust
// interview.rs
/// The control outcome an interview reaches. `Proceed` is ordinary and the
/// only outcome that permits writing files or running hooks. Derived by the
/// walk from the recorded confirm answers; never persisted (see §6).
///
/// Invariant (monotone): a walk only ever escalates the directive
/// `Proceed < DryRun < Stop`; it never downgrades. `Stop` suppresses any
/// later confirm action (see `Advance::fire_action`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Directive {
    #[default]
    Proceed,
    /// A confirm answer requested a stop. The interview still reached a full
    /// `Completed` (the remainder was skip-recorded); callers must not build
    /// or apply.
    Stop { by: Id },
    /// A confirm answer requested a dry run. The interview ran to completion;
    /// callers build and show the plan but must not apply.
    DryRun { by: Id },
}

impl Directive {
    /// Whether files may be written / hooks run. True only for `Proceed`.
    pub fn writes_allowed(&self) -> bool { matches!(self, Directive::Proceed) }
    /// Whether a plan should be built and shown. True for `Proceed` and `DryRun`.
    pub fn plan_shown(&self) -> bool { !matches!(self, Directive::Stop { .. }) }
    /// The confirm that requested a stop, if any.
    pub fn stopped_by(&self) -> Option<&Id> {
        if let Directive::Stop { by } = self { Some(by) } else { None }
    }
    /// The confirm that requested a dry run, if any.
    pub fn previewed_by(&self) -> Option<&Id> {
        if let Directive::DryRun { by } = self { Some(by) } else { None }
    }
    /// Raises the directive to the higher of the two, per the monotone invariant.
    fn escalate(&mut self, other: Directive) { unimplemented!("max(Proceed<DryRun<Stop)") }
}
```

Policy (directive -> effect) lives here, so every driver routes identically
through `writes_allowed` / `plan_shown`; no driver re-implements the mapping
(per boundary-discipline; per the task's "route the outcome, don't
re-implement policy").

### 2.2 `Completed` gains one field (the load-bearing decision)

```rust
// interview.rs:61 — additive; the two Interview variants are unchanged.
pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    /// The control outcome. `Proceed` unless an active confirm's answer mapped
    /// to `stop` or `dry-run`. Single source of truth for the outcome;
    /// recomputed on every walk from the recorded answers.
    pub directive: Directive,
    skipped: Skipped,
    step_start: usize,
}
```

Every existing `match interview { Asking | Complete }` site keeps compiling
untouched (`main.rs:611/716/919/981`, `staging.rs:143`, `terminal.rs:211`,
`protocol.rs:214`). Only sites that act on stop/dry-run read `directive`. This
is the deep-interface payoff of a field over a third enum variant (§ rationale).

### 2.3 Template declaration — action on the confirm variant

```rust
// template.rs:114 — additive to the variant; keeps `default` and the boolean answer intact.
QuestionKind::Confirm {
    default: Option<Typed<bool>>,
    /// The action fired when the answer is `true`; `None` on an ordinary confirm.
    on_true: Option<Action>,
    /// The action fired when the answer is `false`; `None` on an ordinary confirm.
    on_false: Option<Action>,
},

/// A control action a confirm answer maps to. Declared data, never inferred
/// from prompt text. Only `QuestionKind::Confirm` can carry one, so a
/// non-confirm question is structurally incapable of an action (encode-lessons-
/// in-structure); the load rule rejects the YAML before this type is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Stop,
    DryRun,
    Skip(SkipScope),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipScope { Interview, Group }
```

Placing the mapping on the `Confirm` variant (not on `Question`) makes "actions
are confirm-only" a type invariant rather than a runtime check.

### 2.4 Walk state — reuse the skip machinery, add a scope latch

```rust
// interview.rs:1011 — Advance gains a directive accumulator and a skip latch.
struct Advance<'a> {
    // ...existing fields...
    /// Escalated as active confirms fire terminal actions. Copied to
    /// `Completed.directive` when the walk completes.
    directive: Directive,
    /// Engaged when a confirm fires `skip` (or `stop`, which skips the whole
    /// interview). While engaged, the walk records defaults for reached
    /// questions via the existing `skip` branch. `Group(depth)` clears when the
    /// walk leaves that group's node list; `Interview` never clears.
    skip_scope: Option<Scope>,
}
/// Runtime skip scope. `Group` carries the nesting depth of the group whose
/// remainder is skipped, so it can clear on exit.
enum Scope { Interview, Group(usize) }
```

### 2.5 The crate apply-boundary funnel (new, deep, tiny surface)

```rust
// lib.rs / a new apply seam — the single place the directive gates effects.
/// Realizes a completed interview against `target`. Builds the plan unless the
/// interview stopped; applies it only when the directive is `Proceed`. This is
/// the one function that turns a directive into (no) effects, so every driver
/// and every crate caller shares one policy.
///
/// `Plan::build` is pure w.r.t. the target; only `Proceed` reaches
/// `Plan::apply`, the sole write/hook site.
pub fn realize(
    template: &Template,
    completed: &Completed,
    target: &Path,                 // &CanonicalTarget post-1056; never normalized here
    options: ApplyOptions,
    runner: &dyn HookRunner,
) -> Result<Realized, RealizeError> {
    unimplemented!("branch on completed.directive: Stop -> Stopped; DryRun -> build only; Proceed -> apply")
}

/// The outcome of `realize`. Wraps `Applied` rather than adding variants to it,
/// keeping the confirm-directive concern separate from apply's trust concern.
#[derive(Debug)]
pub enum Realized {
    /// Interview stopped: nothing built, nothing written.
    Stopped { by: Id },
    /// Dry run: the plan was built and returned, not applied.
    Previewed { plan: Plan, by: Id },
    /// Ordinary apply outcome (`Applied::Written` or `Applied::NeedsTrust`).
    Applied(Applied),
}
```

`Applied` (`apply.rs:18`) is unchanged: stop/dry-run never enter `apply` at
all, so they are not apply outcomes. `Realized` wraps it one level up.

---

## 3. Function signatures (deltas only)

```rust
// interview.rs — the single funnel: an active, answered confirm reached by the walk.
impl<'a> Advance<'a> {
    /// Applies the action of an active, answered confirm `q` reached at group
    /// nesting `depth`. Fires at most once per confirm per walk and only when
    /// no skip scope is engaged and the directive has not stopped. `stop`
    /// engages an interview skip and sets `Stop`; `dry-run` sets `DryRun`
    /// without skipping (so the plan is complete); `skip` engages the scope.
    ///
    /// Invariants: never called for a skipped/inactive confirm (the walk's
    /// `when`/skip branches route those to `skip`), nor for a rejected answer
    /// (the `Pending::answer` transaction gate precedes `advance`).
    fn fire_action(&mut self, q: &Question, depth: usize) -> Result<(), EvalError> {
        // TODO
        // if self.skip_scope.is_some() { return Ok(()); }           // suppressed inside a skip
        // if matches!(self.directive, Directive::Stop { .. }) { return Ok(()); }
        // let (t, f) = confirm actions of q else return Ok(());
        // let Some(Answer::Bool(b)) = self.answers.get(&q.id) else { return Ok(()); };
        // match if *b { t } else { f } {
        //   None => {}
        //   Some(Action::Stop)   => { self.directive.escalate(Directive::Stop { by: q.id.clone() });
        //                             self.skip_scope = Some(Scope::Interview); }
        //   Some(Action::DryRun) => self.directive.escalate(Directive::DryRun { by: q.id.clone() }),
        //   Some(Action::Skip(SkipScope::Interview)) => self.skip_scope = Some(Scope::Interview),
        //   Some(Action::Skip(SkipScope::Group))     => self.skip_scope = Some(Scope::Group(depth)),
        // }
        // Ok(())
        unimplemented!()
    }

    /// Effective skip at a node: the walk's local `skip` OR any engaged scope.
    fn skipping(&self, local: bool) -> bool { local || self.skip_scope.is_some() }
}

// walk gains `depth`; two touch points call the funnel. (interview.rs:1054)
fn walk(&mut self, nodes: &'a [Node], prefix: &str, skip: bool, depth: usize)
    -> Result<bool, EvalError> {
    // For Node::Question(q):
    //   at the already-answered branch (`:1059`): before `continue`, if
    //   `!self.skipped.contains(&q.id)` call `self.fire_action(q, depth)?`.
    //   at the active held-applied branch (after `:1098` insert): call
    //   `self.fire_action(q, depth)?` before `continue`.
    //   everywhere `skip` gated a record, use `self.skipping(skip)`.
    // For Node::Group(g): recurse with `depth + 1`; after it returns, if
    //   `self.skip_scope == Some(Scope::Group(d))` and `d >= depth + 1`, clear it.
    unimplemented!()
}

// advance copies the accumulator onto the result. (interview.rs:1297)
//   Completed { /* ... */, directive: state.directive, /* ... */ }

// protocol.rs — complete document carries the directive additively.
pub fn complete_document(completed: &Completed, context: &Context) -> Value {
    // as today, plus: if completed.directive != Proceed, insert
    //   "outcome": "stop"|"dry-run" and "outcome-by": <id>.
    unimplemented!()
}
```

No new function is added to the terminal/staged drivers: `terminal::drive`
still returns `Completed` (it now carries the directive); `main.rs` routes on
`completed.directive.writes_allowed()` / `plan_shown()` into its **existing**
plan-print vs apply branches.

---

## 4. Module / seam diagram

```
template.yml (+ action: when-true/when-false)
   │  Template::load  ── load rules (beside loop/options, template.rs:618) reject:
   │                     action on non-confirm; skip:group outside a group; empty action
   ▼
QuestionKind::Confirm { default, on_true, on_false }        [template.rs]
   │
   ▼
Advance::walk  ── reaches an ACTIVE, ANSWERED confirm ──► Advance::fire_action   ★ ONE FUNNEL
   │   (after `when`, after held-answer check; never for skipped/rejected)   │
   │                                                                          ├─ Stop    → skip_scope=Interview + directive=Stop
   │   skip machinery (Advance::skip / Skipped / group recursion) records     ├─ DryRun  → directive=DryRun (no skip)
   │   defaults for the skipped region under warn_unused semantics            └─ Skip(scope) → skip_scope engaged
   ▼
advance ──► Interview::Complete(Completed { directive, .. })   [sole result constructor, interview.rs:1276]
   │
   ├── crate            → realize()  ─┐
   ├── terminal (run)   → directive.writes_allowed()/plan_shown() ─┤   Plan::build (always safe)
   ├── direct (apply)   → realize() / same predicates             ─┤        │ only Proceed →
   ├── headless (wire)  → complete_document + "outcome"/"outcome-by"┤   Plan::apply  (sole write/hook site, apply.rs:66)
   └── staged (continue)→ replay recomputes directive; wire outcome ┘
```

Trace depth from input to effect is three files (template -> interview ->
apply-boundary), per minimize-reader-load.

---

## 5. Error / results contract

- **Load time (preferred), `Template::load`** — beside the `loop`/`options`
  rules (`template.rs:618-638`):
  - `action` on a non-confirm question ->
    `"<path>.action": action only allowed on confirm`.
  - `{ skip: group }` on a confirm not inside any group ->
    `"<path>.action": skip: group requires the question to be inside a group; use skip: interview`.
  - an `action` with neither `when-true` nor `when-false` ->
    `"<path>.action": action must map when-true or when-false`.
  - unknown verb / malformed scope -> rejected by the closed schema before the
    load rules run.
  Every message names the corrective form. All are collected into the existing
  `LoadError { problems }`.
- **Run time** — a confirm action changes no error type. `fire_action` returns
  `EvalError` only if reading the confirm answer is itself a template fault
  (it is not, for a recorded `Answer::Bool`); it is effectively infallible.
- **Atomic transaction (1072 contract)** — the funnel runs inside `advance`,
  which `Pending::answer` reaches only after its rejection gate
  (`interview.rs:1495` returns before `advance` on any rejection). So a rejected
  confirm answer never fires an action, and a rejected document never persists
  (composes with policies 1 and the "do not accept a rejected answer into probe
  state" constraint).
- **Results** — `Directive::Stop` and `DryRun` are both **complete** interviews:
  wire `status` stays `complete`; the outcome rides on `outcome`/`outcome-by`.
  Exit codes: a chosen stop is success (`0`), matching "the user asked to
  stop"; dry-run reuses the existing dry-run exit path. Terminal/direct print a
  one-line notice to stderr and exit `0`.
- **Driver parity** — for identical submissions, terminal / headless / staged /
  crate produce byte-equal complete documents (including `outcome`) and, on
  `Proceed`, byte-equal target trees. On `Proceed` with no action anywhere, the
  complete document is byte-identical to today (the `outcome` key is omitted).

---

## 6. Determinism and composition

- **No persisted control state.** `StagedRecord` (`staging.rs:19`) still stores
  only raw `submissions` + identity + frozen `now`. The directive is recomputed
  by `advance` on every replay from the same submissions (`replay` /
  `replay_with_resolution`), so resume reproduces the identical directive and
  answers. (Composes with the replay-determinism invariant.)
- **`when` / default / held / nested groups.** The funnel fires only in the
  walk's *active* branch, i.e. after `when` evaluated true and after the held-
  answer check applied the value; a `when`-inactive or ancestor-skipped confirm
  is routed to `self.skip` and never fires. A confirm answered by its
  configured/question default (omitted in the document, default taken at
  `interview.rs:1455-1471`) is still active-and-answered, so its action fires on
  the default value — the default *is* the answer.
- **Batch siblings.** A confirm and later questions can share a batch. When the
  confirm fires `stop`/`skip`, `skip_scope` engages; later same-batch questions
  already carry their submitted answers, but the funnel is suppressed for them
  (`skip_scope.is_some()`), so their actions do not fire. The user's stop/skip
  decision wins, deterministically from the submission order.
- **Predecessor sequencing (error-attribution 1072 / impl 1056).** Confirm-flow
  depends on the approved 1072 **contract**, not on merged 1056 runtime:
  1. the **answer-transaction atomicity invariant** — the funnel sits behind the
     rejection gate, true of today's emergent `unless_skipped`/`advance` loop and
     of the future `SubmissionTxn`/`SkipDisposition` alike;
  2. the **target-identity type** — `realize`/`Plan` take the identity type
     (`&Path` now, `&CanonicalTarget` post-1056) and never normalize (no second
     normalizer; composes with policy 5 and the "do not add a second target
     normalizer" constraint).
- **Producer-composition correction (configured defaults).** Wherever confirm-
  flow touches configured-default consumption — interview **start**, **replay**,
  **headless** — it routes through the **origin-bearing consuming `Resolution`**
  (`Resolution::warnings()`, `Resolution::start(template, now)`,
  `StagedRecord::replay_with_resolution(template, resolution)`), the final
  1072/1056 contract; it does **not** consume the flat a62061f
  `Resolution { defaults, warnings }` projection (that is grounding-only). It
  never reconstructs configured origins and never re-derives an id-only key from
  a flattened map; `into_flat_defaults()` remains the only sanctioned
  provenance-shedding escape, and the ordinary flat `Seed` route is unchanged and
  independent. Confirm-flow reads the confirm's recorded `Answer::Bool`
  regardless of the answer's origin and inspects no provenance, so it composes
  with either resolution route without touching origin attribution.

---

## 7. Proposed canonical-document edits (described, owned by the paired implementation 1062)

1. **`template-format.schema.yml` `$defs/question`** — add an optional `action`
   property (keeping `additionalProperties: false`, keeping the `type` enum
   unchanged; `action` is schema-legal on any question and load-rejected off
   confirm, mirroring `loop`/`options`):
   ```yaml
   action:
     type: object
     additionalProperties: false
     minProperties: 1
     properties:
       when-true:  { $ref: "#/$defs/action" }
       when-false: { $ref: "#/$defs/action" }
   ```
   and a new `$defs/action`:
   ```yaml
   action:
     oneOf:
       - enum: [ stop, dry-run ]
       - type: object
         additionalProperties: false
         required: [ skip ]
         properties:
           skip: { enum: [ interview, group ] }
   ```
2. **`template.rs` load rules** — beside `loop`/`options`
   (`template.rs:618-638`): reject `action` on a non-confirm, `skip: group`
   outside a group, and an empty `action`, with the messages in §5.
3. **`template-format.yml`** — prose: confirm actions are declared data, mapped
   by `when-true`/`when-false`; vocabulary `stop` / `dry-run` / `skip:
   interview|group`; and the three load rejections added to `validation-criteria`
   beside the `loop`/`options` sentences.
4. **`interview-protocol.schema.yml` `$defs/complete`** — add optional
   `outcome` (`enum: [proceed, stop, dry-run]`) and `outcome-by`
   (`$ref: identifier`), keeping `additionalProperties: false` and `protocol: 1`.
   Additive: omitted on `proceed`.
5. **`interview-protocol.yml`** — prose: a complete document may carry
   `outcome`/`outcome-by`; `stop` writes no files and runs no hooks; `dry-run`
   builds and shows the plan but does not apply and is distinct in origin from
   the `apply --dry-run` CLI flag (which they compose with); confirm-driven
   `skip` reuses the `warning: answer for "<id>" was not used: the question was
   skipped` message and is orthogonal to `when`-based skip.
6. **`docs/template-interviews.md` / `docs/template-flow.md`** — usage guide:
   the `action` key, scopes, and the driver behaviors above.

---

## 8. Behaviors to prove (falsifiable)

Regression / parity:
- An ordinary confirm (no `action`) yields byte-identical answers, complete
  document (no `outcome` key), and target tree to the current build, across all
  five drivers.
- Driver parity: identical submissions produce byte-equal complete documents
  (including `outcome`) and byte-equal trees on `Proceed`, for terminal /
  headless / staged / direct / crate.

Actions, both truth values:
- `when-true: stop` and `when-false: stop`: no file written and no hook run
  (assert against the `fs::write`/`runner.run` sites) in every driver; complete
  document carries `outcome: stop`; exit code `0`.
- `when-*: dry-run`: plan built and shown, no writes, no hooks; distinct in
  `outcome` from a `--dry-run`-flag run of the same template; composes when both
  are present.

Skip scope + nesting:
- `skip: interview` records each subsequent reachable question's default/empty
  and completes with `Proceed`; a held answer for a skipped question emits the
  existing "was not used" warning once, in interview order.
- `skip: group` inside a group skips only that group's remainder; questions
  after the group are asked normally; a `skip: group` inside a deeply nested
  group clears on group exit and leaves outer siblings active.

Composition / determinism:
- A `when`-inactive confirm never fires its action, even when its recorded
  default would map to one.
- An early/held confirm answer fires the action when the question is reached,
  not before the `when` check.
- A rejected document that includes the confirm never fires an action and never
  persists; the atomic transaction is preserved.
- Replay from stored submissions reproduces the identical directive and answers
  (directive is not present in `StagedRecord`).
- Batch-sibling suppression: a confirm firing `stop`/`skip` in a batch with
  later same-batch questions suppresses their actions; no writes occur.
- Precedence: `dry-run` then a later `stop` completes with `Stop` (stop wins,
  nothing written); two `dry-run`s are idempotent.

Load-time negatives:
- `action` on a non-confirm is rejected at load with `action only allowed on
  confirm`.
- `skip: group` on a confirm outside any group is rejected with the corrective
  message.
- An empty `action` map is rejected.
- An unknown verb / malformed `skip` scope is rejected by the schema.
