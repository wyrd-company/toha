---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Error attribution and early-answer behavior

## Caller's usage

### Integration assumption

The configured-defaults prerequisite supplies
`ConfigEntry<T> { value, origin }` for
each winning mapping and preset. `configured_defaults` is the only constructor
that projects those entries into an interview default. It creates one opaque
`Resolution` whose internal entries contain the typed answer, the winning mapping
origin, and the optional winning preset origin.

The provisional configured-defaults sketch exposes a flat
`Resolution.defaults` and suggests an
optional formatted `EvalError.config_key`. That shape cannot carry the exact
selected provenance into later constraint validation without a second sidecar or
path reconstruction. This design assumes its repair replaces that draft
carrier with the opaque `Resolution` below. It does not change the approved
`presets`, `{ preset: <name> }`, exact-`formal_name` selection, precedence, warning,
or diagnostic behavior. `ConfigEntry` remains the sole config-origin source.

### Library caller

The default path is one resolution followed by one consuming start operation.
Warnings remain available before the resolution is consumed.

```rust
let config = toha::config::load(&config_paths, &cwd)?;
let resolved = resolve_template(request, &config, &registry, &dirs, &cwd)?;
let template = Template::load(&resolved.folder)?;

let resolution = toha::interview::configured_defaults(
    &resolved.formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
for warning in resolution.warnings() {
    report_warning(warning);
}

let interview = resolution.start(&template, jiff::Zoned::now())?;
```

`Interview::start(&template, Seed { now, defaults })` remains public and unchanged
for callers that provide their own flat defaults. Such defaults retain their
existing id-based attribution. A configured-default caller uses `Resolution::start`
so provenance cannot be discarded accidentally.

### Staged and headless flow

Replay resolves live configuration once, constructs the same opaque default bank,
and replays accepted submissions through the same interview engine. The answers
document is still one atomic submission.

```rust
let target = toha::staging::canonical_target(requested_target)?;
let saved = store.load(&target)?.ok_or(NoStagedInterview)?;
let resolved = resume_template(&saved.template, &saved.commit, &scope)?;
let template = Template::load(&resolved.folder)?;
let resolution = toha::interview::configured_defaults(
    &resolved.formal_name,
    &template,
    &scope.config.presets,
    &scope.config.template_defaults,
)?;

let replayed = saved.replay_with_resolution(&template, resolution)?;
let result = toha::protocol::answer_headless(&template, replayed, document)?;
```

An invalid configured default remains recoverable. An omitted answer can reject
a headless submission with the exact mapping and optional preset hop; a later
`continue` document can provide a valid answer. The terminal driver uses
`Pending::check_default` when the person selects the default and reprompts with
the same attributed rejection.

### Completed-plan flow

The command boundary normalizes the target once. The same returned `PathBuf` is
the staged identity, protocol context value, plan target, and future Jinja target.

```rust
let target = toha::staging::canonical_target(requested_target)?;
let (interview, record) = start_or_replay(&target, request)?;
let completed = drive_to_completion(interview)?;

let context = protocol::Context {
    target: target.to_string_lossy().into_owned(),
    template: record.template.clone(),
    commit: record.commit.clone(),
};
let plan = toha::Plan::build(&template, &completed, &target)?;

// The future Jinja-context work receives `&target`.
// It does not normalize the path.
```

`canonical_target` retains its public `PathBuf` return. Audited ownership makes
it deep enough: `setup` is the only command boundary that calls it, and every
consumer accepts the returned path. No wrapper, second normalizer, or dependency
on the future Jinja-context work is introduced.

## Domain data and type sketch

### Resolved defaults are one bounded composite

```rust
// src/interview.rs

/// Configured defaults for one selected template. Construction is transactional:
/// no partially resolved bank escapes a missing-preset or kind failure.
pub struct Resolution {
    defaults: ResolvedDefaults,
    warnings: Vec<String>,
}

struct ResolvedDefaults(IndexMap<Id, ConfiguredDefault>);

struct ConfiguredDefault {
    /// Kind-checked by `configured_defaults`.
    answer: Answer,
    /// Derived once from the winning ConfigEntry values and origins.
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
    /// Existing public Seed.defaults behavior.
    Seed { id: Id, raw: RawAnswer },
    /// The configured-default projection has no flat copy or parallel origin map.
    Configured(ConfiguredDefault),
}

struct DefaultBank(IndexMap<Id, DefaultBankEntry>);

struct InterviewSeed {
    now: jiff::Zoned,
    defaults: DefaultBank,
}
```

