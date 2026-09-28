# Template-specific defaults — a value library with sigil references (candidate 3)

Config holds a **value library** (`values:`) of reusable, config-author-named
data, and a **`template-defaults:`** map keyed by template **formal name** →
question id → a default that is either an inline literal or a `${name}`
reference into the library. References may chain through the library, so a small
**graph resolver** walks them, refuses cycles, reports missing names, and — at
the point a value actually reaches a question — checks the value's kind against
the question's answer kind. Every error names its config site. Resolution runs
at the boundary and flattens to the unchanged engine contract
`Seed.defaults: IndexMap<Id, RawAnswer>`; the pure engine stays identity- and
store-unaware.

---

## Caller's usage (the spec)

### Quickstart the config author reads

```text
Toha never guesses which template a saved answer belongs to. You give your
reusable answers names, then say which template question each one fills.

  values:                     # your named data — pick any names you like
    contact: contact@example.invalid
    org:     owner/collection

  template-defaults:          # keyed by a template's FORMAL NAME
    "gh:owner/newsletter":
      email: ${contact}       # ${name} points at a value above
      owner: ${org}
    toha-demo:
      title: My Notes         # or an inline literal — no ${…}, no reference

A value reaches a question only because a mapping says so. Two questions with
different ids — in the same or different templates — may point at one value.
A question elsewhere that happens to share an id is untouched.
```

Rules the author needs, stated once:

- **Reference vs literal.** A string that is exactly `${name}` (a value-library
  name in `${ }`) is a *reference*. Every other string, boolean, or list is an
  inline *literal*. To write a literal that would otherwise read as a reference,
  double the leading `$`: `$${contact}` means the literal text `${contact}`.
- **Formal name is the key.** `template-defaults` keys are formal names — the
  same stable identity Toha records for a staged interview: `gh:owner/repo`,
  `gh:owner/repo@ref#sub`, a git URL, an absolute template folder path, or the
  reserved `toha-demo`. Aliases and short names are **not** keys (they are not
  stable identity), so two templates that share a short name stay separate.

### Call sites (unchanged CLI; config drives the difference)

```console
# A — a value fills two differently-named questions in two templates
$ toha apply gh:owner/newsletter ./out     # email question ← contact@example.invalid
$ toha apply gh:owner/support ./out        # contact_address question ← contact@example.invalid

# B — the bundled demo, referenced by its reserved formal name
$ toha apply toha-demo ./out               # title ← My Notes (literal), topic ← ${sample_topic}

# C — resume re-resolves against live config, never frozen
$ toha stage gh:owner/newsletter ./out     # records formal name gh:owner/newsletter
$ toha continue ./out                       # re-runs the library resolver from current config
```

### Crate caller's view

Unchanged. Callers still build `Seed { now, defaults }` and call
`Interview::start` / `replay_with_defaults`. The library and references are a
config-boundary concern; the engine receives the same flat map it does today.

---

## The four required config examples

### 1. One value, two differently-named questions

```yaml
# user config.yml
values:
  contact: contact@example.invalid

template-defaults:
  "gh:owner/newsletter":       # this template's question id is `email`
    email: ${contact}
  "gh:owner/support":          # a DIFFERENT template, question id `contact_address`
    contact_address: ${contact}
```

Resolved outcome:

| Selected template | Question | Default seeded |
| --- | --- | --- |
| `gh:owner/newsletter` | `email` | `contact@example.invalid` |
| `gh:owner/support` | `contact_address` | `contact@example.invalid` |

Reuse comes from two mappings pointing at one value, not from a shared id.

### 2. An unrelated same-id question, no mapping → untouched

```yaml
values:
  handle: octo-org

template-defaults:
  "gh:owner/service":
    owner: ${handle}
# gh:owner/docs ALSO defines an `owner` question, but has NO mapping here.
```

Resolved outcome:

| Selected template | `owner` default |
| --- | --- |
| `gh:owner/service` | `octo-org` (its mapping applied) |
| `gh:owner/docs` | its own `template.yml` default — Toha does not touch it |

Nothing is applied by id. The old `defaults: { owner: octo-org }` would have hit
both; this model cannot.

