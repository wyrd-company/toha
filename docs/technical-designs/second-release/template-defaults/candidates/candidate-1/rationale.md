# Rationale — template-specific configured defaults (candidate 1)

## Problem

Toha's `defaults` maps a question **id** to a value that overrides that
question's default in **every** template asking the id (`src/config.rs:26`,
`configured_defaults` `src/interview.rs:677`, `configuration.md:64-80`). The
product owner rejected this: a config author and a template author cannot assume
that two questions sharing an id (`title`, `email`, `owner`) mean the same thing,
so applying one value by id couples independent authors through an accidental
namespace. The replacement must let a value be **reused deliberately** across
questions without any implicit id match, while honoring the constraints Phase A
traced: the pure engine consumes a flat `Seed.defaults: IndexMap<Id, RawAnswer>`
and must stay identity- and store-unaware (`src/interview.rs:37-40`); every
driver funnels through the one `configured_defaults` seam
(`src/main.rs:422,496,664,902`); `ResolvedTemplate.formal_name` is the only
unambiguous identity (`src/cli/resolve.rs:17`, short names collide,
`registry.rs:485`); defaults are re-resolved against live config on resume, never
frozen (`src/main.rs:496,664,902`); and errors must name the config site
(`EvalError`/`CONFIGURED_DEFAULT`, `src/interview.rs:228`). No new trust,
timeout, permission, or subprocess surface.

## Usage (caller's view)

The config author names data once in `values`, then, per template *by formal
name*, points chosen questions at a stored value (`{ value: name }`) or an inline
literal. A default reaches a question only through such a mapping. The crate
consumer changes one call: `configured_defaults` now takes the selected
template's `formal_name`, the `values` store, and the `template_defaults`
mappings, and still returns the same flat `IndexMap<Id, RawAnswer>` the engine
already consumes. Below `Interview::start`, nothing changes. Full quickstart, the
four required examples with resolved outcomes, and the two error strings are in
`design.md`.

## Shape

**Two config structures, three key types, one seam.**

- `values: IndexMap<ValueName, Value>` — named literals. `ValueName` is a newtype
  (identifier pattern), distinct from `Id` and from formal-name `String`s, so the
  three namespaces cannot be confused in code.
- `template_defaults: IndexMap<String, IndexMap<Id, DefaultSource>>` — per formal
  name, per question id, a `DefaultSource`.
- `DefaultSource = Ref(ValueName) | Literal(Value)` — the load-bearing type. It
  is classified **once** at the config boundary by structure: a YAML/JSON object
  is a reference, everything else is a literal. Because toha answers are only
  scalars or string arrays — never objects — a mapping is *always* a reference
  and a bare value is *always* a literal. This is what makes a literal that
  spells a stored name (`license: primary_contact`) unambiguous: it is the string
  `primary_contact`, and no downstream code re-parses it (per
  `encode-lessons-in-structure`, `boundary-discipline`).

**Cycles are designed out, not detected.** `Ref` can appear only in
`template_defaults`; the `values` store holds literals. So a stored value can
never reference another — there is no cycle to find, and no detector to write or
get wrong. The type system carries the invariant.

**Data flows** config file → `config.rs` (parse+merge+classify, reject legacy
`defaults`) → `configured_defaults(formal, template, values, mappings)` → the
unchanged `Seed`. The seam picks `mappings[formal]`, and for each question the
template *actually defines* resolves the source (a missing `Ref` or a kind
mismatch is an attributed `EvalError` naming
`template-defaults."<formal>".<id>`), then type-checks with the existing
`parse_kind`. It validates only what is used: unreferenced store values and other
templates' mappings are untouched (per `laziness-protocol`).

**Interface depth.** The public surface grows by two `Config` fields and four
arguments to one existing function. Behind it sits identity selection, a
two-structure layered merge, reference resolution, per-kind type-checking, and
attributed errors — all pulled into the callee. Wire shapes (`{ value: … }`,
bare scalars) never cross the boundary; callers see domain types only (per
`boundary-discipline`, interface depth). The engine's public types are
byte-for-byte unchanged.

