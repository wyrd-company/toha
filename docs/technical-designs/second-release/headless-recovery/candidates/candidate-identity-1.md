# Candidate design: identity-gated answers and staged recovery

## Usage (caller’s view)

### Matching answers document

A question batch supplies the formal template name in `context.template`:

```json
{
  "protocol": 1,
  "status": "questions",
  "context": {
    "target": "/tmp/output",
    "template": "forge:catalog/receipt@stable",
    "commit": "0123456789abcdef"
  },
  "schema": {
    "type": "object",
    "properties": {
      "label": {
        "type": "string",
        "title": "Display label"
      },
      "quantity": {
        "type": "string",
        "title": "Quantity"
      }
    }
  },
  "messages": []
}
```

The caller copies `context.template` into the answers document:

```json
{
  "protocol": 1,
  "template": "forge:catalog/receipt@stable",
  "answers": {
    "label": "Sample",
    "quantity": "4"
  }
}
```

It works for a new interview:

```console
$ toha apply --answers answers.json forge:catalog/receipt@stable ./output
```

It also works for the incomplete interview already staged at `./output`:

```console
$ toha apply --answers answers.json ./output
```

The target-only command obtains the expected identity from the staged record. The caller does not repeat the template operand.

### Cross-template document with matching question ids

This document has valid answers and the same question ids, but identifies another template:

```json
{
  "protocol": 1,
  "template": "forge:catalog/shipment@stable",
  "answers": {
    "label": "Sample",
    "quantity": "4"
  }
}
```

Using it for the receipt template fails before any answer is evaluated:

```console
$ toha apply --answers other.json forge:catalog/receipt@stable ./output
other.json: answers document template "forge:catalog/shipment@stable" does not match expected template "forge:catalog/receipt@stable"
$ echo $?
1
```

No staged submission, warning, message, flow action, plan, target file, or hook is produced.

### Missing identity and bare-map migration

The former bare-map shape is not accepted:

```json
{
  "label": "Sample",
  "quantity": "4"
}
```

```console
$ toha continue ./output old-answers.json
old-answers.json: bare answers maps are not supported; use {"protocol":1,"template":"forge:catalog/receipt@stable","answers":{...}}
$ echo $?
1
```

There is no compatibility fallback because accepting the map would bypass the required identity assertion.

### Named staged recovery

The command operand, staged record, and document must agree:

```console
$ toha apply --answers answers.json forge:catalog/receipt@stable ./output
```

The driver compares:

```text
command formal name == staged formal name == document formal name
```

A command/staged mismatch is reported before the document is read. A document/staged mismatch is reported before answers enter the interview engine.

### Terminal and machine output

When `apply` cannot prompt and standard output is a terminal, an incomplete interview produces a concise summary:

```console
$ toha apply ./output
Questions remain for forge:catalog/receipt@stable at /tmp/output:
  label — Display label
  quantity — Quantity
```

Recovery commands remain on standard error. The process exits with code `4`.

A pipe or redirect receives the canonical JSON batch:

```console
$ toha apply ./output | jq '.context.template'
"forge:catalog/receipt@stable"
```

These routes always retain JSON, including when standard output is a terminal:

- `stage --async`;
- `continue PATH FILE` and `continue PATH -`;
- `apply --answers FILE`;
- `apply --answers -`.

Only an implicit, non-scripted `apply` recovery with no answers document selects the terminal summary.

### Public crate callers

An external document crosses the identity gate:

```rust
use toha::protocol;

let document = protocol::parse_answers_document(json_text)?;
let verified = document.verify(&resolved.formal_name)?;

let outcome = protocol::answer_headless(
    &template,
    interview,
    verified,
)?;
```

A staged one-batch submission extracts `RawAnswers` only after verification:

```rust
let document = protocol::parse_answers_document(json_text)?;
let verified = document.verify(&staged_record.template)?;
let next = pending.answer(verified.into_raw_answers())?;
```

An in-memory caller has no external answers document and keeps the pure engine API:

