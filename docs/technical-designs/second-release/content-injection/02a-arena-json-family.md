# Arena framing addendum — JSON-family / structured-mutation question

Extends `02-arena-rubric.md`. The base arena settled the mechanism for arbitrary
text (managed-region markers). This focused re-run resolves only the part whose
shape changed: how the JSON family (JSON/JSONC/JSON5) is handled and whether a
structured mutation mode belongs in the 0.2.0 slice. The marker base is not
re-litigated.

## Artifact each candidate produces

A focused design fragment (per the arena briefing at
`03-candidates/json-family-revision/00-arena-briefing.md`): author surface,
concrete BEFORE/AFTER examples (required), ownership/drift/idempotency model,
data/type sketch, error contract, format-preservation statement with cited
evidence, dependency justification, and a 0.2.0-slice scope recommendation.
Candidates receive the task, the base design, and the source-cited research; they
do not receive the scoring rubric.

## Gradeable criteria (rubric — picker's tool, not given to candidates)

1. **Format/comment/order preservation** — preserve a user's comments, key order,
   whitespace, or honestly bound where it cannot. Evidence-cited.
2. **Ownership / drift / idempotency** — legible ownership; byte no-op re-apply
   when unchanged; drift detected or its absence honestly stated; survives
   staging replay and plan-before-write purity.
3. **Conflict / failure / recovery** — refusal cases, exit codes,
   `--force`/`--dry-run`, atomicity, no-silent-overwrite of user-owned files.
4. **Interface depth / small surface / composition** — small author surface,
   reuses `TargetPath`, composes with the marker base and whole-file writes.
5. **Scope honesty & dependency justification** — the jaq question answered on
   real evidence; any new dependency justified (adopt-vs-build, maintained);
   sound 0.2.0-slice recommendation.
6. **Evidence quality** — tool claims backed by real repo revision + `file:line`.

## Runners and champion directions

Three structurally distinct candidates, cross-family, launched **foreground**
(`run_in_background: false`) in one message; the parent collected every result
before judging and stayed nonterminal while any child was active. Each wrote to
an isolated directory; no shared writable path.

| Candidate | Model family | Champions | Output dir |
|---|---|---|---|
| A | GPT (`ocx-gpt-5-6-sol`) | Embedded `jaq` structured mode (jq filter, reserialize) | `/tmp/arena-injection-v2/candidate-a/` |
| B | GPT (`ocx-gpt-6-astra`) | Format-preserving CST via `jsonc-parser` (owns value at path) | `/tmp/arena-injection-v2/candidate-b/` |
| C | GPT (`ocx-gpt-5-6-terra`) | No structured engine; JSON-family via markers (JSONC/JSON5) + strict-JSON boundary | `/tmp/arena-injection-v2/candidate-c/` |

Cross-judge: one readonly judge on `ocx-gpt-5-6-luna` (a family different from the
parent, `claude`/opus), scoring every candidate against every criterion and
recommending a base; self-scoring by the parent in parallel, reconciled in
`04a-synthesis-json-family.md`. **No dropouts** — all three produced complete
packages and the judge completed all scoring.

Candidate packages preserved verbatim under `03-candidates/json-family-revision/`
(`candidate-a-embedded-jaq.md`, `candidate-b-jsonc-parser-cst.md`,
`candidate-c-markers-jsonc.md`, `cross-judge-report.md`) alongside the four
research reports the candidates cited.
