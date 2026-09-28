---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Stage environment admission

## Caller usage first

The public Jinja context remains ordinary variables. A template may use a value directly, through an alias, or through `debug()` in any supported expression. Stage admits the possible observations before it asks a question.

```jinja
{% set preferred = toha_env_editor %}
editor = {{ preferred if preferred is not none else "none" }}
{{ debug() }}
```

```console
toha stage --trust sample ./output --async
toha continue ./output answers.json
toha apply ./output                 # its --trust still decides hook execution
toha apply sample ./output          # explicit trust or eligible live review
```

The command adapter crosses one admission seam after template load. `stage` uses an explicit grant only. New `apply` uses explicit trust or a live, matching approval. A denied direct invocation receives five nulls and makes no gated read.

```rust
let target = staging::canonical_target(path)?;
let resolved = resolve_template(operand, &config, &registry, &dirs, cwd)?;
let template = Template::load(&resolved.folder)?; // retains every compiled render source
let access = match operation {
    NewStage { trust: true, .. } => EnvironmentDecision::Grant,
    NewStage { trust: false, .. } => EnvironmentDecision::RequireExplicitStageTrust,
    NewApply { trust, .. } => direct_environment_decision(
        trust, &resolved, &template, &registry, // live evaluate_trust/HookSurface
    )?,
};
let admitted = template.admit_environment(access, &mut five_value_reader)?;
let context = InvocationContext::new(
    target, SelectedTemplate::from_resolved(&resolved),
    HostFacts::capture(), ExecutionFacts::new(is_admin(), originating_interactive),
    admitted,
)?;
let Resolution { defaults, warnings } = configured_defaults(
    &resolved.formal_name, &template, &config.presets, &config.template_defaults,
)?;
report(warnings);
let pending = Interview::start(&template, Seed { now, defaults, context })?;
```

`HostFacts::capture` and `is_admin` above are command adapters. They do not run inside `Interview`. Their order relative to admission can be chosen by the command, but `Seed`, interview evaluation, and staged writes occur only after admission succeeds. A direct crate caller supplies facts and a typed reader without ambient engine reads:

```rust
let target = staging::canonical_target(Path::new("./output"))?;
let template = Template::load(folder)?;
let env = template.admit_environment(EnvironmentDecision::Deny, &mut no_reads)?;
let context = InvocationContext::new(target, selected, host, execution, env)?;
let completed = finish(Interview::start(&template, Seed { now, defaults, context })?)?;
let plan = Plan::build(&template, &completed)?;
```

## Domain shape and interface

```rust
pub struct Template {
    // Existing parsed configuration and identity.
    sources: CompiledSources, // private, immutable after load
}

struct CompiledSources {
    renders: Vec<CompiledRender>, // includes config, explicit bodies, tree paths/bodies
    needs: EnvNeeds,              // union over every render and literal include closure
    // Keys are authored source identity, not paths reread at plan time.
    by_source: SourceIndex,
}
struct CompiledRender {
    origin: RenderOrigin, // authored path + field/span or included file
    compiled: CompiledJinja,
    needs: EnvNeeds,
}
#[derive(Clone, Copy)]
struct EnvNeeds(u8); // five fixed bits; union, contains, empty

pub enum EnvironmentDecision { Grant, Deny, RequireExplicitStageTrust }
pub trait FiveValueReader {
    fn read(&mut self, name: EnvironmentName) -> Option<String>;
}
pub enum EnvironmentName { User, Hostname, Editor, Shell, Visual }
pub struct EnvironmentSnapshot {
    // Private; exactly the needed slots. Missing slot = unreferenced or denied.
    values: [Option<Option<String>>; 5],
}

impl Template {
    pub fn load(folder: &Path) -> Result<Self, LoadError>; // TODO compile all sources
    pub fn admit_environment(
        &self, decision: EnvironmentDecision, reader: &mut impl FiveValueReader,
    ) -> Result<EnvironmentSnapshot, AdmissionError>; // TODO use sources.needs
}
pub enum AdmissionError {
    StageTrustRequired { first_reference: RenderOrigin },
}

pub struct InvocationContext {
    target: CanonicalTarget,
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
    environment: EnvironmentSnapshot,
}
pub struct SelectedTemplate { formal_name: String, aliases: Vec<String>, source: Option<String> }
pub struct HostFacts {
    os: String, arch: String, os_name: Option<String>, os_id: Option<String>,
    os_id_like: Vec<String>,
}
pub struct ExecutionFacts { is_admin: bool, is_interactive: bool }
pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}
pub struct Pending { /* existing state + immutable InvocationContext */ }
pub struct Completed { /* existing state + immutable InvocationContext */ }

impl InvocationContext {
    pub fn new(target: CanonicalTarget, selected: SelectedTemplate, host: HostFacts,
        execution: ExecutionFacts, environment: EnvironmentSnapshot) -> Result<Self, ContextError>;
    pub fn target(&self) -> &CanonicalTarget;
}
impl Plan {
    pub fn build(template: &Template, completed: &Completed) -> Result<Self, PlanError>;
    // TODO consume template.sources, never reopen a Jinja source
}
```

