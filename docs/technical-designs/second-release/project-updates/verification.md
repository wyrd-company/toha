---
relationships:
  depends-on: project-updates
  informs: project-updates
---

# Verification — project updates

This file checks `design.md` and `design.yml` against the confirmed outcome
frame, the approved product decisions, the approved prerequisite designs, the
code at `epic/second-release` @ `7a966c6`, and the gitoxide spike.

## Prerequisite revisions consumed

| Prerequisite | Artifact | SHA-256 at `7a966c6` |
|---|---|---|
| Content injection | `content-injection/05-design.md` | `33f2f943e9a1b0165c690d1779499b8ea1c69130eb10f30e11f71ab7b0229eec` |
| Caller routes and answers identity | `headless-recovery/design.md` | `a7e081784604e78c8349baa9f33fdb456c615074c2319d7f2eb6d1724b409ed5` |
| Hook results | `hook-results/design.md` | `c3be45a767bcb98046d7130f89bf09a9ecd42575216700d88a4f1b9bac4b8d96` |

- **Content injection.** Apply-time ownership is unchanged: the throwaway
  checkout applies the plan with the approved resolvers. Update behaviour for
  owned regions and values was left to this design; the design merges owned
  content by line like the rest of the file (overlapping edits conflict) and
  asks for approval of that choice (`owned-content-merge`). Retraction reuses
  the resolvers and never deletes an operator file. A `.json` target keeps its strict check because the
  resolver runs unchanged.
- **Caller routes.** The three routes, the result statuses, the exit table, the
  staged refusal, and identity checking through `parse_and_verify` are reused.
  The staged record gains one optional member, `base`.
- **Hook results.** Hooks run in the existing hook loop inside the throwaway
  checkout, so result availability and deferred rendering are unchanged. Hook
  results still last one apply.

## Outcome frame

| Frame item | Where the design meets it |
|---|---|
| Problem: template changes cannot reach a generated project without overwriting edits | Snapshots; Applying from a snapshot; The merge |
| No operator edit lost without consent | Merge by git rules; conflicts in index; clean target required; occupied-path refusal; re-check under the index lock; rollback and target-scoped recovery; injection retraction never deletes a file; behaviors 1–4, 10–12, 22–24 |
| Interview engine free of UI; every route works | Routes; staged `base`; update replay adapter over the unchanged engine; behaviors 13, 14, 19 |
| Hooks only under trust | Throwaway checkout step 4; dry run runs no hook; behavior 20 |
| Injection ownership and answers-document contracts unchanged | Throwaway checkout step 3; Answers; prerequisite notes above |
| No new subprocess, permission, timeout, or pinned check | Repository access removes driver commands in memory; behavior 29; `design.yml` disclosures |
| Contract changes updated in the same change | Compatibility and canonical documents |
| One command brings a project to a newer version | `apply --from` |
| Recorded questions are not asked again | Answers; behavior 13 |
| A pre-existing project can join once | Baseline; behavior 18 |
| Preview before writing; a second run changes nothing | Dry run; already current; behaviors 1, 15, and 20 |
| Acceptance scenario across two clones | Behavior 1 |

## Approved decisions

Every entry of `approved-decisions` in `design.yml` maps to a section of
`design.md`:

| Decision | Section |
|---|---|
| git-required-for-update, no-update-command | Purpose; Preconditions; Routes |
| base-is-a-snapshot, parentless-snapshots, snapshot-identity | Snapshots |
| snapshot-selection, no-version-ordering | Person route; `snapshots list` |
| clean-target-for-from, monorepo-scope | Preconditions; When a snapshot is saved; The merge |
| toha-never-commits, conflicts | The merge |
| hooks-in-throwaway-checkout | The throwaway checkout |
| automatic-snapshots | When a snapshot is saved |
| snapshot-commands, sharing | Snapshot commands; `toha init` |
| adoption | Baseline |
| answers, version-checks | Answers |
| routes | Routes |
| onboarding-deferred | Out of scope (task 1108) |

## Code facts relied on

- Replay must use raw submissions: `format` runs before an answer is stored.
- Replay must filter to the rendered template's question ids: unknown ids are
  rejected.
