# Cross-judge — in-project generators

Read-only cross-family judge. Runner: Codex (GPT), via `async-codex-mcp`, session
`05af1395-fb15-4eb5-a7c7-e7b9180a1d8e` (codex thread
`01a0f435-be02-72a3-a397-d70447ff7a23`), 2026-09-30. It saw the rubric, the
grounding, the common task, the consumed producer contract, and all three
candidate packages by path. It scored each candidate against C1–C6, screened for
red flags, recommended a base, and flagged producer-contract violations. Captured
verbatim below; the parent's reconciliation is in `synthesis.md`.

## Score table (candidates × C1–C6, /30)

| Candidate | C1 | C2 | C3 | C4 | C5 | C6 | Total |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 — `apply --like` | 4 | 4 | 4 | 4 | 4 | 2 | **22** |
| 2 — `generate` verb | 5 | 4 | 2 | 5 | 2 | 3 | **21** |
| 3 — declared generators | 3 | 1 | 3 | 3 | 1 | 2 | **13** |

## Base recommendation

**Candidate 1, with revisions.** Deepest useful seam, least new machinery;
selection contained, precedence stays at the engine boundary, no alternate
template model. C2's C1 (axes) and C4 (identity) are excellent but its public
selection leaks `Project`/`SnapshotId`/`ProjectError` and mixes repository access
with prompting (C5 red flags); C3's declared concept scores lowest (leakage,
temporal decomposition, pass-through, and — its own finding — no snapshot-keying
leverage).

## Required revisions before accepting the base

- Define **wrong-kind, rejected-value, and missing-value** seed behavior
  separately.
- Expose **one dependency-accepting selector interface** for crate use, with
  prompting **outside** it.
- **Keep normal apply capture**; scope "reader-only" to seed resolution.
- Define newest-selection and picker ordering completely.
- Make snapshot **discovery** work across application subpaths.
- Add dedicated selection, ambiguity, no-id-coupling, and purity guards; **record
  the exact consumed revisions**.
- State explicitly how opt-in `--like` satisfies startup policy A.

## Red-flag screen

- **Candidate 1:** none clear; its constructor performs real precedence work
  (small ≠ shallow).
- **Candidate 2:** information leakage (public `Project`, `SnapshotId`,
  `ProjectError`); shallow `GeneratorApplication` wrapper; selection mixes repo
  access and UI.
- **Candidate 3:** information leakage, temporal decomposition, and pass-through
  (`candidates → latest/require → seed_from` assembled by the caller; `latest`
  forwards to `first()`).

## Graft / reject

- **From Candidate 2 — graft:** identity defined before capture; dirty targets can
  still read existing snapshots; returned stale-value warnings; a resume guard so
  a newer snapshot cannot change a pinned choice. **Reject:** "likely base" as an
  automatically deterministic cross-subpath winner; hidden prompting in public
  selection; the redundant application wrapper.
- **From Candidate 3 — graft:** explicit loop-array preservation guard; fixed
  precedence encoded inside the engine boundary; its candid finding that a
  declaration provides no snapshot-keying advantage. **Reject:** the `generators:`
  overlay model under the unchanged producer contract; source-wide reuse across
  distinct generator interviews; public multi-step snapshot selection.

## Producer-contract / concurrent-ownership flags (all verified against 1066)

1. **C1 mischaracterizes the update replay adapter.** C1's rationale says the
   update adapter treats a rejected recorded value as fatal and submits whole
   batches; the producer assigns those to the *existing staged replay* and the
   update adapter *fixes* them with overrides and re-asking
   (project-updates `design.md:292`). The reason not to reuse it is ownership and
   semantics, not this false shortcoming.
2. **C1's whole-run "never writes a ref" guard would disable producer-owned
   capture.** A normal clean apply saves a snapshot (`design.md:218`); C1's own
   fixture expects two. "Reader-only" must be scoped to *seed resolution*, not the
   whole run; the generate apply still captures its own snapshot.
3. **C1 invents a source-filtered `snapshots list`.** The producer lists by target
   PATH (`design.md:534`); there is no `--source` filter, so `snapshots list .`
   need not reveal sibling-subpath applications. Discovery must come from the
   repo-wide reader in-flow, not a nonexistent command.
4. **C2 overclaims `likely_bases` determinism.** The producer's mark never selects
   a base and returns `Vec<LikelyBase>` without a global winner/tie rule
   (`design.md:538`). A consumer must define its own total rule.
5. **C2 uses sub-minimum prefixes.** `01J9`/`ZZZZ`/`01` cannot exercise lookup;
   accepted prefixes are ≥6 characters (`design.md:158`).
6. **C3 conflates dirty with snapshot-unavailable.** Dirtiness prevents new
   capture, not reading existing refs (`design.md:218`).
7. **C3 cannot reconstruct its captured generator view for updates.** Producer
   metadata carries template/source/target, not generator selection
   (`design.md:186`); repair would edit concurrently-owned surfaces.

> No candidate named the exact consumed producer revision; the synthesis and
> final design must (1066 revision 3 + completeness correction 1110, at the
> integrated base).
