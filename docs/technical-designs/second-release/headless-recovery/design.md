---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Recovery from incomplete headless answers

## Purpose

An incomplete staged interview is recoverable from the target alone. Both accepted `apply` spellings use one answer transaction and one staged-state policy. Question batches remain JSON for non-terminal consumers and become short instructions when stdout is a terminal. The pure interview engine, wire schema, staged-record schema, target identity, and configured-default provenance remain authoritative.

## Caller usage

### Resume by target

```console
$ toha apply --answers answers.json ./output
create README.txt
create settings.toml
$ echo $?
0
```

The target identifies its staged record. The command resolves the record's stored template identity and immutable source, replays committed submissions with the origin-bearing configured `Resolution`, submits the answers document through `protocol::answer_headless`, and applies only after `Completed(Proceed)`.

Without a staged record, the same command fails before it reads the answers file:

```text
no interview is staged at ./output
start and apply one: toha apply --answers answers.json <TEMPLATE> ./output
```

### Assert the template identity

```console
$ toha apply --answers answers.json ./fixture ./output
create README.txt
create settings.toml
```

When `./fixture` resolves to the staged formal identity, this command enters the same recovery path as the target-only form. The template operand is an identity assertion. Replay still loads the stored formal identity and commit. A different identity fails before the answers document is read.

### Read the next questions

When the document leaves questions unanswered and stdout is a terminal:

```text
$ toha apply --answers answers.json ./output
answers needed:
  label — Display label
  quantity — Item count
next: update answers.json, then run:
  toha apply --answers answers.json ./output
$ echo $?
4
```

A rejected answer is shown with its question:

```text
answers needed:
  quantity — must be at least 1
next: update answers.json, then run:
  toha apply --answers answers.json ./output
```

When stdout is redirected or piped, the same result is the existing JSON question document:

```console
$ toha apply --answers answers.json ./output > questions.json
$ status=$?
$ jq -r .status questions.json
questions
$ echo "$status"
4
```

`stage --async` remains an explicit protocol-producing command. It emits or writes JSON even when stdout is a terminal.

## Product behavior

### Command selection

| Invocation | No staged record | Incomplete staged record | Complete staged record | Different named template staged |
|---|---|---|---|---|
| `apply PATH --answers FILE` | Exit 1; name the template-taking form; do not read `FILE`. | Replay, answer, and apply on completion; otherwise save accepted progress and exit 4. | Exit 1 because `FILE` is unused; do not read it. | N/A; no template assertion was supplied. |
| `apply TEMPLATE PATH --answers FILE` | Start a new headless interview. | Resume through the same path when formal identity matches. | Exit 1 because `FILE` is unused; do not read it. | Exit 1 with both intents named; do not read `FILE`. |
| `apply PATH` | Exit 1. | Prompt when stdin and stdout are terminals; otherwise produce the next question result. | Apply. | N/A. |
| `continue PATH FILE` | Exit 1. | Submit one document, commit accepted progress, and return the next protocol result. | Exit 1 because `FILE` is unused. | N/A. |

Target setup, stored-source resolution, configured-default resolution, and replay precede reading an answers document. A wrong identity, stale source, or broken staged record cannot consume stdin.

### Terminal question presentation

`Outcome::Questions` is the only exit-4 presentation seam. It carries one canonical protocol document and one human summary derived from the same `Batch` and effective rejections.

| Producer | stdout | Representation |
|---|---|---|
| `stage --async` | Terminal or non-terminal | Existing JSON. |
| `apply` or `continue`, question result | Terminal | Concise summary and next command. |
| `apply` or `continue`, question result | Non-terminal | Existing pretty JSON bytes; existing machine guidance remains on stderr. |

The terminal summary includes reached messages, question ids and titles in batch order, effective rejection text, and one command built from parsed values. It does not parse its own JSON document. Terminal mode suppresses duplicate long guidance on stderr.

No new output-format option is part of this design. A program that needs JSON uses non-terminal stdout, as stated by the product decision.

### Recovery outcomes

Recovery handles engine results, not confirm actions. A `confirm` remains an ordinary boolean answer. A later `flow` node may use that value in its `when` expression and produce one of the accepted outcomes.

