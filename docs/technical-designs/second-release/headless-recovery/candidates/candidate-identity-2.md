# Route-bound answers identity and staged recovery

## Usage (caller’s view)

### Matching answers document

Every external answers document uses one envelope:

```json
{
  "protocol": 1,
  "template": "/opt/templates/sample-a",
  "answers": {
    "label": "Sample",
    "quantity": 2
  }
}
```

The `template` value is copied from `context.template` in a question batch or result. It is the formal template name, not an alias, short name, target, or commit.

A new headless run accepts the document when the resolved template has that formal name:

```console
toha apply /opt/templates/sample-a /tmp/output --answers answers.json
```

An incomplete staged interview also accepts it without repeating the template:

```console
toha apply /tmp/output --answers answers.json
```

`continue` consumes the same document:

```console
toha continue /tmp/output answers.json
```

### Cross-template document with matching question ids

This document has valid answers for `sample-a`, but declares another template:

```json
{
  "protocol": 1,
  "template": "/opt/templates/sample-b",
  "answers": {
    "label": "Sample",
    "quantity": 2
  }
}
```

Using it for `sample-a` fails before either answer is evaluated:

```text
answers.json: answers template "/opt/templates/sample-b" does not match expected template "/opt/templates/sample-a"
```

The matching question ids do not affect the result.

### Missing identity

A retired bare map is not an answers document:

```json
{
  "label": "Sample",
  "quantity": 2
}
```

It fails with exit code 1:

```text
answers.json: answers document has no template identity; wrap the answers as {"protocol":1,"template":"<formal name>","answers":{...}} and copy the formal name from context.template
```

There is no automatic bare-map fallback.

### Public crate caller: external document

The protocol route binds one established formal name to one pending interview. Parsing, identity comparison, answer conversion, and headless driving are one operation.

```rust
use toha::Interview;
use toha::protocol::AnswersRoute;

let pending = match interview {
    Interview::Asking(pending) => pending,
    Interview::Complete(_) => return Err("answers document is unused".into()),
    Interview::Ended(_) => return Err("answers document is unused".into()),
};

let result = AnswersRoute::new(
    resolved.formal_name.as_str(),
    &template,
    pending,
)
.submit(&document_text)?;
```

`AnswersRoute` accepts JSON text. It does not accept `RawAnswers`.

### Public crate caller: in-memory answers

`RawAnswers` remains the pure engine input for answers constructed in memory:

```rust
use toha::{Interview, RawAnswer, RawAnswers};

let mut answers = RawAnswers::new();
answers.insert(label_id, RawAnswer("Sample".into()));

let next = match interview {
    Interview::Asking(pending) => pending.answer(answers)?,
    Interview::Complete(completed) => Interview::Complete(completed),
    Interview::Ended(ended) => Interview::Ended(ended),
};
```

`RawAnswers` is not an answers document. It has no envelope, template identity, protocol version, target, or commit.

## External contract

### JSON shape

The protocol schema gains one top-level document definition:

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
      type: string
      minLength: 1
      description: Formal name of the template this document answers.
    answers:
      type: object
      propertyNames:
        $ref: "#/$defs/identifier"
```

Answer values remain unrestricted JSON at the protocol boundary. The interview engine validates their kinds and constraints.

The envelope contains no target or commit. One document can answer another interview for the same formal template, including one at another target or resolved commit.

### Formal identity comparison

The route compares the decoded `template` string with its established formal name using exact string equality.

The declared value is data. The comparison performs no:

- alias or short-name resolution;
- source parsing or fetching;
- path canonicalization;
- host-shorthand rewriting;
- registry, trust, target, or commit lookup.

JSON escape decoding occurs during the single JSON parse. No later normalization occurs.

The expected identity comes from the existing authority for each route:

| Route | Expected formal identity |
|---|---|
| New `apply TEMPLATE PATH --answers FILE` | `ResolvedTemplate.formal_name` |
| Staged `apply PATH --answers FILE` | `StagedRecord.template` |
| Staged `apply TEMPLATE PATH --answers FILE` | `StagedRecord.template`, after the command operand resolves to the same name |
| `continue PATH FILE` | `StagedRecord.template` |
| Public crate document route | The formal name supplied when constructing `AnswersRoute` |

A declared alias, short name, alternate Git spelling, relative path, or noncanonical folder spelling is a mismatch even when it might resolve to the same source.

## Data types

### Public protocol surface

```rust
pub struct AnswersRoute<'a> {
    expected_template: &'a str,
    template: &'a Template,
    pending: Pending<'a>,
}