`Resolution` does not expose a mutable flat map. A public flat map beside the
resolved bank could drift after construction. `Interview::start` converts the
existing public `Seed.defaults` into `DefaultBankEntry::Seed`; `Resolution::start`
consumes its bank directly. Both enter the same private constructor.

`ConfiguredDefaultOrigin` is a derived carrier, not an origin source. Its file
paths are cloned from the winning mapping and preset `ConfigEntry.origin` values.
Later code never consults config layers, resolves a preset, rebuilds a config file
path, or invents `template-defaults` keys from an id alone. Its formatter combines
the stored sites with the selected answer only when text is required.

### Defaults retain authorship until validation

```rust
struct PreparedPrompt {
    prompt: Prompt,
    default: Option<PreparedDefault>,
}

struct PreparedDefault {
    answer: Answer,
    source: PreparedDefaultSource,
}

enum PreparedDefaultSource {
    Template(TemplateDefaultSite),
    Configured(ConfiguredDefaultOrigin),
    Seed(Id),
}

struct TemplateDefaultSite {
    field: String,               // for example, `mode.default`
    expression: Option<String>,  // present for an authored expression/template
}
```

`Advance` stores private prepared defaults by question id beside the public batch;
`Prompt` and the interview protocol schema do not gain provenance fields. An active
template-authored default is validated in `make_prompt` after every constraint and
option for that question is evaluated. A failure is an `EvalError` at
`<id>.default`, not an answer rejection. A configured or caller-seeded default is
validated when it is taken and remains recoverable.

For typed defaults, `Typed::Literal` has no expression and `Typed::Expr` supplies
its exact source. A string default is a Jinja template, so its authored template
source is retained. Constraints of a skipped question are not evaluated: the
question is not offered as an answer point, and dynamic constraints need not be
ready. A template default is checked when its active question is materialized,
which is the first point where all of its literal and dynamic constraints are
known.

### Module-local faults share one formatting contract

```rust
// src/diagnostic.rs — crate-private rendering contract, not a public error type.
pub(crate) struct TemplateFaultDisplay<'a> {
    pub field: &'a str,
    pub expression: Option<&'a str>,
    pub message: &'a str,
}

impl std::fmt::Display for TemplateFaultDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `template error in <field> `<expression>`: <message>` when expression
        // exists; `template error in <field>: <message>` otherwise.
        unimplemented!()
    }
}

// src/plan.rs — planning owns its source path and field taxonomy.
struct PlanRenderFault {
    source_path: PathBuf,
    field: PlanField,
    expression: String,
    message: String,
}

enum PlanField {
    RuleWhen(usize),
    RuleEach(usize),
    RulePath(usize),
    OrdinaryTargetPath,
}

impl PlanField {
    fn display(&self) -> String {
        // files[i].when, files[i].each, files[i].path, or target path
        unimplemented!()
    }
}
```

`EvalError` remains the interview module's error and `PlanRenderFault` remains the
plan module's value. They adapt to `TemplateFaultDisplay` only for text. This keeps
question state out of planning and source paths out of interview errors while one
crate-private formatter owns the exact contract.

Only Jinja evaluation faults use this form. Structural `TargetPath::parse` errors,
I/O errors, and duplicate targets keep their existing `PlanError` variants.

### Answer submission is a transaction

```rust
struct SubmissionTxn<'a> {
    original: Pending<'a>,
    held: RawAnswers,
    valid_current: Answers,
    rejections: Vec<PendingRejection>,
    early_failures: IndexMap<Id, usize>, // id -> rejection position
}

enum SkipDisposition {
    Skipped,
    Active,
    Unresolved,
}

enum WalkEffects {
    Commit,
    Probe {
        unavailable_current: HashSet<Id>,
        classify: HashSet<Id>,
    },
}
```

