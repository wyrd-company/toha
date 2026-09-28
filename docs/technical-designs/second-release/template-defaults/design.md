# Template-specific configured defaults — design

Scope a configured default answer to a template **identity**, so two templates that
both ask `title` can receive different defaults, while the existing
global-by-question-id `defaults` stay as the base tier. Identity is resolved at the
config/resolve boundary and flattened into the same `Seed.defaults: IndexMap<Id,
RawAnswer>` the pure interview engine already consumes. The engine learns nothing
new.

This is the synthesized design. Base: Candidate 1 (identity-scoped overlay).
Grafts from Candidate 3 (boundary-owned validation and attribution, the
`TemplateIdentity` match model, config-relative path handling, refuse-on-tie) and
Candidate 2 (specificity reserved as an ordinal for a future tier). Evidence:
`grounding.md`, `rubric.md`, `candidates/`, `cross-judge.md`, `synthesis.md`,
`verification.md`.

## Caller's usage (the spec)

### The whole new surface a user sees

```yaml
# ~/.config/toha/config.yml  (user layer)

# Global-by-id defaults — UNCHANGED. Apply to every template that asks the id.
defaults:
  title: Untitled
  include_summary: false

# NEW: identity-scoped defaults. Each key is a template's stable identity.
# For that one template only, these ids override the global default above.
template-defaults:
  "gh:owner/collection":            # an installed / git-addressed template
    title: Team Notes
    include_summary: true
  "toha-demo":                      # the reserved bundled-demo identity
    title: Demo Title
  "/home/me/work/note-template":    # a folder template's canonical-path identity
    title: Work Note
```

The reading rule a user internalizes, in one sentence: **a template-specific
default always beats a global default of the same id; within each of those two
tiers, a higher config layer (local > user > system) wins.**

### Call site A — two templates, same id, different default (the core win)

```console
# Both templates ask `title`. Distinct identities → distinct scoped defaults.
$ toha apply gh:owner/collection ./a --answers empty.json
create note.txt          # title defaulted to "Team Notes"

$ toha apply /home/me/work/note-template ./b --answers empty.json
create note.txt          # title defaulted to "Work Note"

# A third template with no scoped entry falls back to the global default:
$ toha apply gh:someone/other ./c --answers empty.json
create note.txt          # title defaulted to "Untitled" (global-by-id, preserved)
```

### Call site B — specificity beats layer, re-resolved on resume

```console
# user config: template-defaults."gh:owner/collection".title = "Team Notes"
$ toha stage gh:owner/collection ./out       # interview begins; title default "Team Notes"

# ...edit user config: change the scoped title to "Q4 Notes"...
$ toha continue ./out                        # default is now "Q4 Notes"
                                             # (re-resolved from live config,
                                             #  never frozen into the staged record)
```

### Call site C — crate caller

```rust
use toha::interview::{Seed, validate_default};   // validate_default is the new pure helper
// A crate caller that wants global-only defaults keeps calling configured_defaults,
// which is unchanged. A caller that wants identity scoping resolves the identity it
// already holds and builds the flat map, then seeds as today:
let seed = Seed { now: jiff::Zoned::now(), defaults };   // IndexMap<Id, RawAnswer>, unchanged
let interview = toha::interview::Interview::start(&template, seed)?;
```

The identity-scoping resolver lives in the binary's boundary layer (it needs the
`Registry`/`Dirs` a library caller does not have); library callers see no new type
on the engine surface.

## Module and seam map

