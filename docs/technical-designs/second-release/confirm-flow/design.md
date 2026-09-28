---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
    - command-line-interface
---

# Confirm-controlled interview and apply flow

A template maps a `confirm` question's answer to a control **action** — **stop**,
**dry-run**, or **skip** — declared as data on the question. The action is an
effect the interview walk emits at the confirm's node position, in the same place
the walk already emits reached messages, hooks, skips, and recorded answers. Stop
ends the interview as a terminal outcome that carries nothing to plan or apply;
dry-run completes the interview and marks it plan-only; skip records the ordinary
default/empty answers for a scoped remainder and continues. Ordinary confirms keep
pure boolean behavior and are unaffected unless an action is configured. Nothing is
inferred from prompt text.

Base: arena candidate C4 (node-level walk effect). C1 independently converged on
the same shape (terminal stop variant, dry-run field, in-walk skip). Grafts: the
crate/direct effect gate `realize` from C2; the pure funnel naming and
group-equals-interview clarity from C3. See `synthesis.md`.

## Caller's usage

### Declaring a confirm action (template author)

```yaml
interview:
  - id: overwrite_ok
    type: confirm
    prompt: "The target is not empty. Continue anyway?"
    action:
      when_false: stop          # "no" ends the run: no files, no hooks

  - id: preview_first
    type: confirm
    prompt: "Show the plan before writing?"
    action:
      when_true: dry-run        # "yes" completes, shows the plan, writes nothing

  - group: telemetry
    when: features.telemetry
    nodes:
      - id: configure_now
        type: confirm
        prompt: "Configure telemetry now?"
        action:
          when_false: { skip: group }   # "no" skips the rest of THIS group
      - id: telemetry_key
        type: text
        prompt: "Telemetry write key"

  - id: proceed
    type: confirm
    prompt: "Ready to scaffold?"
    action:
      when_false: { skip: rest }        # "no" skips every remaining question
```

An action arm value is `stop`, `dry-run`, or `{ skip: rest | group }`. At least one
of `when_true` / `when_false` is present. A confirm with no `action` records
`Answer::Bool` exactly as today. The action is declared data; it is never inferred
from the prompt text.

### Crate / library caller

The decision lives on the value the driver returns, so a library embedder observes
it without any command glue. Either match the outcome directly, or route every
effect through the one `realize` gate (graft from C2):

```rust
let interview = Interview::start(&template, Seed { now, defaults })?;
let outcome = drive_to_end(interview)?;             // terminal: Complete | Ended

// One deep call gates every effect: build unless stopped, apply only when proceeding.
match realize(&template, outcome, &target, ApplyOptions::default(), &runner)? {
    Realized::Stopped { by }         => eprintln!("stopped at `{by}`: nothing written"),
    Realized::Previewed { plan, by } => show(&plan),        // dry-run: built, not applied
    Realized::Applied(applied)       => report(applied),    // proceed: normal apply
}
```

A caller that wants manual control uses the same single-source predicates:

```rust
match outcome {
    Interview::Ended(ended) => return Ok(()),               // no Completed exists: cannot plan
    Interview::Complete(done) => {
        let plan = Plan::build(&template, &done, &target)?;  // always safe (no writes/hooks)
        if done.disposition().writes_allowed() {            // false for dry-run
            plan.apply(&target, ApplyOptions::default(), &runner)?;
        } else {
            show(&plan);
        }
    }
    Interview::Asking(_) => unreachable!("driven to a terminal outcome"),
}
```

### Configured library caller (composition with presets, origin-bearing `Resolution`)

Configured-default consumption is unchanged and routes through the final
origin-bearing consuming `Resolution`; confirm-flow adds no new consumption point.

```rust
let resolution = toha::interview::configured_defaults(
    &resolved.formal_name, &template, &config.presets, &config.template_defaults)?;
for warning in resolution.warnings() { report_warning(warning); }
let interview = resolution.start(&template, clock.now())?;   // origin-bearing route
```

### Terminal / direct `apply TEMPLATE PATH`

```text
$ toha apply ./web-service ./out
The target is not empty. Continue anyway? no
stopped: overwrite_ok — no files were written and no hooks ran
$ echo $?
0
```

