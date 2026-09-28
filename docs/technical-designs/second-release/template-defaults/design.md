# Template-specific configured defaults — design

Reuse an answer across templates without pretending two questions mean the same
thing because they share an id. Config gains a **named store of reusable presets**
(`presets`) and a **`template-defaults`** map that, for one template *identity*,
points a named question's default at a preset or an inline literal. Nothing is
applied by matching question ids. Resolution happens at the config/resolve boundary
and flattens to the unchanged `Seed.defaults: IndexMap<Id, RawAnswer>`; the pure
interview engine stays identity- and preset-unaware.

This is the synthesized design for the reframed feature, with Bob's approved naming
(`presets`; reference form `{ preset: <name> }`). Base: Candidate 1 (preset store +
identity-keyed mapping, references appear only in mappings). Grafts from Candidate 2
(file-naming attribution) and Candidate 3 (reference provenance in errors). The
prior global-by-id design is superseded (git history `37e2215`); the arena evaluated
the store under the working name `values`, renamed to `presets` on approval.
Evidence: `grounding.md`, `rubric.md`, `candidates/`, `cross-judge.md`,
`synthesis.md`, `verification.md`.

## Caller's usage (the spec)

### The whole new surface a user sees

```yaml
# ~/.config/toha/config.yml

# NEW: a named store of reusable presets. You choose the names; they are NOT
# question ids. A preset is just data you want to reuse.
presets:
  primary_contact: contact@example.invalid
  house_owner: owner/collection

# NEW: per-template defaults, keyed by a template's stable identity (formal name).
# A question's default points at a preset (a reference) or is an inline literal.
# A preset reaches a question ONLY because a mapping says so.
template-defaults:
  "gh:owner/collection":            # this template asks a question id `email`
    email: { preset: primary_contact }   # reference the preset
    license: MIT                          # inline literal
  "gh:owner/newsletter":            # a DIFFERENT template, question id `contact`
    contact: { preset: primary_contact }
  "toha-demo":                      # the reserved bundled-demo identity
    title: { preset: primary_contact }
```

One rule the author internalizes: **`{ preset: <name> }` is a reference to a
preset; any bare scalar or list is an inline literal.** A literal that spells a
preset name (`license: primary_contact`) is the string `primary_contact`, never the
preset it names — the two forms are different YAML shapes.

There is **no** implicit application by question id. A question in another template
that merely shares the id `email` is untouched unless its own mapping references a
preset.

### Required config examples and resolved outcomes

**1 — one preset, two differently-named questions**

```yaml
presets:
  primary_contact: contact@example.invalid
template-defaults:
  "gh:owner/collection": { email:   { preset: primary_contact } }   # asks `email`
  "gh:owner/newsletter": { contact: { preset: primary_contact } }   # asks `contact`
```

| Selected template | Question id | Default becomes |
| --- | --- | --- |
| `gh:owner/collection` | `email` | `contact@example.invalid` |
| `gh:owner/newsletter` | `contact` | `contact@example.invalid` |

Reuse comes from two questions pointing at one **name**, not from a shared id.

**2 — a same-id question with no mapping is untouched**

```yaml
presets: { primary_contact: contact@example.invalid }
template-defaults:
  "gh:owner/collection": { email: { preset: primary_contact } }
  # gh:owner/blog also asks `email`, but has no entry.
```

| Selected template | Question id | Default becomes |
| --- | --- | --- |
| `gh:owner/collection` | `email` | `contact@example.invalid` (mapped) |
| `gh:owner/blog` | `email` | the template's own `default` (untouched) |

**3 — the bundled `toha-demo` identity as a selector** (it asks `title`, `topic`)

```yaml
presets: { default_title: Sample Title }
template-defaults:
  "toha-demo":
    title: { preset: default_title }   # from the preset store
    topic: Sample Topic                # inline literal
```

`toha apply toha-demo ./out` → `title` = `Sample Title`, `topic` = `Sample Topic`.
An installed/aliased/discovered `toha-demo` resolves to a different formal name
(predecessor fallback-only precedence), so its mappings live under that other key.

**4 — a missing reference and a type mismatch (exact attribution)**

