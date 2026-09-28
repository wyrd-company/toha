---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Invocation session owns the Jinja context

## Caller usage first

A command resolves and loads once, captures host facts once, and gives a session
the canonical target returned by `staging::canonical_target`. The session owns
the loaded template, immutable facts, interview state, and eventual plan. The
command handles terminal input and applies the returned plan.

```rust
// New terminal apply. `selected` is the selected domain identity, not the operand.
let target = staging::canonical_target(&path)?;
let template = Template::load(&resolved.folder)?;
let facts = InvocationFacts::new(
    target.clone(), selected, HostFacts::capture(),
    Execution { admin: detect_admin(), interactive: stdin_is_terminal },
    EnvAccess::Denied,
)?;
let mut run = Run::start(template, Seed { now, defaults }, facts)?;
while let RunStatus::Asking(batch) = run.status() {
    run.submit(read_terminal(batch)?)?;
}
let plan = run.plan()?;
plan.apply_reporting(&target, apply_options)?;
```

```rust
// Headless stage and resume. Driver stores run.snapshot() with the existing
// template identity/commit and submissions; it does not store a second context.
let mut run = Run::start(template, seed, facts)?;
run.submit(answers_document)?;
store.save(&run.snapshot()?)?;
// Later: load the recorded template revision and restore recorded facts.
let mut run = Run::replay(template, staged, defaults)?;
run.submit(next_document)?;
let plan = run.plan()?;
```

```rust
// Direct crate caller supplies domain facts; no process, registry, or CLI read.
let facts = InvocationFacts::new(
    canonical_target, selected, host,
    Execution { admin: false, interactive: false },
    EnvAccess::Denied,
)?;
let mut run = Run::start(template, seed, facts)?;
run.submit(raw_answers)?;
let plan = run.plan()?;
```

## Interface and ownership

```text
CLI adapter: resolution, canonical_target, host/env capture, access decision
             | domain values
             v
run::Run [Template, InvocationFacts, Interview state, submitted documents]
  | start / replay / status / submit / plan / snapshot
  | private context view: template.data + answers + immutable facts + now
  +--> interview::engine (pure progression and rendering)
  +--> plan::builder (pure rendering and conflict discovery; file reads)
             |
             v
          Plan::apply_reporting (existing effect seam)
```

`run` is the public seam. It owns the complete lifetime, so the caller cannot
pair a completed interview with another template or target. `interview` remains
the pure engine; internal evaluation takes an immutable context view supplied by
`run`. `plan` takes that same view through an internal builder. Neither module
reads process state. The context builder lives in `run::context` and is the sole
owner of Jinja insertion and name policy. `Run::plan` uses the owned template
and the exact target in the facts. It returns the existing `Plan`, and does not
apply it.

```rust
pub struct Run { // fields private
    template: Template,
    facts: InvocationFacts,
    interview: InterviewState,
    submissions: Vec<RawAnswers>,
}
pub enum RunStatus<'a> { Asking(&'a Batch), Complete(&'a Completed) }
pub struct InvocationFacts { // private, immutable after construction
    target: PathBuf,
    selected: SelectedTemplate,
    host: HostFacts,
    execution: Execution,
    environment: TrustedEnvironment,
}
pub struct SelectedTemplate {
    pub formal_name: String,
    pub aliases: Vec<String>,
    pub source: Option<String>,
}
pub struct HostFacts {
    pub os: String,
    pub architecture: String,
    pub distribution_name: Option<String>,
    pub distribution_id: Option<String>,
    pub distribution_id_like: Vec<String>,
}
pub struct Execution { pub admin: bool, pub interactive: bool }
pub enum EnvAccess { Denied, Granted(TrustedEnvironment) }
pub struct TrustedEnvironment { // five Option<String> fields, private
    user: Option<String>, hostname: Option<String>,
    editor: Option<String>, shell: Option<String>, visual: Option<String>,
}
impl Run {
    pub fn start(
        template: Template, seed: Seed, facts: InvocationFacts,
    ) -> Result<Self, RunError>;
    pub fn replay(
        template: Template, record: RunSnapshot,
        defaults: IndexMap<Id, RawAnswer>,
    ) -> Result<Self, RunError>;
    pub fn status(&self) -> RunStatus<'_>;
    pub fn submit(&mut self, answers: RawAnswers) -> Result<RunStatus<'_>, RunError>;
    pub fn plan(&self) -> Result<Plan, RunError>;
    pub fn snapshot(&self) -> Result<RunSnapshot, RunError>;
}
```