| Engine result | Staged effect under normal execution | Plan | Files/hooks | Exit |
|---|---|---|---|---|
| `Pending` | Save the accepted prefix. | None. | None. | 4. |
| `Completed(Proceed)` | Save accepted progress for an existing record. | Build. | Apply. | 0 on success. |
| `Completed(DryRun)` | Save accepted progress for an existing record. | Build and show. | None. | 0. |
| `Ended(Stop)` | Save accepted progress for an existing record. | None. | None. | 0. |
| `Ended(Abort)` | Remove the existing record through `Store::remove(&CanonicalTarget)`. | None. | None. | 0, or nonzero on removal failure. |

Skip stays inside the interview walk. It can lead to pending, completed, or ended results; recovery adds no skip branch and reconstructs no skipped value.

The caller's `--dry-run` suppresses plan application and every staged mutation, including removal requested by `Ended(Abort)`. This follows the existing command contract that dry-run prints what apply would produce and changes nothing. A normal run still honors the accepted abort contract and removes the staged record. The flow action remains adopted; dry-run simulates its result without applying its effect.

## Data structures

All new types are private to the CLI binary.

```rust
// src/cli/recovery.rs

type Submission = IndexMap<String, serde_json::Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordOrigin {
    New,
    Existing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutationMode {
    Commit,
    Preview,
}

struct StagedTransaction<'s> {
    origin: RecordOrigin,
    target: &'s CanonicalTarget,
    store: &'s Store,
    record: StagedRecord,
    mutation: MutationMode,
}

struct Transition<'a> {
    state: TransitionState<'a>,
    accepted: Vec<Submission>,
}

enum TransitionState<'a> {
    Pending {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
    },
    Completed(Completed),
    Ended(Ended),
}

enum RecoveryOutcome<'a> {
    Questions {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
        record: StagedRecord,
    },
    Completed {
        completed: Completed,
        record: Option<StagedRecord>,
    },
    Ended(Ended),
}
```

Only `RecoveryOutcome::Completed` carries `Completed`, so a caller cannot build a plan from questions or an ended run without violating an exhaustive match. The optional record distinguishes a fresh completion from a recovered staged completion; apply removes an existing record only after `Applied::Written`.

`StagedTransaction` receives an already-created `CanonicalTarget` and already-resolved record. It does not accept a raw path, flattened defaults, or a transport document.

```rust
// src/cli/presentation.rs

struct QuestionOutput {
    document: serde_json::Value,
    summary: QuestionSummary,
    machine_guidance: Option<String>,
}

struct QuestionSummary {
    reached_messages: Vec<String>,
    answers: Vec<AnswerNeed>,
    next_command: String,
}

struct AnswerNeed {
    id: String,
    title: String,
    errors: Vec<String>,
}
```

`Outcome` adds one focused variant:

```rust
enum Outcome {
    Written(Vec<String>),
    Error(String),
    Document(serde_json::Value, u8),
    Questions(QuestionOutput),
    Saved(u8),
    NeedsTrust(String),
    Ambiguous { name: String, matches: Vec<String>, retry: Vec<String> },
}
```

`Document` keeps complete and ended protocol documents and `stage --async` output. `Questions` always exits 4 and owns only the human-versus-machine representation.

## Interfaces and ownership

```rust
// Adapt protocol::answer_headless without reimplementing its walk.
fn answer_apply<'a>(
    template: &'a Template,
    interview: Interview<'a>,
    answers: RawAnswers,
) -> Result<Transition<'a>, EvalError> {
    unimplemented!()
}

// Adapt one Pending::answer call without changing continue semantics.
fn answer_continue<'a>(
    pending: Pending<'a>,
    answers: RawAnswers,
) -> Result<Transition<'a>, EvalError> {
    unimplemented!()
}

// The sole staged-effect policy for accepted answer transitions.
fn commit_transition<'a>(
    transition: Transition<'a>,
    staged: StagedTransaction<'_>,
) -> Result<RecoveryOutcome<'a>, StagingError> {
    unimplemented!()
}
```

The answer adapters are private constructors around existing engine results. They own no parsing, validation, walk, or persistence. `commit_transition` earns its interface by hiding accepted-prefix extension and every state mutation for pending, completion, stop, abort, new records, existing records, and preview mode.

```rust
// src/cli/presentation.rs
fn questions(
    batch: &Batch,
    context: &Context,
    rejections: Option<&Rejections>,
    next_command: String,
    machine_guidance: Option<String>,
) -> QuestionOutput {
    unimplemented!()
}
```

`questions` calls `protocol::batch_document` with the same batch, context, and rejections used to derive the summary. It does not move protocol serialization into the CLI.

