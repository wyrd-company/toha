---
relationships:
  realizes: toha
  references:
    - error-attribution
    - template-registry
    - jinja-context
---

# Configured-default producer composition re-grounding

## Evidence identities

- Integration base and merge base:
  `a62061fdce74b6b1a743c70565dd9fbeec2413bb`.
- Approved producer publication:
  `dfe7ba017ebef525310db8b8ab4ead58fae2d147`.
- Producer design SHA-256:
  `9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
- Original approved producer anchor:
  `067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
  `72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
- Read-only producer implementation evidence was inspected at
  `807ad9cc4b510f30dc927242c214a7201d54c72c`. That branch remains owned by the
  producer task and is evidence, not a dependency or an edit surface here.

The producer design is authoritative for the future composition. The integration
base still contains the predecessor flat runtime shape, so it is grounding only.

## Source-to-consumer trace

1. `config::ConfigEntry<T> { value, origin }` is the sole origin source. Config
   loading walks system, user, then local layers. Each winning per-key insertion
   replaces value and `ConfigOrigin` together in `presets` or
   `template_defaults` (`src/config.rs:38-47`, `196-250`).
2. `interview::configured_defaults` receives the selected formal name plus the
   two source-bearing maps. For each winning mapping it builds a private
   `MappingSite`; a preset reference also copies its winning `PresetSite`. It
   stores `ResolvedDefault { raw, origin }` in `Resolution` without creating a
   new merge or lookup authority (`src/interview.rs:950-1015` on the inspected
   producer branch).
3. Callers read `Resolution::warnings()` while the carrier is borrowed. The
   configured start and replay paths then consume the carrier. Only
   `into_flat_defaults()` deliberately discards `ConfiguredDefaultOrigin`.
4. `Resolution::start_with_context(self, template, now, context)` is the context
   sibling of producer-owned `Resolution::start`. It maps every private entry to
   `DefaultBankEntry::Configured` and creates private `InterviewSeed { now,
   defaults, context }`. It does not construct a public `Seed`.
5. The ordinary public `Interview::start(template, Seed { now, defaults,
   context })` route remains separate. It maps its raw values only to
   `DefaultBankEntry::Seed`. It does not claim or infer configured origin.
6. Current configured replay restores `InvocationContext` from the staged wire
   plus the producer-created `CanonicalTarget`, reads configured warnings, and
   calls `StagedRecord::replay_with_resolution(template, resolution, context)`.
   That method parses the recorded instant, calls `start_with_context`, and then
   replays accepted submissions. It never calls `replay_with_defaults` or
   `into_flat_defaults`.
7. `render_default` retrieves one default-bank entry. A configured entry carries
   its origin into `PreparedDefaultSource::Configured`; a constraint failure
   reports the winning mapping and optional preset site and leaves the question
   available for recovery (`src/interview.rs:423-441`, `554-612`, `1380-1402`
   on the inspected producer branch). Replay reconstructs the same configured
   bank before applying submissions, so fault attribution and recovery match a
   fresh configured start.

## Signatures that the context design composes with

```rust
impl Resolution {
    pub fn warnings(&self) -> &[String];

    pub fn start_with_context<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
    ) -> Result<Interview<'a>, EvalError>;

    pub fn into_flat_defaults(
        self,
    ) -> (IndexMap<Id, RawAnswer>, Vec<String>);
}

impl StagedRecord {
    pub fn replay_with_resolution<'a>(
        &self,
        template: &'a Template,
        resolution: Resolution,
        context: InvocationContext,
    ) -> Result<Interview<'a>, StagingError>;
}
```

The producer's existing `Resolution::start` remains its consuming no-context
entry. The context feature adds the sibling above. The replay method keeps its
configured-resolution identity and gains the restored context needed by the
current context contract.

## Failure and recovery trace

- A mapping or preset selection fault is reported by `configured_defaults`
  before a `Resolution` exists.
- The caller reports `warnings()` before consuming the successful result.
- A kind fault during resolution names the winning mapping/preset source.
- A constraint fault after start uses the origin stored in
  `DefaultBankEntry::Configured`, removes the invalid default from the prompt,
  and allows a submitted answer to recover.
- A replayed configured constraint fault follows the same route before accepted
  submissions are reapplied. Flattening at either consuming seam loses the only
  stored attribution and is therefore a falsifiable defect.
- The stage admission adapter maps only typed `TrustRequired` to
  `StagingError::EnvironmentTrustRequired`. Other admission faults stay as the
  typed source of `StagingError::EnvironmentAdmission`, preserving authored
  location and evaluator detail.

## Preserve, change, avoid, risk

Preserve:

- `ConfigEntry` as the sole origin source;
- private `ResolvedDefault` and configured default-bank storage;
- warnings-before-consumption ordering;
- the ordinary flat `Seed` route;
- producer-owned `CanonicalTarget`, `StagingError`, and `as_path()`; and
- all approved context, access, storage, replay, collision, and mutable-folder
  behavior.

Change:

- add `InvocationContext` to private `InterviewSeed`;
- add the consuming `Resolution::start_with_context` sibling; and
- pair configured replay with the restored context.

Avoid:

- destructuring configured `Resolution` to raw defaults;
- configured use of `into_flat_defaults`, `Seed`, or `replay_with_defaults`;
- a second origin map, origin reconstruction, resolver, or target normalizer;
  and
- any runtime or shared canonical edit in this design task.

Risk:

- a caller can accidentally choose a flat compatibility route because the raw
  route remains public. The named start and replay sole-kill tests must execute
  the plausible mutants and fail on the missing winning origin.