`drive` returns a terminal outcome; a stop routes to a one-line notice and exit `0`
with nothing built; a dry-run routes into the existing `--dry-run` plan-print path.
The confirm dry-run and the `apply --dry-run` flag meet at one gate — either being
set previews without writing.

### Headless (wire protocol) and staged callers

```rust
match protocol::answer_headless(&template, interview, document)? {
    Headless::Pending { pending, rejections, .. } =>
        emit(protocol::batch_document(pending.batch(), &ctx, Some(&rejections))), // status: questions
    Headless::Completed { completed, .. } =>
        emit(protocol::complete_document(&completed, &ctx)),   // status: complete (+ disposition if dry-run)
    Headless::Stopped { ended, .. } =>
        emit(protocol::stopped_document(&ended, &ctx)),        // status: stopped
}
```

`stopped_document` is the only serializer of a stop, used by the headless and
staged drivers alike, so their byte output is identical for identical submissions
(driver parity). Staged `replay`/`replay_with_resolution` returns the same
`Interview` outcomes; a stop is re-derived from the stored submissions.

## Product behavior

### Stop writes nothing and runs no hooks — by construction

A confirm whose recorded answer maps to `stop` ends the interview at that node.
`advance` yields `Interview::Ended(Ended)`, which carries the answers and messages
gathered so far **for reporting only** and deliberately carries **no `Completed`
and no hooks**. There is no API path from `Ended` to `Plan::build` or `Plan::apply`,
so "stop writes nothing and runs no hooks" is a property of the type, not a rule a
driver must remember. A stop does no downstream work: it does not render or validate
the defaults of questions after it, so a stop can never fail on an unreached
question's default.

### Dry-run is normal planning with apply suppressed

A confirm whose recorded answer maps to `dry-run` does **not** alter control flow.
It sets a sticky `Disposition::DryRun` on the walk that rides to a normal
`Completed`, so the plan it previews is the full plan over the complete answer set.
Every driver builds the plan (`Plan::build` is pure) and suppresses `Plan::apply`,
the sole write/hook site — the same branch the existing `apply --dry-run` flag uses.
The template-declared dry-run and the CLI flag compose by union: either previews.
This dry-run is distinct from the CLI `--dry-run` flag (caller-initiated,
apply-only) and from ordinary planning (which applies).

### Skip records defined values for a scoped remainder

A confirm whose recorded answer maps to `{ skip: rest }` skips every remaining
question in the interview; `{ skip: group }` skips the remaining siblings of the
immediately enclosing group. Skip reuses the engine's existing skip machinery
verbatim: each skipped question records its configured/own default, or
`empty_answer(kind)` when it has none (`[]` for a looped `text`/`multiselect`,
`Answer::None` otherwise), and a held answer for a skipped question emits the exact
existing warning `answer for "<id>" was not used: the question was skipped` in
interview order. No new skipped-default rule is introduced. At interview top level
the enclosing frame is the whole interview, so `{ skip: group }` there equals
`{ skip: rest }` with no special case.

### Ordinary confirms are unchanged

A confirm with no `action` is byte-for-byte unchanged: `Typed<bool>` default →
`Answer::Bool` → JSON boolean → `inquire::Confirm` in the terminal. An action fires
only for an **active** confirm (its `when` was true) whose answer is recorded, keyed
off active-vs-skipped, never off the recorded boolean alone — so a `when`-inactive
confirm that records a default `Answer::Bool` never fires.

## Data structures

### Declared action (template domain)

```rust
// template.rs — extends the existing closed QuestionKind::Confirm.
pub enum QuestionKind {
    // ... Text / Multiline / Select / MultiSelect / TextLoop unchanged ...
    Confirm {
        default: Option<Typed<bool>>,   // UNCHANGED — pure boolean behavior preserved
        action: Option<ConfirmAction>,  // None => an ordinary confirm
    },
}

/// Actions bound to a confirm's two answers. Invariant: at least one arm is
/// `Some` — the loader rejects an empty `action`, so a constructed value always
/// carries a live action.
pub struct ConfirmAction {
    pub on_true: Option<ActionKind>,
    pub on_false: Option<ActionKind>,
}

/// A control action a confirm answer triggers. Declared data, never inferred.
pub enum ActionKind { Stop, DryRun, Skip(SkipScope) }

/// Invariant: `Group` is constructed only for a confirm inside a group; the
/// loader rejects `skip: group` at interview top level, so the runtime never sees
/// a `Group` scope without an enclosing group to bound it.
pub enum SkipScope { Rest, Group }
```