- A folder template has no commit: `snapshot.json` allows `commit: null`.
- Plain apply refuses existing files without `--force`: `--baseline` is the
  only merge into existing files, and plain apply is unchanged.
- The throwaway checkout applies with overwrite semantics because it starts as
  a copy of `HEAD`, where the target's files already exist.
- `inquire::MultiSelect` exists for the `snapshots clean` list.

Citations are in `grounding.md`.

## Spike-backed feasibility

Every git operation the design uses is shown in process by the spike, with the
added gix features `merge` and `status` (see `grounding.md`). The spike is
evidence of feasibility, not proof of the final behaviour:

- It merged with the template as "ours"; the design uses the operator as
  "ours". Swapping the trees is mechanical; behavior 1 asserts stage 2 holds
  the operator's bytes.
- It used one subdirectory target. The repository-root target, empty and
  removal-only snapshots, file/directory conflicts, symbolic links, and CRLF are
  implementation fixtures (behaviors 4, 22, 25).
- Its binary case checked index stages, not the bytes left on disk; behavior 3
  and 22 check the operator's bytes.
- Its no-process proof covers fixtures without drivers; behavior 29 adds
  configured drivers inside and outside the target.
- Removing driver values from the in-memory configuration was not exercised by
  the spike; it is the first implementation check.

Corrections the spike forced into the design: plain `git commit` refuses after
the merge while `git commit -a` does not, as after `git merge`; the merge uses
whole-repository trees with snapshot content grafted at the target; the index
cached tree is dropped after entries change; driver programs are a process path.

## Cross-review

A read-only reviewer from another model family reviewed the design twice. The
second pass checked each first-pass finding and raised four more (released
edit paths, ownership transitions, the replay outcome and resume contract, and
a dry run built by the same code as the update); all are addressed. The
first-pass findings are addressed as follows: injected files are recorded
with their owned regions and values and are retracted, never deleted; every plan
target is captured, including unchanged ones; ignore rules exclude only
untracked files; the template is matched by source identity; answers go through
an update replay adapter; plans are applied through a destination seam; owned
content merges by line; driver values are removed in memory at every entry
point; capture keeps links and modes and the snapshot boundary checks tree
shape; a dry run runs no hook; HEAD, the index, and the target are re-checked
under the index lock; a write failure rolls back and recovery is scoped to the
target; occupied paths refuse; guards map to single behaviors.

## Falsifiability

Each behavior in `design.md` names an observable outcome, and each sole-kill
guard names the behavior it fails. Behaviors 1, 26, and 28 need two clones or a
rewritten history; 23 and 24 need fault injection between steps; the others run
in one temporary repository.

## Gaps

- Removing driver values in memory is unproven until the first implementation
  check; if gitoxide cannot do it, the design returns for review.
- `likely base (by content)` is a heuristic. It only marks a list entry and
  never selects a base.

## Candidate lifecycle and structured validation

The transaction's unsuccessful post-capture exits remove only their own
candidate ref, with compare-before-delete ownership. Behavior 31 compares the
complete ref map and preserves operator state; its omission mutation must
fail at each exit. Cleanup failure is a separately tested error, not a promise
that ref removal cannot fail. Successful conflicts and reporting errors after
transaction commit retain the snapshot.

The governing artifact type is
`/workspaces/context/refinery-ontology/realization/design/technical-design.yml`.
The complete structured instance validates against its objective, background,
design-challenges and proposed-design constraints, including the required
`high-level-overview`. No external schema references occur in that type.
Reproduce the validation from the repository root:

```sh
python3 - <<'PY'
from pathlib import Path
import jsonschema
import yaml
schema = yaml.safe_load(Path('/workspaces/context/refinery-ontology/realization/design/technical-design.yml').read_text())
instance = yaml.safe_load(Path('docs/technical-designs/second-release/project-updates/design.yml').read_text())
jsonschema.Draft202012Validator(schema).validate(instance)
print('PASS complete governing technical-design validation')
PY
```

The command passes. Runtime cleanup witnesses and omission mutations belong
to the paired implementation; this documentation change does not claim they
have run.