```text
   config file(s): defaults + template-defaults
        │  parse ONCE at the boundary (ids -> Id; selectors -> domain form)
        ▼
   src/config.rs ── Config { defaults, template_defaults, .. }        [+1 field]
        │
        ▼
   src/cli/resolve.rs ── ResolvedTemplate (UNCHANGED) + Registry
        │  TemplateIdentity::of(&resolved, &registry)   ← identity derived, not stored
        ▼
   src/cli/defaults.rs (NEW) ── resolve_defaults(template, identity, config)
        │   match identity against scoped entries → overlay onto global
        │   → validate only the WINNING value per id (via interview::validate_default)
        │   → IndexMap<Id, RawAnswer>          ← identity flattened away here
        ▼
   src/main.rs::seed / continue / apply / progress
        ▼
   Seed { now, defaults }
        ▼
   src/interview.rs ── PURE ENGINE, identity-UNAWARE               [behavior UNCHANGED]
```

Trace input→result reads through the types alone: config → `config.rs` →
`defaults.rs` (with the identity from `resolve.rs`) → `seed` → engine. Three
boundary files touched; one new boundary module; the engine gains only one
*extracted* pure validator it already contained. Below `resolve_defaults` the flow
is byte-for-byte the existing path.

## Data structures expected

### Config (parsed domain type, `src/config.rs`)

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub defaults: IndexMap<Id, Value>,                 // UNCHANGED: global-by-id base tier
    pub template_defaults: Vec<TemplateDefault>,       // NEW: system, then user, then local
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}

/// One identity-scoped block: which template, which layer it came from, its answers.
#[derive(Debug, Clone)]
pub struct TemplateDefault {
    pub selector: Selector,             // how this block names a template
    pub layer: ConfigLayer,             // System | User | Local — drives the layer tiebreak
    pub values: IndexMap<Id, Value>,    // ids parsed at load (bad id -> ConfigError::Parse)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLayer { System, User, Local }
```

`Config.defaults` keeps its exact current type and meaning, so every existing
reader and crate caller is unaffected. `template_defaults` is a `Vec` because the
recommended selector set (D2 = minimal) matches at most one block per identity per
layer, but the `Vec` + explicit `layer` shape already supports the qualified-key
extension (D2 = qualified), where one resolved template can match several blocks
(a broadcast plus a refinement). The dominant access pattern — "for this one
resolved template, gather every matching block" — is a single walk of the `Vec`.

### Selector (recommended shape, `src/config.rs`)

```rust
/// How a template-defaults block names a template. Recommended (D2 = minimal):
/// the stable formal identity only. The enum leaves room for the qualified
/// extension (D2 = qualified) without reshaping the resolver.
#[derive(Debug, Clone)]
pub enum Selector {
    /// The template's stable `formal_name`: a git address (`gh:owner/collection`),
    /// an installed formal key, an absolute folder path, or the reserved
    /// `toha-demo`. Matched by equality against the resolved identity —
    /// never resolved through `registry.resolve`, so config can never raise the
    /// exit-5 ambiguity path.
    Formal(String),
    // Reserved for D2 = qualified (NOT built unless Bob selects it):
    //   Alias(String)  — registry alias, registry-relative
    //   Name(String)   — template.yml short name, an intentional broadcast
    //   Path(PathBuf)  — folder, canonicalized against the config file's directory
}
```

### The identity a selector matches against (`src/cli/defaults.rs`, new)

```rust
/// The identity a resolved template presents to config selectors. Built where the
/// registry is already in hand; the pure engine never sees it. `ResolvedTemplate`
/// is UNCHANGED (honors the bundled-demo contract: no new field or variant).
pub struct TemplateIdentity<'a> {
    pub formal_name: &'a str,          // resolved.formal_name (git addr, path, or toha-demo)
    pub short_name: Option<&'a str>,   // registry entry short name, if listed (D2=qualified)
    pub aliases: &'a [String],         // registry entry aliases, if listed   (D2=qualified)
    pub folder: &'a Path,              // resolved.folder (canonical)          (D2=qualified)
}
```

### Wire types (boundary only, never leave `config.rs`)

```rust
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    defaults: Option<IndexMap<String, Value>>,                          // UNCHANGED
    template_defaults: Option<IndexMap<String, IndexMap<String, Value>>>, // NEW
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
}
```

Values stay `serde_json::Value` at the boundary; per-question-kind validation is
deferred to the resolver that holds the `Template`, exactly as global defaults work
today (parse structure at the boundary; validate against the question kind where the
kind is known).

## Public interfaces expected (signatures)

### The boundary resolver (`src/cli/defaults.rs`, new module)

```rust
/// Resolve the effective configured defaults for ONE resolved template.
///
/// Invariants:
/// - Output is the flat map the pure engine consumes; identity is flattened away.
/// - Total & deterministic: every id resolves to exactly one value or one error.
/// - Only the WINNING value per id is validated (shadowed values are never parsed).
/// - Re-runnable: same (template, identity, config) always yields the same map, so
///   terminal / headless / staged / direct / crate and resume paths agree.
pub fn resolve_defaults(
    template: &Template,
    identity: &TemplateIdentity<'_>,
    config: &Config,
) -> Result<IndexMap<Id, RawAnswer>, DefaultsError> {
    // For each question id the template defines:
    //   candidates = [ global if config.defaults[id]           -> Specificity::Global ]
    //             ++ [ (block.selector.specificity(), block.layer, block.values[id])
    //                  for block in config.template_defaults
    //                  if block.selector.matches(identity) && block.values.has(id) ]
    //   winner = max by (specificity, layer)          // specificity PRIMARY, layer secondary
    //   if the top two tie on (specificity, layer) with different values -> Conflict
    //   validate winner via interview::validate_default(template, id, value)
    //       -> RawAnswer, or DefaultsError::Kind attributed to the winning key
    // Collect into IndexMap<Id, RawAnswer> in template question order.
    unimplemented!("design sketch — not implemented")
}

