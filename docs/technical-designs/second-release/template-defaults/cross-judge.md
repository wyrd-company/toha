# Cross-judge — template-specific configured defaults

Readonly cross-judge, one runner, scoring every candidate against every rubric
criterion and recommending a base. Runner family: `inherit-parent` (Claude). A
different model family (gpt-5.6) was configured for the two gpt runner slots but
was unavailable in this environment (see `synthesis.md` dropout note); the
cross-judge therefore ran on the parent family. The judge saw the rubric and the
three candidate packages by path; it did not see the orchestrator's own scores.

## Scores (judge)

| Criterion | C1 overlay | C2 rule-list | C3 qualified-keys |
| --- | ---: | ---: | ---: |
| C1 Unambiguous identity selection | 5 | 4 | 5 |
| C2 Global preserved + total precedence | 5 | 5 | 5 |
| C3 Engine purity & path parity | 4 | 5 | 4 |
| C4 Boundary validation & attribution | 4 | 3 | 5 |
| C5 Interface depth & contained surface | 5 | 5 | 4 |
| C6 Future reusable direction | 5 | 5 | 5 |
| **Total** | **28** | **27** | **28** |

## Judge recommendation

Base **C1** (breaks the C1/C3 tie in C1's favor for foundation reasons): keys on
the one identity grounding proves unambiguous (`formal_name`) with zero new
ambiguity surface; smallest, deepest interface; every graft is additive; its
precedence rule is already forward-designed for a middle tier.

Grafts recommended:

- From **C3**: extract a pure `validate_default` from the engine and own
  default-error attribution at the boundary — removes C1's need to add a
  `config_key` field to `EvalError` and to place the resolver inside
  `interview.rs`, and lets scoped errors name the exact config key plus its
  layer/file. One graft closes both of C1's soft spots (its C3 and C4 dips).
  Bring C3's config-relative `path:` handling if local-config portability matters.
- From **C2**: reuse the existing no-fetch `formal_name` normalizer so a scoped
  key can be written in any CLI selector form, normalized before lookup —
  answering C1's own flagged discoverability/verbosity risk — with C2's rule that
  a selector is refused only when it is ambiguous about the running target.

## Risk the judge says all three candidates missed

**The role of "version" in the default key is inconsistent and undocumented.** All
three key on `formal_name` and exclude `commit`, which produces two different
answers to "does upgrading a template keep its defaults?":

- A **ref-pinned git address** carries `@ref#sub` inside the formal name, so
  `gh:a/b@v1` and `gh:a/b@v2` are different keys — a ref bump silently drops the
  defaults.
- An **installed template updated in place** keeps its formal name (new `commit`),
  so its defaults carry over across the same kind of version change.

Related: **local-layer path/folder keys are machine-absolute and non-portable** — a
`.toha.yml` committed to a repo with an absolute-path-keyed default breaks on
another clone; only C3's config-relative `path:` mitigates it.

Both point at one unresolved product question, elevated to the Phase C checkpoint:
what is allowed to change a default's identity key — a git ref, a content digest, a
folder move — and is that rule the same for git, installed, and folder templates?