The transaction borrows or owns the original `Pending` until its result is known.
The probe uses the same node-walk semantics as commit, but buffers no messages,
warnings, answers, hooks, or template faults. A rejected current answer is marked
unavailable. The probe can continue across unrelated nodes; an expression that
references an unavailable answer is unresolved. There is no separately implemented
skip oracle.

Only `SubmissionTxn::commit` consumes the original state. A rejected transaction
returns it unchanged, so no answer, warning, message, hook, or staged submission
escapes a rejected document.

## Public and crate-visible signatures

```rust
// Public, unchanged.
pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
}

// Public, unchanged.
impl Interview<'_> {
    pub fn start<'a>(
        template: &'a Template,
        seed: Seed,
    ) -> Result<Interview<'a>, EvalError> {
        unimplemented!()
    }
}

// Public configured-default boundary from the prerequisite design.
pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &IndexMap<PresetName, ConfigEntry<Value>>,
    mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
) -> Result<Resolution, EvalError> {
    unimplemented!()
}

impl Resolution {
    pub fn warnings(&self) -> &[String] {
        unimplemented!()
    }

    pub fn start<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
    ) -> Result<Interview<'a>, EvalError> {
        unimplemented!()
    }
}

impl StagedRecord {
    // Existing entry points remain for unattributed caller defaults.
    pub fn replay<'a>(&self, template: &'a Template)
        -> Result<Interview<'a>, StagingError> { unimplemented!() }

    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: IndexMap<Id, RawAnswer>,
    ) -> Result<Interview<'a>, StagingError> { unimplemented!() }

    // New provenance-preserving entry point.
    pub fn replay_with_resolution<'a>(
        &self,
        template: &'a Template,
        resolution: Resolution,
    ) -> Result<Interview<'a>, StagingError> { unimplemented!() }
}

// Public, unchanged and sole normalization authority.
pub fn canonical_target(path: &Path) -> Result<PathBuf, StagingError> {
    unimplemented!()
}

// Public, unchanged.
impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Plan, PlanError> { unimplemented!() }
}

// Crate-visible/private seams.
fn start_with_seed<'a>(
    template: &'a Template,
    seed: InterviewSeed,
) -> Result<Interview<'a>, EvalError> { unimplemented!() }

fn render_default(
    question: &Question,
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
) -> Result<Option<PreparedDefault>, EvalError> { unimplemented!() }

fn make_prompt(
    question: &Question,
    template: &Template,
    answers: &Answers,
    seed: &InterviewSeed,
) -> Result<PreparedPrompt, EvalError> { unimplemented!() }

impl<'a> Pending<'a> {
    pub fn check_default(&self, id: &Id) -> Result<Answer, CheckError> {
        unimplemented!()
    }

    pub fn answer(self, incoming: RawAnswers)
        -> Result<Interview<'a>, AnswerError<'a>> { unimplemented!() }
}

impl<'a> SubmissionTxn<'a> {
    fn prepare(pending: Pending<'a>, incoming: RawAnswers)
        -> Result<Self, EvalError> { unimplemented!() }
    fn classify_early(&mut self) { /* probe; no effects */ }
    fn finish(self) -> Result<Interview<'a>, AnswerError<'a>> {
        unimplemented!()
    }
}
```

The terminal adapter returns either `Provided(Value)` or `TakeDefault` from its
prompt. `TakeDefault` calls `check_default`, which applies the prepared default's
source-aware validation. After a successful check, the adapter submits the
concrete value through `answer`, so staged terminal submissions keep their current
replay behavior. A value explicitly typed by the person uses `check` and is caller
input even when it equals the displayed default.

## Module and seam diagram

