---
relationships:
  realizes: toha
  references: stage-admission-grounding
---

<!-- rumdl-disable MD013 -->

# Rationale

## Problem

Stage must know whether any supported Jinja source can observe five gated environment values before interview progress. Current reference lists miss `debug()`, self-shadowing reads, and include closures, while source-tree Jinja is compiled only during planning. A mutable source can also change after admission. The target carrier has one producer factory, and staged replay has no permission or ambient-read path.

## Usage (caller's view)

A command calls `AdmittedTemplate::load(folder)`, asks its `environment_needs()`, makes its own access decision, supplies a typed `InvocationContext`, and starts `Interview::start(&package, Seed { ... })`. Stage calls `stage_access(stage_trust, needs)` before seed construction and receives `StagingError::StageEnvironmentTrustRequired` when needs are nonempty and the flag is absent. Continue calls `Store::load`, replays submissions against the reloaded matching package, and builds a plan without host or environment reads. Direct crate callers supply the same domain facts without CLI or registry dependencies. The complete call sites and signatures are in `design.md`.

## Shape

`AdmittedTemplate` owns the analyzed compiled render program, transitive literal include closure, environment-needs mask, and stable content identity. All later renders use that program; replay requires the same identity. This makes the admission fact and rendered bytes one invariant, per encode-lessons-in-structure. Its small interface hides source walking, Jinja analysis, collision checks, and render selection, giving callers one deep seam. The CLI retains trust choice and capture; the pure engine retains only typed facts, per boundary-discipline. The staged wire stores one optional string for each referenced name and the fixed-order mask; the domain context keeps the opaque factory-created `CanonicalTarget` separate from wire data. The single private Jinja projection serves every surface.

## Synthesis decision

This capsule requests one candidate, not an arena synthesis. The chosen whole shape is the admitted package from `direction.md`. It keeps compiled sources rather than asking callers to coordinate a scan, digest check, and later compile. The package is viable because planning can consume its owned program, and replay can reject changed content before evaluation. No other runner candidate was supplied or incorporated.

## Tradeoffs accepted

- We accept a full source-tree walk and earlier source syntax errors at template load in exchange for complete pre-interview admission.
- We accept retaining compiled source bodies through the invocation in exchange for rendering exactly the analyzed program.
- We accept a replay failure when staged source identity changes in exchange for no replay-time environment access decision.
- We accept plaintext storage of only referenced present values, plus explicit nulls for referenced absent values, in exchange for deterministic replay. These bytes remain in existing staged JSON until successful apply, abort, or operator removal; storage of them is the decision requiring explicit approval.
- We accept legacy-record refusal in exchange for never inventing historical host, execution, or environment facts.
- We accept the approved hook digest's limited scope: it does not attest all Jinja content, even when it grants direct/new-apply access under option 1A.

## Alternatives considered

- Analyze at load but recompile source-tree files at plan time: a smaller internal cache, but callers need a second identity/access coordination seam, and mutable sources can make admission unsound.
- Capture all five optional values after any admitted reference: simpler wire encoding, but it stores unobservable plaintext and makes the interface's least-data promise false.
- Persist the whole admitted program in the staged record: replay would not need source identity checks, but wire would expose compiled/source representation and keep more plaintext bytes. Content identity plus needed values keeps storage private and smaller.

## Open questions and risks

- Does Bob approve persisting the referenced optional plaintext strings in the existing staged record, with its current directory and umask behavior, until apply, abort, or operator removal?
- Does the MiniJinja AST interface expose enough callable and alias information for a sound conservative `debug` analysis without rejecting supported syntax? A failing alias fixture must stop implementation and trigger a new sketch.
- Can every planning render consume the owned compiled source program without reopening a source path? Any remaining reopen would break the admission invariant.

## Next implementation step

Build `AdmittedTemplate::load` and its analyzed compiled-source owner, then prove the needs and content-identity tests before threading the context through the engine.
