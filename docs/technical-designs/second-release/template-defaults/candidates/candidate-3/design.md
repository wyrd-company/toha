# Template-specific configured defaults — design (Candidate 3: qualified selector keys)

Scope a configured default to a template **by an explicitly qualified key**. Each
key names *which kind of selector it is* — `formal:`, `alias:`, `name:`, or
`path:` — so the selector kind is unambiguous **by construction**. A short-name
collision, or an alias that happens to spell the same string as a formal name,
becomes unrepresentable as a mistake: `alias:notes` and `formal:notes` are
different keys with different match rules, and neither can silently stand in for
the other. Identity is resolved at the config/resolve boundary and flattened into
the existing `Seed.defaults: IndexMap<Id, RawAnswer>`. The pure interview engine
learns nothing new.

## Caller's usage (the spec)

### Quickstart the config author reads

```text
`defaults:` sets an answer for a question id across every template that asks it.
`template-defaults:` sets answers for ONE template, addressed by a qualified key:

    formal:<formal-name>   the template's stable identity (never depends on your
                           registry). A git address (gh:owner/collection), an
                           installed formal key, an absolute folder path, or the
                           built-in `toha-demo`.
    alias:<alias>          a name you gave the template in your registry.
    name:<short-name>      the template.yml short name — matches EVERY installed
                           template with that short name (a broadcast, on purpose).
    path:<path>            a folder, resolved relative to this config file.

When two keys can set the same question for the same template, the more precise
key wins: formal > path > alias > name > (plain `defaults:`). Ties within one
file are refused, not guessed.
```

### Config-file example

`~/.config/toha/config.yml` (user layer):

```yaml
# Global-by-id, unchanged. Applies to any template that asks these ids.
defaults:
  owner: acme
  include_summary: false

# Template-specific. Keyed by qualified identity.
template-defaults:
  "formal:gh:owner/collection":     # a git-addressed / installed template
    title: Project Alpha
    include_summary: true
  "alias:my-notes":                 # whatever I aliased locally
    title: Personal Notes
  "name:notes":                     # ALL installed templates short-named `notes`
    owner: notes-team
  "formal:toha-demo":               # the bundled demo, addressed by its identity
    title: Demo Sample
```

`./.toha.yml` (local layer, in a project):

```yaml
template-defaults:
  "path:./templates/report":        # a folder beside this project
    title: Q3 Report
  "formal:gh:owner/collection":     # local refinement of the same template
    title: Project Alpha (this repo)
```

### Concrete call sites and resolved outcome

```console
# A — two templates share the question `title`; each gets its own default.
$ toha stage gh:owner/collection ./a      # resolved formal_name = gh:owner/collection
#   title default  <- "Project Alpha (this repo)"   (formal, local beats formal, user)
#   include_summary default <- true                  (formal, user beats global false)
$ toha stage other/collection ./b         # a DIFFERENT template, also asks `title`
#   title default  <- template's own default          (no matching qualified key)
#   include_summary default <- false                  (global-by-id)

# B — headless apply of the bundled demo, default supplied by identity.
$ printf '{}' > answers.json
$ toha apply --answers answers.json toha-demo ./out
#   title default  <- "Demo Sample"   (formal:toha-demo matches the reserved identity)

# C — short-name broadcast plus per-identity override.
#   Two installed templates are both short-named `notes`
#   (formal keys gh:x/notes and gh:y/notes). `name:notes` sets owner for BOTH;
#   a `formal:` key would override just one.
$ toha stage gh:x/notes ./x    # owner default <- "notes-team"  (name:notes matches)
$ toha stage gh:y/notes ./y    # owner default <- "notes-team"  (name:notes matches)
```

Crate callers are unchanged: they still build `Seed { now, defaults }` and call
`Interview::start` / `replay_with_defaults` with a flat `IndexMap<Id, RawAnswer>`.
The qualified-key machinery lives entirely in the binary's boundary layer.

## Data shapes

### Boundary types (parsed once, in `src/config.rs`)