impl<'a> AnswersRoute<'a> {
    pub fn new(
        expected_template: &'a str,
        template: &'a Template,
        pending: Pending<'a>,
    ) -> Self;

    pub fn submit(
        self,
        text: &str,
    ) -> Result<Headless<'a>, SubmitDocumentError>;
}

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

    #[error("unsupported answers protocol {found}; expected protocol 1")]
    UnsupportedProtocol { found: serde_json::Value },
}

#[allow(clippy::large_enum_variant)]
pub enum Headless<'a> {
    Completed {
        completed: Completed,
        accepted: Vec<IndexMap<String, Value>>,
    },
    Pending {
        pending: Box<Pending<'a>>,
        rejections: Rejections,
        accepted: Vec<IndexMap<String, Value>>,
    },
    Ended {
        ended: Ended,
        accepted: Vec<IndexMap<String, Value>>,
    },
}
```

`Pending::answer(RawAnswers)` remains unchanged.

### Private protocol types

```rust
struct WireAnswersDocument {
    protocol: u64,
    declared_template: String,
    answers: serde_json::Map<String, serde_json::Value>,
}

/// Capability created only after schema validation and identity comparison.
struct VerifiedSubmission {
    answers: RawAnswers,
}

fn parse_and_verify(
    text: &str,
    expected_template: &str,
) -> Result<VerifiedSubmission, AnswersDocumentError>;

fn drive_verified<'a>(
    template: &'a Template,
    pending: Pending<'a>,
    submission: VerifiedSubmission,
) -> Result<Headless<'a>, EvalError>;
```

`VerifiedSubmission` is private, has no unchecked constructor, and exposes no conversion to callers. `drive_verified` is crate-private.

The existing public `protocol::parse_answers(text) -> RawAnswers` and public `answer_headless(..., RawAnswers)` are replaced. Keeping either would permit a file-driven caller to erase identity before submission.

### CLI boundary

```rust
fn read_answers_text(path: &str) -> Result<String, String>;

fn submit_answers<'a>(
    path: &str,
    expected_template: &'a str,
    template: &'a Template,
    pending: Pending<'a>,
) -> Result<Headless<'a>, String>;
```

`read_answers_text` performs the only file or standard-input read. `AnswersRoute::submit` performs the only JSON parse.

The CLI adds the source name to protocol errors. It does not deserialize or inspect the JSON itself.

## Module and seam diagram

```text
<TEMPLATE> argument ──► cli::resolve ──► established formal name
                                                │
staged target ─► CanonicalTarget ─► StagedRecord.template
                                                │
                                                ▼
answers file/stdin ─► main::read_answers_text ─► protocol::AnswersRoute
                                                    │
                          private wire envelope ─────┤ parse once
                          exact identity check ──────┤
                                                    ▼
                                           VerifiedSubmission
                                                    │
                                                    ▼
                                     interview::Pending::answer
                                         pure RawAnswers transaction
                                                    │
                     ┌──────────────────────────────┼─────────────────────┐
                     ▼                              ▼                     ▼
              Headless::Pending            Headless::Completed     Headless::Ended
                     │                              │                     │
              staged persistence              plan/apply          stop or abort
