---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

<!-- rumdl-disable MD013 -->

# Toha-owned Jinja context

## Caller usage first

Templates use ordinary variables in every Jinja field. For example, `{{ toha_template_name }}-{{ toha_target_name }}` can name a rendered file, `toha_interactive` can guard a question, and `toha_env_editor is none` can select a default. Every name below is present in every evaluation, including when its value is `null`.

```rust
// Terminal command driver, before Interview::start. The resolver supplies the
// selected formal name; the existing target normalizer supplies the target.
let target = staging::canonical_target(&path)?;
let selected = resolve_template(&operand, &scope)?;
let template = Template::load(&selected.folder)?;
let context = InvocationContext::capture(
    SelectedTemplate::from_resolution(&selected, registry_entry.as_ref(), &template),
    target.clone(),
    HostFacts::from_process(),
    Execution { admin: host_is_admin, interactive: stdin_is_terminal },
    EnvAccess::Denied,
)?;
let interview = Interview::start(&template, Seed { now, defaults, context })?;
```

```rust
// Headless stage. The command captures once and saves before accepting answers.
let context = InvocationContext::capture(
    selected, target.clone(), host, Execution { admin, interactive: false },
    EnvAccess::Granted,
)?;
let mut record = StagedRecord::new(target, selected_record, now, context);
let interview = Interview::start(&template, record.seed(defaults)?)?;
store.save(&record)?;
```

```rust
// Direct crate caller. No CLI resolver, registry, or process probe is required.
let context = InvocationContext::new(
    SelectedTemplate::new("example", "example", vec![], None)?,
    canonical_target, // caller supplies the canonical absolute target
    HostFacts::unknown(),
    Execution { admin: false, interactive: false },
    EnvValues::denied(),
)?;
let completed = drive(Interview::start(&template, Seed { now, defaults, context })?)?;
let plan = Plan::build(&template, &completed, &target)?;
```

`EnvAccess::Granted` above represents an explicit driver decision; the policy that permits it is an approval item below. A direct caller can supply a granted `EnvValues` only by explicitly choosing that enum variant. The library does not read the process, registry, terminal, or trust state.

## Data and signatures

```rust
pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvocationContext {
    selected: SelectedTemplate,
    target_name: Option<String>,
    host: HostFacts,
    execution: Execution,
    env: EnvValues,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedTemplate {
    short_name: String,
    formal_name: String,
    aliases: Vec<String>,
    source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostFacts {
    os: String,
    arch: String,
    os_name: Option<String>,
    os_id: Option<String>,
    os_id_like: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Execution { pub admin: bool, pub interactive: bool }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EnvValues {
    Denied,
    Granted { user: Option<String>, hostname: Option<String>,
              editor: Option<String>, shell: Option<String>, visual: Option<String> },
}

pub enum EnvAccess { Denied, Granted }

impl InvocationContext {
    pub fn new(selected: SelectedTemplate, canonical_target: PathBuf,
               host: HostFacts, execution: Execution, env: EnvValues)
               -> Result<Self, ContextError>;
    // CLI adapter; reads only the five allowed values when access is Granted.
    pub fn capture(selected: SelectedTemplate, canonical_target: PathBuf,
                   host: HostFacts, execution: Execution, access: EnvAccess)
                   -> Result<Self, ContextError>;
}

impl SelectedTemplate {
    pub fn new(short_name: impl Into<String>, formal_name: impl Into<String>,
               aliases: Vec<String>, source: Option<String>)
               -> Result<Self, ContextError>;
}

impl HostFacts {
    pub fn unknown() -> Self;
    // CLI adapter only; no host read happens in interview, Jinja, or plan.
    pub fn from_process() -> Self;
}

impl StagedRecord {
    pub fn new(target: PathBuf, selected: SelectedRecord,
               now: jiff::Zoned, context: InvocationContext) -> Self;
    pub fn seed(&self, defaults: IndexMap<Id, RawAnswer>)
                -> Result<Seed, StagingError>;
}

// Existing public operations retain their shape; the seed carries context.
pub fn Interview::start(template: &Template, seed: Seed) -> Result<Interview, EvalError>;
pub fn Plan::build(template: &Template, completed: &Completed,
                   target: &Path) -> Result<Plan, PlanError>;
```