`action` never appears on `Prompt`, on the JSON Schema batch, or on any wire type;
it is parsed into these domain types behind the interview boundary (per
boundary-discipline). The terminal, protocol, and staging layers do not know
actions exist; they observe only the outcome types below.

### Result surface (public)

```rust
// interview.rs
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),   // now carries a disposition
    Ended(Ended),          // NEW — a confirm `stop` action ended the interview
}

/// How a completed interview should be applied. Re-derived every walk from the
/// recorded confirm answers; never persisted (replay recomputes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition {
    #[default]
    Proceed,   // normal completion: plan then apply (writes + hooks)
    DryRun,    // a confirm dry-run fired: plan and show, write nothing, run no hooks
}
impl Disposition {
    /// True only for `Proceed`; the single predicate every driver routes on.
    pub fn writes_allowed(&self) -> bool { matches!(self, Disposition::Proceed) }
}

pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    pub disposition: Disposition,   // NEW
    skipped: Skipped,
    step_start: usize,
}
impl Completed {
    /// Single source of truth for the apply disposition.
    pub fn disposition(&self) -> Disposition { self.disposition }
}

/// A stop action ended the interview. Carries NO `Completed` and NO hooks: there
/// is no value here `Plan::build`/`apply` will accept, so "stop writes nothing and
/// runs no hooks" is an invariant of the type. Answers/messages are for reporting.
pub struct Ended {
    by: Id,
    answers: Answers,
    messages: Vec<String>,
    last_messages: Vec<String>,
    skipped: Skipped,   // private; used only to classify early-answer failures (§atomicity)
}
impl Ended {
    pub fn by(&self) -> &Id { unimplemented!() }
    pub fn answers(&self) -> &Answers { unimplemented!() }
    pub fn messages(&self) -> &[String] { unimplemented!() }
}
```

### Walk effect (engine-private)

The walk's `Result<bool, EvalError>` return (`true` = reached, `false` = blocked)
cannot express a third outcome, so it widens to a small **engine-private** enum. No
public surface sees `Flow`/`Act`.

```rust
/// The control flow a walked node induces. Replaces the walk's `bool`.
enum Flow { Reached, Blocked, Stopped, SkipRest }

/// What consulting an active confirm's action yields, mapped onto `Flow`.
/// `skip: group` is handled locally (flip the loop's skip flag), so it needs no
/// `Flow` variant.
enum Act { None, Stop, DryRun, SkipHere /* group */, SkipRest /* rest */ }

struct Advance<'a> {
    // ... existing fields ...
    /// Raised to DryRun when an active confirm's dry-run fires. Re-derived each
    /// walk; never persisted.
    disposition: Disposition,
    /// The confirm id that stopped the walk; set only with Flow::Stopped.
    stopped_by: Option<Id>,
}
```

### Crate effect gate (graft from C2)

```rust
/// Realizes a terminal interview outcome against `target`. Builds the plan unless
/// the interview stopped; applies it only when the disposition is `Proceed`. The
/// one function that turns an outcome into (no) effects, so every crate and direct
/// caller shares one policy and cannot leak a write on stop or dry-run.
pub fn realize(
    template: &Template,
    outcome: Interview<'_>,        // must be terminal (Complete | Ended)
    target: &CanonicalTarget,      // approved 1072 identity type; &Path until 1056 lands
    options: ApplyOptions,
    runner: &dyn HookRunner,
) -> Result<Realized, RealizeError> { unimplemented!() }

pub enum Realized {
    Stopped { by: Id },            // nothing built, nothing written
    Previewed { plan: Plan, by: Id }, // dry-run: plan built, not applied
    Applied(Applied),              // proceed: wraps apply's own outcome
}
```

`Applied` (`apply.rs`) is unchanged; stop/dry-run never enter `apply`, so they are
not apply outcomes. `Realized` wraps it one level up, keeping the confirm concern
separate from apply's trust concern.

