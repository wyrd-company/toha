---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Error attribution and early-answer behavior — grounding

Phase A traces the current caller-to-result flow for the six reviewed items in
design 1072. Evidence is from epic head
`6ae8676441b8a372655b489d7d541614406d0a13`, the first-release UAT record, the
closed local review `01M3FY1Y9X5AXA4XTADGJW50Z0`, and the approved behavior of
configured-defaults design 1073.

## Phase position

- Ground: complete at the evidence level recorded here.
- Sketch: candidate arena follows this artifact.
- Agree: Bob approves the exact synthesized revision and its product decisions.
- Implement and Scrap belong to the paired implementation after approval.

## The six observed items

### File-rule and target-path faults lose template-field identity

`Plan::build` creates the completed-answer Jinja context, walks ordinary source
files, then evaluates each explicit `files[i]` rule (`src/plan.rs:178-224`). Its
top-level hook path already emits the established form:

```text
template error in hooks[0].when `'bad' | dateformat`: <message>
```

Three file paths do not:

1. `files[i].when` wraps the evaluator error as `files[i].when: <message>`.
2. `files[i].each` wraps it as `files[i].each: <message>`.
3. An explicit `files[i].path` render passes only the evaluator message.
   Ordinary source-tree path segments go through the same unlabelled `rendered`
   helper (`src/plan.rs:146-176`, `320-386`).

The error retains the source file through `PlanError::Render { path, message }`,
so the caller sees `<source path>: ...`; the missing information is the template
field and expression. Direct observations from the built epic binary were:

```text
.../part.txt: files[0].when: invalid operation: ...
.../part.txt: files[0].each: expected array
.../part.txt: invalid operation: ...
```

`HookFault` and `EvalError` each independently carry field/source/message
triples (`src/interview.rs:106-158`, `217-248`). Plan rendering carries only a
string. The design must settle whether one shared template-fault value crosses
both modules or whether planning keeps a private equivalent while preserving one
diagnostic contract.

### A template default can be reported as caller input

`render_default` evaluates a question's configured override or template default
and returns only `Option<Answer>` (`src/interview.rs:385-420`). `make_prompt`
places that value in `Prompt.default` after it has evaluated the prompt,
constraints, and options (`422-502`). It does not check the rendered template
default against those constraints.

When an answers document omits a current question, `Pending::answer` feeds the
default back through `check_inner` (`src/interview.rs:1390-1418`). A configured
default gets a special attributed rejection. A template default follows the same
branch as caller input, so a default that violates its own `options`,
`required`, length, regex, or list constraint becomes a normal answer rejection.
The terminal driver similarly validates a returned value through
`Pending::check` and can reprompt without identifying the template as faulty
(`src/cli/terminal.rs:203-263`).

The reviewed requirement classifies this as a template fault. Validation must
therefore occur where the engine can still distinguish a template-authored
default from caller input. A literal default and an expression default both need
an actionable field identity; expression defaults additionally retain their
source text.

### Configured-default provenance is lost before constraint validation

Today `configured_defaults` validates only answer kind and returns
`IndexMap<Id, RawAnswer>` (`src/interview.rs:677-690`). `Seed.defaults` carries
the same flat map (`src/interview.rs:36-40`). `render_default` can tell that an
id has a configured override, but it can reconstruct only the old hard-coded key
`defaults.<id>`. Later constraint validation uses that hard-coded key in the
recoverable batch rejection (`src/interview.rs:1399-1414`).

The approved behavior from configured-defaults design 1073 is more precise:

```text
<file>: template-defaults."<formal-name>".<id>: <message>
<file>: template-defaults."<formal-name>".<id>
  → presets."<preset-name>" (<value>): <message>
```

Its approved selector is exact `formal_name`; reusable values are `presets`,
references are `{ preset: <name> }`, and mappings are `template-defaults`. The
approved design revision is:

- branch head: `40ae3fc01122bc898cc49367c9f799828a6e4ca8`
- subtree tree object: `b5f8c1ab0a8649e7069a40e6ee6f2355221ae08f`
- `design.md` SHA-256:
  `fd2d41e1a3acab54485ef10e8a73d49c6766e3ab571cb2a1a9e131972dc450b6`

That design requires kind and missing-preset faults to name the mapping's config
file and the preset hop. A configured default that passes kind checking can
still fail a literal or dynamically rendered question constraint later, when the
terminal or headless driver may recover by supplying a different value. The full
origin must therefore survive until `Pending::answer` without teaching the pure
interview engine about config layers, presets, or template selection.

Design 1073 entered review with `Seed.defaults` explicitly unchanged and an
optional `EvalError.config_key`. Its review has since requested repair of
internal origin carriage, caller coverage, and verification while leaving the
approved product behavior unchanged. Design 1072 must consume that repaired
origin contract before synthesis and must not create a competing provenance
carrier.