```yaml
presets: {}                                  # no default_title
template-defaults: { "toha-demo": { title: { preset: default_title } } }
```
```
error: ~/.config/toha/config.yml: template-defaults."toha-demo".title:
       no preset named "default_title"
```

```yaml
presets: { flag: yes-please }                # a string
template-defaults: { "gh:owner/collection": { agree: { preset: flag } } }  # agree is Confirm
```
```
error: ~/.config/toha/config.yml: template-defaults."gh:owner/collection".agree
       → ~/.config/toha/config.yml: presets."flag" ("yes-please"):
       must be true or false
```

The attribution names the mapping **file** and site and — when the value came
through a reference — the winning preset file and entry, so the author knows
exactly what to fix. The kind message (`must be true or false`, `must be a string`,
`must be an array of strings`) is the engine's existing `parse_kind` sentence.

### Crate caller

```rust
let config = toha::config::load(&dirs.config_paths(), &cwd)?;
let resolved = cli::resolve::resolve_template(arg, &config, &registry, &dirs, &cwd)?;
let template = Template::load(&resolved.folder)?;
// Resolution lives in the toha library, so crate callers get identity scoping too:
let toha::interview::Resolution { defaults, warnings } =
    toha::interview::configured_defaults(
        &resolved.formal_name,
        &template,
        &config.presets,
        &config.template_defaults,
    )?;
warnings.iter().for_each(|warning| eprintln!("warning: {warning}"));
let interview = toha::interview::Interview::start(&template, Seed { now: jiff::Zoned::now(), defaults })?;
```

`Resolution` is never passed as `Seed.defaults`. Supported command drivers print
each warning to stderr; a crate caller receives the same warnings and chooses its
own reporting adapter. The example reports them to stderr so it does not silently
discard a mapped-id warning.

## Module and seam map

```text
   config files: presets + template-defaults   (system, user, local .toha.yml)
        │  parse ONCE at the boundary: classify each leaf (reference vs literal),
        │  parse preset names and question ids, tag each with its file/layer
        ▼
   src/config.rs ── Config { presets, template_defaults, .. }   [+2 fields, -defaults]
        │
        ▼
   src/cli/resolve.rs ── ResolvedTemplate (UNCHANGED)   → formal_name (stable identity)
        │
        ▼
   src/interview.rs ── configured_defaults(formal_name, template, presets, mappings)
        │   pick mappings[formal_name]; for each id the template DEFINES:
        │     reference → look up presets (missing → attributed error)
        │     literal   → use verbatim
        │     validate the used value's kind vs the question kind (attributed)
        │   → Resolution { defaults, warnings } ← identity + presets flattened here
        ▼
   src/main.rs::seed / continue / apply / progress
        ▼
   Seed { now, defaults }
        ▼
   PURE ENGINE, identity- and preset-UNAWARE        [behavior UNCHANGED below Seed]
```

Two files carry the feature (`config.rs`, `interview.rs`); the third change is four
one-line call-site edits in `main.rs`. Resolution stays in the `toha` library so
every driver and crate path is served by one code path (grounding: path parity).

## Data structures expected

```rust
// src/config.rs — parsed once at the boundary into domain types.

/// A config-author-chosen name in the preset store. Matches the existing
/// `identifier` pattern `^[a-z_][a-z0-9_]*$`. Deliberately NOT a question id —
/// the two namespaces never touch.
pub struct PresetName(String);

/// A configured default's source. The reference/literal split is decided ONCE,
/// here, and encoded in the type; downstream code never re-parses it.
pub enum DefaultSource {
    Ref(PresetName),            // `{ preset: <name> }` — resolve against `presets`
    Literal(serde_json::Value), // a bare scalar or string array — used verbatim
}

/// The layer and exact file that supplied a winning merged entry.
pub enum ConfigLayer { System, User, Local }
pub struct ConfigOrigin { pub layer: ConfigLayer, pub path: PathBuf }

/// Merge replaces this whole value, so the winner and its origin cannot drift.
pub struct ConfigEntry<T> { pub value: T, pub origin: ConfigOrigin }

pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    // REMOVED: pub defaults: IndexMap<Id, Value>   (global-by-id is replaced)
    pub presets: IndexMap<PresetName, ConfigEntry<Value>>, // NEW: sourced store
    pub template_defaults:
        IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>, // NEW
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
```

