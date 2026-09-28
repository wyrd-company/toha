# Template-specific configured defaults — candidate 1 (identity-scoped overlay)

Scope a default answer to a template **identity** (its stable `formal_name`), so
two templates that both ask `title` can receive different defaults, while the
existing global-by-id `defaults` stay as the base layer. All identity resolution
happens at the config/resolve boundary and flattens into the same
`IndexMap<Id, RawAnswer>` the pure interview engine already consumes. The engine
learns nothing new.

Base shape: an **identity-scoped overlay**. Global `defaults` is the base;
`templates.<formal_name>.defaults` overlays it for exactly one identity.

---

## Caller's usage (the spec)

### Config file (the whole new surface a user sees)

```yaml
# ~/.config/toha/config.yml  (user layer)

# Global-by-id defaults — UNCHANGED. Apply to every template that asks the id.
defaults:
  title: Untitled
  include_summary: false

# NEW: identity-scoped overlays. Each key is a template's formal name.
# For that one template only, these ids override the global default above.
templates:
  "gh:owner/collection":          # installed-by-name / git formal name
    defaults:
      title: Team Notes
  "toha-demo":                    # the reserved bundled-demo identity
    defaults:
      title: Demo Title
  "/home/me/work/note-template":  # a folder template's canonical path identity
    defaults:
      title: Work Note
      include_summary: true
```

Reading rule the user internalizes, in one sentence: **a template-specific
default always beats a global default of the same id; within each of those two
tiers, a higher config layer (local > user > system) wins.**

### Call site A — two templates, same id, different default (the core win)

```console
# Both templates ask `title`. Distinct formal names → distinct overlays.
$ toha apply gh:owner/collection ./a --answers empty.json
create note.txt          # title defaulted to "Team Notes"

$ toha apply /home/me/work/note-template ./b --answers empty.json
create note.txt          # title defaulted to "Work Note"

# A third template with no overlay falls back to the global default:
$ toha apply gh:someone/other ./c --answers empty.json
create note.txt          # title defaulted to "Untitled" (global-by-id, preserved)
```

### Call site B — specificity beats layer, re-resolved on resume

```console
# user config: templates."gh:owner/collection".defaults.title = "Team Notes"
$ toha stage gh:owner/collection ./out          # interview begins; title default "Team Notes"

# ...edit user config: change the overlay to title = "Q4 Notes"...
$ toha continue ./out                           # default is now "Q4 Notes"
                                                # (defaults re-resolved from live config,
                                                #  never frozen into the staged record)
```

### Call site C — crate caller opting into scoping

```rust
use toha::interview::{resolved_defaults, Seed};

// The caller already resolved identity to a formal name (e.g. via cli::resolve
// or their own registry). They pass it plus the two config maps.
let scoped = config.template_defaults.get(&formal_name);       // Option<&IndexMap<Id, Value>>
let defaults = resolved_defaults(&template, &formal_name, &config.defaults, scoped)?;
let seed = Seed { now: jiff::Zoned::now(), defaults };
let interview = toha::interview::Interview::start(&template, seed)?;
```

A global-only crate caller keeps calling the existing `configured_defaults`
untouched; scoping is opt-in.

---

## Data shapes

### Config (parsed domain type, `src/config.rs`)

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub defaults: IndexMap<Id, Value>,                                 // UNCHANGED: global-by-id base
    pub template_defaults: IndexMap<String, IndexMap<Id, Value>>,      // NEW: formal_name -> (id -> value)
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
```

Invariants encoded in the type:
- Outer key is a **formal name** (`String`) — the one selector proven unambiguous
  across folders, git addresses, installed names, aliases, duplicate short names,
  and the bundled `toha-demo` identity (grounding, "Template identity"). It is a
  free-form string on purpose; a folder identity is an absolute path, a git
  identity is `gh:a/b@ref#sub`, and these are not `identifier`s.
- Inner keys are `Id` (parsed at load, same as global `defaults`), so a bad id is
  a `ConfigError::Parse` naming the file — the existing behavior, extended.
