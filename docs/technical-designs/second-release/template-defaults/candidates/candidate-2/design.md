# Template-specific configured defaults — Candidate 2 (ordered default rules)

A default rule is a `match` selector plus a `set` of id→value pairs. The config
grows one ordered list, `template-defaults`, of these rules. The bare `defaults`
map stays exactly as it is and remains the ONLY way to express a global (every
template) default. At the CLI boundary, for one resolved target identity, the
list is folded onto the global map into the same flat `IndexMap<Id, Value>` the
system already produces, and that map flows into the unchanged
`configured_defaults` → `Seed.defaults` → pure engine path. The engine, staging,
`Template`, `Plan`, and the `Seed` contract learn nothing new.

> Divergence from the runner's lead hypothesis, stated up front: the hypothesis
> asked for a single uniform list where global is `match: any`. I keep global as
> the existing `defaults` map and forbid `any` in the list. Reason in one line:
> making global a list entry forces per-layer global tiers to be reconstructed
> for precedence, which the current `config::load` merge (`src/config.rs:151`)
> has already collapsed; keeping global as the map lets `Config.defaults` keep
> its exact type and makes the precedence fold trivial and total. The list gives
> the expressiveness (ordered, selector-typed rules); the map gives the stable
> global tier. See rationale for why this is the stronger shape.

---

## Caller's usage (the spec)

### Quickstart the config author reads

```text
`defaults` sets an answer for EVERY template that asks that question id — this is
unchanged. To set an answer for ONE template, add a `template-defaults` rule.
Each rule names a template with `match` and lists answers with `set`:

    defaults:                       # global: applies to every template (unchanged)
      title: Untitled

    template-defaults:              # template-specific: applies to one identity
      - match: gh:owner/collection
        set:
          title: Field Notes
      - match: ./templates/journal
        set:
          title: Daily Log

`match` accepts the same template forms as the command line: an installed name or
alias, a `gh:owner/collection` git address, a folder like `./x` or `/x`, or the
reserved `toha-demo`. Two templates that both ask `title` but resolve to
different identities receive different titles.

A template-specific rule always wins over a global default for the template it
names. When two rules name the same template, the more local config layer wins;
inside one layer, the rule written later wins. Name templates by their git
address or installed formal name — a bare short name shared by two templates is
ambiguous and refused when it would decide the template you are running.
```

### Call site A — two templates, same question id, different defaults

`~/.config/toha/config.yml`:

```yaml
defaults:
  title: Untitled
template-defaults:
  - match: gh:owner/collection
    set:
      title: Field Notes
  - match: gh:owner/journal
    set:
      title: Daily Log
```

```console
$ toha apply gh:owner/collection ./out
Note title: Field Notes          # rule for gh:owner/collection wins over global

$ toha apply gh:owner/journal ./out
Note title: Daily Log            # rule for gh:owner/journal wins over global

$ toha apply gh:owner/misc ./out
Note title: Untitled             # no rule matches → global default
```

Both `collection` and `journal` ask the id `title`; today they are forced to
share one value. The rule list separates them by identity.

### Call site B — headless apply, precedence across layers

System `/etc/toha/config.yml`:

```yaml
template-defaults:
  - match: gh:owner/collection
    set:
      owner: acme            # org-wide policy default for this template
```

Local `.toha.yml` (this project only):

```yaml
template-defaults:
  - match: gh:owner/collection
    set:
      owner: field-team      # this project overrides the org default
```

```console
$ printf '{"title":"Q3"}' > answers.json
$ toha apply --answers answers.json gh:owner/collection ./out
# owner default = field-team   (same specificity → local layer wins over system)
```

Same selector, same id, two layers → the local layer wins. A person running the
interactive form still sees `field-team` pre-filled and can change it
(`src/cli/terminal.rs:81`).

### Call site C — stage now, resume later; defaults re-resolve against live config

```console
$ toha stage gh:owner/collection ./out --async batch.json   # process 1
# ...edit config: change the collection rule's title to "Renamed"...
$ toha continue ./out answers.json                          # process 2
$ toha apply ./out                                          # process 3
Note title: Renamed          # re-resolved from CURRENT config, not frozen at stage
```