```rust
/// Which config file a template default came from. Drives layer precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLayer { System, User, Local }

/// A qualified selector, parsed from a `template-defaults` key. The variant IS
/// the selector kind: it cannot be reinterpreted as another kind downstream.
#[derive(Debug, Clone)]
pub enum Selector {
    /// `formal:<formal-name>` — the stable identity. Registry-independent.
    Formal(String),
    /// `alias:<alias>` — a registry alias. Registry-relative by design.
    Alias(String),
    /// `name:<short-name>` — the template.yml short name. Matches every
    /// installed template with that short name. Registry-relative by design.
    Name(String),
    /// `path:<path>` — canonicalized against the config file's directory at
    /// parse time. Registry-independent (it is a folder identity).
    Path(PathBuf),
}

/// One qualified block: a selector, the layer it came from, and its answers.
#[derive(Debug, Clone)]
pub struct TemplateDefault {
    pub selector: Selector,
    pub layer: ConfigLayer,
    /// Question id -> raw value. Ids validated as `identifier` at parse time.
    pub values: IndexMap<Id, Value>,
}

/// Config gains ONE field. `defaults` (global-by-id) is byte-for-byte preserved.
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub defaults: IndexMap<Id, Value>,             // PRESERVED: global-by-id
    pub template_defaults: Vec<TemplateDefault>,   // NEW: system, then user, then local
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
```

The wire type extends the private `Layer` struct only:

```rust
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    defaults: Option<IndexMap<String, Value>>,
    // NEW: qualified key -> (id -> value). Keys/ids validated after parse.
    template_defaults: Option<IndexMap<String, IndexMap<String, Value>>>,
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
}
```

Rationale for a `Vec<TemplateDefault>` rather than a map keyed by selector: a
single resolved template can match several keys (a `name:` broadcast plus a
`formal:` refinement), and we need every match with its layer to run precedence.
A flat map keyed by selector-string would force a re-parse of the key kind at
every read — the classic "we'll add an index later" smell the runner-prompt warns
against. The dominant access pattern is "for this one resolved template, gather
all matching blocks," which a `Vec` walked once serves directly.

### The identity a selector matches against (in `src/cli/defaults.rs`)

```rust
/// The identity a resolved template presents to config selectors. Built where
/// the registry is already in hand; the engine never sees it.
pub struct TemplateIdentity<'a> {
    pub formal_name: &'a str,        // resolved.formal_name (git addr, path, or toha-demo)
    pub short_name: Option<&'a str>, // registry entry's short `name`, if listed
    pub aliases: &'a [String],       // registry entry's aliases, if listed
    pub folder: &'a Path,            // resolved.folder (canonical)
}

impl<'a> TemplateIdentity<'a> {
    /// Derive from a resolved template and the registry it resolved against.
    /// For a template with no registry entry (bare folder, git-by-address,
    /// bundled demo) `short_name`/`aliases` are empty, so only `formal:`/`path:`
    /// keys can match it.
    pub fn of(resolved: &'a ResolvedTemplate, registry: &'a Registry) -> Self { unimplemented!() }
}
```

`ResolvedTemplate` is **unchanged** (respecting the bundled-demo coordination
contract: no new field or variant). Identity is derived, not stored.

## Function signatures and the merge

### Parse at the config boundary (`src/config.rs`)

```rust
impl Selector {
    /// Parse a qualified key. `dir` is the config file's parent directory (used
    /// for `path:`); `home` expands a leading `~`. Errors on unknown qualifier,
    /// missing `:`, or empty body — reported as `ConfigError::Parse` naming the file.
    fn parse(key: &str, dir: &Path, home: &Path) -> Result<Self, String> {
        // "formal:X" -> Formal(X); "alias:X" -> Alias; "name:X" -> Name(validate identifier);
        // "path:X"  -> Path(canonicalize X against dir/home, mirroring config::path()).
        unimplemented!()
    }
}
// In `load`, after reading each Layer, fold its `template_defaults` into
// Config.template_defaults in (system, user, local) order, tagging each with its
// ConfigLayer, parsing selectors, and parsing inner keys to `Id` exactly as the
// global `defaults` loop already does (a bad id -> ConfigError::Parse naming the file).
```

### Resolve at the boundary (`src/cli/defaults.rs`, new module)