`InvocationFacts::new` requires an already canonical target and validates that
it has a final component; it never normalizes a path. The driver calls the
existing `staging::canonical_target` and maps its `StagingError`. A direct
caller has the same precondition. `SelectedTemplate` is built from the selected
formal name and registry entry at resolution time. Aliases are the entry's
complete alias set, sorted and deduplicated; folder, direct Git, local, and
bundled selections supply an empty list unless their selected entry actually has
aliases. `source` is the entry's stored source address when present, otherwise
`None`; it is never inferred from `formal_name` or a cache path. Template short
name always comes from `Template.name`. Empty formal name is rejected by
`InvocationFacts::new`.

`HostFacts::capture` belongs to the command adapter. It copies
`std::env::consts::OS` and `ARCH` as strings. On Linux it parses
`/etc/os-release` only for `NAME`, `ID`, and `ID_LIKE`: valid quoted or unquoted
UTF-8 assignments; last duplicate key wins; `ID_LIKE` splits on ASCII
whitespace. Missing, unreadable, non-UTF-8, or malformed file yields `None`,
`None`, and `[]` together. A missing individual key yields its empty form. On
other platforms all distribution fields are empty. No host metadata fault fails
a run. Direct callers supply these domain values; `InvocationFacts::new`
validates no strings contain NUL.

The command adapter derives `admin` from effective administrator status at
invocation capture: effective UID zero on Unix, an administrator token on
Windows, otherwise `false` if detection is unavailable. It is a status
observation, not an elevation request. `interactive` means this run can prompt
now: terminal apply uses its current stdin check, resumed apply uses its current
stdin plus stdout check, headless answers and `stage --async` use `false`.
Direct callers state both booleans. Replay uses the recorded booleans, even if
the resuming terminal differs; the driver still decides whether it can actually
solicit answers. This separates template-visible invocation status from driver
I/O ability.

The command captures each environment value with `var_os` and strict
`into_string`: absent or non-Unicode becomes `None`; no lossy conversion. `USER`
is preferred on Unix, `USERNAME` on Windows; no fallback to the other platform's
name. `HOSTNAME`, `EDITOR`, `SHELL`, and `VISUAL` use exactly those keys. Empty
strings remain empty strings. The command's hostname is the `HOSTNAME`
environment value, not a system API lookup. Only an explicit
`EnvAccess::Granted` accepts these captured values. `Denied` stores five `None`
values and discards the captured values before the session starts. No
`toha_env_` name reveals whether access was denied or the source was absent.

## Jinja contract

All values are variables, never functions. They are typed data, so reference
discovery and readiness use the same exact reserved-name set. `now()` remains
the existing function. Every expression and render receives the same immutable
facts plus accumulated answers: prompts, defaults, conditions, computed values,
messages, interview hooks, rendered paths and bodies, explicit file rules,
top-level hooks, and apply messages.

| Name | Jinja type | Value |
| --- | --- | --- |
| `toha_target_name` | string | Final component of canonical target, Unicode required; non-Unicode target is an invocation error. |
| `toha_template_name` | string | `Template.name` short name. |
| `toha_template_formal_name` | string | Selected formal name. |
| `toha_template_aliases` | array of strings | Sorted complete alias set; empty array if none. |
| `toha_template_source` | string or null | Stored source address, or null. |
| `toha_host_os` | string | Rust target OS name supplied by adapter. |
| `toha_host_arch` | string | Rust target architecture name supplied by adapter. |
| `toha_host_distribution_name` | string or null | Linux os-release `NAME`. |
| `toha_host_distribution_id` | string or null | Linux os-release `ID`. |
| `toha_host_distribution_id_like` | array of strings | Linux os-release `ID_LIKE` tokens. |
| `toha_is_admin` | boolean | Captured administrator status. |
| `toha_is_interactive` | boolean | Captured invocation prompt status. |
| `toha_env_user` | string or null | Trusted user value. |
| `toha_env_hostname` | string or null | Trusted `HOSTNAME`. |
| `toha_env_editor` | string or null | Trusted `EDITOR`. |
| `toha_env_shell` | string or null | Trusted `SHELL`. |
| `toha_env_visual` | string or null | Trusted `VISUAL`. |

Exactly these 17 names are reserved. Template load rejects them in top-level
`data`, question ids, computed ids, and `each` bindings, reporting every
collision through `LoadError.problems` with the authored location. The private
`__toha_now` remains reserved by its existing mechanism. There is no Jinja
`preset` or `presets`; configured defaults remain inputs to `Seed.defaults`
keyed by exact formal name.

