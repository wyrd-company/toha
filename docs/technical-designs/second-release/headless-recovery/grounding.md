---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Incomplete headless answer recovery: grounding

## Scope and evidence basis

This design decides how `apply` recovers when an answers document leaves a staged interview incomplete. It covers three command questions: path-only apply with an answers file, same-template apply with an answers file, and terminal presentation of exit-4 question batches. It does not change the interview language, the answers document, or the staged record.

The source basis is epic head `1e663bd661304698ebd10219c8c52d78020a189e`. The accepted flow design is `docs/technical-designs/second-release/confirm-flow/design.md` at SHA-256 `54827ce90a185fa24568c79a14707815e225121fbaad7c060154ae17adf5381e`; its verification is `9167bf7a48e3c65aaee4dc1dfb7dbbae62cf8d2188b0d9ff8ffc63bc370f8d52`. The integrated error-attribution design is `docs/technical-designs/second-release/error-attribution/design.md` at SHA-256 `9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`; its runtime producer is integrated through `8a19269d78f6da0738edb17e56310621f143923d`.

## How the current command works

### Entry and output

`Command::Apply` parses one operand as the target and two as template plus target. `--answers` is an optional file name and `-` means stdin (`src/main.rs:82-95`, `143-159`). `main` passes the parsed values to `run`. `Outcome::Document(value, code)` owns JSON serialization and writes the pretty JSON to stdout before returning the supplied exit code; it does not inspect whether stdout is a terminal (`src/main.rs:312-370`). Human guidance is produced separately through `guidance` and printed to stderr at the call sites.

`read_answers` reads a UTF-8 file or stdin, requires a JSON object keyed by question id, then delegates value parsing to `protocol::parse_answers` (`src/main.rs:398-414`). This is one parse boundary shared by new and resumed apply paths.

### Target identity and staged ownership

Every staged operation first calls `setup`, which creates the sole `CanonicalTarget` through `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>` and constructs a `Store` (`src/main.rs:440-445`; `src/staging.rs:109-154`). The opaque canonical target is the staging key and the target passed to planning and apply. Recovery must carry this value; it must not normalize a raw path again.

`StagedRecord` owns the template identity, immutable source commit, target key, fixed interview time, and committed submission sequence. It stores no derived interview state. `replay_with_resolution` consumes the current origin-bearing `Resolution`, starts the interview with its defaults, and replays committed submissions (`src/staging.rs:156-236`). `Resolution` remains the sole carrier of configured default values and origins into start/replay. Recovery must not rebuild origins from flattened answers.

### Current behavior by command form

| Invocation | No staged record | Matching incomplete staged record | Matching complete staged record | Other template staged |
|---|---|---|---|---|
| `apply PATH --answers FILE` | Exit 1 and name the template-taking form | Exit 1 and name `continue`, plain `apply`, and named `apply --answers` | Exit 1 | Exit 1 |
| `apply TEMPLATE PATH --answers FILE` | Start headless, accept what stands, then apply on completion or stage and exit 4 | Resume, accept what stands, then apply on completion or persist and exit 4 | Exit 1 because the document is unused | Exit 1 with both intents named |
| `apply PATH` | Exit 1 | Prompt when stdin and stdout are terminals; otherwise emit the JSON batch with exit 4 | Apply | N/A |
| `continue PATH FILE` | Exit 1 | Accept one batch, persist it, return complete JSON or the next JSON batch | Exit 1 because the document is unused | N/A |

The path-only answers refusal is an explicit early branch before apply routing (`src/main.rs:809-816`) and is pinned by `answers_without_template_name_the_commands_that_take_them` (`tests/staged_commands.rs:628-661`). The matching-template form is already a resume path: `staged_refusal` permits equal formal identity, `template.filter(|_| existing.is_none())` converts it to the staged arm, and that arm replays then calls `protocol::answer_headless` (`src/main.rs:822-835`, `916-1013`). Tests cover same-template dry-run and partial recovery (`tests/staged_commands.rs:299-320`, `398-440`).

### Answer transaction and persistence

`protocol::answer_headless` submits the supplied document once, then submits empty documents so configured defaults and optional empty values can advance later batches without another file. It returns every accepted submission, including any empty submissions, plus one of `Completed` or `Pending`; a rejection stops the loop with the original pending state for that step (`src/protocol.rs:125-205`). The caller owns persistence. For a resumed staged apply, accepted submissions extend the record only after the headless result returns. A normal incomplete run saves them and exits 4; `--dry-run` saves none. A completed normal run saves accepted submissions before planning, then removes the record only after `Plan::apply` succeeds (`src/main.rs:955-1013`, `1047-1146`). This places staging mutations after the engine's accepted transaction and preserves rejected-document atomicity.

`continue PATH FILE` implements a narrower copy of the same policy: replay, parse one document, call `Pending::answer` once (the document may also hold answers for later questions), save only an accepted submission, then return complete or pending JSON (`src/main.rs:655-774`). The duplication is a locality risk: recovery semantics can drift between `continue` and the two apply spellings.

### Effects and failures

