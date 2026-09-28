---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Rationale

## Problem

Stage must decide environment admission before interview progress, while some Jinja sources are discovered only during planning and MiniJinja's undeclared-variable set misses `debug()`, self-shadowing reads, and includes. The same immutable invocation context must reach every render and staged replay. The canonical target factory, configured defaults, registry review, and hook trust already have separate owners.

## Usage (caller's view)

The command calls `Template::load`, then one `template.admit(AdmissionRequest::Stage { explicit_trust, facts }, reader)` before constructing `Seed`. A direct/new apply caller passes `AdmissionRequest::Direct { grant, facts }`, with `grant` computed from explicit trust or eligible live registry review. A crate caller uses `admit_supplied` with typed `TrustedEnvironment`. `continue` calls `Store::load` and replays the saved context without a trust option or ambient read. The examples and exact signatures are in [design.md](design.md).

## Shape

`Template` owns a load-time manifest, compiled render sources, and their content identity. It analyzes configuration Jinja, source-tree paths and bodies, and transitive literal body includes. Its conservative AST walk recognizes possible fixed-name reads and context enumeration through `debug()`. It refuses an untrusted stage before `Seed`; after admission it captures all five fixed optional values once. `InvocationContext` contains typed selected, target, host, execution, and environment facts. The staged wire stores these facts but reconstructs `CanonicalTarget` only through the producer factory. The engine projects one private Jinja context. This is a deep module: callers know one admission operation, while source traversal, reference analysis, include closure, identity, and capture ordering stay inside `Template`, per boundary-discipline and interface-depth. Planning renders the analyzed compiled sources; cross-process replay checks their identity first. One source identity owns the admission/render relation, per single-source-of-truth.

## Synthesis decision

This capsule produces one candidate, not an arena synthesis. The directed manifest/content-identity shape is the base. I compared fixed-five capture with referenced-only capture and whole-source persistence. The former has the shortest replay proof while the identity check prevents changed sources from exploiting the snapshot. No other candidate is claimed or silently merged.

## Tradeoffs accepted

- We accept up to five plaintext optional values in staged JSON after an admitted need in exchange for a fixed replay contract that covers `debug()` and aliases.
- We accept earlier source-tree compile errors in exchange for refusal before interview progress.
- We accept a changed-source replay refusal in exchange for using only bytes whose possible reads were admitted.
- We accept a source break in `Seed` in exchange for requiring every direct caller to supply explicit typed facts.
- We accept unconditional legacy-record refusal in exchange for avoiding invented host/access facts and unsound exact-name scans.
- We accept that matching hook review can grant environment access to changed non-hook Jinja content, because the approved review digest covers `HookSurface`, not all render sources.

## Alternatives considered

- Capture only referenced values: less plaintext, but aliases and `debug()` force callers or replay to reason about a variable-specific snapshot and its proof. The interface leaks analysis details.
- Store all analyzed source bytes in the staged record: replay can render without a source-identity check, but duplicates the template and expands the wire and lifecycle policy. It hides source mutation at greater storage cost.
- Scan at each planning step: exposes a temporal pipeline to callers and permits an interview to start before admission, so it cannot meet stage failure ordering.

## Open questions and risks

- Is plaintext persistence of up to five optional values in the existing staged JSON acceptable until apply, abort, or operator removal?
- Should the user-visible error for a changed source include only an attributed source location and restage guidance, with no digest or path bytes?
- Can the current MiniJinja parser expose every supported AST form needed for sound alias and `debug()` analysis, or must Toha retain its own parsed representation at load?

## Next implementation step

Add the `Template` load-time render-source manifest and AST analyzer, with fixtures that prove `StageTrustRequired` precedes seed and store activity.
