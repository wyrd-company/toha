# Template-specific configured defaults — explicit binding list

Reuse comes from a small **named store of literal values** and an **ordered
list of explicit bindings**. Each binding names a template *identity*, a
question, and a source that is **either** a stored value **or** an inline
literal — placed in two distinct keys so a literal that spells a value name is
never misread. Nothing is applied by matching question ids.

The pure interview engine is untouched. Resolution flattens, at the CLI
boundary, to the exact map the engine already consumes:
`Seed.defaults: IndexMap<Id, RawAnswer>` (`src/interview.rs:39`), keyed by the
selected template's question ids. `configured_defaults`
(`src/interview.rs:677`) keeps its signature and behavior byte-for-byte; it
stays identity- and store-unaware.

## Caller's usage (the spec)

```text
A toha config file has two independent parts for answers:

  values:            a store of named, reusable literals YOU name
  template-defaults: an ordered list of bindings, each pointing ONE question
                     of ONE selected template at a stored value or a literal

A value reaches a question only because a binding says so. Two questions with
DIFFERENT ids — in the same or different templates — may point at the SAME
stored value. A question is never touched just because its id equals another
template's question id.

  values:
    primary_contact: contact@example.invalid

  template-defaults:
    - template: gh:owner/newsletter    # a formal template identity
      question: email                  # a question that template asks
      value: primary_contact           # reference the stored value by name
    - template: toha-demo
      question: topic
      literal: Sample Topic            # an inline literal (distinct key)

Select a template by its formal identity: a git address (gh:owner/collection),
an absolute folder path, an installed alias or short name that resolves to one
identity, or the reserved bundled name `toha-demo`. Ambiguous short names are
refused for THIS run with a message telling you to use the formal name.
```

Concrete call sites (unchanged CLI surface; only config content is new):

```console
# A — a stored value reaches two differently-named questions
$ toha apply gh:owner/newsletter ./out     # email default  = contact@example.invalid
$ toha apply gh:owner/press-kit  ./out      # contact default = contact@example.invalid

# B — the bundled demo, offline, first run, defaults from config
$ toha apply toha-demo ./out                # title/topic defaults from config

# C — a missing value reference is a clear, attributed error (exit 1)
$ toha apply toha-demo ./out
error: ~/.config/toha/config.yml: template-defaults #1 for template
  "toha-demo", question "title": no value named "licence" is defined under
  `values`
```

Crate callers of the `toha` library see **no new surface**. They still build
`Seed { now, defaults }` and call `Interview::start` /
`replay_with_defaults` (`src/staging.rs:131`). The store-and-bindings feature
is a binary/CLI concern — like the bundled demo, it lives in `src/cli/` and
depends on registry/selector resolution, which is a CLI responsibility.

## The four required examples

### 1 — one stored value, two differently-named questions

```yaml
values:
  primary_contact: contact@example.invalid

template-defaults:
  - template: gh:owner/newsletter    # this template asks `email`
    question: email
    value: primary_contact
  - template: gh:owner/press-kit     # this template asks `contact`
    question: contact
    value: primary_contact
```

Resolved outcome:

- `toha apply gh:owner/newsletter ./out` → the `email` question's default is
  `contact@example.invalid`.
- `toha apply gh:owner/press-kit ./out` → the `contact` question's default is
  `contact@example.invalid`.

One value reaches two questions with different ids because each binding points
at it. There is no shared `email`/`contact` id and no implicit coupling.

### 2 — a shared id with no binding stays untouched

```yaml
values:
  house_owner: owner/collection

template-defaults:
  - template: gh:owner/press-kit
    question: owner              # press-kit's `owner` question
    value: house_owner
# gh:owner/blog ALSO asks a question with id `owner`, but no binding names it.
```

Resolved outcome:

- `toha apply gh:owner/press-kit ./out` → `owner` default is
  `owner/collection`.