Invariants encoded in the types:

- **Reference vs literal is a type, not a convention** — once past the boundary,
  code matches on `DefaultSource`; a literal can never be re-read as a reference.
- **The preset store holds only literals**; `DefaultSource::Ref` can appear only in
  `template_defaults`. A preset can never reference another preset, so **cycles are
  impossible by construction** — there is no cycle detector because no cycle can
  exist (recommended; chaining is decision D4).
- **Three disjoint key types** — `PresetName` (preset names), `String` (formal
  identities), `Id` (question ids) — so no code can confuse a preset name with an
  identity or a question id.
- **Every merge winner retains its origin** — `ConfigEntry<T>` couples the parsed
  value with its exact config path and system/user/local layer. Per-name and
  per-`(formal name, question id)` replacement moves both together.

The wire form (`Layer`) parses `presets: Option<IndexMap<String, Value>>` and
`template_defaults: Option<IndexMap<String, IndexMap<String, Value>>>`, then
classifies each leaf into `DefaultSource`, validates the names, and wraps every
preset and mapping leaf in `ConfigEntry` with the current file/layer — parse once
at the boundary, trust the types inside.

## Public interfaces expected (signatures)

```rust
// src/interview.rs — replaces today's configured_defaults(template, &config.defaults).
/// Project the preset store and per-identity mappings onto ONE selected template,
/// producing the flat seed the pure engine consumes. `formal_name` selects which
/// mapping applies (exact match). Only questions the template DEFINES are
/// considered; for each with a mapping, the source is resolved (a Ref against
/// `presets`, a Literal verbatim) and kind-checked against the question. Unknown
/// mapped ids are skipped with a warning (templates evolve), never an error.
///
/// Errors, each attributed to the config file + site (+ the preset for a Ref):
///   - Ref to a name absent from `presets` → "no preset named \"<name>\""
///   - resolved value kind ≠ question kind → the parse_kind message
pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    presets: &IndexMap<PresetName, ConfigEntry<Value>>,
    mappings: &IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
) -> Result<Resolution, EvalError>;

/// The resolved seed plus non-fatal warnings (mapped ids the template lacks).
pub struct Resolution { pub defaults: IndexMap<Id, RawAnswer>, pub warnings: Vec<String> }
```

Attribution (graft from C2/C3): `EvalError` gains one optional field carrying the
full config-site string, built from the retained winning origins; the engine's own
template-error rendering is unchanged.

```rust
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
    pub expression: Option<String>,
    pub config_key: Option<String>,   // NEW: e.g.
    //  <mapping-file>: template-defaults."gh:owner/collection".agree
    //  → <preset-file>: presets."flag" ("yes-please")
}
```

Origin selection is deterministic:

- A missing reference and a literal kind error render the winning **mapping
  entry's** path first; there is no second origin for a missing preset, and the
  literal lives at the mapping site.
- A referenced kind error also renders the winning mapping path first, preserving
  the approved leading diagnostic contract, then renders
  `→ <preset-file>: presets."<name>" (<value>)` from the winning preset entry.
- An unknown-question warning names the winning mapping path and site.
- The retained layer is available for structured diagnostics and tests; human text
  uses exact paths, which already distinguish system, user, and local files.

Wiring — all four sites already hold the formal name (`resolved.formal_name` for
direct stage/apply and resume; `saved.template` for progress):

```rust
// seed() gains the formal name; the three staged sites swap the call the same way:
let Resolution { defaults, warnings } = interview::configured_defaults(
    &resolved.formal_name, &template, &config.presets, &config.template_defaults,
)?;
warnings.iter().for_each(|w| eprintln!("warning: {w}"));
// call sites: seed src/main.rs:422, continue :664, apply-staged :902, progress :496
```

On resume the formal name comes from the recorded `StagedRecord.template` and the
preset store + mappings are re-read from live config; nothing is frozen into the
record.

## Precedence — total and deterministic

Layers read system → user → local, as today.

