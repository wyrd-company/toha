---
relationships:
  depends-on: headless-recovery
  informs: headless-recovery
---

# Headless recovery arena setup

## Phase checklist

- [x] Frame the common candidate task and isolate the permitted evidence.
- [x] Fan out two blind candidates.
- [x] Cross-judge both complete candidates read-only.
- [x] Score each candidate independently and pick a base.
- [x] Graft useful parts of the losing candidate and record rejections.
- [x] Verify the synthesized design against the command and predecessor contracts.

## Runner selection

`~/.pi/agent/pstack/models.json` is absent, so there is no configured architect or cross-judge pool. Two independent `gpt-5.6-sol` processes at high reasoning are the candidate runners. They receive the same task and capsule concurrently. Native agent capacity was occupied during setup, so isolated foreground Codex CLI runners provide the required independent candidates. The cross-judge uses a different model family after both candidate artifacts are terminal.

## Round 1 blindness audit

Each runner receives its own exported source capsule:

- `/tmp/arena-headless-recovery/round-1/candidate-1/capsule`
- `/tmp/arena-headless-recovery/round-1/candidate-2/capsule`

Both capsules have the same content manifest digest: `d447bef453b2cac4292aa8fccade376aaaf4ee09bf4c6a979199f0500b737f36`. The export itself has no `.git` directory, refs, objects, worktree link, task-board data, rubric, judge prompt, score, or candidate artifact. Its declared source-history boundary is the file snapshot from epic commit `1e663bd661304698ebd10219c8c52d78020a189e`.

The isolation is input packaging plus prompt withholding, not a filesystem sandbox. Both Codex runners used `danger-full-access`; the wider host filesystem and repository history were technically reachable. The rubric did not exist when the corrected processes launched, but it was created in the design worktree while they ran and was therefore technically reachable after creation. The terminal JSON event transcripts are the evidence for actual reads:

- Candidate 1 used relative paths inside its capsule for all task, design, and source evidence. It also read three global operating skills outside the capsule: `memory/SKILL.md`, `authoring-artifacts-documentation/SKILL.md`, and `authoring-specifications/SKILL.md`.
- Candidate 2 used relative paths inside its capsule for all task, design, and source evidence. It also read the global `authoring-specifications/SKILL.md`.
- Neither transcript contains a `git` command, task-board read, worktree path, rubric path, judge path, score path, parent-directory traversal, or other-candidate output read. Neither candidate read the rubric after it was created.

The extra global skill reads supplied general artifact-writing guidance rather than problem facts or evaluation criteria. They are disclosed because they were outside the audited capsule inventory. The two candidates remain valid blind attempts with respect to the withheld rubric, judge, scores, repository history, and each other's output.

The complete permitted file inventory is:

```text
HISTORY_BOUNDARY.txt
INVENTORY.txt
docs/prerequisites/confirm-flow/design.md
docs/prerequisites/confirm-flow/verification.md
docs/prerequisites/error-attribution/design.md
docs/prerequisites/error-attribution/verification.md
grounding.md
instructions/architect.md
instructions/codebase-design.md
instructions/design-red-flags.md
instructions/launch-prompt.md
instructions/rationale-template.md
instructions/runner-prompt.md
src/cli/guidance.rs
src/interview.rs
src/main.rs
src/plan.rs
src/protocol.rs
src/staging.rs
task.md
```

Candidate outputs and execution events use distinct sibling `output` directories. The scoring rubric does not exist anywhere at corrected process launch; it is authored only after both candidate processes start. The judge is not launched until both output artifacts are complete.

The first CLI invocation exited before model execution because the exported capsule is intentionally not a Git repository and the runner required `--skip-git-repo-check`. It produced no candidate artifact. The corrected concurrent launches use process handles `84201` and `25081`; the capsules and manifest remained unchanged.

Both corrected runners exited successfully. Candidate 1 is SHA-256 `27cadb578f57414ad50a602d191eb1ebe9c7166701b599cb8a71ef5d63de363a`; candidate 2 is `ee0cff26d5a2a7aba831cfdbf35e9a3834db3d3532fd471ae65a2740a223cc2a`. The other-family judge completed after both hashes existed.
