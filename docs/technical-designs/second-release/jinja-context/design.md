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
---

# Toha-owned Jinja context

Toha supplies one immutable, typed invocation context before the first
interview evaluation. The same value is carried by the pure interview engine,
stored for staged replay, and used by planning. The engine never reads the
process, registry, terminal, host files, or trust state.

This is a design artifact. Runtime, shared specification, schema, and guide
changes belong to the paired implementation after Phase C approval.

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

The command driver resolves the selected identity and canonical target once,
decides access, captures facts, and starts the existing engine:

```rust
let target = staging::canonical_target(path)?;
let resolved = resolve_template(operand, &config, &registry, &dirs, cwd)?;
let template = Template::load(&resolved.folder)?;

let Resolution { defaults, warnings } = configured_defaults(
    &resolved.formal_name,
    &template,
    &config.presets,
    &config.template_defaults,
)?;
report(warnings);

let access = effective_environment_access(
    trust_flag,
    resolved.approval.as_ref(),
    HookSurface::of(&template)?.digest(),
    trustable(&resolved, &registry),
);
let context = InvocationContext::capture(
    target.clone(),
    SelectedTemplate::from_resolved(&resolved),
    HostFacts::capture(),
    ExecutionFacts::new(is_admin(), terminal_driver_selected),
    access,
);
let interview = Interview::start(
    &template,
    Seed { now, defaults, context },
)?;
```

The same commands cover the supported modes:

```console
# A new terminal interview; the snapshot is interactive.
toha apply sample ./output

# A new headless interview; the snapshot is not interactive.
toha apply --answers answers.json sample ./output

# A trusted asynchronous stage; repeat --trust when replay needs stored values.
toha stage --trust sample ./output --async
toha continue --trust ./output answers.json
toha apply --trust ./output
```

A direct crate caller supplies equivalent domain facts and makes no process or
registry read through the engine:

```rust
let context = InvocationContext::new(
    canonical_target,
    SelectedTemplate::new("sample", vec![], None)?,
    HostFacts::new("linux", "x86_64", None, None, vec![]),
    ExecutionFacts::new(false, false),
    EnvironmentAccess::Denied,
)?;
let seed = Seed { now, defaults, context };
let completed = finish(Interview::start(&template, seed)?)?;
let plan = Plan::build(&template, &completed, completed.context().target())?;
```

The target supplied to `InvocationContext` is the separator-free canonical
absolute `PathBuf` returned by the existing
`staging::canonical_target(&Path) -> Result<PathBuf, StagingError>`. This design
does not add a normalizer or a stronger canonical-target type.

## Exact Jinja contract

All seventeen values are variables, not functions. Variables make their types
and absence visible to Jinja reference discovery and readiness checks. Every
name is present in the context map. `null` is Jinja `none`; an absent value is
never `undefined`. The existing `now()` function is unchanged.

| Name | Jinja type | Exact value |
| --- | --- | --- |
| `toha_target_name` | string or null | Final Unicode component of the canonical target; `null` for a filesystem root or non-Unicode component. |
| `toha_template_name` | string | Short name from the loaded `template.yml`; this value comes only from `Template.name`. |
| `toha_template_formal_name` | string | Stable selected `ResolvedTemplate.formal_name`, never the alias or short spelling used by the caller. |
| `toha_template_aliases` | array of strings | Effective aliases of the selected named registry entry, in registry order; `[]` for no aliases or a folder, direct Git, or bundled selection. |
| `toha_template_source` | string or null | `Entry.source` of the selected named registry entry; `null` when selection has no registry entry. |
| `toha_host_os` | string | Lowercase Rust target OS vocabulary from `std::env::consts::OS`. |
| `toha_host_arch` | string | Rust target architecture vocabulary from `std::env::consts::ARCH`. |
| `toha_host_os_name` | string or null | Linux os-release `NAME`; `null` elsewhere or when unavailable. |
| `toha_host_os_id` | string or null | Linux os-release `ID`; `null` elsewhere or when unavailable. |
| `toha_host_os_id_like` | array of strings | Linux os-release `ID_LIKE`, unquoted and split on ASCII whitespace in source order; `[]` elsewhere or when unavailable. |
| `toha_is_admin` | boolean | Effective administrative status captured by the caller. |
| `toha_is_interactive` | boolean | Whether the originating interview driver was allowed to prompt a human. |
| `toha_env_user` | string or null | Trusted user value. |
| `toha_env_hostname` | string or null | Trusted native hostname observation. |
| `toha_env_editor` | string or null | Trusted `EDITOR`. |
| `toha_env_shell` | string or null | Trusted `SHELL`. |
| `toha_env_visual` | string or null | Trusted `VISUAL`. |

