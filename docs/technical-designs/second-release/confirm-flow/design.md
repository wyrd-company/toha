---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
    - command-line-interface
---

# Interview flow control — the `flow` node

A template steers the interview with a new **`flow` node**: a control node that
carries a `when` expression and one control **action** — **stop**, **dry-run**, or
**skip** (rest of the interview, or rest of the current group). It is a peer of the
`message`, `hook`, and `computed` nodes: it has no `id`, records no answer, and
fires at its position when its `when` is true. The action is declared data, never
inferred from prompt text. The `confirm` question type is **not modified at all**,
so ordinary confirms keep pure boolean behavior by construction; a confirm gates a
flow node exactly like any other answer, through the existing `when` construct.

This design supersedes the round‑1 shape (an action attached to `confirm` types),
which is preserved for comparison as `design-round1-confirm-action.md`. It is the
synthesis of round‑2 arena base **flow-1** (minimal standalone node) with grafts
from **flow-3** (the `Completed::step()` routing seam, same-batch readiness, and the
idempotent dry-run raise); flow-2's group-attached guard is surfaced as an optional
decision (D-attach). See `synthesis.md` and `grounding-flow.md`.

## Caller's usage

### Declaring flow (template author)

```yaml
interview:
  - id: proceed
    type: confirm                     # an ORDINARY confirm — pure boolean, unchanged
    prompt: "Ready to scaffold?"
  - flow: stop                        # end the run; write nothing, run no hooks
    when: "not proceed"
    label: "declined at proceed gate" # optional; names the node in results

  - id: mode
    type: select
    options: [apply, preview]
  - flow: dry-run                     # plan and show; write nothing
    when: "mode == 'preview'"

  - group: telemetry
    when: features.telemetry
    nodes:
      - id: configure_now
        type: confirm
        prompt: "Configure telemetry now?"
      - flow: { skip: group }         # skip the REST of this group from here
        when: "not configure_now"
      - id: telemetry_key
        type: text
        prompt: "Telemetry write key"

  - flow: { skip: rest }              # skip every remaining question in the interview
    when: "minimal"
```

The `flow:` key carries the action, mirroring `message: <text>`; `when` and `label`
are the usual sibling keys. Action values: `stop`, `dry-run`, `{ skip: rest }`,
`{ skip: group }`. `when` is optional — a `flow` with no `when` always fires at its
position. Because the trigger is the existing `when`, it can read **any** expression
over prior answers or computed values, not just one confirm.

### Crate / library caller

Stop is a distinct terminal outcome; a completed interview routes through one seam.

```rust
let mut interview = Interview::start(&template, Seed { now, defaults })?;
loop {
    interview = match interview {
        Interview::Asking(pending) => pending.answer(next_document(&pending))?,
        Interview::Ended(ended) => {                 // a flow `stop` fired
            for line in &ended.messages { println!("{line}"); }
            return Ok(());                           // no Completed exists → cannot plan
        }
        Interview::Complete(completed) => {
            match completed.step() {                 // one seam: proceed vs dry-run
                Step::Plan { apply, .. } => {
                    let plan = Plan::build(&template, &completed, &target)?; // pure
                    if apply { plan.apply(&target, ApplyOptions::default(), &runner)?; }
                    else { print_plan(&plan); }      // dry-run: show, do not apply
                }
            }
            return Ok(());
        }
    };
}
```

### Configured library caller (composition with presets, origin-bearing `Resolution`)

Unchanged and provenance-neutral — the flow node reads answers through `when`,
touches no configured-default consumption:

```rust
let resolution = toha::interview::configured_defaults(
    &resolved.formal_name, &template, &config.presets, &config.template_defaults)?;
for warning in resolution.warnings() { report_warning(warning); }
let interview = resolution.start(&template, clock.now())?;   // origin-bearing route
```

### Terminal / direct `apply TEMPLATE PATH`

```text
$ toha apply ./web-service ./out
Ready to scaffold? no
stopped: declined at proceed gate — no files were written and no hooks ran
$ echo $?
0
```

`terminal::drive` returns the terminal outcome; a stop routes to a one-line notice
and exit `0` with nothing built; a dry-run routes into the existing `--dry-run`
plan-print path. The flow dry-run and the `apply --dry-run` flag compose by union —
either previews.