## Public and crate-visible interfaces

```rust
// Engine — signatures unchanged; only the Ok shapes widen.
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError>;
}
impl<'a> Pending<'a> {
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>>;
}
// advance(state) -> Result<Interview, EvalError> — sole result constructor, widened Ok.

// Terminal driver — returns the terminal outcome, not a bare Completed.
pub enum DriveResult { Completed(Completed), Stopped(Ended) }
pub(crate) fn drive<'a>(
    interview: Interview<'a>, ask: &mut impl Ask,
    accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>,
) -> Result<DriveResult, String>;

// Headless — one new arm.
pub enum Headless<'a> {
    Completed { completed: Completed, accepted: Vec<IndexMap<String, Value>> },
    Pending  { pending: Box<Pending<'a>>, rejections: Rejections, accepted: Vec<IndexMap<String, Value>> },
    Stopped  { ended: Ended, accepted: Vec<IndexMap<String, Value>> },   // NEW
}

// Staging — replay returns the widened Interview; a submission after a terminal
// (Complete or Ended) interview is the existing terminal-replay error.
impl StagedRecord {
    pub fn replay_with_resolution<'a>(
        &self, template: &'a Template, resolution: Resolution,
    ) -> Result<Interview<'a>, StagingError>;
    // Existing replay / replay_with_defaults remain, widened the same way.
}

// Wire — one new document builder; complete gains an optional disposition field.
pub fn stopped_document(ended: &Ended, context: &Context) -> Value;   // status: stopped
pub fn complete_document(completed: &Completed, context: &Context) -> Value; // + disposition when dry-run
```

## Module and seam map

```text
template.yml ──load──▶ template.rs
  QuestionKind::Confirm { default, action: Option<ConfirmAction> }
  LOAD RULES (beside loop/options): action-on-confirm-only · non-empty ·
              closed value set · skip:group-needs-a-group        (LOAD TIME)
                         │
                         ▼
interview.rs  Advance::walk ── active, answered confirm ──▶ confirm_act(q) ─▶ Act ─▶ act() ─▶ Flow
   (THE FUNNEL, seen once)     │  Stop   → stopped_by + Flow::Stopped → Interview::Ended
                               │  DryRun → disposition=DryRun (keeps walking)
                               │  SkipHere/SkipRest → existing skip flag (render_default · warn_if_held)
                               ▼
                    advance ─▶ Interview::{ Asking | Complete(Completed{disposition}) | Ended }
                               │
        ┌──────────────┬───────┴───────┬───────────────┬──────────────┐
        ▼              ▼               ▼               ▼              ▼
   terminal.rs     protocol.rs     staging.rs       main.rs        lib.rs
   DriveResult     Headless::      replay ─▶ same    routes via     realize() /
   {Completed,      {Completed,    Interview arms;   disposition    Interview arms
    Stopped}         Pending,      stopped_document  + realize()
                     Stopped}      on the wire
                               │
   Completed(Proceed) ─▶ Plan::build ─▶ Plan::apply   (sole write/hook site)
   Completed(DryRun)  ─▶ Plan::build ─▶ show, no apply (reuses --dry-run path)
   Ended              ─▶ report only; no Completed exists, structurally unplannable
```

`interview` owns the action decision (one funnel). `template` owns declaration and
load rules. Drivers, protocol, and staging are thin adapters that route the
outcome; none re-implements policy. `plan`/`apply` are unchanged: "no files, no
hooks" on stop and dry-run is achieved by not entering the apply effect loop —
exactly the existing `--dry-run`/`NeedsTrust` precedent.

## Submission ordering and atomicity

The action is consulted at the single funnel — the walk's active-and-answered
confirm site — after `when` and after the held-answer check. It reads only
`self.answers`, which holds only committed values.

- **A rejected document fires nothing.** `Pending::answer` runs the tentative
  `advance` (which may now yield `Ended` or a dry-run `Completed`); on any surviving
  rejection it returns `AnswerError::Rejected` with the **original** `Pending`, and
  the tentative outcome — with any effect inside it — is discarded. A valid confirm
  effect that shares a batch with a rejected sibling never leaks, and no rejected
  document persists. This is the approved 1072 atomicity contract, honored by
  placement rather than a new guard.
