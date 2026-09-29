---
relationships:
  synthesizes:
    - candidate-identity-1
    - candidate-identity-2
  informs: headless-recovery
---

# Identity redesign synthesis

## Result

Candidate 2 is the base for the document boundary. Its private verified
submission is consumed by the operation that creates it, so external JSON cannot
be converted to a reusable raw capability after an unrelated identity check.
The final route surface preserves the distinct engine semantics of the scripted
apply walk and one-step continue.

The external document is:

```json
{
  "template": "<formal template name>",
  "answers": { "<question id>": "<JSON value>" }
}
```

The declared template is compared exactly with the already-established formal
identity. It is never resolved. There is no target binding, commit binding,
protocol-version field, second source normalizer, second target normalizer, or
configured-origin reconstruction.

## Parent and judge reconciliation

Both scorecards pass both candidates and select candidate 2 as the architectural
base. They weight defects differently:

- The judge scored candidate 1's atomicity higher and placed its public raw
  extraction defect under interface depth. The parent treats that extraction as
  a direct weakening of the identity invariant as well. The final design removes
  the extraction path.
- The judge scored candidate 2's identity lower because its migration error can
  be preempted by unknown-field validation. The parent initially scored the core
  comparison highly but accepts the concrete ordering defect. The final design
  recognizes a legacy bare map before generic unknown-envelope errors.
- The judge viewed candidate 2's single operation as coherent. The parent traced
  the current callers and found that `continue` is one step while apply performs
  a headless walk. The final design supplies separate consuming operations.
- The judge accepted buffered route diagnostics as part of atomicity. The parent
  compared actual current ordering and found no authorization to change existing
  configured-warning timing. The final design preserves that timing and forbids
  only answer-derived diagnostics and effects after an identity failure.

## Grafts

### From candidate 2

- Private wire and verified-submission types.
- One protocol-owned parse, envelope validation, exact comparison, id parsing,
  and raw conversion path.
- No public constructor, raw extraction, or separately reusable verified value.
- Route, flow, migration, and compile-fail proof matrices.

### From candidate 1

- Explicit document-validation priority.
- Legacy-map detection before unknown-envelope diagnostics.
- No-staged-record and unused-document read priority.
- Separate one-step and headless caller meanings.
- Exact migration wrapper guidance.
- Detailed failure injection for store, plan, hook, warning, and target effects.

### Parent corrections

- Split candidate 2's single driver into `answer_document_once` and
  `answer_document_headless`, both backed by one private `parse_and_verify`.
- Keep existing `Pending::answer(RawAnswers)` and raw `answer_headless` for
  in-memory callers; replace only the public external-JSON-to-raw parser.
- Remove the `protocol: 1` input member. Template identity requires an envelope,
  but no input-version check is needed.
- Preserve current configured-warning timing and disclose possible source-cache
  preparation before document read.
- Return the accepted storage map from the one-step operation only after engine
  acceptance, so the caller cannot persist tentative input.

## Rejections

- Candidate 1's public `VerifiedAnswers` and `into_raw_answers` are rejected
  because verification can be separated from the interview that consumes it.
- Candidate 2's constructor holding expected identity, template, and pending
  state behind one headless result is rejected because it cannot preserve
  one-step continue semantics without a hidden mode.
- Both candidates' required `protocol: 1` member is rejected because it does not
  contribute to the required identity invariant.
- Both candidates' diagnostic buffering is rejected because it changes existing
  route-preparation output without being necessary for identity atomicity.
- Target and commit members are rejected because they would impose additional
  reuse restrictions beyond the authorized template identity.
- Alias resolution, path normalization, and source fetching from declared data
  are rejected because the existing producer already emits the formal identity.
- A bare-map compatibility fallback is rejected because it bypasses the decided
  identity requirement.
- Candidate 2's terminal-summary scope across apply and continue question results
  is rejected. It selected output from stdout terminal status for calls that
  carry an answers document, which cannot tell an agent on a pseudo-terminal
  from a person. The cross-judge treated the task's reassessment question as
  product authority; it was an investigation question. Bob's caller-route model
  replaces it.

## Interface conclusion

```rust
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

Each operation parses once and consumes a private `VerifiedSubmission`. The CLI
provides the expected identity from the resolved command template or staged
record. A crate caller provides a formal identity it has already established.
The document declaration itself has no authority to select or resolve a source.

## Product conclusions

The route surface follows Bob's caller-route model rather than the candidates'
recovery recommendations:

- the scripted route, `apply TEMPLATE PATH --answers FILE`, is one shot, never
  stages, refuses when an interview is staged, and writes one JSON result
  document;
- the agent route is `stage --async`, `continue PATH FILE`, and `apply PATH`,
  with batch JSON and instructions on standard output;
- the person route is `stage`, `continue PATH`, and `apply TEMPLATE PATH`, with
  prompts and batch saving.

Both document-consuming routes use the synthesized boundary unchanged:
`answer_document_headless` for the scripted route and `answer_document_once` for
`continue PATH FILE`. Target-only and named staged `--answers` recovery are not
part of the design.
