# Headless apply recovery

## Caller usage

### Resume by target

An incomplete staged interview is identified by its target. The caller does not need to repeat the template.

```console
$ toha apply --answers answers.json ./result
create summary.txt
create schedule.txt
$ echo $?
0
```

The command:

1. Loads the staged record for `./result`.
2. Resolves its recorded template and configured defaults.
3. Replays its committed submissions.
4. Submits `answers.json` through `protocol::answer_headless`.
5. Applies when the interview reaches `Completed(Proceed)`.

### Receive the next batch on a terminal

```console
$ toha apply --answers answers.json ./result
answers required: season, soil
season: must be one of: spring, summer, autumn, winter
next: update answers.json and run `toha apply --answers answers.json ./result`
$ echo $?
4
```

The summary replaces the protocol document only when standard output is a terminal.

A pipe retains the protocol JSON:

```console
$ toha apply --answers answers.json ./result > questions.json
the interview at ./result has questions remaining
to answer them: toha continue ./result in a terminal, or toha continue ./result '<ANSWERS>' with an answers document (- reads standard input)
then write its files: toha apply ./result
$ echo $?
4
```

`questions.json` contains the same pretty-printed protocol document produced by the existing `batch_document` path.

A caller using a pseudoterminal can request the machine representation explicitly:

```console
$ toha apply --json --answers answers.json ./result
{
  "protocol": 1,
  "status": "questions",
  ...
}
$ echo $?
4
```

`continue` accepts the same presentation override:

```console
toha continue --json ./result answers.json
```

`stage --async` is always a machine-output command and emits JSON even when its output is a terminal.

### Resume while asserting the template identity

A named apply remains valid when the name resolves to the staged formal identity.

```console
$ toha apply --answers answers.json ./sample ./result
create summary.txt
create schedule.txt
```

This command and the path-only form enter the same staged recovery function. Naming a different template fails before the answers file or standard input is read.

## Product decisions

1. `toha apply PATH --answers FILE` resumes an incomplete staged interview and applies when the supplied document completes it.
2. An exit-4 question result uses a concise terminal summary. Non-terminal output, `--json`, and `stage --async` use protocol JSON.
3. `toha apply TEMPLATE PATH --answers FILE` resumes when `TEMPLATE` resolves to the staged formal identity.

The target owns staged identity. The optional template operand is an assertion when a staged record exists; it does not select a second recovery path.

## Command behavior

| Invocation | No staged record | Incomplete staged record | Complete staged record | Different staged template |
|---|---|---|---|---|
| `apply PATH --answers FILE` | Exit 1; name the template-taking form. Do not read `FILE`. | Recover through the staged record. | Exit 1 because `FILE` is unused. Do not read it. | Not applicable; no template assertion was supplied. |
| `apply TEMPLATE PATH --answers FILE` | Start a new headless interview. | Resume when the resolved formal identity matches. | Exit 1 because `FILE` is unused. | Exit 1 with both template intents named. Do not read `FILE`. |
| `apply PATH` | Exit 1. | Prompt when both streams are terminals; otherwise return exit 4. Apply if replay is complete. | Apply. | Not applicable. |
| `continue PATH FILE` | Exit 1. | Submit one document and return the next protocol state. | Exit 1 because `FILE` is unused. | Not applicable. |

Resolution, stale-source checks, configured-default resolution, and replay all precede reading an answers document. This preserves error priority and prevents a wrong or unusable staged record from consuming standard input.

## Core data and result types

All recovery and presentation types are private to the CLI binary.

```rust
// src/cli/recovery.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageOrigin {
    /// The apply command has prepared a record, but no record existed on entry.
    New,
    /// The target had a staged record on entry.
    Existing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MutationMode {
    /// Commit accepted submissions and committed abort effects.
    Commit,
    /// Evaluate the complete command without changing staged state.
    Preview,
}

pub(crate) struct StageContext<'s> {
    pub origin: StageOrigin,
    pub target: &'s CanonicalTarget,
    pub store: &'s Store,
    pub record: &'s mut StagedRecord,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Recovery<'a> {
    Pending {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
    },
    Completed {
        completed: Completed,
        cleanup: PostApply,
    },
    Ended(Ended),
}

/// Cleanup permitted only after Plan::apply reports success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PostApply {
    None,
    RemoveStaged,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecoveryError {
    #[error("{0}")]
    Evaluation(#[from] EvalError),
    #[error("{0}")]
    Staging(#[from] StagingError),
}
```