```text
config.rs
  ConfigEntry(mapping value + winning mapping origin)
  ConfigEntry(preset value + winning preset origin)
       │ exact formal_name selection; derive once
       ▼
interview.rs
  configured_defaults ──► Resolution(ResolvedDefaults)
                              │ consume
          ┌───────────────────┴─────────────────────┐
          ▼                                         ▼
  Resolution::start                   StagedRecord::replay_with_resolution
          └───────────────────┬─────────────────────┘
                              ▼
                  InterviewSeed(DefaultBank)
                              │
        render default ──► PreparedDefault ──► constraint validation
                              │
        Pending::answer ──► SubmissionTxn ──► probe / reject or commit
                              │ complete
                              ▼
plan.rs                  Plan::build
  PlanRenderFault ─────────────────┐
interview.rs                       │
  EvalError ───────────────────────┴─► diagnostic.rs formatting contract

requested target
       │ exactly once
       ▼
staging::canonical_target -> separator-free PathBuf
       ├─ staged-state digest and StagedRecord.target
       ├─ protocol Context.target
       ├─ Plan::build target
       └─ future Jinja context input (consumer only)
```

The public surface adds two capability-bearing operations:
`Resolution::start` and `StagedRecord::replay_with_resolution`. They hide config
provenance carriage and replay setup. Callers do not coordinate a defaults map with
an origins map.

## Error and result contracts

### Planning render faults

Given a source support file at `/tmp/f`, the exact message bodies are:

```text
/tmp/f: template error in files[0].when `'x' | dateformat`: <message>
/tmp/f: template error in files[0].each `42`: expected array
/tmp/f: template error in files[0].path `{{ bad() }}`: <message>
/tmp/{{ bad() }}: template error in target path `{{ bad() }}`: <message>
```

For an ordinary source-tree file, `target path` is the synthetic plan-owned field;
the `PlanError::Render.path` prefix names the source entry and the backticked value
is the exact failing segment. For a file rule, the field is the exact
`files[i].<member>` and the expression is the authored expression or path template.

### Template-authored defaults

A literal list default that violates literal options is a template fault:

<!-- rumdl-disable MD013 -->

```text
template error in labels.default: default ["unknown"] is not allowed: each item must be one of: alpha, beta
```

<!-- rumdl-enable MD013 -->

An expression default that renders a disallowed value retains its source:

<!-- rumdl-disable MD013 -->

```text
template error in labels.default `suggested_labels`: default ["unknown"] is not allowed: each item must be one of: alpha, beta
```

<!-- rumdl-enable MD013 -->

A string default is a Jinja template, so even a constant string retains the
authored template source:

<!-- rumdl-disable MD013 -->

```text
template error in mode.default `legacy`: default "legacy" is not allowed: must be one of: compact, detailed
```

<!-- rumdl-enable MD013 -->

These faults abort evaluation. They are never placed in protocol `errors` and the
terminal cannot reprompt around them because the template is faulty.

### Configured defaults

Resolution-time kind and missing-preset failures retain the approved text:

```text
/tmp/config.yml: template-defaults."gh:example/forms".enabled: no preset named "missing"

/tmp/local.yml: template-defaults."gh:example/forms".enabled
→ /tmp/user.yml: presets."switch" ("yes"): must be true or false
```

A kind-valid configured value that later violates a question constraint is a
recoverable per-question rejection. Inline and referenced examples are:

<!-- rumdl-disable MD013 -->

```text
mode: default "legacy" from /tmp/local.yml: template-defaults."gh:example/forms".mode is not allowed: must be one of: compact, detailed

mode: default "legacy" from /tmp/local.yml: template-defaults."gh:example/forms".mode → /tmp/user.yml: presets."display_mode" ("legacy") is not allowed: must be one of: compact, detailed
```

<!-- rumdl-enable MD013 -->

The mapping file and site always lead. The preset hop appears only for
`{ preset: <name> }` and uses the winning preset's file. The `Resolution` entry
supplies both sites; later validation does not know config layers or presets.

The terminal's `check_default` prints the rejection and asks again. Headless
`apply` or `continue` returns the same batch with the rejection, records no part
of the rejected document, and accepts a later valid answer.

### Result invariants

- One rejected answers document records no answers, submissions, messages,
  warnings, or hooks.
- A probe result is not observable. A probe template fault is reported only when
  the document has no answer rejection and the real commit reaches the same fault.
- A skipped answer warns once, in interview order, only on an accepted step.
- Direct apply, staged continue, replay, terminal, headless, and library entry
  points use the same default bank and `Pending::answer` transaction.