```rust
use toha::{Id, Pending, RawAnswer, RawAnswers};

let mut raw = RawAnswers::new();
raw.insert(Id::parse("label")?, RawAnswer("Sample".into()));

let next = pending.answer(raw)?;
```

`RawAnswers` is an in-memory engine input. It is not an answers-file representation and carries no template identity.

## Problem

The current protocol parser validates a bare object and returns `RawAnswers`, permanently discarding document-level identity (`src/protocol.rs:170-195`). `RawAnswers` is also the intended in-memory input to the pure `Pending::answer` transaction (`src/interview.rs:33-36,1704-1713`). Changing that engine type would mix transport policy into the engine.

The command driver already has authoritative formal identities from `ResolvedTemplate.formal_name` and `StagedRecord.template`. Protocol output already publishes the same value as `context.template`. The design therefore needs an identity-bearing transport boundary that cannot release `RawAnswers` before comparison, while preserving staged replay, configured-default provenance, canonical targets, answer atomicity, and accepted flow outcomes.

## Exact external document

The only accepted external shape is:

```json
{
  "protocol": 1,
  "template": "<formal template name>",
  "answers": {
    "<question id>": "<JSON value>"
  }
}
```

Its schema is:

```yaml
answers-document:
  type: object
  additionalProperties: false
  required:
    - protocol
    - template
    - answers
  properties:
    protocol:
      const: 1
    template:
      description: Exact formal name copied from question context.
      type: string
      minLength: 1
    answers:
      $ref: "#/$defs/answers"
```

`protocol` versions the transport envelope. The former input had no protocol field, so the new envelope starts at protocol `1`, matching the existing emitted protocol.

There is no target or commit field. One matching document can be used at another target or against another resolved commit with the same formal template name. The required contract binds the document to a template identity only.

### Formal identity rules

The identity is the exact UTF-8 string emitted as `context.template`.

Comparison is Rust string equality:

```rust
declared_template == expected_formal_name
```

The comparison performs no:

- trimming or case folding;
- path canonicalization;
- `.git` removal;
- host-prefix expansion;
- alias or registry lookup;
- source fetch;
- trust lookup.

A non-string or empty identity is malformed. Formal names have multiple valid representations, including canonical filesystem paths and Git addresses, so no second grammar attempts to normalize them.

An unfamiliar non-empty declaration is syntactically valid transport data. If it differs from the established expected identity, it is a mismatch. The document declaration is never resolved.

## Shape

### Public types

```rust
// protocol.rs

/// A parsed external answers document.
///
/// Its fields are private. It cannot expose or submit its answers until the
/// declared template identity has been verified.
pub struct AnswersDocument {
    declared_template: String,
    answers: serde_json::Map<String, serde_json::Value>,
}

/// Answers released from an external document after identity and answer-key
/// validation.
///
/// This is not a serializable wire type.
pub struct VerifiedAnswers {
    answers: RawAnswers,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid JSON: {0}")]
    InvalidJson(serde_json::Error),

    #[error("bare answers maps are not supported")]
    BareAnswersMap,

    #[error("answers document is missing required template identity")]
    MissingIdentity,

    #[error("answers document template must be a non-empty string")]
    MalformedIdentity,

    #[error("answers document protocol must be 1")]
    UnsupportedProtocol,

    #[error(
        "answers document template {declared:?} does not match expected template {expected:?}"
    )]
    TemplateMismatch {
        declared: String,
        expected: String,
    },

    #[error("invalid answers document: {0}")]
    InvalidEnvelope(String),

    #[error("invalid question id {0:?}")]
    InvalidQuestionId(String),
}
```

`AnswersDocument` is an opaque domain boundary value, not a public Serde representation. `VerifiedAnswers` is an identity-gated capability to obtain in-memory `RawAnswers`.

Existing public engine types remain unchanged:

```rust
pub struct RawAnswer(pub serde_json::Value);
pub type RawAnswers = IndexMap<Id, RawAnswer>;

impl<'a> Pending<'a> {
    pub fn answer(
        self,
        incoming: RawAnswers,
    ) -> Result<Interview<'a>, AnswerError<'a>>;
}
```

