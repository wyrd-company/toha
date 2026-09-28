# Cross-judge — template-specific configured defaults (named stored values)

Readonly cross-judge, one runner, scoring every candidate against every rubric
criterion and recommending a base. Runner family: `inherit-parent` (Claude); the
gpt-5.6 family remains unavailable in this environment. The judge saw the rubric
and the three candidate packages by path; it did not see the orchestrator's own
scores.

## Scores (judge)

| Criterion | C1 nested-map/`{value:}` | C2 binding-list | C3 sigil/graph |
| --- | ---: | ---: | ---: |
| C1 No implicit id coupling; refs explicit | 5 | 4 | 5 |
| C2 Ref/literal representation unambiguous & named | 4 | 5 | 3 |
| C3 Resolution safety (missing/typing/cycles) | 5 | 5 | 4 |
| C4 Layering, engine purity, path parity | 5 | 4 | 5 |
| C5 Safe transition of `defaults:` (no silent loss) | 5 | 5 | 4 |
| C6 Interface depth, contained surface, future | 5 | 4 | 3 |
| **Total** | **29** | **27** | **24** |

## Judge recommendation

Base **C1**. It wins on balance and on the axes the reframe weights hardest:
strictest identity keying with no registry coupling; **cycles impossible by
construction** rather than detected (C1 and C2 delete the hazard the grounding
lists under *Avoid*; C3 opens it then guards it); resolution stays in the `toha`
library so the crate path is served without reimplementing selection; smallest
deep surface. Its two soft spots graft cleanly from the losers.

Grafts recommended:

- From **C2**: the explicit `value:` / `literal:` key discriminator (removes C1's
  reliance on the "answers are never objects" reading), and C2's `Origin{file,
  1-based index}` for the most precise config-site attribution.
- From **C3**: the reference-provenance chain in error text — name both the mapping
  site and the value it resolved through (`template-defaults."X".email →
  values."contact"`), so a shared-value kind error points at the value to fix. Do
  **not** graft C3's chaining or sigil — both are anti-requirements here.

## Risks the judge says all three candidates missed

1. **Same-answer-kind reuse limit (sharpest).** A stored value's kind equals its
   JSON shape (string / bool / list-of-strings), so one stored value can seed only
   questions of a single answer kind — you cannot point both a text question and a
   confirm question at one value. The feature is sold as "reuse one value across
   differently-named questions," but the reuse is bounded to one answer kind, and
   none of the three surfaces or documents it.
2. **Migration cognitive trap.** Authors migrating from `defaults:` will name
   `values` entries after question ids (`title`, `email`, `owner`), making the new
   *explicit* model look like the old *implicit* one and inviting the expectation
   that it still applies globally. None turns this into an active migration/doc
   guardrail.
3. **Eager validation of conditionally-unreachable questions.** All three validate
   any mapping whose question the template *defines*, regardless of `when`/batch
   reachability, so a kind mismatch on a question that would never be asked this
   run still errors at seed time — a tension with "validate only the value actually
   used." Worth an explicit decision.

All three risks are carried into the final design (`design.md`) and its decisions
(D5 covers 1 and 3; the migration section and D1 cover 2).
