---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Toha-owned Jinja values — candidate 2

## Caller's usage

A template uses ordinary Jinja variables at every evaluation site:

```jinja
{% if toha_interactive and not toha_admin %}
Target: {{ toha_target_name }}
{% endif %}
{{ toha_template_name }} ({{ toha_template_formal_name }})
{% if toha_env_user is not none %}User: {{ toha_env_user }}{% endif %}
```

The same variable expressions work in interview prompts, defaults, conditions,
computed values, messages, and hooks, and in rendered paths, file contents,
explicit file rules, top-level hooks, and apply messages.

```console
# Interactive command: the driver captures facts before starting the interview.
toha apply sample ./output
# Headless command: toha_interactive is false even if launched from a terminal.
toha apply --answers answers.json sample ./output
# Later process: the stored snapshot is replayed; this process grants access again.
toha stage sample ./output --async answers.json
toha apply --trust ./output
```

```rust
let target = staging::canonical_target(output)?;
let context = InvocationContext::new(
    target,
    TemplateIdentity::new("sample".into(), vec![], None),
    HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
    RunFacts { admin: false, interactive: false },
    EnvAccess::Denied,
);
let seed = Seed { now, defaults, context };
let completed = run_to_completion(&template, Interview::start(&template, seed)?)?;
let plan = Plan::build(&template, &completed, output)?;
```

Here `run_to_completion` is caller pseudocode for submitting answers; it is not
a proposed Toha method. A crate caller supplies facts explicitly and receives an
ordinary `Completed` and `Plan`.

## Names and values

All names below are reserved top-level Jinja **variables**, never functions.
`null` means Jinja `none`; absent strings are never empty-string sentinels.
Values are immutable for one interview.

| Name | Jinja type | Value |
| --- | --- | --- |
| `toha_target_name` | string | Final component of the separator-free absolute path returned by `staging::canonical_target`; root has its platform root component as its name. This is a basename, not a path. |
| `toha_template_name` | string | `Template.name` from `template.yml`. |
| `toha_template_formal_name` | string | Selected `ResolvedTemplate.formal_name`, supplied by direct callers. |
| `toha_template_aliases` | array of strings | Every alias on the selected **named registry entry**, sorted lexically and deduplicated; `[]` for zero aliases, folder, direct Git, local, and bundled selections. Multiple aliases remain multiple elements; the spelling used to select the template does not receive special treatment. |
| `toha_template_source` | string or null | Registry `Entry.source` for a named entry; canonical source address from a parsed direct Git address; canonical folder path for folder selection; `null` for bundled selection or a direct caller without a source. This is the template address, never the source subdirectory. |
| `toha_host_os` | string | Host runtime OS family supplied by driver; `"unknown"` if unavailable. It is not `std::env::consts::OS` (the compile target). |
| `toha_host_arch` | string | Host runtime architecture supplied by driver; `"unknown"` if unavailable. It is not `std::env::consts::ARCH` (the compile target). |
| `toha_host_os_name` | string or null | Parsed Linux `/etc/os-release` `NAME`; `null` elsewhere or when missing, unreadable, malformed, or non-Unicode. |
| `toha_host_os_id` | string or null | Parsed Linux `/etc/os-release` `ID`, under the same absence rule. |
| `toha_host_os_id_like` | array of strings | Whitespace-separated Linux `ID_LIKE` tokens in source order; `[]` elsewhere or on absence/failure. |
| `toha_admin` | boolean | Explicit caller decision; binary detects effective administrative identity at capture. Unknown detection is `false`. |
| `toha_interactive` | boolean | Whether this invocation will offer terminal answers. |
| `toha_env_user` | string or null | `USER`, then `USERNAME` if the first is absent or non-Unicode. |
| `toha_env_hostname` | string or null | Hostname from the host adapter, not a dump of process environment. |
| `toha_env_editor` | string or null | Unicode `EDITOR`. |
| `toha_env_shell` | string or null | Unicode `SHELL`. |
| `toha_env_visual` | string or null | Unicode `VISUAL`. |

Each `toha_env_*` value is `null` when access is denied, even when a value was
supplied. Empty Unicode strings remain empty strings. A non-Unicode value is
absent; it is never converted with loss. An unreadable, absent, malformed, or
non-Unicode hostname is `null`. The binary's host adapter records only these
five allowed environment values. No generic lookup or environment map is
exposed.

For OS release, parse the standard key/value quoting and escaping once in the
host adapter. An unreadable file or invalid syntax invalidates the complete OS
release record; a valid file with a missing key gives that key its absence
value. `ID_LIKE` splits on ASCII whitespace after unquoting. The adapter does
not substitute a distribution guess. Host OS and architecture are runtime
observations; if the platform adapter cannot determine them, use `"unknown"`.
Direct callers can supply the same domain fields without process inspection.