### Private wire type

```rust
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAnswersDocument {
    protocol: u8,
    template: serde_json::Value,
    answers: serde_json::Value,
}
```

The implementation can use an equivalent `serde_json::Value` parser when stable validation priority requires inspecting missing fields before deserialization. The private wire type never leaves `protocol`.

### Public functions

```rust
pub fn parse_answers_document(
    text: &str,
) -> Result<AnswersDocument, ProtocolError>;

impl AnswersDocument {
    /// Compares the declaration with an already-established formal name.
    ///
    /// This method does not resolve either value.
    pub fn verify(
        self,
        expected_formal_name: &str,
    ) -> Result<VerifiedAnswers, ProtocolError>;
}

impl VerifiedAnswers {
    /// Releases the engine input after successful identity verification.
    pub fn into_raw_answers(self) -> RawAnswers;
}

pub fn answer_headless<'a>(
    template: &Template,
    interview: Interview<'a>,
    document: VerifiedAnswers,
) -> Result<Headless<'a>, EvalError>;
```

The former `parse_answers(&str) -> RawAnswers` API is replaced. Keeping it would preserve a public identity bypass.

### Binary boundary

```rust
fn read_verified_answers(
    path: &str,
    expected_formal_name: &str,
) -> Result<protocol::VerifiedAnswers, String>;
```

This helper:

1. reads the file or standard input once;
2. calls `parse_answers_document` once;
3. calls `verify` once;
4. adds the file or `stdin` prefix to any diagnostic.

It does not resolve the expected identity.

### Interface depth

The public protocol surface exposes two opaque states and one transition. It hides JSON shape, schema validation, migration recognition, identity diagnostics, identifier parsing, and conversion to `RawAnswers`.

The two states encode the security boundary: an `AnswersDocument` cannot be passed to `Pending::answer` or `answer_headless`. The caller must obtain `VerifiedAnswers`.

## Ownership and seams

```text
                     existing identity authorities
              ┌───────────────────┴────────────────────┐
              │                                        │
cli::resolve::ResolvedTemplate.formal_name     StagedRecord.template
              │                                        │
              └───────────────────┬────────────────────┘
                                  │ expected &str
                                  ▼
external UTF-8 JSON ──► protocol::AnswersDocument
                                  │ verify exact identity
                                  ▼
                         protocol::VerifiedAnswers
                           │                  │
             into_raw_answers               │ answer_headless
                           │                  │
                           ▼                  ▼
                    Pending::answer ─────► pure interview engine
                                                │
                            ┌───────────────────┼──────────────────┐
                            ▼                   ▼                  ▼
                         Asking              Complete             Ended
                            │                   │                  │
                     staged save          Plan::build       stop/abort policy
                                                │
                                         Plan::apply gate

StagedRecord.submissions ──► replay ──► Pending::answer(RawAnswers)
terminal prompt answers ──────────────► Pending::answer(RawAnswers)
in-memory crate answers ──────────────► Pending::answer(RawAnswers)
```

Ownership remains:

- `source` and registry resolution own formal-name production.
- `staging` owns saved formal identity and accepted replay submissions.
- `protocol` owns the external answers envelope, parsing, and identity gate.
- `interview` owns answer validation and the atomic transaction.
- the binary owns file I/O, route selection, terminal presentation, and exit codes.
- `canonical_target` remains the only target constructor.
- the configured-default resolver remains the only producer of origin-bearing `Resolution`.

## Route matrix

