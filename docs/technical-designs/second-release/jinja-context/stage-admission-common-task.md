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

# Candidate task — stage environment admission

Design the changed ownership seam for Toha's Jinja invocation context. Produce
a candidate package with caller-first usage, concrete domain and wire types,
function signatures, module ownership, data flow, error attribution, migration
behavior, falsifiable tests, tradeoffs, and rationale.

Read `stage-admission-grounding.md` completely. Treat its fixed product
decisions as requirements. Do not inspect or infer any scoring rubric.

Your design must answer:

1. How does template load find every possible environment observation before
   `Seed` or `Interview::start`, including source-tree paths and bodies,
   literal include closures, aliases, self-shadowing reads, dynamic
   attribute/item expressions, and context enumeration through `debug()`?
2. What compiled-source or content-identity owner prevents a mutable template
   from changing the answer between admission and later planning?
3. What is the smallest deterministic stage wire representation? State exactly
   which optional plaintext values are stored, how referenced-but-absent differs
   from unreferenced or denied, and how long those bytes remain.
4. How does `Store::load` reconstruct a live context through the approved
   `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>` factory
   without deserializing or manufacturing `CanonicalTarget`?
5. How do direct/new apply, stage, continue, staged apply, headless, terminal,
   and direct crate callers use the same pure engine while preserving their
   distinct trust decisions?
6. What exact typed failure occurs when `stage` needs environment access but
   lacks `--trust`, and how is it proved to precede interview/state progress?
7. What happens to a legacy staged record that has no snapshot? Do not invent
   host or access facts and do not use exact-name scans to claim `debug()` is
   harmless.
8. How are admin, originating-interactive state, host types, selected identity,
   aliases, target basename, collisions, presets, and all render surfaces kept
   consistent with the broader invocation-context design?

Do not implement runtime behavior or edit shared specifications, schemas, or
guides. Do not add a timeout, application subprocess, generic environment map,
second canonical-target authority, reusable permission, replay-time access
check, `continue --trust`, or unsupported restriction on existing Jinja syntax.

Choose a coherent whole shape. A caller should cross one deep seam rather than
coordinate a temporal analysis pipeline. Explain which complexity the module
hides and why its interface is no larger than required.
