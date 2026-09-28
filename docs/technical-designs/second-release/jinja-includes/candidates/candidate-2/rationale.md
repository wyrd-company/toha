# Rationale — confined Jinja includes (candidate 2)

## Problem

Toha renders every template string as a lone MiniJinja template named `"value"`
in a fresh environment with no loader (`src/jinja.rs`), so `{% include %}`,
`import`, and `extends` all fail today. We want to turn `{% include %}` on for
template authors while honoring inherited, non-negotiable constraints: included
content must stay within the selected template root; absolute paths, `..`
escapes, and symlink escapes — including a symlink *inside* the root that
resolves *outside* it — must be refused; the YAML `!include` feature stays a
separate load-time data feature with its own diagnostics; and no new
timeout/permission/config capability is introduced. The shape is non-obvious for
three reasons the grounding surfaced: MiniJinja's `path_loader`/`safe_join` does
not canonicalize, so it does not stop a symlink escape; MiniJinja resolves
include names verbatim, not relative to the including file; and `Tmpl` is used on
~six render surfaces (file bodies, `files:` sources, path segments, interview
fields, apply messages, hook fields), only some of which should gain includes.
The single source of truth for confinement already exists: `Template::root`,
canonicalized by `Template::load`.

## Usage (caller's view)

A template author drops shared fragments in support material outside the source
subdirectory and pulls them in with a root-relative name:

```
my-template/
├── partials/header.md      # support material, never emitted
└── template/README.md      # {% include "partials/header.md" %}
```

```jinja
{# template/README.md #}
{% include "partials/header.md" %}
# {{ project | title }}
```

The partial sees the same variables as the file; nested includes name their
target by the same root-relative path; `partials/` is never emitted because it
lives outside the source subdirectory. In code, the only new call is inside
`plan.rs`'s file-body path: `Partials::rooted(&template.root)` once, then
`partials.compile(text)?.render(ctx)` per file body. Every other surface keeps
calling `Tmpl::compile`/`Expr::compile` exactly as today, so `{% include %}`
stays inert there. Full quickstart and three call sites are in `design.md §1`.

## Shape

Data structures first. Two new types in `src/jinja.rs`: `Partials` (a cloneable
value object wrapping the canonical `template.root` in an `Arc`) and `FileTmpl`
(a compiled body whose environment already has the confined loader attached).
`Tmpl` and `Expr` are untouched. The load-bearing decision is the **type split**:
includes are reachable if and only if you hold a `FileTmpl`, and only
`Partials::compile` mints one. That encodes "includes on file bodies only" in the
type system rather than a runtime flag (per `encode-lessons-in-structure`), which
is why the two file-body call sites in `plan.rs` are the *entire* enabled
surface and no per-site opt-out exists elsewhere.

Data flow: `Plan::build` derives `Partials` from `template.root` (single source
of truth; it defines no new identity — `boundary-discipline` and single-source
principles). `read_render` gains a `&Partials` and routes both the source-tree
walk and the `files:` rule source through `Partials::compile(...).render(...)`.
The confinement is one closure, `confined_loader(root)`, set on the env — the
single, un-bypassable seam every include (nested included) passes through. It
runs all three escape checks: reject absolute; refuse a symlink at any component
from root to leaf (mirroring `plan::has_symlink_component` and the source-walk
"source symlink not supported" policy); canonicalize and require
`starts_with(root)` as defense in depth. The symlink-component refusal is what
closes the vector MiniJinja's own `safe_join` misses.

Validation lives at the boundary (the loader) and returns crafted
`minijinja::Error`s with distinct wording — every message contains "partial" so
it never collides with `!include`'s strings. Missing partials return `Err`
(Toha wording) not `Ok(None)` (MiniJinja wording). Cycles are handled by
translating MiniJinja's recursion-limit error inside `FileTmpl::render`, rather
than building a static include-graph walker, because include names may be
computed and a static walk would be both incomplete and dependent on AST access
Toha does not take.

