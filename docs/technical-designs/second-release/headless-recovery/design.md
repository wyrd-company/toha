---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Caller routes, identity-bearing answers, and staged recovery

## Purpose

Toha has three kinds of caller: scripts, agents, and people. Each kind has its
own command route. The route alone selects the input and output modality. No
command guesses the caller from terminal status of standard output, and only the
routes that exist to prompt a person inspect standard input.

Every external answers document identifies the template it answers. Toha
compares that declaration with the formal template identity already established
by the route before it evaluates any answer. A document authored for another
template cannot be accepted because question identifiers happen to match.

## Caller routes

| Caller | Route | Stages | Standard output |
|---|---|---|---|
| Script | `apply TEMPLATE PATH --answers FILE` | Never | One JSON result document for every outcome. |
| Agent | `stage TEMPLATE PATH --async [FILE]` | Yes | Question batch JSON and plain-text instructions. |
| Agent | `continue PATH FILE` (`-` reads standard input) | Yes | Question batch JSON and instructions, or instructions only when complete. |
| Agent | `apply PATH` | Consumes | Files written; batch JSON and instructions when incomplete. |
| Person | `stage TEMPLATE PATH` | Yes | Prompts, then the dry-run plan and apply instructions. |
| Person | `continue PATH` | Yes | Prompts, then the dry-run plan and apply instructions. |
| Person | `apply TEMPLATE PATH` | Yes | Prompts, then files written. |

Agent and person routes share one staged record. An agent can stage an
interview and a person can finish it with `continue PATH` or
`apply TEMPLATE PATH`. A person can stop with Ctrl-C and an agent can finish it
with `continue PATH FILE`. The staged record carries no caller kind.

### Scripted route

`apply TEMPLATE PATH --answers FILE` is one shot. The caller supplies every
answer the run needs. The route:

- refuses before reading the document when an interview is staged at the path;
- never creates, changes, or removes staged state;
- writes one JSON result document to standard output for every outcome after
  argument parsing, and nothing else to standard output;
- writes no recovery instructions.

`--dry-run` builds and reports the plan and writes nothing. `--trust` and
`--force` keep their meanings.

### Agent route

```console
$ toha stage forge:catalog/receipt@stable ./output --async
{ ...question batch... }

answer these questions with an answers document:
  {"template": "forge:catalog/receipt@stable", "answers": { ... }}
then run: toha continue ./output <ANSWERS> (- reads standard input)
$ echo $?
4
```

`stage --async FILE` writes the batch document to FILE and writes the
instructions to standard output.

`continue PATH FILE` refuses when nothing is staged. It submits one batch. When
the batch is accepted and questions remain, it saves the accepted submission and
writes the next batch with instructions; exit 4. When answers are rejected, it
writes the same batch with per-question errors and instructions; nothing is
saved; exit 4. When the interview completes, it saves the submission and writes
instructions only; exit 0:

```text
the interview at ./output is complete
to see the files it will write: toha apply ./output --dry-run
to write them: toha apply ./output
```

`apply PATH` applies a complete staged interview. On an incomplete interview it
does not prompt. It writes the current batch and instructions that name both
`continue PATH <ANSWERS>` and `continue PATH`; exit 4. With nothing staged it
refuses and names `stage` and `apply TEMPLATE PATH`.

On agent routes, a question that has no answer yet is part of the next batch. It
is not an error. `errors` holds only rejected answers and held answers that
failed when their questions were reached.

### Person route

`stage TEMPLATE PATH` prompts for every question and saves the staged record at
each batch boundary. At completion it shows the dry-run plan and the apply
instructions, and writes nothing to the target.

`continue PATH` refuses when nothing is staged. It prompts for the remaining
questions with the same batch saving, and at completion behaves as `stage`
does. It is the recovery route after an interrupt or a fault.

`apply TEMPLATE PATH` is the direct route. With nothing staged it starts the
interview, saves at each batch boundary, and applies at completion. With an
incomplete staged interview for the same template it prompts for the remaining
questions, then applies. With a complete staged interview for the same template
it applies. A staged interview for another template is refused, naming both
templates. After an interrupt, `continue PATH` and `apply TEMPLATE PATH` both
resume.

Prompting routes need a terminal on standard input. Without one they refuse and
name the agent route.

## Answers document contract

An external answers document has exactly two top-level members:

```json
{
  "template": "forge:catalog/receipt@stable",
  "answers": {
    "label": "Sample",
    "quantity": 4
  }
}
```

The schema is:

```yaml
answers-document:
  type: object
  additionalProperties: false
  required:
    - template
    - answers
  properties:
    template:
      type: string
      minLength: 1
      description: Exact formal template name copied from context.template.
    answers:
      $ref: "#/$defs/answers"
```

The envelope has no protocol-version member and adds no pinned version check. It
has no target or commit member. A matching document can answer another target or
another resolved commit of the same formal template identity.

A bare answer map is invalid because it contains no template assertion. The
diagnostic shows the required wrapper and the expected formal identity. Toha
does not infer the template from the active interview.

## Template identity authority

`ResolvedTemplate.formal_name` and `StagedRecord.template` are the identity
authorities. Question output publishes that value as `context.template`. A
producer copies it without reconstructing a source name.

The declared `template` is compared with the route's established formal name by
exact string equality. Toha does not trim, case-fold, canonicalize a path, remove
`.git`, expand a host prefix, resolve an alias, inspect a registry, fetch a
source, or run a trust check on the declared value.

A non-empty declared string is never independently "unresolved." It matches the
expected formal name or it is a mismatch. Source-resolution errors belong to
route preparation and occur before document comparison.

| Route | Expected identity |
|---|---|
| `apply TEMPLATE PATH --answers FILE` | `ResolvedTemplate.formal_name` of the command template. |
| `continue PATH FILE` | `StagedRecord.template` at the canonical target. |
| Crate document operation | A formal name the caller has already established. |

## Public interfaces expected

The protocol module owns external-document parsing and identity verification. A
verified submission is private and is consumed by the operation that creates it.

```rust
// protocol.rs

#[derive(Debug, thiserror::Error)]
pub enum SubmitDocumentError {
    #[error(transparent)]
    Document(#[from] AnswersDocumentError),

    #[error(transparent)]
    Evaluation(#[from] EvalError),
}

#[derive(Debug, thiserror::Error)]
pub enum AnswersDocumentError {
    #[error("invalid JSON: {message}")]
    Json { message: String },

    #[error("invalid answers document: {message}")]
    Shape { message: String },

    #[error("answers document has no template identity")]
    MissingIdentity,

    #[error("answers document template must be a non-empty string")]
    MalformedIdentity,

    #[error(
        "answers template {declared:?} does not match expected template {expected:?}"
    )]
    TemplateMismatch {
        declared: String,
        expected: String,
    },
}

pub enum DocumentStep<'a> {
    Accepted {
        interview: Interview<'a>,
        submission: IndexMap<String, serde_json::Value>,
    },
    Rejected {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
    },
}

pub fn answer_document_once<'a>(
    expected_template: &str,
    pending: Pending<'a>,
    text: &str,
) -> Result<DocumentStep<'a>, SubmitDocumentError>;

pub fn answer_document_headless<'a>(
    expected_template: &str,
    template: &'a Template,
    interview: Interview<'a>,
    text: &str,
) -> Result<Headless<'a>, SubmitDocumentError>;
```

Both operations call one private boundary:

```rust
struct WireAnswersDocument {
    template: String,
    answers: serde_json::Map<String, serde_json::Value>,
}

struct VerifiedSubmission {
    raw: RawAnswers,
    stored: IndexMap<String, serde_json::Value>,
}

fn parse_and_verify(
    expected_template: &str,
    text: &str,
) -> Result<VerifiedSubmission, AnswersDocumentError>;
```

`VerifiedSubmission` is not public, has no unchecked constructor, and cannot be
converted to `RawAnswers` by a caller. `continue PATH FILE` uses
`answer_document_once`, which calls `Pending::answer` at most once. The scripted
route uses `answer_document_headless`, which drives the multi-batch headless walk.

The pure in-memory interfaces remain supported:

```rust
Pending::answer(RawAnswers)
protocol::answer_headless(&Template, Interview, RawAnswers)
```

Terminal prompting, staged replay, and crate callers that construct values in
memory use those interfaces. The public `parse_answers` external-JSON parser is
replaced, because returning `RawAnswers` from external JSON would erase the
required identity before submission.

The protocol module also owns the result documents:

```rust
pub fn batch_document(
    batch: &Batch,
    context: &Context,
    errors: Option<&Rejections>,
) -> Value;
pub fn ended_document(ended: &Ended, context: &Context) -> Value;
pub fn applied_document(report: &ApplyReport, context: &Context) -> Value;
pub fn planned_document(plan: &Plan, context: &Context, trust: TrustState) -> Value;
pub fn error_document(error: &ResultError, context: Option<&Context>) -> Value;
```

`complete_document` is removed. No command route emits a `complete` status.

The binary owns file, standard-input, and presentation I/O:

```rust
fn read_answers_text(path: &str) -> Result<String, String>;
fn instructions(outcome: &AgentOutcome, path: &Path) -> String;
```

`read_answers_text` reads once and adds the source name to diagnostics. It does
not inspect the JSON. `instructions` builds text from parsed command values and
the outcome; it does not parse its own JSON.

## Result documents

Every document carries `protocol: 1`, a `status`, and `context` when the route
has established a target and template.

| Status | Route | Contents | Exit |
|---|---|---|---|
| `questions` | Scripted, agent | `schema`, `messages`, `errors` | 4 |
| `ended` | Scripted | Flow `kind` (`stop` or `abort`), `messages`, optional `label` | 0 |
| `applied` | Scripted | Each written path with its action, messages, and hook results | 0 |
| `planned` | Scripted | Each planned path with its action, messages, planned hooks, and `trusted` | 0, or 3 when hooks are untrusted |
| `error` | Scripted | `kind`, `message`, optional `commands` | 1, or 5 for an ambiguous template |

Error `kind` values are `input`, `document`, `identity`, `staged`, `source`,
`replay`, `conflict`, `render`, `hook`, and `ambiguous`. An `ambiguous` error
lists the matches in `commands`. A `staged` refusal lists the commands that
finish or discard the staged interview.

On the scripted route, a required question without an answer is an error of the
document, so `errors` holds `is required` for it. On agent routes, `errors`
never holds `is required` for a question the caller has not yet been asked.

## Module and seam

```text
command TEMPLATE ─► cli::resolve ─► ResolvedTemplate.formal_name ──┐
                                                                    │ expected
target PATH ─► canonical_target ─► Store::load ─► StagedRecord.template ┤ formal
                                                                    │ name
answers file/stdin ─► read once ─► protocol::parse_and_verify ◄─────┘
                                         │ private VerifiedSubmission
                      ┌──────────────────┴───────────────────┐
                      ▼                                      ▼
          answer_document_once                   answer_document_headless
          (continue PATH FILE)                   (apply TEMPLATE PATH --answers)
                      │                                      │
               Pending::answer                     existing headless walk
                      │                                      │
          staged-effect policy                     no staged effect
                      │                                      │
     batch JSON + instructions             one JSON result document
```

- source and registry resolution own formal-name production;
- staging owns the saved identity and accepted replay submissions;
- protocol owns the external envelope, the identity gate, and result documents;
- interview owns answer validation and its atomic transaction;
- the CLI owns route selection, file I/O, persistence, instructions, planning,
  target effects, and exit codes;
- `staging::canonical_target` remains the sole target constructor;
- origin-bearing `Resolution` remains the sole configured-value carrier.

The accepted Jinja-context contract restores invocation context inside start and
each public staged replay route from the same producer-created
`CanonicalTarget`. Document submission begins after that start or replay result.
It does not flatten `Resolution`, create context independently, or add a second
target carrier.

## Route contract

| Route | Terminal check | Document | Staged effect | Output |
|---|---|---|---|---|
| `apply T P --answers F` | None | Headless | None; refuses if staged | Result document |
| `stage T P --async [F]` | None | None | Creates record | Batch JSON + instructions |
| `continue P F` | None | One step | Saves accepted submission | Batch JSON + instructions, or instructions |
| `apply P` | None | None | Removes record after a successful write | Files; or batch JSON + instructions |
| `stage T P` | stdin | None | Saves each batch | Prompts, plan, instructions |
| `continue P` | stdin while questions remain | None | Saves each batch | Prompts, plan, instructions; or instructions |
| `apply T P` | stdin while questions remain | None | Saves each batch; removes after a successful write | Prompts, files |
| Crate document operation | Caller | One step or headless | Caller | Engine result |
| Crate in-memory | Caller | None | Caller | Existing pure APIs |

