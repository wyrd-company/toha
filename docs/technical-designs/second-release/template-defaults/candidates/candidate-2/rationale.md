# Rationale — explicit binding list

## Problem

Toha's `defaults` maps a question id to a value that silently overrides that
question's default in *every* template asking the id (`src/config.rs:26`,
`configured_defaults` `src/interview.rs:677`, `configuration.md:64-80`). Two
authors who both name a question `email`, `owner`, or `title` are coupled through
an accidental namespace. The product owner rejects this: no value may reach a
question implicitly by id. The replacement must let a config author (a) keep
reusable literals and (b) point specific template questions at them explicitly.

The shape is constrained by seams that must not move. The pure engine consumes a
flat `Seed.defaults: IndexMap<Id, RawAnswer>` (`src/interview.rs:39`), keyed by
the selected template's question ids, and must stay identity- and store-unaware;
resolution happens *before* it and flattens away the template
(`grounding.md`). Every driver — terminal, headless, staged, direct, crate —
funnels through the same `configured_defaults` chokepoint, and staged interviews
re-derive defaults from *live* config on resume, never frozen (`src/main.rs:496,
664, 902`). The only unambiguous template identity is `formal_name`; short names
collide by design (`src/registry.rs:485`). Errors must name the config site
(`src/interview.rs:230`).

## Usage (caller's view)

A config has two independent parts. `values:` is a store of named literals the
author names. `template-defaults:` is an ordered list of bindings; each names a
template identity, a question, and a source in one of two distinct keys —
`value:` (reference a stored value) or `literal:` (inline). A value reaches a
question only because a binding says so.

```yaml
values:
  primary_contact: contact@example.invalid
template-defaults:
  - template: gh:owner/newsletter   # asks `email`
    question: email
    value: primary_contact
  - template: gh:owner/press-kit    # asks `contact`
    question: contact
    value: primary_contact          # one value, two differently-named questions
  - template: toha-demo
    question: topic
    literal: Sample Topic           # a literal, distinguished by the key
```

`toha apply gh:owner/newsletter ./out` seeds `email` with the stored value;
`toha apply gh:owner/press-kit ./out` seeds `contact` with the same value; a
template that merely also asks `email` is untouched. Crate callers are
unaffected — they still build `Seed { now, defaults }` and call
`Interview::start`; the store/binding feature is CLI-only, like the bundled demo.

## Shape

Data first. `Config.defaults` is replaced by two fields:
`values: IndexMap<ValueName, Value>` (the store) and
`template_defaults: Vec<Binding>` (the ordered bindings). A `Binding` carries a
raw `selector`, a `question: Id`, a `source: Source`, and an `Origin`
(file + index). `Source` is an enum — `Ref(ValueName)` or `Literal(Value)` —
and *that enum is the whole anti-ambiguity mechanism*: reference vs literal is
the YAML key that was present (`value:` vs `literal:`), parsed once into two
variants, so a literal that spells a value name lands in `Literal` and is never
misread (`per encode-lessons-in-structure`). No sigil, no shape sniffing.

The dominant access pattern — "for this selected identity, what are the seeded
defaults?" — traces cleanly: `config::load` merges layers into `values` and the
concatenated `template_defaults`; `cli::defaults::for_template` filters bindings
whose selector resolves to `resolved.formal_name`, resolves each `Source`
against the merged store, and produces the same `IndexMap<Id, Value>` the old
`config.defaults` was; the unchanged `configured_defaults` kind-checks it into
`IndexMap<Id, RawAnswer>`. No structure needs a later index or cache: the
binding list is walked once in precedence order, last-wins into an `IndexMap`
keyed by question id.

Load-bearing decisions:

- **The engine seam is preserved byte-for-byte.** `configured_defaults`, `Seed`,
  `replay_with_defaults`, and staging are untouched (`per boundary-discipline`).
  All new behavior is a CLI-side projection that produces the map the engine
  already eats. Type-mismatch attribution reuses the engine's own `parse_kind`
  message, re-wrapped with the binding site.
