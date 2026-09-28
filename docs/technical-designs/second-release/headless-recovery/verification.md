---
relationships:
  verifies: headless-recovery
  depends-on:
    - confirm-flow
    - error-attribution
---

# Identity-bearing recovery design verification

## Basis

- Current epic basis: `9a6d902a97c225069cd87d3299a5f0b2703683df`.
- Candidate source snapshot: `d33245065f80e3ed9a8c75ea6526e8a9f9386f21`.
- Accepted flow design SHA-256: `54827ce90a185fa24568c79a14707815e225121fbaad7c060154ae17adf5381e`.
- Accepted flow verification SHA-256: `9167bf7a48e3c65aaee4dc1dfb7dbbae62cf8d2188b0d9ff8ffc63bc370f8d52`.
- Accepted flow synthesis SHA-256: `52375a56c635b9be567887fb34cd73a25e3ab39981bc13ded8b68b22c2320e2e`.
- Integrated error-attribution design SHA-256: `9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
- Integrated error-attribution verification SHA-256: `41da2344a6422c996965c134f86268a849f8705c8ffce36d132613ef9cdbef05`.
- Accepted context design SHA-256: `2711043ef216d965bd05c8d8e477f2f0f4d2bca020b936c7f405d06b537565e9`.
- Accepted context verification SHA-256: `c8c7a7fa8f259edcf138305d08bcf00db2b89efecee22f6f22976633367d17df`.
- Identity candidate 1 SHA-256: `3028c72482dcf69abad2dd07527bd51490e9c9d0f9206cdc23d2ca4b1fa5ceb3`.
- Identity candidate 2 SHA-256: `13e8a42fcaeb90a175215d11e1df27ffae7bee4379202543d964d555a587a415`.
- Other-family judge: async session `7559ff37-b315-4755-9edc-a67fed21f79a`.

The current epic advance after the candidate snapshot adds only the accepted
Jinja-context design package. It changes no runtime source cited by grounding.
That design keeps the public staged replay routes, extends them with the same
producer-created `CanonicalTarget`, and consumes the origin-bearing `Resolution`.
The recovery boundary begins after start or replay and accepts those carriers
unchanged.

## Required identity correction

| Required property | Design carrier | Result |
|---|---|---|
| Every external answers document identifies its template. | Closed `template` + `answers` envelope. | Carries. |
| Matching ids in another template are insufficient. | Exact comparison precedes id and value validation. | Carries. |
| One identity authority. | Resolver/staged record produces expected formal name; declaration is data only. | Carries. |
| No second normalizer. | Plain equality; no parse, canonicalize, alias, registry, fetch, or trust call. | Carries. |
| Missing/malformed/unresolved/mismatch behavior. | Ordered document faults; declared value is never independently resolved. | Carries. |
| Atomic failure. | Private verified capability precedes engine and all staged/flow/plan/target/hook effects. | Carries. |
| Route compatibility. | One shared private parser with separate one-step and headless consuming operations. | Carries. |
| No extra binding. | Envelope has no target, commit, or version field. | Carries. |

## Caller usage against interfaces

### New and staged apply

Both pass the route's expected formal identity, active template, active interview,
and document text to `answer_document_headless`. The function parses once,
compares once, and drives the existing headless walk only after the comparison.
Its `Headless` result retains accepted submissions for the staging policy.

### Continue

`continue` passes the staged formal identity, active `Pending`, and document text
to `answer_document_once`. The operation calls `Pending::answer` at most once.
It returns the accepted storage map only with a successful engine transition.
Rejected input returns the pending state and rejections without a persistable
submission.

### Terminal, replay, and in-memory crate calls

These calls have no external answers document. They keep `RawAnswers` and the
pure engine interfaces. The identity requirement is not copied into terminal
questions or staged submissions.

### File boundary

The binary reads text once and adds the path or `stdin` label to protocol errors.
It cannot obtain a `VerifiedSubmission` or raw map from the external parser. The
protocol module owns the whole parse-compare-convert-consume operation.

No usage example relies on an interface absent from the design sketch.

## Authority traces

### Template identity

```text
command template -> resolve_template -> ResolvedTemplate.formal_name
staged target    -> Store::load      -> StagedRecord.template
question output  -> Context          -> context.template
                                      |