- `toha apply gh:owner/blog ./out` → `owner` is **untouched**; the template's
  own default (or empty) stands.

Sharing the id `owner` does not carry the value to `blog`. Only a binding that
names `blog`'s identity would.

### 3 — the bundled `toha-demo` as a selector

```yaml
values:
  demo_title: Sample Title

template-defaults:
  - template: toha-demo           # the reserved bundled identity
    question: title
    value: demo_title
  - template: toha-demo
    question: topic
    literal: Sample Topic         # inline literal — distinguished by the key
```

Resolved outcome:

- `toha apply toha-demo ./out` → `title` default `Sample Title`, `topic`
  default `Sample Topic`.

`toha-demo` resolves as `Address::Name` (`src/source.rs:115`). With no registry
entry it is `NotFound`; the raw token is then used as the candidate formal name,
which equals the bundled demo's `formal_name = "toha-demo"` (predecessor
`f949b6a`). An installed/aliased/discovered `toha-demo` would resolve first, and
the binding would apply to *that* identity — consistent with the bundled demo's
fallback-only precedence.

### 4 — a missing reference and a type mismatch

```yaml
values:
  license: MIT

template-defaults:
  - template: toha-demo
    question: title
    value: licence               # typo: no such value name
  - template: toha-demo
    question: include_summary    # a Confirm (boolean) question
    literal: yes-please          # wrong kind: a string, not a boolean
```

Missing-reference error (exact text):

```
~/.config/toha/config.yml: template-defaults #1 for template "toha-demo",
question "title": no value named "licence" is defined under `values`
```

Type-mismatch error (exact text):

```
~/.config/toha/config.yml: template-defaults #2 for template "toha-demo",
question "include_summary": literal "yes-please" is not a valid answer: must
be true or false
```

The tail `must be true or false` is the engine's own `parse_kind` message for a
`Confirm` question (`src/interview.rs:513`); the CLI wraps it with the binding
site. A **reference** type mismatch names the value instead of the literal:
`value "license" ("MIT") is not a valid answer: <message>`. Only the value
actually used is validated — an unmatched or unreferenced value is never
kind-checked.

## Data shapes

Domain types parsed once at the config boundary (`toha::config`). `defaults` is
**removed**; two independent properties replace it.

```rust
// toha::config
pub type ValueName = String;   // matches ^[a-z_][a-z0-9_]*$

#[derive(Debug, Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub values: IndexMap<ValueName, Value>,   // NEW — merged named store
    pub template_defaults: Vec<Binding>,      // NEW — merged, ordered bindings
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
    // `pub defaults: IndexMap<Id, Value>` (src/config.rs:26) is REMOVED
}

/// One row of the binding table. Reference vs literal is encoded in the type,
/// which mirrors "which key was present" in YAML — never a string shape.
#[derive(Debug, Clone)]
pub struct Binding {
    pub selector: String,   // raw <TEMPLATE> selector, exactly as written
    pub question: Id,       // question id in the selected template
    pub source: Source,
    pub origin: Origin,     // file + list index, for attribution
}

#[derive(Debug, Clone)]
pub enum Source {
    Ref(ValueName),   // came from `value: <name>`
    Literal(Value),   // came from `literal: <any yaml/json value>`
}

/// The exact config site a binding came from, for attributed errors
/// (preserves "error attribution that names the config site").
#[derive(Debug, Clone)]
pub struct Origin {
    pub file: PathBuf,
    pub index: usize,   // 1-based position within that file's template-defaults
}
```

The wire types stay at the boundary and are parsed into the domain types above
(`per boundary-discipline`):

```rust
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    values: Option<IndexMap<String, Value>>,
    template_defaults: Option<Vec<RawBinding>>,
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
    // presence of the removed key drives the migration message (see below)
    defaults: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawBinding {
    template: String,
    question: String,
    value: Option<String>,     // exactly one of value / literal, enforced by
    literal: Option<Value>,    // schema `oneOf` and re-checked on conversion
}
```