### Headless (wire) and staged callers

```rust
match protocol::answer_headless(&template, interview, document)? {
    Headless::Pending { pending, rejections, .. } =>
        emit(batch_document(pending.batch(), &ctx, Some(&rejections))),   // status: questions
    Headless::Completed { completed, .. } =>
        emit(complete_document(&completed, &ctx)),   // status: complete (+ disposition when dry-run)
    Headless::Ended { ended, .. } =>
        emit(ended_document(&ended, &ctx)),          // status: ended (NEW, additive)
}
```

`ended_document` is the only serializer of a stop, used by the headless and staged
drivers, so their byte output is identical for identical submissions. Staged
`replay`/`replay_with_resolution` returns the same outcomes; a stop is re-derived
from the stored submissions.

## Product behavior

### Stop writes nothing and runs no hooks — by construction

A `flow: stop` whose `when` is true ends the interview at that node. `advance`
yields `Interview::Ended(Ended)`, which carries only the messages/label reached (for
reporting) and **no `Completed` and no hooks**. There is no API path from `Ended` to
`Plan::build`/`Plan::apply`, so "stop writes nothing, runs no hooks" is a property of
the type. A stop does no downstream work: it renders no later question's default, so
it can never fault on a question the run never reached.

### Dry-run is normal planning with apply suppressed

A `flow: dry-run` sets a sticky `Disposition::DryRun` that rides to a normal
`Completed`, so the previewed plan is the full plan over the complete answer set.
Every driver builds the plan (`Plan::build` is pure) and suppresses `Plan::apply`
(the sole write/hook site) — the same branch the existing `apply --dry-run` flag
uses; the two compose by union. Distinct from the CLI flag (caller-initiated,
apply-only) and from ordinary planning.

### Skip records defined values for a scoped remainder

`{ skip: rest }` skips every remaining question in the interview; `{ skip: group }`
skips the remaining siblings of the immediately enclosing group from the flow node's
position. Skip reuses the engine's existing machinery: each skipped question records
its configured/own default, or `empty_answer(kind)` when it has none, and a held
answer for a skipped question emits the existing `answer for "<id>" was not used: the
question was skipped` warning in interview order. No new skipped-value rule. To skip
a **whole** group conditionally, an author uses the group's own `when` (existing);
`{ skip: group }` is specifically the mid-group early-out. At interview top level the
enclosing frame is the whole interview, so a top-level `{ skip: group }` is a load
error naming `{ skip: rest }` (D3).

### Ordinary confirms are unchanged

`QuestionKind::Confirm { default }` is not touched. An ordinary confirm records
`Answer::Bool`/`Answer::None`, maps to JSON boolean, and prompts via
`inquire::Confirm`, exactly as today. A flow node fires only when reached with its
`when` true and not under an ancestor skip.

## Data structures

### The flow node (template domain)

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
pub struct FlowNode {
    pub when: Option<Expr>,         // reused `when`; None ⇒ always fires
    pub label: Option<String>,      // optional diagnostic name (the node has no id)
    pub action: FlowAction,         // declared data, never inferred
}

pub enum FlowAction { Stop, DryRun, Skip(SkipScope) }

