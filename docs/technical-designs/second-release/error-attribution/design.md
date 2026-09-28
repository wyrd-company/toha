---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Error attribution and early-answer behavior

Toha reports template faults at the field that owns the authored value, keeps
the winning configured-default origin until every constraint has passed, treats
one answers document as one transaction, and uses one constructed target
identity in storage, protocol, planning, apply, and Jinja context.

## Caller's usage

### Configured library caller

Configured resolution retains exact origins. Warnings are read before the
resolution is consumed by the source-aware start operation.

```rust
let resolution = toha::interview::configured_defaults(
    &resolved.formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
for warning in resolution.warnings() {
    report_warning(warning);
}

let interview = resolution.start(&template, clock.now())?;
```

The existing flat seed remains available for application-supplied defaults:

```rust
let interview = Interview::start(
    &template,
    Seed {
        now: clock.now(),
        defaults: application_defaults,
    },
)?;
```

Flat seed defaults keep their existing id-based recovery behavior. They do not
claim config provenance.

### Staged and headless caller

One target construction precedes storage and replay. Replay re-resolves live
configured defaults and consumes their origins with their values.

```rust
let target = staging::canonical_target(requested_target)?;
let saved = store.load(&target)?.ok_or(NoStagedInterview)?;
let resolution = configured_defaults(
    &saved.template,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
for warning in resolution.warnings() {
    report_warning(warning);
}

let interview = saved.replay_with_resolution(&template, resolution)?;
let result = protocol::answer_headless(&template, interview, document)?;
```

A constraint-invalid configured default appears as a recoverable error on its
question. A supplied answer replaces it. A rejected document adds no staged
submission.

### Completed-plan caller

The same target value crosses every identity-sensitive interface.

```rust
let target = staging::canonical_target(requested_target)?;
let plan = Plan::build(&template, &completed, &target)?;
let context = protocol::Context::new(&target, &record);
let outcome = plan.apply(&target, options, runner)?;
```

The Jinja-context component receives `&CanonicalTarget`. It reads the path and
does not normalize it.

## Product behavior

### Template faults name the authored field

Jinja compilation or evaluation failures use one text contract:

```text
template error in <field> `<expression>`: <message>
```

The backticked expression is omitted only when the authored value has no
expression source, such as a typed literal default.

Planning uses these fields:

- `files[i].when` with the exact `when` source;
- `files[i].each` with the expression before the binding;
- `files[i].path` with the exact path-template source;
- `path` with the exact ordinary source-path segment being rendered.

`PlanError` retains the support or source path before the template-fault text.

<!-- rumdl-disable MD013 -->

```text
/tmp/source/item.txt: template error in files[0].when `'bad' | dateformat`: invalid operation
/tmp/source/item.txt: template error in files[0].each `42`: expected array
/tmp/source/item.txt: template error in files[0].path `{{ item | dateformat }}.txt`: invalid operation
/tmp/source/{{ label | dateformat }}/note.txt: template error in path `{{ label | dateformat }}`: invalid operation
```

<!-- rumdl-enable MD013 -->

### Template defaults are template-owned

An active template-authored default is validated after the question's options
and constraints are rendered. A failure terminates as a template fault. It never
enters protocol `errors`, never becomes caller input, and never causes a
terminal retry.

<!-- rumdl-disable MD013 -->

```text
template error in labels.default: default ["unknown"] is not allowed: each item must be one of: alpha, beta
template error in labels.default `suggested_labels`: default ["unknown"] is not allowed: each item must be one of: alpha, beta
```

<!-- rumdl-enable MD013 -->

A skipped question validates its template-authored default against every
constraint whose references are already ready. It does not block the skipped
branch to render a dynamic constraint whose references are unavailable.
Presentation fields are not rendered for a skipped question.

Configured and application-supplied defaults on skipped questions retain their
current skip behavior. There is no prompt at which a caller can recover from a
constraint error. Their answer kind has already been checked at their owning
boundary.

### Configured defaults keep exact provenance

The configured-default resolver remains the only place that knows config layers,
exact template selection, `presets`, or `{ preset: <name> }`. It derives one
configured-default entry from the winning mapping `ConfigEntry` and, for a
reference, the winning preset `ConfigEntry`.

A missing preset or answer-kind mismatch remains a fatal resolution fault. A
value that has the correct answer kind but fails a question constraint is
recoverable.

<!-- rumdl-disable MD013 -->

