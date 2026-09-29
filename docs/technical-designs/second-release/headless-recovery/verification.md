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

### Scripted apply

`apply TEMPLATE PATH --answers FILE` checks for a staged record first and
refuses without reading the document when one exists. Otherwise it passes the
resolved formal identity, active template, started interview, and document text
to `answer_document_headless`. The function parses once, compares once, and
drives the headless walk only after the comparison. The route consumes the
`Headless` result without any staged effect and emits one result document.

### Continue with a document

`continue PATH FILE` passes the staged formal identity, active `Pending`, and
document text to `answer_document_once`. The operation calls `Pending::answer`
at most once. It returns the accepted storage map only with a successful engine
transition. Rejected input returns the pending state and rejections without a
persistable submission.

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

| Route | Identity source | Read priority | Engine meaning | Staged effect | Result |
|---|---|---|---|---|---|
| Scripted apply with file | Resolved command template | After staged refusal and start | Headless walk | None | Carries. |
| Continue with file/stdin | Staged record | After replay proves input usable | Exactly one step | Accepted submission | Carries. |
| `stage --async` | Resolved command template | No document | Start only | Create record | Carries. |
| `apply PATH` | Staged record | No document | Replay only | Remove after write | Carries. |
| Person prompts | Active interview | No document | In-memory steps | Save each batch | Carries. |
| Staged replay | Staged record | No document | Stored in-memory steps | None | Carries. |
| Crate external document | Caller-established formal name | Caller-owned I/O | Explicit one-step or headless operation | Caller | Carries. |
| Crate in-memory | Active interview | No document | Existing raw APIs | Caller | Carries. |

Only two command routes read an answers document. Each has one expected
identity source. One public operation does not claim both one-step and headless
behavior.

## Caller-route verification

| Property | Carrier | Result |
|---|---|---|
| Scripts receive one JSON document for every outcome. | Result documents with status and exit table. | Carries. |
| Scripts never stage. | Staged refusal before read; no store call on the route. | Carries. |
| Agents receive the batch and its instructions together. | Batch JSON then instructions on stdout. | Carries. |
| Agents see missing answers as the next step. | `errors` excludes questions not yet asked. | Carries. |
| People can pause and resume the direct route. | New `apply T P` saves each batch. | Carries. |
| No output selection from terminal status. | stdout check removed; stdin checked only on prompting routes. | Carries. |
| Handoff between agent and person. | Shared staged record; no caller-kind field. | Carries. |

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

| Accepted result | Scripted route | `continue PATH FILE` | Guard |
|---|---|---|---|
| Pending | `questions`; nothing saved. | Save accepted submission. | No plan or target effect. |
| Completed proceed | Plan, apply, `applied`. | Save; completion instructions. | Only completed values are plannable. |
| Completed flow dry-run | Plan, `planned`. | Save; completion instructions. | No target write or hook. |
| Ended stop | `ended`. | Save; retain record. | No completed value or plan. |
| Ended abort | `ended`; no record exists. | Remove record. | No plan, write, or hook. |
| Skip | Inside the walk. | Inside the step. | No recovery-specific skip policy. |
| CLI `--dry-run` | `planned`; no mutation. | Not applicable. | Abort removal suppressed. |

An ordinary confirm value enters the answer transaction. It is not a recovery
action. A later flow `when` can use it and produce one of the listed results.

## Original problem reassessment

| Original question | Evidence | Resolution |
|---|---|---|
| Target-only apply with answers | Staged recovery belongs to `continue PATH FILE` and `apply PATH`; the scripted route has no staged effect. | Not needed; scripted route refuses when staged. |
| Exit-4 presentation | Reported by an agent; the defect is inconsistent meaning of an incomplete interview and separated instructions. | Resolved by caller routes. |
| Matching named staged apply | People use `apply TEMPLATE PATH` directly. | Kept for the person route, by prompting. |

## Compatibility verification

Toha is before 1.0.0 and has no dependent users. The breaking changes are listed
in `design.md` under compatibility and consequential changes. In addition:

- The migration wrapper preserves the inner answer map without reinterpretation.
- Staged record bytes and accepted replay submissions do not change.
- `RawAnswers`, `Pending::answer`, and raw headless driving remain available for
  in-memory callers.
- The batch and ended documents keep their accepted shapes; `applied`,
  `planned`, and `error` are added; `complete` is removed.
- External documents remain portable across target and commit for one formal
  template identity.
- Exit codes keep their meanings.

## Arena verification

The identity round contains two structurally distinct, terminal candidates:

- candidate 1 exposes parsed and verified document states;
- candidate 2 binds parse and verification to one consuming route capability.

The candidates used separate writable output paths. Both input manifests were
unchanged. The runner directories were packaging boundaries, not enforced
filesystem sandboxes; `arena-setup-identity.md` records actual reads and technical
reachability. The rubric was created only after both candidates were terminal.

The other-family judge and parent both selected candidate 2 as the base for the
document boundary. `synthesis-identity.md` and `cross-judge-identity.md` resolve
the score disagreement and every graft.

The caller-route model is Bob's product decision, not an arena output. It
changes which routes consume the document boundary, not the boundary itself.
Bob approved revising the design directly without a further arena round for
this reason.

## Red-flag verification

- The final design has no public verified-to-raw extraction.
- It has no shallow generic load/validate stages.
- It has no pass-through wrapper around `Pending::answer`; each document
  operation adds the identity invariant and consumes the private capability.
- It has no hidden mode for one-step versus headless behavior.
- It has no caller-kind flag and no output selection from terminal status.
- It adds no alternate source identity, target identity, or configured origin.
- It adds no permission/access rule, timeout, pinned-version check, or
  application subprocess.

## Canonical-document impact

The paired implementation owns changes to the interview protocol/schema, command
specification, architecture ownership, crate documentation, guides, fixtures,
and examples. Each will state one two-field envelope and exact comparison rule.
No shared canonical document changes occur in this design task.

## Falsifiable implementation contract

The 27 numbered behaviors and 15 sole-kill guards in `design.md` cover:

- the required cross-template same-ids failure;
- every missing, malformed, and mismatch route;
- zero identity-resolution calls;
- read priority and unused-input refusal;
- staged and target byte preservation;
- the scripted route's single document and zero staged effect;
- agent-route output, instructions, and completion;
- person-route prompting, batch saving, and resume;
- one-step and headless distinction;
- all accepted flow results and CLI preview;
- crate API source and compile-fail compatibility;
- target and commit portability;
- schema and full-path examples.

Each load-bearing guard has a named test that fails when the guard is removed or
inverted. The implementation task binds these exact behaviors after approval.

## Verdict

The design carries the formal template identity from its existing producer to a
single external-document boundary without reconstruction. A mismatched document
cannot enter the engine or reach staged, flow, planning, target, or hook
effects. Each caller kind has one route whose modality is fixed by the route.
The exact revision remains pending Bob's approval.