`apply PATH` no longer prompts. The standard-output terminal check is removed
from every route.

A complete staged interview needs no terminal. `continue PATH` on a complete
interview writes the completion instructions; exit 0. `continue PATH FILE` on a
complete interview refuses before reading the document and names
`apply PATH --dry-run` and `apply PATH`.

## Validation and effect order

### Scripted route

1. Construct one `CanonicalTarget` and check for a staged record.
2. Refuse with a `staged` error document when a record exists. The document is
   not read.
3. Resolve the command template, load it, produce the origin-bearing configured
   `Resolution`, and start the interview.
4. Read the document once as UTF-8.
5. Parse JSON and require an object with only `template` and `answers`.
6. Require a non-empty string identity and an answers object.
7. Compare the declaration with `ResolvedTemplate.formal_name`.
8. Parse answer identifiers and drive the headless walk.
9. Report `questions` or `ended`, or build the plan for a completed result.
10. Report `planned` for CLI or flow dry-run, or apply and report `applied`.

### `continue PATH FILE`

1. Construct one `CanonicalTarget` and load the staged record; refuse when none
   exists.
2. Resume the stored immutable source, load the template, produce the configured
   `Resolution`, and replay accepted submissions.
3. Refuse a supplied document when replay is complete or ended. The document is
   not read.
4. Steps 4–6 of the scripted route.
5. Compare the declaration with `StagedRecord.template`.
6. Parse answer identifiers and call `Pending::answer` once.
7. Save an accepted submission, or remove the record for an accepted abort.
8. Write the result and instructions.

Route preparation keeps its existing diagnostics. On the scripted route, those
diagnostics are carried in the result document or on standard error and never
precede the JSON on standard output. Identity failure emits no answer-derived
warning, message, question result, or flow result.

Source resolution can populate the existing source cache before a document is
read. An identity failure does not create, change, migrate, or remove staged
state, build a plan, change target files, or run a hook.

### Document failure priority

1. Invalid JSON.
2. Top-level value is not an object.
3. A legacy bare answer map: migration diagnostic.
4. Missing or additional envelope fields.
5. Missing, non-string, or empty `template`.
6. Non-object `answers`.
7. Template mismatch.
8. Invalid question identifier.
9. Engine question, kind, constraint, expression, and flow results.

A malformed envelope wins over a mismatch. A mismatch wins over every nested
question or value fault.

## Persistence and accepted flow results

The external envelope is never stored. `StagedRecord.submissions` contains only
accepted inner answer maps; the record owns template identity.

| Result after identity succeeds | Scripted route | `continue PATH FILE` |
|---|---|---|
| Rejected answers | `questions` with errors; exit 4. | Same batch with errors; nothing saved; exit 4. |
| Pending | `questions` with `is required` errors; exit 4. | Save; next batch; exit 4. |
| `Completed(Proceed)` | Plan, apply, `applied`; exit 0. | Save; completion instructions; exit 0. |
| `Completed(DryRun)` | Plan, `planned`; exit 0. | Save; completion instructions; exit 0. |
| `Ended(Stop)` | `ended`; exit 0. | Save; retain record; stop notice and `abort` instruction; exit 0. |
| `Ended(Abort)` | `ended`; exit 0. | Remove record; abort notice; exit 0. |

Skip remains inside the interview walk. A `confirm` value remains an ordinary
boolean answer and can affect a later flow `when` expression. Recovery handles
the resulting engine state; it does not reinterpret confirm values as actions.

CLI `--dry-run` changes neither staged state nor target state, including
suppressing an abort removal.

A person route that stages a record keeps the accepted flow meanings: stop
retains the record and names `abort PATH`; abort removes it.

## Errors and exit behavior

| Condition | Scripted route | Agent and person routes |
|---|---|---|
| File, standard-input, or UTF-8 failure | `error` `input`; exit 1. | Source-prefixed error; exit 1. |
| Invalid JSON or envelope | `error` `document`; exit 1. | Same message on standard error; exit 1. |
| Bare map or missing identity | `error` `identity` with wrapper and expected identity; exit 1. | Same message; exit 1. |
| Empty or non-string identity | `error` `identity`; exit 1. | Same message; exit 1. |
| Identity differs | `error` `identity` naming declared and expected; exit 1. | Same message; exit 1. |
| Interview staged at path | `error` `staged` with commands; exit 1. | Not applicable. |
| Nothing staged | Not applicable. | Refusal naming `stage` and `apply TEMPLATE PATH`; exit 1. |
| No terminal on a prompting route | Not applicable. | Refusal naming the agent route; exit 1. |
| Ambiguous template | `error` `ambiguous` with matches; exit 5. | Existing ambiguity result; exit 5. |
| Source or replay failure | `error` `source` or `replay`; exit 1. | Existing attributed error; exit 1. |
| Trust required | `planned` with `trusted: false`; exit 3. | Existing trust result; exit 3. |
| Conflict, render, or hook failure | `error` of that kind; exit 1. | Existing error; exit 1. |

