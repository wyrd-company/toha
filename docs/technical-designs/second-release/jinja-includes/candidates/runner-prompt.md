# Candidate design runner — Jinja includes within the Toha template root

You are ONE candidate in a parallel architect exploration. Produce the best
candidate design your model can make. Do NOT hedge toward a safe middle — the
differences between candidates are the signal the orchestrator uses to pick a
base and graft. This is a DESIGN task: sketches, signatures, pseudocode, and
`not implemented` bodies only. **Do not edit any production code, spec, or
schema. Do not run cargo/build.** You may READ the repository freely.

## Read first (required grounding — do not re-derive)

1. The architect runner discipline:
   `/workspaces/references/skills/pi-pstack/skills/architect/references/runner-prompt.md`
2. The rationale template you must follow:
   `/workspaces/references/skills/pi-pstack/skills/architect/references/rationale-template.md`
3. The Phase A grounding (authoritative; trust it, verify by reading code if you
   wish):
   `/workspaces/worktrees/toha/design-jinja-includes/docs/technical-designs/second-release/jinja-includes/01-grounding.md`
4. The repository (branch `epic/second-release`) at `/workspaces/tools/toha`.
   Key files: `src/jinja.rs`, `src/plan.rs`, `src/template.rs`,
   `docs/template-jinja.md`, `docs/specifications/template-format.yml`,
   `tests/fixtures/` (see `err-include-*`, `err-path-escape`, `basic-example`).

## The task

Toha renders template source files with MiniJinja (2.24). Today Jinja
`{% include %}`, `import`, and `extends` are disabled (each renderable string is
compiled as a lone template named `"value"` in a fresh environment with no
loader). Design turning **`{% include %}` on**, confined to the selected
template root, so a template author can factor shared partials out of a file and
pull them in at render time.

Hard requirements (inherited feature constraints — non-negotiable):

- Included content MUST remain within the selected template root. No arbitrary
  filesystem access through MiniJinja's loader.
- Reject absolute include paths, `..` parent traversal, and symlink escapes —
  including a symlink INSIDE the root that resolves OUTSIDE it (note from
  grounding: MiniJinja's `safe_join`/`path_loader` does NOT canonicalize and so
  does NOT stop this; you must add confinement mirroring the existing
  `!include` / `confined_file` precedent: canonicalize + `starts_with(root)` +
  refuse symlink).
- Preserve the YAML `!include` tag as a SEPARATE feature with its own
  diagnostics — do not merge or reword them.
- Do not grant a new timeout/permission/config capability. If your design needs
  one, flag it explicitly as requiring separate human approval.

Decisions your design MUST resolve (state and justify each):

1. **Supported render surfaces.** Which of these gain includes and why:
   source-tree file contents, `files:` rule sources, apply messages, interview
   fields (prompt/default/computed/when/format/options), target path segments,
   hook fields. Prefer ONE coherent rule and a single enforcement seam over a
   per-site patchwork. Justify every exclusion.
2. **Include root & lifetime/identity.** Is the confinement/resolution root the
   source subdirectory (`source_dir`) or the whole template root
   (`template.root`)? How does it interact with `ignore`/`static` and with
   partials that should NOT be emitted as their own output files? The root is
   the selected template root that resolution/1074 already establishes — you
   consume it, you do not define a new identity.
3. **Relative / local / nested include semantics.** Is `{% include "x" %}`
   resolved relative to the including file's directory or to a fixed root?
   (Grounding: MiniJinja does NOT resolve relative-to-includer by default — it
   looks the name up verbatim; relative semantics require you to rewrite names.)
   How do nested includes (a partial that includes another) work?
4. **Missing includes** diagnostics.
5. **Recursion / cycle diagnostics.** MiniJinja degrades a cycle to a generic
   "recursion limit exceeded" after ~50 levels (INCLUDE_RECURSION_COST=10,
   MAX_RECURSION=500). Either accept that with rationale or provide precise
   cycle diagnostics (e.g. static include-graph walking, like the `!include`
   stack). Errors must name the offending file.
6. **Load-time reference validation.** Today variables reached only through an
   include are not validated by `undeclared_variables` (per-template AST, does
   not follow includes). Decide whether includes are validated at load (walking
   the graph) or only surface at render, and why.

## Deliverables — write EXACTLY these two files, nothing else, to YOUR dir

Your isolated output directory is: `CANDIDATE_DIR` (given below). Write only:

- `CANDIDATE_DIR/design.md` — caller usage FIRST (a template author's view: a
  quickstart plus two or three concrete call sites showing a source file that
  includes a partial, the directory layout, and what the output becomes), THEN
  the data/type sketch, function signatures (Rust, with `not implemented`
  bodies where a body would go), the module/seam map showing how the loader
  threads into `jinja::environment()`/`Tmpl`/`plan.rs`, and the full error/results
  contract (every diagnostic string). Include the proposed (not applied) edits
  to `docs/template-jinja.md` and `docs/specifications/template-format.yml` and
  the fixtures you would add (positive + negative), described.
- `CANDIDATE_DIR/rationale.md` — shaped per `rationale-template.md`: Problem,
  Usage (caller's view), Shape, Tradeoffs accepted, Alternatives considered,
  Open questions and risks, Next implementation step. Leave "Synthesis
  decision" empty (the orchestrator fills it).

Discipline the orchestrator will grade you on: confinement correctness (all
three escape vectors, one un-bypassable seam), render-surface scope clarity,
include-semantics coherence, diagnostics quality, interface depth (small public
surface hiding the confinement complexity; template root as single source of
truth; no wire types leaked; testable through the caller's interface), and
compatibility/separation (preserve `!include`, `static`, pure interview logic,
all driver paths; spec/schema edits proposed not made). Write the caller usage
before the types and reconcile the types to the usage.

Your final message to the orchestrator: a 4–8 line summary of your design's
shape and its single most distinctive decision. The files are the deliverable.