1. **The preset store `presets`** merges per name: `local > user > system` (the
   existing per-key merge used for `defaults`/`hosts`). Other names are kept. Each
   replacement installs the winning value and its `ConfigOrigin` together.
2. **The mappings `template-defaults`** merge per `(formal name, question id)`:
   a later layer replaces that one pair, keeping sibling questions and other
   identities. A layer adding a new identity or id just adds it. The winning
   mapping's origin is replaced with the value.
3. **Interaction.** A reference always resolves against the **fully merged** preset
   store, regardless of which layer the mapping came from — one store, one source of
   truth. A local mapping may reference a user- or system-defined preset.
4. Both `presets` and `template-defaults` are available in system, user, and local
   layers (as `defaults` was). A `template-defaults` key is a formal name, so a
   local mapping supplies *answers for* an identity; it never *remaps* an identity —
   preserving the identity-stability guarantee that keeps `hosts`/`local-config-name`
   out of the local layer.

This is total (every used `(identity, id)` maps to exactly one value or one error)
and deterministic across terminal, headless, staged, direct, and crate paths.

## Selectors and identity

`template-defaults` keys are the stable `formal_name`, matched by **equality**
against the resolved identity — never re-resolved through `registry.resolve`, so a
config key can never raise the exit-5 ambiguity path.

| `<TEMPLATE>` form | resolved `formal_name` | config key |
| --- | --- | --- |
| bare folder / `./x` / `/x` | canonical absolute path | that absolute path |
| git address | `gh:a/b@ref#sub` (normalized) | that normalized address |
| installed name / alias / short | registry formal key (a source address) | that formal key |
| **duplicate short name** | each entry's distinct formal key | one key per entry |
| bundled demo | `toha-demo` (reserved) | `"toha-demo"` |

Two templates that share a short name or a question title have distinct formal
names, hence distinct keys, hence independent defaults. An alias or short name
written as a key simply never equals a `formal_name` and is inert (addressing by
alias/short is not offered — those are not stable identity).

## Typing, missing references, cycles

- **Missing reference** → an attributed `EvalError` before the engine runs; never a
  silent empty default.
- **Type mismatch** → the used value's kind is checked against the question kind via
  the existing `parse_kind`; the error is attributed to the config site (and the
  preset, for a reference).
- **Cycles** → impossible by construction under the recommended shape (references
  appear only in mappings and resolve one hop into a literal preset store). If
  preset→preset chaining is later chosen (D4), a cycle detector and a `$$`-style
  escape become necessary; this design recommends against it.
- **Validate only what is used** — only mappings for the selected identity whose
  question the template defines are resolved and kind-checked; an unreferenced
  preset and a mapping for another identity are never validated. See D5 for the
  reachability nuance (`when`/batch).

## Error / results contract

| Situation | Result |
| --- | --- |
| Mapping id resolves, kind matches | inserted into `Seed.defaults` |
| Ref to a name absent from merged `presets` | `EvalError`, `<mapping-file>: template-defaults."<formal>".<id>: no preset named "<name>"`, exit 1 |
| Value kind ≠ question kind (literal) | `EvalError`, `<mapping-file>: template-defaults."<formal>".<id>: <parse_kind message>`, exit 1 |
| Value kind ≠ question kind (via ref) | `EvalError`, `<mapping-file>: template-defaults."<formal>".<id> → <preset-file>: presets."<name>" ("<value>"): <parse_kind message>`, exit 1 |
| Mapping id the selected template does not define | skipped; one stderr warning naming the winning mapping file/site; non-fatal |
| Mapping for a non-selected identity | ignored |
| Unreferenced preset | never validated |
| Legacy `defaults:` present in any layer | migration error naming the file (see D1), exit 1 |
| Malformed leaf (object that is not `{ preset: <name> }`) | schema rejection naming the file + path, exit 1 |

No new exit code, no new trust/permission/timeout/pinned-check/subprocess surface;
resolution reads config data only.

## Behaviors to prove (falsifiable)

- **B1 reuse_by_name** — one preset referenced by two differently-named questions in
  two templates yields that preset's value for each. *Fails if reuse needs a shared
  id.*
- **B2 no_implicit_by_id** — a template with an `email` question and no mapping keeps
  its own default while another template maps `email`. *Fails if global-by-id leaks
  back.*