## Original problem reassessment

The original report came from agent-driven acceptance testing of 0.1.0. A
partial `apply --answers` exited 4 with the next questions listed under `errors`
as `is required`, and the recovery commands appeared only on standard error.
The route mixed the one-shot scripted contract with staged recovery.

### Target-only apply with answers

Not needed. The agent route answers a staged interview with
`continue PATH FILE` and applies with `apply PATH`. `--answers` belongs to the
one-shot scripted route, which never uses staged state and refuses when an
interview is staged.

### Exit-4 presentation

Resolved by route. The scripted route reports a JSON result document where an
incomplete document is an error. Agent routes report the next batch as a normal
step, with instructions on standard output. Person routes prompt. No route
selects its output from terminal status.

### Matching-template named apply

Kept for people. `apply TEMPLATE PATH` resumes a staged interview for the same
template by prompting. With `--answers` the same command is the scripted route
and refuses when an interview is staged.

## Compatibility and documentation

Toha is before 1.0.0 and has no dependent users, so these breaking changes carry
no migration path beyond documentation:

- answers files require the two-field envelope; a file migrates by wrapping the
  map and copying `context.template`;
- `apply --answers` no longer stages and refuses when an interview is staged;
- `apply --answers` writes a JSON result document for every outcome;
- `apply PATH` no longer prompts;
- agent-route instructions move from standard error to standard output after
  the batch JSON;
- the `complete` result document is removed;
- `stage` and `continue PATH` show the dry-run plan at completion;
- a new `apply TEMPLATE PATH` saves staged state at each batch boundary.

```diff
- { "label": "Sample", "quantity": 4 }
+ {
+   "template": "forge:catalog/receipt@stable",
+   "answers": { "label": "Sample", "quantity": 4 }
+ }
```

Staged record bytes and replay submissions do not change. `RawAnswers`,
`Pending::answer`, and raw headless driving remain for in-memory callers.

The paired implementation updates:

- interview protocol prose and schema, including the answers envelope and the
  result documents;
- command specification, help text, and command examples, stating which caller
  uses which route;
- the embedded `toha` agent skill, so agents use the agent route;
- public crate API documentation and README examples;
- architecture ownership for protocol, interview, staging, and CLI;
- fixtures for every route and public document operation;
- user guidance for scripts, agents, and people.

## Behaviors to prove

1. A matching envelope works through the scripted route, `continue PATH FILE`,
   and both public document operations.
2. A different template fails even when all question identifiers and values are
   valid for the active template.
3. Missing, empty, non-string, and mismatched identities have distinct
   diagnostics with exit 1.
4. A bare map fails with wrapper guidance and never reaches `Pending::answer`.
5. Unknown top-level members and a non-object `answers` member fail as shape
   errors.
6. The declared identity causes no source parsing, registry lookup, fetching,
   canonicalization, trust evaluation, or configured-default selection.
7. The scripted route with an interview staged at the path refuses with a
   `staged` error document and leaves a deliberately unreadable document unread.
8. `continue PATH FILE` on a complete interview leaves an unreadable document
   unread.
9. A document mismatch wins over unknown question identifiers and invalid values.
10. Identity failure leaves staged and target bytes unchanged and invokes no
    store save or remove, plan builder, target writer, or hook runner.
11. The scripted route never calls store save or remove for any outcome.
12. Every scripted outcome writes exactly one schema-valid JSON document to
    standard output and nothing else, with the exit code in the result table.
13. Scripted missing answers report `questions` with `is required` errors.
14. `stage --async`, `continue PATH FILE`, and incomplete `apply PATH` write
    batch JSON followed by instructions, with no `is required` errors for
    questions not yet asked.
