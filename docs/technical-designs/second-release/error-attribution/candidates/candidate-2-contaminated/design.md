---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Error attribution and early-answer behavior

## Caller's usage

The public surface keeps interview policy inside the interview engine and target
identity inside `staging::canonical_target`. Callers resolve configured defaults
once, construct one seed, and use the same seed contract for a new interview or
a replayed interview.

### Library caller

```rust
use toha::{Interview, RawAnswer, RawAnswers, Seed, Template};
use toha::interview::configured_defaults;

let template = Template::load(template_root)?;
let resolved = configured_defaults(
    &formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;

// `resolved.defaults` contains each value and its selected 1073 origin as one
// value. The caller does not coordinate a value map with an origin map.
let seed = Seed::new(frozen_now, resolved.defaults);
let interview = Interview::start(&template, seed)?;

let Interview::Asking(pending) = interview else { unreachable!() };
let mut answers = RawAnswers::new();
answers.insert(Id::parse("label")?, RawAnswer(json!("sample")));
let next = pending.answer(answers)?;
```

`Pending::answer` returns the same
`AnswerError::Rejected { pending, rejections }` recovery surface as before. A
constraint-invalid configured default is a recoverable rejection on its
question. A template-authored default that violates the template's own
constraint returns `AnswerError::Eval`.

### Staged headless flow

```rust
use toha::protocol;
use toha::staging::{canonical_target, Store};

let target = canonical_target(requested_target)?;
let store = Store::new(state_dir);
let saved = store.load(&target)?.expect("staged interview");

let resolved_defaults = configured_defaults(
    &saved.template,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
let interview = saved.replay_with_defaults(&template, resolved_defaults.defaults)?;
let answers = protocol::parse_answers(document)?;
let result = protocol::answer_headless(&template, interview, answers)?;
let context = protocol::Context::new(&target, &saved.template, saved.commit());
```

The staged record persists accepted raw submissions, not derived prompt state,
rejections, early-answer classifications, or rendered origin text. Replay reads
live configured defaults, reconstructs the origin-bearing seed, and submits the
accepted documents through `Pending::answer` again.

### Completed-plan flow

```rust
use toha::{Interview, Plan};
use toha::staging::canonical_target;

let target = canonical_target(output)?;
let Interview::Complete(completed) = drive(interview)? else {
    return Err("interview still has questions".into());
};

// Planning consumes the already canonical target. It cannot normalize a second
// time or receive an arbitrary PathBuf.
let plan = Plan::build(&template, &completed, &target)?;
let outcome = plan.apply(&target, options, runner)?;
```

`Plan::build` reports the source path and the exact template field and source
for file-rule and ordinary target-path render faults before trust or writes.

### Terminal recovery

```rust
for prompt in pending.batch().prompts() {
    if let Some(rejection) = pending.batch().error_for(&prompt.id) {
        eprintln!("{rejection}");
    }
    let raw = terminal.ask(prompt)?;
    match pending.check(&prompt.id, raw) {
        Ok(_) => submission.insert(prompt.id.clone(), raw),
        Err(CheckError::Rejected(error)) => terminal.retry(error)?,
        Err(CheckError::Eval(error)) => return Err(error.into()),
    };
}
```

An invalid configured default is carried as a batch error and is not exposed as
the prompt's usable default. The terminal names the exact config origin and asks
for a replacement. A headless document can replace it in the same submission.

## Integration assumption and prerequisite conflict

The repaired configured-defaults design supplies one prerequisite-owned resolved
default type that couples `RawAnswer` to the winning mapping origin and optional
preset hop. `Resolution.defaults` and `Seed.defaults` carry that type. Its
origin formatter produces the approved paths:

```text
<mapping-file>: template-defaults."<formal-name>".<id>
<mapping-file>: template-defaults."<formal-name>".<id>
  → <preset-file>: presets."<preset-name>" (<value>)
```

