---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
    - error-attribution
    - jinja-includes
    - stage-admission-synthesis
---

# Toha-owned Jinja context rationale

## Problem

Interview and plan rendering rebuild Jinja data independently, while staged
replay retains only time, answers, target, and selected identity. Environment
access must be decided before the first question, but source-tree Jinja is
currently discovered later and the existing undeclared-variable list misses
`debug()` context enumeration and self-shadowing reads. Later batches cannot
ask for access or reread ambient values. Canonical target, configured defaults,
includes, and registry review already have separate owners.

## Usage (caller's view)

The [design](design.md) starts with terminal, headless, staged, and crate call
sites. A caller loads one complete render program, crosses one environment
admission method, constructs one typed context, and starts the existing engine.
Stage uses `--trust` once when analysis finds an environment need. Continue and
staged apply replay the recorded snapshot without a trust option or ambient
read.

## Shape

`Template` owns the complete compiled render program and a conservative
environment need. `Template::admit_environment` hides the relationship between
that need, the caller's decision, failure ordering, and fixed-value capture.
`InvocationContext` moves through `Seed`, `Pending`, `Completed`, staging, and
`Plan`. One private projection serves readiness, interview rendering, and plan
rendering.

The interface is deep: one load and one admission call hide source-tree walk,
literal include closure, AST evaluation order, alias/shadow analysis,
`debug()` enumeration, attributed refusal, five-value capture, collision
knowledge, and redaction. Command adapters retain current trust, host, terminal,
and privilege decisions. The pure engine accepts only domain values.

The staged wire is separate from the live target carrier. The producer's sole
factory and `Store::load(&CanonicalTarget)` remain the only target authority.

## Synthesis decision

The original arena established the immutable invocation context, exact names,
host/admin/interactive semantics, collision boundary, and caller parity. Its
historical evidence remains unchanged.

The targeted arena addresses the stage-only trust decision. Candidate 2 is the
base for its fixed-five snapshot and exact trust eligibility. Candidate 1
contributes the single deep admission method and attributed render locations.
Candidate 3's public analysis pipeline and program wrapper are rejected.

The architect and read-only cross-judge reject every source/program-identity
replay gate. Such a gate would make a mutable folder stage fail after an edit,
which is a new restriction unrelated to access. Capturing all five values after
any admitted need is the smallest deterministic snapshot over the closed
vocabulary that preserves current folder replay without later access.

The architect adds a legacy context contract. A staged record without context
uses the pre-context projection, so it exposes no new fact and needs no
access-related abort/restage path.

The target shape consumes approved producer revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
It uses `CanonicalTarget`, `StagingError`, `as_path()`, and no unchecked
constructor or consumer normalization.

## Tradeoffs accepted

- We accept eager compilation of source-tree Jinja and earlier attributed
  syntax/include errors in exchange for admission before interview progress.
- We accept conservative all-five classification for uncertain AST forms and
  possible `debug()` calls in exchange for preserving supported syntax.
- We accept a required `Seed.context` migration in exchange for explicit facts
  on every supported caller path.
- We propose plaintext persistence of up to five optional values in exchange
  for deterministic later batches without ambient reads or a source-change
  refusal. This remains an explicit checkpoint decision.
- We propose that later folder edits use the frozen decision: unavailable stays
  null; captured supplies all five recorded values. This remains an explicit
  checkpoint decision.
- We accept a snapshot of originating interaction, host, privilege, and selected
  identity in exchange for stable branches across every continuation modality.
- We accept a private legacy projection in exchange for no invented historical
  facts and no access recovery for existing staged records.
- We accept exact-name reservation rather than the whole `toha_` prefix in
  exchange for avoiding an unrequested compatibility restriction.
- We accept MiniJinja's unstable AST feature behind one private adapter in
  exchange for using the parser that actually defines supported syntax. A
  dependency update can require a compile-time adapter change; no runtime pinned
  version check is added.

## Alternatives considered

- Process-reading globals expose hidden state and cannot preserve replay or
  direct-caller parity.
- `undeclared_variables(false)` alone has false negatives for `debug()`,
  self-shadowing reads, and includes.
- Referenced-value-only storage needs a source-identity refusal or produces
  false nulls after mutable folder edits.
- Persisting a render program is larger and would silently ignore folder edits.
- An admission boolean without values authorizes fresh reads in later
  processes.
- Rechecking trust on continue contradicts the stage-only decision and creates
  later abort/retry behavior.
- A generic environment map or dynamic lookup exceeds the fixed contract.
- A consumer target wrapper duplicates the producer's identity policy.

## Open questions and risks

- Will the manager approve storing up to five optional plaintext values under
  the existing staged-state directory and umask until apply, abort, or operator
  removal?
- Will the manager approve the two mutable-folder outcomes under the frozen
  decision, with no source-change refusal?
- Can every supported MiniJinja AST form be conservatively classified without
  an unacceptable false-positive rate? The verification suite must include
  shadowed `debug`, callable aliases, dynamic operands, and an uncertainty
  fallback.
- Does the retained in-process render program cover every planning path after
  the include design integrates? Any plan-time reopen of Jinja source would
  break the admission invariant.

## Next implementation step

After approval of the exact checkpoint, implement the retained render program
and AST need analysis, then prove the admission matrix before threading current
and legacy contexts through staging and planning.
