---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Error attribution and early-answer behavior — candidate design

## Caller usage

The public flow remains template → interview → plan. A caller resolves configured
defaults once, gives the resulting values-with-origin to the interview seed, and
normalizes the target once before any staging, protocol, planning, or apply work.

### Library caller

```rust
use toha::{Interview, RawAnswers, Seed, Template};

let template = Template::load(template_root)?;
let resolution = toha::interview::configured_defaults(
    &resolved.formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;

// `resolution.defaults` is the repaired configured-default carrier. Each selected
// value still owns its mapping origin and optional preset hop.
let interview = Interview::start(
    &template,
    Seed {
        now: clock.now(),
        defaults: resolution.defaults,
    },
)?;

let next = match interview {
    Interview::Asking(pending) => pending.answer(RawAnswers::new())?,
    complete => complete,
};
```

The caller does not construct or synchronize an origin map. It receives one
resolved-default map from configured-default resolution and moves that map into
`Seed`.

### Staged headless flow

```rust
use toha::staging::{Store, canonical_target};
use toha::protocol;

let target = canonical_target(path)?; // the only normalization call
let mut record = store.load(&target)?.unwrap_or_else(|| {
    StagedRecord::new(&target, &resolved, clock.now())
});

let defaults = toha::interview::configured_defaults(
    &record.template,
    &template,
    &config.presets,
    &config.template_defaults,
)?.defaults;

let interview = record.replay_with_defaults(&template, defaults)?;
let result = protocol::answer_headless(&template, interview, document)?;

match result {
    protocol::Headless::Pending { pending, rejections, accepted } => {
        record.submissions.extend(accepted);
        store.save(&target, &record)?;
        emit(protocol::batch_document(
            pending.batch(),
            &protocol::Context::new(&target, &record),
            Some(&rejections),
        ));
    }
    protocol::Headless::Completed { completed, accepted } => {
        record.submissions.extend(accepted);
        store.save(&target, &record)?;
        emit(protocol::complete_document(
            &completed,
            &protocol::Context::new(&target, &record),
        ));
    }
}
```

A rejected document does not extend `record.submissions`. A configured default
that fails a question constraint appears as a recoverable rejection in the batch;
an explicit answer in a later call can replace it.

### Completed-plan flow

```rust
use toha::{ApplyOptions, Interview, Plan};
use toha::staging::canonical_target;

let target = canonical_target(output_path)?;
let Interview::Complete(completed) = next else {
    return Err("interview still has questions".into());
};

let plan = Plan::build(&template, &completed, &target)?;
let applied = plan.apply(
    &target,
    ApplyOptions { force: false, trusted: false },
    runner,
)?;
```

The same `CanonicalTarget` supplies conflict checks, output destinations, hook
working directories, staged identity, protocol context, and the future Jinja
target value. No consumer normalizes it again.

## Integration assumption

The repaired configured-defaults design owns one resolved-default carrier. This
design calls it `ResolvedDefault` and assumes it has these capabilities:

```rust
// Owned by the configured-defaults design; this is an integration contract.
impl ResolvedDefault {
    pub fn raw(&self) -> &RawAnswer { unimplemented!() }

    /// Formats the exact winning mapping site, followed by the optional winning
    /// preset site and value. The result includes both config file paths.
    pub fn attribution(&self) -> &ConfiguredDefaultAttribution {
        unimplemented!()
    }
}

pub type ResolvedDefaults = IndexMap<Id, ResolvedDefault>;
```

`configured_defaults` returns `ResolvedDefaults`, and `Seed.defaults` stores that
same map. The final symbol names may follow the repaired configured-defaults
design, but the value-plus-origin ownership and accessor behavior are required.
If that design retains only `IndexMap<Id, RawAnswer>`, implementation stops at
that conflict. This design does not add a sidecar map, rebuild attribution from
an id, or define another origin carrier.

The approved configured-default behavior is unchanged: selectors are exact
`formal_name` matches, reusable values are under `presets`, references are
`{ preset: <name> }`, and attribution names the mapping config file, mapping site,
and optional preset file/site/value.