```text
mode: default "legacy" from /tmp/local.yml: template-defaults."gh:sample/forms".mode is not allowed: must be one of: compact, detailed

mode: default "legacy" from /tmp/local.yml: template-defaults."gh:sample/forms".mode → /tmp/user.yml: presets."display_mode" ("legacy") is not allowed: must be one of: compact, detailed
```

<!-- rumdl-enable MD013 -->

The invalid configured value is removed from `Prompt.default`. Its attributed
rejection is placed in the batch. The existing terminal prints the error and
asks; headless callers receive the same error. An explicit answer takes
precedence over the carried error.

### Early known failures are conditional on reachability

An early answer is checked immediately against its answer kind and every literal
constraint already known. A known failure stays held while an effect-free probe
classifies its question:

| Probe result | Result for the early failure |
| ------------ | ---------------------------- |
| Active       | Keep the rejection.          |
| Skipped      | Remove the rejection.        |
| Unresolved   | Keep the rejection.          |

An unresolved known failure rejects the document. The value already violates an
answer-independent rule and does not enter accepted staged history.

A failure proven skipped by the same document is removed even when another
current answer fails. The remaining error rejects the document. The rejected
probe emits no warning. A corrected document that commits emits the
skipped-answer warning once in interview order.

```text
valid current selector: include_details = false
invalid current answer: title = ""
invalid early answer: detail_mode = "unknown"
detail_mode.when: include_details

result: title error only
state change: none
warnings: none
```

If the skip depends on the invalid current answer, the early question is
unresolved and both errors remain.

### Canonical target is one constructed identity

`canonical_target` creates an absolute target, removes `.` and lexically
resolves `..`, canonicalizes the closest existing ancestor, and appends only a
non-empty suffix. An existing non-root directory therefore has no redundant
trailing separator. A filesystem root keeps the separator required to represent
the root.

`CanonicalTarget` has no public unchecked constructor and no `From<PathBuf>`.
Storage, staged records, protocol context, planning, apply, and Jinja context
require this value. Crate callers cannot pass an arbitrary path to an
identity-sensitive operation.

## Data structures

### Configured resolution and interview defaults

```rust
/// The result of configured-default selection for one exact template identity.
pub struct Resolution {
    defaults: ResolvedDefaults,
    warnings: Vec<String>,
}

struct ResolvedDefaults(IndexMap<Id, ResolvedDefault>);

struct ResolvedDefault {
    raw: RawAnswer,
    origin: ConfiguredDefaultOrigin,
}

struct ConfiguredDefaultOrigin {
    mapping: MappingSite,
    preset: Option<PresetSite>,
}

struct MappingSite {
    origin: ConfigOrigin,
    formal_name: String,
    id: Id,
}

struct PresetSite {
    origin: ConfigOrigin,
    name: PresetName,
}

enum DefaultBankEntry {
    Seed(RawAnswer),
    Configured(ResolvedDefault),
}

struct InterviewSeed {
    now: jiff::Zoned,
    defaults: IndexMap<Id, DefaultBankEntry>,
}
```

`ConfigOrigin` and `ConfigEntry<T>` belong to config merging. `ResolvedDefault`
copies their winning origins once and carries no merge or selection behavior.
Value and origin cannot drift because one map entry owns both.

`Resolution::into_flat_defaults` is the explicit escape for a crate caller that
wants the existing flat seed behavior. It returns the warnings and raw map and
intentionally discards configured provenance.

### Prepared defaults

```rust
struct PreparedDefault {
    answer: Answer,
    source: PreparedDefaultSource,
}

enum PreparedDefaultSource {
    Template(TemplateDefaultSite),
    Configured(ConfiguredDefaultOrigin),
    Seed,
}

struct TemplateDefaultSite {
    field: String,
    expression: Option<String>,
}

struct PreparedPrompt {
    prompt: Prompt,
    configured_rejection: Option<Rejection>,
}
```

These values are private. Protocol `Prompt` and the interview protocol schema do
not gain provenance fields.

### Template fault formatting

```rust
pub(crate) struct TemplateFault {
    field: String,
    expression: Option<String>,
    message: String,
}

impl std::fmt::Display for TemplateFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}
```

Interview adapts this value into its existing `EvalError` fields. Planning
adapts it into `PlanError::Render.message` and retains its source path. The
value stays crate-private, so the public error shapes do not gain a shared
module dependency.

### Answer transaction

```rust
struct SubmissionTxn<'a> {
    original: Pending<'a>,
    held: RawAnswers,
    valid_current: Answers,
    rejections: Vec<PendingRejection>,
    early_failures: IndexMap<Id, usize>,
}

enum SkipDisposition {
    Active,
    Skipped,
    Unresolved,
}
```

