---
relationships:
  realizes: toha
  references:
    - error-attribution
    - template-registry
    - jinja-context
---

# Configured-default producer composition

## Producer authority

The approved error-attribution contract is anchored at revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
Bob's approval accepts all recommendations, including the opaque canonical
target and removal of raw-path inputs from identity-sensitive consumers. The
published producer design is at `dfe7ba017ebef525310db8b8ab4ead58fae2d147`,
design SHA-256
`9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
Its implementation is integrated at epic revision
`8a19269d78f6da0738edb17e56310621f143923d`. The current design review base
and branch merge-base are `d33245065f80e3ed9a8c75ea6526e8a9f9386f21`.

The integrated target interface is
`canonical_target(&Path) -> Result<CanonicalTarget, StagingError>`, with
`CanonicalTarget::as_path() -> &Path`. `CanonicalTarget(PathBuf)` has a private
field and no unchecked constructor or `From<PathBuf>`. The factory is the only
normalizer. The wire keeps target text; consumers receive the factory-created
carrier. There is no `TargetError` in the approved design or integrated source.

This producer approval is distinct from Bob's earlier exact approval of the
Jinja-context design and stage-only amendment. It adds no context or environment
policy.

## Source-to-consumer trace

1. `config::ConfigEntry<T> { value, origin }` is the sole origin source.
   Config loading visits system, user, then local layers. Winning inserts keep
   the value and `ConfigOrigin` together in `presets` or `template_defaults`
   (`src/config.rs`, `ConfigEntry`, `load`; current source lines 38–47 and
   196–250).
2. `interview::configured_defaults` resolves the selected formal name and
   source-bearing maps. It records each mapping as a private `MappingSite`, and
   each referenced preset as a private `PresetSite`, then stores
   `ResolvedDefault { raw, origin }` in `Resolution` (`src/interview.rs`,
   `configured_defaults`; lines 950–1017).
3. The caller reads `Resolution::warnings()` before consuming the resolution.
   The producer's `Resolution::start` consumes the private entries into
   `DefaultBankEntry::Configured`. Only `into_flat_defaults()` deliberately
   removes their origins (`src/interview.rs`, `Resolution`; lines 863–946).
4. Context-bearing configured start is the consuming sibling
   `Resolution::start_with_context(self, template, now, context)`. It moves the
   same entries into the configured bank and places context in private
   `InterviewSeed`. It does not construct a public flat `Seed`.
5. The public ordinary `Seed { now, defaults, context }` route remains
   separate. Its values enter `DefaultBankEntry::Seed` and carry no configured
   origin.
6. At the inspected producer source, staged replay routes parse the recorded
   instant and replay submissions but do not yet carry invocation context
   (`src/staging.rs`, `StagedRecord::{replay,replay_with_defaults,
   replay_with_resolution}`; lines 295–331). The context design extends each
   public route with the same producer-created target used to load the record.
   Each route restores current or legacy context from the record before replay.
   Configured replay also reports warnings before consuming `Resolution`, calls
   `start_with_context`, and replays submissions without using
   `replay_with_defaults` or `into_flat_defaults`.
7. `render_default` preserves a configured entry's origin through preparation;
   a constraint failure names the winning mapping and optional preset source,
   removes the invalid default from the prompt, and permits answer recovery
   (`src/interview.rs`, `render_default`, `PreparedDefaultSource`; lines
   423–441, 554–612, and 1380–1402).
8. Command callers report warnings before configured start or replay. The
   terminal, staged continuation, and staged apply paths use
   `configured_defaults` and `replay_with_resolution` (`src/main.rs`; lines
   430–441, 509–518, 685–695, and 935–945). The plan and staging store consume
   `&CanonicalTarget` (`src/plan.rs`, `Plan::build`; `src/staging.rs`,
   `Store::{load,save,remove}`; `src/protocol.rs`, `Context::new`).

## Context composition signatures

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
    pub fn replay<'a>(
        &self,
        template: &'a Template,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError>;

    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: IndexMap<Id, RawAnswer>,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError>;

    pub fn replay_with_resolution<'a>(
        &self,
        template: &'a Template,
        resolution: Resolution,
        target: &CanonicalTarget,
    ) -> Result<Interview<'a>, StagingError>;
}
```

The context feature extends the producer's consuming composition seams. It does
not replace `Resolution`, create another origin store, or change the producer's
`StagingError` owner. All existing replay routes remain public. They take the
producer carrier and restore context inside `StagedRecord`, so external callers
do not need to construct `InvocationContext` or the private legacy mode. The
configured route retains configured identity and consumes `Resolution` without
flattening. Only the separate ordinary `Seed.defaults` route is flat.

## Failure and recovery

- Mapping and preset selection errors occur before a `Resolution` exists.
- Callers report `warnings()` before consuming a successful resolution.
- Kind errors identify the winning mapping or preset.
- Constraint errors after configured start retain that origin in
  `DefaultBankEntry::Configured`, remove the invalid default from the prompt,
  and allow a submitted answer to recover.
- Configured replay rebuilds the same origin-bearing bank before accepted
  submissions are reapplied.
- The stage adapter maps only typed `EnvironmentAdmissionError::TrustRequired`
  to `StagingError::EnvironmentTrustRequired`. Other admission faults retain
  their typed source, authored location, and evaluator detail.

## Preserve, change, avoid

Preserve `ConfigEntry` as the sole origin source, private configured storage,
warnings-before-consumption, the ordinary flat `Seed` route, the producer's
`CanonicalTarget`/`StagingError`/`as_path()` contract, and all approved context
and access behavior.

Add context to private `InterviewSeed`, add consuming
`Resolution::start_with_context`, and extend each public replay route with the
producer carrier so `StagedRecord` restores its own current or legacy context.

Do not flatten configured resolution, reconstruct origins, call
`replay_with_defaults` for configured replay, add another resolver or target
normalizer, or edit runtime/shared canonical files in this design task.

The ordinary flat API remains visible, so a caller could select the wrong route.
Named start/replay sole-kill checks prove that flattening loses the winning
origin and fails the expected attribution assertion.