The engine seam is unchanged:

```rust
// toha::interview — UNCHANGED signature and behavior
pub fn configured_defaults(
    template: &Template,
    values: &IndexMap<Id, Value>,
) -> Result<IndexMap<Id, RawAnswer>, EvalError>;

// toha::interview::Seed — UNCHANGED
pub struct Seed { pub now: jiff::Zoned, pub defaults: IndexMap<Id, RawAnswer> }
```

## Function signatures (new CLI module)

New file `src/cli/defaults.rs` — the only store- and identity-aware code, a
sibling of `src/cli/bundled.rs`. Selector resolution (registry + hosts) lives
here so the library engine learns nothing.

```rust
use crate::{cli::resolve::ResolvedTemplate, Dirs};
use indexmap::IndexMap;
use serde_json::Value;
use std::{fmt, path::Path};
use toha::{
    config::{Binding, Config, Origin, Source},
    interview::{configured_defaults, EvalError, RawAnswer},
    registry::Registry,
    source::{self, Address},
    template::Template,
    Id,
};

/// The configured defaults for the selected template: the flat, engine-ready
/// map the pure interview seeds from. Bindings whose selector resolves to
/// `resolved.formal_name` supply values (a stored value or an inline literal);
/// nothing is applied by id. This replaces every current
/// `configured_defaults(&template, &config.defaults)` call site.
pub fn for_template(
    config: &Config,
    resolved: &ResolvedTemplate,
    template: &Template,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<IndexMap<Id, RawAnswer>, DefaultsError> {
    // 1. project bindings that name this identity into raw values by question
    let projected = project(
        config,
        template,
        |b| match_selector(b, &resolved.formal_name, config, registry, dirs, cwd),
    )?; // IndexMap<Id, (Value, &Binding)>

    // 2. hand the flat map to the UNCHANGED engine validator, then re-attribute
    //    a kind error back to the binding that produced it.
    let values: IndexMap<Id, Value> =
        projected.iter().map(|(id, (v, _))| (id.clone(), v.clone())).collect();
    configured_defaults(template, &values).map_err(|e: EvalError| {
        let (_, binding) = &projected[&e.id];
        DefaultsError::TypeMismatch { binding: BindingRef::of(binding), message: e.message }
    })
}

/// Whether a binding's selector names the selected identity. Pure decision
/// over registry + hosts; performs no fetch.
enum Match { Applies, Skip, Ambiguous(Vec<String>) }

fn match_selector(
    binding: &Binding,
    selected: &str,                 // resolved.formal_name — the stable identity
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Match {
    // Fast path: the selector is already the formal name.
    if binding.selector == selected { return Match::Applies; }
    match source::parse(&binding.selector, &config.hosts, cwd, &dirs.home) {
        // git / host forms normalize purely, no I/O (src/source.rs:118)
        Ok(Address::Git { .. }) => bool_match(
            source::parse(&binding.selector, &config.hosts, cwd, &dirs.home)
                .map(|a| a.formal_name(&config.hosts)),
            selected,
        ),
        // a folder that no longer exists cannot be the running template → skip
        Ok(Address::Folder(p)) => bool_match(Ok(p.to_string_lossy().into_owned()), selected),
        Ok(Address::Name(n)) => match registry.resolve(&n) {
            Ok(r) if r.formal_name == selected => Match::Applies,
            Ok(_) => Match::Skip,
            // NotFound → use the bare token as the formal name (matches the
            // reserved `toha-demo` and any not-installed formal-name token)
            Err(toha::registry::ResolveError::NotFound(_)) if n == selected => Match::Applies,
            Err(toha::registry::ResolveError::NotFound(_)) => Match::Skip,
            // ambiguity is only fatal when it could have meant THIS run
            Err(toha::registry::ResolveError::Ambiguous { matches, .. })
                if matches.iter().any(|m| m == selected) => Match::Ambiguous(matches),
            Err(toha::registry::ResolveError::Ambiguous { .. }) => Match::Skip,
        },
        // a folder selector that fails to canonicalize can't be the running
        // folder template → skip, never break an unrelated run
        Err(_) => Match::Skip,
    }
}

fn bool_match(formal: Result<String, source::SourceError>, selected: &str) -> Match {
    match formal { Ok(f) if f == selected => Match::Applies, _ => Match::Skip }
}

/// Pure projection: walk the merged bindings in order; for each that names the
/// selected identity, resolve its source and record it by question id. Later
/// bindings override earlier ones for the same question (layer + intra-layer
/// order precedence). Errors before the engine sees anything: missing ref,
/// ambiguous selector, unknown question.
fn project<'a>(
    config: &'a Config,
    template: &Template,
    matches: impl Fn(&Binding) -> Match,
) -> Result<IndexMap<Id, (Value, &'a Binding)>, DefaultsError> {
    let mut out: IndexMap<Id, (Value, &Binding)> = IndexMap::new();
    for binding in &config.template_defaults {
        match matches(binding) {
            Match::Skip => continue,
            Match::Ambiguous(matches) => {
                return Err(DefaultsError::AmbiguousSelector {
                    binding: BindingRef::of(binding), matches,
                });
            }
            Match::Applies => {}
        }
        if !template.has_question_id(&binding.question) {
            return Err(DefaultsError::UnknownQuestion { binding: BindingRef::of(binding) });
        }
        let value = match &binding.source {
            Source::Literal(v) => v.clone(),
            Source::Ref(name) => config.values.get(name).cloned().ok_or_else(|| {
                DefaultsError::MissingValue { binding: BindingRef::of(binding), name: name.clone() }
            })?,
        };
        out.insert(binding.question.clone(), (value, binding)); // last wins
    }
    Ok(out)
}

/// Attributed failures. Each names the file, the 1-based list index, the
/// selector, and the question — the config site.
#[derive(Debug)]
pub enum DefaultsError {
    MissingValue     { binding: BindingRef, name: String },
    TypeMismatch     { binding: BindingRef, message: String },
    UnknownQuestion  { binding: BindingRef },
    AmbiguousSelector{ binding: BindingRef, matches: Vec<String> },
}

/// The renderable coordinates of a binding.
#[derive(Debug)]
pub struct BindingRef {
    file: std::path::PathBuf,
    index: usize,
    selector: String,
    question: Id,
    source: SourceDesc,
}
#[derive(Debug)]
enum SourceDesc { Literal(Value), Ref { name: String, value: Value } }

impl BindingRef {
    fn of(_b: &Binding) -> Self { unimplemented!("copy origin/selector/question/source") }
}

impl fmt::Display for DefaultsError { /* exact texts from the examples */ }
```

