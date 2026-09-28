---
relationships:
  evaluates:
    - candidate-identity-1
    - candidate-identity-2
  informs: headless-recovery
---

# Parent scoring for the identity redesign

## Scores

| Criterion | Candidate 1 | Candidate 2 |
|---|---:|---:|
| Identity correctness and atomicity | 4 | 5 |
| Authority and interface depth | 3 | 4 |
| Route coherence and problem reassessment | 3 | 3 |
| Compatibility and predecessor composition | 3 | 3 |
| Proof quality and implementation readiness | 5 | 5 |
| **Total** | **18/25** | **20/25** |

Both candidates meet the viability threshold of 3 in every criterion. Candidate
2 is the base because its private verified capability makes identity checking
harder to bypass. Neither candidate is accepted unchanged.

## Candidate 1

### Identity correctness and atomicity — 4/5

The envelope, exact comparison, failure priority, and no-effect boundary are
precise. `AnswersDocument::verify` prevents an unverified document from entering
the engine. The score loses one point because public `VerifiedAnswers` can be
verified against one formal name, extracted as `RawAnswers`, and submitted to a
different active interview. The proposed type marks that a comparison happened;
it does not bind the comparison to the consuming operation.

### Authority and interface depth — 3/5

The design correctly reuses `ResolvedTemplate.formal_name`,
`StagedRecord.template`, `CanonicalTarget`, and origin-bearing `Resolution`.
Protocol parsing hides substantial policy. Public verification, raw extraction,
and submission still require callers to coordinate a load-bearing sequence.

### Route coherence and problem reassessment — 3/5

The candidate correctly distinguishes one-step `continue` from the multi-batch
headless walk, and its route/read-order matrix is complete. It makes terminal
presentation depend on whether an answers option was used: implicit apply gets
a summary, while `apply --answers` and `continue FILE` keep JSON on a terminal.
That changes the earlier question from result presentation to command-intent
presentation without evidence that a person at a terminal stops needing help
after supplying one incomplete file.

### Compatibility and predecessor composition — 3/5

The candidate preserves engine inputs, staged records, flow meanings, target
identity, configured origins, and reuse across target and commit. It correctly
names the bare-map break. It also buffers configured-value warnings until
identity succeeds. Current routes emit those warnings during preparation, and
the required no-mutation guarantee does not authorize changing their timing.

### Proof quality and implementation readiness — 5/5

The document supplies concrete callers, shapes, signatures, route and error
matrices, thirty falsifiable behaviors, mutations, and compile-fail guards.

## Candidate 2

### Identity correctness and atomicity — 5/5

The private `VerifiedSubmission` has no raw extraction path. Parsing,
comparison, conversion, and submission are one operation. Cross-template input
cannot reach the engine through the public document route.

### Authority and interface depth — 4/5

`AnswersRoute` reuses the existing identity producers and hides the external
document policy behind one operation. Its constructor still accepts expected
identity, template, and pending interview separately, so a crate caller can
pair an expected identity with an unrelated active interview. The final design
must keep the private capability while narrowing each public operation around
the active engine state it actually consumes.

### Route coherence and problem reassessment — 3/5

The route matrix and all three reassessments are clear. The single public
`submit -> Headless` interface performs the multi-batch headless walk, while
`continue PATH FILE` must perform exactly one `Pending::answer` step. The design
claims both behaviors through one operation without a mode or second adapter.
That is a material interface gap. Its terminal summary recommendation is
otherwise coherent across apply and continue results.

### Compatibility and predecessor composition — 3/5

The candidate preserves the same predecessor contracts and migration boundary
as candidate 1. It also buffers configured warnings and replay messages before
identity comparison. That is an unapproved diagnostic-order change and is not
required for atomic staged, flow, plan, target, or hook behavior.

### Proof quality and implementation readiness — 5/5

The route, failure, result, migration, and effect matrices are thorough. The
named fault injections make the identity and effect guards independently
testable, including public API compile failures.

## Red-flag screen

Candidate 1 has an information-leakage warning: its public capability exposes
the raw engine input and makes verification and consumption a temporal
protocol. Candidate 2 avoids that leak, but its single driver hides two
different route semantics and therefore lacks a complete abstraction. Neither
candidate adds a second target or template normalizer, a permission rule, a
timeout, a pinned dependency check, or an application subprocess.

## Parent correction before synthesis

The synthesized design will:

- use the common closed `protocol` / `template` / `answers` envelope;
- use exact comparison with the already-established formal identity;
- keep the verified submission private and consume it in one operation;
- expose separate one-step and headless document operations so `continue` and
  apply retain their existing engine meanings;
- preserve configured-warning and replay-diagnostic timing from route
  preparation while prohibiting answer-derived output and every mutation on an
  identity failure;
- keep bare `RawAnswers` only for terminal, replay, and in-memory engine calls.
