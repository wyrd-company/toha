---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Invocation session rationale

## Problem

The current pure interview engine keeps `Seed` during progression, but
`Completed` retains only answers and time; planning independently rebuilds the
Jinja map. Selected identity, canonical target, host facts, execution status,
and gated environment values must stay consistent through both phases and staged
replay. The existing direct crate interface must still work.

## Usage (caller's view)

A terminal command calls `canonical_target`, resolves and loads a template,
captures domain facts, then calls `Run::start`, `status`, `submit`, and `plan`.
A headless command uses the same calls with `interactive: false` and saves
`run.snapshot()` for `Run::replay`. A direct crate caller supplies its own facts
and uses the same run interface without process access. The exact call sites are
in `design.md` before its type sketch.

## Shape

`Run` owns the template, immutable invocation facts, current pure interview
state, and submission log. It alone makes a plan, so callers cannot mismatch a
completed interview with a different template or target. One private context
builder serves the pure engine and plan builder. `Seed` and `Completed` do not
gain context fields. Domain facts cross the seam after adapters parse process
and registry input once, per boundary discipline. A versioned snapshot freezes
all Jinja-visible values before staging, per single-source-of-truth and
idempotent replay. The interface has five operations; it hides context assembly,
replay validation, and plan wiring, so its depth is greater than the legacy pair
of public orchestration calls.

## Synthesis decision

This is an isolated candidate. No other candidate was read or synthesized. The
deliberate whole shape is an owning run/session module, with the existing
interview engine kept pure inside it.

## Tradeoffs accepted

- We accept a new primary crate workflow in exchange for keeping `Seed` and
  `Completed` free of invocation facts.
- We accept a temporary legacy direct path in exchange for source compatibility
  for existing crate callers.
- We accept recording environment values in a staged snapshot, when granted, in
  exchange for deterministic replay.
- We accept explicit domain host facts from direct callers in exchange for a
  pure library that does not inspect the process.

## Alternatives considered

Putting facts on `Seed` and carrying them into `Completed` gives `Plan::build`
an easy input, but spreads lifecycle ownership across public types and lets
callers pair a completion with the wrong template or target. Adding global
MiniJinja functions hides process reads and makes replay depend on current
state. Adding facts only to `Plan::build` leaves interview expressions without
the values. The owning run module is deeper because callers learn one lifecycle
interface rather than coordinate these seams.

## Open questions and risks

- Will Bob approve the recommended explicit per-run grant for the five
  environment values, separate from hook trust?
- Does the existing staged record migration need a stronger user-visible warning
  when a stored run resumes under the version-zero compatibility path?
- Can the selected registry source and full alias set be captured at resolution
  without broadening the resolver's public surface beyond domain values?

## Next implementation step

Build the private context builder and `Run` state around the existing interview
engine, then route plan construction through the same context view.
