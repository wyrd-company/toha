---
relationships:
  realizes: toha
  implements:
    - template-format
    - interview-protocol
    - command-line-interface
  references:
    - template-registry
    - architecture
    - error-attribution
    - jinja-includes
---

# Toha-owned Jinja context

Toha supplies one immutable, typed invocation context before the first
interview evaluation. The same value is carried by the pure interview engine,
stored for staged replay, and used by planning. The engine never reads the
process, registry, terminal, host files, privilege state, or trust state.

This is an approved design artifact. Runtime, shared specification, schema,
and guide changes belong to the paired implementation.

## Caller usage first

A template uses ordinary variables at every supported Jinja surface:

```jinja
{% if toha_is_interactive %}
Preparing {{ toha_template_name }} for {{ toha_target_name }}.
{% endif %}

{% if toha_env_editor is not none %}
editor = {{ toha_env_editor }}
{% endif %}
```

Stage analyzes every Jinja source before it asks a question. Any possible
environment observation requires the explicit stage flag:

```console
# No environment observation and no flag: record unavailable.
toha stage sample ./output --async

# An explicit stage grant is captured even when no reference exists yet.
toha stage sample ./output --async --trust

# Later batches use the recorded decision and values.
toha continue ./output answers.json
toha apply ./output
```

The staged `apply --trust` spelling remains available for hook execution. It
does not change the recorded Jinja environment snapshot.

The command driver resolves the selected identity and canonical target once,
loads the complete render program, admits environment access, captures facts,
and starts the existing engine:

```rust
let target = staging::canonical_target(path)?;
let resolved = resolve_template(operand, &config, &registry, &dirs, cwd)?;
let template = Template::load(&resolved.folder)?;

let decision = match invocation {
    NewStage { trust: true, .. } => EnvironmentDecision::CarryStageGrant,
    NewStage { trust: false, .. } => EnvironmentDecision::RequireStageGrant,
    NewApply { trust, .. } => EnvironmentDecision::grant_if_needed(
        trust || reviewed_environment_access(&resolved, &template, &registry)?,
    ),
};
let environment = template.admit_environment(decision, &mut fixed_environment)?;

let resolution = configured_defaults(
    &resolved.formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
report(resolution.warnings());

let context = InvocationContext::new(
    target.clone(),
    SelectedTemplate::from_resolved(&resolved, &template),
    HostFacts::capture(),
    ExecutionFacts::new(is_admin(), originating_driver_can_prompt),
    environment,
)?;
let interview = resolution.start_with_context(&template, now, context)?;
```

`configured_defaults` consumes the winning source-bearing `ConfigEntry` values
and returns a `Resolution` whose private `ResolvedDefault` entries keep each raw
value with its winning `ConfiguredDefaultOrigin`. The caller reads `warnings()`
before consuming the result. `start_with_context` then moves those entries into
`DefaultBankEntry::Configured` together with the invocation context. It does not
pass through `Seed`, flatten provenance, reconstruct an origin, or add a second
resolver or origin map.

For stage, `RequireStageGrant` with an environment need fails before the five
values are read, before `Seed`, before `Interview::start`, before any question
or message renders, and before any staged write. `CarryStageGrant` captures all
five values once even when the loaded program has no initial environment need,
so the explicit decision survives a later mutable-folder edit. Target,
configuration, selected-template, and template-load failures can occur first
because they supply the facts needed to analyze the template.

A direct crate caller supplies equivalent typed facts and a fixed five-value
adapter. The engine performs no ambient read:

```rust
let target = staging::canonical_target(Path::new("./output"))?;
let template = Template::load(folder)?;
let environment = template.admit_environment(
    EnvironmentDecision::Deny,
    &mut supplied_environment,
)?;
let context = InvocationContext::new(
    target.clone(),
    selected,
    host,
    execution,
    environment,
)?;
let completed = finish(Interview::start(
    &template,
    Seed { now, defaults, context },
)?)?;
let plan = Plan::build(&template, &completed, &target)?;
```

## Canonical target contract

The approved target producer contract is anchored at revision
`067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
`72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`,
and integrated at revision `dfe7ba017ebef525310db8b8ab4ead58fae2d147`,
design SHA-256
`9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`:

```rust
pub fn canonical_target(
    path: &Path,
) -> Result<CanonicalTarget, StagingError>;