`EnvironmentSnapshot` construction stays private to admission and staging reconstruction. `EnvironmentDecision::Deny` returns an empty snapshot without calling the reader. A granted admission reads only `needs` slots, once, and records `Some(None)` for a referenced but absent value. `RequireExplicitStageTrust` returns `StageTrustRequired` if any bit is set, before a reader call; with no bits it returns the empty snapshot. `StageTrustRequired` is surfaced as `StagingError::StageTrustRequired { origin }` with guidance to start a new stage using `stage --trust`. The origin comes from the first stable catalog reference, without printing a value.

The exact public Jinja names and types remain: `toha_target_name` string/null; `toha_template_name` and `toha_template_formal_name` strings; `toha_template_aliases` string array; `toha_template_source` string/null; `toha_host_os` and `toha_host_arch` strings; `toha_host_os_name` and `toha_host_os_id` string/null; `toha_host_os_id_like` string array; `toha_is_admin` and `toha_is_interactive` booleans; `toha_env_user`, `toha_env_hostname`, `toha_env_editor`, `toha_env_shell`, and `toha_env_visual` string/null. All names are always present. The private Jinja projection maps an absent snapshot slot and a present null to Jinja `none`, so a template cannot distinguish denied from unavailable. No environment map or dynamic environment function is exposed.

## Complete load analysis and retained source identity

`Template::load` compiles every configuration render listed in the grounding, explicit `files:` bodies and their transitive literal includes, and every rendered source-tree path segment and file body with its literal include closure. It enumerates the tree at load, retains compiled path/body objects and source metadata, and gives planning that catalog. Planning uses those compiled objects and source bytes; it never rereads a source-tree path, body, or include for rendering. This is the single owner of analyzed/rendered content identity. The target path still comes only from `staging::canonical_target`.

The internal analyzer walks the same parsed MiniJinja AST used for compilation, in evaluation order. It tracks lexical bindings and possible aliases as a five-bit environment taint plus a `debug` callable taint. It visits every expression, filter/function argument, dynamic attribute and item expression, assignment right-hand side before binding the left, macro body, and branch regardless of reachability. A root read of `toha_env_*` sets its bit; `object["toha_env_user"]` alone does not. A dynamic key expression is visited. A possible call through the built-in `debug` or its alias sets all five bits, including when local shadowing makes the result conservative. Literal include closures are unioned. Any supported AST form not modeled conservatively fails template load as an attributable analysis error; it does not silently classify the form as safe. Existing syntax remains supported. A future generic context lookup needs an explicit new sound rule.

The catalog preserves authored `RenderOrigin` through include expansion, compilation, evaluation, and plan rendering. `LoadError.problems` aggregates compilation, include, analysis, and reserved-name collisions with authored locations. Earlier parsing of a bad source-tree body may move that load error before interview; the attributed error remains the same kind of template fault. Stable catalog order supplies deterministic first reference and diagnostics. The source walk is complete at load; files added after load are outside that invocation's catalog.