## Product policies

1. A known failure of an early answer remains a rejection when the question is
   active or its skip is not yet decidable. This preserves prompt feedback at the
   document boundary and prevents a known-bad value from becoming accepted staged
   state.
2. A known failure of an early answer is omitted when the same document proves the
   question skipped, including when another current-batch answer fails. The
   rejected document emits no skipped-answer warning. A corrected, accepted
   document emits that warning once in interview order.
3. A template-authored default is checked against the fully rendered constraints
   when its active question becomes a prompt. A failure is a template fault.
4. A configured default is checked at the same point. A failure becomes an
   attributed batch error, and the invalid value is not exposed as the prompt's
   default. Terminal and headless callers can replace it.

The first early-answer policy has two viable choices:

- Reject while skip is undecidable. This design recommends it so that every known
  literal failure is reported at submission and rejected documents never become
  replay state.
- Hold without rejection until active or skipped. This delays useful feedback and
  makes a document's acceptance depend on a later submission even though its value
  is already known to be invalid.

The second policy also has two viable choices:

- Omit a failure once the same document proves the question skipped. This design
  recommends it because the value cannot affect the transition and the current
  failure already rejects the document.
- Keep all early failures whenever the current batch fails. This is simpler, but
  reports an error for a value that the same input proves unused.

## Domain shape

### One template-fault value

`TemplateFault` concentrates the field, optional authored expression, and engine
message used by interview and planning faults.

```rust
// src/error.rs

/// A fault in a value authored by a template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TemplateFault {
    field: String,
    expression: Option<String>,
    message: String,
}

impl TemplateFault {
    pub(crate) fn expression(
        field: impl Into<String>,
        expression: impl Into<String>,
        error: impl ToString,
    ) -> Self {
        unimplemented!()
    }

    pub(crate) fn literal(
        field: impl Into<String>,
        error: impl ToString,
    ) -> Self {
        unimplemented!()
    }

    pub fn field(&self) -> &str { unimplemented!() }
    pub fn expression_source(&self) -> Option<&str> { unimplemented!() }
    pub fn message(&self) -> &str { unimplemented!() }
}

impl std::fmt::Display for TemplateFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // With expression: template error in <field> `<expression>`: <message>
        // Without one:     template error in <field>: <message>
        unimplemented!()
    }
}
```

`TemplateFault` contains no file path. `PlanError` supplies the source path because
the planner has one; interview fields already include the node id, such as
`palette.default`. Config attribution is not a `TemplateFault` and stays owned by
the configured-defaults design.

```rust
// src/interview.rs

#[derive(Debug)]
pub struct EvalError {
    fault: EvaluationFault,
}

#[derive(Debug)]
enum EvaluationFault {
    Template(TemplateFault),
    Answer { id: Id, field: &'static str, message: String },
    ConfiguredDefault(ConfiguredDefaultFault), // config-resolution failures
}

// src/plan.rs

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{0}")]
    Path(String),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {fault}")]
    Template { path: PathBuf, fault: TemplateFault },
    #[error("duplicate target {target}: {first} and {second}")]
    Duplicate { target: TargetPath, first: PathBuf, second: PathBuf },
}
```

`HookFault` is removed. Hook rendering returns `TemplateFault`, with its caller
adding `hooks[i].` or the interview node id to the field during construction. This
leaves one formatter and one field/expression/message representation.

### Prompt preparation distinguishes default ownership

Default origin is needed only until the engine prepares a fully constrained
prompt. A private candidate couples the value and its source so that they cannot
drift.

```rust
// src/interview.rs

enum DefaultCandidate<'s> {
    Template {
        answer: Answer,
        site: AuthoredDefaultSite,
    },
    Configured {
        answer: Answer,
        source: &'s ResolvedDefault, // prerequisite-owned value and origin
    },
}

struct AuthoredDefaultSite {
    field: String,              // "<question-id>.default"
    expression: Option<String>, // None for Typed::Literal
}

struct PreparedPrompt {
    prompt: Prompt,
    /// Present only when a configured default failed a rendered constraint.
    default_rejection: Option<Rejection>,
}

pub struct Prompt {
    pub id: Id,
    pub kind: PromptKind,
    pub title: String,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub default: Option<Answer>,
    pub options: Vec<String>,
    pub constraints: Constraints,
}
```