```rust
impl Outcome {
    fn finish(self) -> ExitCode {
        match self {
            Outcome::Questions(output) if io::stdout().is_terminal() => {
                println!("{}", output.summary);
                ExitCode::from(4)
            }
            Outcome::Questions(output) => {
                if let Some(guidance) = output.machine_guidance {
                    eprintln!("{guidance}");
                }
                println!("{}", serde_json::to_string_pretty(&output.document).unwrap());
                ExitCode::from(4)
            }
            // Existing variants keep their behavior.
            _ => unimplemented!(),
        }
    }
}
```

## Module and seam map

```text
Clap parses apply / continue
          │
          ▼
main.rs selects fresh, staged, or refused route
          │
          ├─ optional TEMPLATE + staged record
          │       └─ resolve formal identity assertion
          │
          ├─ stored template + commit ─► resume_template / Template::load
          ├─ configured_defaults ──────► origin-bearing Resolution
          ├─ replay_with_resolution ──► Interview
          └─ read_answers ─────────────► RawAnswers
                                           │
                 ┌─────────────────────────┴────────────────────────┐
                 ▼                                                  ▼
       answer_apply / answer_continue                    existing terminal driver
                 │
                 ▼
       Transition { state, accepted }
                 │
                 ▼
       recovery::commit_transition
          ┌──────────────┼──────────────────────┐
          ▼              ▼                      ▼
       Questions      Completed               Ended
          │              │                  Stop / Abort
          │              ▼
          │          Plan::build
          │        ┌─────┴──────┐
          │        ▼            ▼
          │      preview   Plan::apply_reporting
          │                         │ success
          │                         ▼
          │              Store::remove(CanonicalTarget)
          ▼
   presentation::questions
          │
          ▼
   Outcome::finish
     ├─ stdout terminal ─► concise summary
     └─ other stdout ────► existing JSON

raw PATH ─► canonical_target (sole factory) ─► CanonicalTarget
                                                 ├─ Store load/save/remove
                                                 ├─ protocol Context
                                                 ├─ Plan::build
                                                 └─ apply_reporting
```

`recovery` owns staged transition policy. `presentation` owns two real display adapters. `main` retains command routing and the plan/apply effect path. `interview`, `protocol`, `staging`, `plan`, and `apply` retain their existing domain ownership.

## Transaction and effect ordering

For staged recovery:

1. Build one `CanonicalTarget` through the sole factory and load the staged record.
2. Resolve an optional template assertion. Resolve the stored immutable source and load the template.
3. Produce the origin-bearing configured `Resolution` and report its warnings.
4. Replay committed submissions.
5. If replay is already complete, refuse the unused answers document without reading it.
6. Read and parse the answers document through the existing parser.
7. Run the existing pure engine transaction: `answer_headless` for apply, one `Pending::answer` for continue.
8. Extend a copy of the record only with returned accepted submissions.
9. Perform the approved staged effect in `commit_transition`.
10. Build a plan only for `Completed`.
11. Apply only for effective proceed: `Completed::step()` permits apply and the caller did not use `--dry-run`.
12. Remove a recovered record only after `Plan::apply_reporting` returns `Applied::Written`.

A rejected document returns the original pending state for that step. A tentative flow action does not persist, does not set a lasting dry-run disposition, and cannot remove a record. If `answer_headless` committed an earlier step before a later empty submission rejects, only the committed prefix returned in `accepted` is saved.

A planning, trust, conflict, file, or hook failure retains the staged record with accepted answers. A post-apply removal failure is reported after target effects have completed; the record remains for operator recovery.

## Compatibility

- Exit 4 continues to mean that answers are required.
- Non-terminal question output remains the existing pretty JSON document and schema.
- `stage --async` remains JSON on terminals and files.
- The staged-record schema is unchanged.
- Same-template named apply retains its behavior and becomes a parity peer of path-only apply.
- The optional template operand remains an identity assertion for a staged target.
- `CanonicalTarget` is constructed once and crosses every target consumer unchanged.
- `Resolution` remains the only carrier of configured values and origins into start/replay.
- `read_answers` and `protocol::parse_answers` remain the sole answer-file parse path.
- Wrong-template, no-record, complete-record, stale-source, replay, parse, evaluation, staging, planning, trust, conflict, file, hook, and cleanup failures remain distinct.
- No permission, access, timeout, pinned-version, application subprocess, trust, or hook policy is added.

## Failure behavior