```

`protocol` owns the external envelope and identity gate. `interview` owns answer validation and transaction atomicity. `staging` owns replay and persistence. The binary owns file I/O, TTY presentation, and exit codes.

## Route matrix

| Route | External document | Identity gate | Result |
|---|---:|---|---|
| New `apply TEMPLATE PATH --answers FILE` | Yes | Document = resolved command template | Headless answer, stage if incomplete, otherwise plan/apply |
| Staged `apply PATH --answers FILE` | Yes | Document = staged template | Resume, answer, then apply or stage |
| Staged `apply TEMPLATE PATH --answers FILE` | Yes | Command = staged template, then document = staged template | Resume, answer, then apply or stage |
| `continue PATH FILE` | Yes | Document = staged template | Save accepted submission and emit next result |
| `continue PATH -` | Yes | Document = staged template | Same behavior using standard input |
| `apply PATH` | No | Staged identity only | Prompt, emit recovery output, or apply |
| `apply TEMPLATE PATH` | No | Command/staged comparison when state exists | Prompt or resume |
| `continue PATH` | No | Staged identity only | Prompt or emit completed result |
| `stage --async` | No | Resolved template identity enters output context | Always emit JSON |
| Terminal prompting | No | Active interview already owns its template | Build `RawAnswers` locally |
| Staged replay | No | Stored record and stored immutable source | Replay stored submissions as `RawAnswers` |
| Public crate `AnswersRoute` | Yes | Document = route’s established identity | Same `Headless` outcomes |
| Public crate `Pending::answer` | No | None; input is in-memory | Pure answer transaction |

## Validation and read ordering

### Common staged route

| Order | Operation | File read? | Mutation? |
|---:|---|---:|---:|
| 1 | Construct the sole `CanonicalTarget` | No | No |
| 2 | Load the staged record | No | No |
| 3 | Resolve a named template assertion, when present | No | No |
| 4 | Refuse command/staged identity mismatch | No | No |
| 5 | Resume the stored source and load the template | No | Source/cache behavior only |
| 6 | Resolve configured defaults and replay accepted submissions | No | No |
| 7 | Refuse an answers file when replay is complete or ended | No | No |
| 8 | Read the file or standard input once as UTF-8 | Yes | No |
| 9 | Parse and schema-check the envelope once | Yes | No |
| 10 | Compare declared and expected formal identities | Yes | No |
| 11 | Submit the private verified answers transaction | Yes | Pure engine state only |
| 12 | Commit accepted submissions or a flow effect | Yes | According to outcome |
| 13 | Build a plan and apply when permitted | Yes | Target effects begin here |

### New apply route

The new route constructs the canonical target and checks staged state first. When no staged record exists, it resolves and loads the command template, creates the configured `Resolution`, starts the interview, and then follows steps 7–13.

Configured-resolution warnings, replay messages, and other route diagnostics are buffered until the document identity succeeds. A missing or mismatched identity publishes none of them. The route still reads `Resolution::warnings()` before consuming the origin-bearing `Resolution`.

### Failure priority

| Concurrent condition | Reported result |
|---|---|
| Another template is staged and the command names a different template | Command/staged mismatch; document remains unread |
| Stored source cannot resume | Source-resolution error; document remains unread |
| Replay fails | Replay error; document remains unread |
| Replay is complete and a file was supplied | Unused-document refusal; document remains unread |
| Document is malformed and declares another template | Malformed document |
| Document is well-formed but declares another template | Template mismatch |
| Identity matches but an answer id is unknown | Existing per-question rejection |
| Identity matches but a template expression fails | Existing attributed template error |

## Identity errors and exit codes

| Condition | Behavior |
|---|---|
| File read or UTF-8 failure | Exit 1; source-prefixed read error |
| Invalid JSON | Exit 1; source-prefixed JSON error |
| Non-object or unknown envelope member | Exit 1; shape error |
| Missing `protocol` or `answers` | Exit 1; shape error |
| Protocol other than `1` | Exit 1; unsupported-protocol error |
| Missing `template` | Exit 1; missing-identity error with migration guidance |
| Non-string or empty `template` | Exit 1; malformed-identity error |
| Declared identity cannot be independently resolved | It is not resolved; exact comparison determines match or mismatch |
| Declared identity differs from expected | Exit 1; mismatch names both values |
| Command template is ambiguous | Existing exit 5 before document read |
| Expected source cannot resolve or resume | Existing exit 1 before document read |
| Identity matches and answers are rejected | Exit 4 with the existing question errors |
| Identity matches and interview completes | Existing completion, flow, planning, and apply behavior |

A declared formal name that no longer resolves is not an “unresolved document identity.” The consumer never resolves document data. The route already established the source it is running. If the strings match, the identity requirement is satisfied; if they differ, the result is a mismatch.

## Persistence, flow, planning, and target effects

| Outcome | Staged state | Plan | Target files/hooks |
|---|---|---|---|
| Read, envelope, or identity failure | Unchanged; a new record is not created | None | None |
| Rejected answers document | Existing record unchanged; new apply may stage the empty accepted prefix | None | None |
| Pending after accepted answers | Save accepted prefix unless CLI `--dry-run` | None | None |
| `Completed(Proceed)` | Save accepted submissions before planning; remove record after successful apply | Build | Apply |
| `Completed(DryRun)` flow | Save accepted submissions under normal execution | Build | Show only |
| `Ended(Stop)` | Save accepted submissions and retain staged state | None | None |
| `Ended(Abort)` | Remove staged state after the answer transaction commits | None | None |
| Any outcome under CLI `--dry-run` | No staged mutation | Build only when completion requires it | None |
| Store save failure | Existing atomic record remains authoritative | None | None |
| Planning failure | Accepted staged submissions remain recoverable | Failed build only | None |
| Apply or hook failure | Existing apply error rules remain | Built | Existing partial-apply rules |

The answers envelope is never stored. `StagedRecord.submissions` continues to contain only accepted inner answer maps. Replay therefore remains transport-independent.

A rejected tentative answer transaction cannot save a submission, execute a flow action, remove staged state, build a plan, write a target file, or run a hook.

## Exit-4 presentation

Exit code 4 keeps one semantic meaning: questions remain or answers were rejected.

Presentation depends on the output contract:

| Producer | Standard output is a terminal | Pipe or redirected standard output |
|---|---|---|
| `stage --async` | JSON question batch | JSON question batch |
| `continue PATH FILE` | Concise recovery summary | JSON question batch/result |
| `continue PATH -` | Concise recovery summary | JSON question batch/result |
| `apply … --answers FILE` | Concise recovery summary | JSON question batch |
| `apply PATH` without usable prompt terminals | Concise recovery summary | JSON question batch |

`stage --async` is an explicit machine-output request and never changes format.

TTY detection uses standard output. Standard input may be a file or pipe while the caller still receives a terminal summary.

A summary lists the formal template, canonical target, pending ids in batch order, per-id errors when present, and the commands that continue the staged interview:

```text
Interview incomplete for /opt/templates/sample-a at /tmp/output.
Questions: label, quantity
label: is required