Resume re-derives defaults from the current config keyed by the recorded identity
(`StagedRecord.template` = formal name), exactly as global defaults do today
(`src/main.rs:664,902`). Nothing about the rule set is written into the staged
record.

### Crate callers — unchanged

```rust
let seed = Seed { now, defaults };            // IndexMap<Id, RawAnswer>, unchanged
let interview = Interview::start(&template, seed)?;
```

Library callers build `Seed.defaults` themselves and see no new type. Rule
flattening is a binary/CLI concern (it needs the `Registry` and `Dirs` to
normalize selectors), living beside the other CLI resolution code.

---

## Module and seam map

```text
config files (system / user / local)
    defaults: { id: value }                 ← global tier (UNCHANGED)
    template-defaults: [ { match, set } ]   ← NEW ordered rule list
        │
        ▼   parse ONLY — raw selector strings, ids parsed to Id, layer+order tagged
config::load                         src/config.rs  (Config gains `default_rules`)
    Config { defaults: IndexMap<Id,Value>,          ← type UNCHANGED
             default_rules: Vec<Rule>, .. }         ← NEW field
        │
        ▼   CLI boundary: has Registry + Dirs + cwd to normalize selectors
resolve::scoped_defaults(target_formal, config, registry, dirs, cwd)   ← NEW
    base = config.defaults.clone()                  # global tier (specificity 0)
    for rule in config.default_rules (in (layer,order) ascending):
        if normalize(rule.match) == target_formal:  # specificity 1 overrides base
            base.extend(rule.values)                # last write wins ⇒ (layer,order)-max
    → IndexMap<Id, Value>                           # SAME shape defaults was
        │
        ▼   UNCHANGED FROM HERE DOWN
configured_defaults(template, &flat)     src/interview.rs:677   (signature unchanged)
    → IndexMap<Id, RawAnswer>
Seed { now, defaults }  →  Interview (pure)  →  Plan  →  apply / staging
```

Trace: config → `config.rs` (parse) → `resolve.rs` (`scoped_defaults`, the one
new function) → the existing `configured_defaults` → the pure engine. Three files
touched below the schema/docs: `src/config.rs` (types + parse), `src/cli/resolve.rs`
(new function), `src/main.rs` (four call sites pass the target). The flow below
`configured_defaults` is byte-for-byte the existing path.

---

## Data shapes

### Config-boundary types (`src/config.rs`)

```rust
/// A layer's origin, used only to order rules deterministically.
/// Local is most local; system is least. Higher rank wins ties of equal
/// specificity (see the precedence rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin { System = 0, User = 1, Local = 2 }

/// One template-specific default rule, parsed at the config boundary.
/// `selector` is the raw `<TEMPLATE>` grammar string (never `any`); it is
/// normalized to a formal name lazily, at match time, where the Registry exists.
#[derive(Debug, Clone)]
pub struct Rule {
    pub selector: String,            // e.g. "gh:owner/collection", "./x", "my-alias"
    pub origin: Origin,              // which layer declared it
    pub values: IndexMap<Id, Value>, // ids already parsed; values still raw JSON
}

pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub defaults: IndexMap<Id, Value>,   // UNCHANGED: the global tier
    pub default_rules: Vec<Rule>,        // NEW: system, then user, then local; each
                                         //      layer's rules in file order
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
```

`Config.defaults` keeps its exact current type and meaning, so every existing
reader and crate caller is unaffected. `default_rules` is stored already sorted
in ascending precedence order — system block, then user block, then local block,
each block in the file's declaration order — so `scoped_defaults` never sorts;
it applies in vector order and lets the last write win.

### Layer parse type (`src/config.rs`, private)

```rust
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    defaults: Option<IndexMap<String, Value>>,       // unchanged
    template_defaults: Option<Vec<RawRule>>,         // NEW
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawRule {
    r#match: String,                    // selector string
    set: IndexMap<String, Value>,       // id (as string) → raw value
}
```

Nothing wire-shaped reaches past `config::load`; ids are parsed to `Id` and the
selector kept as a raw domain string, per boundary-discipline.

### The flat result — unchanged

`scoped_defaults` returns `IndexMap<Id, Value>`: the exact type
`configured_defaults` already accepts (`src/interview.rs:679`). No new type
crosses into the engine; `Seed.defaults: IndexMap<Id, RawAnswer>` is untouched.

