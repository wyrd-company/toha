---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

<!-- rumdl-disable MD013 -->

# Jinja context rationale

## Problem

Interview and plan rendering rebuild context independently from answers, template data, and `now`. The new selected identity, canonical target, host, execution, and restricted environment values must be available before any question and remain stable through staged replay. The pure engine cannot infer these values from process or registry state.

## Usage (caller's view)

The command canonicalizes the target with the existing `canonical_target`, resolves the selected template, captures one `InvocationContext`, and places it in `Seed` before `Interview::start`. Headless staging saves that snapshot before accepting answers. A direct crate caller supplies the same domain value explicitly; `Plan::build` receives the completed interview and uses its retained snapshot. The concrete terminal, staged, and crate calls are at the start of [design.md](design.md).

## Shape

`InvocationContext` is the one immutable input carried from `Seed` through `Pending` to `Completed` and staged storage. Its private projection adds fixed typed `toha_` variables to the existing Jinja map. The command adapter reads host and the five permitted environment values only after an explicit access decision; the engine reads no ambient state. This keeps validation at the input seam and the interview pure, per boundary discipline. The interface is deep: one value and two existing operations cover all interview and plan render sites. The selected formal name and canonical target come from their existing owners, and the context module owns the only reserved-name set and projection, per single-source-of-truth and locality.

## Synthesis decision

This is an isolated candidate. No other candidate was consulted or synthesized. The proposed base is the immutable invocation snapshot because it gives replay and direct callers the same interface.

## Tradeoffs accepted

- We accept a staged record containing captured environment strings when access was granted in exchange for deterministic replay; the existing stage storage model must protect that record accordingly.
- We accept a fixed `false` interactive value for a headless stage later continued at a terminal in exchange for stable interview branches.
- We accept an explicit direct-caller context argument in exchange for keeping the library independent of the command environment.
- We accept a new CLI grant option in exchange for keeping environment access separate from hook execution approval.

## Alternatives considered

- MiniJinja global functions would be easy to add but would hide absence and permission semantics and force interview readiness and plan rendering to coordinate with process state. That is a shallow interface for callers.
- Reconstructing context at each render site would avoid carrying a seed field but would expose timing and source choices to every caller and break replay locality.
- A nested Jinja object would reduce top-level names but would require a new object shape and nested collision/reference rules. Flat typed variables fit the present Jinja identifier model and keep each value explicit.

## Open questions and risks

- Will Bob approve option A, the separate explicit environment grant, and its CLI spelling before implementation?
- Does the existing stage storage location adequately protect captured environment values, or should granted staging be declined until storage policy is established?
- Is a nullable `toha_target_name` for a filesystem root or non-Unicode basename the expected Jinja contract?

## Next implementation step

Add the domain context types and one Jinja projection, then thread the value through seed, completion, and staged replay before changing command adapters.
