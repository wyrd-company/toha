# Template-specific configured defaults — design (candidate 1)

Named stored values plus explicit per-template references. Config gains two
top-level properties: **`values`** (a named store of reusable literals the
config author names) and **`template-defaults`** (per template *identity*, a
map of question id → a stored-value reference or an inline literal). Nothing is
applied by matching question ids; a value reaches a question only because a
mapping says so. Resolution happens at the existing `configured_defaults` seam
and flattens to the unchanged `Seed.defaults: IndexMap<Id, RawAnswer>`. The pure
engine stays identity- and store-unaware.

## Caller's usage (the spec)

### Quickstart (README voice)

```text
Reuse an answer across templates without pretending two questions mean the same
thing just because they share an id.

`values` names data you want to reuse. `template-defaults` says, for one
template *by its formal name*, which of that template's questions take a stored
value or an inline literal as their default.

    values:
      primary_contact: contact@example.invalid

    template-defaults:
      gh:owner/collection:
        email: { value: primary_contact }   # reference the stored value
        license: MIT                         # inline literal

A default reaches a question only through a mapping. A question in some other
template that merely happens to share the id `email` is untouched. A person
running an interactive interview can still change any default.
```

Two forms, one rule: **a mapping (`{ value: <name> }`) is a reference; anything
else is an inline literal.** Answers in toha are only strings, booleans, or
lists of strings — never objects — so a mapping is *always* a reference and a
bare scalar or list is *always* a literal. A literal that spells a stored-value
name (`license: primary_contact`) is the string `primary_contact`, never the
value it names.

### Call sites (crate consumer, same code every driver runs)

```rust
// 1. Load layered config once at the boundary; `values` and `template-defaults`
//    are already parsed into domain types (references classified, ids parsed).
let config = toha::config::load(&dirs.config_paths(), &cwd)?;

// 2. Resolve the template to its stable identity (unchanged).
let resolved = cli::resolve::resolve_template(arg, &config, &registry, &dirs, &cwd)?;
let template = Template::load(&resolved.folder)?;

// 3. Project config → the flat seed the engine already consumes. NEW: keyed by
//    the selected template's formal name; references resolved against the store.
let defaults: IndexMap<Id, RawAnswer> = toha::interview::configured_defaults(
    &resolved.formal_name,   // the identity that selects which mappings apply
    &template,               // the questions that decide "actually used" + kinds
    &config.values,          // the merged store
    &config.template_defaults,// the merged per-identity mappings
)?;

// 4. Same engine, same Seed contract, byte-for-byte unchanged from here down.
let seed = Seed { now: jiff::Zoned::now(), defaults };
let interview = Interview::start(&template, seed)?;
```

The public surface a caller sees grows by exactly: two `Config` fields and four
arguments to one existing function. Everything below `Interview::start` is
untouched.

## Required config examples (concrete YAML + resolved outcomes)

### 1. One stored value, two differently-named questions in two templates

```yaml
values:
  primary_contact: contact@example.invalid

template-defaults:
  gh:owner/collection:      # this template asks a question with id `email`
    email: { value: primary_contact }
  gh:owner/newsletter:      # this template asks a question with id `contact`
    contact: { value: primary_contact }
```

Resolved outcome:

| Selected template (formal name) | Question id | Default becomes |
| --- | --- | --- |
| `gh:owner/collection` | `email` | `contact@example.invalid` |
| `gh:owner/newsletter` | `contact` | `contact@example.invalid` |

One named value, reused by two questions with **different** ids. Reuse comes
from pointing at a name, not from a shared id.

### 2. A same-id question with no mapping is untouched (no implicit application)

```yaml
values:
  primary_contact: contact@example.invalid

template-defaults:
  gh:owner/collection:
    email: { value: primary_contact }
  # gh:owner/blog has NO entry, though it also asks a question id `email`
```

Resolved outcome:

| Selected template | Question id | Default becomes |
| --- | --- | --- |
| `gh:owner/collection` | `email` | `contact@example.invalid` (mapped) |
| `gh:owner/blog` | `email` | the template's own `default` (untouched) |

The store value never reaches `gh:owner/blog`'s `email`. Sharing an id with a
mapped question grants nothing.