impl CanonicalTarget {
    pub fn as_path(&self) -> &Path;
}
```

`CanonicalTarget` has no public unchecked constructor and no `From<PathBuf>`.
Context, storage, protocol, plan, and apply consumers receive this carrier and
inspect it only through `as_path()`. No context code normalizes a path.

`StagedRecord.target` remains serialized path text. `Store::load` receives the
already constructed `&CanonicalTarget`, validates the stored text against it,
and uses that carrier to reconstruct the live invocation context.

## Exact Jinja contract

All seventeen values are variables, not functions. In the current contract,
every name is present. `null` is Jinja `none`; an unavailable value is never
`undefined`. The existing `now()` function is unchanged.

| Name | Jinja type | Exact value |
| --- | --- | --- |
| `toha_target_name` | string or null | Final Unicode component of the canonical target; `null` for a filesystem root or non-Unicode component. |
| `toha_template_name` | string | Short name captured from the loaded `template.yml`. |
| `toha_template_formal_name` | string | Stable selected formal name, never the caller's alias or short spelling. |
| `toha_template_aliases` | array of strings | Effective aliases of the selected named entry in registry order; `[]` for a folder, direct Git, bundled, or alias-free selection. |
| `toha_template_source` | string or null | Selected named entry source; `null` when selection has no registry entry. |
| `toha_host_os` | string | Lowercase Rust target OS vocabulary from `std::env::consts::OS`. |
| `toha_host_arch` | string | Rust target architecture vocabulary from `std::env::consts::ARCH`. |
| `toha_host_os_name` | string or null | Linux os-release `NAME`; `null` elsewhere or when unavailable. |
| `toha_host_os_id` | string or null | Linux os-release `ID`; `null` elsewhere or when unavailable. |
| `toha_host_os_id_like` | array of strings | Linux os-release `ID_LIKE`, split on ASCII whitespace in source order; `[]` elsewhere or when unavailable. |
| `toha_is_admin` | boolean | Effective administrative status captured by the caller. |
| `toha_is_interactive` | boolean | Whether the originating interview driver was allowed to prompt a human. |
| `toha_env_user` | string or null | Captured user value. |
| `toha_env_hostname` | string or null | Captured native hostname observation. |
| `toha_env_editor` | string or null | Captured `EDITOR`. |
| `toha_env_shell` | string or null | Captured `SHELL`. |
| `toha_env_visual` | string or null | Captured `VISUAL`. |

### Identity and source rules

`SelectedTemplate` is assembled at the resolution boundary. A name, alias, or
short-name resolution carries the selected registry entry's full effective
alias list and source. A folder, direct Git address, or bundled demo carries
`[]` and `null`. The bundled formal identity is `toha-demo`.

The resolver adds aliases and source to its existing result while preserving
the configured-default contract's formal name. Config `presets` and
`{ preset: <name> }` remain config-side inputs to `configured_defaults`. Their
selected values and origins enter the private origin-bearing `Resolution`, not
the ordinary flat `Seed.defaults` route. No Jinja value is named `preset` or
`presets`.

### Host fallback rules

The command host adapter reads `/etc/os-release` only on Linux. It accepts
standard quoted and unquoted assignments, unescapes values without executing
the file, and uses the last valid assignment for a duplicate key. A missing key
gets its own fallback. An absent, unreadable, non-UTF-8, or malformed file
makes all three os-release values unavailable: `null`, `null`, and `[]`. Empty
`NAME` or `ID` is `null`; empty `ID_LIKE` is `[]`. Host metadata absence never
fails template load, interview, or planning.

Admin is `true` only when a platform adapter positively observes effective
administrative authority: effective UID zero on Unix or an elevated token on
Windows. Failed or unsupported detection is `false`. No username inference,
elevation, timeout, or subprocess is used.

### Environment fallback rules

The fixed adapter reads user from `USER` on Unix and `USERNAME` on Windows.
Hostname is a native hostname observation. `EDITOR`, `SHELL`, and `VISUAL` are
independent and have no cross-fallback. Absent, empty, or non-Unicode input is
`null`, with no lossy conversion.

Denied or unnecessary access performs none of these reads and projects five
nulls. A template cannot distinguish denial, no initial need, and an unavailable
captured value. No arbitrary environment lookup, environment map, dynamic
environment variable name, or alias exposes another process value.

## Complete pre-interview analysis

`Template::load` owns one immutable in-process render program. It compiles:

- every question prompt, description, placeholder, default, required rule,
  condition, computed value, format, option expression, and numeric bound;
- every interview message and hook Jinja field;
- every explicit file rule expression, target, and source body;
- every top-level hook and before/after apply message;
- every non-static source-tree path segment and file body; and
- every transitive literal file-body include allowed by the approved include
  contract.

Regexes, hook script paths, file-rule source paths, source-directory names,
globs, template metadata, answers, configured defaults, and data values are not
recursively interpreted as Jinja. YAML `!include` remains structured
configuration loading, not a Jinja surface.

Planning in the same process renders the retained compiled sources. It does not
reopen a Jinja source after admission. A resumed invocation loads current
folder bytes, as it does today, and applies the recorded environment snapshot.
Named and Git selections remain commit-pinned by their existing identity.

The admission analyzer uses MiniJinja's existing parser AST through its
`unstable_machinery` feature. This adds no crate; it exposes the parser already
used by Toha's MiniJinja dependency. The implementation isolates that unstable
interface inside `jinja.rs`, so a dependency update produces a compile-time
adapter change rather than a runtime version check.

The private walk is control-flow insensitive and read-accurate:

- it visits assignment right-hand sides before introducing targets;
- it tracks lexical shadowing and possible aliases of environment values and
  the built-in `debug` callable;
- it visits macro bodies, false branches, filter/function arguments, and
  dynamic attribute and item operands;
- `object[toha_env_user]` is a reference, while the literal string in
  `object["toha_env_user"]` is not a root lookup;
- a possible call of the built-in `debug`, directly or through an alias, marks
  all five values observable; and
- literal include closures union their needs into the containing body.

An AST form that is not proven safe is classified as able to observe all five.
Supported syntax is not rejected to make analysis easier. A future generic
context lookup must add an explicit sound analysis rule before it is supported.

The analyzer retains the first deterministic `RenderOrigin` for an environment
need. It carries a configuration field path or source/include path without any
captured value.

## Data structures and signatures

```rust
#[derive(Clone)]
pub struct InvocationContext {
    target: CanonicalTarget,
    contract: ContextContract,
}

