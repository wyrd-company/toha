# Bundled offline toha-demo — arena rubric

Six concrete, gradeable criteria derived from the task's observable outcome and
the feature invariants. The picker and the cross-judge score every candidate on
these. Candidates receive the task and grounding, not this rubric.

## C1 — Offline first-run by name

From a clean environment — no `templates.yml` in any layer, empty cache, no
network, arbitrary current directory — `toha apply toha-demo <dir>`,
`toha apply --dry-run toha-demo <dir>` (the preview path), and
`toha stage toha-demo <dir>` resolve and run using only the binary. Score: does
resolution reach the demo with zero required disk state, zero cache, and zero
git?

## C2 — Collision-preserving resolution

An installed or aliased `toha-demo` (user or system registry entry, a discovered
folder, or a user alias) keeps its exact current meaning; the bundled demo never
shadows it. The precedence rule between the bundled demo and every registry layer
is explicit and decided. Score: does the design preserve every name-resolution
outcome that exists today, and is the "which wins" rule stated, not implied?

## C3 — Resume and identity integrity

A staged bundled-demo interview (`stage --async` then `continue`, or
`stage` then `apply <PATH>`) resumes in a later process, offline, and produces
identical files. `resume_template` reconstructs the template root from the staged
identity `(formal_name, commit)` alone. The selected-template identity
(`formal_name`, `commit`, root) is stable across runs of one build and is
specified for the downstream defaults (1073) and Jinja-context (1075) designs.
Score: is identity reconstructable offline and stable, and is the shared
coordination surface pinned?

## C4 — Interface depth and blast radius

The fact that a template is embedded is hidden behind the smallest possible
surface. The pure interview engine, `Template`, `Plan`, `apply`, `protocol`, and
`staging` stay unchanged or nearly so. Score: how few modules learn the word
"embedded", how small is the added public interface, and is complexity
concentrated behind one deep seam rather than leaked across the pipeline?

## C5 — Single maintained source with an enforcing check

The embedded content derives from exactly one source (`docs/examples/demo/`), so
the embedded copy, the documented example, tests, and docs cannot drift. A
falsifiable check proves the embedded copy equals the source and that the demo
applies to its expected output tree. Score: is there one source, and a test that
fails when the copy or the applied output drifts?

## C6 — Trust and safety boundary preserved

The bundled demo obeys the documented trust rules — its hooks (it ships none)
would not run without `--trust`, and it is treated as untrusted like a
folder/discovered template. No new permission, timeout, pinned-version-check, or
application-subprocess capability is introduced. Path-safety (no write outside
the target, none into `.git`) and no-overwrite-without-`--force` are intact.
Score: does the design stay inside the existing capability envelope, needing no
separate capability approval?