Bodies are `unimplemented!()` here; the shapes and the flow are the contract.

## Module and seam map

```text
config.yml
  values: { name: literal }
  template-defaults: [ {template, question, value|literal}, ... ]
        │  toha::config::load  — boundary parse + LAYER MERGE only
        │     • per-name merge of `values` (local > user > system)
        │     • concat of `template-defaults` (system → user → local, file order)
        │     • RawBinding → Binding (Id parse, exactly-one-of, Origin)
        │     • presence of removed `defaults:` → migration error (before schema)
        ▼
  Config { values, template_defaults, hosts, ... }         (toha library)
        │
   [per driver path, AFTER the template + registry are resolved]
        ▼
  src/cli/defaults.rs  ── the only store/identity-aware code ──
     for_template(config, resolved, template, registry, dirs, cwd)
        • match each binding's selector to resolved.formal_name (no fetch)
        • resolve Source: Ref → values store | Literal → inline
        • last-wins per question id
        • IndexMap<Id, Value>                          (same shape as old defaults)
        ▼
  toha::interview::configured_defaults(&template, &values)   ── UNCHANGED ──
        • validates each value against the question's kind
        • IndexMap<Id, RawAnswer>
        ▼
  Seed { now, defaults }  →  Interview::start / replay_with_defaults   ── UNCHANGED ──
```

