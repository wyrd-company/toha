# Arena rubric — Jinja includes within the template root (task 1076)

Phase B framing. Derived from the task's observable outcomes and invariants.
The picker and cross-judge use this; **candidates do not see it** — they receive
only the common task and grounding. Each criterion is concrete and gradeable.

## Artifact each candidate produces

A design package in its isolated directory: `design.md` (caller usage first,
then type/data sketch, function signatures, module/seam map, error contract)
plus `rationale.md` shaped per `rationale-template.md`. Pseudocode and
`not implemented` sketches only — no runtime stubs, no production edits.

## Criteria

1. **Confinement correctness (weight: highest).** Every escape vector is
   rejected with the confinement mirrored on the codebase precedent
   (canonicalize + `starts_with(root)` + refuse symlink): absolute include
   paths, `..` parent traversal, and — the vector `safe_join`/`path_loader`
   miss — a symlink **inside** the root that resolves outside it. Grade up for
   a design whose enforcement point cannot be bypassed by any render surface
   that gains includes. Falsifiable: named negative fixtures for absolute, `..`,
   and symlink-escape each fail with a Jinja-include diagnostic **distinct** from
   the `!include` wording.

2. **Render-surface scope (weight: high).** States exactly which surfaces gain
   includes (source-tree files, `files:` rule sources, apply messages,
   interview fields, path segments, hook fields) and why, with one coherent
   rule rather than a per-site patchwork. Unsafe/meaningless surfaces (path
   segments) are excluded deliberately. Grade up for a single enforcement seam
   shared by all included-enabled surfaces.

3. **Include semantics (weight: high).** Resolves: the include **root**
   (`source_dir` vs whole template root) and its interaction with
   `ignore`/`static` and with partials that should not be emitted;
   relative-to-includer vs root-relative name resolution; nested includes. The
   choice is justified against the caller's mental model and documented.

4. **Diagnostics quality (weight: medium).** Missing include, and
   recursion/cycle, produce clear, actionable messages. Addresses that
   MiniJinja degrades a cycle to a generic "recursion limit exceeded" after ~50
   levels — either accepts that with rationale or provides precise cycle
   diagnostics via static graph walking. Errors carry the offending file/name.

5. **Interface depth & integration (weight: high).** The loader threads into
   the existing `jinja::environment()`/`Tmpl` path with a small public surface
   that hides the confinement complexity; the template root is a single source
   of truth, not scattered across call sites; no transport/wire types leak onto
   the public surface. Deep module, not a shallow pass-through. Testable through
   the same interface callers use. Handles the load-time reference-validation
   gap (variables reached only through includes) coherently.

6. **Compatibility & separation (weight: medium).** Preserves YAML `!include`
   as a distinct feature (no shared diagnostics, no behavior change), `static`
   copy semantics, `template.yml`-never-rendered, pure interview logic, and all
   driver paths. Describes canonical spec/schema/guide edits as *proposed* (owned
   by implementation 1061) without making them. Introduces no new
   timeout/permission/config knob — or flags any such as requiring separate
   approval.

## Scoring

Each criterion scored 1–5 by the parent (reading every candidate end to end)
and by the readonly cross-judge (different model family). Disagreements
reconciled explicitly in the synthesis record. Base picked on which design a
maintainer can extend without breaking the confinement invariant; ties broken
toward the smaller public surface.

## Runners

Foreground `Agent` calls (`run_in_background: false`), isolated output dirs
`/tmp/arena-jinja-includes/candidate-<n>/`; no shared writable paths. Parent
collects every result before composing any final response (SDK background
children are killed on parent end). Selected:

- candidate-1: `claude` (inherit-parent, opus/high)
- candidate-2: `claude` (inherit-parent, opus/high)
- candidate-3: `ocx-gpt-5-6-sol` (different model family)
- candidate-4: `ocx-gpt-5-6-terra` (different model family)
- cross-judge: `ocx-gpt-5-6-luna` (readonly, different family from parent)

Dropouts recorded in the synthesis note; if fewer than two structurally
distinct viable candidates survive, another candidate direction is run.