**State.** Config is read-only per invocation; both structures are derived from
the layer files by a deterministic system→user→local merge (single source of
truth per invariant; no sync). Because resolution runs at the seam against live
config, resume re-resolves and defaults are never frozen — the same property the
old design had, preserved.

**Encoded invariants.** reference-vs-literal (type, not convention);
no-cycles (store type admits no reference); three disjoint key namespaces
(`ValueName`/`Id`/formal `String`); attribution carries the formal name
(`EvalError.config_key`, the one additive field the id-only form could not
express).

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- **We accept that `template-defaults` keys are raw formal names** (including
  machine-specific absolute folder paths and full git addresses) in exchange for
  unambiguous identity selection. Keying on short name or alias would be friendlier
  to type but reintroduces exactly the ambiguity the feature exists to remove
  (`registry.rs:485`).
- **We accept verbosity — a mapping per (template, question)** in exchange for
  eliminating implicit coupling. Reuse is explicit (several questions point at one
  name); there is no "apply everywhere" shortcut, by design.
- **We accept a hard break for existing `defaults:` users** (reject-with-conversion)
  in exchange for no silent behavior change and no revival of global-by-id. Their
  data stays in their file; toha refuses until they convert.
- **We accept one new field on `EvalError`** (`config_key`) in exchange for
  attribution that names the formal name; the alternative (packing the formal name
  into `message`) would scatter the attribution format across call sites.
- **We accept classifying reference-vs-literal by structure** (object ⇒ reference)
  rather than by an explicit literal escape, in exchange for ergonomics — safe only
  because the answer domain has no object kind. Documented as an invariant so a
  future object-valued answer kind would force a revisit.

## Alternatives considered

- **Explicit tags on both sides** (`{ ref: name }` vs `{ literal: value }`,
  requiring the tag even for literals). Deeper symmetry, but it taxes every
  literal — the common case — with ceremony, and exposes more surface than the
  domain needs. Rejected: structural classification is airtight here because
  answers are never objects.
- **YAML custom tags** (`!value owner`). Maximally explicit, but they do not
  survive cleanly through `serde_norway` → JSON-model schema validation
  (`config.rs:70-86` validates the JSON value), and would need bespoke parsing.
  Rejected: fights the existing boundary machinery.
- **Resolve references into literals at config load** (flatten the store into each
  mapping eagerly). Simpler seam, but it validates *all* mappings at load, so a
  missing ref in an unused template's mapping errors before you touch that
  template — violating "validate only the value actually used." Rejected: lazy
  per-selected-template resolution keeps the blast radius to the template in hand.
- **One combined structure** (`template-defaults` values allowed to be either a
  literal or a nested store) with no separate `values`. Loses the named,
  cross-template reuse that is the whole point; you could not point two templates'
  questions at one shared value. Rejected on the primary requirement.
- **Store values may reference other store values.** Adds a cycle-detection
  burden and a resolution order for zero shown benefit. Rejected: designed out via
  the type (`Ref` only in mappings).

## Open questions and risks

- Should a mapping for a question id the selected template does **not** define be
  a warning (this design) or a hard error? Warning tolerates template drift;
  error catches typos sooner. Which does the product owner prefer?
- Should `template-defaults` keys ever accept an **alias** (resolved to a formal
  name at the seam, refusing ambiguity) for ergonomics, or stay strictly formal
  names? Allowing aliases couples config meaning to registry state.
- For a **bare folder** template whose formal name is a canonical absolute path,
  a `template-defaults` key must be that machine-specific path. Is that acceptable,
  or should folder-selected templates be out of scope for mappings?
- Migration: is a hard reject (Option A) acceptable for the 0.2.0 audience, or is
  a one-release deprecation warning (a softened Option A that still refuses to
  *apply* global-by-id) preferred to ease the transition?

## Next implementation step

Add `ValueName`, `DefaultSource`, and the `values` / `template_defaults` fields
to `config.rs` with the layered merge and the legacy-`defaults` rejection, then
rewrite `interview::configured_defaults` to the new signature and update the four
call sites — proving `no_implicit_by_id`, `reuse_by_name`, and
`missing_ref_is_attributed` first.
