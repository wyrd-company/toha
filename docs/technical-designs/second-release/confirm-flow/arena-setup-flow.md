# Confirm-flow arena round 2 (flow node) — setup and blindness record

This records the round-2 (`flow` node) arena's runners, capsules, and blindness
evidence. It distinguishes three kinds of statement: evidence retained from the
time of the round, evidence obtained by inspecting surviving artifacts, and what
that evidence cannot establish. Round 1's setup and audit are in `arena-setup.md`.

## Runners

Three blind candidate runners (flow-1, flow-2, flow-3) on the parent model via
`general-purpose` subagents, each seeded a distinct whole-shape structural
direction for the standalone `flow` node (minimal node; attachable node+group
guard; node + unified outcome). One other-family read-only cross-judge
(`ocx-gpt-5-6-sol`), whose report is `cross-judge-flow.md`. The parent's own
scoring is `own-score-flow.md`. Candidate outputs are preserved under
`candidates-flow/flow-{1,2,3}/`.

## Capsules and shared export

- Shared read-only source export: `/tmp/arena-confirm-flow/toha-src/`, the same
  export round 1 produced (`git archive` of `a62061f`, then stripped of every
  design/scoring package except the two approved predecessor `design.md` files
  the grounding references), made read-only.
- Per-candidate capsule `/tmp/arena-confirm-flow/flow-<n>/` held copies of
  `grounding.md`, `grounding-flow.md`, `runner-task.md` (the flow runner task),
  `runner-prompt.md`, and `rationale-template.md`, plus a symlink `toha-src` to
  the shared export. `rubric.md` was not copied. Each candidate wrote `design.md`
  and `rationale.md` into its own capsule.

A capsule is a directory, not an access boundary. The runners were
`general-purpose` subagents with full filesystem tools; the launch prompt
instructed each to read only inside its capsule and the read-only `toha-src`
export and not to read `/workspaces` or other capsules, but nothing enforced that.

## Blindness evidence

### Retained contemporaneous evidence

- The shared export was audited when round 1 created it (`arena-setup.md`): it is
  a `.git`-free `git archive` of `a62061f`; the confirm-flow package (which holds
  this arena's `rubric.md`, cross-judge, scores, and candidate outputs) does not
  exist at `a62061f`, so it is absent from the export; all other features'
  design/scoring packages were stripped, leaving only the `error-attribution` and
  `template-defaults` predecessor `design.md` files.
- The round-2 capsule-creation step copied only `grounding.md`,
  `grounding-flow.md`, the flow `runner-task.md`, `runner-prompt.md`, and
  `rationale-template.md` into each flow capsule and symlinked the shared export.
  `rubric.md` was not among the supplied inputs.
- No dedicated round-2 blindness-audit artifact was produced at launch; the reuse
  and blindness were noted only in one line of a progress log.

### Retrospective inspection (from surviving artifacts)

- **What the supplied `grounding-flow.md` disclosed.** `grounding-flow.md` was
  copied into every flow capsule. It does not contain the rubric's criteria, but
  it names their existence: its lines 10–11 list the retained round-1 "grounding,
  rubric, runner-task, candidates 1‑4, cross‑judge, own‑score", and a later line
  states the round-2 criteria are the "unchanged six". A runner reading it is told
  that a scored rubric and scored candidates exist as retained round-1 artifacts.
- **Capsule contents.** The surviving `flow-{1,2,3}` capsules contain only each
  candidate's own `design.md`/`rationale.md`, the copied
  grounding/grounding-flow/runner-task/runner-prompt/rationale-template, and the
  `toha-src` symlink. No `rubric.md`, `cross-judge*`, or `own-score*` file is in
  any flow capsule. The shared export still has no `confirm-flow` package, no
  `.git`, is read-only, and carries only the two predecessor `design.md` files.
- **Runner transcripts (surviving).** All three flow runner transcripts survive.
  Inspecting each runner's recorded file operations: none references `rubric.md`
  (zero occurrences); each runner's reads are within its own capsule and the
  read-only `toha-src` export; no read of a sibling capsule or a `/workspaces`
  path appears (the only `/workspaces` occurrence is the launch prompt's own
  prohibition). This is a retrospective read of the recorded tool calls; it
  evidences the runners' recorded behaviour, not an enforced sandbox.

### What the surviving evidence cannot establish (unknowns)

- **Whether any runner read outside its capsule.** Nothing enforced confinement.
  The runners had full filesystem tools; the `runner-prompt.md` discipline asks
  only for "independence between candidates"; and on the same filesystem the
  rubric (`rubric.md`, committed to the design worktree before round 2), the
  scores (`own-score.md`, `cross-judge.md`), and the round-1 candidate outputs
  (readable siblings at `/tmp/arena-confirm-flow/candidate-{1..4}/`) all existed
  and were readable in principle. The surviving transcripts show no such read, but
  the environment did not prevent one and the transcript is not proof of an
  enforced boundary.
- **Launch-time state.** No blindness audit ran at launch; the retrospective
  inspection observes the surviving state of the capsules, export, and transcripts
  (which matches the recorded creation commands), not the exact launch-time state
  independently of those commands.

## Assessment

The supplied input set carried no rubric criteria, no scores, and no candidate
output. Blindness therefore rests on **runner discipline** — the instruction to
stay inside the capsule plus not supplying the scoring material — **not on
filesystem isolation**: the capsule was a directory on a shared filesystem where
the rubric, the scores, and the round-1 candidate outputs existed and were
reachable in principle, and the round-2 criteria were the same six as round 1. The
surviving runner transcripts are consistent with that discipline (no `rubric.md`
read; operations confined to the capsule and `toha-src`), but isolation was not
enforced and is not proven. The procedural gap — no per-round audit artifact at
launch — is what the arena skill now requires for every round (see the
`docs(arena): re-audit blindness every round` update).
