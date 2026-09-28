---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Admitted template package

## Caller usage

A caller loads one render program, admits its environment needs, then gives the admitted package and one immutable context to the existing engine. The caller does not run an analysis pipeline.

```rust
let target = staging::canonical_target(output)?;
let selected = resolve_template(operand, &config, &registry)?;
let package = AdmittedTemplate::load(&selected.folder)?;
let Resolution { defaults, warnings } = configured_defaults(
    &selected.formal_name, package.template(), &config.presets,
    &config.template_defaults,
)?;
report(warnings);
let access = direct_access(trust_flag, &selected, &package, &registry)?;
let context = InvocationContext::new(
    target, selected.identity(), host.capture(),
    ExecutionFacts::new(host.is_admin(), terminal_driver_selected),
    capture_needed(access, package.environment_needs(), &environment),
)?;
let seed = Seed { now, defaults, context };
let pending = Interview::start(&package, seed)?;
```

`direct_access` grants on explicit `--trust` or a current matching `evaluate_trust` result for an eligible named user/system registry entry and the live `HookSurface` digest. A folder, direct Git operand, bundled selection, or local alias overlay cannot inherit eligibility. A denied direct/new apply run reads none of the five gated sources and renders their globals as `none`.

Stage uses the same package with a stricter command decision:

```rust
let package = AdmittedTemplate::load(&selected.folder)?;
let access = stage_access(stage_trust, package.environment_needs())?;
// Err(StagingError::StageEnvironmentTrustRequired { names }) returns here.
let context = stage_context(target, selected, host, access, &package)?;
let pending = Interview::start(&package, Seed { now, defaults, context })?;
store.save(&pending, &package)?;
```

`stage_access(false, nonempty_needs)` fails before `Seed`, `Interview::start`, question/message rendering, or any staged write. Template selection, canonical target construction, configuration and `AdmittedTemplate::load` may fail first. `stage --trust` is the only stage grant; a registry approval does not substitute.

Continue and staged apply load the same recorded program identity and context, then replay without capture or access decisions:

```rust
let (package, seed, submissions) = store.load(&selected, &config)?;
let pending = Interview::replay(&package, seed, submissions)?;
let completed = pending.complete()?;
let plan = Plan::build(&package, &completed)?;
```

`continue` has no `--trust`; staged apply's existing `--trust` affects hook execution only. A direct crate caller constructs `InvocationContext` from typed `HostFacts`, `ExecutionFacts`, `SelectedTemplate`, and `EnvironmentSnapshot`; it calls `Interview::start` and `Plan::build` as above. The engine performs no ambient reads.

## Domain types and interface

```rust
pub struct AdmittedTemplate {
    template: Template,
    program: RenderProgram,
    needs: EnvironmentNeeds,
    identity: ProgramIdentity,
}

/// Exactly the five gated names; a set is ordered in the fixed enum order.
pub enum EnvironmentName { User, Hostname, Editor, Shell, Visual }
pub struct EnvironmentNeeds(EnumSet<EnvironmentName>);
pub struct ProgramIdentity([u8; 32]);

/// Opaque compiled configuration, source-tree paths and bodies, and literal
/// file-body include closures; all renders consume this program.
struct RenderProgram { /* compiled slots and source entries */ }

impl AdmittedTemplate {
    pub fn load(folder: &Path) -> Result<Self, LoadError>;
    pub fn template(&self) -> &Template;
    pub fn environment_needs(&self) -> EnvironmentNeeds;
    pub fn identity(&self) -> ProgramIdentity;
    // Internal render methods select compiled slots; no source recompilation.
}

pub struct InvocationContext {
    target: CanonicalTarget,
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
    environment: EnvironmentSnapshot,
}
pub enum EnvironmentSnapshot {
    Denied,
    Admitted(NeededValues),
}
/// Contains one Option<String> for each name in needs, including an absent read.
pub struct NeededValues { needs: EnvironmentNeeds, values: Vec<Option<String>> }

pub struct Seed { pub now: jiff::Zoned, pub defaults: IndexMap<Id, RawAnswer>, pub context: InvocationContext }
pub struct Completed { /* existing result and context */ }
impl Plan {
    pub fn build(package: &AdmittedTemplate, completed: &Completed) -> Result<Self, PlanError>;
}
```

