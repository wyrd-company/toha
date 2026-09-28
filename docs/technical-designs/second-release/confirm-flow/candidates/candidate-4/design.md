# Confirm-controlled interview and apply flow — candidate design

**Structural shape:** the confirm action is a **node-level effect emitted inside
the interview walk**, in the same place the walk already emits reached
messages, hooks, skips, and recorded answers. When an *active* confirm's boolean
is recorded, the walk consults the action bound to that truth value and emits one
of three effects at that node position: **skip** (reuses the existing group-skip
flag verbatim), **dry-run** (a sticky disposition carried to a normal
completion), or **stop** (a walk-terminal effect that yields a new `Ended`
result). Nothing is bolted onto the state machine except one new `Interview`
variant; everything else is the machinery the walk already owns.

---

## 1. Usage first (caller's view)

### 1.1 README excerpt — declaring a confirm action

A `confirm` question keeps pure boolean behavior. Adding an `action` block — and
**only** an explicit `action` block — makes a specific answer drive control flow.
Actions are declared data; nothing is inferred from the prompt text.

```yaml
# template.yml (interview excerpt)
interview:
  - id: overwrite_ok
    type: confirm
    prompt: "The target directory is not empty. Continue anyway?"
    action:
      when_false: stop          # "No" ends the run: no files, no hooks.

  - id: preview_first
    type: confirm
    prompt: "Show the plan before writing?"
    action:
      when_true: dry-run        # "Yes" plans and shows it, writes nothing.

  - group: telemetry
    when: "features.telemetry"
    nodes:
      - id: configure_now
        type: confirm
        prompt: "Configure telemetry now?"
        action:
          when_false:
            skip: group         # "No" skips the rest of THIS group only.
      - id: telemetry_key
        type: text
        prompt: "Telemetry write key"

  - id: proceed
    type: confirm
    prompt: "Ready to scaffold?"
    action:
      when_false:
        skip: rest              # "No" skips every remaining question.
```

The action grammar (closed, additive to the existing `question` object):

| `action` arm            | value                          | effect                                              |
| ----------------------- | ------------------------------ | --------------------------------------------------- |
| `when_true`             | `stop`                         | terminal: end the interview, plan/apply nothing     |
| `when_false`            | `dry-run`                      | complete normally, then plan + show, write nothing  |
| (either)                | `{ skip: rest }`               | skip every remaining question (whole interview)     |
| (either)                | `{ skip: group }`              | skip the remaining siblings of the enclosing group  |

At least one of `when_true` / `when_false` must be present. An ordinary confirm
omits `action` entirely and is byte-for-byte unchanged.

**Load-time rejections** (all raised by `Template::load`, before any interview):

- `action` on any non-confirm question →
  `action is only valid on a confirm question; remove action or change type to confirm`.
- `action` present but empty →
  `action must set when_true or when_false`.
- an arm value that is not `stop`, `dry-run`, or `{ skip: rest|group }` →
  `action.when_true must be stop, dry-run, or {skip: rest|group}`.
- `{ skip: group }` on a confirm that is **not** inside a group →
  `skip: group requires the question to be inside a group; use skip: rest`.

### 1.2 Call site A — terminal, direct `apply TEMPLATE PATH`

The template above; the user answers "No" to `overwrite_ok`.

```console
$ toha apply ./web-service ./out
The target directory is not empty. Continue anyway? No
stopped: overwrite_ok — no files were written and no hooks ran
$ echo $?
0
```

Driver code (`main.rs`, direct apply path, replacing the `drive -> Completed`
call site at `main.rs:818`/`terminal.rs:195`):

```rust
match cli::terminal::drive(interview, &mut ask, record)? {
    DriveResult::Stopped(ended) => {
        // No `Completed` exists here, so there is no way to reach `Plan::build`.
        println!("stopped: {} — no files were written and no hooks ran", ended.by());
        return Outcome::Written(vec![]);          // exit 0
    }
    DriveResult::Completed(completed) => {
        let dry_run = cli_dry_run || completed.disposition() == Disposition::DryRun;
        let plan = Plan::build(&template, &completed, path)?;
        if dry_run {
            // Existing `--dry-run` rendering path, unchanged (main.rs:1069).
            print_plan(&plan);
            return Outcome::Written(vec![]);
        }
        plan.apply_reporting(path, options, runner, &mut report)?  // the only write site
    }
}
```

