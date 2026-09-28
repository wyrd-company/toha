---
relationships:
  evaluates:
    - candidate-identity-1
    - candidate-identity-2
  informs: headless-recovery
---

# Other-family cross-judge: identity redesign

## Execution

- Runner: Claude Agent SDK, high effort, read-only prompt.
- Async session: `7559ff37-b315-4755-9edc-a67fed21f79a`.
- Claude session: `d585bce6-fa2d-47c4-9b36-8f12e4a11e14`.
- Working directory: `/tmp/arena-headless-identity/round-2/judge`.
- Terminal status: completed; 7 turns; stop reason `end_turn`; no reported problems.
- Inputs: revised grounding, runner task, scoring rubric, and the two terminal candidate artifacts.

The judge reported reading all five supplied files completely. The runner had
normal host filesystem access; the capsule and prompt constrained the assigned
inputs but did not make other files unreachable.

## Verdict and scores

The judge preferred candidate 2 as the base because its private
`VerifiedSubmission` binds verification to submission and provides no public raw
extraction. It recommended grafting candidate 1's ordering and proof detail.

| Criterion | Candidate 1 | Candidate 2 |
|---|---:|---:|
| Identity correctness and atomicity | 5 | 4 |
| Authority and interface depth | 3 | 5 |
| Route coherence and problem reassessment | 4 | 4 |
| Compatibility and predecessor composition | 5 | 4 |
| Proof quality and implementation readiness | 5 | 4 |
| **Total** | **22/25** | **21/25** |

Both candidates passed the 3-per-criterion threshold.

## Candidate findings

### Candidate 1

The judge credited its exact comparison, explicit validation priority,
no-effect list, route matrix, migration path, predecessor preservation, and
proof tables. It found one architectural defect: public
`VerifiedAnswers::into_raw_answers` lets a caller verify a document against one
formal name and then submit the extracted values to another interview. The
separate public parse, verify, extract, and submit steps expose a temporal
protocol.

The judge also flagged its terminal rule as a possible unsupported narrowing:
`apply --answers` and `continue FILE` keep JSON on a terminal even though the
original decision question reserves JSON specifically for pipes, redirects, and
`stage --async`.

### Candidate 2

The judge credited its private capability, single consuming operation, reuse of
existing formal-name and target authorities, complete route matrix, and
compile-fail proof. It found one concrete migration defect: schema rejection of
unknown top-level fields could report an unknown question key before recognizing
a legacy bare map, contradicting the promised wrapper guidance.

It also observed that a public route taking only `Pending` leaves each crate
caller to handle complete or ended interviews and the unused-document refusal.

## Judge graft recommendations

The judge recommended carrying these candidate 1 details into the candidate 2
base:

1. Recognize a legacy bare map and issue migration guidance before rejecting its
   question keys as unknown envelope fields.
2. State the purpose of any envelope version member.
3. Specify that target-only apply without staged state fails before reading the
   file and names the template-taking form.
4. Give exact wrapper guidance for bare maps.
5. Carry the post-identity fault injections for warnings, staged save, planning,
   and trust.

The judge asked the parent to settle terminal-summary scope, encapsulation of
complete/ended document refusal, and CLI mapping of document faults versus
answer rejections.

## Parent disposition

The parent agrees with the preferred base and private-capability finding. The
final design adopts recommendations 1, 3, 4, and 5.

Recommendation 2 becomes unnecessary: the final envelope contains only
`template` and `answers`. A protocol-version field is not needed to satisfy the
identity contract and would add a version check beyond the required correction.

The parent found an additional route defect in candidate 2. Its only public
`submit -> Headless` operation performs a multi-batch walk, but current
`continue PATH FILE` performs exactly one `Pending::answer` step. The final
surface therefore has separate consuming one-step and headless operations over
one private parse-and-verify function.

The parent also rejects both candidates' proposed buffering of configured-value
warnings. Current route preparation emits those diagnostics before file read.
The required atomicity boundary prohibits answer-derived output and staged,
flow, plan, target, or hook effects; it does not authorize changing existing
configured-warning timing.

Terminal summary selection follows candidate 2 and the literal decision scope:
apply and continue question results use the selected terminal representation;
pipes, redirects, and `stage --async` retain JSON.