Continue with prompts:
  toha continue /tmp/output
Submit an answers document:
  toha continue /tmp/output <FILE>
```

The summary does not reproduce the embedded JSON Schema. Exit status remains 4. Machine output retains the canonical JSON document byte-for-byte.

The binary owns this renderer. `protocol::batch_document` remains the sole machine-document builder.

## Migration and compatibility

External bare answer maps are a deliberate breaking change.

Migration wraps the existing map and copies `context.template`:

```json
{
  "protocol": 1,
  "template": "/opt/templates/sample-a",
  "answers": {
    "label": "Sample"
  }
}
```

Compatibility rules are:

- Bare maps fail. They are not inferred or rewritten.
- Existing staged records remain valid.
- Existing staged submissions remain bare internal maps.
- `Pending::answer(RawAnswers)` remains source-compatible for in-memory callers.
- Public callers of `protocol::parse_answers` or `answer_headless` migrate to `AnswersRoute`.
- Batch, complete, and ended output documents retain their existing shapes.
- The protocol schema, command specification, architecture description, examples, guides, and fixtures use the same envelope.
- Fixtures distinguish `answers.json` external documents from serialized staged `submissions`.
- Protocol version `1` identifies this exact envelope. Other values fail explicitly.

## Reassessment of the recovery questions

### Does `apply PATH --answers FILE` safely resume and apply an incomplete staged interview?

Yes.

The target selects one staged record and its immutable source. Replay establishes the expected formal template identity before the file is read. The document supplies an independent identity assertion. Only an exact match releases its answers to the engine.

No repeated template operand is required. A missing staged record remains an exit-1 error.

### Should exit-4 output become a concise summary on a terminal while pipes, redirects, and `stage --async` retain JSON?

Yes.

TTY presentation is a binary concern and does not change the protocol document. Pipes and redirects remain stable machine interfaces. `stage --async` always remains machine-readable because the caller explicitly selected asynchronous protocol output.

### Should `apply TEMPLATE PATH --answers FILE` resume when its template matches staged state?

Yes.

The route requires all three identities to agree:

```text
resolved command operand
          =
staged record formal template
          =