Interface depth: the public surface is three methods (`Partials::rooted`,
`Partials::compile`, `FileTmpl::{render, references, source}`). Behind it sit
canonicalization, three escape-vector checks, MiniJinja loader wiring, and
recursion→cycle translation. No MiniJinja loader type or wire type is exposed;
callers pass `&Path` and `Serialize` context in and get domain results out. The
whole feature is exercised through `Plan::build` and the existing fixture
harness. What the system deliberately does not do: it does not enable includes
on path segments, interview fields, messages, or hooks; it does not resolve
names relative to the includer; it does not walk the include graph at load; and
it adds no config/timeout/permission knob.

## Synthesis decision

*(left for the orchestrator)*

## Tradeoffs accepted

- We accept the generic-shaped recursion limit for cycles (translated to a
  clear partial-cycle message) in exchange for not carrying a static
  include-graph walker that dynamic include names would defeat anyway.
- We accept that a variable reached only through a partial surfaces at render,
  not load, in exchange for no include-graph walk at load — safe because the
  answer namespace is flat and global, so the failure is deterministic.
- We accept a second type (`FileTmpl`) alongside `Tmpl` in exchange for making
  "includes on file bodies only" impossible to violate by accident; a flag on
  `Tmpl` would be smaller code but a weaker invariant.
- We accept root-relative (not includer-relative) names in exchange for trivial,
  unambiguous nested includes and no AST rewriting.
- We accept partials-outside-source as the recommended layout in exchange for
  zero-ceremony non-emission (no `ignore` entries); a partial placed inside the
  source subdirectory still works but emits unless ignored.

## Alternatives considered

- **Confined lazy loader on a shared `Tmpl` with an `include: bool` flag.**
  Smaller surface, but the capability becomes a runtime argument any call site
  can pass, so the "file bodies only" rule is prose, not structure. Rejected on
  interface depth: it exposes the choice instead of hiding it.
- **Eager static include-graph resolution at load** (register every referenced
  partial as a named template, walk the graph for precise cycle diagnostics and
  cross-include reference validation, render with no loader at all). Attractive —
  it would give named-file cycle errors and load-time validation, mirroring the
  `!include` resolve() precedent, and would make render filesystem-free. Rejected
  because discovering include targets statically needs the parsed AST (not part
  of Toha's stable surface) and still cannot see computed include names, so it
  would be incomplete precisely where it claims precision. Its wins (precise
  cycles, load validation) are the two things this design consciously trades away
  with rationale.
- **Include root = `source_dir`.** Keeps partials near the files that use them,
  but forces every partial into `ignore` or it is emitted as output — a
  standing foot-gun. Rejected for the non-emission ergonomics of
  root-with-partials-outside-source.
- **Relative-to-includer names via AST rewriting.** More familiar to Jinja
  users, but requires rewriting every include target against the includer's
  directory and depends on AST access; nested includes get subtle. Rejected as
  disproportionate to the benefit.

## Open questions and risks

- Should a partial's references be validated at load (walking literal includes)
  so undefined-id errors surface before render, or is deterministic render-time
  failure against the flat global namespace acceptable? (This design chooses
  render-time.)
- Is the recursion-limit-translated cycle message sufficient, or do authors need
  the exact offending include edge named — accepting a literal-only static walk
  that ignores computed includes?
- Should `{% include %}` be extended to apply messages later? The rule "file
  bodies only" excludes them today; is that the durable line?
- Confirm the intended symlink policy for includes is *refuse* (matching the
  source walk) rather than *follow-if-inside-root* (matching `!include`); this
  design refuses, which is stricter than `!include`.
- Does MiniJinja 2.24 reliably surface a self/mutual include cycle as a
  recursion-kind error whose display contains "recursion"? The translation keys
  on that; a fixture must pin it.

## Next implementation step

Add `Partials` and `FileTmpl` with the `confined_loader` closure in
`src/jinja.rs` (loader wired, `not_implemented!()` bodies), then thread
`Partials::rooted(&template.root)` through `read_render` at the two `plan.rs`
file-body call sites and land the `include-basic` positive fixture red-to-green.