### 3. The bundled `toha-demo` as a selector

```yaml
values:
  sample_topic: Sample Topic

template-defaults:
  toha-demo:                   # the reserved formal name is the key
    title: My Notes            # inline literal
    topic: ${sample_topic}     # reference
```

Resolved outcome for `toha apply toha-demo ./out`: `title` seeds `My Notes`,
`topic` seeds `Sample Topic`. An installed or aliased `toha-demo` resolves to a
different formal name, so its mappings live under that other key — the bundled
identity stays separate.

### 4. A missing reference and a type mismatch, with exact attribution

```yaml
# user config.yml
values:
  optin: "yes"                 # a STRING value

template-defaults:
  toha-demo:
    subscribe: ${optn}         # typo: no value named `optn`
  "gh:owner/newsletter":
    confirm_optin: ${optin}    # confirm question, but the value is a string
```

Missing-reference error (Stage 1, before the template is even consulted):

```
<user config.yml>: template-defaults."toha-demo".subscribe references value
"optn", which is not defined under values
```

Type-mismatch error (Stage 2, at the question that uses it):

```
<user config.yml>: template-defaults."gh:owner/newsletter".confirm_optin: the
default from values."optin" ("yes") must be true or false
```

Bonus — a cycle in the library is refused with the chain:

```
<user config.yml>: values."a" and values."b" reference each other:
a → b → a
```

---

## Data shapes (parsed once at the boundary)

Wire input is plain YAML; it is parsed into these domain types in `config::load`
and never surfaced raw again (per boundary-discipline).

```rust
// src/config.rs

pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub values: ValueLibrary,               // replaces `defaults`
    pub template_defaults: TemplateMappings, // new
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}

/// A config-author datum: an inline literal or a pointer to another value.
/// The reference/literal distinction is decided ONCE, here, by `Binding::parse`.
#[derive(Debug, Clone)]
pub enum Binding {
    Literal(serde_json::Value), // string | bool | array<string> (answer shapes)
    Ref(String),                // the referenced value name, `${ }` stripped
}

/// Where an entry was written, so any error can name its file.
#[derive(Debug, Clone)]
pub struct Site {
    pub file: PathBuf,
    pub layer: Layer,          // System | User | Local
}

/// One library entry.
#[derive(Debug, Clone)]
pub struct StoredValue { pub binding: Binding, pub site: Site }

/// `values`: config-author name → value. Merged per name across layers,
/// local wins. The single source of truth every reference resolves against.
pub type ValueLibrary = IndexMap<String, StoredValue>;

/// One per-template default.
#[derive(Debug, Clone)]
pub struct Mapping { pub binding: Binding, pub site: Site }

/// `template-defaults`: formal name → (question id → mapping). Merged per
/// (formal, id) across layers, local wins for that one pair.
pub type TemplateMappings = IndexMap<String, IndexMap<Id, Mapping>>;

impl Binding {
    /// A string exactly matching `^\$\{([a-z_][a-z0-9_]*)\}$` becomes `Ref`.
    /// A string whose value, after removing one leading `$`, matches that
    /// grammar becomes a `Literal` of the de-escaped text. Everything else
    /// (other strings, booleans, arrays) is a `Literal` verbatim.
    fn parse(value: serde_json::Value) -> Binding { unimplemented!() }
}
```

Why kind is not declared on a value: an answer kind is exactly one of
{string, bool, list-of-strings}, so a value's JSON shape *is* its kind. A
separate `type:` field would be a second source that can disagree with the value
(per single-source-of-truth). The only check that carries information is
value-kind vs *question*-kind, and that is derived at the use site. This
diverges from the "entries may be typed" hypothesis; see rationale.

```rust
// src/defaults.rs  — the graph resolver (boundary logic, template-unaware)

/// A value that reached a question, with the chain that produced it.
#[derive(Debug, Clone)]
pub struct ResolvedDefault {
    pub value: serde_json::Value,
    /// e.g. `template-defaults."gh:owner/newsletter".email → values."contact"`
    pub origin: String,
    pub site: Site,            // the mapping's file, for a later kind error
}

/// An already-attributed configuration failure. `Display` is `message`.
#[derive(Debug)]
pub struct DefaultsError { pub message: String }
```