### 3. The bundled `toha-demo` referenced as a selector

The bundled demo asks `title` (Text) and `topic` (Text).

```yaml
values:
  default_title: Sample Title

template-defaults:
  toha-demo:                 # the reserved bundled identity (formal name)
    title: { value: default_title }   # from the store
    topic: Sample Topic               # inline literal
```

Resolved outcome for `toha apply toha-demo ./out`:

| Question id | Default becomes | Source |
| --- | --- | --- |
| `title` | `Sample Title` | stored value `default_title` |
| `topic` | `Sample Topic` | inline literal |

An installed, aliased, or discovered `toha-demo` uses that same formal name, so a
mapping keyed `toha-demo` applies to whichever template that identity resolves
to — consistent with the predecessor's reserved-fallback contract.

### 4. Missing-reference and type-mismatch errors (exact attribution text)

Missing reference — a mapping points at a name the store does not define:

```yaml
values: {}                 # note: no `default_title`
template-defaults:
  toha-demo:
    title: { value: default_title }
```

```
error: configuration key template-defaults."toha-demo".title: no stored value named "default_title"
```

Type mismatch — a value's kind does not match the question's answer kind. The
demo's `title` is a text question; here it is given a boolean literal:

```yaml
template-defaults:
  toha-demo:
    title: true
```

```
error: configuration key template-defaults."toha-demo".title: must be a string
```

Same attribution shape when the mismatch comes through a reference (the stored
value `flag: yes-please` is a string, the question `agree` is a Confirm):

```yaml
values:
  flag: yes-please
template-defaults:
  gh:owner/collection:
    agree: { value: flag }
```

```
error: configuration key template-defaults."gh:owner/collection".agree: must be true or false
```

Attribution rule: `configuration key template-defaults.<formal>.<id>: <message>`
where `<formal>` is double-quoted (formal names contain `:` `/` `@` `#`) and
`<message>` is the missing-ref text or the existing `parse_kind` kind message
(`must be a string`, `must be true or false`, `must be an array of strings`).
The message names the config site, never the template file — the fault is in the
config.

## Data shapes

### Boundary (`src/config.rs`) — parsed once into domain types

```rust
/// A config-author-chosen name in the `values` store. Matches the existing
/// `identifier` pattern `^[a-z_][a-z0-9_]*$` so it reads as a name, not a path
/// or an address. Deliberately NOT a template question id — the two namespaces
/// never touch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueName(String);
impl ValueName {
    pub fn parse(value: &str) -> Result<Self, String>; // identifier pattern
    pub fn as_str(&self) -> &str;
}

/// A configured default's source: a reference into the store, or an inline
/// literal. Classified at the boundary; the `oneOf` is decided by structure —
/// a JSON/YAML object is a reference, everything else is a literal.
#[derive(Debug, Clone)]
pub enum DefaultSource {
    /// `{ value: <name> }` — resolve against the merged `values` store.
    Ref(ValueName),
    /// A bare scalar or list of strings; used verbatim.
    Literal(serde_json::Value),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    // REMOVED: pub defaults: IndexMap<Id, Value>,
    /// The merged named store: name -> literal value.
    pub values: IndexMap<ValueName, serde_json::Value>,
    /// The merged per-identity mappings: formal name -> (question id -> source).
    pub template_defaults: IndexMap<String, IndexMap<Id, DefaultSource>>,
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
```

Invariants encoded in the types (per `encode-lessons-in-structure`):

- **Reference vs literal is a type, not a convention.** Once past the boundary
  no code re-parses `{ value: … }`; it matches on `DefaultSource`. A literal can
  never be re-read as a reference downstream.
- **The store holds only literals** (`serde_json::Value` that is a scalar or a
  string array). `DefaultSource::Ref` can only appear in `template_defaults`, so
  **a stored value can never reference another stored value — cycles are
  impossible by construction.** No cycle detector exists because no cycle can.
- **Store keys and mapping keys live in different key types** (`ValueName` vs
  `String` formal names vs `Id` question ids). A value name can never be
  mistaken for a template id or a formal name in code.