`PreparedPrompt` is crate-private. A valid default becomes `Prompt.default`. An
invalid template default returns `EvalError::Template`. An invalid configured
default becomes `default_rejection`, while `Prompt.default` becomes `None`.
`Advance` adds that rejection to `Batch.errors` before adding the prompt.

This avoids changing the public `Prompt` shape and avoids carrying config
concepts into `Pending::answer`. The pure engine knows only an opaque resolved
default's raw value and diagnostic accessor; it does not know layers, presets,
formal names, or selection rules.

### Early-answer classification is a discarded transition

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EarlyDisposition {
    Active,
    Skipped,
    Undecidable,
}

struct Probe<'a> {
    result: Result<Interview<'a>, EvalError>,
    dispositions: IndexMap<Id, EarlyDisposition>,
}

impl Pending<'_> {
    fn probe_document(
        &self,
        held: RawAnswers,
        valid_current: Answers,
        failed_early: &[Id],
    ) -> Probe<'_> {
        // Clone all transition state.
        // Walk once with valid current answers and all held values.
        // Classify each failed early id:
        //   skipped map contains id       => Skipped
        //   next batch reaches id         => Active
        //   otherwise                     => Undecidable
        // Do not publish messages, hooks, answers, warnings, or batch state.
        unimplemented!()
    }
}
```

The implementation can reuse `Advance`; `Probe` is a result view, not a second
interview engine. Invalid current answers remain held during the probe. `Advance`
can place their failures in its discarded batch and continue far enough to prove
that an independent later question is skipped.

### Canonical target is a constructed domain value

```rust
// src/staging.rs

/// An absolute, lexically normalized target whose closest existing ancestor was
/// filesystem-canonicalized. Its display form has no redundant trailing separator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalTarget(PathBuf);

impl CanonicalTarget {
    pub fn as_path(&self) -> &Path { unimplemented!() }
    pub fn into_path_buf(self) -> PathBuf { unimplemented!() }
}

impl std::fmt::Display for CanonicalTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}

/// The only public constructor for a live canonical target.
pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError> {
    // 1. Make absolute.
    // 2. Remove `.` and lexically resolve `..`.
    // 3. Find and canonicalize the closest existing ancestor.
    // 4. If the suffix is empty, return the ancestor directly.
    // 5. Otherwise join the non-empty suffix.
    // Filesystem roots retain their root separator; no extra separator is added.
    unimplemented!()
}
```

The private field prevents an arbitrary `PathBuf` from entering identity-sensitive
APIs. The type is deeper than a formatting wrapper: construction performs the only
filesystem canonicalization, and signatures require proof that it happened.

`StagedRecord.target` remains a `PathBuf` in its serialized representation for
wire compatibility. `Store::load` validates and rewrites it to the supplied
`CanonicalTarget` before returning the record. No public constructor accepts a
stored path as already canonical.

## Public and crate-visible signatures

```rust
// src/interview.rs

pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: ResolvedDefaults, // configured-defaults-owned carrier
}

pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &IndexMap<PresetName, ConfigEntry<Value>>,
    mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
) -> Result<Resolution, EvalError>;

fn render_default<'s>(
    question: &Question,
    template: &Template,
    answers: &Answers,
    seed: &'s Seed,
) -> Result<Option<DefaultCandidate<'s>>, EvalError> {
    unimplemented!()
}

fn prepare_prompt(
    question: &Question,
    template: &Template,
    answers: &Answers,
    seed: &Seed,
) -> Result<PreparedPrompt, EvalError> {
    // Render prompt fields and all constraints first.
    // Render/parse the selected default into DefaultCandidate.
    // Validate it against Rules::of_prompt.
    // Template failure => EvalError::Template.
    // Configured failure => attributed Rejection + no exposed default.
    unimplemented!()
}