---

## Function signatures (the seam)

```rust
// src/defaults.rs — STAGE 1: config author names → flat question ids.
// Pure over config. Knows identity and the store; knows nothing of templates.
pub fn resolve_template_defaults(
    formal: &str,
    library: &ValueLibrary,
    mappings: &TemplateMappings,
) -> Result<IndexMap<Id, ResolvedDefault>, DefaultsError>;

/// Resolve one binding through the library. `stack` holds the value names on
/// the current path for cycle detection; `chain` accumulates the origin text.
fn resolve_binding(
    binding: &Binding,
    library: &ValueLibrary,
    from: &Site,
    stack: &mut Vec<String>,
    chain: &mut String,
) -> Result<serde_json::Value, DefaultsError> {
    // Literal            -> Ok(value)
    // Ref(name):
    //   library.get(name) is None      -> Err("… references value \"{name}\",
    //                                      which is not defined under values")
    //   stack.contains(name)           -> Err(cycle chain "a → b → a")
    //   else push name, recurse, pop
    unimplemented!()
}
```

```rust
// src/interview.rs — STAGE 2: flat ids → engine seed. REPLACES today's
// `configured_defaults(template, &config.defaults)`. Still receives a flat map;
// the engine below it is untouched.
pub fn configured_defaults(
    template: &Template,
    resolved: &IndexMap<Id, ResolvedDefault>,
) -> Result<IndexMap<Id, RawAnswer>, EvalError> {
    // for (id, rd) in resolved:
    //   let Some(q) = question_by_id(&template.interview, id) else {
    //       eprintln!("warning: template-defaults default for \"{id}\" is unused: \
    //                  this template has no question \"{id}\"");
    //       continue;                       // stale/typo mapping is inert, not fatal
    //   };
    //   parse_kind(id, prompt_kind(q), rd.value.clone())        // reuse existing
    //       .map_err(|e| kind_error(id, rd, e.message))?;        // attribute to origin
    //   defaults.insert(id.clone(), RawAnswer(rd.value.clone()));
    unimplemented!()
}
```

`EvalError` gains an attributed rendering for a configured default. The existing
`CONFIGURED_DEFAULT` field and its `configuration key defaults.<id>` text are
replaced by an origin-carrying message (`template-defaults."<formal>".<id>: …`),
reusing the `parse_kind` sentences (`must be true or false`, `must be a string`,
`must be an array of strings`).

Boundary wiring (`src/main.rs`, `seed()` at 414; every call site already has the
formal name in scope — `resolved.formal_name` at 573/820, `saved.template` at
496/648):

```rust
fn seed(formal: &str, template: &Template, config: &Config) -> Result<Seed, String> {
    let now = /* unchanged */;
    let resolved = toha::defaults::resolve_template_defaults(
        formal, &config.values, &config.template_defaults,
    ).map_err(|e| e.to_string())?;
    Ok(Seed { now, defaults: toha::interview::configured_defaults(template, &resolved)
        .map_err(|e| e.to_string())? })
}
```

The staged/`progress`/`continue`/`apply-staged` paths call the same two
functions with `&saved.template`, so defaults re-resolve against live config on
resume and never freeze into the record (`StagedRecord` is unchanged).

---

## Module and seam diagram

```text
config author: values: + template-defaults:   (system / user / local .yml)
        │  parse ONCE (Binding::parse decides ref vs literal)
        ▼
src/config.rs  load()
   ├─ ValueLibrary     (merged per name;        local > user > system)
   └─ TemplateMappings (merged per formal+id;   local > user > system)
        │        formal name in scope at every driver path
        ▼        (resolved.formal_name  |  saved.template)
src/defaults.rs  resolve_template_defaults(formal, library, mappings)   ── STAGE 1
        │  graph walk: ${ref}→value→…; refuse cycle; report missing; carry origin
        ▼
   IndexMap<Id, ResolvedDefault>          (flat, keyed by question id)
        │
        ▼
src/interview.rs  configured_defaults(template, resolved)               ── STAGE 2
        │  keep defined ids (warn on unknown); parse_kind vs question kind
        ▼
   IndexMap<Id, RawAnswer>                = Seed.defaults  (UNCHANGED contract)
        │
        ▼   ───────────────── UNCHANGED FROM HERE DOWN ─────────────────
   Interview::start / replay_with_defaults   (engine: identity- and store-unaware)
```

