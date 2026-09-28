---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Identity-bearing answers and staged recovery

## Purpose

Every external answers document identifies the template it answers. Toha compares
that declaration with the formal template identity already established by the
active route before it evaluates any answer. A document authored for another
template cannot be accepted because question identifiers happen to match.

An incomplete staged interview remains recoverable from its target. A caller can
also repeat the template operand as an assertion. Both recovery spellings use the
same answer transaction and staged-effect policy. Question results stay JSON for
machine consumers and can become short recovery instructions on a terminal.

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

The envelope has no protocol-version member. The required identity change does
not add a pinned version check. It has no target or commit member. A matching
document can answer another target or another resolved commit of the same formal
template identity.

The bare map is invalid because it contains no template assertion:

```json
{
  "label": "Sample",
  "quantity": 4
}
```

The diagnostic shows the required wrapper and the expected formal identity. Toha
does not silently infer the template from the active interview.

## Template identity authority

`ResolvedTemplate.formal_name` and `StagedRecord.template` are the existing
identity authorities. Protocol question output already publishes that value as
`context.template`. A producer copies it without reconstructing a source name.

The declared `template` is compared to the route's already-established formal
name with exact string equality. Toha does not trim, case-fold, canonicalize a
path, remove `.git`, expand a host prefix, resolve an alias, inspect a registry,
fetch a source, or run a trust check on the declared value.

A non-empty declared string is therefore never independently “unresolved.” It
matches the expected formal name or it is a mismatch. Source-resolution errors
belong to route preparation and occur before document comparison.

## Caller usage

### New headless apply

A question result gives the value to copy:

```json
{
  "protocol": 1,
  "status": "questions",
  "context": {
    "target": "/tmp/output",
    "template": "forge:catalog/receipt@stable",
    "commit": "0123456789abcdef"
  },
  "schema": {},
  "messages": []
}
```

The next document uses only the formal template identity and answers:

```console
$ toha apply --answers answers.json forge:catalog/receipt@stable ./output
create README.txt
create settings.toml
```

### Resume by target

```console
$ toha apply --answers answers.json ./output
create README.txt
create settings.toml
```

The target selects one staged record. The stored record supplies the expected
formal identity and immutable source. The document must assert the same identity.
The caller does not repeat a template operand.

Without a staged record, the command fails before it reads the document and
names the template-taking form.

### Assert the template while resuming

```console
$ toha apply --answers answers.json forge:catalog/receipt@stable ./output
```

The command identity, staged identity, and document identity must all agree:

```text
resolved command formal name
             =
staged record formal name
             =
document template
```

The command/staged comparison happens before document read. Recovery loads the
stored source and commit; it does not substitute a newly resolved mutable source.

### Reject a document for another template

```json
{
  "template": "forge:catalog/shipment@stable",
  "answers": {
    "label": "Sample",
    "quantity": 4
  }
}
```

```console
$ toha apply --answers other.json forge:catalog/receipt@stable ./output
other.json: answers template "forge:catalog/shipment@stable" does not match expected template "forge:catalog/receipt@stable"
$ echo $?
1
```

The answer identifiers and values are not evaluated. The staged record and
target remain unchanged.

## Public interfaces expected

The protocol module owns external-document parsing and identity verification.
A verified submission is private and is consumed by the same operation that
creates it.

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
converted to `RawAnswers` by a caller. `answer_document_once` preserves the
one-`Pending::answer` meaning of `continue`. `answer_document_headless` preserves
the apply route's multi-batch walk.

The existing pure in-memory interfaces remain supported:

```rust
Pending::answer(RawAnswers)
protocol::answer_headless(&Template, Interview, RawAnswers)
```

Terminal prompting, staged replay, and crate callers that construct values in
memory use those interfaces. `RawAnswers` is not an answers-document type.
The public raw document parser is replaced because returning `RawAnswers` from
external JSON would erase the required identity before submission.

The binary owns file and standard-input I/O:

```rust
fn read_answers_text(path: &str) -> Result<String, String>;
```

It reads once, passes text and the established formal name to one protocol
operation, and adds the source name to diagnostics. It does not inspect the JSON.

## Module and seam