#[derive(Clone)]
enum ContextContract {
    Legacy,
    Current(CurrentContext),
}

#[derive(Clone)]
struct CurrentContext {
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
    environment: EnvironmentSnapshot,
}

#[derive(Clone, Debug)]
pub struct SelectedTemplate {
    formal_name: String,
    short_name: String,
    aliases: Vec<String>,
    source: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HostFacts {
    os: String,
    arch: String,
    os_name: Option<String>,
    os_id: Option<String>,
    os_id_like: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct ExecutionFacts {
    is_admin: bool,
    is_interactive: bool,
}

#[derive(Clone)]
pub enum EnvironmentSnapshot {
    Unavailable,
    Captured(FixedEnvironment),
}

#[derive(Clone)]
pub struct FixedEnvironment {
    user: Option<String>,
    hostname: Option<String>,
    editor: Option<String>,
    shell: Option<String>,
    visual: Option<String>,
}

pub enum EnvironmentDecision {
    Deny,
    GrantIfNeeded,
    RequireStageGrant,
    CarryStageGrant,
}

pub trait FixedEnvironmentSource {
    fn capture(&mut self) -> FixedEnvironment;
}

pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}

// Producer-owned configured-default carrier; fields stay private.
pub struct Resolution {
    defaults: ResolvedDefaults,
    warnings: Vec<String>,
}

struct ResolvedDefaults(IndexMap<Id, ResolvedDefault>);

struct ResolvedDefault {
    raw: RawAnswer,
    origin: ConfiguredDefaultOrigin,
}

type DefaultBank = IndexMap<Id, DefaultBankEntry>;

enum DefaultBankEntry {
    Seed(RawAnswer),
    Configured(ResolvedDefault),
}

struct InterviewSeed {
    now: jiff::Zoned,
    defaults: DefaultBank,
    context: InvocationContext,
}

pub struct Completed {
    // Existing fields remain.
    context: InvocationContext,
}

impl Template {
    pub fn admit_environment(
        &self,
        decision: EnvironmentDecision,
        source: &mut impl FixedEnvironmentSource,
    ) -> Result<EnvironmentSnapshot, EnvironmentAdmissionError>;
}

impl Resolution {
    pub fn warnings(&self) -> &[String];

