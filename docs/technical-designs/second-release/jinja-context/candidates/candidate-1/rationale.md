---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Invocation context rationale

## Problem

Template identity, canonical target, host facts, execution status, and five
access-gated environment values must reach both interview and planning. The
current seed carries time and defaults, while the planner rebuilds its own
Jinja map. Staging replays an interview from saved inputs, so live process
reads inside evaluation would change results on resume.

## Usage (caller's view)

The [design](design.md) starts with three call sites: terminal run captures a
single `InvocationContext` before `Interview::start`; async stage saves that
snapshot and resume checks its grant before replay; a direct crate caller
supplies explicit host and execution facts. Each completes with the existing
`Plan::build(&template, &completed, &target)` call. Template authors use flat
variables such as `{{ toha_target_name }}` and
`{% if toha_interactive %}`; optional values can be tested with `is none`.

## Shape

`InvocationContext` is an immutable domain snapshot owned by `Seed`, moved to
`Completed`, and serialized with a staged interview. It holds a target name,
selected identity, host facts, two execution booleans, and a denied or granted
five-value environment record. The command adapter captures process facts and
derives access from existing trust policy; evaluation only projects the
snapshot into one shared Jinja map. The public interface hides null semantics,
aliases, projection, and replay policy while exposing only facts the caller
alone knows. This is a deep module per codebase-design. Boundary validation
occurs once; the pure interview trusts the type. A denied variant makes the
absence of values structural. One builder is the single source of truth for
Jinja names and types. The same snapshot makes replay deterministic and keeps
the call chain short.

## Synthesis decision

This is one isolated candidate for later arena synthesis. It has no access to
other candidates and makes no claim about a final synthesis decision.

## Tradeoffs accepted

- We accept a required new crate input in exchange for explicit semantics
  across command and direct callers.
- We accept a versioned staged-record incompatibility in exchange for replay
  that never invents missing host or permission facts.
- We accept flat reserved variables in exchange for simple Jinja expressions
  and one stable type per value.
- We accept saved access values in exchange for deterministic replay, with a
  current-grant check before any saved value is used.

## Alternatives considered

- MiniJinja globals reading process state have a small apparent interface but
  expose time and permission behavior to every caller and break replay.
- A command-only map passed separately into interview and plan has a broad
  coordination interface and duplicates projection policy. Direct crate
  callers would have to implement it themselves.
- A nested `toha` object reduces reserved names but makes one large object's
  shape the authoring interface and complicates existing root identifier
  readiness checks. Flat typed variables keep each supported fact explicit.
- Functions such as `toha_host_os()` could preserve call syntax with `now()`
  but conceal absence and imply values might change during one interview.

## Open questions and risks

- Will Bob approve the recommended grant rule that treats a live approved
  named template like `--trust` for the five environment values?
- Does a supported direct caller need a migration constructor, or is the
  required explicit snapshot appropriate for the 0.2.0 crate change?
- Can the host adapter report runtime OS and architecture on every supported
  platform without substituting compile-target constants?

## Next implementation step

After the access decision, add the snapshot type and one Jinja projection,
then carry it through `Seed`, `Completed`, and staged replay before wiring
command adapters.
