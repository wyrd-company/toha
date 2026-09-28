# Synthesis — template-specific configured defaults

Records the base pick, the orchestrator's own criterion-by-criterion scoring, the
reconciliation with the cross-judge, the grafts and rejections, dropouts, and the
one risk elevated to the Phase C checkpoint.

## Runners and dropouts

Configured architect runner intent: four `inherit-parent` slots plus a
cross-judge preferring a different model family. Actual:

- Candidate 1 (identity-scoped overlay): produced by `inherit-parent` (Claude). ✅
- Candidate 2 (ordered rule list): first attempt on `gpt-5.6-sol` **failed** (model
  unavailable in this environment); re-run on `inherit-parent` (Claude). ✅
- Candidate 3 (qualified selector keys): first attempt on `gpt-5.6-terra`
  **failed** (model unavailable); re-run on `inherit-parent` (Claude). ✅
- Cross-judge: preferred a non-parent family; gpt-5.6 unavailable, so it ran on
  `inherit-parent`.

**Dropout note:** the two gpt-5.6 runner slots dropped out on a terminal API
"model not available" error and were replaced by `inherit-parent` runners on the
same distinct design directions. Three structurally distinct viable candidates
were produced, satisfying the ≥2 requirement. The lost cross-family diversity is
recorded as a limitation of this run, not of the design.

## Own scores vs cross-judge

Scored independently, criterion by criterion, after reading all three end to end.

| Criterion | C1 | C2 | C3 |
| --- | ---: | ---: | ---: |
| C1 Unambiguous identity selection | 5 | 4 | 5 |
| C2 Global preserved + total precedence | 5 | 5 | 5 |
| C3 Engine purity & path parity | 4 | 5 | 4 |
| C4 Boundary validation & attribution | 4 | 3 | 5 |
| C5 Interface depth & contained surface | 5 | 5 | 4 |
| C6 Future reusable direction | 5 | 5 | 5 |
| **Total** | **28** | **27** | **28** |

The orchestrator's totals match the cross-judge exactly, including the C1/C3 tie
at 28 and C2 at 27. No disagreement to reconcile on the numbers. The one place the
two readings differ in emphasis: the orchestrator rates C3's "config keys are
*matched* against a resolved identity, never *resolved through* `registry.resolve`"
as the single most valuable correctness idea in the arena — it is what removes the
exit-5 ambiguity path from configuration entirely. C1 already has this property
(it keys on `formal_name`, which is matched by equality), so the base keeps it; the
synthesis adopts C3's explicit `TemplateIdentity` matching model to make it
structural rather than incidental.

## Base

**Candidate 1 — identity-scoped overlay keyed on the stable `formal_name`.** Chosen
for the reasons the cross-judge gives and the orchestrator confirms: it keys on the
one selector grounding proves unambiguous, its user surface is the smallest and
deepest, its precedence rule is already forward-designed for a middle tier, and
every graft below is additive rather than a reshape. A maintainer extends it
without relearning it.

## Grafts (source → what → why)

- **From C3 — boundary-owned validation and attribution.** Extract a pure
  `validate_default(template, id, value)` from the engine and resolve/validate
  defaults in a new boundary module (`src/cli/defaults.rs`), not inside
  `interview.rs`. This drops C1's proposed `EvalError.config_key` field and its
  placement of `resolved_defaults` in the engine, so the pure engine stays 100%
  identity- and config-unaware (fixes C1's C3 dip), while scoped errors name the
  exact config key **and** its layer/file, and only the *winning* value per id is
  validated (fixes C1's C4 dip). One graft closes both of the base's soft spots.
- **From C3 — the `TemplateIdentity` match model.** Match config keys against a
  `TemplateIdentity` derived from `ResolvedTemplate` + `Registry`, never by
  re-resolving the key through `registry.resolve`. Guarantees configuration can
  never raise the exit-5 ambiguity path. `ResolvedTemplate` is unchanged (honors
  the bundled-demo coordination contract: no new field or variant).
- **From C3 — config-relative path handling and refuse-on-tie.** Folder/path
  identity keys in a *local* config resolve relative to the config file's
  directory (reusing the existing `config::path` behavior at `src/config.rs:92`),
  which mitigates the "committed `.toha.yml` breaks on clone" portability risk. A
  genuine same-layer, same-specificity tie is refused (`Conflict`), not guessed.
- **From C2 — reserve specificity as an ordinal.** Model specificity as an ordinal
  (`global < … < identity`) so a future glob or tag tier slots in *between* without
  redefining the precedence rule. C2's honest "matches nothing → inert, optional
  one-line stderr note" behavior is adopted for unresolved keys.

## Rejections (what was dropped and why)

- **C2's ordered rule list with registry-normalized `match` (rejected as the base
  shape).** Normalizing a selector *through* `formal_name`/`registry.resolve` at
  match time reintroduces exactly the ambiguity/exit-5 surface that keying on a
  resolved identity removes — a config file could raise an ambiguity error. The
  ergonomic goal it served (write a scoped key in any selector form) is better met,
  if wanted, by C3's qualified keys matched against the identity's own facets. The
  ordered-list *expressiveness* (order-dependent precedence) is more machinery than
  the stated problem needs.
- **C2's deferred attribution (rejected).** Leaving a bad rule value attributed to
  `defaults.<id>` fails criterion C4; the C3 graft supersedes it.
- **C3's four-qualifier vocabulary as a *mandatory* now-surface (deferred, not
  rejected).** `formal:`/`alias:`/`name:`/`path:` and the intentional `name:`
  broadcast are genuine capabilities, but they widen the user vocabulary and make
  `alias:`/`name:` registry-relative. The stated problem (two templates, one id,
  separate defaults) is fully solved by formal-name keying alone. Whether to ship
  qualified selectors now or reserve them as the recommended future direction is
  **decision D2** at the Phase C checkpoint — deferred to Bob, not silently
  decided. The base architecture (identity matching + boundary resolution) makes
  either choice additive.
- **Global disposition options B/C from C1/C3 (rejected as recommendation).**
  Removing or folding away `defaults` narrows a supported capability; keeping
  global-by-id unchanged as the base tier is recommended (decision D3). A
  warn-on-overlap variant adds per-run noise for what is the intended mechanism.

## Convergence signal

All three candidates independently converged on: (a) key on the stable identity,
never the short name; (b) resolve at the config/resolve boundary and hand the pure
engine the unchanged flat `Seed.defaults`; (c) re-resolve on resume, never freeze;
(d) **specificity dominates layer** in precedence, each flagging that exact
precedence choice as the load-bearing decision for Bob; (e) global-by-id preserved
as the base tier. This convergence is a strong agreement signal and is carried
into the final design as settled shape, with the precedence direction itself still
put to Bob as decision D1 (recommended: specificity-primary).

## Risk elevated to the checkpoint

The cross-judge's identity-key-versioning risk (ref-pinned git addresses fragment
defaults across refs while installed-in-place templates carry them over; local
absolute-path keys are non-portable) was missed by all three candidates and is
real. It is carried into the final design's compatibility section and raised as
Phase C **decision D4** (what may change a default's identity key). The
config-relative path graft already mitigates the portability half.

## Verification

See `verification.md`: caller usage re-checked against the sketch, every
requirement and inherited constraint checked against the design, failure cases and
falsifiable scenarios listed, and compatibility with the approved bundled-demo
predecessor revision confirmed.