| Route | Expected identity | Document handling | Incomplete presentation |
|---|---|---|---|
| New `apply TEMPLATE PATH --answers FILE` | `ResolvedTemplate.formal_name` | Parse, compare, then headless transaction | JSON batch, exit `4` |
| Staged `apply TEMPLATE PATH --answers FILE` | `StagedRecord.template`; command formal name must match first | Parse after replay and unused-input check | JSON batch, exit `4` |
| Staged `apply PATH --answers FILE` | `StagedRecord.template` | Parse after replay and unused-input check | JSON batch, exit `4` |
| `continue PATH FILE` or `-` | `StagedRecord.template` | Parse, compare, then one `Pending::answer` transaction | JSON batch, exit `4` |
| `apply PATH` without a file | No external document | Terminal answers produce `RawAnswers` directly | Prompt, terminal summary, or JSON according to modality |
| `apply TEMPLATE PATH` without a file | No external document | Terminal answers produce `RawAnswers` directly | Same |
| `stage --async` | No external document | None | JSON batch regardless of terminal |
| Staged replay | Record already owns template identity | Stored accepted maps become `RawAnswers` | Normal replay result |
| Public crate document route | Formal name already established by caller | `AnswersDocument::verify` required | Caller selects presentation |
| Public crate in-memory route | No external document exists | Direct `Pending::answer(RawAnswers)` | Caller-owned |

`apply PATH --answers FILE` with no staged record fails before reading the file and names `apply TEMPLATE PATH --answers FILE`.

## Validation and input-read order

| Order | New apply | Staged apply / continue |
|---:|---|---|
| 1 | Construct the one `CanonicalTarget`; load staged-state presence | Construct the one `CanonicalTarget`; load the staged record |
| 2 | Resolve the command template to its authoritative formal name | For a named route, resolve the command assertion and compare it with the record |
| 3 | Load the template; create origin-bearing configured `Resolution`; start the interview | Resume the record’s immutable source; load the template; create `Resolution`; replay accepted submissions |
| 4 | If applicable, determine whether the interview can consume a document | Refuse a document for `Complete` or `Ended` replay without reading it |
| 5 | Read the document exactly once | Read the document exactly once |
| 6 | Parse JSON and outer envelope | Parse JSON and outer envelope |
| 7 | Validate `protocol`, required identity, identity type, and non-empty identity | Same |
| 8 | Compare the declaration with the authoritative formal name | Compare with `StagedRecord.template` |
| 9 | Validate the nested answer keys and construct `RawAnswers` | Same |
| 10 | Enter `answer_headless` / `Pending::answer` | Same |
| 11 | Handle flow outcome, persistence, planning, target writes, and hooks | Same |

Configured warnings are buffered while the document is pending verification. They are published only after successful identity verification. A missing or mismatched identity therefore does not publish a warning reached during route preparation.

Resolving an immutable source may populate the source cache. This is route preparation, not staged-interview or target mutation. The staged record, interview history, target, and hooks remain untouched.

### Validation priority inside `protocol`

1. Invalid JSON.
2. Top-level value is not an object.
3. Recognized former bare map: migration error.
4. Missing or invalid `protocol`.
5. Missing identity.
6. Non-string or empty identity.
7. Missing, non-object, or additional envelope fields.
8. Identity mismatch.
9. Invalid answer key.
10. Engine question, kind, constraint, expression, and flow behavior.

A mismatch therefore wins over question-id or answer-value faults. Cross-template contents are not evaluated.

## Errors and exit codes

| Condition | Classification | Exit |
|---|---|---:|
| File or stdin read failure | Input I/O error | `1` |
| Invalid JSON or envelope | Protocol boundary error | `1` |
| Bare map or missing identity | Migration/protocol boundary error | `1` |
| Malformed identity | Protocol boundary error | `1` |
| Command template cannot resolve | Existing resolution error | `1`, or `5` when ambiguous |
| Command/staged mismatch | Existing staged-route refusal | `1` |
| Document/expected mismatch | `ProtocolError::TemplateMismatch` | `1` |
| Unknown id, kind, or answer constraint rejection after identity succeeds | Existing question rejection | `4` |
| Template expression failure | Existing template fault | `1` |
| Questions remain | Existing pending result | `4` |
| Successful complete apply or dry run | Existing success | `0` |
| Untrusted hooks | Existing trust result | `3` |

Missing-identity diagnostics include the exact expected formal name and the replacement envelope. They do not silently rewrite the file.

