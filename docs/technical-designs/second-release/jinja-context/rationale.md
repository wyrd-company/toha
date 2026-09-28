---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
    - error-attribution
---

# Toha-owned Jinja context rationale

## Problem

Interview and plan rendering rebuild Jinja data independently, while staged
replay retains only time, answers, target, and selected identity. Target,
template identity, host facts, execution status, and five access-gated values
must exist before the first question and remain stable through planning without
process reads in the pure engine. The canonical target, configured defaults,
and reviewed registry trust already have separate owners.

## Usage (caller's view)

The [design](design.md) begins with template, command, staged, and crate call
sites. A caller supplies one typed context in `Seed`; it does not register
Jinja variables or coordinate separate interview and plan maps. A staged caller
stores that snapshot and proves current access before replaying a granted one.

## Shape

`InvocationContext` is an immutable domain snapshot owned by `Seed`, moved
through `Pending` to `Completed`, and serialized by staging. One private
projection combines it with template data, answers, and `now` for every
interview and plan render. Command adapters own resolution, host observation,
terminal mode, and trust; direct callers supply the same domain types. This
keeps validation at input seams and the engine pure. The interface is deep:
one value hides seventeen projections, absence rules, collision policy,
redaction, and replay representation.

## Synthesis decision

Candidate 1 is the base. Candidate 2 contributes array-valued `ID_LIKE`, target
consistency, and legacy reference detection. Candidate 3 contributes no-read
denial and staged identity checks. Candidate 4 contributes boolean naming,
platform admin semantics, and host fallbacks. Candidate 5 contributes only the
planning consistency invariant; its session façade is rejected. The architect
also narrows collision reservation to the seventeen exact names and preserves
registry alias order.

The canonical-target producer completed its own synthesis after the arena. This
design therefore binds its target input conditionally to producer revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
Its recommended pending projection accepts `CanonicalTarget`, uses only
`as_path()`, and has no unchecked constructor. If Bob retains `PathBuf`, the
same context and plan fields use the separator-free factory result unchanged.
Both projections keep normalization in the producer.

## Tradeoffs accepted

- We accept a required `Seed.context` migration in exchange for explicit facts
  on every supported caller path.
- We accept plaintext persistence of at most five granted values in existing
  staged state in exchange for deterministic replay; current authorization is
  required before they are evaluated again.
- We accept a snapshot of the originating interaction mode in exchange for
  replay-stable branches while still allowing every documented continuation
  modality.
- We accept nullable target and host text in exchange for preserving root,
  non-Unicode, and metadata-poor hosts without lossy conversion or refusal.
- We accept exact-name reservation rather than the whole `toha_` prefix in
  exchange for avoiding an unrequested compatibility restriction.
- We accept a conditional target-carrier projection in exchange for keeping
  this consumer ready for either independent producer decision. This document
  does not approve the producer's API restriction.

## Alternatives considered

- Process-reading MiniJinja globals expose hidden state and cannot preserve
  replay or direct-caller parity.
- Rebuilding context for planning lets interview and generated output disagree.
- A separate environment permission creates another policy outside the required
  `--trust` gate.
- Re-reading environment data on resume changes prior decisions; replaying it
  without current authorization ignores revocation.
- A `Run` façade prevents some mismatches but adds a second public workflow and
  a broad lifecycle redesign for a context feature.
- A consumer-owned target wrapper or fallback normalizer duplicates the
  producer's identity policy and is rejected under either carrier choice.

## Open questions and risks

- Will Bob approve option A, where either `--trust` or a current matching
  reviewed-registry approval grants the five values?
- Will Bob approve adding `--trust` to `stage` and `continue`, persisting the
  five granted values in existing staged state, and requiring current access
  before replay?
- Does plaintext staged persistence need a separate hardening design after this
  feature, even though this design changes no staged-file permission policy?
- Which target carrier will Bob approve in the independent canonical-target
  checkpoint? The context behavior is fixed; only the Rust carrier projection
  changes.

## Next implementation step

After exact-revision and access approval, add the context domain types and one
Jinja projection, then carry the snapshot through the existing engine and
staging seams before adding host/access adapters.
