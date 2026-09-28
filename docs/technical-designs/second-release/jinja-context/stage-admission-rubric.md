---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
    - error-attribution
    - jinja-includes
---

# Stage environment admission rubric

Score each criterion from 1 to 5. A score of 3 is viable but leaves material
work for synthesis. A score of 5 is complete, internally consistent, and
directly implementable.

Any candidate is nonviable if it misses a supported render surface, uses
`undeclared_variables` as the sole admission oracle, asks for access after
interview progress, re-reads gated values during replay, deserializes or
manufactures `CanonicalTarget`, exposes an open-ended environment map, or
silently restricts supported Jinja syntax.

## Sound admission analysis — 25%

The design finds every potential environment observation before interview
start across configuration expressions/templates, source-tree paths and
bodies, literal include closures, aliases, self-shadowing reads, dynamic
expressions, and `debug()` context enumeration. It states what is conservative,
where source attribution survives, and how future generic lookup features must
extend the proof.

## Render-program ownership — 20%

One deep module owns analyzed sources, reference needs, and later rendering so
mutable bytes cannot invalidate admission. The interface hides compilation,
closure, and content-identity complexity without exposing a temporal pipeline
or duplicating the include design's loader/normalizer authority.

## Trust and lifecycle fidelity — 20%

The design implements approved option 1A for direct/new apply and explicit
`stage --trust` for stage. Missing stage trust fails before seed, render, or
write. Continue and staged apply replay the captured decision without new
access, while hook trust remains independent. Headless, terminal, resumed, and
direct callers share the pure engine.

## Minimal deterministic replay state — 20%

The proposed wire shape stores no more plaintext than its replay proof needs,
distinguishes absent, unreferenced, and denied states, explains byte lifetime,
and reconstructs the live context through the producer factory. It handles
legacy records without invented facts or an environment-access retry path.

## Integration, contract, and proof — 15%

Concrete types, signatures, errors, module changes, collision rules, host/admin/
interactive semantics, selected identity, target projection, preset separation,
and canonical-document impacts agree. Caller examples match the type sketch.
Tests are falsifiable and cover every supported surface, mutation, failure
ordering, serialization, and trust distinction. Risks and manager decisions
are explicit.