pub fn render_hooks(
    hook: &HookNode,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<RenderedHook>, TemplateFault> {
    unimplemented!()
}

impl<'a> Pending<'a> {
    pub fn answer(self, incoming: RawAnswers)
        -> Result<Interview<'a>, AnswerError<'a>>
    {
        unimplemented!()
    }
}

// src/plan.rs

impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &CanonicalTarget,
    ) -> Result<Self, PlanError> {
        unimplemented!()
    }

    pub fn apply(
        self,
        target: &CanonicalTarget,
        options: ApplyOptions,
        runner: &dyn HookRunner,
    ) -> Result<Applied, ApplyError> {
        unimplemented!()
    }
}

fn render_target_segment(
    segment: &OsStr,
    context: &impl Serialize,
    source_path: &Path,
) -> Result<String, PlanError> {
    unimplemented!()
}

// src/staging.rs

pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError>;

impl Store {
    pub fn path_for(&self, target: &CanonicalTarget) -> PathBuf {
        unimplemented!()
    }
    pub fn load(&self, target: &CanonicalTarget)
        -> Result<Option<StagedRecord>, StagingError> { unimplemented!() }
    pub fn save(&self, target: &CanonicalTarget, record: &StagedRecord)
        -> Result<(), StagingError> { unimplemented!() }
    pub fn remove(&self, target: &CanonicalTarget)
        -> Result<bool, StagingError> { unimplemented!() }
}

impl StagedRecord {
    pub fn new(
        target: &CanonicalTarget,
        resolved: &ResolvedTemplate,
        now: Zoned,
    ) -> Self { unimplemented!() }

    pub fn replay_with_defaults<'a>(
        &self,
        template: &'a Template,
        defaults: ResolvedDefaults,
    ) -> Result<Interview<'a>, StagingError> { unimplemented!() }
}

// src/protocol.rs

impl Context {
    pub(crate) fn new(target: &CanonicalTarget, record: &StagedRecord) -> Self {
        unimplemented!()
    }
}
```

`apply_reporting` and hook-runner calls use `target.as_path()` internally. They
do not accept an additional raw target path. This keeps all filesystem consumers
on the canonical value without changing the hook-runner interface.

## Module and seam diagram

```text
config files
  │
  │ configured_defaults(exact formal_name, presets, mappings)
  ▼
configured-default ResolvedDefaults ─┐
  value + exact origin, one carrier  │
                                    ▼
Template ───────────────────────▶ Interview
compiled authored fields          │ prepare_prompt
                                  │ ├─ template default failure → TemplateFault
answers document ────────────────▶ │ ├─ configured failure → recoverable Rejection
                                  │ └─ answer failures → atomic probe/commit
                                  ▼
                               Completed
                                  │
                                  ▼
                          Plan::build(template,
                            completed, CanonicalTarget)
                                  │
                                  ├─ file/path expression → TemplateFault
                                  ▼
                                Plan

raw target path ── canonical_target ──▶ CanonicalTarget
                                         ├─ Store key + staged record
                                         ├─ protocol Context.target
                                         ├─ Plan conflict/destination checks
                                         ├─ Apply destination + hook cwd
                                         └─ future Jinja target context
