---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Rationale

## Problem

Toha needs actionable attribution after template loading, when file planning,
default selection, early-answer classification, and staged identity already
cross module boundaries. The shape must preserve one pure interview engine,
atomic answer documents, replay from accepted submissions, and the approved
configured-defaults behavior. The configured-defaults prerequisite selects by
exact `formal_name`, uses `presets` and `{ preset: <name> }`, and names the
mapping file/site plus an optional preset hop. Its original flat seed sketch
conflicts with the review requirement to retain that selected origin until
dynamic constraint validation. This candidate assumes the repaired prerequisite
couples value and origin in its resolution product and seed; it does not
introduce another carrier.

## Usage (caller's view)

Library callers resolve configured defaults and pass `Seed::new(now, defaults)`
to `Interview::start`. Staged and headless callers replay the same
origin-bearing defaults and submit documents through `Pending::answer`.
Completed callers obtain a `CanonicalTarget` once and pass it to `Plan::build`
and `Plan::apply`. Protocol context and staged storage consume the same target.
Callers still recover from `AnswerError::Rejected` with the returned `Pending`;
template faults remain terminal. No caller coordinates a value map with an
origin map, evaluates skip conditions, or normalizes a target for a second
subsystem.

## Shape

The recommended architecture has four load-bearing values. `TemplateFault` owns
the field, exact compiled source, and message used by both interview and
planning errors, per `encode-lessons-in-structure`. The repaired
configured-defaults `ConfiguredDefault` owns raw value and selected origin
together; `Seed` carries that resolution product opaquely, per
`single-source-of-truth`. Prompt preparation validates an authored default while
authorship is still known: a bad template default is an evaluation fault, while
a bad configured default becomes an attributed batch error that terminal or
headless input can replace. The interview engine learns only that the origin is
displayable; config layers, preset lookup, and selector rules remain behind the
resolution boundary, per `boundary-discipline`.

`Pending::answer` uses a non-committing probe owned by the interview module.
Normal advancement and the probe share dependency readiness and guard
evaluation. The probe uses only accepted state and successfully checked current
answers. It classifies an early failed question as active, skipped, or
undecidable, then discards all effects unless the document is accepted. Known
failures remain errors when undecidable. A failure proven skipped is removed
even beside a separate batch failure; a rejected document emits no warning. This
keeps the state transition atomic and the policy out of drivers, per
`make-operations-idempotent` and `minimize-reader-load`.

`CanonicalTarget` is opaque and has one public constructor,
`staging::canonical_target`. It owns absolute lexical normalization,
closest-existing- ancestor canonicalization, and the no-redundant-separator
invariant. Store identity, records, protocol context, planning, applying, and
future Jinja context accept this type. The public interface is small but deep:
it hides filesystem-dependent normalization and prevents unchecked construction.
A bounded legacy-key fallback in `Store` preserves old trailing-separator
records without becoming another normalizer.

## Synthesis decision

This is Candidate 2; arena synthesis has not selected a base. The candidate
centers the design on origin-bearing defaults, a shared template-fault value, a
pure reachability probe, and an opaque canonical target. It rejects parallel
provenance and target representations because either would make later behavior
depend on maps that can drift. A synthesizer can adopt the whole shape or graft
these four values only if their single-source invariants remain intact.

## Tradeoffs accepted

- We accept a public `Seed` construction change in exchange for making a
  configured value without its selected origin unrepresentable.
- We accept one internal reachability probe in exchange for removing irrelevant
  early errors beside batch failures without committing speculative state.
- We accept conservative `Undecidable` results in exchange for never proving a
  skip from a rejected or unavailable answer.
- We accept canonical JSON spelling for typed literal defaults in exchange for
  one fault format without retaining YAML presentation trivia.
- We accept a bounded legacy store lookup in exchange for continuing staged
  records whose former identity contains the trailing-separator defect.
- We accept an opaque target type at planning and protocol call sites in
  exchange for preventing normalization drift and unchecked target identity.

## Alternatives considered

- Keep `Seed.defaults` flat and add `IndexMap<Id, Origin>` beside it. This keeps
  individual signatures familiar but exposes synchronization to every
  constructor, replay path, and clone; the interface hides almost no complexity
  and permits drift.
- Store a rendered config-key string on `EvalError` only. This handles early
  kind failures but discards provenance before later constraints; callers would
  have to reconstruct approved mapping and preset text, so it cannot meet the
  recovery contract.
- Defer every invalid early answer until its question is reached. This makes the
  engine accept documents containing values already known to be invalid and
  moves failures across staged submissions; it hides less policy from replay and
  drivers than rejecting active or undecidable failures at submission.
- Retain every early error whenever the current batch fails. This is simpler to
  implement but exposes a known irrelevant error to protocol consumers. The pure
  probe hides the classification complexity and preserves atomicity.
- Keep returning `PathBuf` from `canonical_target` and rely on call-site
  discipline. That function remains shallow because any caller can construct an
  unnormalized substitute. The opaque type hides the invariant and lets
  downstream signatures enforce it.
- Move target normalization into a future Jinja-context component. Staging,
  protocol, and planning would retain separate identities until that component
  runs, creating an implementation dependency and a second authority.

## Open questions and risks

- Will the repaired configured-defaults package expose the atomic
  value-plus-origin resolution product assumed here, with one formatter for the
  approved mapping and preset-hop text?
- Does product approval select the recommended early policy: reject known
  failures when skip is undecidable, and omit a proven-skipped failure beside
  another batch failure?
- Should successful migration delete the exact legacy trailing-separator state
  file immediately after the canonical atomic save, or retain it until the
  interview is removed?
- Can any current public crate caller require direct struct-literal construction
  of `Seed` or `StagedRecord`, making the opaque invariants a compatibility
  concern before the second release?

## Next implementation step

Implement and test the repaired configured-defaults value-plus-origin carrier
first, then replace `Seed` construction so every later default-validation path
has the required provenance.
