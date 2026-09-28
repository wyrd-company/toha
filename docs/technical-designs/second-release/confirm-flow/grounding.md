---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
    - command-line-interface
---

# Confirm-controlled interview and apply flow — grounding

Phase A traces the current caller-to-result flow for confirm questions, the
interview walk, and the plan/apply pipeline, so the arena candidates design the
confirm-answer→action feature on real behavior. Evidence is from this worktree's
base `dfe7ba017ebef525310db8b8ab4ead58fae2d147` (epic `second-release`) with the
integrated presets layer read at `a62061fdce74b6b1a743c70565dd9fbeec2413bb`.
File:line citations resolve against `a62061f` unless noted.

## Phase position

- Ground: complete at the evidence level recorded here.
- Sketch: candidate arena consumes this artifact; candidates receive it and the
  common task, not the rubric.
- Agree: Phase C explicit Bob checkpoint follows synthesis and deck.

## Scope and the shape of the change

Today a `confirm` question yields a boolean answer that only feeds later
`when`/template expressions. The feature lets a template map a confirm answer to
a control **action** — **stop**, **dry-run**, or **skip** — declared as data on
the question, never inferred from prompt text. Ordinary confirms keep pure
boolean behavior. The engine has **no** control-flow surface today where an
answer's *value* alters execution, so the change introduces one, and it must
travel through every driver and stay replay-deterministic.

## Approved predecessor contracts consumed

Two predecessor designs sit under `docs/technical-designs/second-release/`.
They are integrated **unevenly** at `a62061f`, which is the single most
load-bearing grounding fact for this design.

### Presets / configured defaults (design `template-defaults`, impl 1058) — INTEGRATED

- `Seed { now: jiff::Zoned, defaults: IndexMap<Id, RawAnswer> }` — `interview.rs:38`.
  Unchanged, and the fixed application-default carrier.
- `Interview::start(template: &Template, seed: Seed) -> Result<Interview, EvalError>`
  — `interview.rs:1322`.
- `pub struct Resolution { pub defaults: IndexMap<Id, RawAnswer>, pub warnings: Vec<String> }`
  — `interview.rs:685` (flat shape).
- `configured_defaults(formal_name, template, presets: &IndexMap<PresetName, ConfigEntry<Value>>, mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>) -> Result<Resolution, EvalError>`
  — `interview.rs:690`.
- Callers destructure and build the seed themselves:
  `let Resolution { defaults, warnings } = configured_defaults(...)?; report warnings; Interview::start(template, Seed { now, defaults })` — `main.rs:430/438`, `:509`, `:685`, `:932`.
- `config.rs`: `DefaultSource::{ Ref(PresetName), Literal(Value) }` — `config.rs:28`;
  `ConfigEntry<T> { value: T, origin: ConfigOrigin }` — `config.rs:44`;
  `ConfigOrigin { layer: ConfigLayer, path: PathBuf }` — `config.rs:39`.

### Error attribution and early answers (design 1072, impl 1056) — APPROVED, NOT YET INTEGRATED

Approved revision consumed: design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`, checkpoint
commit `067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, Postplan `a2q5sdtr0ctw` v1;
Bob accepted all five recommendations. The five fixed policies confirm-flow
composes with:

1. Reject a known-invalid early answer while skip is unresolved.
2. Omit a proven-skipped early error beside a current failure, with no warning
   from the rejected probe.
3. Check a skipped template default only against constraints whose references are
   already ready.
4. Consume configured origins through `Resolution` while preserving the ordinary
   flat `Seed`.
5. Use the opaque `CanonicalTarget` built by
   `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>` at every
   identity-sensitive consumer; the raw-`&Path` source API at those consumers is
   removed.

Approved constraints: do not add a second target normalizer, do not reconstruct
configured origins, do not accept a rejected answer into probe state, do not
infer a different skip/default policy.

