---
relationships:
  depends-on: project-updates
  informs: project-updates
---

# Own scoring — project-updates arena

The orchestrating designer read both candidates end to end and scored them
against `rubric.md` before reading the cross-judge. Scores are 0–5 per criterion.

## Candidates and dropouts

| Slot | Runner | Output | SHA-256 |
|---|---|---|---|
| candidate-1 | Claude (general-purpose, inherit) | `candidates/candidate-1.md` | `c832d8dab2d47da103a34ef9fadf5ae082d74703b5311c5e9abf0e70937bd345` |
| candidate-2 | `ocx-gpt-5-6-sol` | none: dropout (model route `model_not_found`, HTTP 404, on resume) | — |
| candidate-3 | `ocx-gpt-6-astra` | none: dropout (model route `model_not_found`, HTTP 404, on resume) | — |
| candidate-4 | Codex (replacement direction for the dropped slots) | `candidates/candidate-4.md` | `d719938727a9d226a2de16ebc9d2578abfd4c402c505ce907078c916b19cd627` |

All candidates read the pinned input (`2c4a197` plus `runner-task.md` and
`grounding.md`) and not the rubric.

The two candidates are structurally distinct. Candidate-1 records the render
**inputs** and re-derives the merge base by rendering the old template again,
checked by per-identity fingerprints. Candidate-4 records the merge-base
**outputs** (every byte and value Toha wrote) and needs no template history.

## Scores

| Criterion | candidate-1 | candidate-4 |
|---|:---:|:---:|
| C1 Applied record | 4 | 3 |
| C2 Merge semantics per identity | 5 | 3 |
| C3 Merge engine and dependencies | 5 | 3 |
| C4 Composition, purity, routes | 5 | 2 |
| C5 Interface depth, proof | 4 | 2 |
| **Total** | **23** | **13** |

### C1 — Applied record

- **candidate-1 (4).** It compares the three carriers in one table and rejects
  trailer and ref/note on the travel requirement. It records formal name,
  commit, the frozen generation instant, the completed answers, and
  per-identity fingerprints in `.toha/applied/<slug>-<hash>.json`, one file per
  applied template. The target is derived, not stored. Region ownership reuses
  the marker checksum instead of a second copy. The wire type is private, and
  `deny_unknown_fields` applies. Minus one: it makes `Plan::build` reject any
  template output under `.toha/`. That narrows what a template can generate
  today, and it is not flagged as a capability restriction that needs approval.
- **candidate-4 (3).** One root file, `.toha-project.json`, that does not
  collide with `.toha.yml`. It has a boundary parse and an explicit format
  version. It stores full base bytes per identity, including binary copied
  files, inside JSON. This makes the record large, puts answer-derived content
  in the repository a second time, and creates a second source of truth. It
  stores no generation instant. The alternatives analysis is brief.

### C2 — Merge semantics per ownership identity

- **candidate-1 (5).** Complete case tables for Whole, Region, and JsonValue,
  covering present at both versions, only at A (disappeared), only at B,
  operator-deleted, and binary. A disappeared identity is `Retract` when it is
  unedited and `Release` when it is edited, so edited files are never deleted.
  The Region merge works inside the span only, and the checksum records the
  intended body, so a later plain `apply` still reports drift honestly. JsonValue
  stays convergent, as approved. Idempotency follows from the frozen instant
  and from writing the record last. A forged record cannot cause deletion,
  because retraction needs a verified base.
- **candidate-4 (3).** It defines base, theirs, and ours per identity and
  handles removal (delete when unchanged, conflict otherwise). JsonValue
  becomes a three-way value merge that **keeps an operator's change when the
  template value did not change**. That silently replaces the approved
  convergent ownership. Any conflict blocks the whole update, and there is no
  in-file conflict representation. Without a frozen `now()`, a date-rendering
  template changes on every update, so the idempotency claim does not hold.

### C3 — Merge engine and dependency discipline

- **candidate-1 (5).** It uses gitoxide's built-in text merge through the
  `merge` feature of the existing `gix` dependency, with links, and names
  `diffy` as a linked fallback. The claimed function
  `gix_merge::blob::builtin_driver::text::merge(out, input, labels, current,
  ancestor, other, options)` matches the published 0.20.1 source
  (`grounding-delta.md`). There is no subprocess.
- **candidate-4 (3).** It uses `diffy`, linked, in process, with no subprocess.
  It does not consider the capability already present in `gix`, and gives no
  build-vs-buy justification for a new direct dependency.

### C4 — Composition, purity, and route preservation

- **candidate-1 (5).** Update follows the approved three-route grammar
  (person `update P`, agent `update P --async [F]`, script
  `update P --answers F`). It never stages, and it refuses when an interview is
  staged. External documents use the `{template, answers}` envelope and the
  approved `parse_and_verify` gate. Adding the record as an identity authority is
  raised as an open question, not assumed. Hooks re-run under the existing
  trust gate, and results are not persisted. Adoption re-interviews and writes
  only the record. Additions to injection are raised as open questions.
- **candidate-4 (2).** Update never asks questions. A question that is new at
  B, or a recorded answer that B rejects, is the fatal `AnswerReplay` error, so
  the agent and person routes have no way forward. It refuses adoption. It
  changes JsonValue semantics (C2). `prepare(target: &Path, …)` bypasses
  `CanonicalTarget`, the sole target constructor.

### C5 — Interface depth, proof, and implementability

- **candidate-1 (4).** Usage comes first, with three call sites. Types,
  signatures, the seam map, an exhaustive error table, and a results table
  agree. It proposes a fixture shape for version A to B with operator edits,
  plus a second-run no-op assertion. Minus one: the crate surface is sequenced
  (`open` → `application` → `begin_update` → `with_document` → `plan` →
  `apply`). This is justified by the caller-driven interview, but it is wide.
  There is also no named behavior/test list, and a new exit code 6 is proposed.
- **candidate-4 (2).** The two-step `prepare`/`reconcile` surface is small. But
  the seams are temporally decomposed (`load_record`, `record_from_plan`,
  `render_at`, `reconcile_identity`, `persist_success`). It introduces a
  `TemplateResolver` trait with no second adapter. The flow renders the old
  template although the record already holds the base, which contradicts its
  self-contained-base claim. It has no behavior matrix and no fixture for
  version A to B with a conflict.

## Red-flag screen

| Red flag | candidate-1 | candidate-4 |
|---|---|---|
| Shallow module | Partial: sequenced crate surface; justified by driver modality | No |
| Information leakage | No: wire type private | Partial: base bytes and record format spread into reconcile and persist seams |
| Temporal decomposition | No: modules by knowledge (record, update, adopt) | Yes: load, render, reconcile, persist seams |
| Pass-through method | No | No |
| Hypothetical seam | No | Yes: `TemplateResolver` |

## Constraint check

| Check | candidate-1 | candidate-4 |
|---|---|---|
| Application subprocess | None | None |
| New permission, timeout, or pinned check | None | None |
| Supported-capability restriction | **Yes, undisclosed**: reserves `.toha/` against template output | None |
| Silent prerequisite interface change | None; injection additions raised as open questions | **Yes**: JsonValue three-way retention |
| Region/JsonValue collapsed to whole-file | No | No |
| Conflated with XDG staged record | No | No |
| Collides with `.toha.yml` | No | No |

## Base

Candidate-1 is the base. Candidate-4 contributes the grafts listed in
`synthesis.md`.