answers.template ---------------- exact equality
                                      |
                              private submission
```

The declaration cannot choose the source. It asserts the source identity already
selected by the route.

### Target identity

```text
raw CLI path
  -> staging::canonical_target(&Path) -> CanonicalTarget
  -> Store::load/save/remove(&CanonicalTarget)
  -> protocol::Context::new(&CanonicalTarget, record)
  -> Plan::build(..., &CanonicalTarget)
  -> Plan::apply_reporting(&CanonicalTarget, ...)
```

The document API has no target parameter. Recovery cannot normalize or construct
a second target.

### Configured value provenance

```text
ConfigEntry values and source paths
  -> configured_defaults(...)
  -> Resolution { values + origins + warnings }
  -> Resolution::start or replay_with_resolution
  -> attributed evaluation and diagnostics
```

The document operations receive neither defaults nor origins. Existing configured
warnings retain their route-preparation timing.

### Accepted submissions

```text
external text
  -> parse_and_verify
  -> private VerifiedSubmission { RawAnswers + storage map }
  -> Pending::answer / headless walk
  -> accepted result only
  -> staged-effect policy
```

Rejected or mismatched input never appears in the accepted result.

## Route matrix verification

| Route | Identity source | Read priority | Engine meaning | Result |
|---|---|---|---|---|
| New apply with file | Resolved command template | After template/start | Headless walk | Carries. |
| Target-only staged apply with file | Staged record | After replay proves input usable | Headless walk | Carries. |
| Named staged apply with file | Command first equals staged; staged gates document | Command mismatch before read | Headless walk | Carries. |
| Continue with file/stdin | Staged record | After replay proves input usable | Exactly one step | Carries. |
| Terminal prompts | Active interview | No document | In-memory steps | Carries. |
| Staged replay | Staged record | No document | Stored in-memory steps | Carries. |
| Crate external document | Caller-established formal name | Caller-owned I/O | Explicit one-step or headless operation | Carries. |
| Crate in-memory | Active interview | No document | Existing raw APIs | Carries. |

The matrix closes candidate 2's hidden mode: one public operation does not claim
both one-step and headless behavior.

## Validation and failure ordering

The route establishes whether a document can be consumed before opening it. The
protocol boundary then classifies syntax and envelope faults before comparing
identity, and compares identity before parsing question identifiers or invoking
the engine.

The legacy-map detector precedes generic unknown-envelope checks. It exists to
produce migration guidance only; the input remains invalid. A malformed intended
envelope is still rejected even if it resembles a bare map.

A missing, empty, or non-string identity and a mismatch are distinct document
errors. “Unresolved declared identity” has no resolver state: a non-empty string
is accepted on exact match and is otherwise a mismatch.

## Atomicity verification

An identity failure cannot reach:

- `Pending::answer` or the headless walk;
- accepted submission output;
- `Store::save` or `Store::remove`;
- flow disposition;
- `Plan::build` or `Plan::apply`;
- target file creation or change;
- hook or trust execution;
- answer-derived warnings, messages, or question output.

The route may already have loaded configuration, emitted configured-value
warnings, resumed an immutable source, replayed an existing record in memory, or
populated the source cache. Those are existing preparation steps required to
establish whether the document is usable. The staged record and target remain
byte-for-byte unchanged.

## Accepted flow compatibility

| Accepted result | Recovery action | Guard |
|---|---|---|
| Pending | Save accepted prefix under normal execution. | No plan or target effect. |
| Completed proceed | Save accepted progress, plan, apply, then clean staged state after success. | Only completed values are plannable. |
| Completed flow dry-run | Save accepted progress, build/show plan. | No target write or hook. |
| Ended stop | Save accepted progress and retain staged state. | No completed value or plan. |
| Ended abort | Remove through the existing canonical target under normal execution. | No plan, write, or hook. |
| Skip | Stay inside the engine walk. | No recovery-specific skip policy. |
| Any result with CLI `--dry-run` | Simulate without staged or target mutation. | Abort removal is suppressed. |

An ordinary confirm value enters the answer transaction. It is not a recovery
action. A later flow `when` can use it and produce one of the listed results.

## Original problem reassessment

| Original question | Identity-bearing evidence | Recommendation |
|---|---|---|
| Target-only apply with answers | Target selects one staged identity; document independently asserts it; mismatch precedes answer evaluation. | 1A: support recovery. |
| Terminal question presentation | Input identity does not change human need or machine output; one result has two representations. | 2A: terminal summary, non-terminal JSON. |
| Matching named staged apply | Command, record, and document form a three-way identity assertion; stored source remains authoritative. | 3A: preserve recovery. |

The reported problem remains valid in all three cases.

## Compatibility verification

- Bare external maps are rejected as required by the identity correction.
- The migration wrapper preserves the inner answer map without reinterpretation.
- Staged record bytes and accepted replay submissions do not change.
- The public raw JSON parser is replaced because it erases document identity.
- `RawAnswers`, `Pending::answer`, and raw headless driving remain available for
  in-memory callers.
- Batch, complete, and ended output documents retain their accepted shapes.
- External documents remain portable across target and commit for one formal
  template identity.
- Exit 4 retains its question/rejection meaning; document faults use exit 1.
- Exit 3 and accepted flow exits retain predecessor semantics.

The only supported-capability changes are the required rejection of identity-less
answer files and the public raw JSON parser that would bypass that same
requirement. The pure in-memory capability is preserved.

## Arena verification

The revised round contains two structurally distinct, terminal candidates:

- candidate 1 exposes parsed and verified document states;
- candidate 2 binds parse and verification to one consuming route capability.

The candidates used separate writable output paths. Both input manifests were
unchanged. The runner directories were packaging boundaries, not enforced
filesystem sandboxes; `arena-setup-identity.md` records actual reads and technical
reachability. The rubric was created only after both candidates were terminal.

The other-family judge and parent both selected candidate 2 as the base. The
score disagreement and every accepted/rejected graft are resolved in
`synthesis-identity.md` and `cross-judge-identity.md`.

## Red-flag verification

- The final design has no public verified-to-raw extraction.
- It has no shallow generic load/validate stages.
- It has no pass-through wrapper around `Pending::answer`; each document
  operation adds the identity invariant and consumes the private capability.
- It has no hidden mode for one-step versus headless behavior.
- It adds no alternate source identity, target identity, or configured origin.
- It adds no permission/access rule, timeout, pinned-version check, or
  application subprocess.

## Canonical-document impact

The paired implementation owns changes to the interview protocol/schema, command
specification, architecture ownership, crate documentation, guides, fixtures,
and examples. Each will state one two-field envelope and exact comparison rule.
No shared canonical document changes occur in this design task.

## Falsifiable implementation contract

The 23 numbered behaviors and 14 sole-kill guards in `design.md` cover:

- the required cross-template same-ids failure;
- every missing/malformed/mismatch route;
- zero identity-resolution calls;
- read priority and unused-input refusal;
- staged and target byte preservation;
- one-step/headless distinction;
- all accepted flow results and CLI preview;
- terminal and machine presentation;
- crate API source and compile-fail compatibility;
- target/commit portability;
- schema and full-path examples.

Each load-bearing guard has a named test that fails when the guard is removed or
inverted. The implementation task binds these exact behaviors after approval.

## Verdict

The design carries the formal template identity from its existing producer to a
single external-document boundary without reconstruction. A mismatched document
cannot enter the engine or reach staged, flow, planning, target, or hook effects.
The three recovery recommendations remain pending Bob's approval of the exact
revised artifact and deck revision.