## Stage wire and replay

The staged record has a versioned private wire context, separate from `InvocationContext` and `CanonicalTarget`:

```rust
#[derive(Serialize, Deserialize)]
struct StagedContextWire {
    target: PathBuf,                 // validated serialized identity, not a carrier
    selected: SelectedTemplateWire,
    host: HostFactsWire,
    execution: ExecutionFactsWire,
    environment: EnvironmentWire,
}
#[derive(Serialize, Deserialize)]
struct EnvironmentWire {
    referenced: u8, // fixed five-bit mask from admission
    // Fixed field order; present only where referenced bit is set.
    user: Option<Option<String>>,
    hostname: Option<Option<String>>,
    editor: Option<Option<String>>,
    shell: Option<Option<String>>,
    visual: Option<Option<String>>,
}
```

The JSON encoder omits unreferenced fields. A referenced absent value is JSON `null`; a referenced present value is a JSON string. The mask distinguishes referenced absence from an omitted field. For denied direct runs no values are read or stored; a stage with any reference cannot be denied. There is no separate persisted grant or permission token. `Store::save` writes this context with the existing staged record. The existing directory and process umask govern the pretty JSON file; no new file mode or encryption is implied. Plaintext survives until successful staged apply, `abort`, or operator removal.

```rust
impl Store {
    pub fn load(&self, target_path: &Path) -> Result<LoadedStage, StagingError>;
    // TODO parse wire, validate shape and identity, construct live context
}
pub struct LoadedStage { pub template: Template, pub seed: Seed, /* submissions, metadata */ }
```

`Store::load` parses the wire, rejects unknown bits and mask/field mismatch, checks record target and selected identity consistency, calls `staging::canonical_target(&wire.target)` and compares `as_path()` with the serialized target, then builds `InvocationContext` from that factory-created carrier and typed facts. It never deserializes or manufactures `CanonicalTarget`. A malformed or changed target is `StagingError::Replay { field: "target" }`, with the producer's `StagingError` retained as cause where useful. It loads the stage's selected template through existing revision selection, reconstructs `Seed` with recorded `now` and context plus the established live configured defaults, then replays ordered submissions. It does not recheck admission, read current host or environment, or require `continue --trust`. Staged apply uses this same replay; its current `--trust` controls hooks only. New stage and apply use the loaded `Template` catalog throughout that invocation. On resume, the selected source revision must be stable under the existing stage selection contract; if the catalog differs and an exact revision cannot be recovered, replay fails as a source/replay error before evaluation rather than taking fresh environment values.

Every legacy staged record lacking a context snapshot fails `StagingError::LegacyContextUnavailable` before replay or rendering. The diagnostic names `abort` and a new `stage` as recovery. It does not infer host, aliases, interactivity, or environment facts, and it does not use exact-name scans to claim `debug()` cannot observe them. No legacy record is silently upgraded.

## Ownership and flow

| Module | Ownership |
| --- | --- |
| `template` | Full source discovery, compile, literal include closure, AST needs, retained catalog, authored origins, exact-name collision validation. |
| `context` | Typed immutable facts, five-slot snapshot, exact seventeen-name Jinja projection and redacted diagnostics. |
| command adapter | Resolve selected identity, capture host/admin facts and five gated values, choose direct or stage decision. |
| `interview` | Pure `Seed` to `Pending` to `Completed` progression using the single projection. |
| `plan` | Render retained compiled sources and completed context; no source or ambient context lookup. |
| `staging` | Wire schema, atomic record I/O, target factory, validation, replay, legacy error. |