## Persistence, flow, planning, and target effects

### Identity failure

A parse or identity failure produces no:

- accepted submission;
- staged save or removal;
- migration of a legacy staged key;
- warning or interview message;
- flow result;
- plan;
- target directory or file change;
- hook execution.

An existing staged record remains byte-for-byte unchanged.

### Accepted document

After identity verification, existing transaction rules apply:

- a rejected document publishes no partial answer and is not persisted;
- pending saves only accepted submissions;
- staged replay continues to replay raw accepted maps;
- complete staged apply can save the accepted history before planning;
- CLI `--dry-run` changes neither staged nor target state.

The external envelope is never copied into `StagedRecord.submissions`. The staged record already owns the formal template identity, so storing it with every submission would create a second synchronized identity.

### Accepted flow outcomes

Identity verification precedes the answer transaction. After verification:

- `Pending` persists only the accepted prefix under normal execution;
- `Completed` with `Proceed` builds and applies a plan;
- `Completed` with `DryRun` builds and displays a plan without target effects;
- `Ended::Stop` retains staged state and performs no planning;
- `Ended::Abort` removes staged state through `Store::remove(&CanonicalTarget)`;
- CLI `--dry-run` suppresses staged removal, saves, target writes, and hooks;
- skip remains an engine walk operation.

A mismatched document cannot trigger `stop`, `abort`, `dry-run`, or `skip`.

### Planning and targets

The answers document contains neither target nor commit. Template identity does not prove either value.

All target-sensitive work continues to consume the same `CanonicalTarget` produced by `staging::canonical_target`. Identity verification neither constructs nor normalizes a target.

`Plan::build` remains available only from a completed interview. `Plan::apply` remains the sole target-write and hook-execution site.

## Terminal presentation

The binary selects presentation from route intent and standard-output modality:

| Route | Standard output is a terminal | Pipe or redirect |
|---|---|---|
| `apply` without `--answers`, unable to prompt | Concise summary | JSON batch |
| `apply --answers` | JSON batch | JSON batch |
| `continue FILE` or `-` | JSON result | JSON result |
| `stage --async` | JSON batch | JSON batch |
| Interactive prompting | Prompts and human messages | Not selected |

The summary renderer belongs to the binary’s terminal adapter:

```rust
fn terminal_batch_summary(
    batch: &Batch,
    context: &Context,
    errors: Option<&Rejections>,
) -> String;
```

It prints:

1. messages reached in the batch;
2. template and canonical target;
3. question count;
4. one `id — title` line per prompt;
5. carried error sentences beneath their question.

It does not print JSON Schema details, defaults, options, or placeholder values. Scripts obtain those details from the unchanged JSON document.

Exit code `4` retains one meaning: questions remain. Presentation does not change status.

## Migration and compatibility

### External documents

All bare maps fail. Migration wraps the existing map:

```json
{
  "protocol": 1,
  "template": "<copy context.template here>",
  "answers": {
    "...": "..."
  }
}
```

There is no environment switch, compatibility flag, inference from filename, target lookup, or alias resolution.

Fixtures, examples, schemas, guides, and generated snippets use the envelope.

### Public crate API

External-document callers replace:

```rust
let raw = protocol::parse_answers(text)?;
protocol::answer_headless(&template, interview, raw)?;
```

with:

```rust
let document = protocol::parse_answers_document(text)?;
let verified = document.verify(&formal_name)?;
protocol::answer_headless(&template, interview, verified)?;
```

Callers that construct `RawAnswers` in memory retain `Pending::answer` unchanged.

### Staged state

Existing staged-record JSON remains readable. Accepted submissions remain bare internal maps because they are subordinate to one identity-bearing `StagedRecord`.

No staged-record migration is required for answers identity.

## Reassessment of the original questions

### 1. Does `apply PATH --answers FILE` safely resume and apply an incomplete staged interview?

Yes.

The target selects one staged record. That record establishes the expected formal template identity and immutable source. The document independently asserts the same identity. Only a match releases its answers.

With no staged record, the command cannot establish an expected template and fails before reading the document.