The raw wire form (`Layer`) parses `values: Option<IndexMap<String, Value>>` and
`template_defaults: Option<IndexMap<String, IndexMap<String, Value>>>`, then
`classify` turns each leaf `Value` into a `DefaultSource` and `ValueName::parse`
/ `Id::parse` validate the keys — parse once at the boundary, trust the types
inside (per `boundary-discipline`).

```rust
fn classify(raw: &serde_json::Value) -> Result<DefaultSource, String> {
    match raw {
        serde_json::Value::Object(map) => match (map.len(), map.get("value")) {
            (1, Some(serde_json::Value::String(name))) => {
                Ok(DefaultSource::Ref(ValueName::parse(name)?))
            }
            _ => Err("a default must be a reference `{ value: <name> }` or an \
                      inline literal (string, boolean, or list of strings)".into()),
        },
        other => Ok(DefaultSource::Literal(other.clone())), // scalar or array
    }
}
```

### Seam (`src/interview.rs`) — engine-facing, unchanged output type

```rust
pub type Answers = IndexMap<Id, Answer>;      // unchanged
pub struct RawAnswer(pub Value);              // unchanged
pub struct Seed { pub now: jiff::Zoned,
                  pub defaults: IndexMap<Id, RawAnswer> }  // UNCHANGED contract
```

## Function signatures

### Replace the projection (interview.rs)

```rust
/// Project the config store and per-identity mappings onto ONE selected
/// template, producing the flat seed the pure engine consumes.
///
/// `formal_name` selects which mapping applies (exact match against the key).
/// Only questions the template actually defines are considered ("validate only
/// the value actually used"): for each such question with a mapping, the source
/// is resolved (a `Ref` against `values`, a `Literal` verbatim) and type-checked
/// against the question's `PromptKind`. Unreferenced store values and mappings
/// for other identities are never touched here.
///
/// Errors (all attributed to `configuration key template-defaults."<formal>".<id>`):
/// - `Ref` to a name absent from `values`  -> `no stored value named "<name>"`.
/// - resolved value's kind != question kind -> the `parse_kind` message.
///
/// A mapping for an id the template does not define is skipped with a warning
/// (returned out-of-band; see `Resolution`), never an error — templates evolve.
pub fn configured_defaults(
    formal_name: &str,
    template: &Template,
    values: &IndexMap<ValueName, Value>,
    mappings: &IndexMap<String, IndexMap<Id, DefaultSource>>,
) -> Result<Resolution, EvalError>;

/// The resolved seed plus any non-fatal warnings (unknown question ids).
pub struct Resolution {
    pub defaults: IndexMap<Id, RawAnswer>,
    pub warnings: Vec<String>,
}
```

Body sketch (`not implemented` for the real thing):

```rust
// let mut out = IndexMap::new();
// let mut warnings = Vec::new();
// let Some(map) = mappings.get(formal_name) else { return Ok(Resolution::empty()) };
// for (id, source) in map {
//     let Some(q) = question_by_id(&template.interview, id) else {
//         warnings.push(unknown_question_warning(formal_name, id)); // not fatal
//         continue;                                                 // "only used"
//     };
//     let value: Value = match source {
//         DefaultSource::Literal(v) => v.clone(),
//         DefaultSource::Ref(name) => values.get(name).cloned()
//             .ok_or_else(|| missing_ref(formal_name, id, name))?,   // attributed
//     };
//     parse_kind(id, prompt_kind(q), value.clone())                  // type-check
//         .map_err(|e| config_eval_error(formal_name, id, e.message))?;
//     out.insert(id.clone(), RawAnswer(value));
// }
// Ok(Resolution { defaults: out, warnings })
```

### Attribution helpers (interview.rs)

```rust
/// Build the config-site attribution `template-defaults."<formal>".<id>`.
fn config_key(formal: &str, id: &Id) -> String {
    format!("template-defaults.{:?}.{}", formal, id) // {:?} on &str quotes it
}
/// An attributed configured-default fault.
fn config_eval_error(formal: &str, id: &Id, message: String) -> EvalError {
    EvalError { id: id.clone(), field: CONFIGURED_DEFAULT, message,
                expression: None, config_key: Some(config_key(formal, id)) }
}
fn missing_ref(formal: &str, id: &Id, name: &ValueName) -> EvalError {
    config_eval_error(formal, id,
        format!("no stored value named {:?}", name.as_str()))
}
```

