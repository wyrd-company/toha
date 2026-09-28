# Rationale — Candidate 2 (ordered default rules)

## Problem

Toha's `defaults` maps a question id to a value that replaces the matching
question's `default` in *every* template that asks that id (`src/config.rs:26`,
`src/interview.rs:677`). Two templates that both ask `title` are forced to share
one value; there is no way to say "for template X, title = A; for template Y,
title = B." The design must add identity-scoped defaults while honoring hard
constraints traced in Phase A: the pure engine consumes a flat
`Seed.defaults: IndexMap<Id, RawAnswer>` and must stay identity-unaware
(`src/interview.rs:37`); results must be identical across terminal, headless,
staged, direct, and crate paths; defaults must re-resolve against live config on
resume, never freeze into `StagedRecord`; the only unambiguous identity across
folders, git addresses, installed names, aliases, and the reserved `toha-demo` is
`ResolvedTemplate.formal_name` (short names are not unique — `src/registry.rs:485`);
global-by-id must not be silently removed; and nothing may touch trust.

## Usage (caller's view)

Global stays the `defaults` map. Template-specific is a new ordered list,
`template-defaults`, of `{ match, set }` rules. `match` is the same `<TEMPLATE>`
string the command line already accepts (installed name/alias, `gh:owner/collection`,
`./x`, `/x`, `toha-demo`); `set` is an id→value map.

```yaml
defaults:
  title: Untitled
template-defaults:
  - match: gh:owner/collection
    set: { title: Field Notes }
  - match: gh:owner/journal
    set: { title: Daily Log }
```

`toha apply gh:owner/collection ./out` seeds `title = Field Notes`;
`gh:owner/journal` seeds `Daily Log`; any other template seeds the global
`Untitled`. Crate callers are untouched — they still build
`Seed { now, defaults }` themselves.

## Shape

Data structures first. The global tier is unchanged: `Config.defaults:
IndexMap<Id, Value>`, already merged local→user→system. The new tier is
`Config.default_rules: Vec<Rule>`, where `Rule { selector: String, origin, values:
IndexMap<Id, Value> }` is parsed at the config boundary with ids resolved to `Id`
and the selector kept as a raw domain string (per boundary-discipline — no wire
type crosses inward). The vector is stored in ascending precedence order
(system, user, local; each in file order), so no sort is ever needed.

The load-bearing decision is that **global stays a map, not a list entry.** The
runner's lead hypothesis wanted one uniform list with global as `match: any`. I
rejected that: `config::load` has already collapsed the per-layer global values
into one merged map (`src/config.rs:151`), so making global a list entry would
force those per-layer tiers to be reconstructed to place `any` correctly in the
precedence. Keeping global as the existing map means `Config.defaults` keeps its
exact type (zero breakage for readers and crate callers) and the precedence fold
becomes trivial: since specificity dominates layer, every template-specific rule
outranks the whole global map, so `scoped_defaults` just clones the global map and
overwrites it with matching rules applied in `(origin, order)` order — last write
wins. That single fold *is* the total precedence `(specificity, origin, order)`.

Interface depth: the public surface is one function,
`scoped_defaults(target, config, registry, dirs, cwd) -> IndexMap<Id, Value>`. It
hides the entire feature — selector normalization, the four-way match decision,
layer/order precedence — behind a call that returns the *exact type the system
already flows into `configured_defaults`* (`src/interview.rs:679`). The engine,
`Seed`, `staging`, `Template`, and `Plan` gain nothing; even
`configured_defaults` keeps its signature. Selector normalization reuses the
existing no-fetch `formal_name` (`src/cli/resolve.rs:209`), so the feature adds
one small function and four one-line call-site edits, not a new subsystem (per
laziness-protocol / minimize-reader-load: the flow is config → resolve →
existing engine, three files).

Invariants encoded in structure: the engine stays identity-unaware because
identity is resolved and flattened away *before* it (the flat map is the only
thing it sees). Determinism is structural — precedence is a lexicographic key
with a per-layer-unique `order` component, so it is total by construction, not by
runtime tie-break. Validation lives at boundaries: the schema catches selector
shape and `set`-id shape; `selector_matches` decides resolvability against the
live registry; `configured_defaults` validates value types exactly as today.

