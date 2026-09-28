# Caller usage

Path-only `apply` resumes the interview staged for the target. The target supplies the template identity.

```text
$ toha apply --answers answers.json ./output
create README.txt
create config.yml
$ echo $?
0
```

The template-taking form reaches the same recovery path when its resolved formal identity matches the staged record:

```text
$ toha apply --answers answers.json ./fixture ./output
create README.txt
create config.yml
```

If the document remains incomplete and stdout is a terminal, `apply` prints a concise summary:

```text
$ toha apply --answers partial.json ./output
answers needed:
  label — Display label
  quantity — Item count
next: update partial.json, then run:
  toha apply --answers partial.json ./output
$ echo $?
4
```

Rejections appear with the affected answer:

```text
answers needed:
  quantity — must be at least 1
next: update partial.json, then run:
  toha apply --answers partial.json ./output
```

A pipe or redirected stdout retains the existing JSON document and exit code:

```text
$ toha apply --answers partial.json ./output > questions.json
$ status=$?
$ jq -r .status questions.json
questions
$ echo "$status"
4
```

`stage --async` remains an explicit machine-output command and always emits or writes JSON, including when stdout is a terminal.

## Recommended behavior

1. `toha apply PATH --answers FILE` resumes an incomplete staged interview at `PATH`. It applies when the document completes the interview.
2. An exit-4 question result uses the concise summary when written directly to a terminal. Non-terminal output retains the current pretty-printed JSON bytes.
3. `toha apply TEMPLATE PATH --answers FILE` resumes when `TEMPLATE` resolves to the staged formal identity. It uses the same recovery path as the path-only spelling.

A path-only invocation with no staged record fails before reading the answers document and names the template-taking form.

A template-taking invocation with a different staged identity fails before reading the answers document. It retains the existing wrong-template guidance.

# Problem

The engine already supplies the required invariants: `Resolution` retains configured-default origins, replay reconstructs state from accepted submissions, `answer_headless` treats each answers document as a transaction, and `CanonicalTarget` supplies one identity to storage, protocol, planning, and apply.

The non-obvious policy sits in the CLI. Two apply spellings must converge after staged identity selection. Accepted submissions must be persisted at the correct point. Future flow outcomes must not accidentally reach planning. Presentation must vary by output destination without changing engine behavior or the machine contract.

The existing `run` and `continue_run` functions distribute that policy across similar match arms. Recovery belongs in one private CLI module.

# Shape

## Core data structures

```rust
// src/cli/interview_command.rs
// Private to the binary crate.

pub(crate) enum Request<'a> {
    Continue {
        path: &'a Path,
        answers: Option<&'a str>,
    },
    Apply {
        template: Option<&'a str>,
        path: &'a Path,
        answers: Option<&'a str>,
        force: bool,
        dry_run: bool,
        trust: bool,
    },
}

pub(crate) fn execute(
    request: Request<'_>,
    dirs: &Dirs,
) -> Outcome {
    unimplemented!()
}
```

This is the module’s only external interface. `main` translates parsed Clap values into `Request` and finishes the returned `Outcome`.

The module owns target construction, staged selection, template identity checks, replay, answer submission, persistence transitions, flow routing, plan construction, apply, and recovery guidance. It does not own template loading rules, answer validation, protocol serialization, planning rules, storage mechanics, or terminal prompting.

```rust
struct RunContext<'a> {
    target: CanonicalTarget,
    store: Store,
    config: Config,
    registry: Registry,
    cwd: PathBuf,
    dirs: &'a Dirs,
}

enum ApplySource {
    New {
        resolved: ResolvedTemplate,
        record: StagedRecord,
    },
    Staged {
        resolved: ResolvedTemplate,
        record: StagedRecord,
    },
}

enum Persistence {
    Record,
    CliDryRun,
}

/// A protocol/terminal transition plus the submissions committed by the
/// pure interview engine.
struct Transition<'a> {
    state: Interview<'a>,
    accepted: Vec<Submission>,
    rejections: Rejections,
}

type Submission = IndexMap<String, serde_json::Value>;
```