**Integration gap (evidence).** At `a62061f` none of the distinctive 1072 types
exist: `canonical_target(&Path) -> Result<PathBuf, StagingError>` returns a bare
`PathBuf` — `staging.rs:40`; `StagingError { Io, Json, Replay(String) }` —
`staging.rs:31`; `Store::path_for/load/remove` take `&Path` — `staging.rs:78/82/118`;
`Plan::build`/apply take `target: &Path` — `plan.rs:182/277/334`;
`protocol::Context { pub target: String }` — `protocol.rs:14`. Grep count is 0
for `CanonicalTarget`, `SubmissionTxn`, `SkipDisposition`, `TemplateFault`,
`Resolution::start`, `replay_with_resolution`, `into_flat_defaults`. The
early-answer behavior that 1072's `SubmissionTxn` will formalize is presently
**emergent** from the pre-existing `Pending::answer` loop
(`unless_skipped` + one tentative `advance`, `interview.rs:1392-1478`).

**Design consequence.** Confirm-flow composes with the approved 1072 *contract*
(atomic answer transaction, `SkipDisposition`, opaque `CanonicalTarget` at
identity consumers) and does **not** wait for 1056 runtime. It expresses its
dependency as the answer-transaction **atomicity invariant** and the
target-identity **type**, both of which hold whether the mechanism is today's
emergent loop or the future `SubmissionTxn`, and whether the target is `&Path`
(now) or `&CanonicalTarget` (post-1056). The paired implementation (1062)
sequences relative to 1056; see Risk.

**Producer-composition correction (authoritative).** The flat
`Resolution { defaults, warnings }` observed at `a62061f` is **current-state
grounding only**. The **final configured contract** confirm-flow targets is the
1072/1056 **origin-bearing consuming `Resolution`**: configured defaults are
consumed *through* it via `Resolution::warnings()`, `Resolution::start(template,
now)`, and `StagedRecord::replay_with_resolution(template, resolution)` — the
resolution carries its origins to the consuming boundary and is **not** flattened
en route. `into_flat_defaults()` is the *only* sanctioned provenance-shedding
escape, and the ordinary flat `Seed` route is unchanged and independent. Confirm-
flow must therefore, wherever it touches configured-default consumption (start,
replay, headless), route through the origin-bearing `Resolution` and must **not**
reconstruct configured origins or re-derive an id-only key from the flat
projection. The analogous flat-projection text was corrected and held on sibling
design 1075; this design must not reintroduce it.

## Interview engine: caller-to-result flow and state ownership

- State machine: `enum Interview<'a> { Asking(Pending<'a>), Complete(Completed) }`
  — `interview.rs:44`. `advance(Advance) -> Result<Interview, EvalError>` is the
  **sole constructor** of results — `interview.rs:1276`, classifying at
  `:1241` (complete) / `:1252` (asking) / error on unresolved dependency.
- `Pending<'a>` owns all between-step state (private): `template`, `seed`,
  `answers: Answers`, `held: RawAnswers`, `skipped: Skipped`, `messages`,
  `hooks`, `visited`, `batch` — `interview.rs:49`.
- `Pending::answer(self, incoming: RawAnswers) -> Result<Interview, AnswerError>`
  — `interview.rs:1392` — **consumes `self` by value**, rebuilds a fresh
  `Advance` (`step_start = messages.len()`), re-walks, and returns a brand-new
  `Interview`. On rejection it returns the original `Pending` inside
  `AnswerError::Rejected { pending, rejections }` — `interview.rs:257`. There is
  no in-place mutation across steps.
- Walk: `Advance::walk(nodes, prefix, skip)` — `interview.rs:1054` — depth-first
  over `Vec<Node>`, pushing `Item::Prompt`/`Item::Message` into `batch`,
  recording answers and skips, rendering hooks. `Advance::skip` records a
  question's default/empty and marks it in `Skipped` — `interview.rs:1039`.
- `Completed { pub answers, pub messages, pub last_messages, pub hooks, pub now, skipped, step_start }`
  — `interview.rs:61`. `Plan::build` and `Plan::apply` consume only `Completed`
  (answers + hooks + now) — downstream of the engine.
- Answer types: `enum Answer { Text, Bool, List, None, Value }` — `interview.rs:15`;
  `Answers = IndexMap<Id, Answer>` (one flat, insertion-ordered map);
  `RawAnswer(pub Value)` / `RawAnswers` inbound — `interview.rs:33`.

## Confirm / boolean today (must be preserved unchanged)

