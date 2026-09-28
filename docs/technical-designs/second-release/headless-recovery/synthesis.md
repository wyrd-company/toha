---
relationships:
  depends-on:
    - headless-recovery
    - candidate-1
    - candidate-2
  informs: headless-recovery
---

# Headless recovery synthesis

## Base and judge reconciliation

The parent and cross-judge disagreed on the base. The parent scored candidate 1 at 24/25 and candidate 2 at 19/25; the judge scored both 23/25 and preferred candidate 2. The disagreement concerns module scope, not product behavior.

The synthesis uses candidate 1's focused recovery module as the base and grafts candidate 2's missing structural strengths. A single private `commit_transition` policy accepts transitions from both `continue` and headless apply, and a typed `RecoveryOutcome` exposes `Completed` as the only plannable variant. This closes the continue/apply drift and compile-time flow gap that led the judge to candidate 2, without moving target setup, resolution, trust, planning, apply, terminal prompting, and all guidance behind a `Request` that mirrors Clap.

This resolves the judge disagreement by applying the deletion test: deleting the synthesized recovery module redistributes accepted-submission and staged-effect policy across `continue_run` and `run`; deleting candidate 2's whole command module mainly moves the existing command bodies back to `main`. The synthesized module is smaller while still hiding the new complexity.

## Grafts

- **From candidate 2:** `Transition { state, accepted, rejections }` as the common input to one staged commit policy used by continue and apply.
- **From candidate 2:** a result type in which only `Completed` carries a `Completed` value, so pending and ended outcomes cannot reach `Plan::build` without an exhaustive match.
- **From candidate 2:** parity tests compare path-only and matching-template apply through outputs, state, file trees, hooks, and failure injection.
- **From candidate 2:** an exhaustive-match compile guard for the accepted future `Ended` result.
- **From candidate 1:** separate recovery and presentation ownership; routing, trust, planning, and apply remain with current owners.
- **From candidate 1:** an explicit mutation mode that preserves the current CLI preview rule, including no abort removal during simulation.
- **From both:** terminal summaries derive from the same batch and rejection inputs as `protocol::batch_document`; `stage --async` remains explicit machine output.

## Rejections

- **Candidate 1's `--json` flag:** the task requires non-terminal scripts and agents to retain JSON. A new public option adds combinations and a canonical CLI change that is unnecessary to satisfy that requirement. The checkpoint can add it only if Bob wants deterministic JSON while stdout is a pseudoterminal.
- **Candidate 1's separate continue persistence:** rejected because future `Ended` and accepted-prefix handling would remain duplicated.
- **Candidate 1's `run_staged_apply` and `finish_questions` wrappers:** rejected as pass-throughs. Existing command routing consumes the typed recovery result; `Outcome::finish` consumes `Questions` directly.
- **Candidate 2's whole-command `execute(Request, Dirs)` module:** rejected because it groups established command phases by time and mirrors the parsed command interface. The focused policy module supplies the missing depth with less movement.
- **Candidate 2's abort-under-preview rule:** rejected. The existing CLI contract says dry-run changes nothing, so preview reports the ended abort result without removing staged state. A normal run performs the accepted abort removal; this does not reopen adoption of the flow action.
- **Both candidates' continue-then-apply sequence:** rejected because it duplicates outcome handling and creates an intermediate failure seam. No subprocess is introduced.

## Verification result

The synthesized shape carries every required field from its authority to its consumer: `CanonicalTarget` is constructed once and passed to store, context, plan, apply, and removal; origin-bearing `Resolution` is consumed directly by start/replay; `protocol::answer_headless` supplies accepted submissions and engine result; `commit_transition` performs the only staged mutation; `Outcome::Questions` carries the canonical JSON plus a summary derived from the same batch; only `RecoveryOutcome::Completed` reaches planning.

The design answers all three product questions, preserves exit 4 and non-terminal JSON, keeps same-template behavior, and composes with pending/proceed/dry-run/stop/abort/skip. The existing dry-run contract resolves the only cross-contract overlap: simulation performs no staged mutation, including abort removal.