    // Existing consuming producer entry, unchanged.
    pub fn start<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
    ) -> Result<Interview<'a>, EvalError>;

    pub fn start_with_context<'a>(
        self,
        template: &'a Template,
        now: jiff::Zoned,
        context: InvocationContext,
    ) -> Result<Interview<'a>, EvalError>;

    // The explicit compatibility escape. This is the only configured route
    // that deliberately discards ConfiguredDefaultOrigin.
    pub fn into_flat_defaults(
        self,
    ) -> (IndexMap<Id, RawAnswer>, Vec<String>);
}

impl StagedRecord {
    pub(crate) fn invocation_context(
        &self,
        target: &CanonicalTarget,
    ) -> Result<InvocationContext, StagingError>;

    pub fn replay_with_resolution<'a>(
        &self,
        template: &'a Template,
        resolution: Resolution,
        context: InvocationContext,
    ) -> Result<Interview<'a>, StagingError>;
}

impl InvocationContext {
    pub fn new(
        target: CanonicalTarget,
        selected: SelectedTemplate,
        host: HostFacts,
        execution: ExecutionFacts,
        environment: EnvironmentSnapshot,
    ) -> Result<Self, ContextError>;

    pub fn target(&self) -> &CanonicalTarget;
}

impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &CanonicalTarget,
    ) -> Result<Self, PlanError>;
}
```

`FixedEnvironment`, `EnvironmentSnapshot`, and their staged wire types have
redacted `Debug` implementations. Captured values must not enter diagnostics,
protocol output, logs, or design evidence.

`Resolution::start_with_context` is the consuming sibling of the producer's
`Resolution::start`. Both map every private `ResolvedDefault` directly to
`DefaultBankEntry::Configured`; the context-bearing sibling also places the
supplied `InvocationContext` in `InterviewSeed`. Ordinary `Interview::start`
maps public flat `Seed.defaults` only to `DefaultBankEntry::Seed`. Configured
callers never call `into_flat_defaults` as part of start or replay.

`Plan::build` retains the approved producer signature. For the current context,
it compares the supplied carrier with `completed.context.target()` before any
render and returns `PlanError::ContextTarget` on mismatch. It uses
`target.as_path()` for path work. Legacy replay receives the same validated
carrier but projects the pre-context Jinja contract.

## Admission behavior

`Template::admit_environment` combines the immutable analysis need with one
caller decision. The complete matrix is:

| Initial need | Decision | Reads | Result |
| --- | --- | ---: | --- |
| none | `Deny`, `GrantIfNeeded`, or `RequireStageGrant` | 0 | `Unavailable` |
| none | `CarryStageGrant` | all five once | `Captured` |
| present | `Deny` | 0 | `Unavailable` |
| present | `GrantIfNeeded` or `CarryStageGrant` | all five once | `Captured` |
| present | `RequireStageGrant` | 0 | attributed `TrustRequired` error |

Direct and new-apply paths use the approved effective-trust policy. Access is
granted by explicit `--trust`, or by a current matching reviewed approval for a
selected named user/system registry entry. The live `HookSurface` digest must
match through `evaluate_trust`; approval presence alone is insufficient.
Folder, direct Git, bundled, and local-layer selections require the flag.

The reviewed digest covers hook nodes and files executed by hooks. It does not
attest every Jinja-bearing file. A matching review can therefore grant access
to changed non-hook Jinja content. User-facing text must not describe all
rendered content as reviewed.

Stage always uses its explicit flag. A matching registry review does not
substitute for `stage --trust`. The absence of the flag records `Unavailable`
when no need exists and refuses when a need exists. The presence of the flag
records a fixed-five snapshot in both cases. `continue` has no trust flag.
Staged apply's flag controls hook execution only.

## Staging and replay

The staged record keeps its existing target, formal name, commit, named flag,
instant, and submissions. It gains a versioned context field whose wire form is
separate from the live domain carrier:

```rust
#[derive(Serialize, Deserialize)]
enum InvocationContextWire {
    Legacy,
    Current {
        short_name: String,
        aliases: Vec<String>,
        source: Option<String>,
        host: HostFactsWire,
        execution: ExecutionFactsWire,
        environment: EnvironmentWire,
    },
}

