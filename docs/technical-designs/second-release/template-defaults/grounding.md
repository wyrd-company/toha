# Template-specific configured defaults — grounding

Phase A of the architect workflow. Traced caller-to-result flow, types, state
ownership, error paths, persistence/replay, and trust seams for the existing
configured-defaults capability, with file/symbol evidence. This is the constraint
set every candidate design must honor.

## The capability today (how)

`defaults` in a toha config file maps a **question id** to a value. That value
replaces the question's own `default` in **every** template that asks that id.

Traced flow, input to result:

1. **Boundary parse.** `config::load` (`src/config.rs:112`) reads the system,
   user, and local layers. Each `Layer.defaults: Option<IndexMap<String, Value>>`
   (`src/config.rs:51`) is schema-validated against `#/$defs/defaults`
   (`docs/specifications/config.schema.yml:20`): an object whose property names
   match `identifier` (`^[a-z_][a-z0-9_]*$`). The three layers merge key by key,
   higher layer wins, into `Config.defaults: IndexMap<Id, Value>`
   (`src/config.rs:26`, merge loop `151-168`). Keys parse to `Id` here; a bad id
   is a `ConfigError::Parse` naming the offending file.
2. **Per-template projection.** At each driver path, `configured_defaults(template,
   &config.defaults)` (`src/interview.rs:677`) walks `config.defaults`, keeps only
   ids the template actually defines (`question_by_id`, `692`), validates each
   value against that question's prompt kind (`parse_kind`), and returns
   `IndexMap<Id, RawAnswer>`. An invalid value is an `EvalError` with field
   `CONFIGURED_DEFAULT` rendered as `configuration key defaults.<id>: <message>`
   (`src/interview.rs:228-232`).
3. **Seed.** The projected map becomes `Seed.defaults: IndexMap<Id, RawAnswer>`
   (`src/interview.rs:37-40`), built in `seed()` (`src/main.rs:414-425`).
4. **Pure engine.** `Seed.defaults` overrides each question's template `default`:
   `question_ready`/`default_ready` treat a configured default as always-ready
   (`override_default`, `src/interview.rs:338,370`), and `render_default` returns
   the configured value instead of evaluating the template default
   (`src/interview.rs:393-397`). The interactive driver still lets a person change
   it (`src/cli/terminal.rs:81`).

## Where it is consumed (every path)

`configured_defaults` + `Seed.defaults` is the single chokepoint for all drivers:

| Path | Call site |
| --- | --- |
| Terminal / headless `stage` and `apply` (direct) | `seed()` `src/main.rs:422` |
| `continue` a staged interview | `src/main.rs:664` |
| `apply` of a staged interview | `src/main.rs:902` |
| Staged-progress guidance | `src/main.rs:496` → `staging::replay_with_defaults` |
| Crate callers | `Seed { now, defaults }` + `Interview::start` / `replay_with_defaults` (`src/staging.rs:131-140`) |

Every path funnels the same flat `IndexMap<Id, RawAnswer>` into the engine. The
engine never learns which template it is; identity is resolved **before** the
engine and flattened away. This is the seam a template-specific design keys on: it
must still hand the pure engine one flat resolved map, exactly as today.

## Template identity and the selectors (how)

A `<TEMPLATE>` argument classifies into an `Address` (`src/source.rs:69`,
`parse`): `Git`, `Folder`, or `Name`. `resolve_template` (`src/cli/resolve.rs:175`)
produces a `ResolvedTemplate` (`src/cli/resolve.rs:16`) whose `formal_name` is the
stable identity:

| Selection form | `formal_name` produced | Evidence |
| --- | --- | --- |
| Installed name / alias / short | registry formal key (a source address, e.g. `gh:a/b`) | `registry.resolve` `src/registry.rs:471` |
| Discovered (scanned, no entry) | canonical absolute folder path | `src/registry.rs:428,456`; spec `template-registry.yml:37-39` |
| Bare folder / `./x` / `/x` | canonical absolute folder path | `src/cli/resolve.rs:184-190` |
| Git address | normalized address `gh:a/b@ref#sub` | `Address::formal_name` `src/source.rs:118` |
| Bundled demo (predecessor 1074) | `toha-demo` (reserved) | approved design `f949b6a` |