impl TemplateIdentity<'_> {
    /// Derive from a resolved template and the registry it resolved against.
    pub fn of<'a>(resolved: &'a ResolvedTemplate, registry: &'a Registry)
        -> TemplateIdentity<'a> { unimplemented!("design sketch") }
}

impl Selector {
    /// Pure predicate — never resolves through the registry, so it can never raise
    /// registry ambiguity. (D2 = minimal: only the Formal arm exists.)
    fn matches(&self, id: &TemplateIdentity<'_>) -> bool {
        match self {
            Selector::Formal(f) => f == id.formal_name,
        }
    }
    fn specificity(&self) -> Specificity { /* Formal -> Identity */ unimplemented!() }
    fn display(&self) -> String { /* e.g. `template-defaults["gh:owner/collection"]` */ unimplemented!() }
}

/// Specificity as an ordinal so a future glob/tag tier slots BETWEEN global and
/// identity without redefining precedence (graft from Candidate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Specificity { Global = 0, /* future: Glob = 1, Tag = 1, */ Identity = 2 }

#[derive(Debug)]
pub enum DefaultsError {
    /// The winning value does not match the question's answer type.
    /// `key` is `defaults` or `template-defaults["<selector>"]` (+ layer/file).
    Kind { key: String, id: Id, message: String },
    /// Two entries of equal specificity in the SAME layer set the same id to
    /// different values. Unreachable under D2 = minimal (Formal matches one key
    /// per layer); reachable only under D2 = qualified via multiple alias keys.
    Conflict { id: Id, layer: ConfigLayer, first: String, second: String },
}
```

### One pure helper extracted from the engine (`src/interview.rs`)

```rust
/// Validate one configured-default value against question `id`'s answer type.
/// `Ok(None)` when the template does not define `id`. Pure; NO key attribution —
/// the boundary owns the `defaults.` vs `template-defaults[...]` wording.
/// `configured_defaults` is reimplemented on top of this and keeps its exact
/// `configuration key defaults.<id>` error text for the global path (UNCHANGED).
pub fn validate_default(
    template: &Template,
    id: &Id,
    value: &Value,
) -> Result<Option<RawAnswer>, String> { unimplemented!("design sketch") }
```

This extraction is behavior-preserving: `configured_defaults` (`src/interview.rs:677`)
becomes a thin loop over `validate_default` that reproduces today's message and
signature byte-for-byte. `EvalError` gains **no** new field; the identity-aware
attribution lives entirely in `DefaultsError` at the boundary.

### Wiring (the entire blast radius outside the new module)

```rust
// src/main.rs — seed() and the three resume sites build the identity (they already
// hold `resolved: ResolvedTemplate` and `registry`) and call resolve_defaults:
let identity = TemplateIdentity::of(&resolved, &registry);
let defaults = cli::defaults::resolve_defaults(&template, &identity, &config)
    .map_err(|e| e.to_string())?;
