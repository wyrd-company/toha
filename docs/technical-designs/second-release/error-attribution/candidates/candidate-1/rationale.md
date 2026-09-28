---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
    - design
---

# Rationale

## Problem

Toha has one interview engine but loses authorship information at two boundaries:
planning reduces file-expression failures to strings, and configured-default
resolution reduces a selected value to `RawAnswer` before dynamic constraints are
known. Early answers also require a non-mutating skip decision, including when the
current batch fails. Target normalization is shared state identity, but its raw
`PathBuf` result permits inconsistent display and duplicate normalization. The
shape must preserve atomic documents, replay from accepted submissions, terminal
and headless recovery, the approved `presets`/exact-`formal_name` behavior, and
one future-compatible target authority.

## Usage (caller's view)

The library caller resolves configured defaults and moves the returned
values-with-origin directly into `Seed`; it does not build an origin sidecar. A
staged caller calls `canonical_target(path)` once, then passes the resulting
`CanonicalTarget` to `Store`, protocol context, replay, and planning. A completed
caller passes that same value to `Plan::build` and `Plan::apply`. Template authors
see `template error in <field> `<expression>`: <message>` for file fields and
source-path segments. Callers see an invalid template default as a template fault,
while an invalid configured default remains a per-question error they can replace.

## Shape

One `TemplateFault` owns authored field, optional expression, message, and display
format; `PlanError` adds the source path. This removes parallel hook, interview,
and planner formatting per encode-lessons-in-structure. Prompt preparation renders
the full rules and validates the selected default at the first point where both
rules and authorship are known. A private `DefaultCandidate` couples value to
either an authored site or a reference to the configured-default resolver's
carrier. Template failure terminates; configured failure becomes a carried batch
rejection and the invalid default is not offered. This keeps config selection
outside progression and preserves recovery per boundary-discipline.

Early-answer policy stays inside `Pending::answer`. A cloned `Advance` performs
one probe even beside current failures, classifies failed early ids as active,
skipped, or undecidable, and is either committed whole or discarded whole.
Undecidable known failures reject; proven-skipped failures are removed. No driver
implements this policy, and no rejected transition leaks state, per
make-operations-idempotent.

`CanonicalTarget` is an opaque constructed value. Its only public constructor owns
absolute lexical normalization, closest-existing-ancestor canonicalization, and
separator-free output. Identity-sensitive APIs require the type. A private legacy
key lookup reads separator-suffixed staged records, then migrates them after a
successful save. The public surface is one constructor, one value, and existing
domain operations; it hides filesystem and compatibility work and has high
interface depth per minimize-reader-load.

The integration assumption is that the repaired configured-defaults design
returns one `ResolvedDefault` containing raw value and exact mapping plus optional
preset attribution, and `Seed` stores that carrier. This design consumes it by
reference. If that carrier is absent, implementation is blocked; a second carrier
would create two sources of truth.

## Synthesis decision

Candidate 1 uses prompt preparation as the convergence point for default
authorship, rendered constraints, and recovery; a discarded full-engine probe for
all early-answer policy; and an opaque canonical-target value for all target
consumers. These choices form one shape: preserve source information in domain
values until the owning policy consumes it, then expose only the stable result.
The design rejects parallel diagnostic sidecars, driver-specific early handling,
and planner-local path normalization because each duplicates an invariant at a
caller boundary.

## Tradeoffs accepted

- We accept prompt-preparation work before a caller answers in exchange for correct
  dynamic-constraint attribution and identical terminal/headless behavior.
- We accept a speculative cloned transition when early validation fails in
  exchange for one state machine and proof that skipped errors are irrelevant.
- We accept rejecting a known-invalid early answer whose skip is undecidable in
  exchange for immediate feedback and replay containing only accepted documents.
- We accept omitting a proven-skipped error beside another failure in exchange for
  diagnostics that describe only values the document could use.
- We accept a source-breaking target parameter type in exchange for making missed
  normalization unrepresentable at staging, planning, and apply boundaries.
- We accept one private legacy state-key probe in exchange for reading staged
  records created with the trailing-separator defect.

## Alternatives considered

- Validate defaults only when `Pending::answer` substitutes an omitted value. This
  hides less complexity: terminal default acceptance looks like caller input,
  prompt schemas can advertise invalid defaults, and provenance must cross more
  call sites. It lost to prompt preparation.
- Validate all defaults during configured-default resolution or interview start.
  This makes configured constraint errors fatal and cannot evaluate constraints
  that depend on prior answers. Its small implementation surface exposes recovery
  and timing decisions to callers.
- Defer a known early failure until its question is reached. This avoids a probe
  on undecidable paths but permits already-invalid data into accepted staged
  history and delays deterministic feedback.
- Keep early failures whenever the current batch fails. This avoids probing failed
  documents but exposes an irrelevant skipped-question error to every driver.
- Keep `PathBuf` in public target APIs and fix only `join("")`. This is locally
  simple but leaves normalization optional, permits future Jinja-context work to
  duplicate it, and exposes identity correctness to each caller. The opaque value
  hides more complexity behind a smaller rule.

## Open questions and risks

- Will the repaired configured-default carrier expose raw value and exact
  attribution without requiring this design to copy its internal origin fields?
- Do any external pre-release crate callers construct `Prompt`, `Seed`, or call
  plan/apply with raw paths in a way that needs a source-compatibility shim?
- Does the legacy staged-key fallback need to recognize any platform spelling
  beyond one redundant native separator?
- Can a fault reached during a discarded probe be tested across every driver so
  that no message, warning, hook, or answer escapes with a rejected document?

## Next implementation step

Land the repaired configured-default carrier, then add `TemplateFault` and prompt
preparation against that exact type before changing early probing or target
signatures.