```rust
/// Specificity ordering. Higher = more precise = wins under the primary axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Specificity { Global = 0, Name = 1, Alias = 2, Path = 3, Formal = 4 }

impl Selector {
    /// Does this selector select `id`? Pure predicate — never resolves through
    /// the registry, so it can never raise registry ambiguity.
    fn matches(&self, id: &TemplateIdentity) -> bool {
        match self {
            Selector::Formal(f) => f == id.formal_name,
            Selector::Alias(a) => id.aliases.iter().any(|x| x == a),
            Selector::Name(n)  => id.short_name == Some(n.as_str()),
            Selector::Path(p)  => Path::new(id.formal_name) == p || id.folder == p,
        }
    }
    fn specificity(&self) -> Specificity { /* Formal/Path/Alias/Name */ unimplemented!() }
    /// The key text for error attribution, e.g. `formal:gh:owner/collection`.
    fn display(&self) -> String { unimplemented!() }
}

#[derive(Debug)]
pub enum DefaultsError {
    /// The winning value does not match the question's answer type.
    /// `key` is `defaults` or `template-defaults["<selector>"]`.
    Kind { key: String, id: Id, message: String },
    /// Two keys of equal specificity in the SAME layer set the same id to
    /// different values. Only reachable via multiple `alias:` keys in one file.
    Conflict { id: Id, layer: ConfigLayer, first: String, second: String },
}

/// Resolve the effective configured defaults for ONE resolved template.
///
/// Invariants:
/// - Output is the flat map the pure engine consumes; identity is flattened away.
/// - Deterministic and total: every (id) resolves to exactly one value or an error.
/// - Only the WINNING value per id is validated (shadowed values are never parsed).
/// - Re-runnable: same (template, identity, config) always yields the same map,
///   so terminal/headless/staged/direct/crate and resume paths agree.
pub fn resolve_defaults(
    template: &Template,
    identity: &TemplateIdentity,
    config: &Config,
) -> Result<IndexMap<Id, RawAnswer>, DefaultsError> {
    // For each question id the template defines:
    //   candidates = [global if config.defaults[id]] ++
    //                [ (td.selector, td.layer, td.values[id])
    //                  for td in config.template_defaults
    //                  if td.selector.matches(identity) && td.values.contains(id) ]
    //   rank by (specificity, layer) — specificity PRIMARY, layer secondary.
    //   if top two tie on (specificity, layer) with different values -> Conflict.
    //   validate winner via interview::validate_default -> RawAnswer or Kind error.
    // Collect into IndexMap<Id, RawAnswer> in template question order.
    unimplemented!()
}
```

### One pure helper exposed from the engine (`src/interview.rs`)

```rust
/// Validate one configured-default value against question `id`'s answer type.
/// `Ok(None)` when the template does not define `id`. Pure; no key attribution
/// (the boundary owns the `defaults.` vs `template-defaults[...]` wording).
/// `configured_defaults` is reimplemented on top of this and keeps its exact
/// `configuration key defaults.<id>` error text for the global path.
pub fn validate_default(
    template: &Template,
    id: &Id,
    value: &Value,
) -> Result<Option<RawAnswer>, String> { unimplemented!() }
```

### Wiring (the entire blast radius)

```rust
// src/main.rs — seed() gains the identity it resolves against.
fn seed(template: &Template, identity: &TemplateIdentity, config: &Config) -> Result<Seed, String> {
    Ok(Seed {
        now: /* unchanged */ jiff::Zoned::now(),
        defaults: cli::defaults::resolve_defaults(template, identity, config)
            .map_err(|e| e.to_string())?,
    })
}
// Every call site already holds `resolved: ResolvedTemplate` and `registry`:
//   let identity = TemplateIdentity::of(&resolved, &registry);
// Direct stage/apply (main.rs:422), continue (664), apply-staged (902), and
// progress (496) each build `identity` and pass it in place of `&config.defaults`.
```

## Module / seam map

```text
   config file(s): defaults + template-defaults
            │  parse ONCE at the boundary
            ▼
   src/config.rs ── Config { defaults, template_defaults: Vec<TemplateDefault>, .. }
            │                         (Selector parsed; ids validated)
            ▼
   src/cli/resolve.rs ── ResolvedTemplate (unchanged) + Registry
            │  TemplateIdentity::of(&resolved, &registry)
            ▼
   src/cli/defaults.rs ── resolve_defaults(template, identity, config)
            │   match selectors → rank (specificity, layer) → validate winner
            │   → IndexMap<Id, RawAnswer>   (identity flattened away here)
            ▼
   src/main.rs::seed ── Seed { now, defaults }
            ▼
   src/interview.rs ── PURE ENGINE, identity-unaware (UNCHANGED behavior)
```