---

## Function signatures

### New — the whole feature's boundary (`src/cli/resolve.rs`)

```rust
/// Fold the template-specific default rules onto the global defaults for one
/// resolved target identity, producing the flat id→value map that
/// `configured_defaults` already consumes.
///
/// `target` is the resolved template's `formal_name` — the stable identity from
/// `ResolvedTemplate`/`StagedRecord`. Selector strings in the rules are
/// normalized with the SAME no-fetch logic as `formal_name` (this module) and
/// compared for equality against `target`.
///
/// Total precedence, highest wins (see "Precedence"): a matching
/// template-specific rule (specificity 1) always overrides the global map
/// (specificity 0); among matching rules, the more local layer wins, then the
/// rule declared later within that layer.
///
/// Errors only when a selector is *ambiguous about the target itself* — a bare
/// short name shared by two entries, one of which is `target`. A selector that
/// resolves to some other identity, is not installed, or names a folder that is
/// absent simply does not match and is skipped (with a one-line stderr warning,
/// matching the local-alias warning at `resolve.rs:67`).
pub fn scoped_defaults(
    target: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<IndexMap<Id, Value>, ResolveError> {
    let mut flat = config.defaults.clone();          // global tier, already layer-merged
    for rule in &config.default_rules {              // ascending (origin, order)
        match selector_matches(&rule.selector, target, config, registry, dirs, cwd)? {
            true => { for (id, v) in &rule.values { flat.insert(id.clone(), v.clone()); } }
            false => {}
        }
    }
    Ok(flat)
}

/// Does this rule's selector name the target identity?
/// Reuses `formal_name` (resolve.rs:209) — no fetch, no network.
fn selector_matches(
    selector: &str,
    target: &str,
    config: &Config,
    registry: &Registry,
    dirs: &Dirs,
    cwd: &Path,
) -> Result<bool, ResolveError> {
    match formal_name(selector, config, registry, dirs, cwd) {
        Ok(formal) => Ok(formal == target),
        Err(ResolveError::Ambiguous { name, matches }) => {
            if matches.iter().any(|m| m == target) {
                // The selector is ambiguous AND could be this target: refuse.
                Err(ResolveError::Ambiguous { name, matches })
            } else {
                Ok(false) // ambiguous, but about other templates — not our target
            }
        }
        Err(ResolveError::Error(message)) => {
            // NotFound or absent folder: the selector cannot name this target.
            eprintln!("warning: template-defaults match `{selector}` did not resolve: {message}");
            Ok(false)
        }
    }
}
```

`formal_name` already exists and already does exactly the no-fetch classification
required: `Folder → canonical path`, `Git → Address::formal_name`, `Name →
registry.resolve` (alias → short → formal, with `Ambiguous`). `scoped_defaults`
adds only the fold and the match decision.

### Changed call sites (`src/main.rs`) — four, each identical in shape

Before (all four sites, e.g. `src/main.rs:422,496,664,902`):

```rust
let defaults = toha::interview::configured_defaults(&template, &config.defaults)?;
```

After:

```rust
let flat = cli::resolve::scoped_defaults(&resolved.formal_name, &config, &registry, dirs, &cwd)?;
let defaults = toha::interview::configured_defaults(&template, &flat)?;
```

- **`seed()` (`src/main.rs:414`)** gains the target and context. New signature:
  ```rust
  fn seed(
      template: &Template,
      target: &str,           // resolved.formal_name
      scope: &Scope,          // { config, registry, dirs, cwd } already defined at :475
  ) -> Result<Seed, String>
  ```
  Its callers already hold `resolved` and the resolution context.
- **`continue` (`:664`)** and **`apply` of a staged interview (`:902`)** use
  `resolved.formal_name` (they already call `resume_template`, so `resolved` is in
  scope) — the recorded identity, re-resolved against live config.
- **`progress` (`:496`)** is best-effort; it uses `saved.template` as the target
  and keeps its `.ok()?` shape:
  ```rust
  let flat = cli::resolve::scoped_defaults(&saved.template, scope.config,
                                           scope.registry, scope.dirs, scope.cwd).ok()?;
  let defaults = configured_defaults(&template, &flat).ok()?;
  ```

