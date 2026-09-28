# Candidate 3 rationale — Explicit stages explored; minimal surface

## Why this shape (and why the honesty)

The task says to "settle real hook phases." This candidate takes that literally
and asks: do authors need to name phases? After tracing the runtime, the answer
is no — hooks already run in a single deterministic order (interview hooks, then
top-level, in list order). A `stage` label would introduce a *second* ordering
the author must keep consistent with list order, and any mismatch is a new error
class for no new expressive power. So the candidate rejects explicit stages and
instead asks the sharper question the others assume away: **which surfaces should
be able to read a result at all?**

It argues for the narrow answer — gating (`when`) and reporting (after-apply) —
because that covers the two real use cases ("run B only if A succeeded", "tell me
what happened") while keeping `run`/`args`/`cwd` eager, which preserves full
dry-run fidelity (every command line is known before apply). Interpolating a
hook's `stdout` into a later hook's argument is a genuinely larger capability
(arbitrary command-line construction from process output) and should be a
deliberate, disclosed choice, not a default.

## The readable-surface axis (surfaced, not pre-cut)

- **Broad:** any later Jinja surface that runs at/after hooks — including a later
  hook's `run`/`args`/`cwd` (Candidates 1–2). Maximally expressive; puts process
  output onto later command lines.
- **Narrow (this candidate):** only `when` gates and the after-apply message.
- **Broadest (rejected by all):** files/before-apply — impossible without
  reordering writes after hooks, which leaks output into files.

Choosing narrow *excludes* deferred arg/cwd interpolation that the broad shape
would allow. That exclusion is a capability decision and is disclosed to Bob as
a decision with a recommendation, never silently pre-cut.

## How it honors each hard constraint

Same composition guarantees as Candidates 1–2. The narrower surface makes the
"no stdout in files/public output" property even easier to hold and improves
dry-run fidelity.

## Sharpest tradeoff

Less expressive: a template that genuinely needs an earlier hook's output as a
later hook's argument (e.g. `docker tag {{ hooks.build.stdout }}`) cannot express
it in the narrow shape and must fall back to a `script:` that does the work. If
that use case is real, the broad shape (Candidate 1/2) is required.

## Synthesis decision

<!-- filled by the architect -->