### Identity and source rules

`SelectedTemplate` is assembled at the resolution boundary. A name, alias, or
short-name resolution carries the selected registry entry's full effective
alias list and `Entry.source`. A folder, direct Git address, or bundled demo
carries `[]` and `null`; its stable identity remains available through
`toha_template_formal_name`. The bundled identity is `toha-demo`.

The resolver adds `aliases: Vec<String>` and `source: Option<String>` to its
existing result while preserving the final configured-default contract's
`formal_name` field. Config `presets` and `{ preset: <name> }` remain config-side
inputs to the flat `Seed.defaults` map. No Jinja value is named `preset` or
`presets`.

### Host fallback rules

The command host adapter reads `/etc/os-release` only on Linux. It accepts
standard quoted and unquoted assignments, unescapes values without executing
the file, and uses the last valid assignment for a duplicate key. A missing key
gets its own fallback. An absent, unreadable, non-UTF-8, or syntactically
malformed file makes all three os-release values unavailable: `null`, `null`,
and `[]`. Empty `NAME` or `ID` is `null`; empty `ID_LIKE` is `[]`. Host metadata
absence never fails template load, interview, or planning.

Admin is `true` only when a platform adapter positively observes effective
administrative authority: effective UID zero on Unix or an elevated token on
Windows. Failure or unsupported detection is `false`; no username inference or
elevation occurs. Host capture uses platform APIs or an in-process library and
must not spawn a command.

### Environment fallback rules

The command reads the five gated sources only after access is granted. User is
`USER` on Unix and `USERNAME` on Windows. Hostname is a native hostname
observation, not a process-environment dump. `EDITOR`, `SHELL`, and `VISUAL`
are independent; none falls back to another. Absent, empty, or non-Unicode
input becomes `null`, with no lossy conversion. Denied access performs no gated
reads and projects five `null` values. A template cannot distinguish denial
from an unavailable value.

No arbitrary environment lookup, environment map, dynamic variable name, or
alias exposes another process value.