`StageOrigin` distinguishes real storage behavior:

- A new headless interview is saved only when it remains pending.
- An existing interview preserves committed progress on pending, completion, and stop.
- An existing abort removes its record.
- Only an existing successful apply needs post-apply cleanup.

`MutationMode::Preview` represents the CLI `--dry-run` flag. It suppresses every `Store::save` and `Store::remove`, including removal requested by `Ended(Abort)`. It does not suppress parsing, replay, answer validation, flow evaluation, or plan construction.

A template-originated `Completed(DryRun)` is a committed interview outcome rather than a CLI preview. Its accepted submissions persist, its plan is built, and neither target effects nor staged cleanup run.

```rust
// src/cli/presentation.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuestionFormat {
    /// Human summary on a terminal; JSON otherwise.
    Automatic,
    /// Protocol JSON on every output device.
    Json,
}

pub(crate) struct QuestionOutput {
    pub document: serde_json::Value,
    pub summary: QuestionSummary,
    pub machine_guidance: Option<String>,
    pub format: QuestionFormat,
}

pub(crate) struct QuestionSummary {
    pub messages: Vec<String>,
    pub ids: Vec<String>,
    pub errors: Vec<(String, Vec<String>)>,
    pub next: String,
}
```

`QuestionSummary` is derived from the same `Batch` and effective rejections passed to `protocol::batch_document`. It contains no copied prompt schema and no interview policy.

`Outcome` gains a specific exit-4 variant:

```rust
enum Outcome {
    Written(Vec<String>),
    Error(String),
    Document(serde_json::Value, u8),
    Questions(QuestionOutput),
    Saved(u8),
    NeedsTrust(String),
    Ambiguous {
        name: String,
        matches: Vec<String>,
        retry: Vec<String>,
    },
}
```

`Document` continues to carry complete and ended protocol documents where required. `Questions` owns the exit-4 presentation decision.

## Function signatures

```rust
// src/cli/recovery.rs

/// Submit one answers document using the protocol's normal headless walk.
///
/// The function owns accepted-submission persistence and committed abort
/// removal. It never plans or applies target effects.
pub(crate) fn answer_apply<'a>(
    template: &'a Template,
    interview: Interview<'a>,
    document: RawAnswers,
    stage: StageContext<'_>,
    mode: MutationMode,
) -> Result<Recovery<'a>, RecoveryError> {
    unimplemented!()
}
```

The implementation delegates the interview transaction to the existing function:

```rust
let result = protocol::answer_headless(template, interview, document)?;
```

It does not call `Pending::answer` independently, parse another document, reconstruct defaults, or walk the interview.

```rust
// src/cli/presentation.rs

pub(crate) fn questions(
    pending: &Pending<'_>,
    context: &Context,
    rejections: Option<&Rejections>,
    next: String,
    machine_guidance: Option<String>,
    format: QuestionFormat,
) -> QuestionOutput {
    unimplemented!()
}

pub(crate) fn finish_questions(
    output: QuestionOutput,
    stdout_is_terminal: bool,
) -> std::process::ExitCode {
    unimplemented!()
}
```

`questions` calls `protocol::batch_document` exactly once. The summary reads IDs and messages from `Batch::items` and displays effective rejection messages in batch order.

```rust
// src/main.rs

fn run_staged_apply(
    target: &CanonicalTarget,
    store: &Store,
    saved: StagedRecord,
    template: Template,
    interview: Interview<'_>,
    answers: Option<String>,
    options: ApplyRequest,
) -> Outcome {
    unimplemented!()
}
```

Both accepted apply spellings call `run_staged_apply`. Template resolution and replay occur before this function. Fresh apply retains its distinct template-resolution entry but uses `answer_apply` with `StageOrigin::New` for headless answers.

```rust
struct ApplyRequest<'a> {
    invocation: Invocation<'a>,
    force: bool,
    cli_dry_run: bool,
    trust: bool,
    question_format: QuestionFormat,
}
```

No new public library interface is introduced.

## Recovery transaction

`answer_apply` performs one policy-owned transition:

1. Call `protocol::answer_headless`.
2. Copy the existing record and append only the returned `accepted` submissions.
3. Classify the engine result as `Pending`, `Completed`, or `Ended`.
4. Apply the following staged effect when `MutationMode::Commit`:
   - `New + Pending`: atomically save the prepared record.
   - `Existing + Pending`: atomically save the prepared record.
   - `Existing + Completed`: atomically save the prepared record before planning.
   - `Existing + Ended(Stop)`: atomically save the prepared record.
   - `Existing + Ended(Abort)`: remove the staged record without first saving the aborting submission.
   - Every other combination: no staged effect.
5. Replace the caller’s in-memory record only after a successful save.
6. Return the classified result.

A rejected answers document is absent from `accepted`. If an earlier submission in the same headless run commits before a later empty submission rejects, only the committed prefix is saved.

An abort removal failure returns `RecoveryError::Staging`; the old staged record remains available. A rejected document cannot produce a committed `Ended(Abort)`, so it cannot reach removal.

## Result execution

| Recovery result | Staged effect in normal execution | Plan | Target effects | Exit |
|---|---|---|---|---|
| `Pending` | Save committed prefix. | None | None | 4 |
| `Completed(Proceed)` | Save committed prefix for an existing record. | Build | Apply | 0 on success |
| `Completed(DryRun)` | Save committed prefix for an existing record. | Build and show | None | 0 |
| `Ended(Stop)` | Save committed prefix for an existing record. | None | None | 0 |
| `Ended(Abort)` | Remove an existing record. | None | None | 0 |
| Any result with CLI `--dry-run` | None | Build only for a completed result | None | Result-specific |

For `Completed(Proceed)`, `PostApply::RemoveStaged` is consumed only after `Plan::apply_reporting` returns `Applied::Written`. The record remains when planning, trust, conflict detection, a hook, a file write, or another apply operation fails.

A removal failure after successful apply is surfaced. The target effects have completed and the staged record remains, matching the existing effect order.

`Ended(Stop)` and `Ended(Abort)` never enter `Plan::build`. Skip stays inside the engine and can lead to any later recovery result. A boolean confirm has no special handling in recovery.

## Question presentation

| Producer | Standard output | Presentation |
|---|---|---|
| `stage --async` | Any | Protocol JSON |
| `apply` or `continue`, exit 4 | Non-terminal | Protocol JSON |
| `apply` or `continue`, exit 4 | Terminal | Human summary |
| `apply --json` or `continue --json`, exit 4 | Any | Protocol JSON |

The non-terminal JSON is produced by the existing `protocol::batch_document` and retains its schema, messages, errors, context, formatting, and exit code.

The terminal form is:

```text
<reached messages, when present>
answers required: <ids in batch order>
<id>: <error, for each effective error>
next: <source-aware command>
```

The next command preserves the parsed apply flags and operands:

- A named staged apply keeps the named template assertion.
- A path-only apply stays path-only.
- `--force`, `--trust`, `--dry-run`, and `--json` remain present when applicable.
- A named answers file is named in the update instruction.
- Standard input uses “provide a corrected JSON object on standard input”.
- A request without an answers file names `<ANSWERS>`.

Machine guidance remains on standard error when JSON is selected. The terminal summary includes the next action itself and suppresses the longer duplicate guidance.

## Module and seam map

```text
parsed apply operands and flags
            │
            ▼
main.rs: classify apply route
   │
   ├── no record + template ───────────────► fresh resolution/start
   │
   ├── record + no template ───────────────► staged resolution/replay
   │
   └── record + template
          │ formal identity assertion
          └────────────────────────────────► staged resolution/replay
                                                    │
answers file ─► existing read_answers ──────────────┤
                                                    ▼
                                      cli::recovery::answer_apply
                                                    │
                                      protocol::answer_headless
                                                    │
                         accepted submissions + engine result
                                                    │
                    ┌───────────────┬───────────────┴──────────────┐
                    ▼               ▼                              ▼
                 Pending         Completed                       Ended
                    │               │                              │
        save committed prefix   save if staged          Stop: save if staged
                    │               │                   Abort: remove if staged
                    ▼               ▼                              ▼
       cli::presentation       Plan::build                 ended presentation
      terminal text / JSON          │
                                    ├── dry-run: show
                                    └── proceed: Plan::apply_reporting
                                                     │ success
                                                     ▼
                                           remove staged record

requested path ─► staging::canonical_target ─► CanonicalTarget
                                                ├── Store
                                                ├── protocol Context
                                                ├── Plan::build
                                                └── Plan::apply_reporting

live configuration ─► configured_defaults ─► Resolution
                                               │ consumed once
                                               ▼
                                  start / replay_with_resolution
```