## Data and signatures

```rust
// lib.rs exports context domain types.
// Fields shown public only where construction needs them.
pub struct TemplateIdentity {
    pub formal_name: String,
    pub aliases: Vec<String>,       // constructor sorts and deduplicates
    pub source: Option<String>,
}
pub struct HostFacts {
    pub os: String,
    pub arch: String,
    pub os_name: Option<String>,
    pub os_id: Option<String>,
    pub os_id_like: Vec<String>,
}
pub struct RunFacts { pub admin: bool, pub interactive: bool }
pub struct AllowedEnvironment {
    pub user: Option<String>,
    pub hostname: Option<String>,
    pub editor: Option<String>,
    pub shell: Option<String>,
    pub visual: Option<String>,
}
pub enum EnvAccess { Denied, Granted(AllowedEnvironment) }

// Owns the supplied canonical target; it does not normalize it.
pub struct InvocationContext {
    target: PathBuf,
    identity: TemplateIdentity,
    host: HostFacts,
    run: RunFacts,
    environment: EnvAccess,
}
impl InvocationContext {
    pub fn new(target: PathBuf, identity: TemplateIdentity,
               host: HostFacts, run: RunFacts, environment: EnvAccess) -> Self;
}

pub struct Seed {
    pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>,
    pub context: InvocationContext,
}
impl<'a> Interview<'a> {
    pub fn start(template: &'a Template, seed: Seed) -> Result<Self, EvalError>;
}
pub struct Completed {
    // Existing answers, messages, hooks and now remain.
    pub context: InvocationContext,
}
impl Plan {
    pub fn build(template: &Template, completed: &Completed,
                 target: &Path) -> Result<Self, PlanError>;
}
```

`InvocationContext::new` is a domain assembly method, not a filesystem or
environment reader. The binary calls `canonical_target` once and passes the
returned path to it. `Plan::build` keeps its existing target argument for file
operations but checks that its canonical target equals
`completed.context.target`; the driver passes the already canonical path, and
crate callers must do likewise. A mismatch is a typed
`PlanError::ContextTargetMismatch`, before rendering. The constructor checks the
target is absolute and has no `.` or `..` components; this is a domain invariant
check, not a second normalizer. `TemplateIdentity::new` accepts alias input and
normalizes only the alias collection. The actual short name comes from
`Template` at Jinja context construction, keeping one source of truth.

```text
main/CLI adapters ─ capture canonical_target result, selected identity, host,
                    run mode, and access decision ──────────────┐
crate caller ────── supplies those domain facts ────────────────┤
                                                                ▼
context.rs: InvocationContext / immutable snapshot / serialization
                         │
                         ▼
interview.rs: Seed → Pending → Completed ───────────────┐
                         │                              │
                         ▼                              ▼
             jinja.rs: one context builder ◄─────── plan.rs
                         │
                         ▼
             every expression and template evaluation

staging.rs: StagedRecord stores snapshot → replay constructs Seed
```

`jinja.rs` adds the exact reserved variables to the existing shared context
builder. It also exports one private `is_reserved(name)` predicate consumed by
template load validation and readiness checks. `template.rs` rejects reserved
names in `data`, question ids, computed ids, and `each` bindings, regardless of
source order. The error is one entry in the existing aggregate `LoadError {
problems }`, naming the duplicate reserved identifier and its field. Since the
loaded template cannot declare those names, merge order never decides their
value. `presets` and `{ preset: <name> }` remain config inputs for question
defaults and do not create any Jinja variable.

## Access and replay decision for Bob

The environment gate is a new permission and needs explicit approval before
implementation. Options:

1. **Recommended: grant by one-run `--trust` or a currently valid named-registry
   hook approval.** Before the first interview expression, the driver evaluates
   the existing executable-surface digest policy against the live template and
   combines the result with `--trust`. It passes `EnvAccess::Granted` only for
   that result. Folder, direct Git, local registry, and bundled selections
   require `--trust`. A direct crate caller must explicitly construct
   `EnvAccess::Granted`; no implicit process or registry lookup occurs. This
   aligns the gate with the existing hook trust meaning so that one invocation
   does not have conflicting trust decisions.
2. Require `--trust` for environment access even when named-registry hook
   approval is valid. This is a narrower new permission with an extra action for
   previously approved templates.
3. Add a separate `--allow-environment` grant. This separates hook and
   environment permissions but creates a second persistent policy surface and
   another decision for the same template evaluation.

