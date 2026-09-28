---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Stage environment admission

## Caller usage first

A template author can use the fixed invocation variables anywhere Toha renders Jinja, including a literal file-body include:

```jinja
{% if toha_env_editor is not none %}editor={{ toha_env_editor }}{% endif %}
{% set toha_env_user = toha_env_user %}
{{ debug() }}
```

The command driver crosses one admission seam after `Template::load` and before `Seed`. It supplies typed facts; the loaded template owns source analysis and content identity.

```rust
let target = staging::canonical_target(target_path)?;
let resolved = resolve_template(operand, &config, &registry, &dirs, cwd)?;
let template = Template::load(&resolved.folder)?; // compiles all render sources
let Resolution { defaults, warnings } = configured_defaults(
    &resolved.formal_name, &template, &config.presets, &config.template_defaults,
)?;
report(warnings);
let admitted = template.admit(AdmissionRequest::Stage {
    explicit_trust: stage_args.trust,
    facts: InvocationFacts::new(
        target, SelectedTemplate::from_resolved(&resolved),
        host.capture(), ExecutionFacts::new(is_admin(), !stage_args.r#async),
    )?,
}, &mut five_value_reader)?;
let pending = Interview::start(&template, Seed { now, defaults, context: admitted.context })?;
store.save(&pending, &admitted.source_identity)?;
```

`stage` without `--trust` receives `AdmissionError::StageTrustRequired` if *any* compiled render source can observe the five values. Its environment reader is not called. In a direct or new-apply path, the driver computes the approved live decision and passes it to the same seam:

```rust
let grant = args.trust || (resolved.eligible_named_user_or_system_entry()
    && evaluate_trust(resolved.approval.as_ref(), HookSurface::of(&template)?.digest()));
let admitted = template.admit(AdmissionRequest::Direct {
    grant, facts,
}, &mut five_value_reader)?;
let completed = finish(Interview::start(&template,
    Seed { now, defaults, context: admitted.context })?)?;
let plan = Plan::build(&template, &completed, completed.context().target())?;
```

A direct crate caller supplies the five optional strings as typed facts and uses no command adapter:

```rust
let target = staging::canonical_target(Path::new("./output"))?;
let template = Template::load(&folder)?;
let admitted = template.admit_supplied(AdmissionRequest::Direct {
    grant: true,
    facts: InvocationFacts::new(target, selected, host, execution)?,
}, TrustedEnvironment::new(None, None, Some("vi".into()), None, None))?;
let completed = finish(Interview::start(&template,
    Seed { now, defaults, context: admitted.context })?)?;
let plan = Plan::build(&template, &completed, completed.context().target())?;
```

`admit_supplied` applies the same decision and need rules as `admit`; it never queries ambient state. A denied direct run supplies no values and receives five Jinja `none` values. `continue` loads the saved context and source identity and replays ordered submissions. Staged `apply --trust` uses that context; its flag controls hooks only.

## Shape and module ownership

