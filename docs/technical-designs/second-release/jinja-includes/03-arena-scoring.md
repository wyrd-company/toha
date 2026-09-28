# Arena scoring & reconciliation — task 1076

Phase C/D record. Four candidates ran to completion (no dropouts). Readonly
cross-judge: `ocx-gpt-5-6-luna` (different model family from the opus parent).
Parent scores from reading every candidate end to end. Criteria and weights are
in `02-rubric.md`.

## Scores (1–5 per criterion)

| Criterion | C1 (parent / judge) | C2 (parent / judge) | C3 (parent / judge) | C4 (parent / judge) |
| --- | --- | --- | --- | --- |
| 1 Confinement | 5 / 5 | 5 / 4 | 4 / 4 | 3 / 4 |
| 2 Surface scope | 5 / 5 | 5 / 5 | 3 / 3 | 3 / 2 |
| 3 Include semantics | 5 / 5 | 5 / 5 | 4 / 4 | 4 / 4 |
| 4 Diagnostics | 5 / 5 | 3 / 3 | 4 / 3 | 3 / 3 |
| 5 Interface depth | 5 / 5 | 4 / 4 | 3 / 3 | 3 / 4 |
| 6 Compatibility | 4 / 5 | 5 / 5 | 4 / 4 | 3 / 3 |
| **Total** | **29 / 30** | **27 / 26** | **22 / 21** | **19 / 20** |

Ranking (both scorers): **C1 > C2 > C3 > C4.**

## Where parent and judge differ

- **C2 confinement:** judge 4 (a live render-time loader is a standing seam);
  parent 5 (the `confined_loader` closure is a single un-bypassable point and
  closes all three vectors; "no loader at all" in C1 is marginally stronger but
  C2's is sound). Resolved toward the judge's caution for the base pick: the
  no-loader posture is a genuine improvement worth grafting.
- **C1 compatibility:** parent 4 (the `unstable_machinery` MiniJinja feature is
  "no semver guarantees" per `minijinja/src/lib.rs:193` — a real maintenance
  risk for a published crate); judge 5 (flagged and acceptable). Parent weights
  this higher; it becomes the one decision surfaced to Bob rather than a silent
  choice.

## Verified defects (adversarial, confirmed against minijinja 2.24.0 source)

- **C4 — `ConfinedLoader.current_file: RefCell<Option<PathBuf>>` is not `Sync`.**
  `Environment::set_loader` requires `Fn(...) + Send + Sync + 'static`
  (`environment.rs:234`). The design would not compile as written. CONFIRMED.
- **C4 — dead symlink check.** `normalize_path` calls `canonicalize()` (which
  resolves every symlink) and *then* tests `actual.is_symlink()`; that branch is
  unreachable. The real symlink-escape guard is absent on the canonicalized
  path, and the `normalize_without_canonicalize` fallback checks no symlink at
  all. CONFIRMED.
- **C4 — cycle wording `"include cycle: x → y → x"` collides with the YAML
  `!include` diagnostic `"include cycle"`** (`template.rs:280`). Violates the
  distinct-diagnostics hard requirement. CONFIRMED.
- **C3 / C4 — relative-to-includer resolution is under-specified.** MiniJinja
  looks include names up **verbatim** (`vm/mod.rs:840`); it does not resolve
  relative to the including template. Both designs need per-includer name
  rewriting, which requires threading the current file into a name-only loader
  callback. C4 does it with the non-`Sync` `RefCell`; C3 does not show the
  threading at all. CONFIRMED shortfall.
- **C3 — includes leak onto interview fields (`prompt`/`description`/
  `placeholder`) and apply messages,** attaching a filesystem loader to the
  interview path. This breaks the "pure interview engine, filesystem-free across
  headless/staged/crate drivers" invariant (`AGENTS.md`, grounding Preserve).
  CONFIRMED as a scope defect.
- **C1 / C2 — no confinement or correctness defect found.** C2's only real
  weakness is diagnostics (cycle via string-matching MiniJinja's "recursion"
  message; no cycle path). C1's only real weakness is the `unstable_machinery`
  dependency and literal-only targets, both flagged by the candidate.

## Base + graft decision

**Base: C2** — the capability-by-type spine (`Partials` → `FileTmpl`), because
it enforces surface scope structurally with the smallest public surface and
needs no unstable dependency for that spine, and it already makes the two
correct foundational choices (confine to `template.root`, root-relative literal
names) that C1 shares.

**Graft from C1 (recommended default):** replace C2's render-time loader +
recursion-string translation with C1's **compile-time confined closure** — the
static include-graph walk that pre-registers partials with no render-time
loader, yields a **named cycle path**, and unions `referenced_ids` across the
graph so include-only variables validate at load. Also graft C1's explicit
`IncludeError` wording and its per-component symlink refusal.

**Graft from C3/C4:** their precise cycle-path formatting (reinforces C1's) and
C3's explicit enumeration of excluded expression surfaces (`when`/`computed`/
`format`/`regex`) for documentation completeness. No structural graft.

**Rejected (recorded):** relative-to-includer semantics (C3, C4) — needs name
rewriting MiniJinja does not do, and C4's stateful non-`Sync` loader is a bug;
root-relative needs none. `source_dir` confinement (C3) — forces partials into
the emitted tree and adds `ignore` ceremony for no security gain. Interview-field
/ apply-message includes (C3) — breaks interview-engine purity. C4's raw
MiniJinja missing message and colliding cycle wording.

The graft creates one localized decision (static closure needs literal-only +
`unstable_machinery`; the loader alternative needs neither but gives weaker
diagnostics) — surfaced to Bob at the checkpoint rather than chosen silently.