Three files from a formal name to `Seed.defaults`; the engine below never learns
which template it is (per minimize-reader-load / short call chains).

---

## Precedence rule (total, deterministic)

Both structures merge in `config::load` in the fixed order **system → user →
local**, extending an `IndexMap` so iteration order is deterministic.

- **`values` (library).** Per name; a later layer replaces an earlier layer's
  entry with that name (local wins), matching today's `defaults`/`hosts` merge
  (`src/config.rs:151-168,176-180`). Available in system, user, and local.
- **`template-defaults` (mappings).** Two-level per key: for each formal name,
  merge the question-id map; a later layer replaces an earlier layer's entry for
  that *(formal, id)* pair only, keeping the other questions' mappings. Available
  in system, user, and local.
- **Interaction.** Every reference resolves against the **fully merged** library,
  never a layer-local slice — so a local mapping may reference a user value. One
  library, one source of truth.
- **Identity stability under local.** Keys are formal names, and local config
  cannot set `hosts` or `local-config-name` (`config.yml`,
  `configuration.md:31-32,93-95`), so a key like `gh:owner/x` denotes the same
  source in every directory. Keying mappings in local config is therefore safe;
  it inherits the identity-stability guarantee rather than breaking it.

---

## Error / results contract

| Situation | Stage | Result |
| --- | --- | --- |
| Literal, or ref that resolves | 1 | value carried with `origin` |
| Ref to a name not in `values` | 1 | `DefaultsError` — `<file>: template-defaults."<formal>".<id> references value "<name>", which is not defined under values` |
| Ref nested in a value, target missing | 1 | `<file>: values."<a>" references value "<b>", which is not defined under values` |
| Cycle among values (`a→b→a`) | 1 | `<file>: values."a" and values."b" reference each other: a → b → a` |
| Mapping id the template does not define | 2 | non-fatal warning to stderr; that mapping is inert (keeps resume across versions working) |
| Resolved value's kind ≠ question kind | 2 | `EvalError` — `<file>: template-defaults."<formal>".<id>: the default from values."<name>" (<value>) must be <true or false \| a string \| an array of strings>` (a literal drops the `from values."<name>"` clause) |
| No mapping for the selected formal name | 1 | empty map; every question keeps its own `template.yml` default |
| Legacy `defaults:` present | 0 | `ConfigError` at load, with conversion guidance (see Migration) |

Only the value that actually reaches a question is kind-checked (Stage 2 walks
`resolved`, which contains solely used entries). Exit codes are the existing
pipeline's; no new code, no new trust/permission/timeout/subprocess surface.

---

## Behaviors to prove (falsifiable)

- **Two questions, one value** — mapping `email` and `contact_address` in two
  formal names to `${contact}`; both seed `contact@example.invalid`. Fails if a
  value cannot fan out to differently-named questions.
- **No implicit id application** — two templates define `owner`; only one is
  mapped; the unmapped template's `owner` keeps its `template.yml` default.
  Fails if any id-based application survives.
- **Reference vs literal** — `${contact}` resolves through the library;
  `$${contact}` seeds the literal `${contact}`; a mistyped `${a b}` (not the
  grammar) seeds verbatim. Fails on either misread.
- **Missing ref attributed** — `${optn}` yields the exact Stage-1 message naming
  file, formal name, question, and the undefined value; never a silent empty
  default.
- **Cycle refused** — `values: {a: ${b}, b: ${a}}` used by any mapping yields the
  cycle message with the chain; resolution terminates (bounded by `stack`).
- **Type mismatch attributed** — a string value under a confirm question yields
  the exact Stage-2 message; a value used by no question is never kind-checked.
