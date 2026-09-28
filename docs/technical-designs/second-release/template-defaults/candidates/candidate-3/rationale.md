# Rationale — qualified selector keys for template-specific defaults

## Problem

Toha's `defaults` maps a question id to a value that replaces the matching
question's `default` in *every* template that asks that id. Two templates that
both ask `title` are forced to share one default. We need to scope a default to a
template **by identity**. The shape is non-obvious because the safe-looking
selector — the template's short `name` — is not unique across templates
(`src/registry.rs:485` treats a shared short name as `ResolveError::Ambiguous`),
and an alias can spell the same string as a formal name. The constraints that
crossed our boundary: the pure engine consumes a flat `Seed.defaults:
IndexMap<Id, RawAnswer>` and must stay identity-unaware (`src/interview.rs:37`);
identity is already resolved to `ResolvedTemplate.formal_name` at the boundary
(`src/cli/resolve.rs:17`); defaults are re-resolved against live config on resume,
never frozen (`src/main.rs:496,664,902`); the global-by-id capability must survive
(feature constraint); and the `configuration key defaults.<id>` error attribution
must be preserved (`src/interview.rs:228-232`).

## Usage (caller's view)

The config author writes a `template-defaults:` block whose keys *say what kind of
selector they are*: `formal:gh:owner/collection`, `alias:my-notes`, `name:notes`,
`path:./templates/report`. The plain `defaults:` block is unchanged. When a
resolved template matches several keys, the more precise key wins
(formal > path > alias > name > global) and, at equal precision, the more local
file wins. Crate callers see nothing new — they still hand the engine a flat
`IndexMap<Id, RawAnswer>`. The full quickstart, two config files, and three call
sites (two templates sharing `title`; the bundled `toha-demo`; a `name:` broadcast
with a `formal:` override) are in `design.md`.

## Shape

Data first. A `template-defaults` key parses **once at the config boundary** into a
`Selector` enum — `Formal | Alias | Name | Path` — where the *variant is the
selector kind*. This is the load-bearing decision: the kind can never be
reinterpreted downstream, so an alias/formal string collision or a "did I mean the
short name or the formal name?" mistake is unrepresentable
(per `encode-lessons-in-structure`). Blocks are held as `Vec<TemplateDefault>`,
each carrying its `ConfigLayer`, because one resolved template legitimately matches
several keys and precedence needs every match with its layer — a selector-keyed map
would force re-deriving the kind on every read (the "add an index later" smell the
runner-prompt flags).

Flow. At the boundary, `TemplateIdentity::of(&resolved, &registry)` derives the
identity view (formal name, short name, aliases, folder) where the registry is
already in hand — `ResolvedTemplate` gains no field, honoring the bundled-demo
coordination contract. `resolve_defaults(template, identity, config)` then, for
each question id the template defines, gathers the global candidate plus every
matching qualified block, ranks by `(specificity, layer)`, validates only the
winning value against the question's answer type, and returns the flat map. All
identity is flattened away here; the engine is untouched (`boundary-discipline`).
The only engine change is *extracting* a pure `validate_default` helper it already
contained, so both the global and qualified paths validate the same way and the
existing global error text is preserved (single source of truth per invariant).

Interface depth. The public surface is one config field, one enum, one function,
and one derived view. Behind it sits the whole precedence lattice, the
match-not-resolve semantics, and per-id winner selection. Crucially, because
config keys are *matched* against an already-resolved identity rather than
*resolved through* `registry.resolve`, the entire registry-ambiguity path
(exit 5) is unreachable from configured defaults — a large class of failure is
designed out, not guarded against (`make-operations-idempotent`: the projection is
a pure function of its inputs and re-runs identically on resume). What stays
exposed to callers is the qualifier vocabulary and the precedence rule — the
irreducible spec — and nothing more.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- **We accept a more verbose config (qualified keys) in exchange for making
  selector-kind mistakes unrepresentable.** `formal:gh:owner/collection` is longer
  than a bare `gh:owner/collection`, but the prefix is exactly what removes the
  alias/formal and short-name ambiguities.
- **We accept specificity-primary precedence in exchange for honoring stated
  intent** — a precise `formal:` key overrides a broad global even from a lower
  layer. This deliberately departs from the "local always wins" mental model that
  `defaults`/`hosts` use today; a future reader might mistake it for an oversight,
  so it is called out as decision D-PRECEDENCE with the alternative encoded.
- **We accept that `alias:`/`name:` are registry-relative** (their meaning tracks
  registry state at resolve time) in exchange for ergonomics. Only `formal:`/`path:`
  are truly identity-stable. The qualifier kind advertises this contract, so the
  cost is visible in the key itself rather than hidden.
- **We accept a `name:` broadcast** (matches every template with that short name)
  rather than refusing short names outright, in exchange for a low-ceremony way to
  set a default across a family of same-named templates; its low specificity means
  any `formal:` key overrides it per identity.
- **We accept validating only the winning value per id**, in exchange for not
  raising spurious type errors from shadowed defaults the run never uses.

## Alternatives considered

- **Single overlay map keyed only by formal name** (`template-defaults:
  { gh:owner/collection: {...} }`). Simpler surface, but it exposes the collision
  we are trying to remove: a bare key that *looks* like a formal name could be a
  short name or an alias, and the reader cannot tell. It hides no complexity that
  qualified keys hide, and it cannot address "the family of templates short-named
  `notes`" at all. Lost on interface depth: the ambiguity leaks to the caller.
- **Ordered rule list** (`[{ match: {...}, defaults: {...} }, ...]`, first match
  wins). Maximally flexible, but precedence becomes *positional* — the author
  hand-maintains order, and layering three files means concatenating three lists
  with unclear cross-file ordering. It exposes ordering complexity to the caller
  that our `(specificity, layer)` total order hides. Lost on "single source of
  truth per invariant": precedence would live in list position, not in a rule.
- **Resolve qualified keys to formal names at load, then collapse to a
  formal-only overlay.** Keeps precedence to formal-vs-global, but it (a) needs the
  registry at config-load time, which `config::load` deliberately does not have,
  and (b) discards the kind distinction that gives `alias:`/`name:` their
  registry-relative meaning and the `name:` broadcast. Lost on separation of
  concerns: it drags registry state into the config boundary.

## Open questions and risks

- Should precedence be **specificity-primary** (recommended) or **layer-primary**?
  Specificity-primary lets a system-level `formal:` key beat a project-level global
  default — intended, but it is the one place this design contradicts the existing
  "local wins" intuition. Which mental model do you want to hold across all config?
- Do you want all four qualifiers now, or the identity-stable pair
  (`formal:`/`path:`) first, adding `alias:`/`name:` once the stability contrast is
  documented?
- Is refusing a same-layer `alias:` tie (rather than last-in-file-wins) the
  behavior you want, given it is the only case the merge cannot silently total?
- Should a `name:` key that matches *nothing installed* stay silently inert, or do
  you want a `config lint` follow-up that reports dead qualified keys?
- Is `configuration key template-defaults["formal:gh:owner/x"].<id>` the attribution
  wording you want, alongside the unchanged `configuration key defaults.<id>`?

## Next implementation step

Add `Selector`, `TemplateDefault`, and the `template_defaults` fold to
`src/config.rs` behind the extended `Layer` struct, with unit tests for qualifier
parsing and per-layer id validation — the boundary parse everything else derives
from.
