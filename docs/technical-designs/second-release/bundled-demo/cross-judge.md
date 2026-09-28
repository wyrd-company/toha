# Bundled offline toha-demo — cross-judge and scoring

Candidates are scored against the six rubric criteria in `rubric.md`. Candidates
received the common task and `grounding.md`; they did not see the rubric. Two
independent scorings were run — a readonly cross-judge and the architect's own
end-to-end read — then reconciled.

## Runners and dropout

- Candidate 1 — Direction A (reserved-name resolution branch + on-demand
  materialization behind one seam). Runner: `claude` (opus, inherit-parent).
- Candidate 2 — Direction B (synthetic lowest-precedence "Bundled" registry
  layer). Runner: `claude` (opus, inherit-parent).
- Candidate 3 — Direction C (embedded-as-source: materialize into the cache,
  reuse install_key/commit + resume machinery). Runner: `claude` (opus,
  inherit-parent).

Default runner slots are four `inherit-parent`. Two runner slots were first
dispatched to a second model family (`ocx-gpt-5-6-sol`, `ocx-gpt-5-6-terra`) for
perspective diversity; both terminated on a terminal API error (model
unavailable in this environment). Those two directions (B and C) were re-run on
`claude` to preserve three structurally distinct candidates. The cross-judge ran
on `claude`; the preferred second model family was unavailable, so judge and
runners share the `claude` family. Independent architect scoring was performed to
offset the shared-family risk. All candidates worked in isolated directories
under `/tmp/arena-bundled-demo/candidate-<n>/`; no shared writable output paths.

## Score matrix (1–5 per criterion)

Two scorers agreed cell-for-cell; a single matrix is shown.

| Criterion | C1 (resolve branch) | C2 (registry layer) | C3 (embedded-as-source) |
|---|---|---|---|
| C1 Offline first-run by name | 5 | 4 | 5 |
| C2 Collision-preserving resolution | 5 | 4 | 5 |
| C3 Resume and identity integrity | 5 | 4 | 5 |
| C4 Interface depth and blast radius | 5 | 3 | 5 |
| C5 Single maintained source + enforcing check | 5 | 4 | 5 |
| C6 Trust and safety boundary preserved | 5 | 5 | 4 |
| **Total (/30)** | **30** | **24** | **29** |
| **Rank** | **1st** | **3rd** | **2nd** |

### Justifications (condensed)

- **C1 offline** — C1/C3 resolve on a `NotFound("toha-demo")` fallback and
  materialize into `cache/sources/…/<commit>`, unchanged pipeline, all three
  paths + cross-process resume. C3 adds an explicit temp-dir fallback when the
  cache is unwritable (most robust). C2 couples first-run success to seeding the
  synthetic layer into every registry-building path, with no cache-unavailable
  degrade.
- **C2 collision** — C1/C3 are reachable only on the exact input that fails
  today; the registry is consulted unchanged and always wins. C2's fallback-only
  is correct but *emergent* from a rewrite of `resolve`/`check_aliases` plus a
  new formal-only concept, and it changes unfiltered `templates list` output.
- **C3 resume/identity** — all three reconstruct from `(formal_name, commit)`
  offline. C2's reconstruction depends on the entry being re-seeded every run
  and surfaces a git-flavoured message on cross-upgrade mismatch. C1/C3 branch
  directly on identity.
- **C4 depth** — C1: zero new public types, `ResolvedTemplate` unchanged, one
  new module + three internal branches. C3: same plus a trivial `ResolveError`
  helper, no library/registry/type change. C2: touches the **library** with a
  new serialized `Layer::Bundled` variant, `seed_fallback`, `by_name`/
  `formal_only`, and rewrites of `resolve` + `check_aliases` — the largest blast
  radius.
- **C5 source/check** — all embed via `include_dir!` (one source, structural).
  C1/C3 derive the commit at runtime from the embedded bytes (cannot go stale);
  C2 bakes it via a `build.rs` env var and needs a separate freshness test.
- **C6 trust/safety** — all keep the demo untrusted and add no new
  permission/timeout/pinned-check/subprocess. C3 loses a point: its
  cache-unwritable `into_temp()` leaks the temp dir (`dir.keep()`, no cleanup)
  and writes outside the documented cache.

## Design-red-flags screen

- **C1** — no significant hit. Reserved-name knowledge sits in three spots in
  `resolve.rs` but the rule is centralized in `is_reserved`; the module is deep;
  `resume`→`resolve` adds a guard, not a pass-through.
- **C2** — *information leakage (confirmed).* "Bundled = formal-only" and the
  `Layer::Bundled` concept span `resolve`, `check_aliases`, the alias guard,
  `by_name`, `seed_fallback`, and the enum — pushed into the library on purpose.
  Mitigated by centralizing the rule in `by_name()`/`formal_only()`, not
  eliminated. No shallow-module/temporal/pass-through hit.
- **C3** — no significant hit. `materialize`→`into_cache`/`into_temp`/
  `write_tree` is decomposition by concept, not temporal; `resume` adds a guard.

## Factual checks

- Commit-pattern compliance is correct in all three: 64-hex satisfies
  `^[0-9a-f]{40}([0-9a-f]{24})?$`.
- No candidate's precedence rule breaks a current *resolution* outcome (all
  fallback-only). C2's added `templates list` row is additive, not a narrowing.
- **`named` divergence:** C1 and C3 set `named=false` (folder-like, never
  registry-trustable); C2 sets `named=true` (a named entry that can never be
  trusted on disk — a latent inconsistency). `named=false` is the correct value
  for an identity backed by no registry entry.
- C3's optional SHA truncation to 40-hex is unnecessary and reduces collision
  resistance; dropped in synthesis.

## Verdict

C1 is the base (deepest, leanest seam; zero new public types; runtime-derived
commit; no registry entry, so the registry stays a truthful record of installed
templates). C3 contributes the cleanest resume seam and a genuine
first-run-resilience idea; C2 contributes discoverability. Grafts and rejections
are recorded in `synthesis.md`.
