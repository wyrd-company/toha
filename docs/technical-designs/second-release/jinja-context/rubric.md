---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Jinja context design rubric

Score each criterion from 1 to 5. A score of 3 is viable but leaves material
design work for synthesis. A score of 5 is complete, internally consistent,
and directly implementable. Any candidate that silently narrows a supported
path, bypasses the approved trust policy, or creates a second canonical target
authority is not viable regardless of its total.

## Exact public contract — 25%

The candidate chooses variables or functions and defines every reserved name,
Jinja type, source, fallback, missing value, malformed host-data behavior, and
collision surface. The caller examples and type sketch agree. Terminal,
headless, staged, resumed, and direct callers have exact admin and interactive
semantics. Config presets remain a separate concept.

## One explicit value flow — 25%

One immutable domain value carries the context from boundary capture through
interview evaluation, staged replay, completion, planning, and every render
surface. The pure engine performs no ambient reads. The design traces canonical
target basename and selected template identity from their existing authorities
without a second normalizer or resolver.

## Trust and replay safety — 20%

Only the five approved environment facts can cross the access gate. The design
states exactly how `--trust` and live reviewed-registry trust combine, how each
command obtains that decision, what untrusted Jinja sees, what a stage stores,
and what resume does when current access differs. No broader environment,
permission, timeout, subprocess, or unsupported restriction is introduced.

## Module depth and locality — 15%

The public interface is small relative to the policy it hides. Types encode
meaningful invariants, validation occurs at the owning boundary, and Jinja name
projection and collision knowledge each have one owner. There are no shallow
pass-through modules, temporal pipelines, duplicated maps, or hidden state.

## Integration and proof — 15%

The proposed changes fit the current modules and named predecessor contracts,
define attributable errors and compatibility behavior, identify canonical
document changes without editing them, and give falsifiable tests across every
required evaluation surface. Risks and approval decisions are explicit enough
for implementation to start after Bob's Phase C answer.