```text
command TEMPLATE ─► cli::resolve ─► ResolvedTemplate.formal_name
                                             │
target PATH ─► canonical_target ─► StagedRecord.template
                                             │ expected formal name
                                             ▼
answers file/stdin ─► read once ─► protocol::parse_and_verify
                                             │ private VerifiedSubmission
                           ┌─────────────────┴──────────────────┐
                           ▼                                    ▼
                 answer_document_once                answer_document_headless
                           │                                    │
                    Pending::answer                    existing headless walk
                           └─────────────────┬──────────────────┘
                                             ▼
                         transition + accepted submissions
                                             │
                                    staged-effect policy
                                             │
                    questions / completed / stop / abort / plan
```

- source and registry resolution own formal-name production;
- staging owns the saved identity and accepted replay submissions;
- protocol owns the external envelope and identity gate;
- interview owns answer validation and its atomic transaction;
- the CLI owns route selection, file I/O, persistence, presentation, planning,
  target effects, and exit codes;
- `staging::canonical_target` remains the sole target constructor;
- origin-bearing `Resolution` remains the sole configured-value carrier.

## Route contract

| Route | Expected identity | Document operation | Result |
|---|---|---|---|
| New `apply TEMPLATE PATH --answers FILE` | Resolved command formal name | Headless | Start, answer, stage if pending, or complete. |
| Staged `apply PATH --answers FILE` | Staged formal name | Headless | Resume, answer, stage if pending, or complete. |
| Staged `apply TEMPLATE PATH --answers FILE` | Command must first match staged name; staged name gates document | Headless | Same recovery path. |
| `continue PATH FILE` or `-` | Staged formal name | One step | Commit at most one accepted submission and emit next result. |
| Terminal prompting | Active interview | None | Build `RawAnswers` in memory. |
| Staged replay | Stored record | None | Replay stored accepted maps. |
| Public crate document route | Formal name already established by caller | One step or headless | Same engine results as CLI route. |
| Public crate in-memory route | No document | None | Existing pure APIs. |

`apply PATH --answers FILE` without a staged record fails before document read.
A complete or ended replay refuses an unused document before document read.

## Validation and effect order

### Staged routes

1. Construct one `CanonicalTarget` and load the staged record.
2. Resolve and compare a command template assertion when present.
3. Resume the stored immutable source, load the template, produce the
   origin-bearing configured `Resolution`, and replay accepted submissions.
4. Refuse a supplied document if replay is complete or ended.
5. Read the document once as UTF-8.
6. Parse JSON and require an object with only `template` and `answers`.
7. Require a non-empty string identity and an answers object.
8. Compare the declaration with `StagedRecord.template`.
9. Parse answer identifiers and enter the appropriate one-step or headless
   engine operation.
10. Commit accepted submissions or an accepted flow effect.
11. Build and apply a plan only for a completed proceed result.

### New apply

The route constructs the canonical target and checks staged presence first. It
then resolves and loads the command template, creates the configured
`Resolution`, and starts the interview before steps 5–11.

Route preparation keeps its existing diagnostics. Configured-value warnings can
therefore appear before a document identity error. Identity failure itself emits
no answer-derived warning, message, question result, or flow result.

Source resolution can populate the existing source cache before a document is
read. This is existing route preparation. An identity failure does not create,
change, migrate, or remove staged state; build a plan; change target files; or
run a hook.

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

## Errors and exit behavior

| Condition | Result |
|---|---|
| File, standard-input, or UTF-8 failure | Source-prefixed input error; exit 1. |
| Invalid JSON or envelope | Source-prefixed document error; exit 1. |
| Bare map or missing identity | Migration error with exact wrapper and expected identity; exit 1. |
| Empty or non-string identity | Malformed-identity error; exit 1. |
| Document identity differs | Mismatch names declared and expected values; exit 1. |
| Command template is ambiguous | Existing ambiguity result; exit 5 before document read. |
| Source cannot resolve or replay fails | Existing attributed error; exit 1 before document read. |
| Identity matches and answers are rejected | Existing question result; exit 4. |
| Identity matches and questions remain | Existing pending result; exit 4. |
| Accepted completion, stop, abort, or dry-run | Accepted flow result and existing exit contract. |
| Trust is required | Existing trust result; exit 3. |