This is the only integration assumption. The approved configured-defaults design
still sketches a flat `IndexMap<Id, RawAnswer>` and an optional rendered
`EvalError.config_key`; that internal sketch cannot retain the selected origin
through later dynamic constraint validation. Its stated review repair replaces
that internal carriage while preserving `presets`, `{ preset: <name> }`, exact
`formal_name` selection, precedence, and diagnostic text. This design consumes
the repaired carrier. It does not define a sidecar origin map, reconstruct an
origin from an id, or retain `config_key` as a second source of truth.

## Domain shape

### Template faults

```rust
/// A fault in one authored template field. `expression` is the exact compiled
/// source. A typed literal default uses its canonical JSON spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFault {
    field: String,
    expression: String,
    message: String,
}

impl TemplateFault {
    pub(crate) fn new(
        field: impl Into<String>,
        expression: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        unimplemented!()
    }

    pub fn field(&self) -> &str { unimplemented!() }
    pub fn expression(&self) -> &str { unimplemented!() }
    pub fn message(&self) -> &str { unimplemented!() }
}

impl std::fmt::Display for TemplateFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // template error in <field> `<expression>`: <message>
        unimplemented!()
    }
}
```

`TemplateFault` is the single field/source/message representation. Interview
evaluation errors and planning errors wrap it with their own location data:

```rust
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("{fault}")]
    Template {
        id: Option<Id>,
        fault: TemplateFault,
    },

    // Constructed by configured-default resolution from the same prerequisite-
    // owned origin that enters Seed. Missing-preset and kind failures are fatal.
    #[error("{origin}: {message}")]
    ConfiguredDefault {
        id: Id,
        origin: ConfiguredDefaultOrigin,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{source}: {fault}")]
    Template {
        source: PathBuf,
        fault: TemplateFault,
    },
    // Existing Io, Path, Duplicate, and unattributed file-content UTF-8 variants.
    // ...
}
```

`ConfiguredDefaultOrigin` in the sketch is the opaque type owned by the repaired
configured-defaults prerequisite. `interview` can clone and display it. It
cannot inspect config layers, selectors, presets, or merge rules.

### Defaults at the interview boundary

```rust
// Supplied by the repaired configured-defaults prerequisite. These declarations
// show the consumed contract; design 1072 does not create another carrier.
pub struct ConfiguredDefault {
    raw: RawAnswer,
    origin: ConfiguredDefaultOrigin,
}

impl ConfiguredDefault {
    pub fn raw(&self) -> &RawAnswer { unimplemented!() }
    pub fn origin(&self) -> &ConfiguredDefaultOrigin { unimplemented!() }
}

pub struct ResolvedDefaults(IndexMap<Id, ConfiguredDefault>);

#[derive(Debug, Clone)]
pub struct Seed {
    now: jiff::Zoned,
    defaults: ResolvedDefaults,
}

impl Seed {
    pub fn new(now: jiff::Zoned, defaults: ResolvedDefaults) -> Self {
        unimplemented!()
    }

    pub fn without_configured_defaults(now: jiff::Zoned) -> Self {
        unimplemented!()
    }
}
```

The value and its origin move, clone, and merge together. A state with a
configured value but no origin is not constructible. `Seed` remains pure data.

Default preparation distinguishes template authorship from configuration before
the prompt loses that distinction:

```rust
enum RenderedDefault<'a> {
    Template {
        answer: Answer,
        field: TemplateField<'a>,
    },
    Configured(&'a ConfiguredDefault),
}

struct PreparedPrompt {
    prompt: Prompt,
    configured_default_rejection: Option<Rejection>,
}

enum DefaultCheck {
    Valid(Answer),
    Recoverable(Rejection),
}

fn validate_rendered_default(
    question: &Question,
    rendered: RenderedDefault<'_>,
    rules: &Rules<'_>,
) -> Result<DefaultCheck, EvalError> {
    // A failed Template variant becomes TemplateFault(question.default).
    // A failed Configured variant uses its opaque selected origin in Rejection.
    unimplemented!()
}

fn prepare_prompt(
    question: &Question,
    template: &Template,
    answers: &Answers,
    seed: &Seed,
) -> Result<PreparedPrompt, EvalError> {
    // 1. Render the prompt and every constraint whose dependencies are ready.
    // 2. Render/parse the selected default.
    // 3. Pass the default and Rules::of_prompt to validate_rendered_default.
    // 4a. Template default failure -> EvalError::Template(question.default).
    // 4b. Configured default failure -> attributed Rejection and prompt.default=None.
    // 5. A valid default becomes prompt.default.
    unimplemented!()
}
```