`configured_defaults` (`src/interview.rs:677`), `Seed`, `replay_with_defaults`
(`src/staging.rs:131`), and `Interview::start` are UNCHANGED.

---

## Precedence (total, deterministic)

For a target formal name `F` and a question id `q`, consider every source that
supplies a value for `q` and applies to `F`:

- the global map `defaults[q]` (already resolved local→user→system by
  `config::load`), if present — **specificity 0**;
- every rule whose `match` normalizes to `F` and whose `set` contains `q` —
  **specificity 1**.

The winner is the maximum of the ordered key `(specificity, origin, order)`,
compared lexicographically, higher wins:

1. **specificity** — a template-specific rule (1) beats the global map (0).
2. **origin** — Local (2) > User (1) > System (0).
3. **order** — within one layer, the rule declared later wins.

Why this is total: within a single layer, `order` is unique per rule, so no two
distinct candidates ever tie on all three components. The `scoped_defaults` fold
realizes this key without sorting: it starts from the specificity-0 map, then
overwrites with specificity-1 rules applied in ascending `(origin, order)`, so
the last write is the `(origin, order)`-maximum among matching rules, and any
matching rule overwrites the global base.

Properties that follow:

- **Global-only configs are byte-identical to today.** With no
  `template-defaults`, `scoped_defaults` returns `config.defaults.clone()` and
  the whole flow is unchanged. The `local > user > system` per-key merge for
  global defaults (`src/config.rs:151-168`) is preserved verbatim.
- **Specificity dominates layer.** A system rule that names `F` beats a local
  global default for `F`. This is the intended "if you named a template, you
  meant it" behavior. A local user is never locked out: a local rule that names
  `F` (equal specificity) wins by layer.
- **Deterministic across all paths.** Terminal, headless, staged, direct, and
  the crate path all call the same `scoped_defaults` → `configured_defaults`
  chain with the same inputs, so results are identical. Defaults are re-resolved
  on resume against live config, never frozen (`StagedRecord` stores only
  `template`+`commit`).

---

## Schema / config / guide / embedded-guidance proposals

Proposed edits (described, NOT applied — no shared schema/spec files are touched
in this design task).

### `docs/specifications/config.schema.yml`

Add to `$defs`:

```yaml
selector:
  type: string
  minLength: 1
  # A <TEMPLATE> argument: installed name/alias, gh:owner/collection, ./x, /x,
  # or the reserved toha-demo. The literal `any` is NOT allowed — global
  # defaults use the `defaults` map.
  not: { const: any }
rule:
  type: object
  additionalProperties: false
  required: [match, set]
  properties:
    match: { $ref: "#/$defs/selector" }
    set:   { $ref: "#/$defs/defaults" }   # reuse: propertyNames match `identifier`
template-defaults:
  type: array
  items: { $ref: "#/$defs/rule" }
```

Add `template-defaults` to the `properties` of BOTH `shared-config` and
`local-config` (local participation preserved, matching the existing `defaults`
availability at lines 40-51). `additionalProperties: false` is retained on both.

What the schema catches: non-string / empty selector, `match: any`, unknown keys
in a rule, and (via the reused `defaults` `$def`) `set` keys that violate the
`identifier` pattern. What the schema deliberately does NOT catch, because it
needs the registry: whether a selector resolves, matches nothing, or is
ambiguous. Value-type validity (a string given where a bool is required) is
caught downstream by `configured_defaults` exactly as it is for global defaults
today.

### `src/config.rs` parse (`load`, after the existing defaults merge loop)

```rust
let mut default_rules = Vec::new();
for (layer, origin, file) in [
    (&system, Origin::System, &dirs.system_config),
    (&user,   Origin::User,   &dirs.user_config),
    (&local,  Origin::Local,  &local_file),
] {
    if let Some(raw_rules) = &layer.template_defaults {
        for raw in raw_rules {
            let mut values = IndexMap::new();
            for (key, value) in &raw.set {
                values.insert(
                    Id::parse(key).map_err(|message| ConfigError::Parse {
                        path: file.clone(), message })?,
                    value.clone(),
                );
            }
            default_rules.push(Rule { selector: raw.r#match.clone(), origin, values });
        }
    }
}
```