### 1.3 Call site B — headless (wire protocol) `--answers`

An agent submits a document that answers `overwrite_ok: false`.

```console
$ toha apply ./web-service ./out --answers answers.json --format json
```

```json
{
  "protocol": 1,
  "status": "stopped",
  "context": { "target": "/abs/out", "template": "web-service", "commit": null },
  "by": "overwrite_ok",
  "answers": { "overwrite_ok": false },
  "messages": []
}
```

And a dry-run answer (`preview_first: true`) completes with a disposition:

```json
{
  "protocol": 1,
  "status": "complete",
  "context": { "target": "/abs/out", "template": "web-service", "commit": null },
  "answers": { "overwrite_ok": true, "preview_first": true, "...": "..." },
  "disposition": "dry-run",
  "messages": []
}
```

Driver code (`protocol::answer_headless` consumer, `main.rs:966`):

```rust
match protocol::answer_headless(&template, interview, raw)? {
    Headless::Stopped { ended, .. } =>
        Outcome::Document(protocol::stopped_document(&ended, &context), 0),
    Headless::Completed { completed, .. } =>
        Outcome::Document(protocol::complete_document(&completed, &context), 0),
    Headless::Pending { pending, rejections, .. } =>
        Outcome::Document(protocol::batch_document(pending.batch(), &context, Some(&rejections)), 4),
}
```

### 1.4 Call site C — crate / library

A library caller assembles the engine directly and matches the new variant.

```rust
use toha::{Interview, Seed, Plan, ApplyOptions, Disposition};

let mut interview = Interview::start(&template, seed)?;
loop {
    interview = match interview {
        Interview::Asking(p)  => p.answer(next_document())?,          // drive as today
        Interview::Ended(ended) => {                                  // NEW: stop
            log::info!("interview stopped by {}", ended.by());        // no Completed -> cannot plan
            return Ok(Report::Stopped { by: ended.by().clone() });
        }
        Interview::Complete(c) => {
            let plan = Plan::build(&template, &c, &target)?;
            return match c.disposition {
                Disposition::DryRun  => Ok(Report::Planned(plan)),    // show, do not apply
                Disposition::Proceed => Ok(Report::Applied(plan.apply(&target, opts, runner)?)),
            };
        }
    };
}
```

The action lives on a type the crate caller observes (`Interview::Ended`,
`Completed::disposition`), never in `main.rs` glue — per the grounding
requirement that the crate driver see the decision.

---

## 2. Shape — data structures first

### 2.1 Declared action (template domain)

```rust
// template.rs — extends the existing closed QuestionKind::Confirm.

pub enum QuestionKind {
    // ... Text / Multiline / Select / MultiSelect / TextLoop unchanged ...
    Confirm {
        default: Option<Typed<bool>>,   // UNCHANGED — pure boolean behavior preserved
        action: Option<ConfirmAction>,  // None => an ordinary confirm
    },
}

/// Actions bound to a confirm's two answers. INVARIANT: at least one arm is
/// `Some` — `ConfirmAction` is only constructed by the loader, which rejects an
/// empty action, so a `Some(ConfirmAction)` always carries a live action.
pub struct ConfirmAction {
    pub on_true:  Option<ActionKind>,
    pub on_false: Option<ActionKind>,
}

/// A control action a confirm answer triggers. Declared data, never inferred.
pub enum ActionKind {
    Stop,
    DryRun,
    Skip(SkipScope),
}

/// INVARIANT: `Group` is only constructed for a confirm inside a group; the
/// loader rejects `skip: group` at interview top level, so the runtime never
/// sees a `Group` scope without an enclosing group to bound it.
pub enum SkipScope {
    /// Skip every remaining question in the whole interview.
    Rest,
    /// Skip the remaining siblings of the immediately-enclosing group.
    Group,
}
```

`action` never appears on `Prompt`, on the JSON Schema, or on any wire type — it
is parsed into these domain types behind the interview boundary, per
boundary-discipline (no transport types on the public surface). The terminal,
schema, and staging layers do not know actions exist; they only observe the
effect through the outcome types below.