```rust
// template module: private wire/parser details; public domain result
pub struct Template {
    // existing parsed configuration
    render_sources: RenderSources, // compiled config fields, source paths/bodies, include closures
    admission: ReferenceManifest,
    source_identity: SourceIdentity,
}
pub struct ReferenceManifest {
    environment: EnvironmentNeed, // None or FiveFixed; private representation
    context_names: NameReferences,
}
pub struct SourceIdentity {
    // deterministic digest of every exact analyzed render source and literal include edge,
    // including relative path, kind, and bytes; excludes mutable non-render assets
    digest: [u8; 32],
}
pub struct AdmittedInvocation {
    pub context: InvocationContext,
    pub source_identity: SourceIdentity,
}
pub enum AdmissionRequest {
    Stage { explicit_trust: bool, facts: InvocationFacts },
    Direct { grant: bool, facts: InvocationFacts },
}
pub enum AdmissionError {
    StageTrustRequired { location: RenderLocation },
    InvalidFacts { field: &'static str },
}
impl Template {
    pub fn load(folder: &Path) -> Result<Self, LoadError>;
    pub fn admit(
        &self, request: AdmissionRequest, reader: &mut impl FixedEnvironmentReader,
    ) -> Result<AdmittedInvocation, AdmissionError>;
    pub fn admit_supplied(
        &self, request: AdmissionRequest, values: TrustedEnvironment,
    ) -> Result<AdmittedInvocation, AdmissionError>;
}
pub trait FixedEnvironmentReader {
    fn read_five(&mut self) -> TrustedEnvironment;
}

// context module: private Jinja projection and redacted Debug
pub struct InvocationFacts {
    target: CanonicalTarget,
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
}
pub struct InvocationContext {
    facts: InvocationFacts,
    environment: EnvironmentSnapshot,
}
pub enum EnvironmentSnapshot {
    Denied,
    Admitted(TrustedEnvironment), // exactly five Option<String> fields
}
pub struct TrustedEnvironment {
    user: Option<String>, hostname: Option<String>, editor: Option<String>,
    shell: Option<String>, visual: Option<String>,
}
pub struct SelectedTemplate {
    formal_name: String, aliases: Vec<String>, source: Option<String>,
}
pub struct HostFacts {
    os: String, arch: String, os_name: Option<String>,
    os_id: Option<String>, os_id_like: Vec<String>,
}
pub struct ExecutionFacts { is_admin: bool, is_interactive: bool }
impl InvocationContext {
    pub fn target(&self) -> &CanonicalTarget;
}

// staging module: private wire; producer owns target construction
pub fn canonical_target(path: &Path) -> Result<CanonicalTarget, StagingError>;
impl CanonicalTarget { pub fn as_path(&self) -> &Path; }
pub struct StagedRecordV2 { /* existing record fields, context_wire, source_identity */ }
struct ContextWire {
    target_path: PathBuf, selected: SelectedTemplateWire,
    host: HostFactsWire, execution: ExecutionFactsWire,
    environment: EnvironmentWire,
}
enum EnvironmentWire {
    Denied,
    Admitted { user: Option<String>, hostname: Option<String>, editor: Option<String>,
               shell: Option<String>, visual: Option<String> },
}
impl Store {
    pub fn load(&self, target: &Path, template: &Template) -> Result<Pending, StagingError>;
}
```

The public interface is `Template::load` followed by one admission call, then the existing engine. The load operation hides source discovery, AST analysis, include expansion, and content pinning. Admission hides the stage refusal and fixed value capture. No caller coordinates a scan or enumerates variable names. `AdmissionRequest` exposes only the two real policy modes. The command owns trust decisions and host/process capture; the engine receives only domain values. `FixedEnvironmentReader` is the command adapter seam and returns exactly five values, never a map.

`RenderSources` keeps compiled source-tree path segments and bodies, explicit file bodies, and each selected literal include closure. Planning renders these compiled objects, not a fresh filesystem read. For an implementation that cannot retain a compiled object across process restart, `Store::load` loads the template anew and compares its `SourceIdentity` with the staged identity **before** `Seed`, replay, or render; mismatch returns `StagingError::SourceChanged`. The digest includes exact Jinja source bytes, path/kind separators, and include edges so reordered or substituted sources cannot collide by concatenation. The loaded template is held through an invocation, and plan consumes that same instance. Non-render files keep their existing plan behavior; the admission invariant covers every Jinja render source.

`Template::load` compiles every supported configuration expression and template, every non-static source-tree path segment and body, and transitive literal includes of file bodies. It does not render them. Its conservative MiniJinja AST walk visits right-hand sides before binding assignment targets, follows expression arguments and dynamic item/attribute expressions, and tracks lexical aliases of the five names and `debug`. Possible calls of built-in `debug`, including aliases, mark all five needed. A shadowed local `debug` is handled by the walk's scope state; uncertainty marks all five. The walk is control-flow insensitive, so dead branches and uncalled macros count. A literal property key is not a root lookup. The manifest unions every supported literal include closure and records an attributed first need location. `undeclared_variables(false)` may support unrelated readiness work but cannot authorize environment access. The load boundary rejects unsupported dynamic include/import/extends/block forms under the existing include design; it does not add a Jinja restriction.