#[derive(Serialize, Deserialize)]
enum EnvironmentWire {
    Unavailable,
    Captured {
        user: Option<String>,
        hostname: Option<String>,
        editor: Option<String>,
        shell: Option<String>,
        visual: Option<String>,
    },
}
```

The wire does not duplicate target or formal name. `Store::load` validates the
record's target through its `&CanonicalTarget` parameter. The existing record
supplies the formal name. The remaining wire fields reconstruct the current
context without deserializing or manufacturing a target carrier.

Replay uses live configured defaults, the recorded instant, context, and
ordered accepted submissions. It does not read current host, environment,
terminal, privilege, registry approval, or environment access. The recorded
snapshot carries through every later batch.

The configured replay caller reads warnings before consumption and restores the
typed context from the staged wire and producer carrier before replay:

```rust
let resolution = configured_defaults(
    &saved.template,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
report(resolution.warnings());
let context = saved.invocation_context(&target)?;
let interview = saved.replay_with_resolution(&template, resolution, context)?;
```

`replay_with_resolution` parses the recorded instant, calls
`resolution.start_with_context(template, now, context)`, and replays accepted
submissions. It never calls `replay_with_defaults`, destructures `Resolution`
to a flat map, or uses `into_flat_defaults`.

Captured optional strings are plaintext in the existing staged JSON. The store
does not add a new file mode; access remains governed by its state directory and
process umask. Values remain until successful staged apply removes the record,
`abort` removes it, or an operator removes it. No grant credential or reusable
permission is stored.

### Mutable folder behavior

No source or program identity is added to replay. A mutable folder continues to
load current bytes.

- If stage had no initial need and no flag, later-added environment references
  see five nulls and cause no ambient read or access gate.
- If stage had the explicit flag, all five recorded values are available to
  later-added references whether or not the initial template had a need.
- A direct/new-apply denial remains unavailable and performs no ambient read.

This is why an explicit stage grant captures all five instead of only the names
first referenced. It is the smallest deterministic snapshot over the closed
five-value vocabulary that carries the approved decision without a new
source-change refusal.

### Legacy staged records

A record without the context field is decoded as `Legacy`. It replays the
pre-context Jinja contract: template data, answers, and private time only. The
seventeen new names are not injected, reserved, or marked available. Existing
`debug()` output therefore cannot reveal new values, and no historical host,
identity, interaction, or access fact is invented.

Legacy is an internal staged-record mode. New stage, new apply, and direct crate
calls always create a current context. A legacy record stays legacy through
continuation and staged apply, then follows the existing removal lifecycle.

## Module and seam

```text
Template::load ─► retained render program + environment need
                                      │
stage/direct decision + fixed source ─┴─► EnvironmentSnapshot

registry/config + resolver ───► SelectedTemplate ─┐
canonical_target factory ─────► CanonicalTarget ──┤
host adapter ─────────────────► HostFacts          ├─► InvocationContext
originating driver ───────────► ExecutionFacts     │          │
environment admission ────────► snapshot ──────────┘          │
                                                              ▼
presets + ConfigEntry origins ─► configured_defaults ─► Resolution
                                                       (raw + origin entries)
                                              │ start_with_context /
                                              │ replay_with_resolution
                                              ▼
                                     configured DefaultBank ─► Pending
ordinary flat defaults ─────────────────────► Seed ────────┤
                                                                │
                      StagedRecord ◄──── serialize/replay ──────┤
                                                                ▼
                                                            Completed
                                                              │
data + answers + now + InvocationContext ─► one Jinja projection
                                      │                       │
                                      └──── interview ────────┴─► Plan
```

`template` owns compilation, source/include discovery, the analysis walk,
reference locations, and exact-name collision validation. `context` owns typed
facts, projection, redaction, and wire conversion. The command adapter owns
current trust, host/process capture, and command-specific decisions.
`interview` carries the immutable context. `staging` owns record I/O,
reconstruction, and replay. `plan` consumes the completed context and the
producer carrier.

Protocol `Context { target, template, commit }` remains output metadata. It is
not the Jinja evaluation context.

One private context builder combines template data, accumulated answers,
`__toha_now`, and the current invocation projection. Readiness, interview
rendering, and planning use the same builder. Legacy mode uses the same builder
without the new projection.

## Collision and availability contract

For current contexts, only the seventeen exact public names are reserved.
`Template::load` rejects a matching top-level data key, question id, computed
id, `each` binding, or callable/global collision, including nested interview,
file-rule, and hook positions. The aggregate load error names every authored
location. Nested object properties remain ordinary keys. Other `toha_` names
remain legal.

The same exact set is registered as available for current load-time reference
checking and runtime readiness. Every value is available in terminal, headless,
staged/resumed, and direct execution at every compiled surface.

Legacy staged loading uses the pre-context available/reserved set. It does not
turn an existing authored identifier into a collision while the old stage is
being completed.

## Errors and compatibility

| Situation | Result |
| --- | --- |
| Reserved current-context identifier | Aggregate `LoadError` with authored locations. |
| Unmodeled supported AST form | Conservatively marks all five observable. |
| Stage need without `--trust` | The stage adapter maps only `EnvironmentAdmissionError::TrustRequired { reference }` to `StagingError::EnvironmentTrustRequired { reference }` before capture, seed, interview, render, submission, or write. |
| Stage without need or `--trust` | `Unavailable`, zero environment reads; later-added references remain null. |
| Stage with `--trust` | Capture all five once, even without an initial reference; carry them through replay. |
| Target construction or stored-target mismatch | Producer `StagingError`; no consumer normalization. |
| Invalid selected identity or context wire | Attributed context/replay error without captured values. |
| Plan target differs from completed current context | `PlanError::ContextTarget` before render or I/O. |
| Missing or malformed host metadata | Typed `null`/`[]` fallback. |
| Denied, unnecessary, or absent gated value | Jinja `none`. |
| Mutable folder changes after stage | Current bytes render under the recorded snapshot; no new access decision. |
| Legacy record | Pre-context projection; no invented facts or access recovery. |
| Interview or plan evaluation fails | Existing attributed evaluation/render error. |

The stage command has one typed adapter for admission errors:

```rust
pub enum StagingError {
    // Existing variants remain.
    EnvironmentTrustRequired { reference: RenderOrigin },
    EnvironmentAdmission {
        #[source]
        source: EnvironmentAdmissionError,
    },
}

pub(crate) fn stage_admission_error(
    error: EnvironmentAdmissionError,
) -> StagingError {
    match error {
        EnvironmentAdmissionError::TrustRequired { reference } =>
            StagingError::EnvironmentTrustRequired { reference },
        source => StagingError::EnvironmentAdmission { source },
    }
}
```

The trust-refusal variant carries the authored `RenderOrigin`. The catch-all
variant retains the original typed admission error as its source, including any
authored location and evaluator detail; it is not relabeled as a trust refusal.
This adds no protocol field or wire variant.

Adding required `Seed.context` is a deliberate source break for ordinary crate
callers, but `Seed.defaults` remains the flat application-default route.
The configured replay signature likewise gains the restored
`InvocationContext`; the paired implementation changes its internal command
callers after the producer implementation is integrated. The producer's
origin-bearing `Resolution` and method identity remain intact.
Configured start consumes the private origin-bearing `Resolution` through
`start_with_context`; configured replay consumes it through
`replay_with_resolution` with the restored context. Both move the producer's
`ResolvedDefault` entries into the private configured default bank.
`configured_defaults` remains the sole origin source and resolver; no second
origin map is added. The public interview state machine
and apply result types otherwise remain. Existing `now()` behavior remains.
Source-tree Jinja compilation moves to template load, so an attributable
syntax/include error can occur earlier.

## Canonical document changes for implementation

The paired implementation updates:

- the template format and schema with the exact variable table, types,
  availability, collision locations, and render surfaces;
- the Jinja guide with direct, staged, denied, mutable-folder, and legacy
  examples;
- the command-line contract with `stage --trust`, the pre-interview refusal,
  no `continue --trust`, and staged apply's separate hook meaning;
- the interview protocol with the versioned current/legacy context wire and
  unchanged per-call input modality; and
- the registry contract only by reference to the approved live review
  evaluator; the digest algorithm is not duplicated.

## Behaviors to prove

1. **All surfaces:** every configuration render, source path/body, explicit
   body, and transitive literal include contributes environment needs before
   interview start and receives the same context at render time.
2. **Static analysis:** direct reads, self-shadowing assignment, value aliases,
   `debug()` aliases, dynamic operands, false branches, uncalled macros, and
   uncertain AST forms are covered. A literal property string alone is not a
   root read.
3. **Failure order:** `stage_refs_without_trust_fails_before_progress` returns
   the attributed error while spies prove zero gated reads, seed construction,
   interview starts, renders, submissions, and store writes.
4. **Capture matrix:**
   `stage_no_refs_without_trust_records_unavailable_without_reads` proves the
   no-flag/no-need result;
   `stage_no_refs_with_trust_captures_fixed_five_once` proves the carried grant;
   `stage_refs_with_trust_captures_fixed_five_once` proves the admitted need;
   and `direct_denial_reads_nothing` proves denial. Empty and non-Unicode values
   become null.
5. **Replay:** environment, host, approval, privilege, and terminal changes do
   not change later batches. Continue has no trust flag. Staged apply trust
   changes hook permission only.
6. **Mutable folders:**
   `mutable_folder_no_flag_added_reference_stays_null` proves no-need/no-flag
   replay with zero read or gate;
   `mutable_folder_carried_grant_supplies_later_reference` proves that an
   explicit no-need stage grant supplies its recorded value after an edit; and
   an initially referenced editor followed by a shell reference uses the same
   fixed-five snapshot.
7. **Legacy:** a no-context record preserves pre-context availability,
   collision, and `debug()` output across continuation and staged apply.
8. **Target:** every identity-sensitive caller receives the sole factory
   carrier; stored text is validated; root/non-Unicode basename is null; a
   mismatched plan carrier fails before render.
9. **Trust:** explicit flag, eligible matching named approval, changed digest,
   folder, direct Git, bundled, local-layer, and stage-specific cases prove the
   approved distinction.
10. **Identity and host:** aliases, source, formal/short name, Linux host
    fallbacks, admin detection, and originating interaction keep exact types
    and remain frozen.
11. **Collisions and presets:** every exact authored collision is attributed; a
    neighboring `toha_` name loads; presets remain answer defaults only;
    configured start and configured replay preserve the winning mapping/preset
    origin through a configured-default constraint fault and recovery, while
    the separate ordinary `Seed.defaults` route stays flat and unchanged.
12. **Redaction and wire:** captured strings appear only in intended rendered
    output and staged JSON, never in `Debug`, errors, protocol metadata, or
    guidance. The wire has no target carrier, generic map, or access token.
13. **Caller parity:** terminal, headless, staged, resumed, and direct callers
    with equal typed facts produce equal answers and plans.
14. **Regression:** the repository gate covers `now()`, protocol, presets,
    staging, plan, apply, hook review, and includes.

## Out of scope

- Runtime or shared canonical-document edits in this design task.
- Arbitrary environment access, secrets, or a dynamic environment function.
- A changed review digest or new registry trust commands.
- A target normalizer, unchecked carrier constructor, or target error type.
- Configured-default behavior, provenance, or naming changes.
- Encryption or a new staged-file permission policy.
- Timeout mechanics, pinned-version checks, or application subprocesses.
- A general public-engine/session redesign.

## Approved contract closure

The approved contract permits the bounded plaintext fixed-five snapshot in the
existing staged JSON. A stage without an initial need or flag records no value.
An explicit `stage --trust` records all five optional values even without an
initial reference. The existing state-directory and process-umask behavior
applies; values remain until successful staged apply, `abort`, or operator
removal.

Mutable folders render current bytes under the recorded decision. Unavailable
stays null; a carried grant supplies the recorded fixed-five snapshot. Replay
performs no live environment read, trust check, or access abort/retry, and adds
no source/program identity refusal.

## Implementation handoff

Implement the complete retained render program and AST analysis first. Prove
the admission matrix and all-surface tests before threading the current/legacy
context through the engine and staged wire.