### 2.2 Effect inside the walk (engine-private)

The walk already returns "keep going vs. blocked" as `Result<bool, EvalError>`.
That boolean cannot express a third outcome, so it is widened to a small
enum — the single load-bearing engine type of this design.

```rust
// interview.rs (private to the walk)

/// The control flow a walked node induces. Replaces the walk's `bool`
/// (`true` == `Reached`, `false` == `Blocked`).
enum Flow {
    /// Finished this node list; the caller continues normally.
    Reached,
    /// Stopped at a node awaiting an answer (the former `Ok(false)`).
    Blocked,
    /// A confirm `stop` action ended the interview at this node.
    Stopped,
    /// A confirm `skip: rest` action fired: this list and every enclosing
    /// list skip their remaining siblings.
    SkipRest,
}

/// What consulting an active confirm's action yields, mapped into `Flow` by the
/// walk loop. `skip: group` is handled locally (flip the loop's skip flag) and
/// needs no `Flow` variant.
enum Act {
    None,
    Stop,
    DryRun,
    SkipHere,   // skip: group — skip the remaining siblings of THIS list
    SkipRest,   // skip: rest  — propagate skip to every enclosing list
}
```

`dry-run` is **not** a `Flow` variant. It does not alter control flow — it is a
sticky *disposition* accumulated on the walk state and carried into a normal
completion (§2.3). This is the deliberate asymmetry: **stop ends the walk;
dry-run rides to completion so the plan it shows is the full plan.**

```rust
struct Advance<'a> {
    // ... existing fields (template, seed, answers, held, skipped, messages,
    //     hooks, visited, batch, blocked) ...
    /// Raised to `DryRun` when an active confirm's dry-run action fires. Re-derived
    /// from recorded answers on every walk, so nothing extra is persisted.
    disposition: Disposition,
    /// The confirm id that stopped the walk, set only on `Flow::Stopped`.
    stopped_by: Option<Id>,
}
```

### 2.3 Result surface (public)

```rust
// interview.rs

pub enum Interview<'a> {
    Asking(Pending<'a>),
    Complete(Completed),   // now carries a disposition
    Ended(Ended),          // NEW — a confirm `stop` action ended the interview
}

/// How a completed interview should be applied. Re-derived every walk from the
/// recorded confirm answers; never persisted (replay recomputes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Normal completion: plan, then apply (writes + hooks).
    Proceed,
    /// A confirm dry-run action fired: plan and show, write nothing, run no hooks.
    DryRun,
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

/// A stop action ended the interview. Deliberately carries NO `Completed` and
/// NO hooks: there is no value here that `Plan::build`/`apply` will accept, so
/// "stop writes nothing and runs no hooks" is an invariant of the type, not a
/// rule a driver must remember. Answers/messages are retained for reporting only.
pub struct Ended {
    by: Id,
    answers: Answers,
    messages: Vec<String>,
    last_messages: Vec<String>,
    /// Kept private, used only to classify early-answer failures during the
    /// tentative advance (§2.6). Not exposed and not written to the wire.
    skipped: Skipped,
}
impl Ended {
    /// The confirm question that stopped the interview.
    pub fn by(&self) -> &Id { unimplemented!("not implemented") }
    pub fn answers(&self) -> &Answers { unimplemented!("not implemented") }
    pub fn messages(&self) -> &[String] { unimplemented!("not implemented") }
}
```

Interface depth: the public surface grows by exactly one enum variant
(`Interview::Ended`), one field (`Completed::disposition`), one small enum
(`Disposition`), and one opaque struct (`Ended`) with three accessors. Behind
that surface sits all of skip-scope unwinding, dry-run stickiness, effect
ordering, and the tentative-advance interaction — pulled into the callee per the
runner discipline. Drivers route the outcome; they re-implement no policy.

### 2.4 The funnel — one place the action is consulted

