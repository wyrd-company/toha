---
relationships:
  depends-on:
    - headless-recovery
    - candidate-1
    - candidate-2
  informs: headless-recovery
---

# Parent scoring: headless recovery arena

Both candidate artifacts were read end to end before scoring. Scores use the 0–5 rubric in `rubric.md`.

| Criterion | Candidate 1 | Candidate 2 |
|---|---:|---:|
| R1 — Product answers and output compatibility | 4 | 5 |
| R2 — Deep ownership seam | 5 | 3 |
| R3 — Transaction, persistence, and effect ordering | 5 | 3 |
| R4 — Accepted flow composition and route equivalence | 5 | 4 |
| R5 — Implementability and proof quality | 5 | 4 |
| **Total** | **24** | **19** |

## Candidate 1

Candidate 1 is the stronger base. Its `answer_apply` interface concentrates the actual new policy: adapt the existing headless transaction, append only accepted submissions, choose the staged effect, and return a result that makes planning possible only for completion. A separate presentation module is justified by two real adapters, terminal text and protocol JSON. Routing, trust, planning, and apply remain with their current owner.

Its main deduction is the proposed `--json` option. The task requires non-terminal scripts and agents to retain JSON; it does not establish a need for a new output-format option. The extra flag expands the public command interface and creates more command permutations. The synthesis drops it and treats the stdout terminal test as the requested product rule. Candidate 1 also sketches more private types than the final design needs; `PostApply` can remain an existing-record fact held by the caller.

Red-flag screen:

- **Shallow module:** pass. `answer_apply` hides a complete transaction and persistence policy.
- **Information leakage:** pass after dropping `--json`; protocol documents remain in `protocol`, staged mutations in the recovery policy, and presentation derives from the same batch.
- **Temporal decomposition:** pass. The module groups recovery knowledge rather than load/validate/save phases.
- **Pass-through methods:** revise. `run_staged_apply` and `finish_questions` add little if the caller can consume the recovery result and `Outcome::finish` can render `Questions` directly.

## Candidate 2

Candidate 2 gives concise caller behavior and avoids the extra public output flag. Its command-level parity tests and exhaustive `Ended` match are useful. The proposed `cli::interview_command::execute(Request, Dirs)` is too broad: the request repeats the Clap command shape and the module claims target setup, routing, resolving, replay, answer submission, persistence, flow routing, planning, apply, and guidance. Deleting it would mostly move the current `run` and `continue_run` bodies back to `main`, so its one-method interface overstates its depth.

Its material defect is the rule that `Ended(Abort)` still removes the staged record under CLI `--dry-run`. The existing flag promises to "change nothing" and currently suppresses accepted-submission storage and staged cleanup. The accepted flow design requires abort removal in normal execution and does not override the caller's preview mode. Candidate 1's explicit mutation mode preserves both contracts without reopening the adopted abort action. Candidate 2 also claims trust failures do not consume the answers document, although trust is evaluated only after interview completion and plan preparation in the current command flow.

Red-flag screen:

- **Shallow module:** revise. `Request` mirrors the command and `execute` hides a relocated command body more than a focused policy.
- **Information leakage:** pass. Existing target, resolution, and protocol owners remain authoritative.
- **Temporal decomposition:** revise. The module gathers sequential command phases whose only shared fact is execution order.
- **Pass-through methods:** revise. `prepare_context`, `replay_staged`, and answer adapters largely forward existing interfaces unless the implementation gives each concrete policy ownership.

## Parent base decision

Use candidate 1 as the base. Remove the new `--json` option and the pass-through wrappers. Graft candidate 2's automatic terminal-only selection, single staged-source normalization, exact route-parity matrix, and exhaustive compile-fail check for a missing `Ended` arm. Preserve candidate 1's preview-safe mutation mode, narrower recovery seam, and separate final presentation adapter.
