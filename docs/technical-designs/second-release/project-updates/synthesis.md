---
relationships:
  depends-on: project-updates
  informs: project-updates
---

# Synthesis — project updates

## Runners and dropouts

| Slot | Runner | Result |
|---|---|---|
| candidate-1 | Claude (general-purpose, inherit) | Complete: `candidates/candidate-1.md` |
| candidate-2 | `ocx-gpt-5-6-sol` | Dropout: model route `model_not_found` (HTTP 404) on resume; no output |
| candidate-3 | `ocx-gpt-6-astra` | Dropout: model route `model_not_found` (HTTP 404) on resume; no output |
| candidate-4 | Codex, replacement direction | Complete: `candidates/candidate-4.md` |
| cross-judge | Codex, read-only | `cross-judge.md`; input hashes verified unchanged after the run |

Candidates read a pinned, read-only snapshot of `2c4a197` plus `runner-task.md`
and `grounding.md`. None read `rubric.md`. `grounding-delta.md` records the
refreshed base, `b97da39`.

## Scores and reconciliation

| Criterion | Own c1 | Judge c1 | Own c4 | Judge c4 |
|---|:---:|:---:|:---:|:---:|
| C1 Applied record | 4 | 5 | 3 | 4 |
| C2 Merge semantics | 5 | 5 | 3 | 4 |
| C3 Merge engine | 5 | 5 | 3 | 3 |
| C4 Composition | 5 | 4 | 2 | 3 |
| C5 Depth and proof | 4 | 4 | 2 | 2 |
| **Total** | **23** | **23** | **13** | **16** |

Both reviewers choose candidate-1 as the base. The disagreements are resolved as
follows:

- **c1 C1 (4 vs 5), c1 C5.** Both reviewers penalize the reservation of all of
  `.toha/` against template output, in different criteria. The synthesis
  narrows the reservation to `.toha/applied/` and presents it as a
  capability restriction that needs approval (decision D2).
- **c1 C4 (5 vs 4).** The judge counts three items as restrictions: the refusal
  of update while an interview is staged, the refusal while Toha conflict
  markers remain, and the record as a new identity authority. None of them
  narrows an existing capability, because all three apply to new commands. The
  staged refusal is the rule the approved scripted route already uses. The
  synthesis keeps them and states each one in the design.
- **c4 C2 (3 vs 4).** The judge's constraint table says candidate-4 makes no
  silent change to injection semantics. The candidate's own text says: "if ours
  equals base, retain operator value" (`candidates/candidate-4.md:118`). That
  keeps an operator's change to an owned JSON value, and the approved
  content-injection design states the opposite: a changed owned value
  "converges without `--force`". The own score stands.
- **c4 C4 (2 vs 3).** In candidate-4, a question that is new at B, or a
  recorded answer that B rejects, is the fatal `AnswerReplay` error. No route
  can supply an answer. The judge does not mention this. The own score stands.

## Base: candidate-1

Candidate-1 records the render **inputs** and re-derives the merge base by
rendering the old template again. A per-identity fingerprint verifies that
re-derived base and falls back safely when the base cannot be reproduced. The
design keeps one source of truth, keeps the record small, and never deletes on
an unverified base.

## Corrections from verification against the engine

Verification against the code at the base found two defects that both
candidates share. The synthesis corrects them.

1. **Answers are recorded as accepted raw submissions, not completed answers.**
   A question's `format` expression transforms its answer
   (`src/interview.rs:1209-1223`), so `Completed.answers` holds post-format
   values. Replaying a formatted value through the same question formats it
   again. The staged record avoids this by storing the accepted raw submissions
   in order (`src/staging.rs:31`, `src/main.rs:768`). The applied record stores
   the same sequence and replays it through the existing staged-replay walk.
   Every reached question's value, including a value taken from a default, is in
   that sequence, so replay never resolves a default again.
2. **Recorded answers for questions removed at B are filtered before replay.**
   The engine rejects an answer whose id "is not a question in this template"
   (`src/interview.rs:1952`). Candidate-1 claims that unknown ids are dropped
   silently, and that claim is false. The update filters each recorded
   submission to the question ids of the template being rendered, as a pure
   step before replay. The engine does not change.

## Grafts

| Graft | Source | Reason |
|---|---|---|
| No error, `Debug`, or `Display` output carries answer values or file contents; conflict content reaches a caller only through the explicit report | candidate-4 | Answers and project files can be sensitive, and this matches the leakage rule of the hook-results design. |
| Clean identities are not written while any conflict is refused | candidate-4 (and candidate-1) | Keeps the unchanged-on-conflict contract for the whole project, not per file. |
| The record advances only after hooks succeed; a retry reconciles from the old base | candidate-4 (and candidate-1) | One commit point makes a crash or hook failure converge on rerun. |
| Compact base/theirs/ours case notation | candidate-4 | Clearer case tables in the design and the deck. |

## Rejections

| Rejected | Source | Reason |
|---|---|---|
| Store every base byte and value in the record | candidate-4 (judge graft suggestion) | Duplicates the template, puts answer-derived and binary content into the repository again, and creates a second source of truth. Fingerprint mode covers the unavailable-base case safely. Retained as the alternative behind decision D4. |
| JsonValue three-way value merge | candidate-4 | Contradicts the approved convergent ownership. |
| Refusing record-less projects | candidate-4 | Adoption writes only the record and changes no project file, so refusal adds no safety. |
| `ProjectUpdate::prepare(&Path, …)` | candidate-4 | Bypasses `CanonicalTarget`, the sole target constructor. |
| `TemplateResolver` trait | candidate-4 | A hypothetical seam with no second adapter. |
| Reserving all of `.toha/` | candidate-1 | Wider than needed. Narrowed to `.toha/applied/` and disclosed (D2). |
| `--on-conflict template` | candidate-1 | A routine policy that overwrites an operator's edits. Refuse and markers cover the need. An operator who wants the template's text removes their edit and runs update again. |
| New exit code 6 | candidate-1 | Not needed. A markers run exits 1 with status `updated` and a non-empty `conflicted` list, so a script cannot mistake it for a clean update. |
| `update P --async [F]` with an input answers file | candidate-1 | In the approved route grammar, `--async FILE` names an *output* batch file. Update never stages, so agents use the one-shot scripted route `update P --answers F` (decision D9). |
| `Ord` derives on injection types | candidate-1 | The identity join sorts on a private key built from `TargetPath::as_path`, `RegionKey::as_str`, and `JsonPath::segments`, so injection types do not change. |
| Public retraction functions in the injection module | candidate-1 | They become crate-private (`pub(crate)`) helpers beside the resolvers. The public producer interface and ownership semantics stay exactly as approved. |
| Recording completed (post-format) answers | both | See correction 1. |
| Silently dropping removed question ids | candidate-1 | See correction 2. |
| `answers_document()` export | candidate-1 | Not required by the feature. It would need its own flattening rules. |

## Revisit of the losing candidate

Candidate-4 was reread after the base was fixed. Its strongest idea is the
self-contained base, which gives full three-way merges offline and after a
template repository loses a commit. The final design keeps that property as the
explicit alternative in D4, with its cost stated. Its small two-step public
surface is matched by the final design's `UpdatePlan::{changes, apply}`. The
final design puts the interview before `plan` so that every driver keeps its
modality.

## Result

`design.md` is the synthesized design. `verification.md` checks it against the
task, the prerequisite contracts, and the code at the base.