When a skipped question takes a template-authored default, the skip path
evaluates only the default-related rules and calls `validate_rendered_default`;
it does not render the prompt's presentation fields. A bad literal or expression
default is therefore a template fault even when the question is skipped. A
skipped configured default does not become a recoverable prompt error because
the caller never has to answer that question; its kind has already passed
configured-default resolution.

Typed literal defaults use their canonical JSON value as `expression`. A string
template uses its exact template source. This keeps one diagnostic form without
requiring the loader to retain YAML trivia.

### Early-answer classification

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reachability {
    Active,
    Skipped,
    Undecidable,
}

struct SubmissionProbe<'a> {
    proposed: Result<Interview<'a>, EvalError>,
    reachability: IndexMap<Id, Reachability>,
}

impl Pending<'_> {
    fn probe_submission(
        &self,
        known_good_current: &Answers,
        early_failures: &[Id],
    ) -> SubmissionProbe<'_> {
        // Clone accepted engine state. Evaluate the same compiled question and
        // ancestor-group guards used by Advance. A guard is evaluable only when
        // every referenced value is already accepted or is a successfully checked
        // current answer. A rejected value never enters this context. Do not emit
        // messages, warnings, hooks, submissions, or staged writes.
        unimplemented!()
    }
}
```

The guard evaluator is extracted from `Advance` and shared by normal advancement
and the probe. There is one implementation of dependency readiness and boolean
`when` evaluation. `Undecidable` means that no false guard proves the question
skipped and at least one required guard dependency is absent.

The recommended policies are:

1. A known literal-constraint failure on an early answer is rejected when its
   reachability is `Active` or `Undecidable`. It is removed only when
   reachability is `Skipped`. This preserves validation on submission and
   prevents accepted staged submissions from containing a value already known to
   be invalid.
2. A known-invalid early answer is removed when the same document proves its
   question `Skipped`, even when another current-batch answer fails. The
   remaining current-batch failure rejects the document. No skipped-answer
   warning is emitted because the rejected document commits no step. A corrected
   document that again supplies the skipped answer emits the warning when that
   document is accepted.

The second rule uses only successfully checked current answers. It never proves
a skip from a value that the same document rejects.

### Canonical target

```rust
/// Absolute target identity with no redundant trailing separator. The filesystem
/// root retains the separator required to represent the root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalTarget(PathBuf);

/// The only public constructor. It makes the input absolute, removes `.` and
/// lexically resolves `..`, canonicalizes the closest existing ancestor, and
/// appends each non-existing suffix component. If the suffix is empty, it returns
/// the canonical ancestor directly instead of `ancestor.join("")`.
pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError> {
    unimplemented!()
}

impl CanonicalTarget {
    pub fn as_path(&self) -> &Path { unimplemented!() }
}

impl std::fmt::Display for CanonicalTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}
```

`CanonicalTarget` has no public unchecked constructor and does not implement
`From<PathBuf>`. `Store`, `StagedRecord`, `protocol::Context`, `Plan::build`,
and `Plan::apply` accept it directly. A future Jinja context accepts the same
value and calls `as_path`; it does not normalize. This design has no
implementation dependency on the future Jinja-context design.

## Public and crate-visible signatures

```rust
// src/interview.rs
pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &PresetStore,
    mappings: &TemplateDefaultMappings,
) -> Result<Resolution, EvalError>; // prerequisite-owned behavior

impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError>;
}

impl Pending<'_> {
    pub fn check(&self, id: &Id, raw: RawAnswer) -> Result<Answer, CheckError>;
    pub fn answer(self, incoming: RawAnswers) -> Result<Interview<'_>, AnswerError<'_>>;
    fn probe_submission(&self, known: &Answers, ids: &[Id]) -> SubmissionProbe<'_>;
}