- Values stay `serde_json::Value`; per-question-kind validation is deferred to the
  projection step that has the `Template` in hand, exactly as global defaults do
  today (`per boundary-discipline`: parse structure at the config boundary,
  validate against the question's kind where the kind is known).

### Wire type (boundary only, `src/config.rs`)

```rust
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    defaults: Option<IndexMap<String, Value>>,                 // UNCHANGED
    templates: Option<IndexMap<String, TemplateSection>>,      // NEW
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct TemplateSection {
    defaults: Option<IndexMap<String, Value>>,
}
```

The wire type never leaves `config.rs`; it is parsed into the domain `Config`
(`per boundary-discipline`). Nesting `defaults` under each template mirrors the
top-level `defaults` exactly, so schema validation of the inner map reuses the
existing `#/$defs/defaults` verbatim and leaves room for future per-template keys
without another shape.

---

## Function signatures

### New boundary resolver (`src/interview.rs`, replaces the projection call)

```rust
/// The effective configured defaults for one template identity: the global-by-id
/// base overlaid with the identity-scoped defaults for `formal_name`. A
/// template-specific value overrides the global value of the same id. Each value
/// is validated against its question's prompt kind; ids the template does not ask
/// are ignored (as global defaults already are). The returned flat map is the
/// exact input contract of `Seed.defaults` — the pure engine is unchanged.
///
/// Error attribution:
///   - a bad global value  -> "configuration key defaults.<id>: <msg>"      (UNCHANGED)
///   - a bad scoped value  -> "configuration key templates.<formal>.defaults.<id>: <msg>"
pub fn resolved_defaults(
    template: &Template,
    formal_name: &str,
    global: &IndexMap<Id, Value>,
    scoped: Option<&IndexMap<Id, Value>>,
) -> Result<IndexMap<Id, RawAnswer>, EvalError> {
    // Project the base, then the overlay; overlay wins per id.
    let mut out = project(template, global, ConfigKey::Global)?;
    if let Some(scoped) = scoped {
        for (id, raw) in project(template, scoped, ConfigKey::Scoped(formal_name))? {
            out.insert(id, raw);              // specificity: scoped overrides global
        }
    }
    Ok(out)
}

/// The existing per-template projection, kept for global-only crate callers and
/// used internally by `resolved_defaults`. UNCHANGED external behavior.
pub fn configured_defaults(
    template: &Template,
    values: &IndexMap<Id, Value>,
) -> Result<IndexMap<Id, RawAnswer>, EvalError> {
    project(template, values, ConfigKey::Global)
}

/// Which config path a configured-default error names.
enum ConfigKey<'a> { Global, Scoped(&'a str) }

/// Project one id->value map against `template`, validating each value against
/// its question's prompt kind and attributing errors to `key`.
fn project(
    template: &Template,
    values: &IndexMap<Id, Value>,
    key: ConfigKey<'_>,
) -> Result<IndexMap<Id, RawAnswer>, EvalError> {
    // TODO: identical to today's configured_defaults body, except the EvalError
    // is built with `configured_default_error(key, id, msg)` so a scoped value's
    // error reads "configuration key templates.<formal>.defaults.<id>".
    unimplemented!("not implemented")
}
```

### Error attribution (`src/interview.rs`, minimal contained edit)

```rust
#[derive(Debug)]
pub struct EvalError {
    pub id: Id,
    pub field: &'static str,
    pub message: String,
    pub expression: Option<String>,
    pub config_key: Option<String>,   // NEW: full config path for CONFIGURED_DEFAULT errors
}

// Display, when field == CONFIGURED_DEFAULT:
//   let key = self.config_key.as_deref()
//       .unwrap_or(&format!("defaults.{}", self.id));   // global default keeps today's text
//   write!(f, "configuration key {key}: {}", self.message)

fn configured_default_error(key: ConfigKey<'_>, id: &Id, message: String) -> EvalError {
    let config_key = match key {
        ConfigKey::Global => format!("defaults.{id}"),
        ConfigKey::Scoped(formal) => format!("templates.{formal}.defaults.{id}"),
    };
    EvalError { id: id.clone(), field: CONFIGURED_DEFAULT, message,
                expression: None, config_key: Some(config_key) }
}
```

`config_key` defaults to `None` everywhere it is constructed today, and the
Display fallback reproduces the exact `defaults.<id>` text, so no existing error
message changes (`per single-source-of-truth`: the key format lives in one
helper).

### Config load (`src/config.rs`)

```rust
// Inside load(), alongside the existing `defaults` merge loop (151-168):
let mut template_defaults: IndexMap<String, IndexMap<Id, Value>> = IndexMap::new();
for (layer, file) in [(&system, &dirs.system_config),
                      (&user,   &dirs.user_config),
                      (&local,  &local_file)] {
    if let Some(sections) = &layer.templates {
        for (formal, section) in sections {
            let bucket = template_defaults.entry(formal.clone()).or_default();
            if let Some(values) = &section.defaults {
                for (key, value) in values {
                    bucket.insert(
                        Id::parse(key).map_err(|message| ConfigError::Parse {
                            path: file.clone(), message })?,
                        value.clone(),                 // higher layer wins, per id (as global)
                    );
                }
            }
        }
    }
}
```

The three layers collapse **per (identity, id)** with local > user > system —
identical to the global merge, one level deeper.

### Wiring edits (the entire blast radius outside `config.rs`/`interview.rs`)

```rust
// src/main.rs — seed() gains the identity; both callers already hold resolved.formal_name.
fn seed(template: &Template, config: &Config, formal_name: &str) -> Result<Seed, String> {
    // ...now unchanged...
    let scoped = config.template_defaults.get(formal_name);
    Ok(Seed { now, defaults:
        interview::resolved_defaults(template, formal_name, &config.defaults, scoped)
            .map_err(|e| e.to_string())? })
}
// call sites: seed(&template, &config, &resolved.formal_name)   (main.rs:581, 831)

// src/main.rs — the three replay sites: swap configured_defaults(...) for
// resolved_defaults(&template, &saved.template, &config.defaults,
//                    config.template_defaults.get(&saved.template))
//   progress()   main.rs:496
//   continue     main.rs:664
//   apply-staged main.rs:902
```

Every driver path — terminal, headless, staged, direct, crate — routes through
`resolved_defaults` and produces the same flat `IndexMap<Id, RawAnswer>`. On
resume, the formal name comes from `saved.template` (the recorded identity) and
the overlay is re-read from live config; nothing is frozen into the staged record
(`per make-operations-idempotent`: resume recomputes, never replays stale
defaults).

---

## Module and seam map

```text
  <TEMPLATE> arg + layered config files
        │
        ▼
  src/cli/resolve.rs  resolve_template / resume_template   (UNCHANGED)
        │   ResolvedTemplate { formal_name, commit, folder, trusted, named }
        │   └─ formal_name is the stable identity (already in scope at every seed/replay site)
        ▼
  src/main.rs  seed() / continue / apply / progress
        │   formal_name ─┐
        │   config.defaults (global) ─┐
        │   config.template_defaults.get(formal_name) (scoped) ─┐
        ▼                                                        ▼
  src/interview.rs  resolved_defaults(template, formal_name, global, scoped)
        │   overlay: scoped overrides global per id; validate per question kind
        │   ▼
        │   IndexMap<Id, RawAnswer>          ← SAME flat contract as today
        ▼
  Seed { now, defaults }
        ▼
  Interview (pure, identity-UNAWARE)          ← UNCHANGED
```

Three files touched (`config.rs`, `interview.rs`, `main.rs` wiring), one new
public function, one new `Config` field. Tracing input→result reads through the
types alone (`per minimize-reader-load`).

---

## Precedence — total, deterministic rule

For a template with identity `F` and a question id `I`, the configured default
value is the **first** of these that exists:

| Rank | Source | Layer order within source |
| ---: | --- | --- |
| 1 | `templates.<F>.defaults.<I>` (scoped) | local, else user, else system |
| 2 | `defaults.<I>` (global) | local, else user, else system |
| 3 | *(none)* — the template's own `default` applies (engine unchanged) | — |

Equivalently: collapse each of the two tiers across layers (as today), then
**overlay scoped onto global, scoped wins per id.** Two orthogonal dimensions
(specificity, layer) resolved by a fixed priority: **specificity dominates
layer.** This is total (every `(F, I)` maps to exactly one outcome) and
deterministic (independent of file read order beyond the defined layer order).

Chosen because a template-specific instruction is a precise statement about that
one template and should not be silently overridden by a broader default set in a
higher layer. The one surprising consequence — a *system* scoped default beats a
*local* global default — is called out as decision **P1** below; the alternative
(layer dominates specificity) is a one-line flip in `resolved_defaults` and is
falsified by behavior B5.

Across driver paths there is no additional precedence: terminal, headless,
staged, direct, and crate all call `resolved_defaults` with the same inputs and
must return byte-identical results (behavior B11). An interactive person can
still edit the seeded default (`src/cli/terminal.rs:81`), unchanged.

---

## Selectors — how each selection form is keyed and matched

The config key is matched by **exact string equality against the resolved
`formal_name`**, not against the raw CLI selector. This is the load-bearing
choice: ambiguity and aliasing are resolved *before* a formal name exists, so
template-specific defaults never face an ambiguous or alias-vs-formal collision.

| Selection form (CLI) | `formal_name` produced (grounding) | Config key to write |
| --- | --- | --- |
| Bare folder / `./x` / `/x` | canonical absolute folder path | that absolute path, e.g. `"/home/me/work/note-template"` |
| Git address | `gh:a/b@ref#sub` (normalized) | that normalized address |
| Installed name / alias / short | registry formal key (a source address, e.g. `gh:a/b`) | that formal key |
| Duplicate short name | each entry's **distinct** formal key | one key per entry — no cross-talk |
| Bundled demo | `toha-demo` (reserved) | `"toha-demo"` |

- **Two templates sharing a title/short name** have distinct formal names, hence
  distinct keys, hence independent defaults (behavior B1, B8) — the feature's
  reason for existing.
- **Selector matches nothing:** if `formal_name` has no key under `templates`,
  only the global tier applies. This is silent and the common case (behavior B6);
  it is never an error.
- **Selector itself ambiguous:** an ambiguous short name is
  `registry::ResolveError::Ambiguous` (exit 5) at `resolve_template`, *before*
  any default is computed. Template-specific defaults are therefore structurally
  incapable of being ambiguous (behavior B8).
- **Alias vs formal name collision:** impossible at this layer — the key is the
  resolved formal name; aliases are already collapsed to a formal name by
  `registry.resolve` (alias → short → formal). An alias string used as a config
  key simply never matches any `formal_name` and is inert.
- **Identity stability:** git formal names are computed with `config.hosts`,
  which local config cannot set (`config.yml:25-27`). A scoped key therefore
  means the same thing in every directory even when set in a local `.toha.yml`
  (grounding, "Identity stability").
- **Known sharp edge:** the *same* template addressed by installed name
  (formal = source address) versus by direct folder path (formal = absolute
  path) has two identities and needs two keys to be defaulted the same way. This
  is inherent to identity and surfaced as open question **Q2**.

---

## Error / results contract

| Situation | Result |
| --- | --- |
| Scoped default for `F`, `F` is the resolved template, id asked | value seeds that question |
| Scoped default id not asked by the template | ignored (as global unknown ids are) |
| No scoped entry for `F` | global tier applies; no error |
| Scoped value wrong type for the question kind | `EvalError` → `configuration key templates.<F>.defaults.<id>: <msg>`, exit 1 |
| Global value wrong type (unchanged) | `configuration key defaults.<id>: <msg>`, exit 1 |
| Bad id inside a `templates.<F>.defaults` map | `ConfigError::Parse` naming the file (as global), exit code unchanged |
| `templates` key with unknown formal name (matches no template ever run) | inert; no error |
| Ambiguous CLI selector | `ResolveError::Ambiguous`, exit 5 (unchanged; occurs before defaults) |
| Same inputs, any driver path | identical resolved defaults |

No new exit code. No new trust, permission, timeout, pinned-version-check, or
subprocess surface: `resolved_defaults` reads only config maps and the template's
question kinds; it never consults `trusted`/`named` and never runs a hook
(grounding, "Trust seam").

---

## Behaviors to prove (falsifiable)

- **B1 two templates, one id, two defaults** — `title` scoped to `F1`=A and
  `F2`=B; `apply F1` yields A, `apply F2` yields B. *Fails if a bare id still
  forces one shared default.*
- **B2 global preserved** — global `title=G`, no scoped entry for `F3`; `apply
  F3` yields G. *Fails if global-by-id was narrowed.*
- **B3 specificity beats global** — global `title=G`, scoped `F.title=S`; `F`
  yields S. *Fails if overlay direction is wrong.*
- **B4 layer within scope** — system scoped `F.title=Sys`, local scoped
  `F.title=Loc`; `F` yields Loc. *Fails if layer order is not local>user>system.*
- **B5 specificity dominates layer (P1)** — system scoped `F.title=Sys`, local
  *global* `title=LG`; `F` yields Sys. *Fails ⇒ the layer-dominates rule was
  chosen instead; this test pins P1.*
- **B6 nonmatching identity is silent** — scoped entry only for `F'`; `apply F`
  uses global/template default, no warning, no error. *Fails if unknown-key
  matching leaks or errors.*
- **B7 invalid scoped type attribution** — scoped `F.include_summary="yes"` on a
  confirm question → `configuration key templates.<F>.defaults.include_summary`.
  *Fails if the error still says `defaults.include_summary`.*
- **B8 duplicate short name isolation** — two entries share short name `notes`,
  distinct formal names `X`,`Y`; scoped `X.title=A`,`Y.title=B`; resolving each
  by its formal name yields A/B; resolving the ambiguous short `notes` exits 5.
  *Fails if defaults key on short name.*
- **B9 re-resolution on resume** — stage `F`; change scoped `F.title` in config;
  `continue`/`apply` reflects the new value. *Fails if defaults were frozen into
  the staged record.*
- **B10 bundled demo identity** — scoped `"toha-demo".title=D`; `apply toha-demo`
  yields D; an installed template whose formal name is a source address is
  unaffected. *Fails if the reserved identity is not keyable.*
- **B11 driver parity** — same config + template + answers give identical output
  via terminal, headless, staged, direct, and crate paths. *Fails if any path
  bypasses `resolved_defaults`.*
- **B12 local layer participates** — a local `.toha.yml` `templates:` overlay
  applies. *Fails if scoping is silently system/user-only.*

---

## Proposed schema / doc edits (described, NOT applied)

### `docs/specifications/config.schema.yml`

```yaml
$defs:
  # identifier, prefix, templates-paths, defaults, hosts — UNCHANGED
  template-key:                 # NEW: a template formal name (free-form, non-empty)
    type: string
    minLength: 1
  template-defaults:            # NEW
    type: object
    propertyNames:
      $ref: "#/$defs/template-key"
    additionalProperties:
      type: object
      additionalProperties: false
      properties:
        defaults:
          $ref: "#/$defs/defaults"     # reuse: inner ids validated exactly as global
  shared-config:
    properties:
      # ...existing...
      templates:
        $ref: "#/$defs/template-defaults"    # NEW
  local-config:
    properties:
      # ...existing (templates-paths, defaults)...
      templates:
        $ref: "#/$defs/template-defaults"    # NEW: local layer may scope too
```

Validation coverage: matching vs nonmatching templates (runtime, B1/B6);
layers (B4/B12); aliases (inert key, selectors section); invalid types
(deferred per-kind, B7); bad ids (`propertyNames` on the inner `defaults`,
`ConfigError::Parse`). `additionalProperties: false` on the section rejects typos
like `default:` (singular) at parse time.

### `docs/configuration.md`

Add a "Set defaults for one template" section after "Set default answers":
- The key is the template's **formal name**, with the selector→formal-name table.
- The one-sentence precedence rule and a worked example matching call site A.
- A note that the key is the resolved identity, so duplicate short names stay
  separate and a folder identity is its absolute path.
- Extend "How the layers combine" to state: within `templates`, layers merge per
  (identity, id); across tiers, a template-specific default beats a global one.

### Embedded guidance

- `configured_default_error` produces the `templates.<F>.defaults.<id>` text used
  by CLI error output; no separate guidance string is needed.
- If `--help`/guidance enumerates config keys, add `templates` beside `defaults`.

---

## Broader reusable-default mechanisms (explored; recommendation)

| Mechanism | Accidental cross-template | User control | Author dependence | Identity stability | Config complexity |
| --- | --- | --- | --- | --- | --- |
| **1. Identity-scoped overlay (this design)** | none — keyed on unique formal name | full | none | high (formal name) | low–moderate |
| 2. Tag/trait defaults — templates declare tags in `template.yml`; config sets defaults per tag | possible — two templates share a tag | shared/reusable | high — needs authors to tag | lower — authors can rename tags | moderate + schema in template |
| 3. Named default profiles — config defines profiles; `--profile X` selects one | none, but manual per run | full but manual | none | n/a | moderate + new CLI surface |

Mechanism 1 is the only one that solves the stated problem (two templates, same
id, different default) **automatically and without author cooperation**, with
zero accidental cross-template effect. Mechanism 2 trades that safety for reuse
across a family of templates; Mechanism 3 does not default automatically at all.

**Recommended future direction:** ship Mechanism 1 now. If demand for reuse
across a template family appears, add tags later as an *additional tier that
slots between global and identity-specific*, preserving the total order:

```
global (rank 3)  <  tag (future rank 2)  <  identity-specific (rank 1)
```

This is deliberately **not** built in this slice; the overlay's precedence rule
is designed to accept a middle tier without reshaping.

---

## Global-by-id disposition — options and recommendation

- **A (recommend): keep global-by-id unchanged as the base layer.** The overlay
  is purely additive; no existing config or behavior changes; no narrowing, so no
  approval needed (grounding: "no silent removal").
- B: deprecate global, require every default to be scoped. Rejected — breaks
  every current user, removes a supported capability (needs Bob's explicit
  approval), and forces per-template duplication of genuinely global answers.
- C: keep both but warn when an id is set both globally and scoped. Rejected —
  the overlap is the intended, documented mechanism; a warning would be noise.

Recommend A. No capability is removed or narrowed.

---

## Out of scope

- The pure interview engine, `Seed`/`RawAnswer`, `Interview`, `Plan`, `apply`,
  `protocol`, `staging`, `ResolvedTemplate`, and the `<TEMPLATE>` classification
  order — all unchanged.
- Tag/trait defaults and named profiles (future tier; not built).
- Fuzzy or ref-insensitive identity matching (keys match `formal_name` exactly).
- Unifying folder-path identity with installed-name identity for the same
  template (open question Q2).
- Any new subcommand, flag, trust, permission, timeout, pinned-check, or
  subprocess surface.
- Production or shared spec/schema edits (this package proposes; the paired
  implementation applies).

---

## Decisions needed first (Bob)

- **P1 — Precedence when tiers and layers disagree:** *specificity dominates
  layer* (recommended; a template-specific default beats a global one even from a
  higher layer) vs *layer dominates specificity* (a local global default beats a
  system template-specific one). Recommend specificity-dominates; pinned by B5.
- **P2 — Top-level key name:** `templates:` (recommended; mirrors `defaults:`,
  extensible to future per-template settings) vs `template-defaults:` (avoids any
  visual confusion with `templates-paths:`). Recommend `templates:`.
- **Q1 — Local-layer scoping:** confirm a local `.toha.yml` may set `templates:`
  overlays (recommended; parity with global `defaults`, which local already
  allows). Recommend yes.
- **Q2 — Multi-identity same template:** should addressing one template by
  installed name vs by folder path share one overlay? Recommended: no (they are
  distinct identities); revisit only if it bites users.