## Persistence and accepted flow results

The external envelope is never stored. `StagedRecord.submissions` continues to
contain only accepted inner answer maps; the record already owns template
identity.

| Result after identity succeeds | Normal staged effect | Plan and target effect |
|---|---|---|
| Rejected answer transaction | None. | None. |
| Pending | Save accepted prefix. | None. |
| `Completed(Proceed)` | Save accepted progress for an existing record. | Build and apply; remove after successful write. |
| `Completed(DryRun)` | Save accepted progress. | Build and show; no target write or hook. |
| `Ended(Stop)` | Save accepted progress and retain staged state. | None. |
| `Ended(Abort)` | Remove through `Store::remove(&CanonicalTarget)`. | None. |

Skip remains inside the interview walk. A `confirm` value remains an ordinary
boolean answer and can affect a later flow `when` expression. Recovery handles
the resulting engine state; it does not reinterpret confirm values as actions.

CLI `--dry-run` simulates the result and changes neither staged state nor target
state, including suppressing an abort removal. Normal execution applies the
accepted abort removal.

## Question-result presentation

The recommended terminal contract is:

| Producer | Terminal stdout | Non-terminal stdout |
|---|---|---|
| `stage --async` | JSON | JSON |
| `apply`, question result | Concise recovery summary | Existing JSON question document |
| `continue`, question result | Concise recovery summary | Existing JSON question document |

The summary and JSON are projections of the same `Batch`, `Context`, and
effective rejections. The summary lists the formal template, canonical target,
question identifiers and titles, per-question errors, and a next command made
from parsed command values. It does not parse its own JSON. Exit code 4 remains
unchanged.

This is a pending product decision. A script or agent retains JSON by piping or
redirecting stdout. `stage --async` remains an explicit protocol route and is
always JSON.

## Original problem reassessment

### Target-only apply with answers

The problem remains valid. The target selects one staged record and immutable
source. The document supplies an independent template assertion. Exact agreement
makes the target-only form safe without requiring a repeated template operand.
The recommendation is to support it.

### Terminal exit-4 presentation

The problem remains valid. Template identity changes input safety, not the need
for a person to see actionable remaining questions. The recommendation is a
summary for apply and continue question results on terminal stdout, with JSON
unchanged for non-terminal consumers and `stage --async`.

### Matching-template named apply

The problem remains valid. The template operand is useful as an explicit
assertion. Command, staged, and document identities must agree, and recovery uses
the staged immutable source. The recommendation is to preserve this route.

## Compatibility and documentation

The answers-file shape is intentionally breaking. Existing files migrate by
wrapping the map and copying `context.template`:

```diff
- { "label": "Sample", "quantity": 4 }
+ {
+   "template": "forge:catalog/receipt@stable",
+   "answers": { "label": "Sample", "quantity": 4 }
+ }
```

The public raw JSON parser is replaced by the two consuming document operations.
The in-memory `RawAnswers`, `Pending::answer`, and raw headless helper remain
supported. Staged record bytes and replay submissions do not change.

The paired implementation updates:

- interview protocol prose and schema;
- command specification and command examples;
- public crate API documentation and README examples;
- architecture ownership for protocol, interview, staging, and CLI;
- fixtures for every file route and public document operation;
- user guidance for producing, migrating, and recovering with answer files.

## Behaviors to prove

1. A matching envelope works through new apply, staged target-only apply, staged
   named apply, `continue`, and both public document operations.
2. A different template fails even when all question identifiers and values are
   valid for the active template.
3. Missing, empty, non-string, and mismatched identities have distinct exit-1
   diagnostics.
4. A bare map fails with wrapper guidance and never reaches `Pending::answer`.
5. Unknown top-level members and a non-object `answers` member fail as shape
   errors.
6. The declared identity causes no source parsing, registry lookup, fetching,
   canonicalization, trust evaluation, or configured-default selection.
7. Command/staged mismatch and complete replay leave a deliberately unreadable
   document unread.
8. A document mismatch wins over unknown question identifiers and invalid answer
   values.
9. Identity failure leaves existing staged bytes and target bytes unchanged and
   invokes no store save/remove, plan builder, target writer, or hook runner.