### 2. Should exit-4 question output become a concise summary on a terminal while pipes, redirects, and `stage --async` retain JSON?

Yes.

A human looking at a terminal needs the remaining question names and recovery guidance. Machine consumers need the stable protocol document. Route intent prevents `--answers`, `continue FILE`, and `stage --async` from changing representation merely because they inherited a terminal.

### 3. Should `apply TEMPLATE PATH --answers FILE` resume when its template matches staged state?

Yes.

It is an explicit three-way assertion:

```text
resolved command template
        ==
staged-record template
        ==
answers-document template
```

The command/staged comparison occurs before the document is read. The document comparison occurs before answer evaluation.

## Falsifiable behaviors

1. A matching envelope produces the same accepted answers, flow outcome, plan, and output tree as the same in-memory `RawAnswers`.
2. Two templates with identical question ids reject each other’s documents.
3. A bare map fails with exit `1` and migration guidance containing the expected formal name.
4. A document cannot use an alias, alternate path spelling, or equivalent Git spelling in place of the emitted formal name.
5. Named staged apply checks the command/staged identity before opening the answers file.
6. Complete or ended staged replay refuses an unused answers path without opening it.
7. Missing or mismatched identity leaves staged bytes and the target tree unchanged.
8. Missing or mismatched identity publishes no configured warning, interview message, or flow result.
9. Question validation begins only after identity succeeds.
10. Rejected questions retain exit `4`; identity failures use exit `1`.
11. Target-only staged apply and named staged apply produce equal results from the same envelope.
12. `continue`, new apply, staged apply, and public crate document routes use the same protocol parser and identity comparison.
13. Terminal implicit recovery emits a summary on a TTY and byte-valid protocol JSON through a pipe.
14. `stage --async`, `continue FILE`, and `apply --answers` emit JSON on a TTY.
15. Existing staged submissions replay without gaining document identities.
16. In-memory `Pending::answer(RawAnswers)` remains source-compatible.
17. Documents without target or commit fields remain accepted; documents adding unknown top-level fields fail the closed envelope.
18. No document identity is passed to `source::parse`, registry resolution, or source fetching.

## Failure injection

| Injection | Expected result |
|---|---|
| File disappears after route preparation | Read error, exit `1`, no staged or target mutation |
| Truncated JSON | Protocol error, exit `1`, no engine call |
| Missing `template` | Migration error, exit `1`, no engine call |
| Empty or non-string `template` | Malformed identity, exit `1` |
| Cross-template identity with valid answers | Mismatch, exit `1`, no question validation |
| Matching identity with invalid question id | Protocol answer-key error after identity verification |
| Matching identity with rejected value | Batch with errors, exit `4`, no accepted submission |
| Configured warning exists and identity mismatches | Identity error only; warning remains unpublished |
| Save fails after an accepted pending result | Staging error; target and hooks remain untouched |
| Plan construction fails after completion | Existing planning error; no apply |
| Hook trust fails | Existing exit `3`; identity and answer acceptance do not alter trust |
| `flow: abort` follows a matching answer | Existing abort removal through the canonical target |
| `flow: abort` would follow a mismatching document | No flow evaluation and no removal |
| Standard output TTY probe changes | Presentation changes only; status and batch contents do not |

## Sole-kill guards

These checks each fail when one load-bearing rule is removed:

- Delete the identity comparison: the cross-template/same-ids test writes output.
- Reverse the comparison: the matching-document test fails.
- Return `RawAnswers` from `parse_answers_document`: a compile-fail test can pass an unverified document to the engine.
- Make `AnswersDocument.answers` public: a privacy compile-fail test begins compiling.
- Resolve the declaration as an alias: a resolver-spy test observes a call and the alternate-spelling rejection changes.
- Restore the target-only refusal: the staged `apply PATH --answers FILE` equivalence test fails.
- Read before command/staged comparison: a missing or unreadable answers path masks the staged-template mismatch.
- Read before terminal-state replay: a poison input is opened for a complete interview.
- Publish configured warnings before verification: the mismatched-document stderr assertion fails.
- Save the envelope instead of accepted raw submissions: existing staged-record compatibility and replay fixtures fail.
- Add target or commit requirements: the minimal-envelope schema fixture fails.
- Select terminal summary solely from `stdout.is_terminal()`: scripted TTY tests stop emitting JSON.
- Construct a second target from the document route: compile-time signature checks or canonical-target identity assertions fail.