- **The `stands` early-failure classifier reads the new outcomes via the `skipped`
  set**, never the effect kind, keeping 1072 policy 2 exact:

  ```rust
  let stands = |id: &Id| match &advanced {
      Ok(Interview::Asking(n))   => !n.skipped.contains_key(id),
      Ok(Interview::Ended(e))    => !e.skipped.contains_key(id),   // NEW
      Ok(Interview::Complete(_)) => false,                          // reached & skipped
      _ => false,
  };
  ```

  A stop that short-circuits before a probe's question leaves that id absent from
  `skipped` → `stands` → the rejection is kept → the document is rejected → the
  `Ended` is discarded. A proven-skipped early error beside a stop is still dropped.

- **Batch siblings.** A confirm and later questions can share a batch. An
  already-answered sibling keeps its value (the walk's `answers.contains_key`
  branch precedes any skip); a stop short-circuits before the unanswered remainder;
  a skip records defaults for the unanswered remainder. The user's decision is
  deterministic from the submitted order.

## Persistence and compatibility

Staged records still persist only `{ submissions, identity, now }`. No action,
disposition, stop marker, or scope is persisted. `disposition` and the stop are
re-derived by `advance` on every walk and every replay from the same submissions,
so resume reproduces the identical outcome and answers (replay determinism). The
wire `complete` document omits `disposition` for `Proceed`, so ordinary complete
documents are byte-identical to today; the `stopped` document is new and additive.

## Composition with approved predecessor contracts

- **Answer-transaction atomicity (1072).** Honored as above; holds under today's
  emergent `unless_skipped`/tentative-advance loop and under the future
  `SubmissionTxn`/`SkipDisposition` alike. Confirm-flow depends on the approved
  contract, not on merged 1056 runtime.
- **Target identity (1072).** Confirm-flow adds no second target normalizer and
  builds no plan on stop; `realize`/`Plan` take the identity type — `&Path` today,
  `&CanonicalTarget` post-1056 — and never normalize. The design is order-
  independent w.r.t. 1056.
- **Origin-bearing consuming `Resolution` (final configured contract).** Wherever
  confirm-flow touches configured-default consumption (start, replay, headless) it
  routes through the origin-bearing consuming `Resolution` (`warnings()`,
  `start(template, now)`, `replay_with_resolution(template, resolution)`), never the
  flat `a62061f` projection. It reconstructs no configured origins, re-derives no
  id-only key, and never calls `into_flat_defaults` (the only sanctioned provenance-
  shedding escape). The ordinary flat `Seed` route is unchanged. Confirm-flow reads
  the confirm's recorded `Answer::Bool` regardless of the answer's origin and
  inspects no provenance, so it is provenance-neutral and composes with the
  origin-bearing route without touching attribution.
- **Skip/default policy (1072).** Reuses `default_ready`/`skipped_descendants_ready`
  and the existing `Advance::skip`/`warn_if_held`; introduces no parallel walker and
  no different skipped-default rule.

## Error / results contract

- **Load time (preferred).** In `template.rs`, beside the `loop`/`options` rules:

  | Condition | Message (names the corrective form) |
  |---|---|
  | `action` on a non-confirm question | `action is only valid on a confirm question; remove action or change type to confirm` |
  | `action` present but empty | `action must set when_true or when_false` |
  | arm value not `stop`/`dry-run`/`{skip: rest\|group}` | `action.<arm> must be stop, dry-run, or {skip: rest\|group}` |
  | `{ skip: group }` on a top-level confirm | `skip: group requires the question to be inside a group; use skip: rest` |

- **Run time.** The funnel evaluates nothing (the mapping is static, the answer is
  recorded), so it adds no `EvalError`. `Ended` and a dry-run `Completed` are `Ok`
  outcomes. `AnswerError`/`EvalError`/`StagingError`/`ApplyError` are unchanged.
- **Exit codes.** A confirm stop exits `0` (a deliberate, successful control
  outcome) with a stderr notice naming the trigger; a confirm dry-run reuses the
  existing dry-run exit `0`. Agents detect a stop via the `stopped` wire status
  rather than the exit code (see Decision D1).