Trace: config → `config.rs` → `defaults.rs` (with identity from `resolve.rs`) →
`seed` → engine. Three boundary files; the engine is untouched except for one
extracted pure validator it already contained.

## Precedence — total and deterministic

For a resolved template `T` (identity `I`) and each question id `q` that `T`
defines:

1. **Candidate set.** `C(q)` =
   - the global candidate `config.defaults[q]` if present (already layer-merged
     by the existing loop), with `Specificity::Global`; plus
   - every `td ∈ config.template_defaults` where `td.selector.matches(I)` and
     `q ∈ td.values`, each contributing `(td.selector.specificity(), td.layer, value)`.
2. **Rank.** Order candidates by the tuple `(specificity, layer)` — **specificity
   primary, layer secondary** — and take the maximum:
   - specificity: `Formal(4) > Path(3) > Alias(2) > Name(1) > Global(0)`;
   - layer: `Local(2) > User(1) > System(0)`.
3. **Ties.** The only way two candidates share `(specificity, layer)` with
   different values is two `alias:` keys in the *same* file both matching `T`
   (a template can carry several aliases). That is a genuine authoring mistake →
   `DefaultsError::Conflict` naming both keys. Every other cell is single-valued
   by construction (`formal:`/`name:`/`path:` can match at most one key per file
   for a given id; the global candidate is pre-merged to one value).
4. **Validate the winner.** Parse the winning value against `q`'s answer type via
   `validate_default`. Mismatch → `DefaultsError::Kind` attributed to the winning
   key. Shadowed values are never validated (you pay only for the default you get).