// call sites: seed() src/main.rs:422, continue src/main.rs:664,
//             apply-staged src/main.rs:902, progress src/main.rs:496 (keeps .ok()?)
```

On resume the identity is re-derived from the recorded formal name
(`StagedRecord.template`) plus the live registry; nothing about the scoped defaults
is frozen into the staged record.

## Precedence — total and deterministic

For a resolved template with identity `I` and each question id `q` the template
defines:

1. **Candidate set** `C(q)` =
   - the global candidate `config.defaults[q]` if present (already layer-merged
     local→user→system by the existing loop), at `Specificity::Global`; plus
   - every scoped block whose `selector.matches(I)` and whose `values` contain `q`,
     each contributing `(selector.specificity(), block.layer, value)`.
2. **Rank** by the tuple `(specificity, layer)` — **specificity primary, layer
   secondary** — and take the maximum:
   - specificity: `Identity(2) > Global(0)` (a future `Glob/Tag(1)` tier sits
     between);
   - layer: `Local(2) > User(1) > System(0)`.
3. **Ties.** Under the recommended minimal selector set, `Formal` matches at most
   one key per layer, so the only tie is the same identity keyed in two layers —
   broken by the layer axis. A genuine equal-specificity, equal-layer tie is
   possible only under D2 = qualified (two alias keys in one file); that is refused
   as `DefaultsError::Conflict`, never guessed.
4. **Validate the winner** via `validate_default`; a mismatch is
   `DefaultsError::Kind` attributed to the winning key. Shadowed values are never
   validated.
5. **Non-defining ids** are ignored, unchanged from today.

This is **total** (every `q` maps to exactly one value or one error) and independent
of config-file key order except within the refused tie. It holds identically across
terminal, headless, staged, direct, and crate paths, and on resume. An interactive
person can still edit the seeded default (`src/cli/terminal.rs:81`), unchanged.

**Why specificity primary (decision D1).** A template-specific default is a precise
statement about one template; a global default is a broad fallback. Precision
winning matches how scoped configuration normally behaves. The one surprising
consequence — a *system* scoped default beats a *local* global default — is called
out as D1; the alternative (layer primary, preserving "local always wins") is a
one-line flip and is pinned by behavior B5 whichever way Bob decides.

## Selectors — how each selection form is addressed

The config key is matched by **equality against the resolved identity**, never by
re-resolving the key through `registry.resolve`. Ambiguity and aliasing are
resolved *before* an identity exists, so template-specific defaults cannot face an
ambiguous or alias-vs-formal collision.

| `<TEMPLATE>` form | resolved `formal_name` (grounding) | recommended key (D2 = minimal) |
| --- | --- | --- |
| bare folder / `./x` / `/x` / drive | canonical absolute path | that absolute path |
| git address | `gh:a/b@ref#sub` (normalized) | that normalized address |
| installed name / alias / short | registry formal key (a source address) | that formal key |
| **duplicate short name** | each entry's **distinct** formal key | one key per entry — no cross-talk |
| bundled demo | `toha-demo` (reserved) | `"toha-demo"` |

- **Two templates sharing a title / short name** have distinct formal names, hence
  distinct keys, hence independent defaults — the feature's reason for existing.