`ApplySource::Staged` is selected for both accepted apply spellings. The optional template operand is only an identity assertion; it does not select a second execution route.

```rust
enum StagedOutcome<'a> {
    Questions {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
        record: StagedRecord,
    },
    Completed {
        completed: Completed,
        record: StagedRecord,
    },
    Ended {
        ended: Ended,
    },
}
```

`StagedOutcome` makes planning impossible for `Questions` and `Ended`. Only `Completed` carries the value accepted by `Plan::build`.

```rust
struct QuestionOutput {
    json: serde_json::Value,
    human: QuestionSummary,
    machine_guidance: Option<String>,
}

struct QuestionSummary {
    answers: Vec<AnswerNeed>,
    next_command: String,
    dry_run_note: Option<String>,
}

struct AnswerNeed {
    id: String,
    title: String,
    errors: Vec<String>,
}
```

`QuestionSummary` is a CLI presentation value. It is derived from `Batch`, its carried errors, and the current rejections. It is not a protocol type and does not enter the library interface.

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

`Outcome::finish` is the only terminal-detection seam:

```rust
impl Outcome {
    fn finish(self) -> ExitCode {
        match self {
            Outcome::Questions(output) if io::stdout().is_terminal() => {
                println!("{}", output.human);
                ExitCode::from(4)
            }
            Outcome::Questions(output) => {
                if let Some(guidance) = output.machine_guidance {
                    eprintln!("{guidance}");
                }
                println!("{}", serde_json::to_string_pretty(&output.json).unwrap());
                ExitCode::from(4)
            }
            // Existing outcomes retain their behavior.
            _ => unimplemented!(),
        }
    }
}
```

Terminal detection changes presentation only. Recovery, persistence, exit status, and the canonical JSON document are already fixed when `finish` runs.

## Internal functions

```rust
fn prepare_context(
    path: &Path,
    dirs: &Dirs,
) -> Result<RunContext<'_>, Outcome> {
    unimplemented!()
}

fn select_apply_source(
    requested_template: Option<&str>,
    existing: Option<StagedRecord>,
    ctx: &RunContext<'_>,
    invocation: &Invocation<'_>,
) -> Result<ApplySource, Outcome> {
    unimplemented!()
}
```

`prepare_context` calls the sole `canonical_target` factory once. `select_apply_source` resolves a supplied template identity before an answers file is opened.

```rust
fn replay_staged<'a>(
    source: &StagedRecord,
    resolved: &ResolvedTemplate,
    template: &'a Template,
    config: &Config,
) -> Result<Interview<'a>, Outcome> {
    // configured_defaults(...)
    // report Resolution::warnings()
    // source.replay_with_resolution(template, resolution)
    unimplemented!()
}
```

This is the only staged replay route for `continue` and both apply spellings. It consumes the origin-bearing `Resolution`; it never uses `into_flat_defaults`.

```rust
fn answer_for_apply<'a>(
    template: &Template,
    interview: Interview<'a>,
    document: RawAnswers,
) -> Result<Transition<'a>, Outcome> {
    // Adapt protocol::answer_headless without reimplementing its walk.
    unimplemented!()
}

fn answer_for_continue<'a>(
    interview: Interview<'a>,
    document: RawAnswers,
) -> Result<Transition<'a>, Outcome> {
    // Preserve continue's one-step behavior through Pending::answer.
    unimplemented!()
}
```

These adapters normalize the existing engine results. They do not validate answers themselves.

```rust
fn commit_staged<'a>(
    transition: Transition<'a>,
    mut record: StagedRecord,
    target: &CanonicalTarget,
    store: &Store,
    persistence: Persistence,
) -> Result<StagedOutcome<'a>, Outcome> {
    unimplemented!()
}
```

`commit_staged` is the single owner of staged transition policy:

- It extends `record.submissions` only with `transition.accepted`.
- `Persistence::CliDryRun` does not save accepted submissions.
- `Pending` saves the extended record under `Persistence::Record`.
- `Completed` saves the extended record under `Persistence::Record` before planning.
- `Ended(Stop)` saves committed submissions under `Persistence::Record` and preserves the record.
- `Ended(Abort)` removes the record through `Store::remove(&CanonicalTarget)` and surfaces removal failure.
- Rejected documents contribute no accepted submission and cause no mutation.

An explicit flow abort remains a terminal discard action when combined with `--dry-run`; `--dry-run` suppresses normal submission persistence and plan application, not the separately declared abort action.

```rust
fn finish_completed_apply(
    completed: Completed,
    record: Option<StagedRecord>,
    source: &ApplySource,
    request: &ApplyRequest<'_>,
    ctx: &RunContext<'_>,
) -> Outcome {
    // Plan::build is pure.
    // apply = completed.step().apply && !request.dry_run
    // Remove staged record only after Applied::Written.
    unimplemented!()
}
```

A flow dry-run and CLI `--dry-run` compose by union. Both build and display the plan. Neither calls `Plan::apply_reporting`.

## Module and seam map

```text
Clap Command::{Apply, Continue}
              │
              ▼
main.rs ──► cli::interview_command::execute(Request, Dirs)
              │
              ├─ canonical_target(raw path) ─► CanonicalTarget
              ├─ Store::load(CanonicalTarget)
              ├─ select new or staged source
              │      └─ optional TEMPLATE is an identity assertion
              ├─ resolve/load template
              ├─ configured_defaults ─► origin-bearing Resolution
              ├─ StagedRecord::replay_with_resolution
              ├─ read_answers ─► protocol::parse_answers
              ├─ protocol::answer_headless / Pending::answer
              │
              ├─ commit_staged
              │      ├─ Pending ───────► save accepted prefix
              │      ├─ Completed ─────► save before planning
              │      ├─ Ended(Stop) ───► save; preserve record
              │      └─ Ended(Abort) ──► Store::remove
              │
              ├─ Completed::step
              │      ├─ Proceed ─► Plan::build ─► Plan::apply_reporting
              │      └─ DryRun ──► Plan::build ─► preview
              │
              └─ Outcome::Questions
                         │
                         ▼
                Outcome::finish
                  ├─ stdout terminal ─► concise summary
                  └─ other output ────► existing JSON
```

The module is deep: one request interface hides staged identity selection, replay, transaction adaptation, persistence order, flow routing, apply cleanup, and output preparation. No transport type appears in its interface.

# Persistence and effects

The ordering is:

1. Construct one `CanonicalTarget`.
2. Load the staged record.
3. Resolve any supplied template identity.
4. Resolve the stored immutable source and load the template.
5. Create and report the origin-bearing `Resolution`.
6. Replay committed submissions.
7. Read and parse the answers document.
8. Submit through the existing engine transaction.
9. Apply the staged transition policy.
10. Build a plan only from `Completed`.
11. Apply only for effective `Proceed`.
12. Remove a staged record only after successful apply, or after a committed `Abort`.

A rejected document leaves the original `Pending` and record unchanged.

A normal incomplete recovery persists every accepted submission returned by `answer_headless`, including accepted empty submissions. A CLI dry-run persists none of them.

Planning, trust refusal, conflict detection, file failure, and hook failure retain the staged record. Successful `Applied::Written` removes it. A removal failure after successful apply is reported as a staging failure; it is not presented as a fully successful staged cleanup.

`Stop` cannot construct a plan because it has no `Completed`. `Abort` cannot construct a plan and uses the existing store removal operation. `DryRun` has a `Completed`, so it builds the complete plan but does not enter the apply effect loop.

# Behavior matrix