```rust
impl<'a> Advance<'a> {
    /// Consults the action bound to an ACTIVE confirm whose boolean is already
    /// recorded. This is the single funnel: it runs only on the active path,
    /// after `when` and after the held-answer check, keyed off active-vs-skipped
    /// (NOT the recorded bool), so a skipped/inactive confirm can never fire.
    ///
    /// Ordinary confirms (action == None) and every non-confirm return `Act::None`.
    fn confirm_act(&self, q: &Question) -> Act {
        // TODO:
        //   let QuestionKind::Confirm { action: Some(a), .. } = &q.kind else { return Act::None };
        //   // A skipped confirm is never routed here (see walk), but guard anyway:
        //   if self.skipped.contains_key(&q.id) { return Act::None; }
        //   let Some(Answer::Bool(b)) = self.answers.get(&q.id) else { return Act::None };
        //   match if *b { &a.on_true } else { &a.on_false } {
        //       None                              => Act::None,
        //       Some(ActionKind::Stop)            => Act::Stop,
        //       Some(ActionKind::DryRun)          => Act::DryRun,
        //       Some(ActionKind::Skip(Rest))      => Act::SkipRest,
        //       Some(ActionKind::Skip(Group))     => Act::SkipHere,
        //   }
        unimplemented!("not implemented")
    }
}
```

### 2.5 Walk integration — effect emitted at the node position

The action fires at the two existing points where an active confirm's boolean
becomes final — unified so there is exactly one funnel call per point:

- **already-answered active confirm** (`interview.rs:1059` branch): the answer
  was recorded by a prior `Pending::answer` step; today the walk `continue`s.
- **held answer accepted this walk** (`interview.rs:1097` Ok branch): today the
  walk inserts and `continue`s.

Both become: record/confirm the answer, then `apply_act(self.confirm_act(q))`.

```rust
fn walk(&mut self, nodes: &'a [Node], prefix: &str, skip: bool) -> Result<Flow, EvalError> {
    let mut skip = skip;         // may be raised locally by `skip: group`/`skip: rest`
    let mut skip_rest = false;   // set when a `skip: rest` must propagate to the parent
    for (i, node) in nodes.iter().enumerate() {
        let key = format!("{prefix}/{i}");
        match node {
            Node::Question(q) => {
                if self.answers.contains_key(&q.id) {
                    if self.skipped.contains_key(&q.id) { self.warn_if_held(&q.id); continue; }
                    // ACTIVE + already answered: consult the action here.
                    match self.act(self.confirm_act(q), &mut skip, &mut skip_rest) {
                        Some(flow) => return Ok(flow),   // Stopped
                        None => continue,
                    }
                }
                if skip { /* existing skip-record path, interview.rs:1065-1072 */ continue; }
                if !question_ready(..) { return self.block_flow(node); }
                let active = /* existing `when` eval, interview.rs:1077 */;
                if active {
                    let prompt = make_prompt(..)?;
                    if let Some(raw) = self.held.shift_remove(&q.id) {
                        match check_answer(.., raw) {
                            Ok(answer) => {
                                self.answers.insert(q.id.clone(), answer);
                                // ACTIVE + accepted-held: same funnel.
                                match self.act(self.confirm_act(q), &mut skip, &mut skip_rest) {
                                    Some(flow) => return Ok(flow),
                                    None => continue,
                                }
                            }
                            Err(CheckError::Rejected(r)) => { /* batch error, interview.rs:1101 */ }
                            Err(CheckError::Eval(e)) => return Err(e),
                        }
                    }
                    self.batch.items.push(Item::Prompt(prompt));   // fresh prompt: action fires later
                } else {
                    /* existing inactive default+skip path, interview.rs:1114-1120 */
                }
            }
            Node::Group(g) => {
                // ... existing readiness/active/skipped_descendants_ready checks ...
                match self.walk(&g.nodes, &key, !active || skip)? {
                    Flow::Reached  => {}
                    Flow::Blocked  => return Ok(Flow::Blocked),
                    Flow::Stopped  => return Ok(Flow::Stopped),
                    Flow::SkipRest => { skip = true; skip_rest = true; }  // propagate outward
                }
            }
            // Node::Computed / Hook / Message unchanged.
            _ => { /* existing */ }
        }
    }
    Ok(if skip_rest { Flow::SkipRest } else { Flow::Reached })
}

/// Maps an `Act` onto the walk. Returns `Some(Flow::Stopped)` to unwind, or
/// `None` to continue the loop (after mutating skip flags).
fn act(&mut self, act: Act, skip: &mut bool, skip_rest: &mut bool) -> Option<Flow> {
    match act {
        Act::None     => None,
        Act::Stop     => { self.stopped_by = Some(/* q.id */); Some(Flow::Stopped) }
        Act::DryRun   => { self.disposition = Disposition::DryRun; None }  // sticky, keep walking
        Act::SkipHere => { *skip = true; None }                            // skip: group
        Act::SkipRest => { *skip = true; *skip_rest = true; None }         // skip: rest
    }
}
```