Trace: `config.rs` → `cli/defaults.rs` → `interview.rs`. Three files, one new.
Everything from `Seed` down is byte-for-byte the existing pipeline; the engine
never learns an identity, a selector, or a store.

### Wiring (the whole blast radius outside the new module)

```rust
// src/main.rs — seed() gains the resolution context it needs to project
fn seed(
    template: &Template,
    resolved: &ResolvedTemplate,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<Seed, String> {
    let now = /* unchanged */;
    Ok(Seed {
        now,
        defaults: cli::defaults::for_template(config, resolved, template, registry, dirs, cwd)
            .map_err(|e| e.to_string())?,
    })
}

// src/main.rs:496, 664, 902 — the three staged paths: replace
//   toha::interview::configured_defaults(&template, &config.defaults)
// with
//   cli::defaults::for_template(&config, &resolved, &template, &registry, dirs, &cwd)
// (resolved, registry, config, dirs, cwd are already in scope at each site)
```

## Precedence rule (total, deterministic)

Two things merge, independently, then interact only through references.

1. **The store `values`** — per name: local > user > system. A higher layer's
   `name` overrides a lower layer's `name`; other names are kept. This is the
   existing per-key merge used for `defaults`/`hosts` today
   (`src/config.rs:151-168`, `configuration.md:98-102`).
2. **The bindings `template-defaults`** — concatenated in layer order
   **system → user → local**, preserving each file's list order, each tagged
   with its `Origin`. No override happens at merge time (a selector's identity
   is not known until it is resolved at a call site).
3. **At projection**, for the one selected identity, bindings are applied in
   that concatenated order; a later matching binding **overrides** an earlier
   one for the same question id. Therefore, per question:
   **local file order last wins > user > system**, and within a file a lower
   entry wins.
4. **References cross layers freely.** A binding in any layer may reference a
   value defined in any layer, because references resolve against the fully
   merged store computed in step 1.

`values` and `template-defaults` are available in both `shared-config`
(system/user) and `local-config`, exactly as `defaults` was
(`config.schema.yml:40-51`). `hosts` remains system/user only, so selector
resolution is stable across directories (a formal name resolves the same way
everywhere — `configuration.md:94-95`).

## Typing, missing refs, cycles

- **Missing reference** → `DefaultsError::MissingValue`, before the engine
  runs. Never a silent empty default. Attributed to the file, index, selector,
  and question (example 4).
- **Type mismatch** → caught by the unchanged engine `configured_defaults`
  (`parse_kind`, `src/interview.rs:504`) and re-attributed to the binding as
  `DefaultsError::TypeMismatch` (example 4). Literals name the literal;
  references name the value.
- **Cycles are structurally impossible.** The store maps names to **literals**;
  a stored value never references another stored value (`Source::Ref` points
  into `Config.values: IndexMap<ValueName, Value>`, not into another `Source`).
  Reference resolution is exactly one hop. No cycle detection exists because no
  cycle can exist. This is a deliberate, load-bearing simplification.
- **Validate only what is used.** Projection filters to matched bindings first,
  resolves only their sources, and kind-checks only those values. A
  type-broken literal or a dangling reference in a binding for another template
  never affects an unrelated run; an unreferenced store value is never checked.

## Selectors and identity

The stable identity is `ResolvedTemplate.formal_name` (`src/cli/resolve.rs:17`),
already in scope at every call site. A binding's `template:` selector is
matched to it, never by short name alone:

| Selector form | Matched to `formal_name` by | Evidence |
| --- | --- | --- |
| Git / host address (`gh:owner/collection`) | `Address::formal_name` (pure) | `src/source.rs:118` |
| Absolute folder path | canonical path string | `src/cli/resolve.rs:184` |
| Installed alias / short / formal name | `registry.resolve` (alias→short→formal) | `src/registry.rs:471` |
| Duplicate short name | **refused** for this run (use formal) | `src/registry.rs:485` |
| Bundled `toha-demo` | `Name` NotFound → bare token as formal | predecessor `f949b6a` |

Two templates that share a short name and even a question title stay separate:
each has a distinct `formal_name`, and a binding names exactly one. A
short-name selector that is ambiguous is refused **only when the ambiguity set
contains the running identity** (it plausibly meant this run); otherwise it is
an irrelevant binding and is skipped — an unrelated run never breaks. A `Name`
selector that is `NotFound` and is not the running identity is skipped (an
irrelevant or typo'd binding), never applied by accident.

## Error / results contract

| Situation | Result |
| --- | --- |
| Binding names this identity, `value:` present in store | value seeds the question |
| Binding names this identity, `literal:` present | literal seeds the question |
| `value:` names a missing store entry | `DefaultsError::MissingValue`, exit 1 |
| Value/literal wrong kind for the question | `DefaultsError::TypeMismatch`, exit 1 |
| Binding names a question the template lacks | `DefaultsError::UnknownQuestion`, exit 1 |
| Selector ambiguous and includes this identity | `DefaultsError::AmbiguousSelector`, exit 1 |
| Selector names another identity / not found / gone | binding skipped (no error) |
| No binding names a question | template's own default (or empty) stands |
| `defaults:` key present in any config file | migration error, exit 1 (see below) |

Exit codes are the existing pipeline's; no new code, no new exit value. The
engine's own faults (template `default`, `when`, …) are unchanged.

## Behaviors to prove (falsifiable)

- **Reuse across differently-named questions** — example 1: one store value
  seeds `email` in one template and `contact` in another; both defaults equal
  the stored value. Fails if a value can only reach a same-id question.
- **No implicit by-id** — example 2: `blog`'s `owner` is byte-identical to a
  no-config run; only `press-kit`'s `owner` changes. Fails if a shared id
  leaks a value.
- **Key-disambiguated source** — a store value literally named `primary` and a
  binding `literal: primary` both present: the literal seeds the question with
  the string `primary`, never the store entry. Fails if source is inferred from
  string shape instead of the `value:`/`literal:` key.
- **Missing ref is loud** — example 4 first binding yields exactly the
  `MissingValue` text; the interview never starts and no default is silently
  empty.
- **Type mismatch is attributed** — example 4 second binding yields exactly the
  `TypeMismatch` text with the engine's kind message as its tail.
- **Only-used validation** — a config with a type-broken literal in a binding
  for template X runs template Y with no error. Fails if all bindings are
  validated eagerly.
- **Precedence** — a system, a user, and a local binding for the same
  (identity, question): the local value wins; within one file, the lower of two
  wins. A local binding referencing a system-defined value resolves.
- **Re-resolution on resume** — stage under one config, change a referenced
  store value, `continue`/`apply`: the new value seeds the question (defaults
  re-derived from live config, never frozen — `src/main.rs:496,664,902`).
- **Identical across drivers** — terminal, headless, staged, direct, and crate
  paths produce the same seeded defaults for the same inputs.
- **No `defaults:` acceptance** — a config carrying `defaults:` produces the
  migration error, never silent application.

## Proposed schema edits (design only — not applied)

`docs/specifications/config.schema.yml`:

```yaml
$defs:
  value-name:
    type: string
    pattern: '^[a-z_][a-z0-9_]*$'
  values:
    type: object
    propertyNames: { $ref: "#/$defs/value-name" }
  template-defaults:
    type: array
    items:
      type: object
      additionalProperties: false
      required: [template, question]
      properties:
        template: { type: string, minLength: 1 }
        question: { $ref: "#/$defs/identifier" }
        value:    { $ref: "#/$defs/value-name" }
        literal:  true            # any JSON value
      oneOf:                      # exactly one source key
        - { required: [value],   not: { required: [literal] } }
        - { required: [literal], not: { required: [value] } }
  shared-config:
    properties:                   # replace `defaults` with the two below
      values:            { $ref: "#/$defs/values" }
      template-defaults: { $ref: "#/$defs/template-defaults" }
  local-config:
    properties:
      values:            { $ref: "#/$defs/values" }
      template-defaults: { $ref: "#/$defs/template-defaults" }
```

The old `defaults` `$def` and property are removed. Because
`additionalProperties: false` would reject a stray `defaults:` with a generic
message, `config::load` checks for the `defaults` key **before** schema
validation and emits the friendly migration error instead.

## Documentation edits (design only)

- `docs/configuration.md`: replace "Set default answers" with "Reusable values
  and template defaults" — the `values` store, the `template-defaults` binding
  list, selector = formal identity, reference (`value:`) vs literal
  (`literal:`) by key, the precedence rule, and the migration note. Remove the
  "Defaults apply by id to every template" paragraph
  (`configuration.md:79-80`) — that behavior is gone.
- `docs/specifications/template-interviews.md`: note that a configured default
  now reaches a question only via an explicit binding to the template's formal
  identity, and that it still overrides the template's own `default` and re-
  resolves against live config on resume.

## Migration disposition (required product decision)

Existing configs may carry `defaults: { id: value }` (global-by-id). That model
is removed. No disposition may silently drop those values or silently change
behavior.

- **Option 1 — Reject with conversion (recommended).** Presence of `defaults:`
  is a hard error naming the file and giving the exact conversion: move each
  `defaults.<id>: <v>` to `values.<name>: <v>`, then add a `template-defaults`
  binding per template+question that should receive it. Loudest; forces the
  now-required explicit intent; no data loss (nothing runs until converted; the
  values are named back to the author in the message).

  ```
  ~/.config/toha/config.yml: `defaults:` is no longer supported — a default no
  longer applies to every template that shares a question id. Move each entry
  under `values:` and bind it explicitly, e.g.
      values:
        title: Untitled
      template-defaults:
        - template: <formal name of the template that asks `title`>
          question: title
          value: title
  ```

- **Option 2 — Adopt as `values` + warn (softer).** Re-read `defaults:` as
  entries of the `values` store and warn that they now apply to **nothing**
  until an explicit binding references them. Preserves the data and forbids
  implicit application, but is a silent *behavior* regression for anyone who
  relied on by-id application (the default stops appearing). Needs a collision
  rule if both `defaults:` and `values:` set the same name (recommend: error on
  collision).

- **Option 3 — Deprecation window (rejected).** Keep `defaults:` working with
  the old by-id semantics for N releases behind a warning. Rejected: it keeps
  the exact coupling the product owner removed alive, and forces a messy dual
  precedence between old `defaults` and new bindings.

- **Auto-translate to bindings (infeasible).** The old semantics targeted
  *every* template; there is no identity to translate into without inventing a
  wildcard selector, which re-introduces implicit by-id. Not offered.

**Recommendation: Option 1.** A loud error beats a silent behavior change, and
per "ceremony scales with blast radius" a hard cutover is cheap while this
capability has essentially no installed base (the second release is landing
now). If real 0.1 users of `defaults:` are confirmed, fall back to Option 2 so
their data is auto-carried into `values:` while application stays explicit.

## Out of scope

The pure interview engine and its `configured_defaults`/`Seed` contract, the
`ResolvedTemplate` shape, the `<TEMPLATE>` classification order, staging record
fields, the trust model, and the crate library surface. No value→value
references (and so no cycle machinery). No new subcommand, trust, permission,
timeout, pinned-version check, or subprocess. No production, schema, or doc
edits are applied in this design task.
