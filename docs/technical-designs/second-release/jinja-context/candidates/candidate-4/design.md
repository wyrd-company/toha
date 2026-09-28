---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Toha-owned Jinja context

## Caller usage

The command driver resolves the template and target once, captures host facts
once, and supplies a typed context before the first interview step. Examples use
generic values.

```rust
// Terminal `toha run sample ./output`: the driver already resolved `selected`.
let target = staging::canonical_target(path)?;
let context = InvocationContext::new(
    TargetIdentity::from_canonical(target.clone())?,
    TemplateIdentity::new(template.name.clone(), selected.formal_name.clone(),
        selected.aliases.clone(), selected.source.clone()),
    HostFacts::capture(),
    Execution::new(is_admin(), stdin.is_terminal()),
    EnvironmentAccess::Denied,
);
let interview = Interview::start(&template, Seed { now, defaults, context });
```

```rust
// `toha stage --async` and resume: persist the captured context with submissions.
record.context = Some(context.clone());
// On resume, decode the record and replay from its stored context and `now`.
let interview = replay(&template,
    Seed { now: record.now, defaults, context: record.context? },
    &record.submissions)?;
let plan = Plan::build(&template, &completed, &record.target)?;
```

```rust
// Direct crate caller: host and execution facts are explicit.
let trusted = TrustedEnvironment::new(user, hostname, editor, shell, visual);
let context = InvocationContext::new(target, identity, host, execution,
    EnvironmentAccess::Granted(trusted));
let seed = Seed { now, defaults, context };
let completed = finish(Interview::start(&template, seed))?;
let plan = Plan::build(&template, &completed, destination)?;
```

`Plan::build` uses the context retained by `Completed`; its destination remains
the write location and must equal the context's canonical target for command
drivers. Direct callers supply the same target to both. Planning rejects a
mismatch before rendering.

## Jinja contract

All new values are **variables**, not functions. They are stable for one
interview, allow ordinary Jinja conditionals, and have inspectable types.
`now()` remains a function. The exact reserved names are:

| Name | Jinja type | Value |
| --- | --- | --- |
| `toha_target_name` | string | Last component of the canonical target; non-Unicode is lossy UTF-8. |
| `toha_template_name` | string | `template.yml` short name. |
| `toha_template_formal_name` | string | Selected formal name, including bundled `toha-demo`. |
| `toha_template_aliases` | list of strings | All aliases of the selected installed entry, sorted and deduplicated; `[]` without a registry entry. |
| `toha_template_source` | string or null | Registry source address when one exists; null for folder, direct Git, local registry, or bundled selection. |
| `toha_host_os` | string | Lowercase compile target `std::env::consts::OS`. |
| `toha_host_arch` | string | Compile target `std::env::consts::ARCH`. |
| `toha_host_os_name` | string or null | Linux os-release `NAME`. |
| `toha_host_os_id` | string or null | Linux os-release `ID`. |
| `toha_host_os_id_like` | list of strings | Whitespace-separated Linux `ID_LIKE`; `[]` when absent. |
| `toha_is_admin` | boolean | Driver-supplied effective administrative status. |
| `toha_is_interactive` | boolean | Whether this invocation may prompt. |
| `toha_env_user` | string or null | Explicitly granted user value. |
| `toha_env_hostname` | string or null | Explicitly granted hostname value. |
| `toha_env_editor` | string or null | Explicitly granted `EDITOR`. |
| `toha_env_shell` | string or null | Explicitly granted `SHELL`. |
| `toha_env_visual` | string or null | Explicitly granted `VISUAL`. |

For denied access, every `toha_env_` value is null. Empty or non-Unicode input
is null, never lossy or an evaluation error. On Windows, `toha_env_user` reads
`USERNAME`; otherwise it reads `USER`. Hostname is an explicit host observation,
not a process environment dump; an unavailable or non-Unicode hostname is null.
`EDITOR`, `SHELL`, and `VISUAL` are independent; no fallback from one to
another. A malformed, unreadable, or absent `/etc/os-release` yields null `NAME`
and `ID` and empty `ID_LIKE`; an individually missing field gets its own
fallback. Parse only valid os-release assignments, quoted values, and escaped
characters; duplicate keys use the last valid assignment. No host file is read
outside Linux.

Interactive means the driver is permitted to request an answer in this
invocation: new terminal run requires stdin TTY, resumed terminal run requires
stdin and stdout TTY, headless answers and `stage --async` are false. A direct
caller states the boolean. Admin means the effective user has administrative
privilege as decided by the command host adapter (effective UID zero on Unix;
elevated token on Windows); unknown status is false. A direct caller states the
boolean. Neither fact grants environment access.

## Derived types and signatures

```rust
pub struct InvocationContext {
    target: TargetIdentity,
    template: TemplateIdentity,
    host: HostFacts,
    execution: Execution,
    environment: EnvironmentAccess,
}
pub struct TargetIdentity { canonical: PathBuf, name: String }
pub struct TemplateIdentity {
    short_name: String,
    formal_name: String,
    aliases: Vec<String>,
    source: Option<String>,
}
pub struct HostFacts {
    os: String, arch: String,
    os_name: Option<String>, os_id: Option<String>, os_id_like: Vec<String>,
}
pub struct Execution { is_admin: bool, is_interactive: bool }
pub enum EnvironmentAccess { Denied, Granted(TrustedEnvironment) }
pub struct TrustedEnvironment {
    user: Option<String>, hostname: Option<String>, editor: Option<String>,
    shell: Option<String>, visual: Option<String>,
}

impl TargetIdentity {
    /// Consumes the result of staging::canonical_target; does not normalize.
    pub fn from_canonical(path: PathBuf) -> Result<Self, ContextError>;
}
impl InvocationContext {
    pub fn new(target: TargetIdentity, template: TemplateIdentity,
        host: HostFacts, execution: Execution,
        environment: EnvironmentAccess) -> Self;
}
pub struct Seed { pub now: Zoned, pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext }
pub struct Completed { /* existing fields */, context: InvocationContext }
pub fn context_from_answers(template: &Template, answers: &Answers,
    now: &Zoned, invocation: &InvocationContext) -> JinjaContext;
pub fn is_available(name: &str, template: &Template, answers: &Answers) -> bool;
```

