---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
  explains:
    - design
---

# Rationale

## Problem

Toha must retain template-field identity across planning faults, distinguish a
faulty template default from recoverable caller input, preserve exact configured-
default provenance through later validation, classify early answers without
committing a rejected document, and give one canonical target spelling to every
consumer. The non-obvious constraint is that configured provenance begins in two
winning `ConfigEntry` values but the provisional integration flattens to
`Seed.defaults`; replay also rebuilds all derived state from accepted submissions.
The approved configured-default names and selection behavior remain fixed.

## Usage (caller's view)

The library caller resolves configured defaults and starts the interview through
the returned capability:

```rust
let resolution = configured_defaults(formal_name, template, presets, mappings)?;
report(resolution.warnings());
let interview = resolution.start(template, now)?;
```

The staged caller preserves the same capability through replay:

```rust
let target = canonical_target(requested)?;
let interview = record.replay_with_resolution(template, resolution)?;
let result = answer_headless(template, interview, document)?;
```

The completed caller passes one normalized value to every downstream consumer:

```rust
let target = canonical_target(requested)?;
let plan = Plan::build(template, completed, &target)?;
let context = protocol_context(record, &target);
```

Existing callers with application-supplied flat defaults keep
`Interview::start(template, Seed { now, defaults })`.

## Shape

The center is an opaque `Resolution` containing one `ResolvedDefaults` map. Each
entry couples a typed answer with mapping origin and optional preset origin derived
once from the winning `ConfigEntry` values. `Resolution::start` and
`replay_with_resolution` consume that map into the private interview seed. No
caller coordinates values and origins, and no later layer knows presets or config
precedence, per boundary-discipline and single-source-of-truth.

Defaults retain authorship until validation. An active template default is checked
after its full prompt constraints are evaluated and becomes a template fault;
configured and public-seed defaults remain recoverable. This is a bounded private
composite rather than another public fault hierarchy.

Planning and interview keep module-local fault values. One crate-private display
value owns the `template error in ...` text, so the contract is shared without
leaking plan paths into interview state. A `SubmissionTxn` prepares all answers,
runs the normal walker in effect-free probe mode, then either returns the original
pending state or commits once. This encodes atomic documents structurally, per
make-operations-idempotent.

`canonical_target` keeps its public `PathBuf` signature and returns the canonical
ancestor directly for an empty suffix. One command-boundary call feeds storage,
protocol, planning, and future Jinja context. The interface stays small while
hiding symlink, ancestor, lexical, separator, and legacy-storage behavior. The two
new public methods are deep: each replaces a multi-object coordination protocol
with one consuming operation.

## Synthesis decision

This isolated candidate selects bounded resolved-default and submission composites
as its base. It retains the existing public `Seed`, `Plan::build`, and
`canonical_target` signatures while adding consuming operations where provenance
must be enforced. No cross-candidate material is used in this candidate.

## Tradeoffs accepted

- We accept an opaque `Resolution` instead of a public mutable defaults field in
  exchange for making value/provenance drift unrepresentable.
- We accept a second, effect-free walk during rejected mixed documents in exchange
  for exact skip classification without a separately implemented skip oracle.
- We accept retaining the public `PathBuf` target carrier in exchange for no wrapper
  migration and one auditable normalization authority.
- We accept a legacy staged-key lookup in exchange for preserving staged interviews
  created with the trailing-separator spelling.
- We accept immediate rejection of unresolved known-invalid early answers in
  exchange for keeping known-invalid data out of accepted submissions.

## Alternatives considered

- Add `default_origins: IndexMap<Id, Origin>` beside `Seed.defaults`. This exposes
  synchronization to every caller and replay path; the interface is shallow and
  permits mismatched ids and origins.
- Put config provenance on public `RawAnswer` or `EvalError`. This leaks config
  layering into ordinary caller answers and makes transport values carry policy;
  it hides little and expands every construction site.
- Create a shared public `TemplateFault` used by interview and planning. This
  centralizes storage as well as formatting, but exposes unrelated module data and
  couples public error evolution. Module-local values with one formatter hide the
  same contract behind a smaller surface.
- Introduce `CanonicalTarget(PathBuf)`. The wrapper prevents arbitrary
  construction only if every consumer changes signatures. Here one command seam
  already owns construction, so the wrapper adds migration and pass-through methods
  without hiding more normalization behavior.
- Defer every invalid early answer until its question is active. This accepts a
  known-invalid durable submission and moves the error across documents and replay;
  callers lose immediate, answer-independent feedback.

## Open questions and risks

- Will the configured-default repair accept an opaque consuming `Resolution` in
  place of the provisional public flat field so that exact origin carriage is
  enforceable?
- Does any published crate caller construct `EvalError` or `Prompt` directly, so
  private internal additions need a compatibility constructor before release?
- Is the legacy staged-key alias required for the supported pre-release install
  base, or can verification prove that no persisted trailing-separator records need
  migration?
- Can the probe share the walker control flow without evaluating an expression
  whose dependencies include an unavailable rejected answer?

## Next implementation step

Implement the private `ResolvedDefaults`/`DefaultBank` constructors and route direct
start plus staged replay through them before changing validation or diagnostics.