The analysis covers question prompt, description, placeholder, default, required, when, computed value, format, options, numeric bounds; interview message text/condition; interview and top-level hook conditions, iteration expressions, command arguments/lists, working directories; explicit file iteration/condition/target/body; before/after apply messages; ordinary source-tree path segments and bodies. It does not recursively parse regexes, hook script paths, file-rule source paths, source-directory names, globs, names, descriptions, answers, configured defaults, or data values. YAML `!include` is configuration loading, not a Jinja include.

## Admission, context, and wire

The five fixed gated observations are user, hostname, editor, shell, and visual. `EnvironmentNeed::FiveFixed` means any one can be observed; `debug()` alone is sufficient. After an admitted stage need, `read_five` captures **all five** optional values once. This deliberately stores at most five plaintext strings, even if only one name was spelled. It makes `debug()` and alias handling replay proof simple: no future source can require a sixth or uncaptured fixed value. If no need exists, admission creates `Denied` and calls no reader even with `stage --trust`; a direct granted run may capture the five for its live render contract. A denied direct run calls no reader and projects five `none` values. `Admitted { editor: None }` means the editor observation was allowed but absent at capture; `Denied` means no value was read. The manifest distinguishes unreferenced from referenced. A template never sees that distinction: all seventeen keys exist, and absence projects `none`.

The staged wire stores the complete fixed non-environment facts, either `Denied` or the five optional strings, and `SourceIdentity`. It never stores a generic environment map, permission token, or serialized `CanonicalTarget`. `Store::load` parses the wire, calls `staging::canonical_target(&wire.target_path)?`, checks `as_path()` against the saved path and the record/lookup target, validates selected identity and source identity, then constructs `InvocationContext` with that factory-made carrier. A mismatch is `StagingError::Replay` with the field name and no value. It re-resolves live configured defaults into the existing flat `Resolution { defaults, warnings }`, uses the recorded instant and submissions, and does no host, environment, registry grant, or privilege read. The wire values remain readable plaintext in the existing staged JSON until successful staged apply, abort, or operator removal; existing directory/umask behavior applies. `Debug`, error messages, protocol output, and command guidance redact them.

A V1 record without a snapshot fails `StagingError::LegacyContextUnavailable` before replay, with `abort` and `stage` recovery guidance. This applies even when an exact-name scan finds no match: `debug()` and other indirect access make that scan unsound, and old host/execution facts cannot be recreated. The record is left intact for explicit recovery. Newly staged records never retry admission on resume.

The private context builder combines template data, flat defaults/answers, `__toha_now`, and exactly these seventeen variables for readiness, interview renders, and plan renders. It reserves only these exact names against top-level data, question/computed ids, `each` bindings, and callable/global collisions at template load, reporting authored locations through aggregate `LoadError.problems`. Neighboring `toha_` names remain legal; configured `presets` remain config defaults only. A plan target different from `completed.context().target().as_path()` yields `PlanError::ContextTarget` before render. No consumer normalizes a target.

| Variable | Jinja value |
| --- | --- |
| `toha_target_name` | final Unicode canonical-target component, else `none` for root or non-Unicode |
| `toha_template_name` | loaded `Template.name` |
| `toha_template_formal_name` | resolved stable formal name, not selected alias |
| `toha_template_aliases` | effective selected named-entry aliases in registry order, else `[]` |
| `toha_template_source` | selected named-entry source, else `none` |
| `toha_host_os`, `toha_host_arch` | Rust target OS and architecture strings |
| `toha_host_os_name`, `toha_host_os_id` | Linux os-release NAME/ID or `none` |
| `toha_host_os_id_like` | Linux ID_LIKE words in source order, else `[]` |
| `toha_is_admin`, `toha_is_interactive` | captured booleans; interactive is originating driver |
| `toha_env_user`, `toha_env_hostname`, `toha_env_editor`, `toha_env_shell`, `toha_env_visual` | captured string or `none` |