## Access decision requiring approval

This changes access to process-derived values, so implementation requires Bob's
explicit approval. The access decision is independent of hook trust and is
supplied before the session starts. Two coherent policies are:

1. **Recommended: explicit per-run environment grant.** A command option grants
exactly the five named values for this run. Registry hook approval and `--trust`
do not imply this grant. Stage records the resulting five values and grant
state. This makes access visible at the command site and keeps template trust
from silently expanding data access.
2. **Tie environment grant to existing hook trust.** The driver grants values
when its final hook trust decision is true. This needs trust evaluation before
interview start and changes when an installed approval changes. It couples
unrelated permissions and makes staged replay depend on a later live decision.

The proposed interface can represent either policy, but the command adapter must
implement exactly the approved one. Until approval, no command path grants
environment values. This is the only unresolved behavior gate in this design.

## Staging, errors, and compatibility

`RunSnapshot` is a versioned serialized record containing the selected template
formal name and commit, canonical target, instant, all invocation facts
including the five effective environment values, and submissions. It is written
atomically by the existing store. Replay loads the recorded template revision,
checks its formal name and commit against the snapshot, restores facts and
instant, then replays submissions through the same pure engine. It does not
reread host or environment values and does not reevaluate access. Duplicate
submission after completion and mismatched template revision are replay errors.
Repeated `plan()` calls produce the same plan if source files are unchanged;
`submit` after completion is a state error. Existing staged records lacking
facts use a version-zero compatibility path: the driver supplies a single
captured legacy fact set at resume, freezes it before replay, and warns that
pre-stage branches had no such variables. A template using a new reserved name
cannot have been loaded before the feature, so old submissions cannot have
depended on it.

`RunError` wraps `EvalError`, answer rejections, `PlanError`, and typed
start/replay/snapshot errors without flattening their field and source
locations. `status` cannot fail. `submit` is transactional: rejection leaves the
prior state and submission log unchanged; successful submission appends once.
`plan` reports incomplete interview as `RunError::Incomplete`. Template
collisions stay `LoadError` problems. Target canonicalization stays
`StagingError` at the command seam. Missing host metadata and absent/non-Unicode
environment values are data, not errors.

The old `Interview::start(template, Seed)` and `Plan::build(template, completed,
target)` remain supported and continue to expose only data, answers, and
`now()`. New `toha_` names render null/empty defaults only through `Run`, so
direct callers that want invocation context migrate to `Run`. The compatibility
cost is two paths through internal interview and plan evaluation, plus
preserving the public `Completed` and `Seed` shapes. Both paths call the same
private engine and context builder; the legacy adapter supplies no invocation
facts. A future major release can retire the legacy public orchestration
surface. `Completed` does not carry `InvocationFacts`, and `Seed` is unchanged.

## Canonical documents to update after approval

Update template-format specification and schema to reserve the 17 names and
define types, null/empty behavior, and their availability at every render site.
Update the Jinja guide with table and examples. Update interview protocol only
if its staged record schema exposes the versioned snapshot; do not conflate
protocol `Context` with Jinja context. Update CLI documentation for the approved
environment grant and staged replay semantics. Keep these as clean current
contracts.

## Falsifiable validation and self-screen

- A fixture uses each value in a prompt, default, condition, computed value,
  message, interview hook, path, body, explicit rule, top-level hook, and apply
  message; every site receives the same captured value.
- A fixture stages after one answer, changes live host and environment, resumes,
  and gets the same next batch and plan as uninterrupted execution.
- Collision fixtures exercise all four authored-id positions and aggregate load
  problems.
- Missing, malformed, and non-UTF-8 os-release; absent, empty, and non-Unicode
  environment values; no, one, and multiple aliases yield the stated Jinja
  types.
- A direct crate test uses `Run` with supplied facts and no process reads;
  legacy direct calls retain their old result.
- Access tests prove denied values remain null even when hooks are trusted,
  under option 1.

The module hides identity assembly, context insertion, replay, interview
progression, and plan creation behind five operations. The extra internal call
from `run` to the pure engine is substantial policy adaptation, not a
pass-through. The only duplicated policy risk is the old direct path; it must
call the same context builder with empty facts. No transport type enters the
public run interface; `RunSnapshot` is a domain snapshot serialized privately by
staging. This is a domain-owned module, not a sequence of load/validate/render
modules.
