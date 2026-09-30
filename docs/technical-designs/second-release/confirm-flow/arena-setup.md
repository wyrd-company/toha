# Confirm-flow arena — setup, runners, and blindness audit

## Runners

No `~/.pi/agent/pstack/models.json` is present, so arena defaults apply: four
`inherit-parent` candidate runners and an other-family readonly cross-judge.
Candidates run on the parent model via `general-purpose` subagents. The
cross-judge runs on a GPT-family model (`ocx-gpt-*`), read-only, preferring a
different family from the parent per the arena skill.

To guarantee the task's "at least two structurally distinct viable candidates",
each candidate is seeded with a distinct **whole-shape structural direction** for
the core design axis — how the confirm action outcome is represented in the
interview state machine and carried to the drivers. Seeding directions is the
arena's "run another candidate direction" provision; it is framing, not the
scoring rubric. Each candidate may reject its direction if grounding shows it
unworkable (that rejection is itself signal).

- Candidate 1 — **new `Interview` variant**: a third terminal state
  (`Interview::Stopped`/equivalent) alongside `Asking`/`Complete`, produced by
  `advance`.
- Candidate 2 — **outcome field on `Completed`**: two `Interview` variants kept;
  `Completed` gains a control directive (proceed / stop / dry-run); skip is
  realized through the existing walk skip machinery.
- Candidate 3 — **separate control signal from the answer transaction**: a
  dedicated `InterviewOutcome`/`Directive` value returned beside the interview,
  unifying stop/dry-run/skip, with a new protocol `status`.
- Candidate 4 — **action as a node-level effect**: the confirm action is modeled
  like a reached message/hook effect within the walk; skip reuses group-skip
  directly; terminal outcome is a new `status`.

## Capsules and output paths

- Root: `/tmp/arena-confirm-flow/`.
- Read-only shared source export: `/tmp/arena-confirm-flow/toha-src/`, produced by
  `git archive a62061fdce74b6b1a743c70565dd9fbeec2413bb` of `src/`, `docs/`
  (specifications, guides, examples), `tests/`, `Cargo.*`, `AGENTS.md`,
  `README.md`. Made read-only (`chmod -R a-w`).
- Per-candidate capsule `/tmp/arena-confirm-flow/candidate-<n>/` holds copies of
  `grounding.md`, `runner-task.md`, `runner-prompt.md`, `rationale-template.md`,
  and a symlink `toha-src` → the shared export. Each candidate writes only
  `design.md` and `rationale.md` into its own capsule; capsules never share a
  writable path.

## Blindness audit (Phase A, before launch)

- History boundary: the export is `git archive` of commit
  `a62061fdce74b6b1a743c70565dd9fbeec2413bb`; it carries no `.git`, so no branch
  history (including this task's branch `1077/design-confirm-flow`, which holds
  the rubric) is reachable from a capsule.
- The confirm-flow design package (which contains `rubric.md`, and will contain
  cross-judge/own-score/synthesis/candidate outputs) exists only on the task
  branch and is **absent** from `a62061f`; verified the export has no
  `docs/technical-designs/second-release/confirm-flow` path.
- All other features' design/scoring packages (their rubrics, cross-judge,
  scores, candidates, decks under `docs/technical-designs/*` and `docs/design/*`)
  were stripped from the export. Only the two approved predecessor **design.md**
  files that grounding references (`error-attribution`, `template-defaults`) and
  `architecture.yml` were restored — these are allowed inputs.
- Re-audit for `rubric | cross-judge | own-score | arena scoring | grade down |
  synthesis decision` over the export returned only the predecessor
  `template-defaults/design.md` "Synthesis decision" rationale header — a section
  of an allowed predecessor design, not this arena's scoring.
- Withheld from every capsule: this arena's `rubric.md`, the cross-judge prompt,
  all scores, and any candidate's output. Confirmed absent and unreachable.

Candidates receive: the common task (`runner-task.md`), grounding
(`grounding.md`), runner discipline (`runner-prompt.md`, `rationale-template.md`),
and read-only source. Nothing else.

## Erratum — reachability framing corrected

This erratum narrows two claims above; the original text and its verified facts
are preserved unchanged. Scope exception: correcting `arena-setup.md` is outside
this correction's original findings (the final synthesis and the
round-2 audit), and is authorized only for this narrow reachability correction —
no other change to the round-1 evidence.

- **"reachable from a capsule" and "absent and unreachable" overstate isolation.**
  The verified facts stand: the shared export is a `.git`-free `git archive` of
  `a62061f` that contains no `confirm-flow` package, no rubric, no scores, and no
  candidate output, and none of this arena's scoring material was copied into any
  capsule. What does not follow is *unreachable*: a capsule was a directory on a
  shared filesystem, and the candidate runners were `general-purpose` subagents
  with full filesystem tools. This arena's `rubric.md` (committed to the design
  worktree before the candidates launched) and the sibling capsules' outputs
  existed on the same filesystem and were reachable in principle. Blindness rested
  on the **supplied input set** (no rubric, scores, or candidate output copied
  into a capsule) plus **runner discipline** (the launch prompt's instruction to
  stay inside the capsule), **not** on filesystem isolation, and is not proven by
  isolation.
- **Transcript evidence (retrospective).** The four round-1 candidate transcripts
  survive; each references `rubric.md` zero times. A small number of
  sibling-capsule and `/workspaces` confirm-flow path strings also appear in each
  transcript; these have not been classified as genuine reads versus launch-prompt
  or echoed text, so whether any round-1 candidate read outside its capsule is not
  established either way. Nothing enforced confinement.
