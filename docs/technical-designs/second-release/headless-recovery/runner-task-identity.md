# Candidate task: identity-bearing answers and staged recovery

Design Toha's complete answers-file identity and incomplete staged-recovery
contract. Every external answers file must contain the formal identifier of the
template it answers. This requirement is decided. Reassess the original
recovery questions under that rule:

1. Does `apply PATH --answers FILE` safely resume and apply an incomplete staged
   interview?
2. Should exit-4 question output become a concise summary on a terminal while
   pipes, redirects, and `stage --async` retain JSON?
3. Should `apply TEMPLATE PATH --answers FILE` resume when its template matches
   staged state?

The design covers one exact external envelope; formal identity representation
and comparison; missing, malformed, unresolved, and mismatched identity;
validation and input-read ordering; atomic no-mutation behavior; migration from
bare maps; and new apply, staged apply, target-only apply, `continue`, terminal,
scripted, direct, staged replay, and public crate routes.

Preserve the pure engine's in-memory answer transaction, the sole canonical
target factory, the origin-bearing configured `Resolution`, accepted flow
meanings, and existing error distinctions. Do not introduce a second identity
normalizer, target normalizer, origin source, permission/access rule, timeout,
pinned-version check, or application subprocess. Do not bind answer files to a
target or commit unless the design separately proves that this follows from the
required template identifier rather than adding another restriction.

Write caller usage first with at least three concrete examples: a matching
document, a cross-template document whose question ids happen to match, and a
missing-identity document. Then provide:

- the exact JSON shape and public/private data types;
- function signatures and ownership boundaries;
- a module/seam diagram;
- route and validation-order matrices;
- errors, exit codes, persistence, flow, planning, and target effects;
- migration and compatibility behavior;
- the reassessment of each original question;
- falsifiable behaviors, failure injection, and sole-kill guards;
- alternatives, tradeoffs, and design-red-flag screening.

Produce design and rationale only. Do not modify runtime, specifications,
schemas, guides, tests, or task records.

