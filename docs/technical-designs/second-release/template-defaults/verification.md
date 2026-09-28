# Verification — template-specific configured defaults

Phase F. The synthesized design held under the same scrutiny as any candidate:
caller usage re-checked against the sketch, every requirement and inherited
constraint checked against the design, failure cases enumerated, and compatibility
with the approved predecessor confirmed.

## Caller usage vs the sketch

The three call sites in `design.md` were traced through the signatures:

- Call site A (two identities, one id, two defaults) resolves through
  `resolve_defaults(template, identity, config)` → per-id winner is the scoped
  `Identity` candidate over the `Global` candidate → flat `IndexMap<Id, RawAnswer>`
  → `Seed`. Matches B1.
- Call site B (specificity beats layer, re-resolved on resume) traces through the
  `continue`/`apply` sites (`src/main.rs:664,902`) which re-derive the identity from
  `StagedRecord.template` and re-read live config. Matches B5, B9.
- Call site C (crate) uses the unchanged `Seed`/`Interview::start`; the resolver is
  binary-only. Matches B14.

Usage and sketch agree; no reconciliation needed.

## Requirements coverage (task Decisions and scope, Definition of Done)

| Requirement | Where satisfied | Proof |
| --- | --- | --- |
| Unambiguous selectors across folder/git/formal/alias/duplicate short/bundled | design "Selectors" table; `Selector::Formal` matched by identity equality | B1, B8, B10 |
| Two templates sharing a title get separate defaults | distinct `formal_name` ⇒ distinct keys | B1, B8 |
| System/user/local merge + precedence across terminal/headless/staged/direct | "Precedence" (total `(specificity, layer)`); all paths call `resolve_defaults` | B2, B4, B5, B11 |
| Global-by-id preserved unless explicitly changed; options + recommendation | D3 (keep unchanged, recommended); B2 | B2 |
| ≥2 broader reusable-default mechanisms compared; future direction | "Alternatives explored" table on all five axes; glob recommended | — |
| Schema/config/guide/embedded-guidance proposals + validation | "Proposed contract edits"; error contract | B7, B12, B13 |
| Interfaces, data shapes, failure behavior, compatibility, canonical-doc impact, falsifiable validation explicit | design body + this file | B1–B14 |
| Keep pure engine + all driver/crate paths | engine untouched; `validate_default` is a behavior-preserving extraction | B11, B14 |
| No production stubs / premature shared schema edits | all sketches are `unimplemented!("design sketch")`; edits described only | — |

## Inherited feature constraints

- **Keyed by identity, not question id or ambiguous short name** — keys are
  `formal_name`; short name is explicitly rejected as a key (B8). ✅
- **No silent removal/narrowing/reinterpretation of global-by-id** — D3 recommends
  keeping it unchanged; any narrowing is flagged as requiring explicit approval. ✅
- **No assumed global default model until alternatives compared** — four mechanisms
  compared before recommending a direction; none forced now. ✅

## Failure cases and falsifiable scenarios

The error/results table maps each situation to a defined outcome; B1–B14 make each
load-bearing behavior falsifiable, including the two sole-kill-style guards a
reviewer will want: B5 pins the precedence direction (removing the specificity-major
rule flips B5), and B8 pins identity keying (keying on short name fails B8). B13
pins "validate only the winner"; B14 pins engine purity.

## Compatibility with the approved predecessor

Consumed revision: bundled-demo design `f949b6a5152904b234bd4f21b7e05aaa843d42e8`,
with Bob's approval of D1–D4 and the downstream identity `formal_name="toha-demo"` +
`commit=<canonical content digest>`.

- This design keys the bundled demo by its formal name `toha-demo` (B10) — exactly
  the stable identity the predecessor fixed. ✅
- `ResolvedTemplate` gains no field or variant (the predecessor's stated invariant);
  `TemplateIdentity` is derived, not stored. ✅
- The predecessor's reserved-fallback precedence (an installed/aliased/discovered
  `toha-demo` wins over the bundled demo) is upstream of this design: by the time
  `resolve_defaults` runs, `formal_name` already reflects whichever `toha-demo`
  resolved, and the scoped key `"toha-demo"` matches that resolved identity. No
  conflict. ✅
- `commit` is deliberately excluded from the default key; the interaction with the
  predecessor's content-addressed `commit` is the subject of decision D4, surfaced
  (not silently resolved). ✅

## Residual risks carried to the checkpoint

- **D1 precedence direction** is a low-reversibility choice (changes resolved
  defaults for existing configs once shipped) — put to Bob, recommended
  specificity-primary, pinned by B5 either way.
- **D4 identity-key versioning** (ref-sensitive key) is the cross-judge-flagged risk;
  surfaced with a recommendation and documentation plan, not silently decided.
- **Cross-family judging was unavailable** this run (gpt-5.6 runners dropped out);
  the base pick and grafts rest on one model family's scoring plus the orchestrator's
  independent read, which agreed exactly. Recorded as a run limitation.

## Verdict

The synthesized design resolves the task's decisions, preserves the pair's inherited
acceptance criteria and constraints, keeps the engine pure, and is compatible with
the approved predecessor. It holds. Remaining open items are product decisions for
the Phase C checkpoint (D1–D4), not design defects.
