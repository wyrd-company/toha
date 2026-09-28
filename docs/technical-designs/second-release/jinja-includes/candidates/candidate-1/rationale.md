# Rationale — Jinja includes as a confined, load-time include closure

## Problem

Toha renders every string as a lone MiniJinja template named `"value"` in a
fresh environment with **no loader** (`src/jinja.rs`). So `{% include %}` fails
today with `TemplateNotFound`, and turning it on means giving the environment a
way to map an include name to source text — the moment confinement matters. The
inherited constraints make the shape non-obvious: included content must stay
inside the *selected template root*; absolute paths, `..`, and symlink escapes
(including a symlink **inside** the root that resolves **outside** it) must be
rejected; MiniJinja's `path_loader`/`safe_join` does **not** canonicalize, so it
cannot stop that symlink vector on its own (grounding). The YAML `!include`
feature (`template.rs::resolve`) must stay a separate load-time data feature with
its own diagnostics. And two existing facts push on the design: MiniJinja does
**not** resolve include names relative to the includer (it looks them up
verbatim), and its `undeclared_variables` does **not** follow includes, so a
variable reached only through a partial is invisible to today's per-template
reference check and a cycle degrades to a generic "recursion limit exceeded".

## Usage (caller's view)

Authors write `{% include "partials/frontmatter.md" %}` in a file body. The name
is a template-root-relative, literal path; partials placed outside the source
subdirectory are pulled in but never emitted as their own files. Includes work in
source-tree files and `files:` rule sources only. Missing, escaping, symlinked,
cyclic, or dynamic includes fail with named, distinct diagnostics; a `files:`
source that reaches an undefined variable through a partial fails at load. Full
quickstart and three call sites (shared front-matter, a `files:` source with a
license header, and a nested partial chain) are in `design.md` §1 — written
before the types and reconciled to them.

## Shape

Data structures first. Two new types carry the design: `IncludeRoot` (a cheap,
cloneable, canonical-root handle — the confinement identity, *derived* from
`Template::root`, not a second copy) and `IncludeClosure` (the transitive set of
confined partial texts plus the unioned reference set). The dominant access
pattern — "render a file body that may pull in partials" — is served by building
the closure once at compile time and registering every partial as a named
in-memory template; render then walks the registered set with **no loader**. No
"we'll add a cache/loader later" escape hatch is needed: the closure is the
structure, resolved eagerly.

Load-bearing decisions:

- **No render-time loader at all.** Because includes are literal-only, the whole
  graph is knowable at compile time, so partials are pre-registered and MiniJinja
  performs zero filesystem access during render. The single thing that could ever
  be misconfigured into an arbitrary read — a loader pointed at a directory —
  simply does not exist. Confinement is enforced in one function,
  `IncludeRoot::locate`, which every partial passes through. This is the deepest
  interface point: a large capability (safe transitive text composition) behind a
  tiny surface (`compile_in_root` + an opaque root handle). *(per boundary-discipline,
  encode-lessons-in-structure — the "no loader" invariant is structural, not a runtime check.)*
- **Resolve relative to a fixed root, literal targets only.** MiniJinja looks
  names up verbatim, so root-relative names need no rewriting: the author's string
  *is* the registered key. Literal-only makes the closure complete and sound,
  which in turn buys precise cycle/missing diagnostics and full reference
  validation for free. *(per minimize-reader-load: one resolution rule, no
  per-includer rewriting to trace.)*
- **Confine to `template.root`, not `source_dir`.** The widest boundary
  consistent with `!include`/`confined_file` (both use the root), and it lets
  partials live outside the emitted source tree so the "partials must not be
  emitted" problem dissolves without new config. `ignore`/`static` are untouched;
  they still govern emission only.
- **Validate by walking the graph at compile time.** `referenced_ids` becomes the
  union over the body and all partials, so `Builder::refs` now catches variables
  reached only through an include in `files:` sources; cycles and missing/escape
  are reported when the including file is compiled (load for `files:` sources,
  plan for source-tree files) with the offending file named. *(single source of
  truth per invariant: one closure computes both the render set and the reference set.)*