- Declaration: `QuestionKind::Confirm { default: Option<Typed<bool>> }` —
  `template.rs:114`, parsed from `type: confirm` at `template.rs:685`.
- Parse: `parse_kind(Confirm) => Answer::Bool` — `interview.rs:511`; the choke
  point that yields the boolean answer.
- Empty: `empty_answer(Confirm) = Answer::None` (not `Bool(false)`) —
  `interview.rs:535`. An optional unanswered confirm is `null`.
- Prompt kind: `PromptKind::Confirm` — `interview.rs:185`; protocol maps it to
  JSON Schema `boolean`, `x-toha-type: confirm` — `protocol.rs:76`,
  `interview-protocol.yml:37`.
- Terminal: `inquire::Confirm` seeded from `Answer::Bool` — `cli/terminal.rs:56`.

Ordinary boolean behavior = this exact chain (`Typed<bool>` default →
`Answer::Bool` → JSON bool). Any action layer must leave `Answer::Bool`
recording intact and must be opt-in per question.

## Groups, nesting, `when`, `default`, early answers, batch semantics

- Group: `struct Group { name: Id, nodes: Vec<Node>, when: Option<Expr> }` —
  `template.rs:144`; `Node::Group(Group)` nests arbitrarily deep. The group name
  is for diagnostics and is **not** an answer id (`template-format.yml:80`).
- Walk handles a group at `interview.rs:1102`: block if `when` refs unready;
  evaluate `active`; if inactive check `skipped_descendants_ready` then recurse
  with the `skip` flag propagated so the whole subtree skips —
  `interview.rs:1122/1131`. Answers stay in one flat `Answers` map; groups only
  affect ordering and conditional skipping.
- Effective order at a reached question (`interview.rs:1002-1064`):
  already-answered → ancestor-skip (default-ready → render default → skip) →
  `question_ready` (else block) → evaluate `when` → if active: `make_prompt`,
  and if a held answer exists validate+format it now (`check_answer`), else emit
  the prompt → if inactive: default-ready → render default → skip. So:
  **readiness → when → (held check | prompt) | (default + skip)**.
- `default`: `render_default` — `interview.rs:385`. Configured default
  (`seed.defaults`) wins over the question's own `default`. A skipped question
  records its default; an inactive `when` still assigns an answer and marks the
  id skipped — a skipped confirm has `Answer::Bool`/`Answer::None` recorded.
- Early/held answers: an inbound answer for a question not in the current batch
  is held (`Pending.held`), eagerly validated against literal-only constraints
  (`Rules::of_question`) and pushed to `unless_skipped`; a single tentative
  `advance` decides which held early failures are `dropped` because their
  question was skipped — `interview.rs:1432-1471`. This is the emergent
  precursor of 1072's `SubmissionTxn`/`SkipDisposition`.
- Batch: every prompt/message the walk reaches before the first `block`
  (a node whose refs are unanswerable) — `Advance::block` — `interview.rs:1028`.
  Independent questions coalesce into one batch; **a confirm and the questions
  after it can be in the same batch**, so an action cannot assume it runs before
  its batch-siblings are answered. `Batch { items: Vec<Item>, errors: Rejections }`
  — `interview.rs:162`.

## Plan → apply → result, and the sole write/hook site

- `Plan::build(template, completed: &Completed, target: &Path) -> Result<Plan, PlanError>`
  — `plan.rs:179`. **Pure with respect to the target**: it reads template
  sources and the target (conflicts via `destination.exists()`, symlink checks)
  but writes nothing and runs no hook. `Plan { files, conflicts, hooks,
  before_apply, after_apply }` — `plan.rs:88`.
- `Plan::apply(self, target, ApplyOptions { force, trusted }, &dyn HookRunner) -> Result<Applied, ApplyError>`
  → `apply_reporting` — `apply.rs:56/66`. **This is the only place files are
  written (`fs::write`/`copy`/`create_dir_all`, `apply.rs:103-153`) and hooks
  run (`runner.run`, `apply.rs:154-169`).**
- `Applied { Written { files, hooks_run, after_apply }, NeedsTrust(Plan) }` —
  `apply.rs:18`. `NeedsTrust` returns the plan and writes/runs nothing — the
  existing precedent for a "return the plan, perform no effect" outcome, but it
  is trust-driven and lives *inside* apply.