- **Selector matches nothing:** only the global tier applies. Silent and common; an
  optional one-line stderr note may report a scoped key that matched no template
  this run. Never an error.
- **Selector itself ambiguous:** unreachable from configuration. An ambiguous CLI
  short name still fails at `resolve_template` (exit 5) *before* defaults are
  computed; a config key is matched against the already-resolved identity.
- **Alias vs formal collision:** under D2 = minimal, keys are formal names; an alias
  string used as a key simply never matches and is inert. Under D2 = qualified,
  `alias:` and `formal:` are distinct kinds and cannot collide by construction.
- **Identity stability:** git formal names are computed with `config.hosts`, which
  local config cannot set (`config.yml:25-27`), so a scoped key means the same
  template in every directory even when set in a local `.toha.yml`.

## Error / results contract

| Situation | Result |
| --- | --- |
| Scoped default for identity `I`, id asked | value seeds that question |
| Scoped default id not asked by the template | ignored (as global unknown ids are) |
| No scoped entry matches `I` | global tier applies; no error |
| Scoped value wrong type for the question kind | `DefaultsError::Kind` → `configuration key template-defaults["<sel>"].<id>: <msg>` (names the layer/file), exit 1 |
| Global value wrong type (unchanged) | `configuration key defaults.<id>: <msg>`, exit 1 |
| Bad id inside a `template-defaults` block | `ConfigError::Parse` naming the file, exit code unchanged |
| Same-layer, same-specificity tie (D2 = qualified only) | `DefaultsError::Conflict` naming both keys and the layer, exit 1 |
| Scoped key matches no template this run | inert; optional one-line stderr note; no error |
| Ambiguous CLI selector | `ResolveError::Ambiguous`, exit 5 (unchanged; before defaults) |
| Same inputs, any driver path | identical resolved defaults |

No new exit code. No new trust, permission, timeout, pinned-version-check, or
subprocess surface: `resolve_defaults` reads only config maps and the template's
question kinds; it never consults `trusted`/`named` and never runs a hook.

## Behaviors to prove (falsifiable)

- **B1 two templates, one id, two defaults** — `title` scoped to identity `I1`=A and
  `I2`=B; `apply I1` yields A, `apply I2` yields B. *Fails if a bare id still forces
  one shared default.*
- **B2 global preserved** — global `title=G`, no scoped entry for `I3`; `apply I3`
  yields G, byte-identical to the current build across terminal, headless, staged
  `continue`, staged `apply`, and progress. *Fails if global-by-id was narrowed.*
- **B3 specificity beats global** — global `title=G`, scoped `I.title=S`; `I` yields
  S. *Fails if overlay direction is wrong.*
- **B4 layer within scope** — system scoped `I.title=Sys`, local scoped
  `I.title=Loc`; `I` yields Loc. *Fails if layer order is not local>user>system.*
- **B5 specificity dominates layer (pins D1)** — system scoped `I.title=Sys`, local
  *global* `title=LG`; `I` yields Sys. *Fails ⇒ layer-primary was chosen; this test
  encodes D1's answer.*
- **B6 nonmatching identity is silent** — scoped entry only for `I'`; `apply I` uses
  global/template default, no error. *Fails if unknown-key matching leaks or errors.*
- **B7 invalid scoped type attribution** — scoped `I.include_summary="yes"` on a
  confirm question → `configuration key template-defaults["<I>"].include_summary`
  naming the file. *Fails if the error says `defaults.include_summary` or omits the
  file.*
- **B8 duplicate short name isolation** — two templates share short name `notes`,
  distinct formal names `X`,`Y`; scoped `X.title=A`,`Y.title=B`; resolving each by
  its formal name yields A/B; the ambiguous CLI short `notes` still exits 5.
  *Fails if defaults key on short name.*
- **B9 re-resolution on resume** — stage `I`; change scoped `I.title` in config;
  `continue`/`apply` reflects the new value. *Fails if defaults were frozen.*
