# Verification — template-specific configured defaults (named presets)

Phase F for the reframed design. Caller usage re-checked against the sketch, every
reframed requirement and constraint checked, failure/falsifiable scenarios listed,
and compatibility with the approved bundled-demo predecessor confirmed.

## Caller usage vs the sketch

The four required config examples were traced through the signatures:

- Example 1 (one preset, two differently-named questions) resolves through
  `configured_defaults(formal_name, template, presets, mappings)`: each identity's
  sourced mapping resolves `{ preset: primary_contact }` against the merged store →
  the same preset seeds `email` and `contact`. Matches B1.
- Example 2 (same-id, no mapping) — `gh:owner/blog` has no entry, so no store value
  reaches its `email`; the template's own default stands. Matches B2.
- Example 3 (`toha-demo`) — keyed by the reserved formal name; literal + reference
  both resolve. Matches B9.
- Example 4 (missing ref, type mismatch) — both produce attributed `EvalError`s
  led by the winning mapping file/site; the ref mismatch also names the winning
  preset file and `→ presets."<name>"`. Matches B4, B5.

Usage and sketch agree.

The caller-first crate example destructures `Resolution { defaults, warnings }`,
reports each warning, and passes only `defaults` to `Seed`. The four command-driver
sites use the same result contract, so neither defaults nor mapped-id warnings are
lost on a supported path (B12).

## Reframed requirements coverage (Bob's canonical instruction, 11:53)

| Requirement | Where satisfied | Proof |
| --- | --- | --- |
| No implicit override by field id | mappings are explicit; nothing applied by id | B2 |
| Equal ids ≠ equal semantics assumption removed | resolution keys on identity + explicit mapping only | B2, B8 |
| Named stored values, property NOT called `defaults` | `presets` store; `defaults` removed | design "Data structures"; B13 |
| Template defaults explicitly reference stored values | `DefaultSource::Ref` / `{ preset: <name> }` | B1, B3 |
| Reference/literal representation | `{ preset: <name> }` vs bare literal; schema `oneOf` (D2) | B3, B7 |
| Names (property + reference) | `presets` + `template-defaults`; `{ preset: }` (D3, closed) | design "Decisions" |
| Typing / missing / cycles | attributed errors; cycles impossible by construction (D4) | B4, B5, B7 |
| Layer precedence | store per-name, mappings per (formal,id); refs resolve vs merged store | B10 |
| Safe transition, no silent data loss | migration disposition D1 (reject-with-conversion) | B13 |
| Two differently-named questions reference one value | example 1 | B1 |
| Unrelated same-id question unaffected | example 2 | B2 |
| Reuse the bundled identity | `toha-demo` key, example 3 | B9 |
| Pure engine + all driver/crate paths | resolution in library; `Seed.defaults` unchanged | B12, B14 |
| No production stubs / premature schema edits | all sketches `unimplemented!`; edits described | — |

## Constraints (grounding Preserve/Change/Avoid)

- **Preserve** — `Seed.defaults` contract, path parity, re-resolution on resume,
  local-layer participation, formal-name identity, no trust surface, parse-once,
  attribution: all held. Winning mapping and preset origins survive merge in
  `ConfigEntry<T>` (B4, B5, B10–B12, B14; error contract). ✅
- **Change/Replace** — global-by-id removed; named store + explicit references
  introduced; names, representation, precedence, missing/type/cycle behavior all
  defined. ✅
- **Migrate without silent data loss** — D1 reject-with-conversion keeps data in the
  file and refuses to run until converted; B13 pins it. ✅
- **Avoid** — no implicit id application (B2); no ambiguous selector (identity
  equality, B8); no literal/reference misread (schema `oneOf`, B3/B7); no frozen
  defaults (B11); cycles impossible (B7); no new trust/permission/timeout/subprocess
  surface. ✅

## Risks (cross-judge) — disposition

- **Same-answer-kind reuse limit** — disclosed in D5 and to be documented; inherent
  to typed values, not a defect.
- **Migration naming trap** — addressed by a `configuration.md` guardrail steering
  `presets` names away from question ids, and by D1's reject-with-conversion (which
  forces explicit intent rather than a silent lift).
- **Eager validation of unreachable questions** — surfaced as D5 (validate-when-
  defined, consistent with today's behavior) rather than left implicit.

## Compatibility with the approved predecessor

Consumed revision: bundled-demo `f949b6a5152904b234bd4f21b7e05aaa843d42e8`
(Bob-approved D1–D4, identity `formal_name="toha-demo"` + content-digest `commit`).

- The bundled demo is addressed by its formal name `toha-demo` (example 3, B9) —
  the stable identity the predecessor fixed. ✅
- `ResolvedTemplate` gains no field or variant. ✅
- The predecessor's reserved-fallback precedence is upstream: by the time
  `configured_defaults` runs, `formal_name` already reflects whichever `toha-demo`
  resolved, and the key `"toha-demo"` matches that resolved identity. ✅
- `commit` is not part of the mapping key; a scoped mapping is keyed on
  `formal_name` alone. Consistent with the predecessor's identity/commit split; no
  new key-versioning question arises because the reframed model keys mappings on
  identity, not content. ✅

## Falsifiable scenarios and sole-kill anchors

B1–B14 make each load-bearing behavior falsifiable. Two anchors a reviewer will
want: B2 pins "no implicit by-id" (reintroducing id-application fails B2), and B3/B7
pin the representation (a literal misread as a reference, or a store entry accepting
a reference, fails them). B13 pins the migration (silent acceptance or silent drop
fails it). B14 pins engine purity. B4/B5/B10 use different winning layer files so
dropping or swapping either mapping or preset origin fails the named assertion.

## Verdict

The reframed design resolves Bob's directed reshape and the task's decisions,
removes the implicit global-by-id model via an explicit named-store-plus-references
model, migrates existing config without silent data loss, keeps the engine pure, and
is compatible with the approved predecessor. It holds. Bob approved D1–D5, with
the requested `presets` / `{ preset: <name> }` naming revision; no product decision
remains open in this design.