`EvalError` gains one optional field so attribution carries the formal name the
old `id`-only form could not:

```rust
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
    pub expression: Option<String>,
    pub config_key: Option<String>,   // NEW: full config-site attribution
}
// Display: when `config_key` is Some, render
//   "configuration key {config_key}: {message}"
// otherwise the two existing arms are unchanged.
```

This is additive; the template-expression arms of `Display` do not change, so
the engine's own faults render exactly as today.

### Boundary (config.rs)

```rust
pub fn load(dirs: &ConfigPaths, cwd: &Path) -> Result<Config, ConfigError>;
// merges `values` (per-name) and `template_defaults` (per formal, per id),
// rejects a legacy `defaults:` block (see Migration).
```

### Call-site edits (main.rs) — the entire blast radius outside the two modules

All four sites already hold the formal name (`resolved.formal_name` or
`saved.template`), per grounding.

```rust
// seed() gains the formal name:
fn seed(formal: &str, template: &Template, config: &Config) -> Result<Seed, String> {
    // ... now as before ...
    let Resolution { defaults, warnings } =
        interview::configured_defaults(formal, template, &config.values,
                                       &config.template_defaults)
            .map_err(|e| e.to_string())?;
    warnings.iter().for_each(|w| eprintln!("warning: {w}"));
    Ok(Seed { now, defaults })
}
// main.rs:581  -> seed(&resolved.formal_name, &template, &config)
// main.rs:831  -> seed(&resolved.formal_name, &template, &config)

// progress (496), continue (664), apply-from-staged (902): replace
//   configured_defaults(&template, &config.defaults)
// with
//   configured_defaults(&resolved.formal_name, &template,
//                       &config.values, &config.template_defaults)?.defaults
// (progress keeps ignoring warnings; continue/apply print them once.)
```

## Module / seam diagram

```text
   config files (system, user, local .toha.yml)
        │  values:            template-defaults:
        ▼
 ┌──────────────── src/config.rs (BOUNDARY: parse once) ─────────────────┐
 │ read+schema-validate each Layer                                        │
 │ merge values  (per name,  local>user>system)                          │
 │ merge template_defaults (per formal, per id, local>user>system)       │
 │ classify leaves -> DefaultSource (Ref | Literal); parse ValueName/Id  │
 │ REJECT legacy `defaults:`                                             │
 └───────────────┬───────────────────────────────────────────────────────┘
   Config { values: IndexMap<ValueName,Value>,
            template_defaults: IndexMap<String, IndexMap<Id,DefaultSource>> }
                 │
   resolve_template(arg) ──► ResolvedTemplate { formal_name, .. }  (unchanged)
                 │            Template::load(folder)               (unchanged)
                 ▼
 ┌──────────── src/interview.rs :: configured_defaults (SEAM) ───────────┐
 │ pick mappings[formal_name]; for each id the template defines:         │
 │   Ref  -> values[name]      (missing -> attributed EvalError)         │
 │   Literal -> value                                                    │
 │   parse_kind(kind, value)   (mismatch -> attributed EvalError)        │
 │ -> Resolution { defaults: IndexMap<Id, RawAnswer>, warnings }         │
 └───────────────┬───────────────────────────────────────────────────────┘
                 ▼   Seed { now, defaults }   (CONTRACT UNCHANGED)
        ───────────── UNCHANGED FROM HERE DOWN ─────────────
        Interview::start / replay_with_defaults  (pure engine;
        never learns the formal name or the store)
```

Trace: config file → `config.rs` → `interview.rs::configured_defaults` → the
unchanged engine. Two files carry the whole feature; the third change is four
one-line call-site edits. Under three files to trace input→output (per
`minimize-reader-load`).

## Precedence rule (total, deterministic)

Layers are read system → user → local (as today, `config.rs:152`).