- **B10 bundled demo identity** — scoped `"toha-demo".title=D`; `apply toha-demo`
  yields D, headless and offline; an installed template is unaffected. *Fails if the
  reserved identity is not keyable.*
- **B11 driver parity** — same config + template + answers give identical output via
  terminal, headless, staged, direct, and crate paths. *Fails if any path bypasses
  `resolve_defaults`.*
- **B12 local layer participates** — a local `.toha.yml` scoped block applies.
  *Fails if scoping is silently system/user-only.*
- **B13 only the winner is validated** — a shadowed, wrong-typed scoped value under a
  valid higher-precedence value does not raise. *Fails if shadowed values are parsed.*
- **B14 engine purity** — `configured_defaults` signature and `Seed.defaults` type
  compile unchanged; a global-only crate build behaves identically. *Fails on any
  engine-surface change.*

## Proposed contract edits (described here, applied by paired implementation 1058)

### `docs/specifications/config.schema.yml`

```yaml
$defs:
  # identifier, prefix, templates-paths, defaults, hosts — UNCHANGED
  template-selector:            # D2 = minimal: a non-empty identity string
    type: string
    minLength: 1
    # D2 = qualified would instead be: pattern '^(formal|alias|name|path):.+$'
  template-defaults:            # NEW
    type: object
    propertyNames: { $ref: "#/$defs/template-selector" }
    additionalProperties: { $ref: "#/$defs/defaults" }   # reuse: inner ids are identifiers
  shared-config:
    properties:
      template-defaults: { $ref: "#/$defs/template-defaults" }   # NEW
  local-config:
    properties:
      template-defaults: { $ref: "#/$defs/template-defaults" }   # NEW: local may scope too
```

Validation coverage: matching vs nonmatching templates (runtime, B1/B6); layers
(B4/B12); aliases (inert key under minimal, B8); invalid value types (per-kind,
B7); bad ids (`propertyNames` on the inner `defaults`, `ConfigError::Parse`).

### `docs/configuration.md` and `docs/template-interviews.md`

- `configuration.md`: add "Set defaults for one template" after "Set default
  answers" — the key is the template's identity, the selector→identity table, the
  one-sentence precedence rule, the worked example from call site A, and an extended
  "How the layers combine" note (scoped merges per (identity, id); template-specific
  beats global). State the identity-key behavior chosen for D4.