Key properties this yields for free from existing machinery:

- **Skip defaults only the unanswered remainder.** An already-answered question
  (even a batch-sibling answered before the confirm in the same document) hits
  the `answers.contains_key` branch and keeps its value regardless of `skip` —
  identical to today's line `interview.rs:1059`. So "what happens to the other
  answers in the batch" needs no new rule: answered siblings stand; unanswered
  ones take defaults via the existing `self.skip(q, default)`.
- **`skip: group` vs `skip: rest`** differ by exactly one bit: whether the skip
  propagates to the parent (`skip_rest`). Group scope flips only the local
  loop's remaining siblings and returns `Flow::Reached`; rest scope returns
  `Flow::SkipRest`, and each enclosing list flips its own remainder. Arbitrary
  nesting composes because propagation is one level per return.
- **Skipped-value semantics are unchanged.** Skipped questions record their
  configured/own default or `empty_answer(kind)` via the untouched
  `Advance::skip`, and held answers for them raise the existing
  `answer for "<id>" was not used: the question was skipped` warning via
  `warn_if_held`. No new skipped-default rule is invented (grounding "Avoid").

### 2.6 `advance` and the tentative-advance atomicity interaction

```rust
fn advance(mut state: Advance<'_>) -> Result<Interview<'_>, EvalError> {
    let flow = state.walk(&state.template.interview, "", false)?;
    match flow {
        Flow::Stopped => Ok(Interview::Ended(Ended {
            by: state.stopped_by.expect("stopped_by set on Flow::Stopped"),
            answers: state.answers,
            last_messages: state.messages[state.step_start..].to_vec(),
            messages: state.messages,
            skipped: state.skipped,
        })),
        Flow::Blocked => { /* existing block/error handling, interview.rs:1284 */ }
        Flow::Reached | Flow::SkipRest => {
            let has_prompt = /* existing */;
            if has_prompt {
                Ok(Interview::Asking(Pending { /* existing fields; disposition NOT stored */ }))
            } else {
                Ok(Interview::Complete(Completed {
                    disposition: state.disposition,   // re-derived this walk
                    /* existing fields */
                }))
            }
        }
    }
}
```

`Pending::answer` (`interview.rs:1392`) keeps its signature and its atomic
transaction. Two precise, load-bearing interactions:

1. **A rejected document discards any effect.** The tentative `advance` at
   `interview.rs:1503` may now produce `Interview::Ended` or a dry-run
   `Completed`. On rejection the function returns `rejected(self, rejections)`
   with the **original** `Pending` (`interview.rs:1531/1533`); the `advanced`
   value — and any effect inside it — is dropped. So a valid confirm effect that
   shares a batch with a rejected sibling never leaks. This is exactly the
   approved 1072 atomicity contract ("a rejected answer never influences
   classification; a rejected document never persists"), and it holds without
   new code because effects live only inside the discarded advance.

2. **The `stands` classifier must read the effect outcome via the skipped set.**
   The early-failure classifier at `interview.rs:1519` inspects `advanced` to
   decide whether a probe error `stands` or is `dropped`. It must handle the two
   new Ok shapes, keyed off the skipped set (never the effect kind), per 1072
   policy 2 (drop an early error only if *proven skipped*):

   ```rust
   let stands = |id: &Id| match &advanced {
       Ok(Interview::Asking(n))   => !n.skipped.contains_key(id),
       Ok(Interview::Ended(e))    => !e.skipped.contains_key(id),   // NEW
       Ok(Interview::Complete(_)) => false,                          // reached & skipped
       _ => false,
   };
   ```

   A stop that short-circuits before the probe's question is reached leaves that
   id absent from `skipped` → `stands` → the rejection is kept → the document is
   rejected → the `Ended` is discarded (interaction 1). A rejected answer can
   therefore never produce a stop, satisfying the "rejected answer never enters
   skip/reachability classification" invariant.

### 2.7 Composition order at a reached confirm

`readiness → when → (held check | fresh prompt) → [answer recorded] → action`.
The action is consulted strictly after `when` (an inactive confirm takes the
default+skip branch and is never routed to `confirm_act`) and strictly after the
held-answer check (a rejected held answer becomes a batch error and is never
recorded, so the funnel's `Answer::Bool` guard fails and nothing fires). This is
the ordering the grounding's insertion point 4 requires.

---

## 3. Module / seam diagram

```
 template.yml ──load──▶ template.rs
   parse `action`        QuestionKind::Confirm { default, action: Option<ConfirmAction> }
   LOAD RULES:           ├─ action on non-confirm            ─┐
     reject at load      ├─ empty action                      ├─▶ LoadError (before any interview)
                         └─ skip: group at top level         ─┘
                                   │
                                   ▼
 interview.rs (engine funnel — the deep interface)
   walk() ── reaches active confirm, records Answer::Bool
        └─▶ confirm_act(q) ─▶ Act ─▶ act() ─▶ Flow
                                   │  Stop     → Flow::Stopped   → Interview::Ended
                                   │  DryRun   → disposition     → Completed{DryRun}
                                   │  SkipHere → skip flag (grp) → Completed (existing skip)
                                   │  SkipRest → skip flag + prop→ Completed (existing skip)
                                   ▼
   Interview::{ Asking | Complete(Completed{disposition}) | Ended(Ended) }
                                   │
        ┌──────────────┬──────────┴───────────┬──────────────────┬──────────────┐
        ▼              ▼                      ▼                  ▼              ▼
   terminal.rs     protocol.rs           staging.rs          main.rs        lib.rs
   DriveResult     Headless::            replay() ──▶ same    routes         crate caller
   {Completed,      {Completed,          Interview arms;      outcome to     matches
    Stopped}         Pending,            `stopped_document`   Plan/apply     Interview::Ended
                     Stopped}            on the wire          or reports     + disposition
        │              │                      │                  │
        └──────────────┴──────────────────────┴──────────────────┘
                 thin adapters: route the outcome, re-implement NO policy
                                   │
          Completed(Proceed) ──▶ Plan::build ──▶ Plan::apply   (only write/hook site, apply.rs)
          Completed(DryRun)  ──▶ Plan::build ──▶ show, no write/hook  (reuses --dry-run path)
          Ended              ──▶ (no Completed exists) ──▶ report only, structurally cannot write
```

Trace length: a reader follows input→output through `template.rs` (declaration),
`interview.rs` (funnel + outcome), and one driver file — three files, per the
laziness-protocol/minimize-reader-load discipline.

---

## 4. Error / results contract

### 4.1 Engine

- `advance` / `Interview::start` / `Pending::answer`: **signatures unchanged**;
  the error type stays `EvalError` / `AnswerError`. The only change is a new
  `Ok` variant `Interview::Ended` and a `disposition` field on `Completed`.
- `Ended` carries no hooks and no `Completed`; there is **no API path from
  `Ended` to `Plan::build`**. "Stop writes nothing / runs no hooks" is therefore
  a property of the type system, not a driver convention.
- `Disposition::DryRun` on a `Completed` means the driver must render the plan
  and not apply. Its no-write guarantee is driver-enforced, identical to the
  existing `apply --dry-run` flag precedent (`main.rs:1069`).

### 4.2 Drivers

- **Terminal:** `drive` returns `DriveResult { Completed(Completed), Stopped(Ended) }`
  instead of a bare `Completed`. Every `drive` call site (`main.rs:818/1016`)
  matches the two arms.
- **Headless:** `answer_headless` returns `Headless` with a new
  `Stopped { ended, accepted }` arm beside `Completed`/`Pending`.
- **Staged:** `stage`/`continue` record the submission (a stop/dry-run is
  triggered by a *recorded answer*, so it is a normal submission) and emit the
  appropriate document; they still never call `Plan`/`apply`. `abort` is
  unaffected. `StagedRecord::replay*` returns `Interview::Ended` when a replayed
  answer stops the interview — deterministic from stored submissions.
- **Direct:** the `apply TEMPLATE PATH` path routes `Ended` → report + exit,
  `Completed` → dry-run-or-apply as in call site A.
- **Crate:** `Interview::Ended`, `Ended`, and `Disposition` are public.

### 4.3 Exit codes (proposed)

- `stopped`  → exit `0` (a deliberate, successful stop). Open question below.
- `complete` with `dry-run` → exit `0`.
- `questions` (headless, more needed) → exit `4` (unchanged).
- rejection → unchanged.

### 4.4 Determinism / persistence

No new persisted state. `StagedRecord` (`staging.rs:20`) still stores only raw
`submissions` + identity + frozen `now`. `disposition` and the stop effect are
**re-derived on every walk** from the recorded confirm answers, so resume/replay
reproduces them exactly. Nothing is added to the replay input.

### 4.5 Composition with the approved error-attribution predecessor (1072)

- Depends on the approved **contract**, not the unmerged 1056 runtime: the
  answer-transaction **atomicity invariant** (§2.6) and the target-identity
  **type**. Both hold whether the mechanism is today's emergent
  `unless_skipped` loop or the future `SubmissionTxn`, and whether the target is
  `&Path` (now) or `&CanonicalTarget` (post-1056).
- Honors all four "do not" constraints: adds no second target normalizer (never
  touches `staging::canonical_target`), reconstructs no configured origins,
  accepts no rejected answer into probe state (§2.6 interaction 1), infers no
  new skip/default policy (reuses `Advance::skip`/`default_ready`).
- **Producer-composition:** confirm-flow adds **zero** new configured-default
  consumption points — it only reads booleans already recorded. Wherever it
  touches start/replay/headless it routes through the origin-bearing consuming
  `Resolution` (`Resolution::warnings()`, `Resolution::start(template, now)`,
  `StagedRecord::replay_with_resolution(template, resolution)`), never the flat
  `a62061f` projection, and never calls `into_flat_defaults` (the only
  sanctioned provenance-shedding path, which confirm-flow has no reason to use).
  The ordinary flat `Seed` route is untouched. Confirm-flow is provenance-neutral.

---

## 5. Proposed canonical-document edits (described, owned by impl 1062)

### 5.1 `template-format.schema.yml` — `$defs/question`

- Add an optional `action` property referencing a new `$defs/confirm-action`.
  Because the object is `additionalProperties: false`, `action` must be listed
  explicitly; it stays additive to the closed schema.
- Add an `allOf` conditional: `if type is not "confirm", then action is absent`
  (best-effort structural echo of the authoritative load rule).
- New `$defs/confirm-action`:

  ```yaml
  confirm-action:
    type: object
    additionalProperties: false
    minProperties: 1                 # empty action rejected
    properties:
      when_true:  { $ref: "#/$defs/action-kind" }
      when_false: { $ref: "#/$defs/action-kind" }
  action-kind:
    oneOf:
      - const: stop
      - const: dry-run
      - type: object
        additionalProperties: false
        required: [ skip ]
        properties:
          skip: { enum: [ rest, group ] }
  ```

- `template-format.yml` prose: document `action`, the two arms, the three
  action kinds, the `skip` scopes, and the four load-time rejections. Add the
  `action`-on-confirm-only rule beside the existing `loop`/`options` gating
  (template-format.yml:119-130). State that `skip: group` requires an enclosing
  group. Cross-reference that action-skip reuses `when`-skip's recorded-default
  and "answer not used" warning semantics.

### 5.2 `interview-protocol.schema.yml`

- Add `#/$defs/stopped` to the top-level `oneOf`:

  ```yaml
  stopped:
    type: object
    additionalProperties: false
    required: [ protocol, status, context, by, answers, messages ]
    properties:
      protocol: { const: 1 }
      status:   { const: stopped }
      context:  { $ref: "#/$defs/context" }
      by:       { $ref: "#/$defs/identifier" }   # the confirm that stopped it
      answers:  { $ref: "#/$defs/answers" }
      messages: { $ref: "#/$defs/messages" }
  ```

- Extend `#/$defs/complete` with an optional `disposition`:

  ```yaml
  disposition:
    description: >-
      Present and equal to "dry-run" when a confirm dry-run action fired: the
      plan should be shown and nothing written. Absent for ordinary completion,
      so ordinary complete documents are byte-identical to today's.
    const: dry-run
  ```

  `disposition` is omitted for `Disposition::Proceed`, keeping ordinary
  documents unchanged (driver-parity, no wire regression).

- `interview-protocol.yml` prose: document the `stopped` document and the
  `disposition` field; state that a template-driven `dry-run` differs from the
  `apply --dry-run` CLI flag (the flag is caller-initiated and apply-only; the
  disposition is template-declared and travels the wire) and from ordinary
  planning (no writes/hooks). State the new wire status is additive to the closed
  document set.

### 5.3 `command-line-interface` docs

- Note that `apply` may terminate via a confirm `stop` (exit 0, no writes/hooks)
  and that a confirm `dry-run` composes with `--dry-run` by union.

---

## 6. Behaviors to prove (falsifiable)

**Declaration / load (prefer load time):**

1. `action` on a `text`/`select`/etc. question → load error naming the confirm
   corrective form; no interview starts.
2. `action: {}` (empty) → load error `action must set when_true or when_false`.
3. `{ skip: group }` on a top-level confirm → load error naming `skip: rest`.
4. An unknown arm value → load error listing `stop`, `dry-run`,
   `{skip: rest|group}`.
5. A confirm with no `action` behaves byte-for-byte as today (records
   `Answer::Bool`, JSON boolean, terminal `inquire::Confirm`) — regression pin.

**Stop (both answers, all five drivers):**

6. `when_false: stop`, answered `false`: terminal / headless / staged-continue /
   direct / crate each yield the stop outcome; **no file exists at target and no
   hook ran** (assert filesystem + `RecordingRunner` empty).
7. Same template answered `true` → interview proceeds to normal completion and
   applies (proves the action fires only on its declared truth value).
8. Driver-parity: the terminal, headless, staged, and direct results for the
   same stop are byte-equal where they share a representation (wire `stopped`
   document identical for headless and staged-continue).

**Dry-run:**

9. `when_true: dry-run`, answered `true`: plan is computed and shown, **no file
   written, no hook ran**; `complete` document carries `disposition: "dry-run"`.
10. Answered `false` → ordinary apply; `complete` document has **no**
    `disposition` field (byte-identical to a no-action confirm).
11. Template `dry-run` unioned with CLI `--dry-run`: either alone, or both,
    yields the plan-only path; both-absent applies.

**Skip scope (whole vs group, nested, recorded values):**

12. `skip: rest` answered to skip: every remaining question — including inside
    deeper groups — records its configured/own default or `empty_answer(kind)`;
    the interview completes; generated output matches an interview where those
    questions were left at default.
13. `skip: group` answered to skip inside a nested group: only that group's
    remaining siblings are skipped; questions after the group are still asked.
14. A held/early answer for a question skipped by an action raises the exact
    existing `answer for "<id>" was not used: the question was skipped` warning
    at the skip position.
15. A confirm and later questions share one batch; the confirm fires `skip: rest`
    while a batch-sibling was answered in the same document → the sibling keeps
    its answer; only unanswered remainder is defaulted.

**Composition / determinism / negative:**

16. A confirm made inactive by `when: false` never fires its action (no stop, no
    skip, no disposition), even though it records a default `Answer::Bool` —
    proves keying off active-vs-skipped, not the recorded bool.
17. Resume/replay: a staged interview whose stored submissions stop / dry-run /
    skip reproduces the identical outcome and identical generated output on
    replay; `StagedRecord` gains no new fields.
18. A batch with one rejected early answer and one valid stop-confirm →
    `AnswerError::Rejected` with the original pending; **no `Ended` is produced**,
    nothing persists (atomicity, §2.6).
19. A batch where a valid stop-confirm sits beside an early answer *proven
    skipped* by the tentative step → the early error is dropped per 1072 policy
    2, and the stop stands.
20. A rejected document that would otherwise dry-run → document rejected, no
    `disposition` leaks into any persisted or returned state.
```