`registry.resolve(name)` (`src/registry.rs:471-498`) resolution order is **alias →
short name → formal key**, and a short name shared by two entries is
`ResolveError::Ambiguous` (`src/registry.rs:485-490`), surfaced as exit 5. Aliases
are globally unique (conflict-checked, `check_aliases` `385`). A template's short
`name` comes from its `template.yml` (`template-registry.yml:23`) and is **not**
unique across templates — this is exactly why the feature constraint forbids keying
template-specific defaults on short name alone.

**Two templates can share a short name and even a question title, yet have
distinct formal names.** Formal name is the one selector that is unambiguous across
folders, git addresses, installed names, aliases, and the bundled identity.
`ResolvedTemplate.formal_name` (and, for content pinning, `commit`) is already in
scope at every `configured_defaults` call site.

## Ownership and layering (why)

- **Config layers.** `defaults` is available in both `shared-config` (system/user)
  and `local-config` (`config.schema.yml:40-51`). A project's local `.toha.yml`
  can set defaults; it cannot set `hosts` or `local-config-name`
  (`config.yml:25-27`). Merge is per key, local > user > system
  (`src/config.rs:151-168`; spec `config.yml:23-25`).
- **Identity stability.** `hosts` and `local-config-name` are user/system only "so
  a formal name always resolves to the same source" (`config.yml:25-27`,
  `configuration.md:94-95`). A template-specific-defaults design keyed on formal
  name inherits this: the key's meaning must not depend on project-local host
  remapping.
- **Registry vs config.** Installed-template facts (source, commit, aliases,
  trust) live in `templates.yml` (`template-registry.yml`); user preferences
  (search paths, defaults, hosts) live in `config.yml`. Defaults are a *user
  preference about answers*, not a fact about an installed template — grounding
  the expectation that template-specific defaults belong in config, keyed by
  identity, not written into registry entries.

## Persistence and replay

A staged interview records `template` (= `formal_name`) and `commit` in
`StagedRecord` (`src/main.rs:464-473`; `src/staging.rs`). Resume re-resolves the
same identity (`resume_template`, `src/cli/resolve.rs:224`) and **re-derives**
`configured_defaults` from the *current* config at replay time (`src/main.rs:496,
664,902`) — configured defaults are not frozen into the staged record. A
template-specific design must keep this property: defaults are re-resolved against
live config on resume, keyed by the recorded identity, and must produce identical
results across terminal, headless, staged, direct, and crate paths for the same
inputs.

## Trust seam

Configured defaults never touch trust. Trust gates hook execution
(`trustable`/`registry_trusted`, `src/main.rs:439-443,896-897`); defaults only
seed answers. A template-specific-defaults design must not become a new way to
influence trust or hook execution, and must not run a hook to resolve a default.

## Preserve / Change / Avoid / Risk

- **Preserve.** The pure engine's `Seed.defaults: IndexMap<Id, RawAnswer>`
  contract; global-by-id defaults as a supported capability (feature constraint,
  no silent removal); identical results across all driver paths; re-resolution on
  resume; local-layer participation; the `defaults.<id>` error attribution; formal
  name as stable identity; no trust influence.
- **Change.** Introduce a way to scope a default to a template **identity** so two
  templates that share a question id can receive different defaults; define
  precedence between global-by-id and template-specific, across layers and
  specificity; extend the config schema and `configuration.md`/embedded guidance.
- **Avoid.** Keying on short name or ambiguous selectors; freezing defaults into
  staged records; engine changes that make it identity-aware; writing defaults
  into the registry; any new trust, permission, timeout, pinned-check, or
  subprocess surface.
- **Risk.** Silent narrowing of global-by-id (needs Bob's explicit decision);
  precedence ambiguity when both a global and a template-specific default match;
  selector collision between an alias and a formal name; a broader "reusable
  default" mechanism causing accidental cross-template effects. These are the
  criteria axes and the Phase C decisions.

## Consumed predecessor contract

Bundled-demo design, exact approved revision `f949b6a5152904b234bd4f21b7e05aaa843d42e8`
(`docs/technical-designs/second-release/bundled-demo/design.md`), with Bob's
approval of D1–D4 and downstream identity. The coordination surface this design
keys on: `formal_name = "toha-demo"` (stable identity) + `commit = <canonical
content digest>` (content version). `toha-demo` is a reserved fallback name; an
installed/aliased/discovered `toha-demo` wins over the bundled demo. A selector
addressing the bundled demo therefore uses the formal name `toha-demo`.