15. `stage --async FILE` writes the batch to FILE and only instructions to
    standard output.
16. `continue PATH FILE` completing the interview writes instructions only,
    naming `apply PATH --dry-run` and `apply PATH`; exit 0.
17. `continue PATH FILE` calls one answer transaction and saves at most one
    submission; rejected input saves nothing.
18. `apply PATH` on an incomplete interview never prompts, with or without a
    terminal on standard input and standard output.
19. `apply TEMPLATE PATH` on an incomplete same-template interview prompts for
    the remaining questions, then applies.
20. A new `apply TEMPLATE PATH` saves at each batch boundary; after an interrupt,
    `continue PATH` and `apply TEMPLATE PATH` both resume from the saved batch.
21. `stage` and `continue PATH` at completion show the dry-run plan and write
    nothing to the target.
22. An agent-staged interview completes through `continue PATH`, and a
    person-staged interview completes through `continue PATH FILE`.
23. Pending, proceed, flow dry-run, stop, abort, skip, and CLI preview keep their
    approved effects on both document routes.
24. Terminal prompting and staged replay still use `RawAnswers` directly, and an
    in-memory crate caller still compiles with `Pending::answer` and raw
    headless driving.
25. External JSON cannot produce or extract a public verified or raw capability;
    compile-fail checks protect that boundary.
26. The same document remains reusable for another target or commit of the same
    formal template identity.
27. Schema examples and full-path fixtures validate the envelope and every
    result document.

## Failure injections and sole-kill guards

- Replace exact identity comparison with `true`; the cross-template test fails.
- Move comparison after the engine call; the engine-call recorder fails.
- Resolve the declared identity; the resolver-zero-call test fails.
- Restore a raw JSON parser; public API compile-fail checks fail.
- Expose `VerifiedSubmission` or raw extraction; visibility checks fail.
- Use the headless operation for `continue`; the one-step assertion fails.
- Read before the staged refusal or complete-replay refusal; unreadable-input
  tests fail.
- Save staged state on the scripted route; store-zero-call tests fail.
- Restore the terminal check on `apply PATH`; the pseudo-terminal no-prompt test
  fails.
- Write instructions or warnings to scripted standard output; the single-document
  parse test fails.
- Move agent instructions back to standard error; the agent stdout test fails.
- Drop batch saving from a new `apply TEMPLATE PATH`; the interrupt-resume test
  fails.
- Add target or commit comparison; reuse tests fail.
- Apply a flow action, build a plan, or call a hook on mismatch; effect recorders
  fail.
- Store the envelope rather than its inner accepted map; record compatibility and
  replay fixtures fail.

## Out of scope

- binding documents to a target or commit;
- resolving identities declared by documents;
- adding identity to staged submissions or pure `RawAnswers`;
- an output-format option or a caller-kind flag on staged state;
- a keyboard shortcut to pause a prompting route;
- changing configured-default provenance, canonical target construction, trust,
  timeouts, permissions, dependency-version policy, or subprocess use;
- implementing runtime or canonical specification changes in this design task.

## Decisions

The caller-route model, the scripted JSON result contract, and the removal of
staged `--answers` recovery are decided. The required answers identity is
decided. The envelope details and this exact revision need approval.

## Consequential changes

Capabilities removed or narrowed:

- `apply --answers` no longer stages a partial interview.
- `apply --answers` on a staged interview is refused.
- `apply PATH` no longer prompts on an incomplete interview.
- The `complete` result document is removed.
- The public external-JSON-to-raw parser is replaced.

The design adds no permission or access rule, timeout, pinned-version check,
application subprocess, target binding, or commit binding.

## Implementation size and complexity

Expected implementation time is one high-cognition agent task, approximately one
to two working days including tests, documentation, and full repository gates.
Complexity is moderate: one protocol boundary, new result documents, and route
changes across `apply`, `stage`, and `continue`. Source identity, target
identity, configured origins, the interview engine, staged storage, and planning
keep their existing owners.

## Reviewer focus

1. No external document can yield raw answers before exact identity comparison.
2. The scripted route has no staged effect and writes exactly one JSON document.
3. No route selects output from terminal status; only prompting routes inspect
   standard input.
4. `continue PATH FILE` remains one step while the scripted route remains a
   headless walk.
5. Identity failure cannot reach staged, flow, plan, target, or hook effects.