The transaction owns the original `Pending` until it either returns that state
or commits one real advance. Its probe uses the normal walker on a clone and
publishes no effect.

### Target identity

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalTarget(PathBuf);

impl CanonicalTarget {
    pub fn as_path(&self) -> &Path { unimplemented!() }
}

impl std::fmt::Display for CanonicalTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}
```

The serialized `StagedRecord.target` remains a path string. Deserialization does
not construct a trusted target. `Store::load` receives the already constructed
target and validates the stored value against it.

## Public and crate-visible interfaces

```rust
// Existing application-default route, unchanged.
pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
}

impl Interview<'_> {
    pub fn start<'a>(
        template: &'a Template,
        seed: Seed,
    ) -> Result<Interview<'a>, EvalError>;
}

// Configured-default boundary. The approved selector and ConfigEntry inputs stay.
pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &IndexMap<PresetName, ConfigEntry<Value>>,
    mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
) -> Result<Resolution, EvalError>;

impl Resolution {
    pub fn warnings(&self) -> &[String];

    pub fn start<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
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
    ) -> Result<Interview<'a>, StagingError>;

    // Existing replay and replay_with_defaults remain available.
}

pub fn canonical_target(
    path: &Path,
) -> Result<CanonicalTarget, StagingError>;

impl Store {
    pub fn path_for(&self, target: &CanonicalTarget) -> PathBuf;
    pub fn load(
        &self,
        target: &CanonicalTarget,
    ) -> Result<Option<StagedRecord>, StagingError>;
    pub fn remove(&self, target: &CanonicalTarget) -> Result<bool, StagingError>;
}

impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &CanonicalTarget,
    ) -> Result<Self, PlanError>;

    pub fn apply(
        self,
        target: &CanonicalTarget,
        options: ApplyOptions,
        runner: &dyn HookRunner,
    ) -> Result<Applied, ApplyError>;
}

impl Context {
    pub(crate) fn new(
        target: &CanonicalTarget,
        record: &StagedRecord,
    ) -> Self;
}
```

`Plan::apply_reporting`, staged save/new helpers, command composition, and the
Jinja-context constructor accept `&CanonicalTarget` at the same boundary.
Internal filesystem calls receive `target.as_path()`.

## Module and seam map

```text
config.rs
  winning mapping ConfigEntry ─┐
  winning preset ConfigEntry ──┴─► interview::configured_defaults
                                      │
                                      ▼
                              Resolution / ResolvedDefaults
                               │                    │
                               │ start              │ replay
                               ▼                    ▼
                            Interview ◄────── StagedRecord
                               │
answers document ─────────► SubmissionTxn
                               │ probe then reject or commit once
                               ▼
                            Completed
                               │
                               ▼
template + CanonicalTarget ─► Plan::build ─► Plan::apply

requested path ─► staging::canonical_target ─► CanonicalTarget
                                                ├─ Store identity
                                                ├─ StagedRecord target
                                                ├─ protocol Context.target
                                                ├─ Plan and apply
                                                └─ Jinja context