### Early answers are held before their question is reached

`Pending::answer` receives one answers document. For each id it distinguishes an
unknown question, an already skipped question, an already answered question, a
current-batch question, and an early question (`src/interview.rs:1334-1387`).

For an early answer, `Rules::of_question` exposes every constraint known without
prior answers: answer kind, literal `required`, literal options, regex, and
literal bounds (`src/interview.rs:565-587`). `validate` checks those constraints
immediately. A failed value and its tentative rejection are still placed in the
held set so that one tentative `advance` can decide whether the question is
active, skipped, or still unreached.

After current-batch questions are checked, the code classifies early failures:

- if the current batch failed, it returns before the tentative step and every
  early failure stands;
- otherwise it tentatively advances with valid current answers;
- an early failure is dropped when the tentative result skipped its question;
- it stands when the next result is asking and has not skipped that question;
- a reached template fault replaces the early error only when no rejection
  remains (`src/interview.rs:1428-1478`).

The first-release review established the reason for the tentative step: an
invalid answer for a question skipped by the same document is unused and becomes
one warning, while the same answer for an active question is an error. The
follow-up matrix tests skipped, active, and unreached questions through both
direct headless apply and staged continue
(`tests/interview_answers.rs:1233-1315`).

Two product choices remain:

1. When the tentative step cannot yet decide whether the question will be
   skipped, should the known literal-constraint failure reject this document
   now, or should the raw value remain held until the question becomes active or
   skipped?
2. When a current-batch value fails but the same document supplies enough other
   valid answers to prove that an invalid early answer's question is skipped,
   should the result omit that irrelevant early error?

The second case is the exact deferred review finding from local review thread
`01M3G2RNGYXYKD76TANMY8FDHX`: the document is rejected for its current-batch
failure either way; the extra skipped-question error is noise and becomes a
warning only after a corrected document is accepted.

### Existing target directories gain a trailing separator

All command paths enter `setup`, which calls `staging::canonical_target(path)`
before loading or saving staged state (`src/main.rs:427-429`). The function
makes the path absolute, removes `.` and lexically resolves `..`, finds the
closest existing ancestor, canonicalizes that ancestor, and joins the
non-existing suffix (`src/staging.rs:40-68`).

For an existing target the suffix is empty. `PathBuf::join("")` preserves a
trailing separator. Staging an existing temporary directory at the epic head
produced this protocol value:

```text
'/tmp/toha-grounding-MgXbHn/target/'
```

The protocol promises a canonical absolute target path
(`docs/specifications/interview-protocol.yml:87-89`). `record` persists this
path, the state-file digest hashes it, and `context(record)` serializes it
(`src/main.rs:431-473`, `src/staging.rs:78-92`). For a non-existing target the
suffix is non-empty and no separator is added. The same logical directory can
therefore have two display shapes based only on whether it existed during
normalization.

The proposed shared interface is the existing public
`staging::canonical_target(&Path) -> Result<PathBuf, StagingError>`, with the
empty-suffix case returning the canonical ancestor directly. Design 1075 has
been notified to consume the returned path for Jinja context and not add a
second normalizer. Arena candidates may strengthen the carrier if they can do so
without duplicating normalization or widening the public surface.

## Caller-to-result flow

```text
CLI stage / continue / apply
  │
  ├─ setup(path)
  │    └─ canonical_target → staged-state key + protocol Context.target
  │
  ├─ resolve/load template + resolve configured defaults
  │    └─ Seed { now, defaults } → Interview::start / staged replay
  │
  ├─ Pending::answer(document)
  │    ├─ validate current answers
  │    ├─ validate known parts of early answers
  │    ├─ tentative Advance::walk classifies active/skipped/unreached
  │    └─ AnswerError::Rejected or EvalError, or accepted next state
  │
  └─ completed interview → Plan::build
       ├─ render ordinary source paths/content
       ├─ render files[i].when / each / path / content
       ├─ render hooks and messages
       └─ PlanError before trust decision or writes
```

`StagedRecord` persists target, template identity, commit, frozen `now`, and the
accepted submissions. It does not persist derived interview state. Resume
reloads the template and live configured defaults, starts the interview again,
and replays each accepted submission (`src/staging.rs:127-164`). Any change to
early-answer or default classification must therefore produce the same result
during original execution and replay.

## Ownership and seams

- `src/interview.rs` owns pure interview progression, answer/default validation,
  skip classification, and interview template faults.
- `src/plan.rs` owns file-rule and target-path rendering after interview
  completion.
- `src/staging.rs` owns canonical target normalization and replay mechanics.
- `src/main.rs` owns command composition, live config resolution, staged
  records, and the adapters from engine results to exits/documents.
- `src/protocol.rs` owns machine-readable batches/results and the headless loop.
- `src/cli/terminal.rs` owns interactive prompting and prints recoverable
  rejections before asking again.