## Early-answer state transitions and ordering

For each incoming id, preparation classifies its state before any commit:

| State at submission | Preparation |
| --- | --- |
| Unknown question | Reject in document order. |
| Already skipped | Hold only for a possible accepted-step warning. |
| Already answered | Ignore an equivalent value; otherwise reject. |
| Current batch | Validate with the complete current prompt in batch order. |
| Early question | Validate kind and every known literal constraint; hold the raw value. |

An early known failure is conditional until the probe returns:

| Probe disposition | Early failure policy |
| --- | --- |
| `Skipped` | Remove the rejection. If another rejection remains, emit no warning. If the document commits, normal walking emits one skipped warning. |
| `Active` | Keep the rejection. |
| `Unresolved` | Keep the rejection. |

The recommended policy for an early answer whose skip cannot yet be decided is to
reject the document now. The value already fails an answer-independent contract.
Accepting it would place known-invalid data in durable submissions and defer the
same failure to a later document or replay. The caller can resubmit the value when
the document also proves that the question is skipped.

The recommended policy beside a current-batch failure is to omit an invalid early
answer proven skipped by the same document. The probe uses every valid current
answer even when another current answer fails. A failed current answer is
unavailable; it cannot prove a skip. Thus:

```text
current `include_details = false` is valid
current `title = ""` fails required
early `detail_mode = "invalid"` fails literal options
`detail_mode.when` is `include_details`

result errors: title only
state change: none
messages/warnings: none
```

When the corrected document is accepted with `include_details = false` and the
same `detail_mode`, normal commit omits the answer and emits its skipped warning.
If the skip depends on the invalid `title`, the disposition is `Unresolved` and
both rejections stand.

Rejection order remains stable: incoming structural and early rejections retain
document order; current/default rejections retain batch order; classification only
removes entries. The transaction returns the original `Pending` when any rejection
remains. With no rejection, it removes current answers from held values and runs
one committing advance.

## Canonical target ownership

`canonical_target` keeps the established normalization algorithm. Its final step
handles an empty suffix without `PathBuf::join("")`:

```rust
let canonical_ancestor = ancestor.canonicalize()?;
let suffix = normalized.strip_prefix(ancestor).expect("ancestor is a prefix");
if suffix.as_os_str().is_empty() {
    Ok(canonical_ancestor)
} else {
    Ok(canonical_ancestor.join(suffix))
}
```

An existing non-root directory therefore has no trailing separator. A filesystem
root retains its root separator because that is the root representation, not an
empty joined component. Non-existing suffixes, `.` and `..` handling, and closest
existing ancestor symlink resolution remain unchanged.

Command composition calls this function once in `setup`. `record`, `Store`,
`context`, `Plan::build`, and future Jinja context accept that result. Tests audit
all command call sites and forbid another function that canonicalizes a target for
these consumers.

## Persistence, replay, and compatibility

`StagedRecord` keeps its JSON shape. It persists only target identity, template
identity, commit, frozen `now`, and accepted submissions. It does not persist the
default bank, early classification, derived interview state, or diagnostics.

Replay reloads live config, resolves winning `ConfigEntry` values into a fresh
`Resolution`, consumes it through `replay_with_resolution`, and submits every
accepted document through `Pending::answer`. Original execution and replay
therefore use the same classification and origin behavior. A config change can
change a configured default on replay, as specified by the prerequisite.

State files written with the former trailing-separator spelling remain readable.
`Store::load` checks the canonical separator-free digest first, then the one legacy
digest produced by the old empty-suffix spelling. The legacy candidate is a storage
key alias, not a target normalizer. A loaded legacy record receives the already
canonical target argument in memory. The next successful save writes the canonical
key and removes the legacy alias; `remove` checks both keys. New records and
protocol contexts use only the separator-free spelling.

Accepted submission compatibility is intentional: the recommended unresolved-skip
policy matches the existing specification. The current-batch noise policy changes
only which errors accompany a rejected document; it does not accept that document
or change its stored history.

## Proposed specification, schema, and guide impact

`docs/specifications/interview-protocol.yml` states:

- a known early failure is rejected when the question is active or unresolved and
  removed only when the same document proves the question skipped;