## Design-red-flag screening

| Red flag | Screening result |
|---|---|
| Shallow module | `protocol` hides parsing, schema, migration detection, comparison, diagnostics, id parsing, and conversion behind one state transition. |
| Information leakage | The wire struct is private. The engine sees only `RawAnswers`; source and staging retain formal-name ownership. |
| Temporal decomposition | Parsing and verification stay together because they own the external-document invariant. They are not split into generic load/validate modules. |
| Pass-through method | `verify` changes the available type and performs identity plus key validation; it is not forwarding. |
| Second identity normalizer | None. Equality uses an already-produced formal name. |
| Second target normalizer | None. All target work continues through `CanonicalTarget`. |
| Origin loss | The document boundary does not alter configured `Resolution`. |
| Unsupported restriction | The document is not bound to a target or commit. |
| New permission, timeout, version check, or subprocess | None. `protocol: 1` is envelope schema validation, not a runtime dependency-version check. |
| Long call chain | A route reaches the engine through binary I/O → `protocol` → `Pending::answer`. |

## Tradeoffs accepted

- We accept a breaking answers-file migration in exchange for mandatory template identity.
- We accept exact-string rejection of equivalent alternate spellings in exchange for one formal-name authority and effect-free comparison.
- We accept a three-field envelope in exchange for unambiguous metadata, answer-key namespace, and protocol evolution.
- We accept opaque parse and verified states in exchange for making premature `RawAnswers` access difficult.
- We accept portability across targets and commits in exchange for implementing only the approved template binding.
- We accept separate terminal and JSON renderers in exchange for stable scripted output and concise human recovery.

## Alternatives considered

### Put `template` beside question ids

```json
{
  "template": "forge:catalog/receipt@stable",
  "label": "Sample"
}
```

This competes with a valid question id named `template` and forces every answer consumer to know which keys are metadata. It exposes more protocol policy to callers and loses to the nested `answers` namespace.

### Reuse the output `context` object

Requiring `context.target` and `context.commit` would bind documents to a run rather than the required template. Making those fields optional would create multiple envelope forms. Both shapes have weaker interface depth than one closed minimal envelope.

### Normalize or resolve the declared identity

Resolving aliases or alternate addresses would duplicate source/registry policy and could fetch from an untrusted declaration. Exact comparison is the only viable shape that preserves the existing formal-name authority.

### Keep `parse_answers -> RawAnswers` and check only in the CLI

This leaves public crate and future file consumers able to bypass identity. It hides little and distributes the invariant across drivers.

### Add identity to `RawAnswers`

This moves a transport requirement into terminal prompts, staged replay, and in-memory crate calls. It changes the pure transaction for callers that have no document.

## Open questions and risks

- Can every maintained script obtain `context.template` from its preceding batch? Migration fixtures must cover scripts that currently keep only `schema`.
- Does the terminal summary retain enough information when replay exposes a carried error? The terminal fixture must include a carried held-answer rejection.
- Can bare-map recognition avoid misclassifying an intended but malformed envelope? Recognition must affect guidance only; both cases remain rejected.
- Will source preparation emit diagnostics before identity verification? Route tests must capture stdout and stderr and require identity failures to be the sole published result.

## Synthesis decision

Reserved for arena synthesis. This candidate recommends the closed `protocol`/`template`/`answers` envelope and opaque `AnswersDocument → VerifiedAnswers → RawAnswers` transition as the base shape.

## Next implementation step

Update the protocol specification and embedded schema, then implement the opaque parse/verify types and their cross-template, bare-map, and compile-fail tests before changing command routes.