What it deliberately does not do: no `match: any` in the list (global is the map);
no glob/tag/profile matching (exact identity only); no engine-facing provenance
enrichment (rule-sourced values are still attributed `configuration key
defaults.<id>`, preserving the exact error contract at zero engine cost).

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept **two syntaxes** (map for global, list for template-specific) in
  exchange for keeping `Config.defaults` unchanged, a trivially-total precedence
  fold, and zero engine/crate breakage. A future migration can present the list
  as canonical with the map as sugar without changing runtime.
- We accept **specificity dominating layer** (a system template-specific rule
  beats a local global default) in exchange for "if you named a template, you
  meant it" and a CSS-learnable model; a local user overrides by being equally
  specific (a local rule for the same template).
- We accept **error attribution that still says `defaults.<id>`** for
  rule-sourced values in exchange for not changing `EvalError`/the engine;
  enrichment is deferred, not designed out.
- We accept **selector normalization touching disk/registry per rule per run** in
  exchange for reusing `formal_name` verbatim; rule counts are config-sized, so
  the cost is negligible and no cache is warranted (if the access pattern were
  hot, that would signal the structure is wrong — it is not).
- We accept **skipping unresolvable selectors with a warning** rather than
  erroring, so a rule for an uninstalled or absent template never breaks an
  unrelated run; the one hard error is reserved for genuine undecidability about
  the running target.

## Alternatives considered

- **Overlay map keyed by formal name** (`defaults-for: { "gh:owner/collection":
  {title: A} }`). Simpler to validate and read, but it loses ordering entirely,
  cannot express same-target tie-breaking, forces the author to write the exact
  formal name (no alias/folder/`./x` selector reuse), and couples the config key
  to the formal-name string form. The rule list hides selector classification
  behind the same grammar the CLI already uses and leaves room for a future glob
  tier. This is the design this candidate is deliberately different from.
- **Uniform list with `match: any` for global** (the runner's lead hypothesis).
  More elegant on paper, but it requires reconstructing per-layer global tiers
  that `config::load` already merged away, changes `Config.defaults`'s type
  (breaking readers/crate callers), and complicates the precedence fold for no
  behavioral gain. Rejected; the map+list hybrid is the stronger shape here.
- **Freeze resolved defaults into `StagedRecord`.** Would make resume trivially
  deterministic but violates the traced invariant that defaults re-resolve
  against live config on resume (`src/main.rs:664,902`); rejected outright.
- **Layer-major precedence** (local beats system regardless of specificity).
  Matches the current global merge's mental model but makes a local global
  silently shadow a deliberate system template-specific policy, and gives no way
  to say "this system rule is more specific than that local blanket default."
  Rejected in favor of specificity-major, with layer as the second key.

## Open questions and risks

- Should a value that comes from a `template-defaults` rule be attributed as its
  rule/selector (e.g. `template-defaults[match=gh:owner/collection].<id>`) rather
  than `defaults.<id>`? Doing so changes `EvalError`/`configured_defaults` — is
  that engine-facing change worth better diagnostics in this slice, or deferred?
- Is **specificity-major** precedence the intended policy, or should the local
  layer always win (layer-major) to match the current global-merge intuition?
  This is the one-way-door decision; the whole fold depends on it.
- For a `defaults` global-by-id disposition, is **keep-and-augment** (recommended,
  no narrowing) acceptable, or do you want the list presented as canonical with
  the map as documented sugar (soft migration), or the map removed entirely
  (breaking, requires your explicit approval)?
- Should an unresolvable selector stay a non-fatal warning (recommended), or would
  you rather it be a hard config error so typos surface immediately at the cost of
  breaking unrelated runs?
- Confirm `formal_name = "toha-demo"` + `commit = <content digest>` is the
  coordination surface a rule keys on for the bundled demo (consumed from the
  approved predecessor design).

## Next implementation step

Add `Rule`/`Origin` and `Config.default_rules` with the `config::load` parse
block, and write the `merges_layers` test's rule-list analogue proving
system/user/local rules land in ascending precedence order — the parse boundary
is the foundation everything else folds onto.