- classification uses valid current answers even when another current answer
  fails, so a proven-skipped early failure is omitted;
- warnings occur only on an accepted step;
- configured-default constraint failures name the exact selected mapping and
  optional preset hop and remain recoverable;
- canonical target strings contain no trailing separator except a filesystem root.

The sentence that makes every early failure stand whenever the batch fails is
replaced by the proven-skipped rule. The staged-state section documents legacy key
lookup and migration.

`docs/specifications/config.yml` and `docs/configuration.md` gain the later-
constraint example above. Their configured-default selector, preset, precedence,
and source rules do not change.

`docs/specifications/template-format` states that an active template-authored
default must satisfy its question's evaluated constraints and that failure is a
template fault at `<id>.default`. `docs/template-interviews.md` shows literal and
expression examples. `docs/template-files.md` documents the four field-attributed
render diagnostics.

No JSON Schema shape changes are required for the interview protocol or template
format. The config schema changes remain solely owned by the configured-defaults
prerequisite. The future Jinja-context work receives the canonical target as an
input contract; this design neither imports nor waits on that work.

## Falsifiable test scenarios

1. **File-rule attribution:** failing `files[0].when`, `.each`, and `.path` fixtures
   produce the exact field, exact source in backticks, source-path prefix, and
   engine message. The test fails on the old `files[0].when: ...` form.
2. **Ordinary target-path attribution:** a failing Jinja source-path segment reports
   `target path` and the exact segment. File content rendering remains unchanged.
3. **Template literal default fault:** an active literal list default outside its
   literal options returns an `EvalError` at `labels.default`, not a `Rejection`.
4. **Template expression default fault:** an expression default that evaluates to
   a constraint-invalid value reports the exact expression. A dynamic constraint
   is evaluated before this check. A skipped question does not evaluate that
   dynamic constraint.
5. **Configured inline recovery:** an inline, kind-valid configured default fails
   a later constraint with the winning mapping file/site. Terminal replacement
   through `check_default` and a later headless document both succeed.
6. **Configured preset recovery:** mapping and preset winners come from different
   files. Later rejection names both exact origins and the preset value. Changing
   precedence changes both winner and reported file together.
7. **Resolution construction is atomic:** a missing preset or kind failure returns
   no usable `Resolution`; no default bank can be partially started.
8. **Unresolved early failure:** an early value fails literal options and its skip
   depends on a future answer. The current document is rejected and no submission
   is stored.
9. **Same-document skipped early failure:** valid current answers prove the early
   question skipped. Its rejection is removed and the accepted step emits exactly
   one warning in interview order.
10. **Current failure plus proven skip:** one current answer fails, another valid
    current answer proves the invalid early answer skipped. Only the current error
    is returned; state and messages are unchanged.
11. **Current failure cannot prove skip:** the skip expression references the
    failed current answer. Disposition is unresolved and both errors remain.
12. **Probe isolation:** a probe traverses a message, hook, and failing template
    expression while another rejection remains. None is emitted or stored. With
    the rejection corrected, the real commit reaches the normal effect or fault
    once.
13. **Apply/continue equivalence:** for active, skipped, and unresolved early
    questions, with and without a current-batch failure, direct `apply --answers`
    and `stage --async` plus `continue --answers` produce identical status, error
    maps, messages, accepted submissions, final answers, and output trees.
14. **Replay equivalence:** replay of every accepted prefix in the equivalence
    matrix produces the same next batch or completion as uninterrupted execution.
    Rejected documents never appear in the record.
15. **Canonical existing target:** an existing directory normalizes to its
    canonical absolute spelling without a trailing separator and yields that exact
    protocol context and plan target.
16. **Canonical non-existing target:** a path with `.` and `..` below a symlinked
    existing ancestor resolves once and has the same spelling before and after the
    leaf directory is created.
17. **Legacy staged identity:** a record stored under the old trailing-separator
    digest loads through the separator-free target, replays, and moves to the new
    key on the next successful save.
18. **Single target authority audit:** staged identity, context, plan, and the
    future context adapter receive the same `PathBuf`; repository search finds no
    second target canonicalizer.