- **Identity, not name.** Selectors match `formal_name`, resolving git/host
  addresses purely, folders by canonical path, and installed names via
  `registry.resolve`; ambiguous shorts are refused *only when they could mean
  the running identity*, so an unrelated run never breaks
  (`per separate-before-serializing-shared-state`: irrelevant layers stay inert).
- **Cycles designed out.** The store holds literals; a value never references a
  value. `Source::Ref` points into a `Value` store, not another `Source`, so
  resolution is one hop and no cycle can exist. Encoded in types, not checked at
  runtime.
- **Validate only what is used.** Filter-then-resolve means a broken literal or
  dangling ref in a binding for another template, or an unreferenced store
  value, is never validated.

Interface depth: the public surface is two config fields plus one CLI function
(`for_template`) with a five-argument context it genuinely needs (config,
resolved identity, template, registry, dirs, cwd). Behind it sits selector
classification, registry resolution, layer precedence, store lookup, kind
validation, and attributed errors. The library engine's surface does not grow at
all. The verbosity the design *exposes* is in the config file, not the code — a
deliberate trade (below).

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- **We accept per-binding verbosity in exchange for zero ambiguity and explicit
  ordering.** A table row per (template, question) is wordier than a nested
  `template → {question: value}` map, but it makes reference-vs-literal a key
  choice, makes override order literal (a later row wins), and reads like the
  table it is. A nested map would need a tagged scalar to mark references,
  re-opening the "is this string a name or a literal?" question the product
  owner's model exists to close.
- **We accept a hard cutover for `defaults:` (Option 1) in exchange for never
  silently changing behavior.** Auto-translation cannot preserve by-id meaning
  without a wildcard selector (the rejected model); a loud error with the exact
  conversion is safer than a quiet regression.
- **We accept that selector convenience forms (alias/short) resolve through the
  registry** in exchange for letting authors write the canonical formal name and
  get a pure string match. This looks like it could be simplified to
  "formal-name-only," but supporting the same selector vocabulary as the CLI is
  what makes the feature usable.
- **We accept that a binding for a not-installed or renamed template silently
  no-ops** in exchange for unrelated runs never breaking. Only ambiguity that
  targets the running identity is fatal.

## Alternatives considered

- **Nested map with tagged references** (`template-defaults: {<selector>:
  {<question>: <source>}}`, a scalar sigil like `$name` marking a reference).
  Loses because the sigil re-introduces literal/reference ambiguity (a literal
  answer that starts with `$`, or a value whose text looks like a reference),
  and a map cannot express intra-layer override order — the exact two properties
  the binding-list shape nails. It hides slightly less config text but exposes a
  subtler failure mode to every author.
- **Keep by-id `defaults`, add per-template overrides on top.** Loses on the
  hard requirement: it preserves the implicit coupling as a base tier, and forces
  a dual-precedence merge between global and per-template values.
- **Value chaining (values may reference values).** Loses on cost/benefit: it
  buys marginal reuse and pays with cycle detection, ordering questions, and a
  deeper resolver. Literal-only values delete the whole problem.
- **Resolve everything at `config::load`.** Loses because the registry is built
  *after* config load (`src/cli/resolve.rs:48-59`), so selector resolution can't
  happen there; projection must run per call site where the identity is known.

## Open questions and risks

- Is there a confirmed installed base of `defaults:` from 0.1.x? If yes, should
  the disposition be Option 2 (adopt as `values:` + warn) rather than Option 1
  (reject)? The recommendation flips on this.
- Should an ambiguous short-name selector that does *not* target the running
  identity stay a silent skip, or emit a one-time warning so authors learn their
  selector is unreliable? (Design currently: silent skip.)
- Should value names share the question-id pattern `^[a-z_][a-z0-9_]*$`, or use
  a visibly distinct pattern (e.g. allow `-`) to reinforce that they are not
  question ids? (Design currently: same pattern, distinct namespace.)
- Should a matched binding naming a question the template lacks be a hard error
  (current design) or a warning-and-skip, given the engine silently skips such
  ids today (`src/interview.rs:684`)?

## Next implementation step

Add `values` and `template_defaults` (with `Binding`/`Source`/`Origin`) to
`toha::config`, including the layer merge and the pre-schema `defaults:`
migration check, with unit tests over precedence and the migration error.