`Pending` owns the seed. `Completed` owns `InvocationContext` and `now`, rather than a second set of derived Jinja values. The one private context builder projects `InvocationContext`, `now`, data, and answers into the evaluation map. It is called by interview rendering/readiness and plan rendering. The projection is not a public crate interface. All fields are private outside their owning module, with accessors for drivers that need to inspect them. `SelectedRecord` above is pseudocode for the existing staged identity fields, not a new public type.

## Exact Jinja contract

All names are variables, not functions. Variables make values visible to reference validation and readiness and keep `null`, array, string, and boolean types clear. `now()` remains the existing function. Empty strings and absent values are distinct: optional strings use JSON `null` when absent. `toha_template_aliases` and `toha_host_os_id_like` are always arrays, including `[]`.

| Name | Jinja type | Value |
| --- | --- | --- |
| `toha_target_name` | string or null | Final Unicode component of the canonical target; `null` for a filesystem root or non-Unicode basename. |
| `toha_template_name` | string | `Template.name`, the short name from `template.yml`. |
| `toha_template_formal_name` | string | Resolver's selected `formal_name`, or the direct caller's equivalent. |
| `toha_template_aliases` | array of strings | Sorted, deduplicated aliases of the selected registry entry; `[]` for no aliases or no registry entry. Multiple aliases remain multiple elements. The selecting spelling does not alter the array. |
| `toha_template_source` | string or null | Stored registry source address, when an entry exists; otherwise `null`. Never derived from the folder, Git URL operand, or formal name. |
| `toha_host_os` | string | Host OS family, lowercase Rust `std::env::consts::OS` vocabulary (`linux`, `windows`, etc.); direct callers supply the value. |
| `toha_host_arch` | string | Host architecture, lowercase Rust `std::env::consts::ARCH` vocabulary; direct callers supply the value. |
| `toha_host_os_name` | string or null | Linux `/etc/os-release` `NAME`, or `null`. |
| `toha_host_os_id` | string or null | Linux `/etc/os-release` `ID`, or `null`. |
| `toha_host_os_id_like` | array of strings | Whitespace-separated `ID_LIKE` identifiers in source order, or `[]`. |
| `toha_admin` | boolean | Driver-observed effective administrative identity at invocation capture. |
| `toha_interactive` | boolean | Whether this interview step may prompt a human; fixed for the interview. |
| `toha_env_user` | string or null | `USER`, else `USERNAME`, if granted. |
| `toha_env_hostname` | string or null | Hostname observed by the driver, if granted. |
| `toha_env_editor` | string or null | `EDITOR`, if granted. |
| `toha_env_shell` | string or null | `SHELL`, if granted. |
| `toha_env_visual` | string or null | `VISUAL`, if granted. |

For `toha_env_*`, denied access yields `null` for all five and causes no read of those sources. A granted read accepts only nonempty Unicode strings; absent, empty, and non-Unicode values become `null`, with no lossy conversion or fallback beyond `USER` to `USERNAME`. The `USER` fallback applies only when `USER` is absent, empty, or non-Unicode. The hostname adapter uses a native host name read, not a process-wide environment dump; failure or non-Unicode is `null`. This is a fixed five-value allowlist, not a general environment lookup.

`HostFacts::from_process` reads `std::env::consts::{OS, ARCH}` and, only on Linux, parses `/etc/os-release` using its quoted and unquoted assignment rules. Missing, unreadable, invalid UTF-8, or syntactically malformed file yields `null`, `null`, `[]` for the three release fields. A valid file missing one key defaults only that key. Empty `NAME` or `ID` yields `null`; empty `ID_LIKE` yields `[]`. Duplicate keys make the file malformed. Other platforms always use `null`, `null`, `[]`. A host fact read never fails template loading or an interview.