1. **Store (`values`).** Merge by name. When a name appears in more than one
   layer, the highest layer wins the whole value: **local > user > system**.
   (Same discipline as today's `defaults`/`hosts` per-key merge.)
2. **Mappings (`template-defaults`).** Merge by `(formal name, question id)`.
   A formal-name key present in several layers is unioned; within it, each
   question id is resolved by highest layer: **local > user > system**. A layer
   adding a new formal-name key or a new id under an existing key just adds it;
   it never wipes sibling entries.
3. **Interaction.** References resolve against the **fully merged store**,
   regardless of which layer the mapping came from. A local mapping may
   reference a user- or system-defined value; a local `values` entry may satisfy
   a system mapping's reference. The two structures merge independently and meet
   only at resolution.
4. **Determinism.** All merges iterate layers in fixed order and overwrite;
   `IndexMap` preserves first-seen order for stable iteration. The result does
   not depend on which driver or path performed the load.

Local layers may set both `values` and `template-defaults` (as local may set
`defaults` today). This does not reintroduce the identity-stability hazard that
keeps `hosts`/`local-config-name` out of local: a `template-defaults` key is a
formal name, so a local mapping supplies *answers for* an identity; it never
*remaps* an identity.

## Error / results contract

| Situation | Result |
| --- | --- |
| Mapping id resolves, kind matches | inserted into `Seed.defaults` |
| `Ref` to a name absent from merged `values` | `EvalError`, `configuration key template-defaults."<formal>".<id>: no stored value named "<name>"`, exit 1 |
| Value kind ≠ question kind (literal or via ref) | `EvalError`, `configuration key template-defaults."<formal>".<id>: <parse_kind message>`, exit 1 |
| Mapping id the selected template does not define | skipped; one `warning: template-defaults."<formal>".<id> names no question in this template` on stderr; non-fatal |
| Mapping for a formal name that is not the selected template | ignored (not the selected identity) |
| Unreferenced store value | never validated (not used) |
| Legacy `defaults:` present in any layer | `ConfigError::Parse` at load naming the file (see Migration); exit 1 |
| Malformed leaf (object that is not `{ value: <name> }`) | schema rejection at load naming the file+path; exit 1 |
| Bad `value` name / bad question id | schema rejection, then `ValueName::parse`/`Id::parse` error naming the file |

No new exit codes; the resolver reuses the pipeline's existing 0/1/2/3/4/5. No
trust, permission, timeout, pinned-check, or subprocess surface is introduced —
resolution reads config data only, exactly as `configured_defaults` does today.

## Behaviors to prove (falsifiable)

- **reuse_by_name** — one `values` entry referenced by two differently-named
  questions in two templates yields that value as each question's default. Fails
  if reuse breaks or couples the ids.
- **no_implicit_by_id** — a template with an `email` question and no mapping
  keeps its own default even while another template maps `email`. Fails if
  global-by-id leaks back.
- **literal_never_a_ref** — `license: primary_contact` (bare) yields the string
  `primary_contact`, not the value named `primary_contact`. Fails if a literal
  is misread as a reference.
- **missing_ref_is_attributed** — a `{ value: N }` with no `values.N` for a
  question the template defines produces the exact missing-ref text, not a silent
  empty default. Fails on silent empty.
- **type_mismatch_is_attributed** — a boolean literal (and a string via ref) on
  a text/confirm question produces the exact `parse_kind` message at the config
  site. Fails if unattributed or accepted.
- **only_used_is_validated** — a missing ref / bad-kind value in a mapping for a
  *different* template, and an unreferenced store value, do NOT error. Fails on
  eager load-time validation.
- **cycles_impossible** — the store type admits no reference, so no cycle path
  exists; a `values` entry that is a mapping `{ value: x }` is rejected by the
  store schema (`values` leaves are literals only). Fails if `values` ever
  accepts a reference.
- **identity_selects** — two templates sharing a short name/title but distinct
  formal names receive their own mappings; neither leaks to the other. Fails if
  selection keys on short name.
- **bundled_demo_selector** — mappings keyed `toha-demo` apply to the bundled
  demo resolved by that formal name. Fails if the reserved identity is not
  matched.
- **precedence_total** — local value overrides user value for the same name;
  local mapping overrides user mapping for the same `(formal, id)` without
  dropping sibling entries; a local mapping resolves a user-defined value. Fails
  on any non-deterministic or destructive merge.
- **resume_reresolves** — change a stored value between `stage` and `continue`;
  the resumed interview reflects the new value. Fails if defaults are frozen into
  the staged record.
- **driver_parity** — terminal, headless, staged, direct, and crate paths
  produce identical `Seed.defaults` for identical inputs. Fails if any path
  diverges.
- **legacy_defaults_refused** — a config with `defaults:` fails at load with a
  message naming the file and pointing to the conversion; the values are not
  dropped from the file. Fails on silent acceptance or silent drop.

## Proposed schema / doc edits (described, not applied)

`docs/specifications/config.schema.yml`:

```yaml
$defs:
  # existing: identifier, prefix, templates-paths, hosts ...
  value-name:
    $ref: "#/$defs/identifier"          # store names read as names
  literal:                              # a stored value / inline default
    oneOf:
      - type: string
      - type: boolean
      - type: array
        items: { type: string }
  reference:
    type: object
    additionalProperties: false
    required: [value]
    properties:
      value: { $ref: "#/$defs/value-name" }
  values:
    type: object
    propertyNames: { $ref: "#/$defs/value-name" }
    additionalProperties: { $ref: "#/$defs/literal" }
  default-source:                       # exactly one of the two shapes
    oneOf:
      - $ref: "#/$defs/reference"
      - $ref: "#/$defs/literal"
  template-defaults:
    type: object                        # keys are formal names (free-form)
    additionalProperties:
      type: object
      propertyNames: { $ref: "#/$defs/identifier" }   # question ids
      additionalProperties: { $ref: "#/$defs/default-source" }
  shared-config:
    properties:
      # REMOVE: defaults
      values: { $ref: "#/$defs/values" }
      template-defaults: { $ref: "#/$defs/template-defaults" }
      # templates-paths, hosts, local-config-name unchanged
  local-config:
    properties:
      # REMOVE: defaults
      values: { $ref: "#/$defs/values" }
      template-defaults: { $ref: "#/$defs/template-defaults" }
      # templates-paths unchanged
```

`docs/configuration.md`: replace the "Set default answers" section with a
"Reusable values and template defaults" section — the two-form rule, the
"mapping is always a reference" invariant, the reference/literal examples, the
missing-ref and type-mismatch messages, the precedence rule, and the note that
`template-defaults` keys are formal names (`toha templates list` shows them).

`docs/specifications/template-interviews.yml` (and `template-interviews.md`):
note that a configured default reaches a question only through a
`template-defaults` mapping for the selected template's formal name, resolved at
seed time against live config, never frozen.

## Migration disposition (required product decision — no silent data loss)

Existing configs use `defaults:` (global-by-id). That semantics is being
*replaced*, not extended.

- **Option A — Reject with conversion (recommended).** A `defaults:` block in
  any layer is a load error naming the file:
  `configuration key defaults: replaced by `values` and `template-defaults`; move
  each value into `values:` and add an explicit per-template mapping — see
  docs/configuration.md`. The user's data stays in their file (nothing is
  dropped); toha refuses to run until the author converts. Honest, one-time,
  and it cannot silently change behavior.