- **Formal-name selection** — mappings under `gh:owner/x` apply to that formal
  name only; a folder-resolved or aliased same-short-name template does not pick
  them up.
- **Bundled demo** — mappings under `toha-demo` reach the bundled demo's
  questions; an installed `toha-demo` (different formal name) does not.
- **Resume re-resolves** — stage, then change a referenced value, then continue;
  the new value appears (defaults not frozen). Same output across terminal,
  headless, staged, direct, and crate paths.
- **Layer precedence** — local overrides one `(formal, id)` mapping and one
  library name while keeping the other layers' siblings; a local mapping
  resolves a user-defined value.

---

## Proposed schema and doc edits (described, not applied)

`docs/specifications/config.schema.yml`:

- Remove `defaults` from `shared-config`/`local-config` `properties` (its
  presence is caught at load for a tailored message; see Migration).
- Add `$defs`:
  - `value-name: { type: string, pattern: '^[a-z_][a-z0-9_]*$' }`
  - `binding`: `oneOf: [ {type: string}, {type: boolean},
    {type: array, items: {type: string}} ]` (a string is later classified as ref
    or literal; the schema does not encode the sigil).
  - `values: { type: object, propertyNames: {$ref: '#/$defs/value-name'},
    additionalProperties: {$ref: '#/$defs/binding'} }`
  - `template-default-map: { type: object,
    propertyNames: {$ref: '#/$defs/identifier'},
    additionalProperties: {$ref: '#/$defs/binding'} }`
  - `template-defaults: { type: object, additionalProperties:
    {$ref: '#/$defs/template-default-map'} }` (formal-name keys are unconstrained
    strings — they include `:/@#.` and paths).
- Add `values` and `template-defaults` to `shared-config` and `local-config`.

`docs/configuration.md`: replace "Set default answers" with two sections — the
value library and template-specific defaults — covering the `${name}` form, the
`$$` escape, formal-name keys, missing/cycle/type errors, and the precedence
rule above. `docs/specifications/template-interviews.md`: note that a configured
default overrides a question's own default exactly as before, now sourced from a
mapping rather than a global id.

---

## Migration disposition (required product decision)

Existing configs carry `defaults:` (global-by-id), the very coupling being
removed. Options:

- **A — Deprecation window.** Keep `defaults:` working (implicit global-by-id)
  for N releases with a warning, beside the new model. *Cost:* keeps the rejected
  coupling live and forces two resolution paths and two doc stories at once.
- **B — Auto-translate.** Read each `defaults: {id: v}` and… there is no template
  identity to translate *to*. The only mechanical move is `values: {id: v}` with
  **no** mapping, which silently stops the value from applying — a silent
  behaviour change even though no data is dropped. Rejected.
- **C — Reject with conversion guidance (recommended).** If any layer contains
  `defaults:`, `config::load` fails with an attributed message that shows the
  exact `values:` + `template-defaults:` rewrite. No value is dropped or silently
  repurposed; intent is made explicit once. Pre-v1 (no external users, small
  blast radius), a single clear error is the honest cost.

Recommended message:

```
<file>: `defaults` is no longer supported. It applied a value to EVERY template
that asked a question with that id; that implicit behaviour is removed. Move each
value under `values`, then map it to a template's question under
`template-defaults`. For example:

  defaults:
    title: Untitled

  becomes

  values:
    title: Untitled
  template-defaults:
    "gh:owner/collection":     # the template(s) that should get it
      title: ${title}
```

Recommendation: **C**. It is the only option with neither data loss nor a silent
behaviour change, and it matches the reframe's intent — no value moves without a
stated target.

---

## Out of scope

The pure interview engine and its `Seed.defaults` contract; `Template`, `Plan`,
`apply`, `protocol`, `staging`, and `StagedRecord`; `ResolvedTemplate`'s shape
and the `<TEMPLATE>` classification order; the trust model; registry entries
(defaults stay a config-side user preference, never written to `templates.yml`).
No string interpolation (references are whole-value only, preserving non-string
kinds); no references inside list elements; no numeric coercion (quote a number
meant for a text question, as today). No production code, schema, or doc edits in
this task — only the sketches above.
