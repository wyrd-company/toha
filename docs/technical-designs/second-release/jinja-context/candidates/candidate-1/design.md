---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Invocation context for Jinja

## Caller usage

The command driver captures one invocation before starting the interview. The
same value reaches the planner without another process read.

```rust
// `run sample ./output` from a terminal.
let target = staging::canonical_target(path)?;
let selected = resolve_template(operand, &config, &registry, &dirs, cwd)?;
let template = Template::load(&selected.folder)?;
let access = environment_access(&selected, &registry, &template, trust_flag)?;
let context = InvocationContext::capture(
    TargetName::from_canonical(&target),
    TemplateIdentity::selected(&template, &selected, &registry),
    HostFacts::capture(),
    Execution { admin: detect_admin(), interactive: stdin_is_terminal },
    access.capture_five_environment_values(),
);
let interview = Interview::start(&template, Seed { now, defaults, context });
let completed = drive(interview)?;
let plan = Plan::build(&template, &completed, &target)?;
```

```rust
// `stage sample ./output --async --answers answers.json` and later resume.
let context = capture_context_with_interactive(false, /* same inputs as run */)?;
let record = StagedRecord::new(target, selected, now, context, submissions);
store.save(&record)?;
// Resume checks the saved grant against current approval before replay.
let context = record.checked_context(current_environment_access)?;
let completed = record.replay(&template, defaults, context)?;
let plan = Plan::build(&template, &completed, &record.target)?;
```

```rust
// A direct crate caller supplies facts from its own host and access policy.
let context = InvocationContext::new(
    TargetName::from_canonical(&canonical_target)?,
    TemplateIdentity::new("sample", "sample", vec![], None)?,
    HostFacts::new("linux", "x86_64", None, None, None),
    Execution { admin: false, interactive: false },
    EnvironmentValues::denied(),
);
let seed = Seed { now, defaults, context };
let completed = finish(Interview::start(&template, seed))?;
let plan = Plan::build(&template, &completed, &canonical_target)?;
```

Here `sample`, `./output`, and `answers.json` are generic examples. The
illustrative driver helpers are command-side adapters; the public crate seam
is `InvocationContext`, `Seed`, `Interview::start`, and `Plan::build`.

## Names and Jinja types

All names are **variables**, not functions. Values are fixed for one invocation;
`now()` remains the existing function. Every listed name is always present in
the map. An absent string is Jinja `none`, never `undefined` or an empty string.

| Reserved name | Jinja type | Meaning |
| --- | --- | --- |
| `toha_target_name` | string or none | Final UTF-8 component of the canonical target. Root or non-Unicode component gives `none`. |
| `toha_template_name` | string | `template.yml` short name. |
| `toha_template_formal_name` | string | Selected formal name, never the input alias. |
| `toha_template_aliases` | array of strings | All aliases of the selected registry entry, sorted and deduplicated; `[]` without an entry. |
| `toha_template_source` | string or none | Stored registry source address, or `none` without an entry. No path or address is inferred. |
| `toha_host_os` | string | Host OS identifier supplied by adapter. |
| `toha_host_arch` | string | Host architecture identifier supplied by adapter. |
| `toha_host_os_name` | string or none | Linux os-release `NAME`. |
| `toha_host_os_id` | string or none | Linux os-release `ID`. |
| `toha_host_os_id_like` | string or none | Linux os-release `ID_LIKE`, kept as one string. |
| `toha_admin` | boolean | Driver-supplied administrator status. |
| `toha_interactive` | boolean | Whether this invocation can ask live questions. |
| `toha_env_user` | string or none | `USER`, then `USERNAME`. |
| `toha_env_hostname` | string or none | Hostname observation from the driver. |
| `toha_env_editor` | string or none | `EDITOR`. |
| `toha_env_shell` | string or none | `SHELL`. |
| `toha_env_visual` | string or none | `VISUAL`. |