- Hooks come from two sources (`completed.hooks` and `template.hooks`,
  `plan.rs:225-255`) and execute only in `apply_reporting`. "No hooks on
  stop/dry-run" therefore means **not entering the apply effect loop at all**;
  `Plan::build` is always safe.

## Drivers and result paths

One engine, five surfaces (AGENTS.md: "one interview engine for every driver"):

- **Terminal (interactive):** `cli/terminal.rs::drive -> Completed`
  (`terminal.rs:195`); `run` then `Plan::build` + `apply_reporting`
  (`main.rs:1031/1072`). `drive` returns a bare `Completed` with no channel for
  "user chose stop/dry-run mid-interview".
- **Headless (answers document):** `protocol::answer_headless -> Headless`
  (`protocol.rs:203`); `Headless { Completed { completed, accepted },
  Pending { pending, rejections, accepted } }` (`protocol.rs:191`) — expresses
  only complete-vs-more-questions, no stop/dry-run.
- **Staged:** `stage`/`continue`/`abort` (`main.rs:543/635/744`). `stage` and
  `continue` record submissions and never call `Plan`/`apply`; `abort` deletes
  the staged record (`Store::remove`) — the only existing "abort", CLI-level.
- **Direct (`apply TEMPLATE PATH`):** `main.rs:818` — start, drive, plan, apply,
  keep no staged record.
- **Crate / library:** no driver function; callers assemble `Interview::start` →
  drive/`answer_headless` → `Completed` → `Plan::build` → `Plan::apply`
  themselves (`lib.rs`, `tests/fixtures.rs`). The action must live on a type they
  observe, not in `main.rs` glue.

## Existing stop / dry-run / skip / abort meanings — collision map

Any confirm-driven vocabulary must be disambiguated from these:

- **`--dry-run`** is an `apply`-only CLI flag (`main.rs:162`), implemented only
  in `run` (`main.rs:1035-1059`): it builds the plan, prints `plan_lines`, and
  returns before `apply_reporting` — no writes, no hooks; and it does not persist
  staged answers (`main.rs:866/956`). Absent from the engine, `protocol.rs`,
  `staging.rs`, `plan.rs`. Headless `apply --dry-run` "emits the batch and leaves
  staged state unchanged" (`interview-protocol.yml:124`).
- **"stop"** today means only the headless-apply staging stop
  (`interview-protocol.yml:122`) and a hook-failure stop (`template-format.yml:109`).
- **"skip"** today means `when`-based question/group skipping
  (`template-format.yml:54-62`) with the fixed warning
  `answer for "<id>" was not used: the question was skipped`
  (`interview-protocol.yml:73`).
- **`abort`** is the CLI command that deletes staged state (`main.rs:744`).

## Protocol / schema contract surface and closure constraints

- Wire statuses are exactly two consts: `questions` and `complete`
  (`interview-protocol.schema.yml:50/82`); `batch` and `complete` are
  `additionalProperties: false` (`:44/76`). A confirm→stop that ends an
  interview early has **no representable result document today** — a new status
  value or a new declared field is required, and both must be added to the schema.
- `template-format.schema.yml` `$defs/question` is `additionalProperties: false`
  (`:101`) with a closed `type` enum `[text, multiline, confirm, select,
  multiselect]` (`:106`). A confirm `action` key literally cannot be authored
  until the schema is amended, and needs an analogous "action only on confirm"
  load-time rejection rule beside `loop`/`options` (`template-format.yml:119-130`).
- Config objects are `additionalProperties: false` (`config.schema.yml`).
- The design only **proposes** these canonical edits; the paired implementation
  (1062) owns the actual spec/schema/guide changes.

## Where a confirm-answer→action decision must be injected

1. **Template declaration** — extend `QuestionKind::Confirm` (`template.rs:114`)
   and its parser (`template.rs:685`) with an action mapping per truth-value,
   plus schema + load rule. Keep `default: Option<Typed<bool>>` and the boolean
   answer intact.
