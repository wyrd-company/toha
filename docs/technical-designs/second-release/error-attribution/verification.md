---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
  verifies:
    - design
---

# Design verification — error attribution and early answers

## Bound inputs

- Epic base and configured-default prerequisite head:
  `4738042a766dc7b2d1d3c76a9c83c41c03e803d2`.
- Configured-default design SHA-256:
  `0538251bb0c51310d0e288c8a44449085bf66e0a3e3547cf7d354f29c49580a5`.
- Configured-default package tree: `9d2d185b7bf54ec63297eeb6f390b2686be1c14e`.
- Approved configured behavior: exact `formal_name`, `presets`,
  `{ preset: <name> }`, mapping-first attribution, optional winning preset
  origin, one-hop resolution, and validate-when-defined.
- The public flat `Seed.defaults` route remains available. Configured callers
  use the approved consuming `Resolution::start` and staged replay route so
  provenance stays attached to the selected value.

## Approved checkpoint

The implementation contract is bound to commit
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
The approved selections are:

1. Reject a known-invalid early answer when its skip is unresolved.
2. Omit a proven-skipped early error beside a current failure and emit no
   warning from the rejected probe.
3. Check a skipped template default against ready constraints without forcing
   unavailable dynamic dependencies.
4. Consume one origin-bearing `Resolution` for configured start and replay
   while preserving the ordinary flat `Seed` route.
5. Require the opaque `CanonicalTarget` from the sole factory at every
   identity-sensitive public consumer.

Selection 5 includes the disclosed removal of raw `&Path` from those supported
public methods. No other supported capability, permission, access, timeout,
pinned-version, subprocess, or trust-policy change is part of the approval.

## Requirement matrix

| Requirement                                                     | Design location                                     | Falsifier                                                                               |
| --------------------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Attribute file-rule and ordinary path render faults             | Template faults; behaviors 1–3                      | Any old `files[i].when: ...` or message-only path fault remains.                        |
| Template-invalid default is a template fault                    | Template defaults; behaviors 4–5                    | It appears in protocol errors or triggers terminal retry.                               |
| Exact configured provenance survives later validation           | Configured defaults; data structures; behaviors 6–9 | A later error reconstructs an id-only key, loses either winning file, or becomes fatal. |
| Undecidable early failure has explicit policy                   | Early behavior; ordering; behaviors 10–14           | A known-invalid unresolved value enters an accepted submission.                         |
| Proven-skipped noise beside current failure has explicit policy | Early behavior; behaviors 11–12                     | The irrelevant early error remains or a rejected probe emits a warning.                 |
| Existing target has separator-free canonical identity           | Canonical target; behaviors 16–19                   | Existing and non-existing forms hash or display differently.                            |
| Direct/apply/continue/replay equivalence                        | Behaviors 3, 6, 14–15                               | Any route reports a different status, errors, accepted history, or output.              |
| One target authority consumed by Jinja context                  | Caller usage; target type; module map               | A second target canonicalizer exists or a raw path enters an identity consumer.         |

## Caller-flow verification

- Library configured start surfaces warnings, consumes one origin-bearing
  resolution, and does not coordinate a value map with an origin map.
- Ordinary library defaults retain the existing `Seed` construction and
  behavior.
- Direct, terminal, headless, continue, staged apply, progress/replay, and crate
  callers all enter the same private default bank and `Pending::answer`
  transaction.
- Planning remains before trust and writes. New faults therefore stop before
  either.
- Protocol and terminal adapters display engine results; they do not decide
  default authorship or early reachability.
- Jinja context receives the target value created by staging. It neither creates
  nor normalizes a target.

## Interface verification

- `Resolution` is the only configured value-plus-origin carrier after config
  selection. `ConfigEntry<T>` remains the only source for config file/layer
  origin.
- `Seed.defaults` is not replaced. `into_flat_defaults` makes intentional origin
  loss explicit for an ordinary seed caller.
- `TemplateFault` is crate-private and adds no public error hierarchy.
- `SubmissionTxn` owns one document and one commit decision; the probe has no
  independently implemented guard or skip semantics.
- `CanonicalTarget` has one public constructor and no unchecked conversion.
- Legacy state lookup derives one alias from the canonical text and does not
  create a competing live target.

## Arena reconciliation

The architect and judge disagreed on the base because Candidate 1 enforced
target identity but replaced the seed contract, while Candidate 3 preserved the
seed contract but left target correctness to caller discipline. The final design
takes the enforced target from Candidate 1 and the dual configured/plain ingress
from Candidate 3. It removes Candidate 3's terminal-only default method and
Candidate 1's configured-only seed.

The contaminated candidate contributed only independently verified edge cases:
staged apply coverage, two-file provenance, ready skipped-default constraints,
and compile-fail target checks. Its score did not select the base.

## Compatibility verification

- Existing flat-seed crate behavior remains callable.
- Existing staged-record JSON remains readable.
- Existing separator-keyed state has a bounded fallback and moves only after an
  atomic canonical save.
- Rejected documents were never persisted, so omitting one irrelevant early
  error does not change staged history.
- The unresolved early policy matches current acceptance behavior.
- Configured-default naming, selection, precedence, migration, and schema do not
  change.
- The opaque target changes public signatures that currently accept `&Path`.
  The exact checkpoint approval explicitly includes this capability change.

## Constraint audit

- No production code, shared specification, schema, or guide changes are in this
  design branch.
- No permission or access change.
- No timeout or pinned-version check.
- No application subprocess integration.
- No trust or hook-execution policy change.
- No implementation dependency on the Jinja-context design.
- Examples use generic values and a domain separate from Toha's implementation.

## Residual implementation risks

- The probe must continue far enough to prove an unrelated skip while treating a
  rejected current answer as unavailable.
- Ready skipped-default validation must not force a dynamic dependency solely to
  validate a skipped branch.
- The configured-resolution amendment must land in the configured-default
  implementation contract before downstream code assumes it.
- Platform root and separator handling needs fixtures on each supported
  platform.
- The public target signature change will expose missed crate call sites at
  compile time; the implementation must update every one in one atomic change.

## Design gate

Grounding, three candidate packages, contamination disposition, independent
score, readonly cross-judge, synthesis, red-flag screen, and this requirement
audit are complete. The exact checkpoint and all five recommended selections
are approved. The paired implementation remains closed only on its recorded
merge prerequisites and this design's independent review and integration.
