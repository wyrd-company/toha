---
relationships:
  depends-on: project-updates
  informs: project-updates
---

# Project-updates arena rubric

Authored after all blind candidate runners were live. Candidates did not receive
it; they received `runner-task.md` and `grounding.md` only. Each criterion is
scored 0–5 for a total of 25. Criteria are derived from the task's observable
outcome (a project moves from template version A to B, re-rendered with the same
answers, reconciled with operator edits) and its invariants.

## C1 — Applied record: shape, ownership, durability

An unambiguous recommendation among a project-root file, a git commit trailer,
and a custom git ref/note, with reasoning on traveling with the project (survive
clone, move, re-checkout). The record captures source formal name, resolved
commit, accepted answers, target, and the per-target ownership/version data a
merge needs. It does not collide with `.toha.yml` (local config) and is not
conflated with the XDG staged record. The on-disk record is treated as
operator-editable, untrusted input parsed once at the boundary into domain
types.

## C2 — Three-way merge semantics per ownership identity

Base (what Toha last wrote), ours (what Toha writes now at B), and theirs (the
operator's on-disk file) are defined precisely. Whole, Region, and JsonValue are
each merged with their own semantics and never collapsed into whole-file: Region
respects the checksum drift boundary; JsonValue converges without `--force`;
Whole is a genuine three-way text merge with a defined conflict representation.
An operator-edited file, a disappeared ownership identity between A and B, and
idempotency (a second update with no change is a byte no-op) are all resolved.

## C3 — Merge engine and dependency discipline

A concrete, named in-process merge engine, consistent with `gix` already being a
dependency. No application subprocess (or, if one is proposed, it is flagged as
requiring explicit separate operator approval). No new permission/access rule,
timeout, or pinned-version check introduced without disclosure. Any proposed
third-party dependency is named and linked and justified against build-vs-buy.

## C4 — Composition, purity, and route preservation

Composes with the approved prerequisite interfaces without silently changing
them: the injection `FileMutation` ownership view, the 1070 identity model (the
applied record is distinct from both the `{template, answers}` document and the
XDG staged record; answers are not target/commit-bound), and the 1078 per-apply
hook-result lifetime (update re-runs hooks under the existing trust gate). The
pure interview engine and every driver route (terminal, headless, async, staged,
direct, crate) keep working. Adoption of a record-less project is defined.

## C5 — Interface depth, proof, and implementability

Caller usage is written first and the usage, types, signatures, module/seam map,
and behavior matrix agree. The public surface is a deep module with a small
interface, not a set of stages the caller sequences. The error/results contract
is exhaustive. Each load-bearing claim has a falsifiable behavior, including the
required version-A-to-B edited-file merge-conflict fixture, and the canonical
document/schema impact is named. No hypothetical seams, no new permissions/
timeouts/pinned checks/subprocesses without disclosure.