- **One coherent surface rule, structurally enforced.** Includes exist exactly
  where `compile_in_root` is called — the two file-body surfaces. Every other
  surface keeps `Tmpl::compile`/`Expr::compile` with no partials and no loader, so
  includes are impossible there by construction, not by a guard that could be
  forgotten.

Validation lives at the boundary (`locate` + the closure walk); inside, the
render trusts an in-memory, confined set. The system deliberately does **not**
support dynamic include targets, `import`, `extends`, or includes on non-file
surfaces.

## Synthesis decision

*(left for the orchestrator)*

## Tradeoffs accepted

- We accept **no dynamic include targets** in exchange for a complete,
  statically-verifiable closure — sound confinement, precise cycle/missing
  diagnostics, and full load-time reference validation. Since includes are a new
  feature (off today), this is an initial-scope choice, not a narrowing of an
  existing capability; flagged for the human anyway.
- We accept a **dependency on MiniJinja's `unstable_machinery` parser** (no
  semver guarantee) in exchange for reusing the renderer's own AST — the walk sees
  exactly what render sees, with no hand-rolled Jinja tokenizer to drift.
- We accept **eager reading of every partial at compile time** (even branches a
  given render would not reach) in exchange for zero render-time filesystem access
  and the "no loader" security posture. Partial sets are small; the cost is
  negligible.
- We accept a **second compile constructor** (`compile_in_root` beside `compile`)
  rather than one polymorphic entry, so the surfaces that must *not* gain includes
  cannot accidentally do so — the type of call site encodes the policy.
- We accept possible **over-reporting** of a reference that a partial expects the
  includer to provide via `{% set %}`; documented, and avoidable by declaring the
  variable or setting it inside the partial.

## Alternatives considered

- **Lazy confining loader** (`env.set_loader` wrapping canonicalize + confine +
  symlink refusal). Simplest to write and supports dynamic targets, but it leaves
  a live filesystem seam on every render environment, gives only the generic
  "recursion limit exceeded" for cycles, and cannot validate include-reached
  variables at load. It exposes *more* to the caller (a render-time failure mode)
  while hiding *less*. Lost on interface depth and diagnostics.
- **Resolve relative to the including file.** More intuitive for deep partial
  trees, but MiniJinja does not do it, so it requires rewriting include names
  against each includer's directory — extra machinery and a resolution rule the
  reader must carry. Rejected for a single fixed-root rule; revisitable later
  without breaking literal-root names.
- **Confine to `source_dir`.** Forces partials into the emitted tree, so every
  shared partial needs an `ignore` glob to avoid becoming an output file. More
  ceremony for the author, no security gain. Rejected.
- **Hand-rolled include scanner** instead of the vendor parser. Avoids the
  unstable-feature dependency but reintroduces a fragile mini-parser (string
  literals, comments, candidate lists) that can disagree with the renderer.
  Rejected as a false economy.

## Open questions and risks

- Is dropping **dynamic include targets** acceptable for the first release, given
  it buys soundness and diagnostics? If a real template needs `{% include var %}`,
  we would add the lazy loader as a narrow, separately-approved fallback.
- Is depending on MiniJinja's **`unstable_machinery`** feature acceptable, and
  should we pin `minijinja` exactly to contain the no-semver-guarantee risk?
- Should **apply messages** (operator-facing text) also gain includes? They are
  file-ish; excluded here for a clean "file bodies only" rule, but the cost of
  adding them later is one more `compile_in_root` call site.
- Should an in-root symlink that resolves **inside** the root be allowed (as
  `confined_file` does) rather than refused outright? This design refuses all
  symlinks to match the stricter source-walk policy; confirm that is the intended
  standing rule.

## Next implementation step

Implement `IncludeRoot::locate` and its confinement tests first (all three escape
vectors), since every other piece depends on that single seam being correct.