- `template-interviews.md:153` ("Configuration can replace a question's default by
  its id") gains a sentence that a default may also be scoped to one template by
  identity, pointing at `configuration.md`.

### Embedded guidance / `docs/specifications/command-line-interface.yml`

The `configuration key template-defaults[...]` attribution string is the only new
user-facing message; it is produced by `DefaultsError`. If guidance enumerates
config keys, add `template-defaults` beside `defaults`.

## Out of scope

The pure interview engine and its `Seed.defaults: IndexMap<Id, RawAnswer>` contract;
`configured_defaults`'s signature; `Template`, `Plan`, `apply`, `protocol`,
`staging`, `StagedRecord`'s shape; `ResolvedTemplate`'s shape and the `<TEMPLATE>`
classification order; the trust model (a scoped default never influences trust and
never runs a hook to produce a value); registry writes (defaults live in config, not
`templates.yml`); glob/tag/profile mechanisms (future tier, not built); any new
subcommand, flag, trust, permission, timeout, pinned-version check, or subprocess;
any production or shared schema/spec edits in this design task.

## Alternatives explored (broader reusable-default mechanisms)

| Mechanism | Accidental cross-template | User control | Author dependence | Identity stability | Config complexity |
| --- | --- | --- | --- | --- | --- |
| **Identity-scoped (this design)** | none — keyed on unique identity | full | none | high (formal name) | low–moderate |
| Formal-name glob (`gh:owner/*`) | bounded to a namespace the author typed | high | none | high | low (one selector extension) |
| Tags/traits (template.yml `tags:`) | wide — a shared/renamed tag hits templates you did not intend | shared with authors | high | low (tags travel with content) | medium (+ template.yml surface) |
| Named answer profiles (opt-in `--profile`) | none (explicit) | highest but manual | none | high | high (+ CLI surface + activation) |

Only the identity-scoped mechanism solves the stated problem (two templates, one id,
different default) automatically and without author cooperation, with zero
accidental cross-template effect. **Recommended future direction:** formal-name glob,
added as the reserved `Specificity::Glob(1)` tier between global and identity, so it
is a later additive change, not a reshape. This slice does **not** build glob, tags,
or profiles.

## Decisions needed first (Bob)

- **D1 — Precedence primary axis.** *Specificity primary* (recommended; a
  template-specific default beats a global one even from a higher layer — precision
  states intent) vs *layer primary* (local always wins, matching the existing
  `defaults`/`hosts` mental model, but a local global default would then override a
  user's deliberate scoped default). All three candidates recommend specificity
  primary; pinned by B5. **Recommend specificity primary.**
- **D2 — Selector surface.** *Minimal* (recommended; keys are the stable formal
  identity only — smallest vocabulary, solves the stated problem, zero new
  ambiguity) vs *qualified* (`formal:`/`alias:`/`name:`/`path:`; richer — supports
  addressing by alias and an intentional short-name broadcast — but a wider
  vocabulary, with `alias:`/`name:` registry-relative). The base architecture makes
  qualified an additive later change. **Recommend minimal now; qualified reserved as
  the documented growth path.**
- **D3 — Global-by-id disposition.** *Keep `defaults` unchanged as the base tier*
  (recommended; purely additive, no narrowing, no approval needed) vs *deprecate or
  fold it away* (removes a supported capability — **requires explicit approval** per
  the inherited constraint; not recommended). **Recommend keep unchanged.** No
  supported-capability restriction is proposed by this design.
- **D4 — What defines a default's identity key across versions.** The key is the
  resolved `formal_name`. Consequence surfaced by cross-judge: a ref-pinned git
  address (`gh:a/b@v1` vs `@v2`) is a *different* key, so a ref bump silently drops
  its scoped defaults, while an installed template updated in place keeps its key and
  carries defaults over; a moved folder orphans its path key. Options: *(i)*
  ref-sensitive exact `formal_name` (recommended; the key is exactly the resolved
  identity, consistent with the rest of the system where `@ref` is part of identity;
  documented, and users key on the unpinned/installed formal name for stability);
  *(ii)* a ref-insensitive convenience key (more forgiving but conflates diverging
  refs and adds a second identity notion). The local-config portability half is
  already mitigated: folder/path keys in a local config resolve relative to the
  config file (reusing `config::path`). **Recommend (i) with clear documentation.**

None of D1–D4 removes, narrows, or reinterprets a supported capability, introduces a
permission/access change, a timeout, a pinned check, or an application subprocess —
provided D2 = minimal / qualified (both additive) and D3 = keep unchanged. Choosing
D3 = deprecate/fold, or any future narrowing, would require separate explicit
approval and is not recommended here.

## Size and complexity

- **Size:** ~M. One new boundary module (`src/cli/defaults.rs`: match, rank,
  overlay, errors), a `Selector`/`TemplateDefault`/`ConfigLayer` addition and one
  fold loop in `config.rs`, one extracted pure `validate_default` in `interview.rs`,
  four one-line wiring changes in `main.rs`, and the schema/doc edits (owned by
  1058). Test suite: the B1–B14 behaviors as unit + fixture tests.
- **Complexity:** low–moderate. The hard part is the precedence table — a small
  total order over two enums; everything else is pure matching and an overlay fold.
  No new concurrency, capability, or engine state. Expected agent implementation
  time: roughly a focused day against this design.