- **Driver parity.** For identical submissions, terminal / headless / staged /
  direct / crate reach the same outcome; headless and staged emit byte-equal
  `stopped` documents, and byte-equal target trees on `Proceed`.

## Proposed canonical-document changes

The paired implementation (1062) owns these edits; the design only describes them.

- `docs/specifications/template-format.yml` + `template-format.schema.yml`: add an
  optional `action` property to `$defs/question` (keeping `additionalProperties:
  false` and the closed `type` enum), a new `$defs/confirm-action`
  (`minProperties: 1`, arms `when_true`/`when_false`) and `$defs/action-kind`
  (`oneOf: [const stop, const dry-run, { skip: enum[rest, group] }]`); document the
  four load rules beside the existing `loop`/`options` rules; state that ordinary
  confirms are unaffected and that action-skip reuses the `when`-skip recorded-value
  and "answer not used" warning semantics.
- `docs/specifications/interview-protocol.yml` + `interview-protocol.schema.yml`:
  add a third top-level document `stopped` (`status: stopped`, with `by`, `answers`,
  `messages`) to the `oneOf`; add an optional `disposition` field on `complete`
  (`const dry-run`, omitted for `Proceed`); document that a stop writes no files and
  runs no hooks, that a template-driven dry-run differs from the `apply --dry-run`
  flag but composes with it, and that both new shapes are additive to the closed
  document set.
- `docs/specifications/command-line-interface.yml` + `.spec.yml`: document that
  `apply`, `continue`, and terminal runs honor a confirm stop (exit 0, no
  plan/apply) and a confirm dry-run (plan shown, no writes/hooks); note the union
  with `--dry-run`.
- `docs/template-interviews.md`, `docs/template-flow.md`, `docs/template-hooks.md`,
  `docs/templates-management.md`: author-facing guide for `action`, the three
  actions, skip scope (rest vs group, nested), that actions are declared data, and
  the driver behaviors.

## Behaviors to prove (falsifiable)

**Declaration / load**

1. `action` on a `text`/`select`/etc. question fails to load naming the confirm
   corrective form; no interview starts.
2. `action: {}` fails to load with `action must set when_true or when_false`.
3. `{ skip: group }` on a top-level confirm fails to load naming `skip: rest`.
4. An unknown arm value fails to load listing `stop`, `dry-run`, `{skip: rest|group}`.
5. A confirm with no `action` produces byte-identical answers, wire documents (no
   `disposition` key), and target tree to the pre-feature build for the same
   answers, across all five drivers (pure-boolean regression pin).

**Stop**

6. `when_false: stop`, answered `false` ⇒ `Interview::Ended`; the target directory
   is unchanged and no hook ran (assert filesystem + `RecordingRunner` empty), in
   every driver; headless and staged emit byte-equal `stopped` documents.
7. The same confirm answered `true` completes and applies (action fires only on its
   declared truth value).
8. A stop does not render or validate any later question's default: a template with
   an invalid downstream default still stops cleanly (no template fault).
9. A confirm sharing a batch with a later question: stop fires on the step that
   submits the confirm; an already-answered sibling keeps its value; nothing is
   written.

**Dry-run**

10. `when_true: dry-run`, answered `true` ⇒ `Complete` with `disposition == DryRun`;
    the plan is built and shown, no writes, no hooks; `Plan::build` over the result
    equals the plan for the same answers with `Proceed` (byte-equal).
11. The confirm dry-run and the `apply --dry-run` flag compose by union: either
    alone, or both, previews; neither applies twice or errors.
12. Answering the dry-run confirm `false` applies normally; the complete document
    has no `disposition` key.

**Skip scope + nesting**

13. `{ skip: rest }` skips every remaining question, including inside deeper groups;
    each records its configured/own default or `empty_answer(kind)`; the interview
    completes; generated output matches leaving those questions at default.
14. `{ skip: group }` inside a nested group skips only that group's remaining
    siblings; questions after the group are still asked; a `{ skip: group }` at
    interview top level equals `{ skip: rest }`.
15. A held/early answer for a question skipped by an action emits the exact existing
    `answer for "<id>" was not used: the question was skipped` warning at the skip
    position.

**Composition / determinism**