`cli::recovery` is a deep module: one operation hides protocol looping, accepted-prefix handling, staged persistence, CLI preview semantics, flow outcomes, and abort cleanup. Callers receive only the next command-level state.

`cli::presentation` is a real seam because question output has two adapters: protocol JSON and terminal text. It owns no answer, staging, or flow policy.

`main.rs` owns command routing, template identity assertions, planning, trust, and post-apply cleanup. `staging`, `protocol`, `interview`, and `plan` retain their existing ownership.

## Persistence and compatibility

The staged record format does not change. It continues to contain target, template identity, immutable source commit, frozen interview time, and accepted submissions. Question batches, rejections, flow outcomes, plans, and presentation choices are derived and are not persisted.

Compatibility guarantees:

- Existing staged JSON remains readable.
- `CanonicalTarget` is constructed once through `canonical_target`.
- The same target enters storage, protocol context, planning, apply, and removal.
- `Resolution` remains the only configured value-and-origin carrier.
- `read_answers` and `protocol::parse_answers` remain the sole answers parse path.
- Non-terminal exit-4 JSON remains byte-for-byte compatible for existing named-apply and continue paths.
- `stage --async` remains JSON on a terminal.
- Exit code 4 continues to mean that answers are required.
- A same-template named apply retains its behavior.
- Path-only apply with answers becomes an additional supported route.
- Wrong-template, stale-source, replay, parse, evaluation, trust, conflict, and staging I/O failures remain distinguishable.
- No permission, access, timeout, pinned-version, subprocess, trust, or hook policy changes.

## Error behavior

- No staged record for path-only answers: fail before reading the document and name `apply TEMPLATE PATH --answers FILE`.
- Different named template: fail before reading the document and name both intents.
- Complete staged interview with answers: fail before reading the document because it is unused.
- Stale source or failed replay: preserve the record and do not read the document.
- Malformed or unreadable answers: preserve the record.
- Rejected document: return exit 4; persist no part of that rejected transaction.
- Staging save failure: return an error before planning.
- Planning, trust, conflict, hook, or target-write failure: preserve the completed staged record.
- Successful apply: remove the record after all target effects succeed.
- Stop: preserve the record and committed submissions.
- Abort: remove the record; surface removal failure.
- CLI dry-run: perform no staged save or removal.
- Flow dry-run: persist committed submissions, build the plan, perform no target effects, and retain the complete staged record.

## Falsifiable tests

1. An incomplete staged interview completed by `apply PATH --answers FILE` produces the same files, hooks, messages, exit code, and final record state as `apply TEMPLATE PATH --answers FILE`.
2. Both spellings return the same next batch and effective errors when the document remains incomplete.
3. Both spellings produce the same evaluation, planning, trust, conflict, hook, and apply failures.
4. A path-only answers command without a staged record does not open the answers file or read standard input.
5. A different named template does not open the answers file or read standard input.
6. A complete staged interview does not consume a supplied answers document.
7. A rejected document leaves the stored submission sequence byte-identical.
8. A committed document followed by a rejected default-only step persists only the committed prefix.
9. A CLI dry-run leaves the staged file byte-identical for `Pending`, `Completed`, `Stop`, and `Abort`.
10. A flow dry-run saves committed submissions, produces a plan, writes no target file, runs no hook, and leaves the staged record.
11. Stop saves the triggering submission and replay returns the same `Ended(Stop)`.
12. Abort removes only the current canonical target’s record. A forced removal failure is reported.
13. A plan failure, trust refusal, conflict, hook failure, or file failure leaves the staged record. Successful apply removes it.
14. Deleting the post-success removal call leaves the successful-apply test failing because the staged file remains.
15. Moving removal before apply leaves the injected apply-failure test failing because the staged file disappears.
16. Non-terminal exit-4 output parses as the existing protocol schema and is byte-equal to the existing named-apply output.
17. Terminal exit-4 output is not JSON, lists every prompt ID in batch order, retains reached messages and effective errors, and names one executable next command.
18. `--json` forces JSON through a pseudoterminal.
19. `stage --async` remains JSON through a pseudoterminal.
20. Exit-4 terminal selection depends on standard output, not standard input.
21. Target spellings involving relative paths, parent segments, symlink ancestors, and roots use the same `CanonicalTarget` for load, save, protocol context, plan, apply, abort, and cleanup.
22. Configured-default warnings and origins are identical between both staged apply spellings.
23. A document that tentatively triggers abort and also contains a surviving rejection does not remove or change the staged record.
24. Skip, `Completed(Proceed)`, `Completed(DryRun)`, `Ended(Stop)`, and `Ended(Abort)` are covered through path-only, same-template, and replayed routes.