| Condition | Result and state |
|---|---|
| Path-only answers without a staged record | Exit 1 before document read; name template-taking apply. |
| Different named template | Exit 1 before document read; name both intents; record unchanged. |
| Complete replay plus answers file | Exit 1 before document read; answers unused; record unchanged. |
| Stale source or replay failure | Existing actionable error; record unchanged; document unread. |
| Unreadable or malformed answers | Exit 1; record unchanged. |
| Rejected answer transaction | Exit 4 question result; no rejected submission persists. |
| Staged save failure | Exit 1 before plan/effects. |
| Plan, trust, or conflict failure | Existing distinct error; accepted answers remain staged. |
| File or hook failure | Existing effect error and partial-effect rules; accepted answers remain staged. |
| Successful recovered apply | Remove record after all target effects succeed. |
| Stop | Exit 0; no plan/effects; committed answers remain staged. |
| Abort | Exit 0 after record removal; removal I/O failure is nonzero. |
| Flow dry-run | Exit 0; accepted answers staged; plan shown; no target effects; record retained. |
| CLI dry-run | No apply, accepted-submission save, or abort removal. The outcome is simulated and staged bytes remain unchanged. |

## Behaviors to prove

1. A staged interview completed by `apply PATH --answers FILE` writes the same files, runs the same hooks, prints the same messages, returns the same code, and removes the same record as matching-template apply.
2. Both spellings return byte-equal non-terminal JSON and the same persisted accepted prefix when questions remain.
3. Both spellings surface the same evaluation, planning, trust, conflict, file, hook, and cleanup failures.
4. Path-only answers without a record do not open the file or read stdin.
5. A different named template does not open the file or read stdin.
6. A complete staged interview does not consume the supplied answers document.
7. A rejected document leaves the stored submission sequence byte-identical.
8. An accepted step followed by a rejected empty/default advance persists only the accepted prefix returned by `answer_headless`.
9. `continue` and apply feed the same `commit_transition` policy while preserving their one-step versus headless-walk engine semantics.
10. CLI dry-run leaves staged bytes unchanged for pending, completion, stop, and abort.
11. Flow dry-run saves committed answers, builds the same plan as proceed, writes no target file, runs no hook, and retains the record.
12. Stop saves its triggering accepted submission, builds no plan, and leaves the record.
13. Abort builds no plan, removes only the current canonical target's record, and surfaces removal failure.
14. A tentative abort plus a surviving rejected answer removes and changes nothing.
15. Plan, trust, conflict, file, and hook failures retain a completed staged record; successful apply removes it after the last effect.
16. A terminal question result is not JSON, lists every current id in order, includes effective errors and reached messages, names one executable next command, and exits 4.
17. The same result with piped or redirected stdout parses as the existing schema and is byte-equal to `protocol::batch_document`.
18. `stage --async` through a pseudoterminal remains JSON.
19. Terminal selection depends on stdout, not stdin.
20. Relative, parent-segment, symlink-ancestor, and root spellings use one `CanonicalTarget` for load, save, context, plan, apply, abort, and cleanup.
21. Configured-default warnings and fault origins are identical through path-only and matching-template recovery; no flat defaults or reconstructed origin appears.
22. Skip-before-pending, skip-to-completion, and skip-before-ended use the existing values and warning order through both apply spellings.
23. Removing the `Ended` match arm fails compilation after the accepted flow types land.

Sole-kill guards:

- Restore the path-only refusal: behavior 1 fails.
- Route matching-template recovery around `commit_transition`: parity behavior 2 or injected-failure behavior 3 fails.
- Save before the engine transaction returns: behavior 7 fails on the record-byte assertion.
- Drop an accepted empty submission: behavior 8 fails on the exact submission sequence.
- Reconstruct the target from a raw path: behavior 20 fails on cross-spelling identity.
- Flatten or rebuild configured origins: behavior 21 fails on exact diagnostic attribution.
- Remove staged state before apply succeeds: behavior 15 fails under injected plan/apply failure.
- Omit abort removal: behavior 13 fails on record presence.
- Render all `Document` outcomes by TTY: behavior 18 fails because `stage --async` changes.

## Canonical document and guide impact

The paired implementation updates:

- `docs/specifications/command-line-interface.yml` and `.spec.yml`: path-only staged recovery, matching-template identity assertion, terminal-versus-non-terminal question representation, exact exit codes, unused-document priority, and the approved preview/abort rule.
- `docs/specifications/interview-protocol.yml`: non-terminal question documents remain the protocol representation; staged answer recovery commits accepted prefixes and carries accepted `ended`/`disposition` results.
- `docs/technical-designs/architecture.yml`: `cli::recovery` owns staged transition effects and `cli::presentation` owns question rendering.
- `docs/template-interviews.md` and `docs/templates-management.md`: concrete apply/continue recovery commands, terminal summary, redirected JSON, stop versus abort retention, and flow dry-run behavior.

The paired implementation changes no staged-record or protocol schema for this feature. Any schema changes for `Ended` and `Disposition` remain owned by the accepted flow implementation.

## Alternatives considered

- **Retain the path-only refusal.** Callers must rediscover the staged template identity even though the target already owns it; recovery remains split.
- **Run continue and apply in sequence.** Two command phases duplicate flow handling and create an intermediate failure point. Launching a subprocess is also outside the approved integration rules.
- **Move all apply and continue logic behind one `execute(Request, Dirs)` method.** This has a small signature but mostly relocates established command phases. A focused commit policy supplies the required depth with less movement.
- **Add small match-arm helpers.** They reduce lines without owning persistence or outcome policy and remain pass-through methods.
- **Always emit JSON and keep guidance on stderr.** Terminal users still receive a protocol document instead of the requested short recovery instruction.
- **Add a `--json` or `--output` option.** Non-terminal stdout already preserves JSON under the proposed product rule. The extra public interface is unnecessary for the requested behavior.

## Tradeoffs

- The design accepts one CLI-private summary projection in exchange for keeping human presentation out of the wire protocol.
- It accepts saving a completed staged record before later planning in exchange for preserving accepted answers across downstream failures.
- It accepts a post-effect error when staged cleanup fails in exchange for never deleting recovery state before apply succeeds.
- It accepts shared transition policy with two engine adapters in exchange for preserving `continue`'s one-step semantics and apply's multi-step headless semantics.

## Decisions requested

### Decision 1 — target-only answers

- **1A — Resume and apply (recommended).** `apply PATH --answers FILE` uses the staged record at `PATH`, commits accepted progress, and applies on completion.
- **1B — Keep the refusal.** The caller must use `continue PATH FILE` or repeat the template operand.

1A uses the target identity the command already resolves and removes an unnecessary rediscovery step while preserving the same replay and apply checks.

### Decision 2 — exit-4 output on a terminal

- **2A — Show a concise summary (recommended).** Question results from `apply` and `continue` use the human summary when stdout is a terminal; piped or redirected stdout retains the existing JSON; `stage --async` always retains JSON.
- **2B — Always show JSON.** Keep the existing representation on terminals and non-terminals.

2A gives a person the missing answers and executable next command while preserving the protocol contract on machine output. It adds no output-format flag.

### Decision 3 — named recovery

- **3A — Preserve same-template resume (recommended).** `apply TEMPLATE PATH --answers FILE` treats `TEMPLATE` as an identity assertion and enters the same recovery path when it matches the staged formal identity.
- **3B — Refuse named recovery.** Require target-only recovery or `continue` whenever staged state exists.

3A preserves the supported command behavior and makes both apply spellings observably equivalent after identity selection.

## Consequential-change disclosure

Decision 2 intentionally changes terminal stdout for exit-4 question results from JSON to concise text. Non-terminal JSON and exit 4 remain unchanged. The design removes no supported command or operator capability. It adds no permission or access rule, timeout, pinned-version check, or application subprocess.

## Risks

CLI preview plus author-declared abort is resolved by the existing dry-run contract: the run reports the ended abort result but retains staged bytes. A normal run performs the approved abort removal. This keeps `--dry-run` effect-free without reopening the adopted flow action.

The implementation can fail if the terminal check moves before result construction, if terminal and JSON renderers derive errors independently, if an accepted prefix is saved before the engine returns it, or if a broad refactor changes error priority. The behaviors above make each failure observable.

## Synthesis decision

Candidate 1 is the structural base. Candidate 2 contributes the shared transition commit policy, completed-only planning type, route-parity matrix, and exhaustive `Ended` guard. The cross-judge preferred candidate 2 on interface depth; the synthesis closes its cited gaps without adopting the whole-command relocation. `synthesis.md` records the complete reconciliation, grafts, and rejections.

## Next implementation step

After exact approval, create the private `Transition` and `commit_transition` policy, route the existing matching-template apply and continue answer paths through it without changing behavior, then enable target-only answers and the focused `Outcome::Questions` renderer.