| Request and state | Engine result | Staged mutation | Files/hooks | Result |
|---|---|---|---|---|
| `apply PATH --answers FILE`, no record | Not evaluated | None | None | Exit 1; name `apply TEMPLATE PATH --answers FILE` |
| Path-only or matching-template apply, incomplete record | `Pending` | Save accepted prefix unless CLI dry-run | None | Exit 4; terminal summary or non-terminal JSON |
| Path-only or matching-template apply, complete record before reading FILE | Complete before submission | None | None | Exit 1; answers document unused |
| Path-only or matching-template apply | `Completed(Proceed)` | Save accepted prefix before plan | Apply | Remove record after apply succeeds |
| Path-only or matching-template apply | `Completed(DryRun)` | Save accepted prefix unless CLI dry-run | Plan only | Exit 0; record remains |
| Path-only or matching-template apply | `Ended(Stop)` | Save committed answers unless CLI dry-run | None | Exit 0; record remains |
| Path-only or matching-template apply | `Ended(Abort)` | Remove current record; surface failure | None | Exit 0 on removal success |
| Any staged answer request | Rejected document | None | None | Exit 4 with rejections |
| Matching-template apply | Broken replay or stale source | None | None | Existing distinct failure and guidance |
| Different-template apply | Identity mismatch | None; FILE unread | None | Existing wrong-template refusal |
| Apply after completed plan | Trust refusal, conflict, plan/apply/hook failure | Record remains | Existing partial-effect rules | Existing distinct failure |
| `stage --async` | `Pending` | Existing stage save | None | JSON regardless of terminal |
| `continue PATH FILE` | `Pending` | Save accepted submission | None | Same terminal/non-terminal exit-4 presentation rule |

Skip stays inside the engine. It can lead to any applicable `Pending`, `Completed`, or `Ended` result without a CLI-specific branch.

# Error behavior

The design retains the existing error owners:

- `read_answers` owns file, UTF-8, object-shape, and JSON answer parsing errors.
- Template resolution owns ambiguous, missing, stale, and wrong-source failures.
- `Resolution` owns configured-default selection and provenance.
- Replay owns stored-submission incompatibility.
- The interview engine owns evaluation and rejection.
- Planning owns render, target-path, duplicate, and conflict discovery.
- Apply owns file and hook effects.
- Staging owns load, atomic save, and removal I/O failures.

An answers document is not opened until target state and an optional template identity are accepted. Wrong-template, no-record, stale-source, replay, and trust failures therefore do not consume stdin or unnecessarily read a file.

# Compatibility

- Exit 4 continues to mean that answers are required.
- Redirected and piped question output remains the existing pretty-printed protocol JSON.
- The JSON schema and document construction remain in `protocol::batch_document`.
- `stage --async` remains machine-oriented even on a terminal.
- Complete documents retain their existing presentation.
- No staged-record field changes.
- No new library interface is added.
- No second answer parser, interview walker, target normalizer, or provenance source is added.
- Existing `CanonicalTarget`, `Resolution`, `Store`, protocol, plan, and apply interfaces remain authoritative.
- The only intentional compatibility change is the exit-4 representation on terminal stdout.

# Falsifiable tests

1. Stage an incomplete interview, then complete it with `apply PATH --answers FILE`; assert files, hooks, exit 0, and record removal.
2. Repeat with `apply TEMPLATE PATH --answers FILE`; assert byte-equal stdout/stderr, file tree, hook recording, and staging state.
3. Use a different template identity; assert the answers file is not opened and the record bytes are unchanged.
4. Use path-only apply without a record; assert exit 1 and guidance naming the template-taking form.
5. Leave required answers missing with stdout attached to a pseudo-terminal; assert exit 4, no JSON object, ordered ids/errors, and the exact next command.
6. Run the same command with stdout piped; assert exit 4 and byte-equal JSON to `protocol::batch_document`.
7. Run `stage --async` with stdout attached to a pseudo-terminal; assert JSON, not the summary.
8. Submit a document with one rejected answer; assert no new staged submission.
9. Submit an accepted prefix that reaches another batch; assert every returned accepted submission is stored in order.
10. Repeat tests 8 and 9 with `--dry-run`; assert record bytes are unchanged.
11. Complete a staged interview, then force `Plan::build` failure; assert the accepted submission is saved and the record remains.
12. Force trust refusal, conflict, file failure, and hook failure; assert the record remains.
13. Complete and apply successfully; assert removal happens after the apply recording and not before it.
14. Trigger `Ended(Stop)`; assert no plan construction, no file/hook effects, and retained staged state.
15. Trigger `Ended(Abort)`; assert no plan construction, record removal, and surfaced removal I/O failure.
16. Trigger `Completed(DryRun)` and CLI `--dry-run`; assert plan equality with `Proceed`, no apply, and union behavior.
17. Combine a flow-triggering answer with a rejected answer; assert no flow effect, no save, and no removal.
18. Supply an early answer that skip discards; assert existing warning order and no CLI-specific traversal.
19. Break replay and stale-source resolution; assert the document is not consumed and the record is unchanged.
20. Compile the apply result match without an `Ended` arm; exhaustive matching must fail after the accepted flow types land.