`NeededValues` has private fields and a constructor that checks cardinality and enum order. It is never a generic environment map. `EnvironmentSnapshot::Denied` projects all five as `none`; `Admitted` projects captured `Some` strings and `None` for both referenced-but-absent and unreferenced names. The private tag and mask distinguish those cases for replay validation. No value is captured for an unreferenced name. `Debug` redacts strings. The exact seventeen globals are always present in the private Jinja projection: `toha_target_name`, `toha_template_name`, `toha_template_formal_name`, `toha_template_aliases`, `toha_template_source`, `toha_host_os`, `toha_host_arch`, `toha_host_os_name`, `toha_host_os_id`, `toha_host_os_id_like`, `toha_is_admin`, `toha_is_interactive`, `toha_env_user`, `toha_env_hostname`, `toha_env_editor`, `toha_env_shell`, `toha_env_visual`. `now()` remains separate. Config presets remain flat defaults, never Jinja context metadata.

The target name is the final Unicode component of `target.as_path()`, or `none` for a root/non-Unicode component. `SelectedTemplate` holds the loaded short name, stable formal name, registry-ordered effective aliases, and optional selected registry source. Folder/direct Git/bundled selections get `[]` and `none`; bundled formal name is `toha-demo`. Host OS/architecture are Rust target strings; Linux os-release `NAME`/`ID` are nullable and `ID_LIKE` is an array, with `none`/`[]` on unavailable data. Admin requires positive platform detection, otherwise false. Interactive records whether the originating driver could prompt, not the current continuation mode. The five gated optional strings use the approved platform/native sources and preserve absence without lossy conversion.

## Load, analysis, and render identity

`AdmittedTemplate::load` parses configuration and compiles every Jinja-bearing configuration slot. It also walks the complete source tree before returning, compiling each non-static path segment and each non-static file body. Explicit `files:` bodies and ordinary source-tree bodies recursively resolve and compile permitted literal includes. The program owns these compiled sources and include closures; planning never opens or recompiles source Jinja. It retains bytes needed for output and a stable source-location manifest. Its `ProgramIdentity` is a deterministic digest of the loaded configuration render sources, all source-tree paths and bytes, and resolved include identities in canonical order. It is an identity check, not a permission or trust token. A changed folder requires a new load/admission; an in-flight package continues to render its owned bytes. On replay, `Store::load` reloads the package and requires identity equality before submission replay; changed or missing bytes fail `StagingError::ProgramChanged` with restage guidance. No fresh environment read follows a mismatch.

The private analyzer walks the compiled MiniJinja AST conservatively, in evaluation order. It records root reads before later bindings can shadow them, including assignment right-hand sides and self-shadowing constructs. It tracks aliases of gated values and of the built-in `debug` callable; any possible call of that callable marks all five needs, including in macros or false branches. It visits filter/function arguments and dynamic attribute/item expressions. A literal property string such as `object["toha_env_user"]` is not a root read; `object[toha_env_user]` is. It unions every literal include closure. It preserves supported Jinja syntax; dynamic include names, imports, extends, and blocks retain their separately approved rejection. Any future generic context lookup needs an explicit admission rule. `undeclared_variables(false)` may still serve ordinary reference diagnostics but never grants access.

At load, reserve exactly the seventeen names against top-level data keys, question/computed ids, local loop bindings, and callable/global collisions across nested interview, file, and hook nodes. Aggregate authored locations in `LoadError.problems`. Neighboring `toha_` names remain legal. The same compiled program and one private context builder serve readiness, interview fields/messages/hooks, source paths/bodies, explicit file rules, top-level hooks, and before/after messages. No render caller inserts individual globals.

## Staged wire and replay

```rust
#[derive(Serialize, Deserialize)]
struct ContextWireV2 {
    target_path: PathBuf,
    selected: SelectedTemplateWire,
    host: HostFactsWire,
    execution: ExecutionFactsWire,
    environment: EnvironmentWire,
}
#[derive(Serialize, Deserialize)]
enum EnvironmentWire {
    Denied,
    Admitted { needs: u8, values: Vec<Option<String>> },
}
#[derive(Serialize, Deserialize)]
struct StagedRecordV2 {
    // Existing instant, formal identity, commit, named flag, ordered submissions.
    program_identity: ProgramIdentity,
    context: ContextWireV2,
}
```