5. **Non-defining ids.** Ids no question defines are ignored (unchanged from
   today's `configured_defaults`).

This order is **total**: every `q` maps to exactly one value or one error, and
the result is independent of config-file key order except within the refused
same-layer/same-specificity tie. It holds identically across terminal, headless,
staged, direct, and crate paths, and on resume (identity is re-derived from the
recorded formal name + live registry; nothing is frozen).

**Worked precedence example** for `title` on `T = gh:owner/collection`, given the
two config files above:

| candidate | specificity | layer | value |
|---|---|---|---|
| `formal:gh:owner/collection` (local) | Formal(4) | Local(2) | "Project Alpha (this repo)" ✅ |
| `formal:gh:owner/collection` (user)  | Formal(4) | User(1)  | "Project Alpha" |
| (no global `title`) | — | — | — |

Winner: the local `formal:` value. Same specificity, local layer breaks the tie.

## Selector behavior across every required form

| `<TEMPLATE>` form | resolved `formal_name` | key that targets it |
|---|---|---|
| bare folder `x`, `./x`, `/x`, drive path | canonical absolute path | `path:./x` (relative to config) or `formal:/abs/x` |
| git address `gh:a/b@ref#sub` | normalized `gh:a/b@ref#sub` | `formal:gh:a/b@ref#sub` |
| installed formal name | registry formal key | `formal:<key>` |
| alias | (its formal key) | `alias:<alias>` or `formal:<key>` |
| short name (unique) | (its formal key) | `name:<short>` or `formal:<key>` |
| **duplicate short name** | each template's own formal key | `formal:<keyA>` and `formal:<keyB>` — **separate defaults** |
| bundled `toha-demo` | `toha-demo` (reserved) | `formal:toha-demo` |

- **Selector matches nothing:** the block is silently inert for that template
  (consistent with today's global default for an id a template does not ask).
  A future `toha config lint` can report qualified keys that match no installed
  template; this slice does not add it.
- **Selector "ambiguous":** impossible for the *kind* (the qualifier fixes it).
  `name:` is intentionally a **broadcast** — it matches every installed template
  with that short name; that is not ambiguity, it is the defined semantics, and
  its low specificity means any `formal:` key overrides it per identity. Because
  config keys are *matched*, never *resolved through `registry.resolve`*, the
  exit-5 registry ambiguity path is unreachable from configured defaults.
- **alias vs formal collision:** unrepresentable. `alias:notes` and
  `formal:notes` are distinct keys with distinct match rules.

## Error / results contract

| Situation | Result |
|---|---|
| Winning value wrong type (global) | `Kind { key: "defaults", id, .. }` → `configuration key defaults.<id>: <msg>` (**unchanged text**) |
| Winning value wrong type (qualified) | `Kind { key: "template-defaults[\"formal:gh:owner/x\"]", id, .. }` → `configuration key template-defaults["formal:gh:owner/x"].<id>: <msg>` |
| Two same-layer `alias:` keys set the same id differently | `Conflict` → `configuration keys template-defaults["alias:a"] and template-defaults["alias:b"] both set <id> in the <layer> file` |
| Unknown qualifier / empty selector body | `ConfigError::Parse` at load, naming the file |
| Bad inner id (`^[a-z_][a-z0-9_]*$`) | `ConfigError::Parse` at load, naming the file (same as global today) |
| Selector matches no resolved template | inert; no error |
| Resume: recorded formal re-derives identity | defaults re-resolved from live config; nothing frozen |

No new exit code; `Kind`/`Conflict` map to the existing `Outcome::Error` (exit 1)
like today's `configured_defaults` failure.

## Behaviors to prove (falsifiable)

- **Two templates, one title, separate defaults** — two templates that both ask
  `title` with distinct formal names receive their own `formal:` defaults; neither
  bleeds into the other. Fails if defaults key on short name or leak across
  identities.
- **Formal beats global** — a `formal:` key in the *system* layer overrides a
  global `defaults` value set in the *local* layer for the same id (specificity
  primary). Fails if layer dominates specificity. *(This is the D-PRECEDENCE
  decision; the test encodes the recommended answer.)*
- **Local beats user at equal specificity** — same `formal:` selector in user and
  local: local wins. Fails if the layer tiebreak regresses.
- **Name broadcast** — two installed templates short-named `notes` both receive a
  `name:notes` default; adding a `formal:<keyA>` key overrides only template A.
  Fails if `name:` targets one template or if `formal:` fails to override.
- **Alias/formal non-collision** — a template with alias `x` and a *different*
  template with formal name `x`: `alias:x` hits only the first, `formal:x` only
  the second. Fails if qualifier kind is ignored.
- **Bundled demo by identity** — `formal:toha-demo` supplies a default to the
  reserved bundled demo, headless and offline. Fails if the reserved identity is
  not matchable.
- **Same-layer alias conflict refused** — two `alias:` keys in one file both
  matching a template and both setting `title` → `Conflict`, not a silent pick.
  Fails if the tie is guessed.
- **Resume re-resolves** — stage a template, change a matching `formal:` default,
  then `continue`: the new value applies (defaults not frozen into the staged
  record). Fails if the staged record carries defaults.
- **Path selector, config-relative** — a local `path:./templates/report` matches a
  sibling folder template regardless of cwd. Fails if `path:` is cwd-relative or
  not canonicalized like `templates-paths`.
- **Global unchanged** — an existing config with only `defaults:` produces the
  exact same seed and the exact same `configuration key defaults.<id>` error as
  before. Fails on any regression to the global path.

## Proposed contract edits (described, not applied)

### `docs/specifications/config.schema.yml`

Add definitions and reference them from both configs:

```yaml
$defs:
  selector:
    type: string
    pattern: '^(formal|alias|name|path):.+$'
  template-defaults:
    type: object
    propertyNames:
      $ref: "#/$defs/selector"
    additionalProperties:
      $ref: "#/$defs/defaults"      # reuse the id->value object shape
  shared-config:
    properties:
      template-defaults:
        $ref: "#/$defs/template-defaults"
  local-config:
    properties:
      template-defaults:
        $ref: "#/$defs/template-defaults"
```

The schema enforces qualifier kind and inner-id shape; it cannot check that a
`name:` value is a valid identifier or that a `path:` exists — those stay as
parse-time (`ConfigError::Parse`) and resolve-time checks respectively. Note the
`name:<short-name>` body is a template short name and follows the same
`identifier` pattern; `formal:`/`path:` bodies are free-form (addresses/paths).

### `docs/configuration.md`

Extend "Set default answers" with a "Scope defaults to a template" subsection
documenting the four qualifiers, the broadcast semantics of `name:`, the
identity-stability contrast (formal/path stable; alias/name registry-relative),
and the precedence rule (formal > path > alias > name > global; local breaks
ties). Add a merge note: template-defaults merge by (id) across layers under the
same precedence.

### Embedded guidance

No new command surface. The `configuration key template-defaults[...]` attribution
string is the only new user-facing message and is produced by `DefaultsError`.

## Global-by-id disposition (OPTIONS + recommendation)

- **G1 — Keep `defaults:` as-is (RECOMMENDED).** Global-by-id remains at
  `Specificity::Global`; `template-defaults:` is purely additive. No narrowing,
  fully backward compatible, no approval needed.
- **G2 — Fold `defaults:` into `template-defaults:` under a `*:` wildcard.** One
  namespace, but it *removes* the `defaults:` key — a narrowing that breaks every
  existing config → requires Bob's explicit approval. Not recommended.
- **G3 — Keep both but warn on any global↔qualified overlap.** Preserves G1 while
  surfacing overlaps; adds runtime noise. Prefer a future `config lint` over a
  warning on every run.

**Recommendation: G1.** Flag: G2 is a capability removal and must not ship
without explicit approval.

## Broader reusable-default mechanisms (future direction)

| mechanism | accidental cross-template | user control | author dependence | identity stability | config complexity |
|---|---|---|---|---|---|
| **A. Tag-based** (`tag:report:` matches templates that declare a tag) | medium (a template gaining a tag silently inherits) | medium | **high** (needs author tags) | low (tags travel with content) | +1 namespace |
| **B. Answer profiles** (named default bundles a qualified key references: `formal:X: { use: work }`) | low (explicit reference) | **high** | none | high (user-defined) | +1 indirection layer |
| **C. Glob selectors** (`formal:gh:owner/*`) | medium-high (new repos under owner inherit) | medium | none | high | small |

**Recommended future direction: B (answer profiles) layered on top of qualified
keys.** Qualified keys stay the addressing primitive; profiles add DRY reuse
without changing the identity model or introducing accidental inheritance. Do
**not** build A, B, or C in this slice — this slice ships qualified keys only.

## Out of scope

The pure interview engine's identity-unawareness and `Seed.defaults` shape; the
`ResolvedTemplate` shape and `<TEMPLATE>` classification order; the trust model
(defaults never touch trust or hooks); the staged-record shape (no defaults
frozen); registry writes (defaults live in config, not `templates.yml`); glob/tag/
profile mechanisms; a `config lint` command. No new subcommand, trust,
permission, timeout, pinned-version check, or subprocess. No production or shared
schema/spec edits in this design task.

## Decisions needed first (Bob)

- **D-PRECEDENCE — specificity vs layer as the primary axis.** *Specificity
  primary* (RECOMMENDED; a precise `formal:` key wins over a broad global even
  from a lower layer — qualification states intent strength) vs *layer primary*
  (local always wins, matching the existing `defaults`/`hosts` mental model, but a
  local global default would then override a user's deliberate `formal:` default).
  This is the "precedence ambiguity when both match" risk from grounding.
  Recommend specificity primary.
- **D-QUALIFIERS — ship all four (`formal`/`alias`/`name`/`path`) or the stable
  pair only.** *All four* (RECOMMENDED; matches every selector form callers
  already use) vs *`formal:`/`path:` only* (registry-independent, simplest, but
  forces long formal keys where an alias would read better). Recommend all four,
  documenting that `alias:`/`name:` are registry-relative.
- **D-GLOBAL — confirm G1** (keep `defaults:` unchanged).
- **D-CONFLICT — refuse same-layer alias ties** vs last-in-file-wins. Recommend
  refuse (loud over silent).

## Size and complexity

- **Size:** ~M. One new boundary module (`src/cli/defaults.rs`, ~150 lines: match,
  rank, merge, errors), a `Selector`/`TemplateDefault` addition and one fold loop
  in `config.rs`, one extracted pure validator in `interview.rs`, and four
  one-line `seed`/resume call-site changes to pass `identity`.
- **Complexity:** low-moderate. The hard part is the precedence table, which is a
  small total order over two enums; everything else is pure matching. No new
  concurrency, capability, or engine state.