## Data structures and signatures

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvocationContext {
    target: PathBuf,
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
    environment: EnvironmentAccess,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedTemplate {
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
pub struct ExecutionFacts {
    is_admin: bool,
    is_interactive: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum EnvironmentAccess {
    Denied,
    Granted(TrustedEnvironment),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TrustedEnvironment {
    user: Option<String>,
    hostname: Option<String>,
    editor: Option<String>,
    shell: Option<String>,
    visual: Option<String>,
}

pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}

pub struct Completed {
    // Existing fields remain.
    context: InvocationContext,
}

impl InvocationContext {
    pub fn new(
        canonical_target: PathBuf,
        selected: SelectedTemplate,
        host: HostFacts,
        execution: ExecutionFacts,
        environment: EnvironmentAccess,
    ) -> Result<Self, ContextError>;

    pub fn target(&self) -> &Path;
}

impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        canonical_target: &Path,
    ) -> Result<Self, PlanError>;
}
```

`SelectedTemplate::new` rejects an empty formal name. `InvocationContext::new`
requires an absolute target without `.` or `..` components but does not resolve,
canonicalize, or otherwise rewrite it. The command passes the exact existing
normalizer result. `Plan::build` compares its target byte-for-byte with the
completed context before any render and returns
`PlanError::ContextTarget { expected, actual }` on mismatch. It never invokes a
normalizer. Command call sites pass the same canonical value to planning and
apply instead of the original CLI spelling.

`EnvironmentAccess` has a redacted `Debug` implementation. Gated values must
not enter diagnostics, protocol output, logs, or task evidence.

## Module and seam

```text
registry/config + resolver ───► SelectedTemplate ─┐
canonical_target result ──────────────────────────┤
CLI host adapter ─────────────► HostFacts         ├─► InvocationContext
terminal driver ──────────────► ExecutionFacts    │          │
effective trust decision ─────► EnvironmentAccess ┘          │
                                                              ▼
config presets ─► Resolution { defaults, warnings } ─► Seed ─► Pending
                                                              │
                    StagedRecord ◄──── serialize/replay ──────┤
                                                              ▼
                                                          Completed
                                                              │
template data + answers + now + InvocationContext ─► one Jinja projection
                                           │                  │
                                           └──── interview ───┴──► Plan
```

The new `context` module owns domain types, the exact reserved-name set, typed
projection, and redaction. The CLI adapter owns host/process capture and the
access decision. `interview` owns progression and carries the immutable value.
`staging` owns serialization and replay. `plan` consumes only the completed
snapshot. Protocol `Context { target, template, commit }` stays output metadata
and does not become the Jinja context.

One private context builder combines template data, accumulated answers,
`__toha_now`, and the typed invocation projection. Interview rendering,
readiness, and planning all use that builder. No caller inserts individual
Jinja values.

## Permissions and access decision

Bob must approve one of these exact policies before implementation:

1. **A — effective template trust grants access (recommended).** The five values
   are granted when the invocation supplies `--trust`, or when a selected named
   user/system registry entry has a stored review digest that matches the live
   `HookSurface` through the approved `evaluate_trust` function. Folder, direct
   Git, local-registry, and bundled selections require `--trust`. This gives one
   explicit meaning to trust for one template invocation and uses the approved
   content-bound review rather than identity alone.
2. **B — `--trust` only.** Registry approval continues to authorize hooks but
   does not authorize the five values. Every template, including a reviewed
   installed template, needs `--trust` for environment access. This separates
   the permissions but asks callers to repeat trust after persistent review.

Both policies use the existing `--trust` spelling. `stage` and `continue` gain
that option because they evaluate Jinja before `apply`; `apply` keeps its
existing option. No separate environment permission is introduced.

The recommended approval includes these access semantics:

- capture happens before `Interview::start` and only after the live decision;
- only the five named optional strings cross the gate;
- a granted staged snapshot stores those five values in the existing staged
  record in the user's state directory;
- every later process must have current effective access before replaying a
  snapshot that contains granted values; a repeated `--trust` satisfies a
  prior one-run grant;
- a saved denial stays denied even if a later command is trusted; changing the
  visible values requires a new interview;
- revocation refuses replay before any template evaluation and prints no stored
  value.

This is a permissions/access expansion. Under option A, approved registry trust
would authorize five additional values. Both options authorize their plaintext
persistence in staged state when granted. No staged location or file-permission
policy changes in this design.

## Interactive, headless, staged, and direct semantics

The snapshot describes the originating interview and is immutable:

| Originating path | `toha_is_interactive` |
| --- | --- |
| `stage` with terminal prompting | `true` |
| `stage --async` | `false` |
| new `apply` with terminal prompting | `true` |
| new `apply --answers` | `false` |
| direct crate caller | caller-supplied boolean |

Resume replays the stored value. `continue` and staged `apply` may still use any
documented input modality; no mode is refused. A terminal continuation of an
asynchronous stage therefore remains template-visible as noninteractive. This
prevents a modality change from changing earlier branches during replay. Admin,
host, selected identity, target, and environment values are frozen by the same
rule.

## Staging and replay

`StagedRecord` gains a version and the complete `InvocationContext`. It retains
its existing target, formal name, commit, named flag, instant, and submissions.
Before replay:

1. load the exact selected template revision;
2. check the stored context target and formal name against the record;
3. compute current access without reading the gated values;
4. refuse with `StagingError::AccessRequired` when the snapshot is granted but
   current access is denied;
5. reconstruct `Seed` with live configured defaults, the recorded instant, and
   the recorded context; then replay submissions through the ordinary engine.

The configured-default design intentionally re-resolves live config on resume;
that flat `Seed.defaults` behavior remains independent from the frozen Jinja
context. The record is written atomically through the existing store.

For a legacy staged record without context, scan every Jinja-bearing template
field for any of the seventeen names before replay. If one is referenced, fail
with an incompatible-record error and name `abort` then `stage` to start over.
Otherwise synthesize an unobservable compatibility context from the record's
target/formal name, loaded template, empty aliases, null source, host fallbacks,
`false` execution flags, and denied environment access; save the upgraded
record before accepting another submission. A collision still fails template
load. No historical trusted value is invented.

## Collision and availability contract

Only the seventeen exact public names are reserved. `Template::load` rejects a
matching top-level `data` key, question id, computed id, or `each` binding,
including bindings in nested interview nodes, file rules, and hooks. Each
problem is aggregated in `LoadError.problems` and names the authored location.
Nested object properties are ordinary keys. Other `toha_` names are not
reserved by this feature.

The same exact set is registered as available for load-time reference checking
and runtime readiness. Every value is therefore available in:

- prompts, descriptions, placeholders, defaults, conditions, computed values,
  messages, and interview hooks;
- rendered source paths, file contents, explicit file-rule paths/conditions/
  iteration, top-level hooks, and before/after apply messages;
- terminal, headless, staged/resumed, and direct crate execution.

Map insertion order is not conflict policy. A template with a reserved authored
identifier never loads.

## Errors and compatibility

| Situation | Result |
| --- | --- |
| Reserved authored identifier | Existing aggregate `LoadError` with authored location. |
| Invalid direct selected identity or target precondition | `ContextError` naming the field; no process data. |
| Plan target differs from completed context | `PlanError::ContextTarget` before rendering or I/O. |
| Missing/malformed host metadata | Typed `null`/`[]` fallback; not an error. |
| Denied or absent gated value | Jinja `none`; not an error. |
| Granted staged value lacks current authorization | `StagingError::AccessRequired` before replay. |
| Malformed or inconsistent staged context | `StagingError::Replay` with the mismatched field. |
| Legacy record references a new name | Compatibility error with restage guidance. |
| Interview expression/render failure | Existing attributed `EvalError`. |
| Plan render failure | Existing attributed `PlanError::Render`. |

Adding `Seed.context` is a deliberate source break for 0.2.0 crate callers so
that no supported caller silently receives different values. The public
interview state machine and plan/apply result types otherwise remain. Existing
templates that do not define a reserved name retain their behavior. Existing
`now()` behavior remains.

## Canonical document changes for implementation

The paired implementation updates clean current contracts:

- `docs/specifications/template-format.yml` and its schema: exact variable
  table, types, absence, collision locations, and evaluation surfaces;
- `docs/template-jinja.md`: concise examples, access behavior, and staged
  snapshot semantics;
- `docs/specifications/command-line-interface.yml`: approved access policy,
  `--trust` on `stage`/`continue`, reauthorization refusal, and guidance;
- `docs/specifications/interview-protocol.yml`: versioned staged context,
  compatibility behavior, and unchanged per-call input modality;
- `docs/specifications/template-registry.yml` only by reference to the approved
  live review decision; the digest algorithm is not duplicated.

## Behaviors to prove

1. **Every surface:** one fixture references every variable from every
   interview and plan surface; all see the same snapshot.
2. **Identity:** an alias and formal-name selection yield the same formal name,
   registry-order alias array, and source; zero and multiple aliases preserve
   exact array types. Folder, direct Git, and bundled selections yield `[]` and
   `null`; bundled formal name is `toha-demo`.
3. **Target:** nonexistent and existing directories use the basename from the
   single canonical result; an existing directory has no trailing separator;
   root and non-Unicode basenames yield `null`; a mismatched plan target fails
   before rendering.
4. **Host:** Linux complete, partial, absent, unreadable, non-UTF-8, malformed,
   empty, and duplicate-key os-release fixtures produce the stated values;
   non-Linux reads no os-release file.
5. **Execution:** new terminal stage/apply, asynchronous stage, answers-file
   apply, resumed terminal/document calls, and direct callers assert exact
   admin and originating-interactive booleans without refusing a modality
   change.
6. **Denied access:** a spy adapter proves the five gated sources are not read;
   all five variables are `null`, including for alias-selected templates.
7. **Granted access:** option A tests `--trust`, matching named approval,
   changed digest, local alias/layer, folder, direct Git, and bundled selection;
   option B omits the approval grant test. Only five values cross the seam.
8. **Replay authorization:** change the live environment after staging and get
   the recorded output; remove current approval and observe pre-replay refusal;
   repeat `--trust` and resume; later trust never elevates a saved denial.
9. **Redaction:** no granted value appears in `Debug`, errors, protocol output,
   or command guidance. The staged record contains only the approved five.
10. **Collisions:** each exact name fails independently in data, question,
    computed, and `each` positions with location; a neighboring `toha_` name
    loads; config `presets` remain defaults only.
11. **Legacy records:** a record whose template has no new-name reference
    resumes and upgrades; a referenced name refuses before replay.
12. **Caller parity:** command terminal, headless, staged, and direct crate
    callers produce the same answers and plan for the same typed inputs; the
    engine performs no ambient read.
13. **Predecessors:** configured-default `Resolution { defaults, warnings }`
    remains exact and flat; hook trust uses the approved live digest function;
    canonical target has one owner.
14. **Regression:** `task ci` passes with existing `now()`, protocol, presets,
    staging, plan, apply, and hook-review fixtures.

## Out of scope

- Runtime code or shared canonical-document changes in this design task.
- Arbitrary environment access, dynamic environment functions, or secrets.
- New registry trust commands or a changed review digest.
- A new target normalizer or dependency on the error-attribution
  implementation.
- Configured-default behavior, provenance, or naming changes.
- A `Run` session façade or general public-engine redesign.
- Encryption or a new file-permission policy for staged records.
- Timeout mechanics, pinned-version checks, or application subprocesses.

## Implementation start after approval

Add the context domain types and the single Jinja projection first, then thread
the snapshot through `Seed`, completion, staging, and planning before adding
command capture and the approved access policy. Any need for a different
context lifetime or trust gate returns to this design checkpoint.