interview fault ─┐
plan fault ──────┴─► crate-private TemplateFault formatter
```

`interview` owns default and answer policy. `config` owns selection and merge
origins. `staging` owns live target construction and legacy storage lookup.
`plan` owns file-rule and source-path taxonomy. Protocol and terminal adapters
serialize or display results without adding policy.

## Submission ordering and atomicity

`Pending::answer` performs these steps:

1. Classify incoming ids as unknown, already skipped, already answered, current,
   or early. Check known rules for early answers and retain their raw values.
2. Check every current answer in batch order. Record only successfully checked
   values in the probe answer context. A failed current answer is unavailable.
3. When an early failure exists, run one effect-free walk on cloned state. The
   walker sees valid current values and held early values. It cannot use a
   rejected current value to evaluate an expression.
4. Classify each failed early id from the probe's skipped set and next batch.
   Remove only failures proven skipped; active and unreached ids remain.
5. If any rejection remains, return the original `Pending`. Discard probe
   answers, messages, warnings, hooks, skip markers, batches, and template
   faults.
6. If no rejection remains, run one real advance. This commit emits normal
   warnings and effects or returns the template fault reached by the accepted
   document.

Filtering does not reorder surviving rejections. Incoming structural and early
errors retain document order. Current/default errors retain batch order.

## Persistence and compatibility

Staged records continue to persist target text, template identity, commit,
frozen `now`, and accepted submissions. They do not persist configured defaults,
provenance, prepared prompts, rejections, or reachability.

Replay reloads live config and creates a new `Resolution`. It runs every
accepted submission through the same `Pending::answer` transaction. Direct and
replayed behavior therefore share the same default and early-answer policy.

The canonical state key is the digest of the separator-free target. On load:

1. Check the canonical key.
2. If absent, check one legacy key made from the same canonical non-root path
   text plus one native separator.
3. Accept the legacy record only when `canonical_target(record.target)` equals
   the requested target.
4. After the next successful canonical atomic save, remove the legacy key.

The legacy key is a storage alias. It does not construct a second live target
identity. Filesystem roots do not gain or lose their required separator.

## Proposed canonical-document changes

The paired implementation updates:

- `docs/specifications/template-format.yml` with field-attributed file/path
  faults and template-default constraint faults;
- `docs/specifications/interview-protocol.yml` with the early-answer matrix,
  configured-default recovery/provenance, probe atomicity, and separator-free
  targets;
- `docs/specifications/interview-protocol.schema.yml` descriptions for target
  and error semantics, with no wire-shape change;
- `docs/template-files.md`, `docs/template-interviews.md`, and
  `docs/configuration.md` with exact examples;
- `docs/technical-designs/architecture.yml` with the configured-resolution and
  canonical-target ownership seams.

The config schema remains owned by configured-default implementation. The
template format and protocol schemas need no structural change.

## Behaviors to prove

1. `files[0].when`, `.each`, and `.path` failures include exact field, authored
   source, evaluator text, and support-file prefix.
2. An ordinary rendered source-path segment reports field `path`, that exact
   segment, and the descendant source path. Content faults stay unchanged.
3. The same completed interview reports byte-equal planning faults through
   direct apply, staged continue plus staged apply, and replay.
4. Literal, expression, and string-template defaults that violate ready
   constraints are template faults. They never enter protocol errors or a
   terminal retry.
5. A skipped template default fails a ready literal constraint, does not render
   presentation fields, and does not force an unavailable dynamic constraint.
6. A configured inline value with the correct kind but an invalid constraint is
   a recoverable rejection with the winning mapping file and site. Replacement
   works through terminal, headless direct apply, continue, staged apply, and
   crate use.
7. A referenced configured value whose mapping and preset win from different
   files reports both exact origins and the preset value. Precedence changes
   value and origin together.
8. A missing preset and wrong-kind preset remain fatal resolution errors. No
   partial `Resolution` escapes.
9. Every supported command path surfaces configured-resolution warnings before
   consuming the resolution.
10. An active invalid early answer rejects; a skipped one is removed; an
    unresolved one rejects.
11. With a separate current failure, a valid current selector can prove an early
    question skipped. Only the current error remains and no warning is emitted.
12. A failed current answer cannot prove a skip. Both errors remain.
13. A probe crossing a message, hook, or template fault leaks no effect when the
    document is rejected. The corrected document reaches each effect once.
14. The complete early matrix has equal exit status, errors, messages, accepted
    submissions, completed answers, and output tree through direct apply, async
    stage plus continue, staged apply, and direct crate use.
15. Replay of every accepted prefix equals uninterrupted execution. Rejected
    documents never appear in the record.
16. Existing, non-existing, relative, parent-relative, symlink-ancestor, and
    root targets have stable canonical spelling with no redundant separator.
17. Store key, record target, protocol context, plan, apply, and Jinja context
    all observe the same `CanonicalTarget`. No consumer calls filesystem
    canonicalization.
18. A legacy separator-keyed record loads, replays, saves under the canonical
    key, and removes the legacy key only after successful save.
19. Compile-fail tests prove raw `PathBuf` cannot enter store, record, protocol,
    plan, apply, or Jinja target interfaces.
20. Existing flat `Seed` callers compile and retain ordinary id-based default
    behavior. `into_flat_defaults` deliberately drops configured provenance.

## Out of scope

- Configured-default selection, naming, precedence, one-hop references,
  migration, and config schema.
- New protocol fields or status values.
- Trust, permissions, access, hooks, timeouts, pinned-version checks, or
  application subprocess integration.
- A second Jinja target normalizer or an implementation dependency on
  Jinja-context work.
- General error-code or localization redesign.

## Size and complexity

Expected implementation time is one to two focused agent days after approval and
base refresh. Complexity is moderate: the change touches public target
signatures, configured start/replay wiring, prompt preparation, one
transactional answer path, storage compatibility, canonical documents, and
cross-route tests. It adds no new dependency, concurrency, protocol shape, or
runtime service.