`TargetIdentity::from_canonical` rejects a root without a final component and an
empty rendered basename. It does not perform path resolution. The command passes
`staging::canonical_target(path)?` directly. The context constructor is
domain-only; the command's capture adapter reads process/host state. Direct
callers may construct facts from their own host adapter.

## Module and seam map

```text
CLI resolve + canonical_target + host/access adapters
                 │ typed InvocationContext
                 ▼
interview::Seed ──► Pending ──► Completed ──► plan::Plan::build
       │                 │                        │
       └─────────────────┴──► jinja::context_from_answers
                                  │
                             all Jinja renders
```

`interview` owns the durable invocation value; `jinja` owns the one projection
into Jinja names and the single reserved-name set. `template` asks that set when
loading authored identifiers. `staging` serializes the context with `now` and
submissions. The command adapter owns trust/access decisions and host
observation. No engine module reads registry, process environment, terminal
state, or host files.

## Access and replay

Access needs an explicit product decision:

1. **Recommended: separate grant aligned with executable trust.** The command
   grants environment access when the invocation has `--trust`, or when a
   selected named user/system entry passes the approved live
   `evaluate_trust(stored, live)` check. Folder, direct Git, local registry, and
   bundled selections require `--trust`. This reuses the known trust decision
   without treating identity itself as permission.
2. **Separate `--allow-env` grant.** This gives finer control but adds a new
   permission and another command mode to explain and persist.
3. **Only one-run `--trust`.** This is simple but ignores approved registry
   trust for environment access even when hooks may run.

The recommendation is pending Bob's explicit approval; no implementation may
treat it as approved. Evaluate access before the first interview render. For
staged work, evaluate it before staging, serialize the *projected five values*
or five nulls plus access state, and replay those exact values. Resume never
re-reads these environment values or re-evaluates a new grant for that record. A
denied staged record stays denied; changing access requires a new interview. The
selected identity, target, host, admin, interactive, and environment facts are
frozen in the staged context. `toha_is_interactive` for `stage --async` is false
on replay even if the resume driver uses a terminal. Replay may present
questions externally, but its template evaluation sees the original invocation
status. Existing records without a context must use a versioned legacy replay
path with the new names unavailable; if their template references a new name,
return a staged-record compatibility error and require a new stage. Never
silently recapture values.

## Collisions, results, and compatibility

Every exact name in the table is reserved at template load. Reject a matching
top-level `data` key, question id, computed id, or `each` binding as an
aggregated `LoadError` problem with its source location. The same reserved set
is available to load-time reference checks and runtime readiness. No `preset` or
`presets` variable is introduced; configured defaults continue to enter only as
question defaults keyed by formal name.

Context construction can return `ContextError` for invalid explicit identity or
target input. Host and environment absence never fail template evaluation.
Existing expression faults remain `EvalError`; plan render faults remain
`PlanError::Render`; staged decode/replay faults remain `StagingError::Replay`.
The proposed `Plan::build` mismatch is a dedicated `PlanError::ContextTarget`
before rendering. The Jinja projection inserts reserved values after data and
answers only because collisions are impossible. Existing `now()` and authored
names retain their behavior. Adding `Seed.context` changes the crate
constructor; use a named constructor or compile-guided migration for existing
direct callers, with no invented ambient default.

All interview prompts, defaults, conditions, computed values, messages, and
hooks use the same projection as rendered paths, contents, explicit file rules,
top-level hooks, and apply messages. A completed interview retains the projected
input even if the process environment later changes.

## Canonical document proposals

After approval, update the template-format specification and schema to define
the reserved names, types, null/list behavior, and collision errors; update the
Jinja guide with example access behavior; update the interview protocol for
staged snapshot semantics and legacy records; update the CLI contract for
`--trust` access behavior. The approved configured-defaults contract remains
separate. The selected formal-name and canonical-target owners remain
authoritative.

## Falsifiable validation

- One fixture uses every value in a prompt, default, condition, computed value,
  message, interview hook, rendered path/body, explicit file rule, top-level
  hook, and apply message; all observe the same snapshot.
- Alias selection by either of two aliases yields the same sorted two-item list
  and formal name; folder and bundled selection yield `[]` and null source.
- Reserved-name attempts in each of the four authored-id positions fail at load;
  neighboring names pass.
- Missing, malformed, and partial os-release inputs yield the stated per-field
  fallbacks; non-Linux never reads the file.
- Absent, empty, and non-Unicode environment and hostname inputs yield null;
  denied access exposes five nulls, including in hooks.
- Terminal, headless, async stage, resume, and direct caller tests assert admin
  and interactive values independently.
- Stage, alter host/environment/trust/terminal status, and resume: all rendered
  values and branch choices match the staged snapshot. A legacy record
  referencing a new name fails with the stated compatibility error.
- A plan target differing from the context target fails before any render or
  write; canonical target input is never normalized a second time.

## Red-flag screen

The external seam is one typed input on `Seed`; a caller does not coordinate
Jinja insertion, readiness, and planning. The Jinja projection hides namespace
and null policy behind one module. The host adapter is real because command and
direct callers provide distinct observations. No pass-through `ContextService`
or load/validate/render pipeline module is added. The only wide type is the
domain snapshot: its fields encode independently meaningful facts, not
implementation stages.