// src/staging.rs
pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError>;
impl Store {
    pub fn path_for(&self, target: &CanonicalTarget) -> PathBuf;
    pub fn load(
        &self,
        target: &CanonicalTarget,
    ) -> Result<Option<StagedRecord>, StagingError>;
    pub fn save(&self, record: &StagedRecord) -> Result<(), StagingError>;
    pub fn remove(&self, target: &CanonicalTarget) -> Result<bool, StagingError>;
}
impl StagedRecord {
    pub fn target(&self) -> &CanonicalTarget;
    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: ResolvedDefaults,
    ) -> Result<Interview<'a>, StagingError>;
}

// src/protocol.rs
impl Context {
    pub fn new(
        target: &CanonicalTarget,
        template: &str,
        commit: Option<&str>,
    ) -> Self;
}

// src/plan.rs
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
```

The public changes are the opaque target carrier, origin-bearing seed
construction, and structured template fault. The early-answer probe and prompt
preparation stay crate-visible or private.

## Module and seam diagram

```text
config files
  presets + exact formal_name mappings
        |
        | repaired 1073 resolution: value and selected origin stay coupled
        v
config/interview boundary ----> ResolvedDefaults ----> Seed
                                      |                 |
                                      |                 v
                                      |          interview state machine
                                      |          + shared guard evaluator
                                      |          + non-committing probe
                                      |                 |
                                      |                 v
                                      |          Prompt / Rejection / Completed
                                      |
requested target                     |                 completed answers
        |                             |                         |
        v                             |                         v
staging::canonical_target            |                  plan::Plan::build
        |                             |                         |
        +--> Store key / record ------+                         |
        +--> protocol::Context ---------------------------------+
        +--> Plan::build / apply
        +--> future Jinja context

jinja compiled values --> TemplateFault <-- interview evaluation
                               ^
                               |
                         plan field rendering
```

`interview` owns answer/default/skip policy. `plan` owns file planning and adds
a filesystem source path around a shared fault. `staging` owns target identity
and persistence compatibility. `protocol` serializes domain results without
deciding policy. No adapter re-creates provenance, evaluates a skip, or
canonicalizes a path.

## Error and result contracts

### File-rule and target-path faults

<!-- rumdl-disable MD013 -->

Given `files[0].when: "'bad' | dateformat"` for `parts/card.txt`:

```text
/templates/sample/parts/card.txt: template error in files[0].when `'bad' | dateformat`: invalid operation: ...
```

Given `files[0].each: "entry as item"` where `entry` is not an array:

```text
/templates/sample/parts/card.txt: template error in files[0].each `entry`: expected array
```

Given `files[0].path: "{{ item.name | unknown }}.txt"`:

```text
/templates/sample/parts/card.txt: template error in files[0].path `{{ item.name | unknown }}.txt`: unknown filter: unknown
```

Given the ordinary source path `template/{{ label | unknown }}.txt`:

```text
/templates/sample/template/{{ label | unknown }}.txt: template error in path `{{ label | unknown }}.txt`: unknown filter: unknown
```

Compilation and rendering failures use the same contract. `files[i].each` names
the expression before `as <binding>`, matching the compiled source.

### Default failures

A template-authored typed literal default `[]` with `required: true`:

```text
template error in labels.default `[]`: is required
```

A template-authored string default `ab` with `validate.min: 3`:

```text
template error in label.default `ab`: must be at least 3 characters
```

A configured default from an inline mapping that fails a later constraint is a
recoverable rejection:

```text
label: /config/toha.yml: template-defaults."gh:example/sample".label ("ab"): default is not allowed: must be at least 3 characters
```

A configured default selected through a preset retains both winning files:

```text
label: /config/local.yml: template-defaults."gh:example/sample".label
  → /config/user.yml: presets."short_label" ("ab"): default is not allowed: must be at least 3 characters