The CLI admin adapter uses the current effective identity: effective UID zero on Unix; Windows elevated-token result on Windows; `false` when the check is unavailable or fails. It does not infer admin from username. The CLI sets interactive `true` only when the command will actually invoke the terminal question driver with both stdin and stdout attached to terminals; headless answers, `stage --async`, and resumed answer submissions set `false`. A resumed terminal continuation uses its staged `toha_interactive` value; a stage created headlessly therefore remains `false`, even if a human later supplies answers at a terminal. Direct crate callers state both booleans explicitly. The contract describes the captured interview mode, not whether a terminal happens to exist at render time.

## Seam and data flow

```text
CLI resolver / direct caller       existing canonical_target result
              \                         /
               SelectedTemplate + canonical PathBuf
                            |
CLI-only host/access adapter -> InvocationContext <- direct caller values
                            |
               Seed -> Pending -> Completed
                |                    |
         one private Jinja projection <--- Plan::build
                |                    |
        prompts/defaults/conditions/computed/messages/hooks
                         rendered files and apply messages
                            |
            StagedRecord stores context + now + submissions
                  replay restores seed and replays answers
```

The external seam is the seed accepted by `Interview::start`; it exposes one invocation value, not individual renderer switches. The CLI adapter owns host reads and the access decision. The context module owns validation, names, collision list, and projection. Interview owns readiness and progression. Plan consumes the completed snapshot. Staging owns serialization and replay. The path normalizer remains `staging::canonical_target`; this design consumes its result and does not copy its algorithm. `Plan::build` may continue to receive the original target path for file placement. Callers must provide a path with the same canonical identity as the snapshot. The command already holds both paths and can enforce that precondition without another normalization pass.

## Permission options and recommendation

The environment grant is a separate permission from hook execution. This design does not assume hook approval implies permission to expose environment values in generated files.

1. **A — Explicit invocation grant (recommended).** The CLI grants the five values only when the caller explicitly requests it for the run, through a dedicated context access option. A staged grant and its captured values persist in that staged interview. Registry hook approval and `--trust` alone do not grant environment access. This keeps the capability visible and independent of hook review.
2. **B — Reuse `--trust`.** Treat the existing one-run `--trust` flag as the environment grant, and persist a staged grant through continuation. This uses fewer options but couples file disclosure to hook permission.
3. **C — Accept registry hook approval.** Permit values when the live executable surface matches stored approval or `--trust` is set. The driver must evaluate approval before `Interview::start`; this makes review of hooks grant a file-rendering permission as well.

Approval is required for the exact option and CLI spelling before implementation. The design proceeds with A as its proposed contract. The grant is recorded as data, so a resumed interview does not silently acquire access from a later hook approval or lose access because an approval changed. A staged record created with a grant contains the five captured values; access to that record must be treated as access to those values under the existing stage storage model. No general environment access is implied.

## Staging, compatibility, and errors

The staged record stores the complete `InvocationContext` beside its existing canonical target, selected formal name, commit, `named`, `now`, and submissions. It stores the grant outcome and captured values, not a recipe for re-reading them. Replay checks that context target and selected formal name equal the record's existing identity fields, restores the same seed, then replays submissions. The source and aliases are a capture-time snapshot; registry edits do not alter an in-progress interview. The host, admin, interactive, and environment values also remain fixed. Configured defaults remain distinct from context and follow the existing resume policy; `presets` and `{ preset: ... }` never enter Jinja.