The established fault interface is
`template error in <field> `<expression>`: <message>`. The source file prefix
belongs to `PlanError`; question/node identity belongs inside interview fields
such as `title.default`; config provenance belongs to the selected mapping
origin supplied by design 1073.

Trust is downstream of planning. `Plan::build` runs before dry-run/trust
handling and hook execution (`src/main.rs:1017-1079`). These changes do not
grant access, run a subprocess, add a timeout, or add a pinned-version check.

## Why the current shape exists

### Direct evidence

- First-release UAT required early answers to be validated on submission and
  again at reach, while held answers remain one document and direct apply equals
  continue (`kanban 1036`, 2026-09-26 20:16 and 21:15; PR 2 summary).
- Local review `01M3FY1Y9X5AXA4XTADGJW50Z0` found that only a tentative step can
  distinguish an active invalid early answer from an unused skipped one. The
  remediation introduced the current one-pass classifier and 48-case equivalence
  matrix.
- The same review deferred the remaining current-batch-noise case into this
  work.
- Commit `f9399eaa13b5fdd265d9033183fe0523a29cf074` introduced the old
  configured default key attribution. Its PR description states that the config
  layer could not be named because `Config.defaults` carried no origin.
- Commit `97f755480e0eaa1a6e68b345049f04b96bc55f64` established the
  field-plus-expression form for hook faults and unresolved dependencies.
- Commit `b52b20348e6214b7c4133e67309ab50779598bb0` introduced target
  canonicalization for cross-platform stable staged identity; the protocol
  hashes that canonical path.

### Inference

- The early-answer classifier appears optimized for atomic document acceptance:
  no answer from a rejected document is recorded. That invariant is stated in
  the protocol and enforced by returning the original `Pending` on rejection.
- The hard-coded configured-default key appears to be a deliberate stopgap
  rather than a desired abstraction: it fixed attribution before layered origin
  data existed, and design 1073 now supplies richer origins.
- The repeated field/source/message structures suggest a shared fault value
  could improve locality, but source history does not record an explicit intent
  to share them. Candidates must justify that seam on present behavior, not
  inferred history.

### Sources searched and gaps

- Source control: git history, PR 2, the complete local gitpr record for the
  first-release answer follow-up, current tests, and current source were
  searched.
- Board and long-form documents: tasks 1036, 1056, 1065, 1072, 1073, 1075 and
  the current specifications/design packages were searched.
- GitHub issues: repository issues matching error attribution, early answers,
  defaults, and target separators returned no additional rationale.
- Real-time chat: no matching searchable source is available; conversational
  rationale outside the task/review records is a gap.
- Infrastructure observability and product analytics are inapplicable: these are
  deterministic local library/CLI paths with no deployed telemetry boundary.
- Error tracking is inapplicable: the evidence is local UAT/review and fixture
  output, not production exceptions.

## Preserve / Change / Avoid / Risk

### Preserve

- One pure interview engine for terminal, headless, staged, direct, and crate
  paths.
- Atomic answers documents: a rejected document records no answers or step.
- Accepted submissions as the durable source; replay derives state and
  diagnostics.
- Apply/continue equivalence for the same answers and staged history.
- Skipped answers are unused and warn once in interview order after an accepted
  step.
- Configured defaults remain recoverable answer defaults after kind resolution;
  the terminal can replace a constraint-invalid configured value.
- Exact configured-default behavior and naming approved in design 1073.
- Canonical target identity remains absolute, symlink-resolved through the
  closest existing ancestor, and stable across staging operations.

### Change

- Give file-rule and target-path render faults field and expression identity.
- Treat a constraint-invalid template-authored default as a template fault.
- Carry selected configured-default origin through later constraint validation.
- Decide deferred early validation and current-batch skipped-error noise
  explicitly.
- Make existing and non-existing canonical targets use the same separator-free
  representation.

### Avoid

- A second target normalizer in Jinja context.
- Config/preset knowledge in interview progression.
- Diagnostics rebuilt from question id when exact origin is available.
- A rejected document that mutates staged submissions or emits an accepted-step
  warning.
- Different classifications between direct apply, staged continue, replay,
  crate, and terminal paths.
- Runtime stubs, shared canonical spec edits, or implementation in this design
  task.

### Risks

- A sidecar provenance map can drift from defaults unless construction makes
  their agreement unrepresentable or validates it once.
- Eager template-default validation must not evaluate constraints whose
  references are not ready; literal and dynamic constraints need a clear reach
  point.
- A tentative step used only for classification must not leak messages,
  warnings, answers, hooks, or template faults when the document is rejected.
- Changing early validation can alter which submissions old staged records
  replay; equivalence and replay fixtures must state the intended compatibility.
- A canonical target wrapper can become a shallow pass-through if it adds
  interface without preventing duplicate normalization or invalid construction.