answers document declared template
```

The command/staged comparison occurs before file read. The document/staged comparison occurs after replay and before answer evaluation. Recovery uses the staged source and commit, not a newly fetched mutable source.

## Falsifiable behaviors

1. A matching envelope produces the same answers and tree through new apply, staged target-only apply, named staged apply, `continue`, and the public crate document route.
2. A document for another template fails even when every question id and value is valid for the active template.
3. Missing, empty, non-string, and mismatched identities each produce their specified exit-1 error.
4. A bare map fails with wrapper guidance and never reaches `Pending::answer`.
5. Unknown envelope members fail schema validation.
6. A document protocol other than `1` fails before identity comparison.
7. An alias or short name in `template` fails when the expected value is its formal name.
8. The declared identity never invokes source parsing, registry resolution, fetching, canonicalization, trust evaluation, or configured-default selection.
9. A named staged mismatch leaves a deliberately unreadable answers path unread.
10. A complete replay leaves a deliberately unreadable answers path unread and reports the existing unused-document refusal.
11. Identity failure emits no configured warning, replay message, question warning, or flow result.
12. Identity failure creates, saves, or removes no staged record.
13. Identity failure never calls `Plan::build`, `Plan::apply`, or a hook runner.
14. Question validation begins only after a matching identity.
15. A rejected transaction publishes no accepted answer, message, warning, hook, skip marker, or flow effect.
16. Target-only staged apply accepts a matching document and applies a completed interview.
17. Target-only staged apply stages an accepted incomplete prefix under normal execution.
18. Named staged apply requires command/staged/document equality.
19. Stored replay submissions do not acquire an envelope or duplicate template field.
20. Terminal exit-4 output is a concise summary, while the same invocation with redirected output emits a schema-valid JSON batch.
21. `stage --async` emits JSON when standard output is a terminal.
22. `RawAnswers` still compiles with `Pending::answer`.
23. `RawAnswers` does not compile as an argument to the public file-driven route.
24. Neither `WireAnswersDocument` nor `VerifiedSubmission` is nameable or constructible outside `protocol`.
25. A flow abort reached by a valid document removes only the current canonical target’s staged record.
26. A document that tentatively reaches abort but is rejected leaves the staged record present.
27. CLI `--dry-run` with a valid document changes neither staged state nor target state.
28. The same identity-bearing document can answer another target for the same formal template.
29. The same identity-bearing document can answer another resolved commit with the same formal template name.
30. Public schema examples and full-path fixtures validate against the embedded schema.

## Failure injection and sole-kill guards

- Replace the identity comparison with `true`; the cross-template test fails.
- Compare after `Pending::answer`; the engine-call spy and tentative-flow tests fail.
- Add alias resolution for the declared identity; the resolver-zero-call test fails.
- Move document reading before named staged comparison; the unreadable-file priority test fails.
- Move document reading before complete-replay refusal; the unused-file priority test fails.
- Emit configured warnings before verification; the no-output-on-mismatch test fails.
- Save a new staged record before verification; the store-zero-call test fails.
- Expose a public constructor or raw extraction method for `VerifiedSubmission`; public API compile-fail checks fail.
- Retain a public `answer_headless(..., RawAnswers)` file route; the misuse compile-fail check fails.
- Add a bare-map compatibility branch; the migration rejection test fails.
- Persist the external envelope instead of its accepted inner map; staged-record compatibility and replay fixtures fail.
- Remove the terminal presentation branch; the TTY summary test fails.
- Apply TTY presentation to `stage --async`; its machine-output test fails.
- Resolve a fresh template for matching named staged apply instead of resuming the recorded source; immutable-source replay fixtures fail.
- Remove the driver’s staged removal for committed abort; the staged-record-gone assertion fails.
- Move abort removal ahead of answer commit; the rejected-abort atomicity test fails.

## Design-red-flag screening

### Shallow module

`AnswersRoute::submit` hides JSON parsing, schema validation, protocol-version checking, identity comparison, question-id parsing, private capability construction, and headless driving behind one operation. Callers do not coordinate these stages.

Result: no shallow-module red flag.

### Information leakage

The wire envelope and verified capability are private. `RawAnswers` remains an interview-domain type and is not re-exported as an answers-document representation. The JSON shape is owned by `protocol` and its schema.

Result: no information-leakage red flag.

### Temporal decomposition

Parsing, identity verification, and wire-to-domain conversion remain together in `protocol`, which owns answers-document knowledge. They are not split into public load, validate, compare, and convert modules.

Result: no temporal-decomposition red flag.

### Pass-through methods

`AnswersRoute::submit` adds the identity policy and constructs the private capability. The binary helper adds file and standard-input I/O plus source attribution. Neither method merely forwards the same arguments.

Result: no pass-through red flag.

### Reader load

The file-driven call chain is:

```text
binary route → protocol::AnswersRoute → interview::Pending
```

Staging appears before the route to reconstruct the pending interview, not as another answers-document layer.

Result: no deep-call-chain red flag.

---

# Rationale

## Problem

External answer files currently become `RawAnswers` before the system knows which template authored them. Matching question ids can therefore make a document valid for the wrong template. The design must add formal template identity without moving transport concerns into the pure interview engine, duplicating source or target normalization, changing staged replay, or weakening the existing answer transaction and flow contracts.

## Usage (caller’s view)

A script copies `context.template` into an envelope containing `protocol`, `template`, and `answers`. CLI callers use that same envelope for new apply, target-only staged apply, named staged apply, and `continue`. Crate callers submit external JSON through `AnswersRoute`; callers that construct answers in memory continue to call `Pending::answer(RawAnswers)`.

## Shape

`AnswersRoute` binds an established formal name, template, and pending interview into one document-consuming capability. Its `submit` method parses once, validates the exact envelope, compares the declared identity, constructs a private `VerifiedSubmission`, and drives the existing pure engine.

The private capability is the load-bearing invariant: the file-driven headless driver cannot receive answers before comparison, per `encode-lessons-in-structure`. External data becomes domain ids at the protocol boundary, per `boundary-discipline`. The engine trusts `RawAnswers` as an in-memory transaction and remains transport-independent.

The public surface consists of one constructor, one submission method, one result enum, and attributed errors. It hides schema handling, comparison, conversion, and default-driven batch traversal. This is a deep interface.

The document’s formal name is compared as emitted. The existing source resolver remains the sole formal-name authority, and `canonical_target` remains the sole target factory. Staged replay remains derived from stored accepted submissions and the origin-bearing configured `Resolution`.

## Synthesis decision

Reserved for arena synthesis.

## Tradeoffs accepted

- We accept a breaking external file migration in exchange for rejecting cross-template submissions before answer evaluation.
- We accept a required `protocol: 1` member in exchange for an explicit, evolvable envelope.
- We accept exact formal-name matching in exchange for one identity authority and no untrusted alias or path resolution.
- We accept replacing the public raw protocol parser and headless function in exchange for compile-time separation between external documents and in-memory `RawAnswers`.
- We accept buffering route diagnostics until identity succeeds in exchange for atomic no-output behavior on identity failure.
- We accept template-only binding in exchange for preserving supported reuse across targets and commits.

## Alternatives considered

- **Expose `parse_answers(text, expected) -> VerifiedAnswers` and pass that value to `answer_headless`.** This permits a caller to verify against one expected name and accidentally submit to another active interview. It exposes coordination that `AnswersRoute` hides.
- **Add identity to `RawAnswers` or `Pending::answer`.** This moves a wire concern into the pure engine, burdens terminal and replay callers, and makes the engine interface shallower rather than safer.
- **Put `template` beside question ids in the existing map.** This reserves a possible question id, mixes metadata with answers, and leaks envelope policy into answer iteration.
- **Reuse the output `context` object.** Its required target and commit would bind documents beyond the decided template identity and would duplicate data that has no role in answer validation.
- **Resolve the declared identity and compare normalized sources.** This gives untrusted data fetch and registry behavior and creates a second identity-resolution path.
- **Retain bare-map fallback.** This allows file-driven callers to bypass the required identity and defeats the contract.

## Open questions and risks

- Can every documentation generator and fixture producer copy `context.template` without independently reconstructing a formal name?
- Can TTY behavior be tested through an injected output-mode probe so platform pseudo-terminal differences do not weaken coverage?
- Will downstream crate callers identify uses of the former raw protocol parser during the breaking-release migration?
- Should diagnostics redact any part of an absolute local formal name, given that protocol context already publishes the same value?

## Next implementation step

Implement the schema definition and `AnswersRoute` with private `WireAnswersDocument` and `VerifiedSubmission`, then prove the cross-template and compile-fail guards before changing CLI routes.