```

`error` owns the template-fault vocabulary. `interview` owns default validation,
answer policy, and speculative transition. `plan` owns source-path context for
file-generation faults. `staging` owns target construction and compatibility
lookup. `protocol`, `main`, and `apply` consume domain values and do not add policy.

## File and path fault attribution

Each planner call constructs the field at the point that knows it:

- `files[i].when`: expression is `rule.when.source()`.
- `files[i].each`: expression is `rule.each.expr.source()`; the binding is not part
  of the expression text.
- `files[i].path`: expression is `rule.path.source()`.
- ordinary source-tree path: field is `path`; expression is the individual source
  path segment being rendered.

`PlanError::Template.path` is the support/source file whose output was being
planned. For an ordinary directory segment, it is the descendant source file that
made Toha render that segment. Content-render faults keep their current source-path
prefix and are outside this attribution change.

If the evaluator supplies the message `invalid operation`, the exact rendered
examples are:

<!-- rumdl-disable MD013 -->

```text
/tmp/template/part.txt: template error in files[0].when `'bad' | dateformat`: invalid operation
/tmp/template/part.txt: template error in files[0].each `42`: expected array
/tmp/template/part.txt: template error in files[0].path `{{ item | dateformat }}.txt`: invalid operation
/tmp/template/template/{{ label | dateformat }}/note.txt: template error in path `{{ label | dateformat }}`: invalid operation
```

<!-- rumdl-enable MD013 -->

The evaluator message remains verbatim. The design standardizes the surrounding
field and expression attribution.

## Default result and error contracts

Prompt preparation evaluates constraints before the default so that one rules
value validates caller answers and defaults.

<!-- rumdl-disable MD013 -->

| Default source and result | Engine result |
| --- | --- |
| Template default passes | Exposed as `Prompt.default` and used normally. |
| Template default fails | `AnswerError::Eval(EvalError::Template)`; no prompt or recoverable rejection. |
| Configured default passes | Exposed as `Prompt.default` and used normally. |
| Configured default fails | Prompt remains available without that default; `Batch.errors` carries one attributed rejection. |
| Explicit answer beside configured-default error passes | Explicit answer replaces the default and clears the carried error for that transition. |
| Explicit answer beside configured-default error fails | Caller-answer rejection is returned; the configured-default error is not duplicated. |
| Configured value has wrong answer kind or missing preset | Existing fatal configured-default error before interview progression. |
| Default's `format` expression faults after constraint validation | Template fault in `<id>.format`; source ownership is the template. |

<!-- rumdl-enable MD013 -->

Exact template-default examples:

<!-- rumdl-disable MD013 -->

```text
template error in palette.default: default ["cyan"] is not allowed: each item must be one of: red, blue
template error in palette.default `suggested_palette`: default ["cyan"] is not allowed: each item must be one of: red, blue
```

<!-- rumdl-enable MD013 -->

The first is a typed literal. The second is an expression default whose evaluated
value is `['cyan']`.

Exact configured-default constraint examples:

<!-- rumdl-disable MD013 -->

```text
choice: default "medium" from /tmp/local.yml: template-defaults."gh:sample/collection".choice is not allowed: must be one of: fast, slow
choice: default "medium" from /tmp/local.yml: template-defaults."gh:sample/collection".choice → /tmp/user.yml: presets."preferred_choice" ("medium") is not allowed: must be one of: fast, slow
```

<!-- rumdl-enable MD013 -->

The preset hop appears only for `{ preset: preferred_choice }`. These are
recoverable rejection sentences. By contrast, the existing fatal kind contract
remains:

<!-- rumdl-disable MD013 -->

```text
/tmp/local.yml: template-defaults."gh:sample/collection".enabled → /tmp/user.yml: presets."flag" ("yes"): must be true or false
```

<!-- rumdl-enable MD013 -->

An active question's default is checked when its prompt and all dynamic
constraints are ready. A skipped question continues to take its default without
answer-constraint validation; question constraints govern submitted or offered
answers, and no recovery prompt exists for a skipped question. Expression
evaluation needed to obtain a skipped default can still produce its existing
template fault.

## Early-answer transitions and ordering

For every early answer, `Rules::of_question` validates answer kind and every
literal constraint available at submission. A known failure enters the tentative
set. After current-batch values are checked, `probe_document` runs even when a
current-batch value failed.

<!-- rumdl-disable MD013 -->

| Known early failure | Probe disposition | No other failure | Current-batch failure also present |
| --- | --- | --- | --- |
| Question is active | Reject early answer | early rejection | current rejection + early rejection |
| Question is skipped | Omit early rejection | accept; emit skipped warning in the committed step | current rejection only; emit no warning |
| Skip is undecidable/not reached | Reject early answer | early rejection | current rejection + early rejection |

<!-- rumdl-enable MD013 -->

The transition order is:

1. Classify incoming ids as unknown, already answered, already skipped, current,
   or early. Validate every known part of each early answer.
2. Validate current-batch raw values in batch order. Prepare omitted defaults.
   Collect successful current answers and failures without mutating `self`.
3. When there is a failed early answer, run one probe from cloned state using the
   successful current answers and held raw values. Do this even if step 2 found
   a failure.
4. Remove only early rejections classified `Skipped`. Keep `Active` and
   `Undecidable` failures.
5. If any rejection remains, return the original `Pending`; discard every probed
   answer, message, warning, hook, skip marker, and template fault.
6. If no rejection remains and the probe reached a template fault, return that
   template fault. Otherwise return the probed next state instead of walking
   twice.

Rejection ordering stays compatible: document-level unknown/already-answered/early
failures remain in incoming document order; current-batch failures follow in batch
order; filtering a skipped early failure does not reorder survivors. `errors`
continues grouping those sentences by id in first-occurrence order.

A template fault reached only by a rejected speculative transition is suppressed.
When dropping skipped early failures leaves no rejection, that reached template
fault becomes the result. This retains the existing rule that a document cannot
both fail atomically and publish consequences from a step it did not take.

## Persistence, replay, and compatibility

Staged records continue to persist accepted answer documents, frozen `now`, and
template identity. They do not persist `Pending`, early dispositions, rendered
constraints, configured defaults, or configured-default attribution. Replay loads
live configured defaults through their owning resolver and replays accepted
documents through the same `Pending::answer` transition.

The early policy changes only which diagnostics accompany a rejected document. It
does not make a previously rejected document accepted, and rejected documents were
never persisted. Existing accepted submissions therefore replay under the same
atomic contract. A newly detected template-default fault can stop replay; that is
the intended classification of a faulty live template, not corrupt staged state.
A constraint-invalid live configured default produces the same recoverable
pending batch during direct execution and replay.

Canonical target persistence uses this compatibility sequence:

1. `Store::load` checks the separator-free key from `CanonicalTarget`.
2. If absent, it checks the one legacy key obtained by hashing the same canonical
   path text with one redundant platform separator. This is a private state-key
   migration rule, not a path normalizer and not a value exposed to consumers.
3. A legacy record is accepted only when normalizing its stored `target` through
   `canonical_target` equals the requested `CanonicalTarget`.
4. The record is rewritten in memory with the separator-free target. Its next
   successful save writes the canonical key before the legacy file is removed.
   `Store::remove` removes either key.

No staged-record schema version changes. Non-existing targets never received the
redundant separator and continue using their existing key. Filesystem roots keep
their platform root spelling.

The future Jinja-context work consumes `CanonicalTarget`. It does not own a
normalizer, and this design neither imports nor waits on that work.

## Proposed specification, schema, and guide impact

The paired implementation updates canonical artifacts; this candidate does not.

- `template-format.yml`: state the exact fault form for `files[i].when`,
  `files[i].each`, `files[i].path`, and ordinary rendered target-path segments;
  state that an active template-authored default violating its rendered
  constraints is a template fault.
- `interview-protocol.yml`: record both early-answer policies, including the
  current-batch-failure matrix; state that rejected probes emit no messages and
  persist no submission; replace the legacy configured-default key sentence with
  the approved exact mapping and optional preset attribution; state that an
  invalid configured default is a recoverable batch error and is not offered as
  a default.
- `interview-protocol.yml`: define `context.target` as the separator-free display
  of the canonical absolute target and document legacy staged-key lookup.
- `interview-protocol.schema.yml`: no structural change. Tighten the description
  of `context.target`; errors remain per-id strings.
- `template-format.schema.yml`: no change. Default and file-field wire shapes do
  not change.
- `config.schema.yml`: no change. The `presets` and `template-defaults` schema
  remains owned by the configured-defaults design.
- `template-files.md`: add the four attributed planning examples.
- `template-interviews.md`: explain template-default faults and the difference
  between an authored faulty default and a recoverable configured default.
- `configuration.md`: show later constraint attribution with a mapping file and
  an optional preset file/hop.
- `architecture.yml`: name `CanonicalTarget` as the shared target value and
  `TemplateFault` as the authored-expression diagnostic.

## Falsifiable test scenarios

### Template and planning faults

1. A file rule with faulting `when`, `each`, and `path` values produces the exact
   field, exact authored source in backticks, evaluator message, and support-file
   prefix. Each assertion fails if any field is reconstructed as a generic path.
2. An ordinary source path with a faulting directory segment produces field `path`,
   the single segment source, and the descendant source-file prefix. A content
   render fault remains unchanged.
3. Build the same completed interview directly and after staged replay. The four
   planner faults are byte-for-byte equal after replacing the temporary root.
4. Top-level hooks and interview hooks still use the same `TemplateFault` formatter;
   their existing exact snapshots do not change.

### Defaults and configured provenance

5. A literal multiselect default outside literal options fails at start with
   `template error in <id>.default:` and no backticked expression. A typed
   expression yielding the same list includes its exact expression.
6. A string-template default that renders outside dynamic options fails only when
   those options are ready and names `<id>.default` plus the authored template.
7. Put the winning mapping in `/tmp/local.yml` and its referenced preset in
   `/tmp/user.yml`. Make the value the correct kind but outside rendered options.
   Terminal prints the exact two-hop rejection, does not preselect the bad default,
   accepts a replacement, and completes.
8. The same setup through headless apply returns status `questions`, the same
   rejection under the id, and no accepted submission. A second document with an
   explicit valid value succeeds.
9. A wrong-kind referenced preset still fails before the interview with the
   prerequisite's fatal exact mapping + preset attribution. This fails if this
   design converts all config failures into recoverable errors.
10. Replay after changing the live preset repeats resolution with the new exact
    origin. This fails if origin is frozen in staged state or rebuilt from the id.

### Early answers

11. Matrix each `EarlyDisposition` (`Active`, `Skipped`, `Undecidable`) against:
    valid/invalid early value, no other failure/current failure, and a later
    template fault. Assert the table above, original `Pending` on rejection, and
    no speculative messages or hooks.
12. For a valid current selector that proves an invalid early value skipped plus
    an invalid independent current answer, assert only the current error. Correct
    the current value in a later document and assert the skipped warning exactly
    once in interview order.
13. For the same documents with the selector proving active, assert both errors.
    With the selector absent or invalid so skip is undecidable, assert both errors.
14. Run every matrix row through direct `apply --answers` and `stage --async` plus
    `continue`. Normalize the target path and assert equal exit status, batch,
    errors, messages, accepted submissions, completed answers, and output tree.
15. Replay each accepted prefix, then submit the matrix document. Assert the result
    equals an uninterrupted interview. This fails if the probe mutates persisted
    or derived state.

### Canonical targets

16. Existing directory, non-existing suffix, relative `.`/`..`, and a symlinked
    existing ancestor each produce the expected absolute path with no redundant
    trailing separator. Calling consumers never invokes filesystem
    canonicalization again.
17. Stage an existing directory. Assert the state-file digest hashes the exact
    separator-free `CanonicalTarget`, `StagedRecord.target` matches it, protocol
    context matches it, planning uses it, and direct/staged output paths match.
18. Place a legacy separator-keyed record in the store. Assert load finds it,
    validates its stored target through the one canonical interface, replay works,
    the next save creates the canonical key, and abort removes the remaining key.
19. Pass a raw `PathBuf` to `Store::path_for`, `Plan::build`, or `Plan::apply` in
    a compile-fail API test. It must not type-check, proving callers cannot bypass
    normalization.

## Scope boundary

This design changes no permission, trust, timeout, pinned-version, subprocess, or
hook-execution policy. It adds no dependency on the future Jinja-context work. It
does not change the approved configured-default selector, naming, reference
syntax, precedence, or fatal resolution errors.