Under option 1, capture allowed environment values only after the grant.
`StagedRecord` stores the full immutable `InvocationContext` snapshot, including
the five values when granted, plus its existing target, selected formal name,
commit, instant, and submissions. It also stores the access basis (`OneRunFlag`
or `NamedApproval`), not a reusable grant. On resume, the driver re-evaluates
access **before** replay and requires a current grant: a new `--trust` for
one-run access, or a still-valid named approval. If no grant exists, resume
refuses with a dedicated access error and does not evaluate or render the
template. If granted, replay uses the stored values, not the current process
values. The approval check uses the current live executable-surface digest, and
existing selected-template/commit checks remain. A staged record without context
is legacy: reconstruct the old seed only if the loaded template references no
new reserved name; otherwise fail with a migration diagnostic that asks the
caller to start a new interview. This avoids silently changing an interview
branch. The record contains plaintext environment values, so storage must use
the existing staged-record protection model; this is an explicit risk of this
option.

Interaction status is a statement of behavior, not terminal presence alone:
terminal `apply` that will ask has `true`; `--answers`, `stage --async`, and
direct nonprompting drivers have `false`; a staged terminal continuation has
`true` only if that continuation can actually ask. Because replay freezes
context, a staged interview's `toha_interactive` remains the value chosen for
its original execution mode. If a continuation changes mode, it must refuse
before replay and ask for a new interview, avoiding a branch change.
`toha_admin` is captured from effective credentials at the initial invocation
and frozen; resume does not redetect it. Direct callers state both booleans,
including when their host has no detector.

## Errors, results, and compatibility

Context capture failure for `canonical_target` remains `StagingError`; no
fallback path is rendered. Host observation failure produces documented absence
values, not an interview error. Invalid authored reserved names are
template-load problems. Jinja evaluation faults retain `EvalError` during
interview and `PlanError::Render` during planning. Access denial does not itself
fail an interview: all five gated variables are present as `none`. Resume of a
snapshot that held granted values without a current grant fails with
`StagingError::AccessRequired`; malformed or incomplete new snapshot data fails
with `StagingError::Replay`. `Plan::build` returns the existing `Plan` on
success; `apply` reports through its existing result. No environment value is
added to protocol output.

The existing `now()` function and existing authored identifiers keep their
behavior. The new reserved names are the only template compatibility break; load
errors make collisions explicit. Adding `Seed.context` is a deliberate crate
source break: every direct caller must supply the invocation facts, so old call
sites cannot silently produce different output. Keep the existing `Plan::build`
shape and add the target match check. The staged-record format gains a version
and snapshot; legacy records without use of new names replay under the old
semantics, while new-name use requires restaging. `toha_template_aliases` stays
an array for both zero and many; no scalar compatibility form is introduced.

## Canonical document proposals

After the access choice is approved, update
`docs/specifications/template-format.yml` to define the table above, reserved
identifiers, all evaluation sites, and absence/replay semantics. Update its
instance schema only if it validates reserved ids; do not encode runtime values
as template fields. Update `docs/specifications/interview-protocol.yml` for the
staged snapshot and access refusal outcome, and its schema for the versioned
record if that schema owns staged records. Update
`docs/specifications/command-line-interface.yml` for grant timing and resumed
`--trust` behavior. Update the Jinja guide with short examples and the variable
table. Keep registry trust semantics in its existing source of truth; link to it
rather than duplicate its algorithm.

## Falsifiable checks

- One fixture references every new name in interview `when`, prompt, default,
  computed, message, and hook fields, then in path, content, explicit file rule,
  top-level hook, and apply message. All surfaces see the same snapshot.
- A template with each reserved name separately in `data`, a question, computed
  node, or `each` binding fails load before an answer is requested; a config
  `preset` remains a default only.
- Resolve a named entry through either of two aliases: both runs produce the
  same formal name and sorted two-element alias array. A named entry with no
  aliases and a direct Git selection produce `[]`.
- Missing, unreadable, malformed, and partial OS release inputs give the stated
  null/empty results; an invalid Unicode environment value gives `none`, without
  lossy replacement. Empty Unicode values stay empty.
- Terminal asking, answers file, async stage, staged continuation, and direct
  crate caller exercise exact `interactive` and `admin` booleans.
- Denied access exposes five `none` values. A valid named approval and `--trust`
  expose only five captured values. Revoked approval or missing renewed
  `--trust` refuses resume before replay; a changed live environment with a
  renewed grant still renders the original snapshot.
- A plan built for a different canonical target fails before writing. A root
  target has a defined basename. Stage, replay, and direct execution render
  identical output for the same captured facts.

## Red-flag self-screen

The public seam adds one required seed value; callers do not coordinate
individual Jinja inserts, so the module is deep. The CLI owns operating-system
observations and trust evaluation; the pure context module owns only typed
snapshot and Jinja projection, keeping policy local. `context.rs` is organized
by the invocation facts it owns, not by load/validate/render time. The one Jinja
builder is shared by interview and plan, with no pass-through wrapper. A
maintainer traces capture → `Seed` → `Completed` or staged record → builder →
output in at most three modules after the driver.
