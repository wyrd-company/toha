---
relationships:
  realizes: toha
  references:
    - stage-admission-grounding
    - stage-admission-rubric
    - stage-admission-cross-judge
---

# Stage environment admission synthesis

## Arena execution

Three independent foreground Codex CLI sessions used `gpt-6-sol` in isolated
Git capsules. Each capsule contained the common task, targeted grounding,
current public design context, architect instructions, and one distinct
whole-shape direction. No scoring rubric was present. The private rubric was
created after all three processes started. All candidates completed; there were
no candidate dropouts.

No configured architect-runner pool existed at
`~/.pi/agent/pstack/models.json`. Codex CLI 0.158.0 and Claude Code 2.1.283 were
available after the recorded 16:30 UTC Claude service limit. The read-only
cross-judge used Claude Opus. Its first launch failed on CLI argument ordering
before reading evidence; the corrected launch completed without edits.

## Architect scores

The architect read all candidate designs and rationales end to end before
scoring.

| Candidate | Analysis | Program ownership | Trust and lifecycle | Replay state | Integration and proof | Weighted total | Disposition |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 4 | 4 | 5 | 3 | 4 | 4.00/5 | Graft interface and locations. |
| 2 | 4 | 4 | 5 | 4 | 4 | 4.20/5 | Base after removing source gate. |
| 3 | 4 | 3 | 5 | 4 | 3 | 3.80/5 | Reject exposed pipeline and wrapper. |

Candidate 1 loses replay-state points because its subset snapshot depends on an
unstored content identity. Candidate 2 provides the only fixed-five snapshot,
but its identity refusal would narrow folder resumes. Candidate 3 owns a useful
render package but exposes analysis stages and over-binds static bytes.

The judge and architect select the same base and the same repair. Their score
differences do not change the order.

## Synthesized shape

`Template` owns one retained in-process render program. `Template::load`
compiles every supported configuration render, non-static source-tree path and
body, and literal file-body include closure. Planning in that process renders
those compiled sources. A private MiniJinja AST walk records whether any of the
five fixed environment values may be observed and retains an attributed first
reference.

The analyzer walks reads in evaluation order, tracks lexical shadowing and
aliases of values and the built-in `debug` callable, visits dynamic expression
operands, and treats every branch and macro as potentially reachable. A
possible `debug` call means all five. An AST form that cannot be proved safe is
classified as all five; supported syntax is not rejected. A future generic
context lookup requires an explicit new analysis rule.

Callers cross one admission interface:

```rust
pub enum EnvironmentDecision {
    Deny,
    Grant,
    RequireGrant,
}

impl Template {
    pub fn admit_environment(
        &self,
        decision: EnvironmentDecision,
        source: &mut impl FixedEnvironmentSource,
    ) -> Result<EnvironmentSnapshot, EnvironmentAdmissionError>;
}
```

`RequireGrant` is the stage decision when `--trust` is absent. With any need it
returns an attributed error before reading, seed construction, interview
progress, or a staged write. `Deny` is a valid direct/new-apply decision and
reads nothing. `Grant` reads all five optional values once only when a need is
present. No-need admission reads nothing for every decision.

The immutable snapshot is either `Unavailable` or `Captured(FixedEnvironment)`.
The latter has exactly five named `Option<String>` fields, not a map. It flows
through `Seed`, `Pending`, `Completed`, and `Plan`. A versioned staged wire
stores the same enum and exact non-environment facts. Stored plaintext remains
until successful staged apply, `abort`, or operator removal under the existing
state-directory and umask behavior.

Replay uses the recorded snapshot without a trust option, registry check, host
read, environment read, or privilege read. Mutable folder templates continue
to load current bytes as they do today. A snapshot captured after any initial
need contains the entire closed five-value vocabulary, so later edits need no
access decision. An unavailable snapshot stays unavailable, so later edits see
five nulls. There is no content-identity refusal.

The staged wire does not contain `CanonicalTarget`. `StagedRecord.target`
remains path text. `Store::load(&CanonicalTarget)` receives the producer-created
carrier, validates the stored path, and uses a clone/reference of that carrier
to reconstruct the live invocation context. `Plan::build` retains the approved
producer signature with `&CanonicalTarget` and verifies it agrees with the
completed context before rendering. No consumer constructs or normalizes a
target.

## Legacy staged records

A record with no context field replays under an explicit `Legacy` context
contract. It projects exactly the pre-context Jinja map: template data,
answers, and private time only. It does not inject, reserve, or mark available
the seventeen new names. `debug()` therefore cannot reveal new host or
environment facts, and no historical values are invented. The record remains
legacy through continuation and staged apply, then disappears on successful
apply or abort.

This compatibility mode is internal to records created before the context
contract. New stage/apply and direct crate calls always use the current
contract. It avoids an access-related abort/restage path while preserving the
old staged behavior.

## Grafts, corrections, and rejections

Grafts:

- Candidate 1's single deep admission method and attributed `RenderOrigin`;
- Candidate 2's fixed-five typed wire, uncertainty fallback, and exact option
  1A eligibility rule;
- Candidate 3's rule that the live domain target is distinct from staged wire.

Architect corrections:

- remove every program/source-identity replay gate;
- preserve the approved `Plan::build(..., &CanonicalTarget)` signature rather
  than deriving a replacement target interface;
- avoid duplicate target and formal-name fields in the context wire;
- give legacy records a behavior-preserving legacy context instead of
  inventing facts or requiring access recovery.

Rejected:

- referenced-only persisted values;
- all source-change and program-change replay refusals;
- public analysis masks or source identities;
- unsupported-Jinja failure as an analyzer fallback;
- any later trust check, ambient capture, generic environment map, or second
  target authority.

## Manager decisions at the revised checkpoint

1. Approve or reject storing up to five optional plaintext values after any
   admitted environment need. At most four can be extra when one fixed name is
   referenced; `debug()` needs all five. No values are stored when there was no
   need or access was denied.
2. Approve or reject mutable-folder replay under the frozen decision: a stage
   that originally had no need exposes nulls to later added references; a stage
   that captured values exposes that recorded five-value snapshot to later
   added references. No new source-change refusal is introduced.
3. Approve or reject the exact revised design package. Approved option 1A,
   explicit `stage --trust`, and the producer target interface are not reopened.
