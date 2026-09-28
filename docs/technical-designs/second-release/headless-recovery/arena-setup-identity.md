# Identity redesign arena setup

## Round purpose

The required template identifier changes the public input shape and the crate
boundary. A fresh round explores that architecture without rewriting the
original round's candidate artifacts.

## Candidate frames

- **Candidate 1 — document-first boundary.** Explore a parsed identity-bearing
  document that must be verified before it can yield raw answers. Concentrate
  transport ownership in the protocol module while keeping the pure engine
  unchanged.
- **Candidate 2 — route-bound capability.** Explore binding expected formal
  identity to document parsing or verification so a valid submission is a
  capability that file-driven routes cannot construct before comparison.
  Concentrate cross-route enforcement and compile-time misuse resistance.

Each candidate must solve the whole task. The frames force distinct seam
placement; neither prescribes the envelope or final answer.

## Capsule boundary

Each runner receives its own directory under
`/tmp/arena-headless-identity/round-2/`. The capsules contain identical copies
of the common task, revised grounding, relevant current source and
specification files, accepted flow and error-attribution designs, architect
runner instructions, and rationale template. Only `direction.md` differs.

The capsules contain no `.git` directory, task board, scoring rubric, judge
prompt, parent score, other candidate output, original-round candidate, or
original-round synthesis. Candidate output and event transcripts use separate
paths.

The runner processes use normal host filesystem access. The directory capsule
is input packaging and prompt discipline, not enforced filesystem isolation.
The terminal event transcripts and before/after capsule manifests are the
evidence for what each runner actually read and whether it modified supplied
input. The final audit records all reads outside the capsule, unknowns, process
handles, model configuration, and file hashes. It does not claim that withheld
files were technically unreachable.

## Runner selection

No configured architect runner pool is present. Two independent foreground
Codex CLI processes use `gpt-5.6-sol` at high reasoning in separate working
directories. An other-family read-only cross-judge starts only after both
candidate artifacts are terminal. The scoring rubric is created after both
candidate runs finish, so it does not exist during candidate generation.

## Terminal run record

| Candidate | Foreground process handle | Codex thread | Terminal result | Artifact |
|---|---:|---|---|---|
| Document-first boundary | `89865` | `01a0ea2f-efe4-7ff2-a420-4f1528f7d40c` | Exit 0; `turn.completed` | `candidates/candidate-identity-1.md` |
| Route-bound capability | `41266` | `01a0ea2f-efd8-73a2-818c-35ee67a79866` | Exit 0; `turn.completed` | `candidates/candidate-identity-2.md` |

Both processes used `codex exec --json -m gpt-5.6-sol` with high reasoning,
`danger-full-access`, and separate capsule working directories. The ordinary CLI
configuration emitted a nonfatal global skill parse warning. Neither candidate
reported a task failure.

The candidate artifacts are terminal and immutable evidence:

| Artifact | Lines | Bytes | SHA-256 |
|---|---:|---:|---|
| `candidate-identity-1.md` | 802 | 32,848 | `3028c72482dcf69abad2dd07527bd51490e9c9d0f9206cdc23d2ca4b1fa5ceb3` |
| `candidate-identity-2.md` | 670 | 31,272 | `13e8a42fcaeb90a175215d11e1df27ffae7bee4379202543d964d555a587a415` |

Event transcript hashes are:

- candidate 1: `e96bab053ddaad0f47ee19051f56bdc38c767a1a29dff766e33142c2e43f4ff7`;
- candidate 2: `6417ee883ab65f69c018688408d554cbce4a6c47bce71b3ca2c828a361ce309a`.

## Capsule integrity

Each manifest contains 24 input files. The before and after manifests compare
byte-for-byte equal for both candidates. Their SHA-256 values are:

- candidate 1: `f2041064e33499dfd92aba05f68916851d4759d9b93c2d8145a9e47f5efeae97`;
- candidate 2: `6f913fd5756532ab759f432065c38a8daca9e79775228507f88764c67ff883c7`.

The hashes differ because each capsule has a different `direction.md`. Candidate
outputs and transcripts were written beside, not inside, the input capsules.

## Actual read audit

The event transcripts show candidate 1 reading only capsule inputs plus:

- `~/.agents/skills/authoring-artifacts-documentation/SKILL.md`;
- `~/.agents/skills/authoring-specifications/SKILL.md`.

It attempted to read two relative source paths absent from its capsule; those
reads failed and did not leave the capsule.

The transcripts show candidate 2 reading only capsule inputs plus:

- `~/.agents/skills/memory/SKILL.md`;
- `~/.agents/skills/authoring-artifacts-documentation/SKILL.md`;
- `~/.agents/skills/authoring-specifications/SKILL.md`.

Neither transcript shows a read of repository history, the task board, the
identity scoring rubric, judge material, original-round candidates or synthesis,
parent scores, the active worktree, or the other candidate output.

This is observed read evidence, not a filesystem-isolation claim. Normal host
access made withheld paths technically reachable.

## Rubric and cross-judge timing

Both candidate processes reached `turn.completed` before
`rubric-identity.md` was created. The read-only other-family judge then received
only revised grounding, runner task, rubric, and both terminal candidate files.
It completed as async session `7559ff37-b315-4755-9edc-a67fed21f79a`, Claude
session `d585bce6-fa2d-47c4-9b36-8f12e4a11e14`, with no reported problems. Its
scores and parent disposition are preserved in `cross-judge-identity.md`.