Sole-kill checks:

- Restore the path-only early refusal: test 1 fails.
- Route matching-template apply separately: parity test 2 detects drift.
- Move terminal detection before outcome creation: pipe/TTY parity tests detect behavioral divergence.
- Save before `answer_headless` returns: rejected-document test 8 fails.
- Remove the record before apply: plan/trust/conflict tests fail.
- Omit abort removal: test 15 fails.

# Rationale

## Synthesis decision

The design uses one private, policy-owning command module as the base. Both apply spellings become the same `ApplySource::Staged` before replay or answer submission. A single staged transition function owns persistence and future flow outcomes. Presentation remains a final adapter concern.

This shape provides more interface depth than a new collection of routing helpers. Deleting the module would redistribute identity selection, replay, transaction adaptation, persistence ordering, flow handling, and cleanup across `main`, `continue`, and apply call sites.

## Tradeoffs accepted

- We accept a larger private command module in exchange for one recovery policy and a one-call interface from `main`.
- We accept changed output for programs that deliberately parse stdout while attached to a terminal in exchange for concise human exit-4 output; pipes and redirection preserve the machine contract.
- We accept automatic terminal selection instead of a new format option in exchange for no additional CLI surface.
- We accept that explicit `flow: abort` still discards staged state when `--dry-run` is present in exchange for preserving the declared abort meaning across drivers.
- We accept storing a completed or stopped submission before later planning in exchange for replayable accepted input when a downstream effect fails.

## Alternatives considered

- Keep path-only `--answers` invalid. This leaves callers responsible for rediscovering the staged template identity and preserves the duplicated recovery paths. It hides little policy and provides low leverage.
- Implement path-only apply as `continue` followed by `apply`. A subprocess violates the integration constraint. An in-process sequence still exposes temporal stages, gives partial failure between them, and handles future flow outcomes twice.
- Add small helpers around the existing match arms. This reduces line duplication but leaves persistence timing and flow exhaustiveness distributed across callers; the helpers are pass-through methods.
- Always emit JSON and add a summary on stderr. This preserves machine stdout but gives terminal users two competing representations and retains split guidance ownership.
- Add `--output human|json`. This provides explicit selection but expands the interface for behavior already determined by the output destination. Non-terminal JSON and `stage --async` cover the machine use cases.

# Risks

- Terminal detection can be placed too early and alter command behavior. The `Outcome::finish` seam and pseudo-terminal tests constrain it to presentation.
- Human and JSON error grouping can drift. `QuestionSummary` derives both carried and current rejections from the same `Batch` and `Rejections` used to build the JSON document.
- The accepted flow implementation widens `Interview`, terminal session, and headless results. Exhaustive matches in the policy module make every missing outcome a compile failure.
- An apply failure after accepted submissions are saved leaves a complete staged record. This is intentional so that retry does not require resubmitting answers.

# First implementation step

Create the private `cli::interview_command` module, move `run` and `continue_run` behind its single `execute(Request, Dirs)` interface without changing behavior, and pin both existing apply spellings to one staged-source branch before adding path-only answers and terminal presentation.