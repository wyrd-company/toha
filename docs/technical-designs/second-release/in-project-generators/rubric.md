# Rubric — in-project generators

**Withheld from candidates.** Candidates receive only the common task and the
grounding. This rubric is the readonly cross-judge's tool and the orchestrator's
own scoring tool. Score each criterion 1–5 (5 = fully satisfied with gradeable
evidence in the candidate's sketch and rationale).

Derived from the task's observable outcome and invariants (Ground /
project-updates producer contract / presets model / startup policy A).

## C1 — Generate and update are distinct, non-overlapping axes

The design cleanly separates **creating a new application** (generate — usually
a fresh subpath) from **updating an application in place** (`apply --from`, owned
by project-updates / task 1029). A caller can always tell which they are doing.
The generate path never silently becomes an update, never redefines `--from`,
snapshot capture, the three-way merge, or the `snapshots` commands. Reusing a
snapshot as a *default source* is not the same as merging from it.
*5:* the two axes and their non-overlap are explicit in the caller usage and the
module seam. *1:* generate is folded onto `--from` or re-defines a 1029-owned
surface.

## C2 — Snapshot-as-optional-default with a total, route-invariant precedence

Available snapshots seed defaults (policy A); the precedence among
**(a) a selected snapshot's `submissions`, (b) configured presets /
template-defaults, (c) the template's own `default` / a normal question** is
**total** (every question id resolves to exactly one default source or none) and
**identical across terminal, headless, staged, agent, and crate paths**. When no
snapshot is available the design falls back to (b) then (c) without pretending a
snapshot existed. No answer is reused by matching question ids. The precedence is
stated as one rule, not per-route prose.
*5:* one precedence rule covers every id and every route, with the engine seam
named. *1:* precedence is ambiguous, route-dependent, or leaks id-coupling.

## C3 — Snapshot selection, collision, ambiguity, and required-but-absent

Selection is well-defined for a source with **zero, one, and many** snapshots
(the many case arising from several subpath applications). Cross-snapshot
precedence — which snapshot defaults the next application — is explicit
(e.g. a named id/prefix, a "likely base", newest-of-source, or a person prompt),
and the **person / script / agent** routes each resolve it deterministically or
fail clearly without interaction. An explicitly **required** snapshot reference
that is unknown, ambiguous, or absent **fails clearly** (exit 1, message naming
the resolving command such as `toha snapshots list`), never a silent empty
default.
*5:* a selection table across counts × routes with an explicit failure contract.
*1:* selection is implicit, non-deterministic on the script/agent routes, or a
required-absent reference degrades silently.

## C4 — Per-application identity without a second answers lifecycle

The design defines what identity a generator application carries — sufficient to
distinguish repeated applications and to key snapshot lookup — and derives it
from the existing `source` + target subpath rather than inventing a new
persisted-answers store in the project. It works in non-git and dirty targets
(where no snapshot exists) by falling back, not by fabricating identity. No
independent second persisted-answers lifecycle is introduced.
*5:* identity is total, reuses snapshot `source`/`target`, and degrades cleanly
off-git. *1:* a new in-project answers file or a parallel lifecycle appears, or
identity is undefined off-git.

## C5 — Interface depth, seam placement, engine purity (codebase-design)

The new surface hides complexity behind a small, deep public interface; reuses
the per-target apply spine (`setup → canonical_target → build_context →
start_with_context → Plan::build → apply_reporting`) rather than forking it;
keeps the interview engine pure (a snapshot default enters only through the
`Seed`/`DefaultBankEntry` boundary); consumes the snapshot **reader** contract
without editing 1029-owned surfaces; and prefers dependency acceptance and
returned results over hidden construction or side effects. If it extends
`DefaultBankEntry` to carry ordered fallback, the change stays inside the engine
boundary and is testable through the same interface callers use. No hypothetical
seam with no real variation.
*5:* small deep interface, correct seam, engine purity preserved, testable as
callers use it. *1:* shallow pass-throughs, engine leakage, or edits to
1029-owned code.

## C6 — Falsifiable validation, including the mandated twice-in-one-project fixture

The design enumerates falsifiable behaviors and specifies the **required
fixture**: applying the same generator **twice with different answers in one
project** (two subpaths), asserting each application's own answers and output.
Every load-bearing rule has a sole-kill (precedence order; selection across
counts; fallback-when-absent; required-absent/ambiguous failure; no id-coupling;
engine purity). The compatibility statement names the exact consumed revisions
and the canonical-document edits the paired implementation (1030) will own.
*5:* falsifiable behaviors + the fixture + a sole-kill per load-bearing rule.
*1:* behaviors are vague, the fixture is missing, or load-bearing rules are
unguarded.

## Red-flag screen (applied before scoring; see design-red-flags.md)

Reject or revise: shallow modules, information leakage (snapshot/gitoxide types
or engine internals surfacing in a public generator signature), temporal
decomposition (a "step 1 / step 2" split with no independent value), pass-through
methods that only forward to the apply spine.