- **B3 literal_never_a_ref** — `license: primary_contact` (bare) yields the string
  `primary_contact`, not the preset. *Fails if a literal is read as a ref.*
- **B4 missing_ref_is_attributed** — a `{ preset: N }` with no `presets.N` for a
  defined question yields the exact missing-preset text led by the winning mapping
  file; no silent empty. Put the mapping in a different layer from an unrelated
  preset to prove the winning entry is used. *Fails on silent empty or wrong file.*
- **B5 type_mismatch_is_attributed** — a bad literal and a bad value-via-ref each
  yield the exact attributed message. Put the winning mapping and preset in
  different files: the ref case starts with the mapping file and names
  `→ <preset-file>: presets."<name>"`. *Fails if either winner's origin is lost,
  unattributed, or accepted.*
- **B6 only_used_is_validated** — a broken mapping for a different template, and an
  unreferenced preset, do not error. *Fails on eager whole-config validation.*
- **B7 cycles_impossible** — a `presets` leaf shaped like a reference is rejected by
  the preset-store schema (store leaves are literals only). *Fails if `presets`
  accepts a reference.*
- **B8 identity_selects** — two templates sharing a short name/title but distinct
  formal names receive their own mappings; neither leaks. *Fails if selection keys
  on short name.*
- **B9 bundled_demo_selector** — mappings keyed `toha-demo` apply to the bundled
  demo resolved by that formal name. *Fails if the reserved identity is not matched.*
- **B10 precedence_total** — local preset overrides user preset for a name; local
  mapping overrides user mapping for one `(formal, id)` without dropping siblings; a
  local mapping resolves a user-defined preset; the surviving entries report the
  local mapping origin and user preset origin. *Fails on non-deterministic or
  destructive merge, or if a value wins without its origin.*
- **B11 resume_reresolves** — change a preset between `stage` and `continue`; the
  resumed interview reflects the new value. *Fails if defaults are frozen.*
- **B12 driver_parity** — terminal, headless, staged, direct, and crate paths receive
  identical `Resolution { defaults, warnings }` for identical inputs; command and
  example crate callers surface every warning before passing only `defaults` into
  `Seed`. *Fails if any path diverges or discards a warning.*
- **B13 legacy_defaults_refused** — a config with `defaults:` fails at load with a
  message naming the file and the conversion; the values are not dropped from the
  file. *Fails on silent acceptance or silent drop.*
- **B14 engine_purity** — the pipeline below `Seed` is unchanged; the engine never
  receives the formal name or the preset store. *Fails if identity/presets leaks
  past the boundary.*

## Proposed contract edits (described; applied by paired implementation 1058)

`docs/specifications/config.schema.yml`:

```yaml
$defs:
  preset-name: { $ref: "#/$defs/identifier" }
  literal:
    oneOf:
      - { type: string }
      - { type: boolean }
      - { type: array, items: { type: string } }
  reference:
    type: object
    additionalProperties: false
    required: [preset]
    properties: { preset: { $ref: "#/$defs/preset-name" } }
  presets:
    type: object
    propertyNames: { $ref: "#/$defs/preset-name" }
    additionalProperties: { $ref: "#/$defs/literal" }
  default-source:
    oneOf: [ { $ref: "#/$defs/reference" }, { $ref: "#/$defs/literal" } ]
  template-defaults:
    type: object                       # keys are formal names (free-form)
    additionalProperties:
      type: object
      propertyNames: { $ref: "#/$defs/identifier" }   # question ids
      additionalProperties: { $ref: "#/$defs/default-source" }
# shared-config AND local-config: REMOVE `defaults`; ADD `presets` and `template-defaults`.
```

The `default-source` `oneOf` makes the representation airtight: a reference is an
object with exactly `preset`; a literal is a scalar or string array; any other
object is a schema error (not a misread). A stray `defaults:` is caught at load
(before schema) for a friendly migration message.

`docs/configuration.md`: replace "Set default answers" with "Presets and template
defaults" — the two properties, the reference-vs-literal rule, the precedence, the
file-naming errors, and a **guardrail** steering `presets` names away from question
ids (so the store does not read like the removed global-by-id model).
`docs/specifications/template-interviews.md`: a configured default reaches a
question only through a `template-defaults` mapping for the selected template's
formal name, re-resolved against live config on resume.