Folder, direct Git, and bundled selection have no named-entry aliases/source; bundled formal identity is `toha-demo`. Host capture uses `/etc/os-release` only on Linux, with quoted/unquoted values and last valid duplicate; unavailable/malformed/non-UTF-8 gives `none`, `none`, `[]`. Empty NAME/ID gives `none`; empty ID_LIKE gives `[]`. Admin is true only on positive effective-authority detection; failure is false. User reads `USER` on Unix or `USERNAME` on Windows; hostname uses a native observation. Empty, absent, or non-Unicode gated values become `None`; editor, shell, and visual have no cross-fallback. The adapter never launches a subprocess. All facts freeze at origin, including admin, host, selected identity, aliases, target, and originating interactivity; a terminal continuation of async stage still projects `false`.

## Failure order and compatibility

For `stage`, canonical target construction, selected-template resolution, live configuration, and complete `Template::load` may fail first. Next, before `Seed`, `Interview::start`, any render, or store write, `Template::admit` returns `AdmissionError::StageTrustRequired { location }` when `FiveFixed` and `--trust` is absent. Its error is typed and attributed to the first source that proves possible access, without exposing a value. An allowed stage captures once and carries those bytes through all batches. `continue` has no trust option; staged apply's existing `--trust` controls hooks only. Direct/new apply uses explicit trust or a current matching `evaluate_trust` approval for an eligible named user/system entry; local alias overlays, folder/direct-Git/bundled entries, or inherited digest alone do not grant. The approved `HookSurface` digest binds hook surfaces, not every Jinja source. A matching approval can therefore admit changed non-hook Jinja content; the UI must not call that content reviewed.

Adding required `Seed.context` is a 0.2.0 crate source break so direct callers supply typed facts. `CanonicalTarget` remains producer-owned, created only by `staging::canonical_target`, inspected only by `as_path()`. Existing `now()`, configured defaults, protocol output context, hook review digest, and per-call submission modalities keep their current ownership. Early compilation may move a source-tree parse error from planning to template load.

## Falsifiable tests

1. A stage fixture puts each gated root in every listed render surface, ordinary source path/body, explicit body, and nested literal include. Without `--trust`, each returns `StageTrustRequired` before spy seed/start/render/store counters increment; the reader count is zero.
2. `debug()`, an alias of `debug`, `{% set toha_env_user = toha_env_user %}`, environment aliases, dead branches, uncalled macros, function/filter arguments, and dynamic `object[toha_env_user]` require trust. Literal `object["toha_env_user"]` alone does not. Read order matches evaluation order.
3. Modify a source-tree body or include edge after stage. `continue` returns `SourceChanged` before replay. Modify source after load in one process: plan renders the compiled old bytes. Unchanged sources replay and produce identical output across batches.
4. With one referenced key and trust, the wire has exactly five optional fixed fields and no other environment key. A missing referenced value is wire `null`; denied has a different variant; no reader is called for a no-need stage. Stored values never appear in `Debug` or diagnostics.
5. Load a V2 record whose path is malformed or inconsistent. The producer factory or replay validation fails before context construction. A valid record uses a factory-made target and no consumer normalizer.
6. Direct explicit trust, eligible matching reviewed approval, changed digest, local overlay, folder, direct Git, and denied cases assert the five reader calls or zero calls. Stage requires its own flag even with matching approval. Staged apply trust cannot change saved values.
7. Continue after host and environment changes, in terminal and document modes. All seventeen projected values match stage origin. No environment/host spy read occurs. An old record always returns `LegacyContextUnavailable` with restage guidance before replay.
8. Exact reserved-name collisions in data, question, computed, nested each, and callable/global positions produce location-bearing load errors; adjacent `toha_` names load. Presets stay in defaults.
9. Root/non-Unicode target basenames are `none`; alias/formal selections share registry-order aliases/source; Linux host fallback and admin detection cases produce specified types. Plan target mismatch fails before render.
10. Terminal, headless, staged, and direct typed callers with equal facts and submissions yield equal answers and plans; `task ci` covers existing protocol, `now()`, presets, staging, plan, apply, and hook-review behavior.