pub enum SkipScope {
    Rest,                           // rest of the whole interview
    Group,                          // rest of the enclosing group; load error at top level
}
```

`action` never reaches `Prompt`, the JSON-Schema batch, or any wire type; it is
parsed into these domain types at the load boundary. `collect_answers` ignores
`Node::Flow` (it joins `Message`/`Hook`), so it never enters the flat `Answers` map
or the id namespace and needs no default/empty handling.

### Result surface (public)

```rust
// interview.rs
pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),            // now carries a disposition
    Ended(Ended),                   // NEW — a flow `stop` ended the interview
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition { #[default] Proceed, DryRun }

pub struct Completed {
    pub answers: Answers,
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub hooks: Vec<RenderedHook>,
    pub now: jiff::Zoned,
    pub disposition: Disposition,   // NEW; re-derived each walk, never persisted
    skipped: Skipped,
    step_start: usize,
}
impl Completed {
    pub fn disposition(&self) -> Disposition { self.disposition }
    /// The one policy seam: how a completed interview's plan step is treated.
    /// Drivers route through this instead of matching `Disposition`.
    pub fn step(&self) -> Step {
        match self.disposition {
            Disposition::Proceed => Step::Plan { apply: true },
            Disposition::DryRun  => Step::Plan { apply: false },
        }
    }
}

/// Routing outcome for a completed interview (graft from flow-3). Stop is NOT here —
/// a stop is `Interview::Ended`, handled before any `Completed` exists.
pub enum Step { Plan { apply: bool } }

/// A stop. Carries only what a driver prints; nothing a plan consumes.
pub struct Ended {
    pub messages: Vec<String>,
    pub last_messages: Vec<String>,
    pub label: Option<String>,
    skipped: Skipped,               // private; classifies early-answer failures (§atomicity)
}
impl Ended {
    pub fn messages(&self) -> &[String] { unimplemented!() }
    pub fn label(&self) -> Option<&str> { unimplemented!() }
}
```

Stop is a variant (type-unplannable); dry-run is a field on `Completed` (a dry-run
is a normal completion that still plans); `step()` is the single routing seam so no
driver re-implements the proceed-vs-dry-run policy. The CLI flag composes at the
seam: `apply = matches!(completed.step(), Step::Plan { apply: true }) && !cli_dry_run`.

### Walk effect (engine-private)

The walk's `Result<bool, EvalError>` widens to a small engine-private enum; no public
surface sees it.

```rust
enum Walk { Complete, Blocked, Ended, SkipRest }   // was bool (true/false)

struct Advance<'a> {
    // ... existing fields ...
    disposition: Disposition,       // raised by a fired dry-run; re-derived each walk
    ended: Option<Ended>,           // set by a fired stop
    skip_rest: bool,                // a `skip: rest` fired; propagates to all frames
}
```

## Public and crate-visible interfaces

```rust
// Engine — signatures unchanged; only the Ok shapes widen.
impl<'a> Interview<'a> { pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError>; }
impl<'a> Pending<'a> { pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'a>, AnswerError<'a>>; }
// advance(state) -> Result<Interview, EvalError> — sole result constructor, widened Ok.

// Terminal driver — returns the terminal outcome.
pub enum Session { Completed(Completed), Ended(Ended) }
pub(crate) fn drive<'a>(interview: Interview<'a>, ask: &mut impl Ask,
    accepted: impl FnMut(IndexMap<String, Value>) -> Result<(), String>) -> Result<Session, String>;

// Headless — one new arm.
pub enum Headless<'a> {
    Completed { completed: Completed, accepted: Vec<IndexMap<String, Value>> },
    Pending  { pending: Box<Pending<'a>>, rejections: Rejections, accepted: Vec<IndexMap<String, Value>> },
    Ended    { ended: Ended, accepted: Vec<IndexMap<String, Value>> },   // NEW
}

// Staging — replay returns the widened Interview; a submission after a terminal
// (Complete or Ended) interview is the existing terminal-replay error.
impl StagedRecord {
    pub fn replay_with_resolution<'a>(&self, template: &'a Template, resolution: Resolution)
        -> Result<Interview<'a>, StagingError>;
}

// Wire — one new document builder; complete gains an optional disposition field.
pub fn ended_document(ended: &Ended, context: &Context) -> Value;      // status: ended
pub fn complete_document(completed: &Completed, context: &Context) -> Value;  // + disposition when dry-run
```

## Module and seam map

```text
template.yml (flow: <action>, when, label)
   │  Template::load — a new Builder arm parallel to `message` (template.rs:600)
   │  LOAD RULES: unknown action; `skip: group` needs an enclosing group
   ▼