## Out of scope

The pure interview engine and its `Seed.defaults` contract; `Template`, `Plan`,
`apply`, `protocol`, `staging`, `StagedRecord`; `ResolvedTemplate`'s shape and the
`<TEMPLATE>` classification order; the trust model (a default never influences trust
or runs a hook); registry writes (defaults stay a config-side preference). No
preset→preset references (D4); no alias/short-name mapping keys; no object-valued
answers; no new subcommand, trust, permission, timeout, pinned-version check, or
subprocess. No production or shared schema/spec edits in this design task.

## Alternatives explored

- **Ordered binding list (arena C2).** `template-defaults` as a list of `{template,
  question, value|literal}` rows with reference/literal in distinct keys. More
  explicit and greppable, and its distinct-key form is airtight without relying on
  the answer shape; rejected as the base for per-row verbosity and because matching
  selectors through the registry re-admits ambiguity. Its file/index attribution is
  grafted; its distinct-key form was offered as decision D2.
- **Sigil references with chaining (arena C3).** `${name}` references, `$$` escape,
  presets may reference presets, resolved by a cycle-detecting graph. Rejected: the
  sigil re-opens the literal-vs-reference collision the grounding lists under
  *Avoid*, and chaining/cycles are an anti-requirement; its reference-provenance
  error text is grafted.
- **Broader reusable-value directions** (compared on accidental effects, user
  control, author dependence, name stability, complexity): a shared preset **group**
  a template can request; tags/traits on templates; named answer **profiles**
  selected per run. Recommended future direction: **named profiles** layered on the
  preset store (explicit, no accidental inheritance, no author dependence) — not
  built in this slice.

## Decisions (Bob-approved)

Bob approved the recommended option for each decision, with one naming revision
(the store is `presets`, the reference form is `{ preset: <name> }`).

- **D1 — Migration of existing `defaults:` = reject-with-conversion.** A `defaults:`
  block is a load error naming the file and showing the exact `presets:` +
  `template-defaults:` rewrite — the user's data stays in the file, nothing runs
  until converted, and no behavior changes silently. Auto-translate is infeasible
  (global-by-id has no identity to translate into); the deprecation-window option
  was rejected. Adopt-into-`presets`-with-warning remains the only fallback and only
  if a real 0.1 `defaults:` install base is confirmed.
- **D2 — Reference/literal representation = concise.** `{ preset: <name> }`
  reference + bare scalar/array literal, airtight via the schema `oneOf`. The
  distinct-key alternative was declined.
- **D3 — Names.** The store is **`presets`** and the mappings are
  **`template-defaults`**; references are `{ preset: <name> }`. (`presets` is the
  approved revision of the arena's working name `values`.)
- **D4 — Value chaining = disallow.** No preset→preset references; cycles impossible
  by construction, no escape rule needed.
- **D5 — Validation scope = validate-when-defined**, consistent with today's
  `configured_defaults`, deterministic, catches config errors early. Disclosure
  (documented, not a defect): a preset has one answer kind (its JSON shape), so one
  preset can seed only questions of that kind.

No decision introduces a permission/access change, a timeout, a pinned check, or an
application subprocess. D1 **removes** the global-by-id capability — the replacement
Bob directed and authorized; the migration disposition is how it is removed without
silent data loss.

## Size and complexity

- **Size:** ~M. Two new sourced-entry `Config` fields + a boundary classify/merge block and the
  `defaults` removal in `config.rs`; a rewritten `configured_defaults` (identity +
  preset resolution, file-naming attribution, provenance) in `interview.rs`; four
  one-line wiring changes in `main.rs`; schema/doc edits (owned by 1058); the
  B1–B14 tests. A migration error path for `defaults:`.
- **Complexity:** low–moderate. The preset store and mappings are plain maps of
  `ConfigEntry<T>` merged per key; resolution is one hop with no graph; cycles and ambiguity are designed
  out. No new concurrency, capability, or engine state. Expected agent
  implementation time: roughly a focused day against this design.
