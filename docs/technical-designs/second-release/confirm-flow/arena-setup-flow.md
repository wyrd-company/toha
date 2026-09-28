# Confirm-flow arena round 2 (flow node) — setup and blindness record

This records the round-2 (`flow` node) arena's runners, capsules, and blindness
evidence. It is written **retrospectively** (as part of the doc-correction task
1097) and deliberately distinguishes three kinds of statement: evidence retained
from the time of the round, evidence obtained by inspecting surviving artifacts
now, and what the surviving evidence cannot establish. Round 1's audit is in
`arena-setup.md`; that document is unchanged.

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
  the shared export. Each candidate wrote `design.md` and `rationale.md` into its
  own capsule.

## Blindness evidence

### Retained contemporaneous evidence (recorded at the time)

- The shared export was audited when round 1 created it (`arena-setup.md`): it is
  a `.git`-free `git archive` of `a62061f`; the confirm-flow package (which holds
  this arena's `rubric.md`, cross-judge, scores, and candidate outputs) does not
  exist at `a62061f`, so it is absent from the export; all other features'
  design/scoring packages were stripped, leaving only the `error-attribution` and
  `template-defaults` predecessor `design.md` files.
- The round-2 capsule-creation step copied only `grounding.md`,
  `grounding-flow.md`, the flow `runner-task.md`, `runner-prompt.md`, and
  `rationale-template.md` into each flow capsule and symlinked the shared export.
  `rubric.md` was **not** copied.
- The round-2 progress note recorded the reuse and blindness only as a one-line
  "reusing the clean a62061f export, blindness re-audited" — i.e. no dedicated
  round-2 audit artifact was produced at launch. That gap is the reason this
  document exists (retrospective finding 3 on task 1077).

### Retrospective inspection (performed now, from surviving artifacts)

The `/tmp/arena-confirm-flow/` capsules and export survived and were inspected:

- No `rubric.md`, `cross-judge*`, or `own-score*` file exists in any flow capsule
  (`flow-1/2/3` each contain only their own `design.md`/`rationale.md` plus the
  copied grounding/task/runner-prompt/rationale-template and the `toha-src`
  symlink).
- The shared export still contains no `docs/technical-designs/second-release/confirm-flow`
  path, no `.git`, is read-only (`-r--r--r--`), and carries only the
  `error-attribution` and `template-defaults` predecessor packages. A fresh search
  for this arena's rubric/cross-judge/own-score criteria strings over the export
  returned nothing reachable.
- The `grounding.md`/`grounding-flow.md` copied into the capsules refer to the
  existence of a *withheld* scoring rubric in prose (e.g. "candidates receive the
  common task and grounding, not the scoring rubric") but do not contain the
  rubric's criteria.

### What the surviving evidence cannot establish (unknowns)

- No launch-time blindness audit was run for round 2, so the isolation rests on
  (a) round 1's export audit, (b) the known capsule file set, and (c) this
  retrospective inspection of surviving artifacts — not on an audit performed at
  the moment the candidates launched.
- Retrospective inspection observes the *surviving* state of the capsules and the
  read-only export, which matches the recorded creation commands; it cannot prove
  the exact launch-time state independently of those commands, nor that no
  transient reachable material existed and was later removed. No evidence of any
  such material was found.

## Assessment

On the retained and retrospective evidence, the round-2 blindness held: no
component of this arena's rubric, cross-judge prompt, scores, or prior-candidate
output was reachable from a flow capsule, and the reused export carried no
reachable branch history. The honest gap is procedural — round 2 did not emit a
per-round audit artifact at launch — which the arena skill now requires for every
round (see the `docs(arena): re-audit blindness every round` update).