2. **Answer→decision** — the single funnel that sees both the question node and
   the final `Answer::Bool` is `check_answer` / the resolution loop in
   `Pending::answer` (`interview.rs:1390-1427`) and the reached-held accept branch
   (`interview.rs:1041`). `parse_kind` (`:511`) is the boolean choke point.
3. **State-machine surface** — `enum Interview` (`interview.rs:44`) needs a third
   outcome (e.g. `Stopped`) or a field on `Completed`, produced by `advance`
   (`:1241/1252`), the sole result constructor.
4. **Walk / groups** — evaluate the action inside the active-question branch of
   `walk` (`interview.rs:1031-1057`), *after* `when` (a skipped/inactive confirm
   must not fire) and *after* the held-answer check; a stop must short-circuit the
   remaining `for` loop and unwind nested group recursion.
5. **Early-answer / batch** — since a batch resolves before stepping and headless
   feeds many submissions, a stop surfaced mid-batch must define the fate of the
   other answers in the batch and of later held answers; reuse the existing
   `unless_skipped`/`dropped` provisional logic rather than a parallel walker.
6. **Downstream consumers** — every `Asking | Complete` match site
   (`main.rs:611/716/919/981`, `staging.rs:143`, `terminal.rs:211`,
   `protocol.rs:214`) must handle the new outcome. A confirm-driven "dry-run" most
   naturally sets a flag consumed at the apply boundary (`main.rs:1035`), but the
   *decision* still originates at insertion point 2.

## Preserve / Change / Avoid / Risk

**Preserve**

- Ordinary confirm boolean behavior end to end (`Answer::Bool`), opt-in per
  question; no action inferred from prompt text (AGENTS.md; inherited constraint).
- Pure interview engine; plan-before-apply; one engine for every driver.
- Replay determinism: only raw `submissions` + identity + frozen `now` persist
  (`staging.rs:20`); resume is full replay (`staging.rs:128`). An action must be
  recomputable from stored submissions, not stored as extra runtime control state.
- The five fixed 1072 policies and their four "do not" constraints.
- Atomic answer transaction: a rejected answer never enters probe/skip
  classification and a rejected document never persists (`interview.rs:1439-1478`).
- The single target normalizer `staging::canonical_target`; the skip/default
  readiness owners `default_ready`/`skipped_descendants_ready`.

**Change (proposed; implementation owns canonical edits)**

- Add a confirm action mapping to the template format + schema + load rule.
- Add an early-exit outcome to the interview result surface and a way to express
  stop / dry-run / skip-scope through every driver and the wire protocol
  (new status value or field on a closed-schema document).
- Extend `Applied`/driver outcomes so stop and dry-run are first-class rather
  than "the caller happens not to call apply".

**Avoid**

- A second target normalizer or `Path::canonicalize` at identity consumers.
- Reconstructing configured origins after the flat `Resolution` boundary.
- A parallel reachability/skip walker or a different skipped-default rule.
- Overloading `Seed.defaults` as a configured-only carrier.
- Depending on 1056 runtime (opaque `CanonicalTarget`, `SubmissionTxn`) being
  merged before confirm-flow — depend on the approved *contract*, not the code.

**Risk**

- Sequencing vs 1056: the design references approved-but-unmerged 1072 types and
  expresses its coupling as the atomicity invariant + the target-identity type, so
  it needs no 1056 code. Runtime integration order is not left open: per the
  effort's sequencing ruling, 1062's runtime integration lands after 1056.
  Independent design preparation is permitted; independent runtime integration is
  not. This is a coordination fact, not a Phase-C decision.
- Schema closure: adding a status/field to `additionalProperties: false`
  documents is a wire-contract change agents depend on; must be additive and
  documented.
- Skip-scope semantics (whole-interview vs current-group, nested) interact with
  the flat `Answers` map and the `warn_unused` warning contract; getting scope
  and skipped-value definitions wrong silently changes generated output.

## Evidence gaps

- No first-release UAT record or closed review exists for confirm-flow yet; this
  is greenfield feature design on integrated engine behavior.
- 1056 (error-attribution impl) not merged at `a62061f`; its runtime types are
  read from the approved design, not exercised.
- `why` was not run as a separate investigation: the change adds a new capability
  rather than redefining existing ownership, and the ownership seams
  (interview/plan/apply/staging/config) are documented above with code evidence.
