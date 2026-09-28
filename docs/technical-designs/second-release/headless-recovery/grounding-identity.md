---
relationships:
  depends-on:
    - confirm-flow
    - error-attribution
  informs: headless-recovery
---

# Answers identity and recovery grounding

## Product correction

Every answers file identifies the template it answers. This is a decided input
contract. The design must place the identity check before answer evaluation and
before any staged, flow, planning, target, or hook effect.

The recovery questions remain in scope and are reassessed below under the
identity-bearing contract.

## Current answers document

The current document is a bare JSON object keyed by question id.

```json
{
  "label": "Example",
  "quantity": 2
}
```

The schema defines `#/$defs/answers` as an object whose property names are
question identifiers (`docs/specifications/interview-protocol.schema.yml:16-20`).
The prose says the same (`docs/specifications/interview-protocol.yml:45-50`).
There is no template field, envelope, protocol version, target, or commit.

`main::read_answers` reads UTF-8 text, requires a JSON object, and delegates to
`protocol::parse_answers` (`src/main.rs:398-413`). `parse_answers` validates the
bare object, parses each key as an `Id`, and returns `RawAnswers`
(`src/protocol.rs:170-195`). The parse result has already lost every document
field other than question ids and values.

`RawAnswers` is a public alias for `IndexMap<Id, RawAnswer>`
(`src/interview.rs:33-36`; `src/lib.rs:20-24`). `Pending::answer` accepts it
directly (`src/interview.rs:1704-1713`). This is the pure in-memory answer seam,
not necessarily a file boundary.

## Existing identity authorities

### Template source identity

`cli::resolve::ResolvedTemplate.formal_name` is the normalized template name
used by the command driver (`src/cli/resolve.rs:17-26`). `resolve_template`
obtains it as follows (`src/cli/resolve.rs:179-216`):

- a folder is canonicalized to an absolute path by `source::parse`, then rendered
  as that path;
- a Git address is rendered by `Address::formal_name`, which removes user info,
  uses configured host shorthand when available, removes `.git`, and retains an
  explicit ref and template subpath;
- a registered short name or alias resolves to the registry entry's formal name.

`cli::resolve::formal_name` performs the same argument-to-formal-name conversion
without fetching a template (`src/cli/resolve.rs:219-239`). It is the existing
authority for comparing a command-line template assertion with staged state.

### Staged identity

`StagedRecord` stores the canonical target, formal template name, resolved
commit, name-resolution fact, start instant, and accepted submissions
(`src/staging.rs:20-29`). Replay uses the stored submissions only after the
caller has loaded the stored template and origin-bearing configured
`Resolution` (`src/staging.rs:312-348`).

The existing named staged route resolves the command operand to a formal name
and compares it with `StagedRecord.template` (`src/main.rs:529-560`). A mismatch
is refused before the answers file is read (`src/main.rs:835-841`).

### Protocol output identity

Every question batch and complete result already emits a context with the
canonical target, staged record's formal template name, and resolved commit
(`src/protocol.rs:14-31,118-161`). A script therefore receives the exact formal
template name that an answer file can carry. Target and commit identify the run
and source revision, but the product correction requires template identity; it
does not by itself require a target or commit binding.

### Target and configured-origin authorities

`staging::canonical_target(&Path) -> Result<CanonicalTarget, StagingError>` is
the sole target constructor. Storage, protocol context, planning, apply, abort,
and cleanup consume the same value. Recovery cannot accept a raw path at an
identity-sensitive seam.

Configured values enter through the origin-bearing `Resolution`, including
their winning mapping and preset sources. Start and replay consume that carrier.
Answers-file identity does not create or flatten configured defaults.

## Actual routes

| Route | Expected template identity today | File parse point | First possible answer effect |
|---|---|---|---|
| New `apply TEMPLATE PATH --answers FILE` | Resolved command template formal name | After template load and interview start (`src/main.rs:848-878`) | `answer_headless` |
| Staged `apply TEMPLATE PATH --answers FILE` | Command formal name must equal staged formal name | After stored source resolution and replay (`src/main.rs:916-968`) | `answer_headless` |
| `apply PATH --answers FILE` | Staged formal name is available | Refused before file read (`src/main.rs:816-824`) | None |
| `continue PATH FILE` | Staged formal name | After stored source resolution and replay (`src/main.rs:655-739`) | `Pending::answer` |
| Terminal prompting | Active `Pending` owns its template | No file | `Pending::answer` with a locally built map |
| Crate `protocol::parse_answers` | None | Parser returns a bare map | Caller chooses `Pending::answer` or `answer_headless` |
| Crate `Pending::answer(RawAnswers)` | `Pending` already owns a template | No file contract | Pure engine transaction |

For all file routes, JSON/schema parsing currently precedes question-id and
constraint validation. Question validation occurs inside `Pending::answer`.
Unknown ids are rejected as “is not a question in this template”
(`src/interview.rs:1710-1713`). Matching ids and valid values are accepted even
when the file was authored for another template because the file has no source
identity.

## State and effect ordering

The current command routes establish target, staged record, environment,
template source, configured defaults, and replay before reading a staged answer
file. A complete replay refuses an unused file without reading it
(`src/main.rs:705-713,955-961`). The recovery design preserves that priority:

1. Build the canonical target and load staged state when the route has a target.
2. Resolve any command template assertion and stored immutable source.
3. Load the template, resolve configured defaults, and replay accepted state.
4. Refuse an unused document if replay is already complete.
5. Read and parse the identity-bearing document once.
6. Compare its declared formal template identity with the already-established
   expected formal identity.
7. Only a match releases raw answers to the pure engine transaction.

Malformed JSON or envelope fails at step 5. Missing or mismatched identity fails
at step 6. These failures cannot validate a question, publish an answer-derived
warning or message, persist an accepted prefix, fire a flow action, build a
plan, mutate staged state, write a target file, or run a hook. Existing
configured-value warnings can already be emitted during route preparation at
steps 2 and 3; identity validation does not suppress, buffer, or reorder them.

The declared identity is data, not a template argument. Comparing it must not
fetch a source, resolve a mutable alias, consult trust, or perform a second path
normalization. The producer emits the formal identity; the consumer compares it
with the formal identity already established by the route.

## Crate boundary

The public protocol parser and headless driver currently erase document
identity by returning and accepting `RawAnswers`. The pure engine also exposes
`Pending::answer(RawAnswers)` for callers that construct answers in memory.

The new design must distinguish these cases:

- an external answers document is parsed into an identity-bearing type and
  verified before its values can enter a file-driven headless transaction;
- a crate caller constructing `RawAnswers` in memory continues to use the pure
  `Pending::answer` seam because no answers file exists;
- staged replay continues to use stored accepted submissions, not external
  answer documents, so replay does not gain a redundant identity field.

This boundary satisfies the required file identity without turning every
in-memory engine call into a transport concern.

## Accepted flow composition

The accepted flow design adds pending, proceed, dry-run, stop, abort, and skip
results. Ordinary `confirm` answers remain boolean values used by `flow.when`;
they are not actions. Identity verification precedes the answer transaction, so
a missing or mismatched identity cannot tentatively reach or commit any flow
result.

After identity succeeds, the recovery transaction retains the accepted flow
semantics:

- pending saves only the accepted prefix under normal execution;
- proceed can plan and apply;
- flow dry-run can save accepted answers and show a plan without target effects;
- stop can save accepted answers and retain staged state;
- abort can remove staged state under normal execution;
- CLI `--dry-run` simulates every result and changes no staged or target state;
- skip remains inside the interview walk.

The accepted flow design SHA-256 is
`54827ce90a185fa24568c79a14707815e225121fbaad7c060154ae17adf5381e`.
Its verification SHA-256 is
`9167bf7a48e3c65aaee4dc1dfb7dbbae62cf8d2188b0d9ff8ffc63bc370f8d52`.

## Error-attribution composition

The integrated error producer passes one `CanonicalTarget` and the consuming
origin-bearing `Resolution` through start and replay. Identity parsing and
comparison happen at the protocol boundary and cannot replace either carrier.
Question-id, kind, constraint, expression, planning, and staging errors remain
distinct after identity succeeds.

The integrated error design SHA-256 is
`9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
Its verification SHA-256 is
`41da2344a6422c996965c134f86268a849f8705c8ffce36d132613ef9cdbef05`.

## Original problem reassessment

### Target-only apply answers

The problem remains valid. The target already selects one staged record and its
immutable source. Requiring the answer file to declare the same formal template
adds an independent assertion; it does not require the caller to repeat the
template as a command operand. A mismatch can fail before answer evaluation.

### Terminal exit-4 presentation

The problem is unchanged. Identity changes the input document, while the
question batch and exit-4 output remain output contracts. A terminal can still
receive a concise recovery summary while a pipe or redirected file receives the
canonical machine document.

### Matching-template named recovery

The problem remains valid. The named route supplies three facts: the command
operand's resolved formal identity, the staged record's formal identity, and the
document's declared formal identity. Recovery proceeds only when all three are
equal. The operand remains a useful explicit assertion and a supported route.

## History and rationale evidence

The bare-map contract entered in the initial specification commit `7f8fa54` and
the first runtime commit `77309c0`. Repository history states the shape but
contains no rationale for omitting template identity. No later commit adds an
alternative identity mechanism to answer input. The current correction is
therefore a product-contract change rather than restoration of a hidden rule.

## Preserve, change, avoid, risk

### Preserve

- One pure interview engine and its in-memory `RawAnswers` transaction.
- Formal template identity from source/registry resolution.
- Canonical target and origin-bearing configured-default carriers.
- Staged replay from accepted submissions.
- Exit 4 and machine-readable question results.
- Accepted flow meanings and effect ordering.

### Change

- External answers input becomes an identity-bearing envelope.
- Every file/document consumer verifies the declared formal template identity
  before releasing raw values to the engine.
- Bare answer maps fail with migration guidance because they do not satisfy the
  required identity contract.
- Protocol schema, examples, README crate usage, fixtures, command contract,
  guides, and architecture ownership describe the same envelope.

### Avoid

- Resolving or fetching the identity declared by an untrusted answers file.
- Accepting aliases or alternate source spellings in place of the emitted formal
  identifier.
- Binding answer files to a target or commit unless separately designed and
  approved.
- Adding identity to staged submissions or `Pending::answer`.
- Parsing the document independently in apply and continue.
- Preserving bare maps through an automatic fallback that defeats the required
  identity.

### Risk

The file-format correction is intentionally breaking for existing bare answer
files. Migration guidance must make the required wrapper and exact expected
formal identity visible. A design that exposes the nested map before identity
verification recreates the defect through the public crate API even if the CLI
checks correctly.