Only `Plan::apply_reporting` writes files or runs hooks. `Plan::build` is pure and receives the existing `CanonicalTarget` (`src/main.rs:1074-1146`). A pending outcome cannot reach plan construction. A completed apply can still become a preview through the CLI `--dry-run` flag, or through the accepted flow design's future `Completed::step()` result. A successful staged apply removes its record after file and hook effects succeed. Conflicts, trust refusal, load/resolve/replay errors, malformed answer documents, and staging I/O failures keep the record available unless the accepted flow result is abort.

## Accepted flow outcomes that recovery must carry

The accepted flow design adds `Interview::Ended(Ended)` for `flow: stop` and `flow: abort`, and adds a `Disposition::DryRun` to `Completed`. `stop` returns an ended document and does not plan. `abort` does the same and asks the driver to remove the current target's staged record through `Store::remove(&CanonicalTarget)`. `dry-run` still completes and builds a plan but suppresses apply. `skip` remains inside the interview walk and can lead to asking, completion, or an ended result.

Ordinary `confirm` questions remain booleans. A confirm answer is only a value used by a later flow node's `when` expression. Recovery therefore maps the result terms `Pending`, `Completed(Proceed)`, `Completed(DryRun)`, `Ended(Stop)`, and `Ended(Abort)`. It does not map a confirm answer to an action.

For a document containing both an answer that would trigger flow and a rejected answer, the accepted transaction discards the tentative flow result. No submission is persisted and abort removal does not run. For a committed `Ended(Stop)`, an existing staged record remains resumable. For a committed `Ended(Abort)`, the driver removes that record; removal failure is a `StagingError`. These constraints come from the accepted design at `confirm-flow/design.md:401-430` and `457-479`.

## Why the current shape exists

### Evidence

- Commit `77309c0b` introduced staged protocol, resumable CLI, answer-file parsing, exit 4, and the apply/continue split in the first release.
- The later commit titled `fix(cli): resume staged interviews from apply and continue` explicitly made named apply resume a staged interview of the same formal template and kept wrong-state refusals actionable.
- Follow-up commits made partial apply name next commands and made command suggestions derive from parsed operands and flags. The present guidance is therefore a product surface, not incidental logging.
- The error-attribution implementation moved all target users to the opaque `CanonicalTarget` and all configured-default callers to origin-bearing consuming `Resolution`; those are current code contracts.
- The accepted confirm-flow design keeps flow evaluation pure and assigns abort removal, plan construction, apply, and output rendering to drivers.

### Inference and source gaps

The repository history explains why matching-template apply resumes and why refusals name working commands. It does not record a product reason for refusing `apply PATH --answers FILE` when the target itself identifies an incomplete staged interview. That refusal appears to preserve the first-release grammar rather than protect a separate invariant.

The repository history and task archive contain no decision about switching exit-4 stdout between JSON and a terminal summary. Lore keyword search for incomplete headless recovery returned no result; semantic retrieval was unavailable. No issue tracker, chat archive, observability store, exception tracker, or product analytics source is connected in this environment. Those categories were not searchable. The checkpoint must therefore treat terminal presentation as a new product decision.

## Design constraints

### Preserve

- Exit 4 means more answers are required.
- Non-terminal/script output remains the same JSON question-batch contract.
- Answer parsing and interview decisions remain in `read_answers` and the pure interview/protocol modules.
- Accepted submissions persist atomically; rejected submissions and CLI dry-runs do not mutate staged state.
- The sole target factory and origin-bearing `Resolution` cross every recovery path unchanged.
- Wrong-template, stale-source, replay, trust, conflict, and I/O failures remain distinguishable and keep actionable guidance.
- Future flow results retain their accepted meanings: stop leaves staged state; abort removes it; dry-run plans without apply; skip uses ordinary answer/default rules.

### Change candidates may explore

- Make path-only apply with `--answers` use an existing staged record.
- Express both apply spellings through one staged-answer recovery policy.
- Select a concise human rendering for an exit-4 question batch when stdout is a terminal, while keeping the JSON bytes for non-terminals and explicit machine-output modes.

### Avoid

- A second interview walker, target normalizer, answer parser, provenance source, or persisted derived state.
- Public transport types or a new library interface for a CLI-only routing concern.
- Treating confirm answers as stop/dry-run/skip actions.
- Saving accepted answers before the engine's rejection gate, planning an ended result, or swallowing abort-removal failures.
- New permissions, access rules, timeouts, pinned-version checks, or application subprocesses.

### Risks

- A helper that only moves the existing `match` arms becomes a shallow pass-through rather than a policy owner.
- Terminal detection at the wrong seam can corrupt piped JSON or change `stage --async`, whose explicit purpose is machine output.
- Applying after completion can remove staged state too early when planning, trust, conflict, hook, or file effects fail.
- The future `Ended` arm can be omitted from one route if recovery is copied instead of centralized.

## Verification targets

The design must make each of these falsifiable through the command interface: both accepted apply spellings yield identical state, output, files, hooks, and failures from the same staged record; non-terminal exit 4 remains valid JSON; a terminal exit 4 uses the approved human form; rejected answers do not persist; CLI dry-run does not persist; successful apply removes staged state; stop preserves it; abort removes it or reports removal failure; dry-run and skip retain their accepted outcomes; unmatched template and broken replay never consume the document or alter state.