```

<!-- rumdl-enable MD013 -->

Protocol `errors.label` omits the leading `label:` supplied by
`Rejection::Display`; it contains the exact origin and constraint sentence. A
replacement answer removes the carried error and is checked normally. Missing
preset and answer-kind failures remain fatal configured-default evaluation
errors before the interview starts, as specified by the configured-defaults
design.

### Early-answer transitions and ordering

`Pending::answer` performs these operations without committing intermediate
state:

1. Classify unknown, skipped, answered, current, and early ids in incoming
   document order.
2. Check all supplied current answers. Build the probe context only from
   previously accepted answers and successfully checked current answers.
3. Check known literal rules for early answers and record tentative failures.
4. Probe each failed early question through the shared guard evaluator.
5. Remove failures proven `Skipped`; retain `Active` and `Undecidable` failures.
6. If any rejection remains, return the original `Pending`. Suppress probe
   messages, warnings, hooks, answers, and evaluation faults.
7. If no rejection remains, surface a probe evaluation fault, if any; otherwise
   commit the proposed state and its normal warnings/messages.

Pruning does not reorder errors. Errors discovered while scanning supplied ids
stay in incoming document order; errors caused by omitted current
answers/defaults follow in batch order.

| Early value | Reachability from known-good view | Current batch | Result                                                     |
| ----------- | --------------------------------- | ------------- | ---------------------------------------------------------- |
| valid       | any                               | succeeds      | held or consumed by normal advancement                     |
| invalid     | active                            | succeeds      | reject early constraint error                              |
| invalid     | undecidable                       | succeeds      | reject early constraint error                              |
| invalid     | skipped                           | succeeds      | accept; emit one skipped warning in interview order        |
| invalid     | active                            | fails         | retain early error beside batch error                      |
| invalid     | undecidable                       | fails         | retain early error beside batch error                      |
| invalid     | skipped                           | fails         | omit early error; reject only remaining errors; no warning |

A template fault reached only by the probe replaces an early rejection only
after all rejections have been removed. A rejected document cannot reach that
fault in the committed interview.

## Persistence, replay, and compatibility

- Accepted raw submissions remain the staged source of truth. No origin,
  reachability, rejection, warning, or prepared prompt is serialized.
- Resume resolves the recorded exact `formal_name` through live config and
  rebuilds the same `ResolvedDefaults`. Replay therefore sees current configured
  values and their current winning origins, as configured-defaults behavior
  requires.
- Old accepted submissions replay through the new early-answer policy. A
  submission that contains a known-invalid early answer whose skip remains
  undecidable fails replay with the same rejection. A submission whose question
  is provably skipped remains accepted and produces its warning at the same
  interview position.
- New rejected documents never enter `StagedRecord.submissions`, so the policy
  that omits a skipped early error beside another failure creates no persistence
  case.
- `CanonicalTarget` serializes as the existing path string. New records omit a
  redundant trailing separator.
- `Store::load` first checks the canonical digest. When absent, it checks one
  legacy key: the digest of the same non-root target string with one trailing
  platform separator. A successful later save writes the canonical key and
  removes that exact legacy file after the atomic rename. This is a storage
  compatibility lookup, not a second path normalizer.
- A loaded legacy record passes its target through `canonical_target`; the
  requested `CanonicalTarget` must equal the result. Protocol context and later
  saves use the requested canonical value.

Direct apply, staged continue, staged apply, progress replay, terminal driving,
and crate callers all construct the same seed and call the same engine. Planning
always receives the target constructed at command setup.

## Proposed specification, schema, and guide impact

The paired implementation updates these canonical artifacts:

- `template-format.yml`: render faults in `files[i].when`, `files[i].each`,
  explicit `files[i].path`, and ordinary target paths name the field and
  compiled source; a template-authored default is validated against its ready
  constraints when it is evaluated, and violation is a template fault.
- `interview-protocol.yml`: an early known failure stands when skip is
  undecidable; a failure proven skipped is omitted even beside a current-batch
  failure; rejected documents emit no skipped warning; configured-default
  constraint errors use the selected mapping origin and optional preset hop; the
  target string has no redundant trailing separator.
- `architecture.yml`: `Seed` carries the configured-default resolution product,
  and the opaque canonical target is shared by staging, protocol, planning,
  applying, and future context construction.
- `configuration.md`: configured-default constraint examples use the approved
  `template-defaults`/`presets` provenance. The config schema receives no
  additional change beyond the configured-defaults design.
- `template-files.md` and `template-interviews.md`: add the actionable fault
  examples and the early-answer matrix.
- `interview-protocol.schema.yml`: no shape change. Context remains a string,
  error entries remain strings, and default validity is semantic.
- `template-format.schema.yml`: no shape change.

The design artifact does not edit these shared files.

## Falsifiable verification scenarios

1. A fault in each of `files[0].when`, `files[0].each`, and `files[0].path`
   produces the exact field, source, and source-file prefix above. Removing any
   field wrapper fails its snapshot.
2. An ordinary source filename with a failing Jinja filter produces field
   `path`, the exact segment source, and the full source-file prefix. Static and
   rendered files use the same path renderer.
3. Direct headless apply and staged continue followed by staged apply produce
   the same `PlanError` for each planning fault.
4. Literal string, literal list, and expression template defaults each fail one
   ready constraint as `EvalError::Template`; none appears in protocol `errors`
   or causes a terminal retry.
5. A configured inline value and a configured preset value pass kind resolution
   but fail a dynamic constraint at reach. Terminal, direct headless apply,
   staged continue, staged apply, and a crate caller all receive a recoverable
   rejection. Correcting the answer completes the interview.
6. Put the winning mapping and preset in different config files. The later
   rejection names the exact mapping file/site, preset file/name/value, and
   constraint. Changing precedence changes both value and origin together.
7. Change the winning preset between stage and continue. Replay uses the live
   value and live origin. The staged record contains neither.
8. An invalid early answer with an active question rejects. The same answer with
   an undecidable guard rejects. Supplying a valid dependency that proves the
   guard false accepts and emits one warning.
9. Repeat scenario 8 with a separate invalid current-batch answer. The skipped
   early error is absent, the current error remains, the original pending state
   is returned, and no warning/message/hook/submission is recorded.
10. If the skip guard depends on the invalid current answer, the early result is
    `Undecidable` and its error remains. A proof must not use rejected data.
11. A probe that encounters a template fault while another rejection remains
    returns the rejection and original state. Correct the rejection; the same
    path then returns the template fault.
12. Run the early-answer matrix through `apply --answers`, `stage --async` plus
    `continue`, staged `apply --answers`, replay, and direct `Pending::answer`.
    Status, errors, answers, and step messages are equal for every row.
13. Canonicalize an existing directory and the same path while its final
    component does not yet exist. Neither non-root display ends in a separator;
    repeated calls compare equal after the component exists.
14. A symlinked existing ancestor resolves once. Store key, record target,
    protocol context, plan destination, apply destination, and a simulated
    future Jinja context all observe the same `CanonicalTarget`.
15. Place an old staged record under the legacy trailing-separator digest. Load
    finds it, context is separator-free, save creates the canonical key, replay
    is equal, and only the exact legacy file is removed after the canonical
    write succeeds.
16. Compile-fail coverage proves that `Plan::build`, `Store::load`, and
    `Context::new` do not accept `PathBuf`; code outside `staging` cannot
    construct a `CanonicalTarget` unchecked.

## Red-flag screen

- `TemplateFault` hides formatting behind one three-part value instead of adding
  wrappers in every caller.
- The configured default and its origin are one value; no synchronized sidecar
  or rendered-key reconstruction leaks config decisions into interview policy.
- The probe is an operation on the interview state machine and shares its guard
  evaluator; it is not a driver-level preflight stage.
- `CanonicalTarget` makes invalid construction unavailable and replaces repeated
  normalization; it is not a pass-through wrapper.
- Call chains remain config resolution → interview → plan, with staging and
  protocol as storage/serialization adapters.