The mask has only five valid bits in fixed enum order. `values` contains exactly one optional plaintext value per set bit; JSON `null` records a referenced but unavailable source. No absent/unreferenced slot is serialized. The admitted mask must equal the reloaded package's needs. A zero-needs package stores `Denied`, with no environment strings. Existing staged JSON storage, directory, and umask behavior stay as they are: these plaintext strings remain until successful staged apply, `abort`, or operator removal. The record contains no grant credential or reusable permission.

`Store::load` parses wire, loads the selected package, checks program identity, formal identity, target consistency, mask/cardinality, and wire structure before replay. It calls only `staging::canonical_target(&wire.target_path) -> Result<CanonicalTarget, StagingError>`, then compares `as_path()` against the stored target path and the record's existing target. It constructs live `InvocationContext` with that factory result; it never deserializes or manufactures `CanonicalTarget`. It uses the recorded instant and facts, live configured defaults, and ordered submissions. It does not inspect current environment, host, registry access, or current terminal mode. Hook review for execution remains independent and current.

A legacy record without a context snapshot fails `StagingError::LegacyContextMissing` before replay, with `abort` and restage guidance. This applies even when no exact new name appears: `debug()` and other indirect observations defeat exact-name scans, and historical host/interactive facts cannot be reconstructed. No placeholder context is invented.

## Module ownership and failure order

- `template` owns `AdmittedTemplate`, compilation, source-tree/include closure, AST needs, collisions, and program identity. It hides the multi-surface analysis and immutable render program behind `load` plus `environment_needs`.
- `context` owns domain facts, redaction, validation, and the one Jinja projection. It does not own target normalization or access decisions.
- `staging` owns `canonical_target`, versioned private wire, atomic storage, reconstruction, and replay checks.
- The CLI adapter owns selected registry identity, current trust evaluation, host/process capture, and the stage-specific refusal. `interview` and `plan` accept typed facts and the admitted package only.

Prerequisite target/config/selection/load failures precede admission. For stage, `StageEnvironmentTrustRequired` precedes capture, seed construction, interview progress, and writes. For continue/staged apply, malformed/legacy/program-changed wire fails before replay; valid replay uses the frozen snapshot. For direct/new apply, denied access is a valid context with five nulls. A plan target mismatch, if an existing target parameter remains for compatibility, is checked before render and returns `PlanError::ContextTarget`; the preferred `Plan::build` derives target from `Completed.context`.

`Seed.context` and `Plan::build` are source changes for direct crate callers. Existing templates without collisions keep their render behavior except that errors in source-tree Jinja now appear during load. No target factory, configured-default resolution, `HookSurface` digest, protocol output metadata, or existing `now()` contract changes. The live hook digest attests hooks and files executed by hooks, not every Jinja source; under direct option 1A an approved named template may read new non-hook Jinja content while its hook approval still matches.

## Falsifiable tests

1. A spy environment adapter reports zero gated reads for denied direct runs and for stage lacking `--trust`; the latter returns `StageEnvironmentTrustRequired` before seed/interview/store spies are called, even with a matching registry approval.
2. Every configuration surface, source path segment, source body, explicit body, and transitive literal include can reference each gated name before stage admission. A false branch, uncalled macro, self-shadowing assignment, alias, `debug()` alias, filter argument, and dynamic item key cause the correct need; a string literal property does not.
3. Change source bytes after load: the in-flight package renders the analyzed bytes. Change them before replay: `ProgramChanged` occurs before replay and any ambient read.
4. Stage one present, one absent, and one unreferenced gated value: wire holds two slots, one string and one JSON null, with the exact two-bit mask; rendered unreferenced and absent values are both `none`. A denied record contains no gated value bytes. Malformed masks/cardinality fail before replay.
5. Change environment, host, registry approval, and terminal mode between stage and continue: outputs match the recorded context. Staged apply `--trust` changes hook permission only. Direct/new apply grants via explicit flag or eligible matching live approval; changed digest, folder, direct Git, bundled, and local alias overlay do not grant.
6. Tamper target path or identity: `Store::load` rejects it using the producer factory and before replay. A legacy record always returns `LegacyContextMissing`; recovery is abort and restage.
7. All seventeen names have exact types on interview, readiness, and plan surfaces. Alias order, source, root/non-Unicode basename, Linux host fallbacks, admin, and originating interactivity persist across batches. Exact collisions report authored locations; a neighboring `toha_` name loads.
8. Terminal, headless, staged, and direct crate callers with equal typed facts produce equal answers and plans. `task ci` retains existing `now()`, protocol, presets, staging, plan, apply, and hook-review behavior.