16. A `when`-inactive confirm never fires its action even though it records a
    default `Answer::Bool` (keyed off active-vs-skipped, not the bool).
17. An early/held confirm answer fires the action when its question is reached, not
    before the `when` check.
18. Resume/replay: a staged record whose submissions stop / dry-run / skip
    reproduces the identical outcome and generated output; `StagedRecord` gains no
    field.
19. A confirm answered by a configured default (origin-bearing `Resolution` route)
    that maps to an action fires that action; provenance is not reconstructed and
    warnings still flow through `Resolution::warnings()`.

**Negative / atomicity**

20. A document that both fires a confirm action and carries a rejected answer
    returns `AnswerError::Rejected`; no `Ended` and no `disposition` is produced;
    nothing persists.
21. A valid stop-confirm beside an early answer proven skipped by the tentative step
    drops the early error (1072 policy 2) and the stop stands.
22. A submission after a terminal (`Complete` or `Ended`) staged interview is the
    existing terminal-replay error.

## Out of scope

- The pure interview engine's answer/skip/default semantics below the funnel; the
  `Seed`/`Resolution`/configured-default selection and precedence (owned by presets
  1058) and error-attribution attribution/`CanonicalTarget`/`SubmissionTxn` (owned
  by 1072/1056).
- Any new trust, permission, or access surface; any new timeout; any pinned-version
  check; any application subprocess integration. A confirm action never influences
  trust or runs a hook.
- New protocol fields beyond the additive `stopped` document and the optional
  `disposition` field; general error-code or localization redesign.
- Actions on non-confirm questions; inferring actions from prompt text; object-
  valued or chained actions; per-run CLI overrides of template actions.

## Decisions needed first (for the Phase C checkpoint)

- **D1 — Exit code for a confirm stop.** Recommend **exit 0** (a deliberate control
  outcome, not a failure) with the machine-readable `stopped` wire status for agents
  and a stderr notice for humans. Alternative: a distinct non-zero code for scripts;
  rejected as the default because it collides conceptually with error/`incomplete`
  codes and the wire status already carries the signal.
- **D2 — May a configured-default (preset) answer fire a terminal action?**
  Recommend **yes** (the default is the answer; consistent with how defaults become
  answers everywhere). Surfaced because a preset could then silently stop or
  dry-run an interview; the alternative is to make defaulted values inert for
  actions.
- **D3 — Skip-scope surface.** Recommend `{ skip: rest | group }`. Alternatives
  considered: `skip-rest`/`skip-group` bare verbs (C1) or `{ skip: interview |
  group }` (C2/C3). `rest`/`group` reads clearly and avoids the `interview` vs
  top-level-`group` synonym.
- **D4 — Sequencing vs 1056.** Recommend landing 1062 **independently against the
  approved 1072 contract** (coupling expressed as the atomicity invariant + the
  target-identity type), not blocking on 1056. Alternative: order 1062 after 1056.
- **D5 — `stopped` wire payload.** Recommend including the answers gathered so far
  (for audit/visibility). Alternative: omit them to avoid implying a usable result.

No decision introduces a capability restriction, a permissions/access change, a
timeout, a pinned check, or an application subprocess; the feature is additive and
opt-in, and ordinary confirms are unchanged.

## Size and complexity

- **Size:** ~M. `template.rs`: the `action` parse + four load rules. `interview.rs`:
  the `Act`/`Flow` widening, the `confirm_act` funnel and `act` mapper, the
  `disposition`/`stopped_by` walk state, the `Ended` result and `Completed`
  disposition field, and the `stands` classifier update. Thin adapters in
  `terminal.rs` (DriveResult), `protocol.rs` (Headless::Stopped + `stopped_document`
  + `disposition` on complete), `staging.rs` (widened replay), `main.rs` (routing +
  `realize`), and `lib.rs` (`realize`/exports). Schema/guide edits (owned by 1062).
  The behaviors above as tests.
- **Complexity:** low–moderate. One new decision funnel, one new terminal variant,
  one completion field; skip reuses existing machinery; no new persisted state, no
  new concurrency, no new dependency, no new protocol status semantics beyond the
  additive shapes. Expected implementation time: one to two focused agent days after
  base refresh (rebase onto the then-current epic head).