Older staged records without `context` get a compatibility snapshot at first resume: selected identity from the record and loaded template, no aliases or source unless the same selected registry entry is available, canonical target from the record, host unknown, admin and interactive `false`, and environment denied. The upgraded record is saved before the next submission. This preserves replay of templates that do not reference the new names. A legacy record whose template references a new name may produce a different branch on first upgraded replay; report a staged compatibility error and require a fresh stage if prior submissions no longer fit. No silent replay repair is allowed.

`InvocationContext::new` requires a canonical absolute target supplied by the caller, rejects a relative path, empty selected short/formal name, or empty aliases, and sorts and deduplicates valid aliases. It does not resolve names or canonicalize paths. The command passes the existing `canonical_target` result; direct callers must uphold the canonical-path precondition. A filesystem root and non-Unicode basename remain valid targets with `null` target name. `ContextError` identifies the invalid field. CLI capture errors that originate in target canonicalization remain `StagingError`. Missing host/environment facts are normal values, not errors. JSON serialization errors while saving or loading remain `StagingError`; an inconsistent record is `StagingError::Replay` with the mismatched field. A Jinja type/render failure in the interview remains `EvalError` with node/field attribution; the same fault during planning remains `PlanError::Render` with source path/field attribution. A reserved authored identifier is aggregated in `LoadError.problems` with its template path. No partial plan or file write occurs on these failures.

The context module has one exact reserved-name set: all names in the table and the private `__toha_now`. Template load rejects any collision in top-level `data`, question ids, computed ids, and every `each` binding, including nested nodes and file rules. The reserved set is also registered as available during load-time reference validation and runtime readiness. Existing `now()` remains reserved by the Jinja environment. A template that does not use or define the new names retains its existing rendered values.

## Canonical document changes proposed

After approval, update `docs/specifications/template-format.yml` and its instance schema if needed with the table, null and array semantics, collision rule, and field coverage. Update the Jinja guide with examples and the five-value access rule. Update the command-line specification with the approved grant option, stage persistence, and interactive definition; update the interview protocol specification only if the staged wire format is documented there. Keep these edits in the canonical documents; this candidate is a design, not a second source of truth.

## Falsifiable validation

1. A fixture references every name from a prompt, default, condition, computed value, message, interview hook, path, body, file rule, top-level hook, and apply message; each field receives the same typed snapshot.
2. A fixture with two aliases receives both sorted aliases; folder, direct Git, local, and bundled selections receive `[]` and `null` source where no registry entry exists. A selected alias still reports the formal name.
3. Every reserved name in each authored-id surface fails template load with the exact location. An unreserved earlier answer remains usable and `now()` retains its seeded value.
4. Denied access does not read any of the five sources and renders all five as `null`; granted access handles absent, empty, and non-Unicode inputs as specified. `USER` fallback and hostname failure are covered.
5. Missing, unreadable, malformed, duplicate-key, and partial os-release fixtures produce the stated null/array fallbacks without failing an interview.
6. Terminal, headless, async stage, terminal continuation, and direct caller fixtures assert both booleans. A staged headless interview stays noninteractive after terminal continuation.
7. Mutate host, environment, registry aliases, source, and hook approval after staging; replay produces identical questions and rendered output from the snapshot. Tamper with duplicated staged identity and assert `StagingError::Replay`.
8. The existing end-to-end fixture suite and `task ci` pass; `Plan::build` still uses the completed snapshot even when the command passes its original target path for placement.

## Self-screen

- **Depth:** One seed field gives interview and plan the entire context; callers do not coordinate per-field globals or renderer setup.
- **Information leakage:** Registry, host, access, and stage wire details stay at their owning seams. Only domain values cross into the pure engine.
- **Temporal decomposition:** Context owns validation and projection together; there is no load/validate/render chain of public wrappers.
- **Pass-through methods:** `capture` earns its place by applying the five-value access policy; `new` validates direct caller values. Neither merely forwards another call.
- **Module depth:** From caller capture through output, maintainers trace the CLI adapter, context module, and interview or plan. Staging is visited only for replay.