The first five environment values are exposed only after an explicit grant.
Without a grant, each is `none`; templates cannot distinguish a denied value
from an absent value. `USER` takes precedence only if it is valid, nonempty
Unicode; otherwise `USERNAME` is tried. For every other optional string,
absent, empty, or non-Unicode input yields `none`. Hostname lookup failure and
non-Unicode hostname yield `none`. No lossy conversion or environment-wide map
is allowed. The OS and architecture adapter uses the host platform, not a
compile target when cross compilation can differ. If the adapter cannot report
either required value, context capture fails before interview with a named
input error. On non-Linux hosts all three os-release fields are `none`. On
Linux, absent or unreadable file, malformed document, invalid UTF-8, invalid
field encoding, or missing individual keys yield `none` for affected fields;
valid independent fields survive malformed neighbors. Parse quoting and escapes
as specified by os-release; do not execute the file. Admin is `true` only
when the driver's platform authority check positively reports administrator;
unknown is `false`. Terminal run/resume is interactive only when the actual
driver will prompt under its existing terminal checks; headless answers and
async staging are `false`. A direct caller states both booleans explicitly.

## Derived types and interfaces

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvocationContext {
    target: TargetName,
    template: TemplateIdentity,
    host: HostFacts,
    execution: Execution,
    environment: EnvironmentValues,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetName(Option<String>); // Built only from canonical_target's result.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateIdentity {
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
    os_id_like: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Execution { pub admin: bool, pub interactive: bool }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EnvironmentValues {
    Denied,
    Granted {
        user: Option<String>, hostname: Option<String>,
        editor: Option<String>, shell: Option<String>, visual: Option<String>,
    },
}

impl InvocationContext {
    pub fn new(target: TargetName, template: TemplateIdentity,
        host: HostFacts, execution: Execution,
        environment: EnvironmentValues) -> Result<Self, ContextInputError>;
    pub(crate) fn jinja_values(&self) -> ContextValues; // exact table above
}

pub struct Seed { pub now: jiff::Zoned,
    pub defaults: IndexMap<Id, RawAnswer>, pub context: InvocationContext }
pub struct Completed { /* existing result fields */ context: InvocationContext }
pub fn Interview::start(template: &Template, seed: Seed) -> Interview<'_>;
pub fn Plan::build(template: &Template, completed: &Completed,
    target: &Path) -> Result<Plan, PlanError>;
```

Constructors validate nonempty required strings, canonical alias order and
duplicates, and optional strings' Unicode at their input seam. Production
adapters use `TargetName::from_canonical(&Path)` after the existing
`staging::canonical_target`; no target normalizer or identity resolver is added
to this module. `ContextValues` is private serialization, never a wire or
protocol type. `Seed` owns the snapshot; transition to `Completed` moves it.
The one context builder combines template data, answers, computed values, the
seeded `now()` backing value, and `context.jinja_values()` for both interview
and plan. Readiness and load reference checks recognize the same reserved
names. The planner still receives `target` for filesystem work, but never
rederives `toha_target_name` from it.

```text
CLI resolve + registry ─┐
canonical_target ───────┤
host/access adapters ───┴─> InvocationContext ─> Seed ─> Interview
                                             │            │
                                             │            └─> Completed ─> Plan
                                             └─> StagedRecord ─> replay Seed
template data + answers + InvocationContext ─> one Jinja map builder
```

The public seam is a single domain snapshot. Command adapters own process and
registry knowledge. `interview`, `jinja`, and `plan` own evaluation knowledge.
`staging` owns persistence and replay. Protocol output `Context` remains output
metadata and does not become this type. Config `presets` remain config-side
question defaults; there is no `preset` or `presets` Jinja name.

## Access decision and replay

The requested access gate needs Bob's explicit approval before implementation.
Options are:

1. **Recommended:** grant the five values when either `--trust` is given or a
   selected named user/system registry entry passes the approved live
   executable-surface digest review. Decide before starting the interview,
   using the existing `evaluate_trust` policy; folder, direct Git, local, and
   bundled selections can only use `--trust`. This matches hook trust while
   retaining an independently passed environment grant.
2. Grant only with per-invocation `--trust`, even for approved named entries.
   This makes access narrower but gives installed approved templates different
   capabilities during one invocation.
3. Require a separate environment-specific grant. This gives finer control but
   adds a new permission, command surface, and staged decision.

The driver derives `EnvironmentValues` from the chosen decision; the pure engine
never infers it from `formal_name` or hook status. The saved staged record
contains the complete context snapshot, a format version, and the existing
`now`, identity, target, and submissions. Save occurs before any asynchronous
reply. Replay uses saved host, admin, interactive, and environment values; it
does not recapture them. `toha_interactive` describes the originating interview
and stays `false` for an async stage, including resumed terminal completion.
Before replay, the driver checks that a saved `Granted` access still passes
the current decision, and returns a staging access error if revoked. A saved
`Denied` remains denied even if access is later granted. This preserves branch
decisions and prevents a stale approval from exposing a saved value. The
checked context is used for every replayed submission and subsequent planning.
Save writes atomically through the existing store so repeating the same stage
cannot create two divergent snapshots. A resume retries from the saved snapshot
and submissions, with no partial context mutation.

## Collisions, errors, and results

Every `toha_` prefix name is reserved for future Toha context growth.
`Template::load` rejects any top-level `data` key, question id, computed id,
or `each` binding that starts with `toha_`, even if the exact name is not in
the table. It aggregates each occurrence into existing `LoadError.problems`
with the authored location. Names in nested objects are ordinary object keys.
The private `__toha_now` stays private. A missing reserved value has the typed
`none` result above. Malformed host optional input is not a render error.
Invalid required context input returns `ContextInputError` at capture. A
revoked staged grant or incompatible record returns `StagingError` before
replay. Jinja expression faults retain `EvalError`; plan rendering faults
retain `PlanError::Render` with source path. Successful interview and plan
results retain their current shapes apart from the owned context.

## Compatibility and canonical documents

Add `context` to `Seed` as a required field and carry it privately on
`Completed`; this is a source-breaking crate change for 0.2.0. Provide a
`InvocationContext::minimal` constructor for migration only if callers cannot
provide selected identity; it must require explicit target, short name, formal
name, admin, and interactive, and deny environment access. Older staged records
without a context must fail with a clear incompatible-record error before
replay because recapturing host and access would not preserve their prior
interview path. The command can instruct the caller to start a new stage. Do
not silently synthesize context or overwrite an old record. No existing
template Jinja name changes; `now()` remains seeded.

After access semantics approval, update `template-format.yml` for the exact
names, types, null rules, collision prefix, and every render site; update its
schema only where identifier validation can encode the reservation. Update
the Jinja guide with examples and trust behavior, the CLI specification with
grant and replay behavior, and the staged-record protocol documentation with
the version and incompatibility rule. Keep this design as implementation
guidance, not a second normative copy of those contracts.

## Falsifiable checks

- One fixture references each name in a prompt, default, `when`, computed
  value, message, interview hook, rendered path/body, file rule, top-level
  hook, and apply message. Every site sees the same captured snapshot.
- A selected formal name survives alias input. Zero aliases yields `[]`;
  multiple aliases are sorted; direct folder and bundled selection have `[]`
  and `none` source. A template short name can differ from formal name.
- Existing and nonexistent target directories yield the same basename from
  `canonical_target`; a root and non-Unicode basename yield `none`.
- Missing, malformed, non-Linux, and partial os-release data follow the table;
  invalid Unicode, empty values, and `USER` fallback produce no lossy string.
- Terminal, headless, async stage, resume, and direct crate fixtures assert
  exact admin/interactive values and no engine process reads.
- Denied access shows five `none` values. Grant exposes only five strings.
  Revoked grant rejects resume before any replay; later grant does not change
  a saved denial. Replay with changed host/process values produces the original
  branches and rendered output.
- Each authored-id surface rejects a `toha_` name at load; nested object keys
  do not. Old staged records fail explicitly. Existing `now()` and presets
  fixtures continue to pass.

## Module-depth and red-flag screen

The context module hides seventeen Jinja projections, null behavior, alias
ordering, and replay-safe representation behind one snapshot interface. The
caller supplies domain facts and one access decision, not evaluation stages.
There is no public process map, wire schema, generic policy hook, or second
target normalizer. A value moves from adapter to `Seed` to `Completed` to the
one Jinja builder to `Plan` within three ownership steps. `StagedRecord` stores
and checks the same type rather than creating a parallel representation. The
private builder earns its place by enforcing collision and projection policy;
no pass-through public method or load/validate/render layer is introduced.