interview.rs  Advance::walk ── one Node::Flow arm (mirrors the message/hook arm)
   (fire once via visited · readiness-gate on `when` refs · inert under ancestor skip)
        │  Stop   → ended + Walk::Ended
        │  DryRun → disposition = DryRun (idempotent raise)
        │  Skip   → existing skip flag (group = rest of frame; rest climbs one frame/return)
        ▼
   advance ─▶ Interview::{ Asking | Complete(Completed{disposition}) | Ended }
        │
        ├── terminal.rs  Session::{Completed, Ended}
        ├── protocol.rs  Headless::{Pending, Completed, Ended} → status questions|complete|ended
        ├── staging.rs   replay → same outcomes (Ended re-derived; no new persisted state)
        ├── main.rs      routes; Completed via completed.step(); dry-run ∨ --dry-run
        └── lib.rs       matches Interview directly
        ▼
   Completed(Proceed) ─▶ Plan::build ─▶ Plan::apply   (sole write/hook site)
   Completed(DryRun)  ─▶ Plan::build ─▶ show, no apply
   Ended              ─▶ report only; no Completed exists → structurally unplannable
```

`interview` owns the decision in one walk arm. `template` owns declaration + load
rules. Drivers, protocol, and staging are thin adapters. `plan`/`apply` are
untouched: "no files, no hooks" on stop/dry-run is achieved by not entering the apply
effect loop — the existing `--dry-run`/`NeedsTrust` precedent.

## Submission ordering and atomicity

The flow decision is made only inside `Advance::walk`, reached by `advance`, on
committed answers.

- **Same-batch readiness (graft from flow-3).** A flow node whose `when` references a
  question still pending in the current batch is not ready (`expr_ready` false) and
  blocks, ending the batch at the flow node; it fires only on the next step after
  that answer commits. So a flow node never races its `when` references — the
  round‑1 same-batch hazard does not arise.
- **A rejected document fires nothing.** `Pending::answer` runs the tentative
  `advance`; on any surviving rejection it returns `AnswerError::Rejected` with the
  original `Pending`, discarding the tentative outcome and any effect in it. A
  rejected document never ends the interview, never sets a disposition, and never
  persists. This is the approved 1072 atomicity contract, by placement.
- **The `stands` early-failure classifier** reads the new outcomes via the `skipped`
  set (a stop returns `Ended`; a held early-failure for a question the stop prevented
  reaching is dropped, matching 1072 policy 2).

## Persistence and compatibility

Staged records still persist only `{ submissions, identity, now }`. No action,
disposition, or stop marker is persisted. The disposition and the stop are re-derived
by `advance` on every walk and replay from the same submissions (idempotent), so
resume reproduces the identical outcome. The wire `complete` document omits
`disposition` for `Proceed`, so ordinary completions are byte-identical to today; the
`ended` document is new and additive.

## Composition with approved predecessor contracts

- **Answer-transaction atomicity (1072):** honored as above; holds under today's
  emergent loop and the future `SubmissionTxn`. Depends on the contract, not merged
  1056 runtime.
- **Target identity (1072):** the flow node never touches the target; it builds no
  plan on stop; `Plan`/apply take the identity type (`&Path` now, `&CanonicalTarget`
  post‑1056) unchanged. No second normalizer.
- **Origin-bearing consuming `Resolution`:** the flow node reads answers through
  `when` and consumes no configured defaults, so it is provenance-neutral; configured
  defaults still flow through `Resolution::warnings()`/`start`/`replay_with_resolution`.
  It reconstructs no origins and never uses the flat projection.
- **Skip/default policy (1072):** reuses `default_ready`/`skipped_descendants_ready`
  and `Advance::skip`/`warn_if_held`; no parallel walker, no new skipped-default rule.
- **Runtime merge order (coordination ruling — not a decision):** the paired
  implementation (1062) integrates **after** the error-attribution implementation
  (1056). Independent design preparation is permitted; independent runtime integration
  is not. The design needs no 1056 code, so this ordering needs no design rework.

## Error / results contract

- **Load time (preferred).** In `template.rs`, beside the `loop`/`options` rules:

  | Condition | Message (names the corrective form) |
  |---|---|
  | `flow` value not `stop`/`dry-run`/`{skip: rest\|group}` | `flow action must be stop, dry-run, or { skip: rest \| group }` |
  | `{ skip: group }` on a flow node not inside any group | `skip: group needs an enclosing group; use { skip: rest } to skip the rest of the interview` |
  | `flow` combined with `id`/`type`/another node key | `flow is a node kind; it cannot be combined with <key>` |

- **Run time.** A flow `when` that fails to evaluate faults at `flow.when` with its
  source (identical to a message/hook `when` fault). A flow blocked as the sole
  unresolved node yields the existing unresolved-dependency error. `Ended` and a
  dry-run `Completed` are `Ok` outcomes.
- **Exit codes.** A stop exits `0` (a deliberate outcome) with a stderr notice naming
  the label/trigger; a dry-run reuses the existing dry-run exit `0`. Agents detect a
  stop via the `ended` wire status (D1).
- **Driver parity.** For identical submissions, terminal / headless / staged / direct
  / crate reach the same outcome; headless and staged emit byte-equal `ended`
  documents and byte-equal trees on `Proceed`.

## Proposed canonical-document changes (described; owned by the paired implementation 1062)

- `template-format.yml` + `.schema.yml`: add a `flow-node` to `$defs/node`'s `oneOf`
  (`additionalProperties: false`, `required: [flow]`, properties `flow` = the action
  `oneOf` [`const stop`, `const dry-run`, `{skip: enum[rest, group]}`], `when`,
  `label`); document the node, the actions, skip scope (rest vs current group, nested,
  and that whole-group conditional skip is `group.when`), and the load rules beside
  `loop`/`options`. `QuestionKind::Confirm` is unchanged.
- `interview-protocol.yml` + `.schema.yml`: add a third top-level document `ended`
  (`status: ended`, with `messages`, optional `label`) to the `oneOf`; add an optional
  `disposition` (`const dry-run`, omitted for proceed) to `complete`; document that a
  stop writes nothing and a dry-run composes with `apply --dry-run`; both additive.
- `command-line-interface.yml` + `.spec.yml`: document that `apply`/`continue`/
  terminal honor a flow stop (exit 0, no plan/apply) and dry-run (plan shown), and the
  `--dry-run` union.
- `docs/template-flow.md`, `template-interviews.md`, `template-hooks.md`,
  `templates-management.md`: author-facing "flow nodes" guide.

## Behaviors to prove (falsifiable)

**Declaration / load**
1. `flow: teleport` (unknown action) → load error naming the allowed set.
2. `{ skip: group }` on a top-level flow node → load error naming `{ skip: rest }`.
3. `flow` beside `id`/`type` → load error (flow is a node kind).
4. A template with no flow node produces byte-identical answers, wire documents (no
   `disposition`/`ended`), and tree to the pre-feature build (confirm-untouched pin).

**Stop**
5. `flow: stop when: "not proceed"`, `proceed=false` → `Interview::Ended`;
   `Plan::build` never called; target unchanged; `RecordingRunner` empty; every driver.
6. `proceed=true` → completes and applies (fires only when `when` is true).
7. A stop renders no later question's default: a template with an invalid downstream
   default still stops cleanly.

**Dry-run**
8. `flow: dry-run when: "preview"`, `preview=true` → `Completed.disposition == DryRun`;
   plan built and shown; no writes/hooks; `Plan::build` output equals the `Proceed`
   plan for the same answers.
9. Flow dry-run and `apply --dry-run` compose by union; neither applies twice nor errors.

**Skip scope + nesting**
10. `{ skip: rest }` skips every remaining question incl. deeper groups; each records
    its default/empty; interview completes.
11. `{ skip: group }` inside a nested group skips only that group's remaining siblings;
    questions after the group still ask; whole-group conditional skip via `group.when`
    is unaffected.
12. A held answer for a question skipped by a flow node emits the existing "answer not
    used" warning at the skip position.

**Composition / determinism**
13. A flow node under a false `when` or an ancestor skip does not fire.
14. Same-batch readiness: a `flow` whose `when` reads a still-pending confirm blocks,
    fires only after that confirm commits, and never appears as a batch item.
15. Resume/replay reproduces the identical stop/dry-run/skip from stored submissions;
    `StagedRecord` gains no field.
16. A flow `when` satisfied by a configured-default/preset answer fires the action;
    provenance is not reconstructed; warnings still flow through `Resolution::warnings()`.

**Negative / atomicity**
17. A document that both triggers a flow action and carries a rejected answer returns
    `AnswerError::Rejected`; no `Ended`/`disposition`; nothing persists.
18. A stop before a proven-skipped early answer drops the early error (1072 policy 2)
    and the stop stands.
19. A submission after a terminal (`Complete` or `Ended`) staged interview is the
    terminal-replay error.

## Out of scope

- The existing question/skip/default rules below the flow decision (reused, not
  changed); presets selection (1058) and error-attribution internals (1072/1056).
- Any new trust, permission, access, timeout, pinned-version check, or application
  subprocess. A flow action never runs a hook or touches trust.
- New protocol shapes beyond the additive `ended` document and optional `disposition`.
- Modifying `confirm`; inferring actions from prompt text; chained/overridden actions.

## Decisions needed first (Phase C)

- **D‑shape (headline) — adopt the `flow` node vs keep the action-bearing confirm.**
  Recommend **the `flow` node**: it reuses `when`/the node walk/skip verbatim, leaves
  `confirm` untouched (zero regression), is strictly more expressive (any condition,
  not one boolean), and has no same-batch race. Cost: one indirection for the simplest
  "no → stop" (the confirm-action form co-locates that single case). Both the parent
  and the other-family cross-judge recommend the flow node.
- **D‑attach — standalone flow node only, or also a group-attached guard?** Recommend
  **standalone only**: a group-attached `{ skip: group }` largely duplicates the
  group's existing `when` (which already skips a whole group conditionally); the
  standalone node's positional `{ skip: group }` (skip the rest of the current group)
  covers the mid-group early-out. Offered because flow-2 explored it.
- **D1 — Exit code when a flow stops the run.** Recommend **exit 0** with the
  machine-readable `ended` wire status. Alternative: a distinct non-zero code.
- **D2 — May a `when` satisfied by a configured-default/preset (or computed) answer
  fire a flow action?** Recommend **yes** (the default is the answer; consistent).
  Surfaced because a preset could then silently stop or preview a run.
- **D3 — Skip-scope wording.** `{ skip: rest | group }`, where `group` = the rest of
  the current group and a top-level `{ skip: group }` is a load error naming `rest`.
- **D5 — `ended` wire payload.** Recommend including the label and reached messages
  (and, optionally, the answers gathered so far) for audit.

Minor: `when` is optional on a flow node (a `when`-less flow always fires) —
recommended, not a separate decision.

No decision introduces a capability restriction, permissions/access change, timeout,
pinned check, or application subprocess; the feature is additive and opt-in, and
`confirm` is unchanged.

## Alternatives explored

- **Action-bearing confirm (round‑1, `design-round1-confirm-action.md`).** Co-locates
  the action on the confirm (`on: { when_false: stop }`), marginally more obvious for
  the simplest case; but it modifies the `confirm` type (regression surface, guarded
  field), binds the trigger to one boolean, must define batch-sibling fate at the
  confirm, and adds an answer→decision funnel. Set aside per Bob's "instead" and the
  merits above; retained for comparison.
- **flow-3 — stop as a `Completed` disposition (no `Ended` variant).** Smallest
  surface and one `step()` seam, but "cannot be planned" becomes runtime discipline
  across ~6 call sites rather than a type fact. Rejected for the type-safe `Ended`
  variant; its `step()` seam and readiness framing are grafted.
- **flow-2 — group-attached guard.** Makes a whole-group skip structural, but that
  largely duplicates `group.when`; surfaced as D-attach rather than baked in.
- **Inferring actions from prompt text / a magic answer** — rejected by the inherited
  constraint.

## Size and complexity

- **Size:** ~M. `template.rs`: the `flow` node parse + load rules. `interview.rs`: the
  `Walk` widening, the one `Node::Flow` walk arm, the `disposition`/`ended`/`skip_rest`
  state, the `Ended` result + `Completed` disposition + `step()`, and the `stands`
  update. Thin adapters in `terminal.rs` (`Session`), `protocol.rs`
  (`Headless::Ended` + `ended_document` + `disposition`), `staging.rs`, `main.rs`,
  `lib.rs`. Schema/guide edits (owned by 1062). The behaviors above as tests.
- **Complexity:** low–moderate. One node kind mirroring message/hook, one terminal
  variant, one completion field; skip and `when` reused; no new persisted state, no
  new concurrency/dependency, no new protocol status semantics beyond the additive
  shapes. Expected implementation time: one to two focused agent days after base
  refresh.
