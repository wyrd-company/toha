# Synthesis — template-specific configured defaults (named stored values)

Records the base pick, the orchestrator's own criterion scoring, the reconciliation
with the cross-judge, grafts and rejections, dropouts, and the risks elevated to
the Phase C checkpoint. This is the reframed round: the implicit global-by-id model
is replaced by named stored values plus explicit references, per the product
owner's direction. The prior global-by-id design and its arena are superseded and
preserved in git history at commit `37e2215`.

## Approval and naming delta

Bob approved this design at revision `24b6301` with **one authorized naming
revision** and all D1–D5 recommendations accepted. The delta, applied faithfully
across the authoritative contract (`design.md` and schema/guide/error/example
text) without re-running the arena:

- The store property `values` → **`presets`**.
- The reference form `{ value: <name> }` → **`{ preset: <name> }`**.
- Derived identifiers and prose: `ValueName` → `PresetName`; schema `value-name` →
  `preset-name` and `values` def → `presets`; the reference schema key `value` →
  `preset`; error text `no stored value named` → `no preset named`; provenance
  `→ values."<name>"` → `→ presets."<name>"`.

The arena evaluated the store under the working name `values`; the candidate
evidence under `candidates/` is preserved as-evaluated (it retains `values`), while
the chosen final contract is `presets` / `{ preset: <name> }`. This is a rename
only — no shape, precedence, representation, or behavior changed.

## Runners and dropouts

Configured intent: four `inherit-parent` architect runner slots plus a cross-judge
preferring a different model family. The gpt-5.6 family remained unavailable this
run (terminal model-access error, as in the prior round), so all three candidates
and the cross-judge ran on `inherit-parent` (Claude). Three structurally distinct
viable candidates were produced (≥2 satisfied). Lost cross-family diversity is a
run limitation, recorded here.

Three distinct directions were explored:

- **C1** — a `values` store plus a `template-defaults` map keyed by formal identity,
  each entry a `{ value: <name> }` reference or an inline literal (object ⇒
  reference, scalar/array ⇒ literal; refs may appear only in mappings).
- **C2** — a `values` store plus an ordered binding **list**, reference vs literal
  in distinct keys (`value:` / `literal:`), selectors matched through the registry.
- **C3** — a `values` library plus a `template-defaults` map using `${name}` sigil
  references with a `$$` escape, values may chain, resolved by a cycle-detecting
  graph resolver.

## Own scores vs cross-judge

Scored independently, criterion by criterion, after reading all three end to end.

| Criterion | C1 | C2 | C3 |
| --- | ---: | ---: | ---: |
| C1 No implicit id coupling; refs explicit | 5 | 4 | 5 |
| C2 Ref/literal representation unambiguous & named | 4 | 5 | 3 |
| C3 Resolution safety (missing/typing/cycles) | 5 | 5 | 4 |
| C4 Layering, engine purity, path parity | 5 | 4 | 5 |
| C5 Safe transition of `defaults:` | 5 | 5 | 4 |
| C6 Interface depth, contained surface, future | 5 | 4 | 3 |
| **Total** | **29** | **27** | **24** |

The orchestrator's totals match the cross-judge exactly, including the ranking
C1 > C2 > C3. No numeric disagreement to reconcile. Full agreement on base = C1.

## Base

**Candidate 1 — a `values` store plus an identity-keyed `template-defaults` map.**
Chosen for: strictest identity keying (exact `formal_name`, no registry-coupled
selector); **cycles impossible by construction** (references appear only in
mappings and resolve one hop into a literal store — no detector needed and none can
be wrong); resolution kept in the `toha` library so every driver and crate path is
served identically; and the smallest deep surface (two config fields; the engine's
`Seed.defaults` contract and the pipeline below it unchanged in behavior).

## Grafts (source → what → why)

- **From C2 — precise, file-naming attribution.** C1's attribution names
  `template-defaults."<formal>".<id>` but not the config **file**. Graft C2's
  `Origin{file, layer}` so every configured-default error names the file (and, for
  the binding case, its position). A user editing three layered config files must
  be told which one to fix.
- **From C3 — reference provenance in errors.** When a value reaches a question
  *through a reference* and fails the kind check, name both the mapping site and the
  value it resolved through: `template-defaults."<formal>".<id> → values."<name>":
  <message>`. The author fixes the stored value, not the mapping.
- **Considered from C2 — the distinct `value:` / `literal:` keys.** Adopted as a
  presented alternative (decision D2), not folded in by default: C1's `{ value:
  <name> }`-or-bare-literal form is already airtight at the schema level (the
  literal branch of the `oneOf` admits only scalars and string arrays, so an object
  can only be a reference and a bare string is always a literal), and it keeps
  literals terse. The distinct-key form is more explicit and future-proofs against
  object-valued answers; Bob chooses (D2).

## Rejections (what was dropped and why)

- **C3's `${name}` sigil + `$$` escape (rejected).** A sigil re-opens exactly the
  literal-vs-reference collision the grounding lists under *Avoid* and then patches
  it by shape-sniffing a string. A structural distinction (object ⇒ reference, or
  distinct keys) is safer and needs no escape rule.
- **C3's value→value chaining + graph/cycle resolver (rejected).** Chaining is a
  capability the reframe never asks for, and cyclic/unbounded resolution is listed
  under *Avoid*. Designing cycles out by construction (refs only in mappings, one
  hop) is strictly stronger than detecting them.
- **C2's binding-list structure as the base (rejected).** One list row per
  (template, question) is more verbose than a nested `identity → {question:
  source}` map for the common case; the nested map is the terser deep surface.
- **C2's registry-resolved alias/short selectors (rejected).** Resolving a selector
  through `registry.resolve` at match time couples a config file's meaning to
  registry state and re-admits the exit-5 ambiguity surface (guarded, but present).
  Keep C1's exact `formal_name` keying; aliases/short names are not stable identity.
- **C2's resolution-in-CLI-module (rejected in favor of C1's library placement).**
  Moving resolution into `src/cli/defaults.rs` leaves crate/library consumers
  without selection; C1 keeps resolution in the `toha` library so the crate path is
  served.

## Convergence signal

All three candidates independently converged on: (a) a top-level `values` store
named exactly `values` (not `defaults`); (b) template selection keyed on the stable
`formal_name`, never a short name; (c) no implicit application by id — a value
reaches a question only via an explicit mapping; (d) resolution at the boundary
flattening to the unchanged `Seed.defaults`; (e) re-resolution on resume, never
frozen; (f) **reject-with-conversion** as the recommended migration for existing
`defaults:`, with auto-translation judged semantically impossible (global-by-id
carries no template identity to translate into). This strong convergence is carried
into the final design as settled shape.

## Risks elevated to the checkpoint

From the cross-judge, missed by all three candidates:

1. **Same-answer-kind reuse limit** — a stored value has one kind (its JSON shape),
   so it can seed only questions of that answer kind. Documented as an inherent
   property; folded into decision **D5** and the docs.
2. **Migration naming trap** — authors will name `values` entries after question
   ids and expect implicit application. Folded into the migration section and the
   `configuration.md` guardrail; part of decision **D1**.
3. **Eager validation of unreachable questions** — validating a mapping whose
   question the template *defines* even if unreachable via `when`/batch. Consistent
   with today's `configured_defaults`; surfaced as decision **D5** (validate-when-
   defined vs validate-when-reached; recommend when-defined).

## Verification

See `verification.md`: caller usage re-checked against the sketch, every reframed
requirement and constraint checked, failure/falsifiable scenarios listed, and
compatibility with the approved bundled-demo predecessor confirmed.