- **Option B — Deprecate window.** Keep `defaults:` working (global-by-id) with
  a warning for N releases. **Rejected:** it preserves exactly the implicit
  id-based application the product owner rejected; users keep depending on the
  behavior we are removing.
- **Option C — Auto-lift to `values`.** On load, copy each `defaults` entry into
  `values` under the same name and warn that it no longer applies until mapped.
  **Rejected:** it silently changes behavior (defaults stop applying), and it
  seeds the store with names equal to question ids — the very coupling the
  reframe removes.

**Recommendation: Option A.** Pre-1.0, with the model deliberately replaced and
auto-translation semantically impossible (global-by-id carries no template
identity to attach), a clear refusal that leaves the user's data intact and
points to the conversion is the only path with no silent loss and no revival of
the rejected semantics.

## Out of scope

The pure interview engine, `Seed`/`Interview`/`Plan`/`apply`/`protocol`/
`staging`, `ResolvedTemplate`'s shape, the `<TEMPLATE>` classification order, the
trust model, and path safety. No stored value referencing another stored value
(cycles designed out, not detected). No mapping key other than an exact formal
name (no alias/short-name keys — that would reintroduce ambiguity). No
object-valued answers. No new subcommand, trust, permission, timeout,
pinned-version check, or subprocess. No production, schema, or spec edits in this
design task — all contract edits above are described only.
