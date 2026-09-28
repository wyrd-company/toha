# Rationale — candidate 1 (identity-scoped overlay)

## Problem

Toha's `defaults` maps a bare question id to a value that replaces that
question's `default` in **every** template that asks the id
(`src/interview.rs:677` `configured_defaults`, `src/config.rs:26`). Two templates
that both ask `title` are forced to share one default. We need to scope a default
to a single template. The shape is non-obvious because of three constraints that
crossed our boundary: (1) the pure interview engine consumes one flat
`Seed.defaults: IndexMap<Id, RawAnswer>` and must stay identity-unaware
(`src/interview.rs:37-40`), so identity must be resolved and flattened *before*
the engine at all five driver paths (`src/main.rs:496,581,664,831,902`); (2) the
only unambiguous key is the resolved `formal_name`, because a template's short
`name` is not unique and selectors can be ambiguous or aliased
(`src/registry.rs:471-498`); (3) defaults are re-resolved from live config on
resume, never frozen into the staged record (grounding, "Persistence and
replay"). The existing `defaults.<id>` error attribution
(`src/interview.rs:228-232`) and the local-layer participation of `defaults`
(`config.schema.yml:44-51`) must survive.

## Usage (caller's view)

A user adds a `templates:` block to any config layer; each key is a template's
formal name and its `defaults` override the global `defaults` for that one
template. `apply gh:owner/collection` picks up `templates."gh:owner/collection"`;
`apply /path/tmpl` picks up the overlay keyed by that absolute path; a template
with no overlay falls back to global `defaults` exactly as today. A crate caller
holding a formal name calls one new function,
`resolved_defaults(&template, &formal_name, &config.defaults, scoped)`, and gets
the same flat map it already feeds to `Seed`. Full README + three call sites are
in `design.md`.

## Shape

Data structure first: `Config` gains `template_defaults: IndexMap<String,
IndexMap<Id, Value>>` — formal name → (id → value) — sitting beside the unchanged
global `defaults`. The dominant access pattern is "given the resolved
`formal_name`, get its overrides," which is one `IndexMap::get`; no later index or
cache is needed, which tells us the structure is right. The overlay is computed by
one new boundary function, `resolved_defaults`, that projects the global base,
projects the scoped map, and lets scoped win per id — returning the identical
`IndexMap<Id, RawAnswer>` the engine already takes. The engine, `Seed`,
`ResolvedTemplate`, staging, and every driver are untouched below that seam.

Load-bearing decisions: key on the resolved `formal_name`, not the CLI selector,
so ambiguity and aliasing are resolved before a default is ever looked up — the
config layer is structurally incapable of an ambiguous or alias-vs-formal
collision (`per encode-lessons-in-structure`). Precedence is a fixed two-tier
order — specificity dominates layer — computed as "collapse each tier across
layers, then overlay," which is total and independent of read order (`per
make-operations-idempotent`). Validation lives where the question kind is known,
at projection, so a scoped type error is attributed to
`templates.<formal>.defaults.<id>` while the global path keeps its exact existing
text through a single key-format helper (`per single-source-of-truth`,
`per boundary-discipline`).

Interface depth: the public surface grows by one field and one function, yet
hides all of the identity resolution, layer collapse, tier overlay, per-kind
validation, and error attribution behind that function. Callers pass three plain
maps and a string and receive the flat seed map — no wire types, no `Config`
dependency inside `interview.rs` (it takes plain `IndexMap`s), no engine change.
The system deliberately does **not** teach the engine about identity, does not
freeze defaults into staged records, does not add fuzzy matching, and does not
touch trust — the overlay never reads `trusted`/`named` and never runs a hook.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept **exact-string identity keys** (a folder template's absolute path, a
  git address with `@ref#sub`) in exchange for zero-ambiguity, stable-meaning
  keys that never depend on local host remapping. A user pinning `gh:a/b@v1`
  writes a different key than `gh:a/b`; that honesty matches how the cache and
  registry already treat those as distinct.
- We accept **specificity-dominates-layer**, meaning a system-level scoped default
  can beat a local-level global default, in exchange for a rule stateable in one
  sentence and the guarantee that a precise per-template instruction is never
  silently overridden by a broad one. (Pinned by B5; flippable via P1.)
- We accept **one small `EvalError` field** (`config_key`) rather than reusing the
  id-derived key, in exchange for correct error attribution for scoped values;
  the field defaults to `None` and reproduces today's `defaults.<id>` text, so no
  existing message changes.
- We accept that **the same template addressed two ways has two identities** and
  needs two overlay keys, in exchange for a single, unambiguous definition of
  identity. This can look like an oversight; it is deliberate (Q2).

## Alternatives considered

- **Nested map in the existing `defaults` key** (e.g. `defaults` values may be
  either a scalar or an object keyed by formal name). Rejected: it overloads one
  key with two shapes, breaks the flat `IndexMap<Id, Value>` domain type and its
  schema, and makes "is this a global value or a per-template map?" a runtime
  discrimination — worse interface depth for callers and worse validation.
- **Key on the CLI selector string** (what the user typed). Rejected: selectors
  are ambiguous, aliasable, and non-unique; it would reintroduce exactly the
  ambiguity the registry resolves and force fuzzy matching. Keying on the resolved
  formal name hides that complexity in resolution, where it already lives.
- **Freeze resolved defaults into the staged record** so resume is cheap.
  Rejected: violates the re-resolution invariant (grounding) and makes a config
  edit invisible on resume (fails B9); the recompute is trivial.
- **Flat `template-defaults: {formal: {id: value}}` (no inner `defaults` key).**
  A real contender — one level shallower. Rejected narrowly for symmetry with the
  top-level `defaults` and room to add future per-template settings without a
  third shape; the cost is one extra nesting level, called out as P2.

## Open questions and risks

- **Precedence (P1):** should specificity dominate layer, or layer dominate
  specificity? The recommendation is specificity-dominates; is a system scoped
  default beating a local global default acceptable to you, or should local always
  be the strongest voice?
- **Key name (P2):** is `templates:` clear enough beside `templates-paths:`, or
  would `template-defaults:` avoid confusion at the cost of symmetry with
  `defaults:`?
- **Multi-identity (Q2):** should addressing one template by installed name and
  by folder path share one overlay, or remain two identities as designed?
- **Risk — key discoverability:** a user must know a template's exact formal name
  to key it. Mitigation to weigh: does `templates list` (or a `--formal` flag)
  already surface formal names, and should the docs point there?
- **Risk — silent no-match:** a mistyped formal-name key is inert (no error),
  which is safe but can hide a typo. Acceptable, or should an unknown key that
  matches nothing across a run be worth a lint later?

## Next implementation step

Add `Config.template_defaults` with its layer merge in `config::load` and the
`resolved_defaults` function in `interview.rs` (with `configured_defaults`
delegating to the shared `project` helper), then rewire the five call sites — the
smallest change that makes B1 and B2 pass together.