Bad `set` ids report the offending file exactly as global defaults do
(`src/config.rs:160-165`).

### `docs/configuration.md`

- Extend "Set default answers": keep the `defaults`-by-id explanation, then add a
  "Set defaults for one template" subsection with the `template-defaults` list,
  the `match` selector forms, and the "template-specific beats global, then local
  layer, then later rule" precedence sentence.
- In "How the layers combine": add that `template-defaults` blocks concatenate
  local-then-user-then-system for precedence and that a selector's meaning does
  not depend on local `hosts` (local cannot set `hosts`, so a git-address
  selector normalizes the same everywhere — the identity-stability property the
  global `hosts` note already relies on, lines 93-95).

### Embedded guidance / `docs/specifications/command-line-interface.yml`

Note that `template-defaults` selectors use the same `<TEMPLATE>` grammar and
resolution order as command arguments, that `toha-demo` is selectable by that
reserved name, and that a bare short name shared by two templates is refused
(exit 5) when it would decide the running template — point authors at git
addresses / formal names.

---

## Error / results contract

`scoped_defaults` returns `ResolveError` (same enum used across resolution);
`configured_defaults` still returns `EvalError` rendered as `configuration key
defaults.<id>: <message>`.

| Situation | Result |
|---|---|
| No `template-defaults`; only `defaults` map | `flat == config.defaults`; behavior byte-identical to today |
| Rule `match` normalizes to `F` (the target) | Its `set` overrides the global map for that target |
| Rule `match` normalizes to some other identity | Rule skipped (does not match `F`) |
| Rule `match` names an uninstalled template (NotFound) | Rule skipped + one-line stderr warning; unrelated runs unaffected |
| Rule `match` is a folder that is absent on disk | Rule skipped + stderr warning (folder cannot be canonicalized) |
| Rule `match` is a short name shared by two entries, and `target` is one of them | `ResolveError::Ambiguous { name, matches }` → exit 5; message names both formal names and points to using a formal name |
| Rule `match` is an ambiguous short name, but `target` is NOT among the matches | Rule skipped (definitely not this target) |
| Rule value has the wrong type for the winning target's question | `EvalError` → `configuration key defaults.<id>: <message>` (attribution preserved) |
| Two same-layer rules name `F` and set the same id | Later-declared rule wins (deterministic) |
| Global `defaults[q]` set, no rule for `F` mentions `q` | Global value used (unchanged) |

Exit codes are the existing ones (0/1/2/3/5); no new code is introduced.
Ambiguity reuses exit 5 (`ResolveError::Ambiguous`), consistent with the CLI
argument path (`src/registry.rs:485`).

**Known imperfection (call-out, not a defect):** a value that came from a
`template-defaults` rule is still attributed as `configuration key defaults.<id>`
by `configured_defaults`, not as its rule/selector. This preserves the exact
existing error contract with zero engine change. Enriching attribution to name
the rule requires changing `EvalError`/`configured_defaults` (an engine-facing
change) and is deferred — see open questions.

---

## Behaviors to prove (falsifiable)

1. **Separation by identity** — two installed templates both ask `title`; rules
   `match: gh:owner/collection {title: A}` and `match: gh:owner/journal {title:
   B}`. Running each seeds its own title; neither leaks. Fails if a rule for one
   identity affects the other.
2. **Global regression** — a config with only `defaults` (no
   `template-defaults`) produces seeds byte-identical to the current build across
   terminal, headless, staged `continue`, staged `apply`, and progress. Fails if
   the fold changes the global-only result.
3. **Specificity beats layer** — `defaults: {title: G}` (local) plus system
   `match: F {title: S}`. Target `F` seeds `S`; a different target seeds `G`.
   Fails if the local global overrides the system template-specific rule for `F`.
4. **Layer tie at equal specificity** — system `match: F {owner: A}`, local
   `match: F {owner: B}` → `B`. Fails if system wins.
5. **Order tie within a layer** — in one file, two rules that both normalize to
   `F` (one by alias, one by git address) set the same id to different values →
   the later rule wins. Fails if resolution is order-independent or nondeterministic.
6. **Ambiguous selector, relevant** — two entries share short name `n`; a rule
   `match: n`. Running one of the two templates → `ResolveError::Ambiguous` (exit
   5) naming both formal names. Fails if it silently picks one or is ignored.