## Canonical document changes

The paired implementation updates:

- The command-line specification with path-only staged recovery, the template assertion rule, terminal question summaries, and `--json`.
- The interview protocol guide with the guarantee that non-terminal and explicit machine output retain the question document.
- The staging and architecture documentation with `cli::recovery` ownership and post-apply cleanup ordering.
- User guidance for `apply PATH --answers FILE`, same-template recovery, terminal summaries, and machine-output selection.

The staged-record schema and protocol question schema do not change.

## Rationale

### Problem

The target already provides an unambiguous staged identity, but one apply spelling refuses to use it. The accepted engine and staging contracts also require careful ordering: answers commit transactionally, accepted prefixes can persist, CLI previews cannot mutate state, abort removes state only after a committed outcome, and successful apply removes state only after target effects succeed. Output has a second concern: humans need a short next action while machine callers require stable JSON.

### Shape

The design normalizes both apply spellings before recovery and puts the complete answer-to-staged-state transition behind `cli::recovery::answer_apply`, per boundary discipline and interface depth. It uses the existing engine transaction, configured resolution, target identity, parser, and serializers. The output choice lives in a separate presentation module because terminal text and protocol JSON are two real adapters.

The interface exposes command-level states rather than protocol bookkeeping. Accepted submissions, empty headless submissions, persistence timing, abort cleanup, and preview behavior remain hidden. The caller retains planning because recovery cannot remove the staged record until the independent apply effect reports success.

### Synthesis decision

This candidate chooses target-owned recovery with a CLI-private policy module and a separate presentation seam. Path-only and same-template apply converge before answer submission. Terminal output changes only at the final question renderer; engine and protocol semantics remain common.

### Tradeoffs accepted

- We accept an additive `--json` flag on `apply` and `continue` in exchange for deterministic machine output through pseudoterminals.
- We accept saving a completed staged record before planning in exchange for preserving accepted work across every later failure.
- We accept retaining a complete record after a template-originated dry-run in exchange for treating that flow decision as committed interview state.
- We accept an error after target effects when final staged cleanup fails in exchange for never deleting recovery state before apply succeeds.
- We accept one CLI-private summary projection of a batch in exchange for keeping terminal presentation out of the public protocol interface.

### Alternatives considered

- Keep path-only refusal and require `continue` followed by `apply`. This exposes sequencing and staged-template knowledge to every caller while hiding almost no complexity.
- Add path-only support directly to the existing `main.rs` match arms. This keeps implementation movement small but duplicates persistence and future flow handling across fresh, named, and path-only routes.
- Implement path-only apply by internally invoking `continue` and then `apply`. This creates a temporal decomposition with an intermediate command failure point and violates the application subprocess constraint.
- Always print JSON and keep next-command guidance on standard error. This preserves one renderer but leaves terminal callers responsible for interpreting a protocol document.
- Select terminal text solely through terminal detection. This is smaller, but scripts and agents using pseudoterminals lose a deterministic machine-output request.

### Open questions and risks

No product decision remains open.

Implementation verification must answer:

- Does every early return occur before reading standard input when the staged identity is unusable?
- Does every flow outcome pass through the same mutation gate?
- Does terminal rendering preserve every reached message and effective rejection without changing protocol JSON?
- Does cleanup remain strictly after successful apply when hooks or file effects fail partway through execution?

### Next implementation step

Create `cli::recovery::answer_apply` and route the existing named staged-answer path through it before enabling the path-only route.