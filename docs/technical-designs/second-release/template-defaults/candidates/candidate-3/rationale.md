# Rationale — value library with sigil references (candidate 3)

## Problem

Toha's current `defaults:` maps a question id to a value that replaces the
matching question's own default in *every* template that asks that id
(`src/config.rs:26`, `configured_defaults` `src/interview.rs:677`). The product
owner rejected this: two questions sharing an id (`title`, `email`, `owner`) are
not the same question, and applying one value to all of them couples independent
authors through an accidental namespace. The reshaped model must let a config
author name reusable data, then say *explicitly* which template question each
datum fills — with no implicit application by id. The design is constrained by a
seam I must not move: every driver path funnels one flat
`Seed.defaults: IndexMap<Id, RawAnswer>` into a pure engine that never learns
which template it is (`src/main.rs:414,496,664,902`; `src/staging.rs:131`).
Identity is resolved *before* the engine to a stable `formal_name`
(`src/cli/resolve.rs:16`), which is already in scope at every call site and is
the one selector unambiguous across folders, git addresses, installed names,
aliases, duplicate short names, and the bundled `toha-demo`. Defaults must
re-resolve against live config on resume, never freeze into the staged record,
and add no trust/permission/timeout/subprocess surface.

## Usage (caller's view)

The config author writes two blocks: `values:` (names they choose → data) and
`template-defaults:` (formal name → question id → `${name}` reference or inline
literal). Reuse is expressed by pointing several questions at one value. The CLI
is unchanged; the same `toha apply`/`stage`/`continue` commands behave
differently only because config now targets questions by identity. Crate callers
see no new surface — they still build `Seed { now, defaults }`. The four required
examples and their resolved outcomes are in `design.md`; the load-bearing usage
rules are: a string that is exactly `${name}` is a reference, everything else is
a literal, `$$` escapes a literal `${`, and keys are formal names (never
aliases/short names, so same-short-name templates stay separate).

## Shape

Data first. `values` is `IndexMap<String, StoredValue>` and `template-defaults`
is `IndexMap<String, IndexMap<Id, Mapping>>`; both a `StoredValue` and a
`Mapping` hold a `Binding` (`Literal | Ref`) plus a `Site` for attribution. The
ref-vs-literal decision is encoded in the `Binding` type and made exactly once,
in `Binding::parse` at the config boundary (per boundary-discipline and
encode-lessons-in-structure) — nothing downstream re-sniffs a string. The
dominant access pattern is "given the selected formal name, produce the flat
per-question defaults," and the structure serves it directly: index
`template-defaults[formal]`, then resolve each `Binding` against `values`. No
later index or cache is needed.

The public surface is two functions and a two-stage pipeline. Stage 1,
`resolve_template_defaults(formal, library, mappings)`, is the deep part: a graph
walk that chases `${ref} → value → ${ref} …`, refuses cycles with a `stack`,
reports missing names, and carries an `origin` string — all *template-unaware*,
so `progress`, `continue`, and `apply` call it identically with `saved.template`
or `resolved.formal_name`. Stage 2 is today's `configured_defaults`, narrowed to
consume the resolved flat map: it keeps ids the template defines, warns (not
fatals) on an unknown id so resume across template versions survives, and reuses
the existing `parse_kind`/`prompt_kind` to check value-kind against question-kind
at the single point a value reaches a question ("validate only the value actually
used"). The output is the unchanged `IndexMap<Id, RawAnswer>`; the engine,
`Seed`, `override_default`, and `StagedRecord` are untouched.

Interface depth: the public surface is small (two functions over plain config
types) but hides cycle detection, missing-ref attribution, layer-merge of both
structures, sigil parsing with escape, and kind checking. Transport (YAML) is
parsed into `Binding`/`Site` behind the boundary and never exposed. The single
invariant that JSON shape *is* answer kind lets me drop a declared `type:` on
values — the check that matters (value-kind vs question-kind) is derived at the
use site, so there is one source of truth, not two that can drift.

## Synthesis decision

Filled in by arena.

## Tradeoffs accepted

- We accept a **graph resolver with cycle detection** (a `stack`, a recursive
  walk) in exchange for chained, reusable values — a value defined once in terms
  of another. If chaining proves unused, the resolver collapses to a single
  lookup with no interface change.
- We accept a **sigil with an escape** (`${name}`, `$$` to escape) in exchange
  for an unambiguous whole-string ref/literal boundary. The escape burden falls
  only on literals that would exactly match the grammar — near-zero in practice.
- We accept **formal-name keys**, which are verbose for folder templates
  (absolute paths) and require the author to know the formal name, in exchange
  for identity that is stable across layers and unambiguous across selectors.
- We accept **rejecting legacy `defaults:` at load** (migration C) — a one-time
  hard error — in exchange for no silent behaviour change and an explicit target
  for every migrated value.
- We accept **whole-value references only** (no interpolation, no per-element
  refs) in exchange for preserving non-string kinds (a ref can resolve to a bool
  or a list) and a bounded resolver.
- We accept an **unknown mapped id being a warning, not an error**, in exchange
  for resume working when a template version drops a question; the risk is a typo
  going unnoticed, mitigated by the loud stderr warning.

## Alternatives considered

- **Explicit `type:` on each value** (the lead hypothesis's "typed entries").
  Rejected: answer kinds are exactly {string, bool, list-of-strings}, so a
  value's JSON shape already *is* its kind; a declaration is a second source that
  can disagree with the value (violates single-source-of-truth) and enlarges the
  schema. The kind check that carries information — value-kind vs *question*-kind
  — is derived at the use site instead. This is the deliberate divergence from
  the hypothesis.
- **Tagged-object reference** (`email: { ref: contact }` / `{ value: "x" }`).
  Deeper YAML, more nesting, and it exposes a wire discriminator on the authoring
  surface; the whole-string `${name}` reads as data and keeps mappings flat. It
  hides no more complexity than the sigil while costing every author more typing.
- **Keying mappings by alias or short name.** Rejected outright: aliases are a
  user-layer, remappable concept and short names are non-unique
  (`src/registry.rs:485-490`), reintroducing the ambiguity the reframe exists to
  remove and coupling config to registry state.
- **A flat binding list** (`[{template, question, value}]`). Loses the natural
  group-by-formal-name access pattern, forces a linear scan per resolution, and
  has no home for the shared library; the map-of-maps matches the query.

## Open questions and risks

- Should an unknown mapped question id stay a warning, or become an error when
  the mapping and template commit agree (e.g. only warn across versions)? A
  stricter default catches typos sooner but can break resume.
- Is the `$$` escape the right spelling, or would a rarer sigil (e.g. a leading
  `@`) reduce collisions further at the cost of familiarity? `@`-prefixed
  literals (handles, scoped packages) are common, which is why I chose the
  whole-string `${ }` form.
- For folder-template mappings, is an absolute-path key acceptable, or is a
  future `toha config resolve-name <selector>` helper worth it to let authors
  write the selector they use on the CLI?
- Should chained references be allowed at all, or would a single indirection
  (mapping → value, value is always a literal) cover every real case and let us
  drop cycle detection entirely?

## Next implementation step

Write `Binding::parse` and its unit tests (ref grammar, `$$` escape, non-string
literals), since it is the single point that decides ref-vs-literal and every
other piece depends on it being unambiguous.