The selected formal name is the resolver's stable identity. A named user/system entry supplies aliases in registry order and `Entry.source`; folder, direct Git, and bundled selections supply `[]` and null, with bundled formal identity `toha-demo`. Local alias overlays do not create approval eligibility. Direct approval requires an eligible named user/system entry and `evaluate_trust` against the live `HookSurface` digest; `approval.is_some()` alone is insufficient. That review covers hook nodes and executed hook files, not every Jinja source, so a matching approval can admit environment access to changed non-hook Jinja content. Explicit `--trust` remains the other direct grant.

The canonical target's final Unicode component becomes `toha_target_name`; root and non-Unicode final components become null. `CanonicalTarget::as_path()` is the only path projection. Host fallback and admin rules follow the current context contract: Linux os-release parsing without execution, null/empty fallbacks, fixed OS/arch vocabulary, positive admin observation only. User is `USER` on Unix or `USERNAME` on Windows; hostname uses a native observation; editor, shell, and visual are independent. Empty, absent, or non-Unicode gated values become null. `toha_is_interactive` records the originating driver's ability to prompt and stays unchanged on continuation. Config presets remain flat `Seed.defaults`, outside Jinja context.

The seventeen exact names are reserved at load against top-level data, question and computed identifiers, local `each` names, and callable/global collisions, with authored locations. The whole `toha_` prefix is not reserved. Every render surface, including readiness, messages, hook expressions, source paths, file bodies, and before/after messages, receives the same private projection. Protocol output metadata remains separate.

## Failure order, compatibility, and tests

For a new stage, prerequisite target, configuration, resolution, and complete template-load failures may occur first. The `StageTrustRequired` check is next and precedes `Seed`, `Interview::start`, messages, questions, submissions, and staged writes. The command does not read the five gated sources on refusal. New direct/apply denial proceeds with nulls. A staged record's snapshot is authoritative for later batches, independent of current registry approval. Adding required `Seed.context` and the opaque target carrier is a deliberate crate source change; there is no unchecked fallback. Existing templates without exact-name collisions retain syntax and rendering semantics, subject to earlier attributed source-tree load errors. Legacy staged records require restaging.

Falsifiable tests:

1. A fixture uses each configuration render surface, tree path and body, explicit body, and nested literal includes; admission finds all five possible references before `Interview::start`, and plan renders only catalogued compiled bytes after on-disk mutation.
2. AST fixtures cover `debug()`, alias to `debug`, `{% set toha_env_user = toha_env_user %}`, alias chains, uncalled macros, false branches, function/filter arguments, dynamic item keys, and literal object keys. The last alone adds no need; every possible root read does.
3. `stage` without `--trust` and a single reference returns `StagingError::StageTrustRequired` with origin. Spies show zero gated reads, zero interview calls, zero renders, and zero staged writes. A no-reference stage succeeds without trust.
4. A trusted stage references editor only. Its JSON has the editor bit and either an editor string or explicit null; four other fields are omitted. Change all five ambient values and registry approval before two `continue` batches and staged apply; all renders use the stored editor and no host/environment reads occur.
5. `Store::load` rejects unknown masks, missing referenced fields, fields without bits, changed target, and malformed identity. It calls the producer target factory; a compile-time API check shows no `CanonicalTarget` deserialization or raw constructor.
6. Direct trust fixtures test explicit flag, matching eligible review, changed digest, local alias overlay, folder, direct Git, and denied path. A spy proves denied paths make zero gated reads. A changed non-hook Jinja body with matching hook digest demonstrates the documented review scope.
7. Terminal, headless, asynchronous stage, continuation, new apply, staged apply, and direct crate callers give identical output from identical typed facts. Admin and originating interactive values remain stable across resume; hook trust affects hooks only.
8. A fixture checks all seventeen values and types at interview and plan surfaces, aliases and basename edge cases, host fallbacks, exact collisions at every authored location, neighboring `toha_` names, and preset defaults outside Jinja.
9. Every legacy no-context record fails with `LegacyContextUnavailable`, including a template using only `debug()`; no upgrade or ambient read occurs. Snapshot/debug/errors/protocol diagnostics do not reveal gated strings except the intended staged JSON and rendered output.