10. Identity failure emits no answer-derived warning, message, question result,
    or flow result; existing route-preparation warnings keep their order.
11. `continue` calls one answer transaction and commits at most one submission.
12. Apply retains the multi-batch headless walk and stores only accepted inner
    maps.
13. Rejected answers publish and persist no tentative prefix or flow effect.
14. Target-only and matching named recovery produce the same completed tree.
15. Named recovery uses the staged immutable source, not a fresh mutable source.
16. Pending, proceed, flow dry-run, stop, abort, skip, and CLI preview preserve
    their approved staged and target effects.
17. Terminal apply and continue question results use the selected presentation;
    redirected output is schema-valid JSON with exit 4.
18. `stage --async` is JSON on a terminal.
19. Terminal prompting and staged replay still use `RawAnswers` directly.
20. An in-memory crate caller still compiles with `Pending::answer` and the raw
    headless helper.
21. External JSON cannot produce or extract a public verified/raw capability;
    compile-fail checks protect that boundary.
22. The same document remains reusable for another target or commit of the same
    formal template identity.
23. Schema examples and full-path fixtures validate the exact two-field envelope.

## Failure injections and sole-kill guards

- Replace exact identity comparison with `true`; the cross-template test fails.
- Move comparison after the engine call; the engine-call recorder fails.
- Resolve the declared identity; the resolver-zero-call test fails.
- Restore a raw JSON parser; public API compile-fail checks fail.
- Expose `VerifiedSubmission` or raw extraction; visibility checks fail.
- Use the headless operation for `continue`; the one-step assertion fails.
- Read before command/staged comparison or complete replay; unreadable-input
  priority tests fail.
- Save or remove staged state before identity success; store-zero-call tests fail.
- Add target or commit comparison; reuse tests fail.
- Buffer or suppress configured warnings; warning-order tests fail.
- Apply a flow action, build a plan, or call a hook on mismatch; effect recorders
  fail.
- Store the envelope rather than its inner accepted map; record compatibility and
  replay fixtures fail.
- Remove terminal selection; terminal/non-terminal presentation tests fail.
- Apply terminal selection to `stage --async`; its JSON test fails.

## Out of scope

- binding documents to a target or commit;
- resolving identities declared by documents;
- adding identity to staged submissions or pure `RawAnswers`;
- adding an output-format flag;
- changing configured-default provenance, canonical target construction, trust,
  timeouts, permissions, dependency-version policy, or subprocess use;
- implementing runtime or canonical specification changes in this design task.

## Decisions needed first

The identity-bearing envelope and exact-comparison rule are part of this design;
the requirement that an answers file identify its template is already decided.
Three recovery decisions remain:

1. **1A — Resume and apply (recommended):** `apply PATH --answers FILE` uses the
   staged record, checks the document identity, saves accepted progress, and
   applies on completion. **1B:** keep the current refusal.
2. **2A — Terminal summary (recommended):** apply and continue question results
   use a concise summary when stdout is a terminal; non-terminal output and
   `stage --async` remain JSON. **2B:** always show JSON.
3. **3A — Preserve matching named recovery (recommended):**
   `apply TEMPLATE PATH --answers FILE` resumes when command, staged, and
   document identities agree. **3B:** refuse named recovery whenever staged state
   exists.

## Consequential changes

The required answers-file migration rejects bare maps and replaces the public raw
JSON parser. The pure in-memory answer and headless capabilities remain. Decision
2A changes terminal stdout for exit-4 apply and continue question results. The
design adds no permission or access rule, timeout, pinned-version check,
application subprocess, target binding, or commit binding.

## Implementation size and complexity

Expected implementation time is one high-cognition agent task, approximately one
working day including tests, documentation, and full repository gates. Complexity
is moderate: the change touches one protocol boundary and several command routes,
but it keeps source identity, target identity, configured origins, the interview
engine, staged storage, and planning ownership in their existing modules.

## Reviewer focus

Reviewers should trace four load-bearing properties:

1. no external document can yield raw answers before exact identity comparison;
2. `continue` remains one step while apply remains a headless walk;
3. identity failure cannot reach staged, flow, plan, target, or hook effects;
4. no code resolves document identity or adds target/commit binding.
