---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Rationale for Toha-owned Jinja context

## Problem

Interview and planning rebuild Jinja context independently, while staged replay
preserves only `now`, identity, target, and answers. Host and access-gated
environment facts must enter before the first interview render, remain stable
through replay, and reach planning without process reads in the pure engine.
Selected formal name and canonical target already have owners.

## Usage (caller's view)

The [design](design.md) starts with three call sites: terminal run captures one
`InvocationContext` for `Seed`; async stage saves that context and replays it; a
crate caller supplies equivalent typed values. `Plan::build` consumes the
context retained in `Completed`.

## Shape

One immutable invocation snapshot sits in `Seed`, moves through `Pending` and
`Completed`, and is projected by one Jinja module for every render. Types encode
denied versus granted environment access and optional host fields. Command
adapters capture observations and apply the permission decision; the engine
accepts domain values. This follows boundary discipline and gives one source of
truth for Jinja names. The external interface is deep: one context input hides
collision policy, readiness, null semantics, replay stability, and every
rendering site. Callers still own host observations, the existing canonical
target, and the selected formal name.

## Synthesis decision

This is an isolated arena candidate, so no cross-candidate synthesis has
occurred. The chosen base is the immutable snapshot shape; synthesis belongs to
the arena orchestrator.

## Tradeoffs accepted

- We accept a larger staged record in exchange for deterministic replay and no
  ambient reads.
- We accept explicit context construction by crate callers in exchange for a
  pure, predictable engine.
- We accept null for unavailable host and environment strings in exchange for
  stable Jinja types and no host-metadata render failures.
- We accept a legacy-record compatibility error for new-name references in
  exchange for avoiding invented historical host facts.

## Alternatives considered

- MiniJinja globals for each value hide little and expose process state at
  evaluation time; callers still need separate readiness and replay rules.
- A context builder called independently by interview and plan has a small
  method but leaks capture timing and identity policy into both callers.
- A flat map in `Seed` looks flexible but exposes spelling, types, and access
  rules to every caller; typed facts give a deeper interface.

## Open questions and risks

- Will Bob approve the recommended access rule that grants the five environment
  values under either `--trust` or approved live registry trust?
- Does the chosen OS-release parser need a stricter compatibility contract for
  duplicate or malformed assignments?
- Should a legacy staged record whose template does not use new names continue
  replay under a legacy context, or should all legacy records require restaging?

## Next implementation step

Add the domain snapshot and one Jinja projection, then thread it through `Seed`,
completion, staging, and planning before adding command capture.