7. **Ambiguous selector, irrelevant** — same ambiguous `match: n`, but running a
   third, unrelated template → rule skipped, no error. Fails if unrelated runs
   break.
8. **Selector matches nothing** — `match: gh:owner/absent` (not installed) or
   `match: ./missing` → rule skipped, warning on stderr, seed unaffected. Fails
   if it errors or alters the seed.
9. **Resume re-resolves live** — stage `gh:owner/collection`, change that rule's
   value in config, `continue`/`apply` → the new value appears. Fails if the
   staged record froze the default.
10. **Bundled demo selectable** — `match: toha-demo {title: X}` sets the default
    when running the bundled demo, and does not apply to an installed template.
    Fails if the reserved identity is not addressable by a rule.
11. **Value-type validation preserved** — a rule sets a bool where the target's
    question is text → `configuration key defaults.<id>: <message>`. Fails if the
    error is dropped or misattributed to a non-`defaults` key.
12. **Crate contract intact** — `Seed { now, defaults: IndexMap<Id, RawAnswer> }`
    compiles and behaves as before; `configured_defaults` signature unchanged.

---

## Broader reusable-default mechanisms (future exploration)

This slice ships EXACT identity match only. Three broader mechanisms were weighed
on the required axes (accidental cross-template effects, user control,
template-author dependence, identity stability, configuration complexity):

| Mechanism | Accidental effects | User control | Author dependence | Identity stability | Config complexity |
|---|---|---|---|---|---|
| **A. Formal-name prefix / glob** (`match: gh:owner/*`) | Bounded to a namespace; legible | High (author writes the glob) | None | High (formal names are stable) | Low — one selector grammar extension |
| **B. Tags / traits** (template.yml `tags: [rust]`; `match-tag: rust`) | Wide; a tag can hit templates you never intended | Split with authors | High (authors must tag, and keep tags stable) | Low (tags are not identity; can change per version) | Medium — new template.yml surface + config surface |
| **C. Named profiles** (config `profiles:`, opt-in via `--profile`/`active-profile`) | None (opt-in) | Highest (explicit activation) | None | High | High — new CLI surface + activation model |

**Recommended future direction: A (prefix/glob over formal names).** It is the
natural extension of this slice — still identity-keyed, still stable, no
template-author cooperation, and its blast radius is a namespace the author typed,
not a trait someone else assigned. It slots into the precedence *between* exact
and global: reserve the specificity axis as an ordinal `exact (2) > glob (1) >
global (0)` so a future glob tier can be added WITHOUT redefining the precedence
rule. Tags (B) are more powerful but push a dependency and a new failure mode
(stale/over-broad tags) onto template authors and widen accidental effects; hold
until a concrete need appears. Profiles (C) solve a different problem (opt-in
sets of answers) and can layer on independently later.

This slice does not build A/B/C. It ships exact match with the specificity axis
defined as an ordinal so glob is a later, additive change.

---

## Out of scope

The pure interview engine and its `Seed.defaults: IndexMap<Id, RawAnswer>`
contract; `configured_defaults`'s signature; `Template`, `Plan`, `apply`,
`protocol`, `staging`, and `StagedRecord`'s shape; the `<TEMPLATE>` classification
order; trust and hook execution (a rule never influences trust and never runs a
hook to produce a value); glob/tag/profile mechanisms; enriching `EvalError`
provenance; any production code, shared-schema, or spec edits (this task produces
Markdown only).

---

## Size and complexity

- **Size:** ~M. One new function (`scoped_defaults` + `selector_matches`,
  ~40 lines) reusing the existing `formal_name`; new `Rule`/`Origin` types and a
  parse block in `config.rs`; four one-line call-site changes in `main.rs`; one
  `seed()` signature change; schema + docs. No new module.
- **Complexity:** moderate, and honestly named. The genuine cost is the selector
  match semantics — the four-way outcome (matches / other identity / not
  resolvable / ambiguous-and-relevant) is the one subtle part, and it is isolated
  in `selector_matches` with a truth-table test per row. The precedence fold is
  trivial because global stays a pre-merged map; the engine, staging, and crate
  surface do not change at all.
