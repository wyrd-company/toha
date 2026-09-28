# Rationale: Jinja `{% include %}` within the template root

## Problem

Toha template authors have no way to factor out repeated fragments of text files without duplicating code. YAML files must be fully self-contained or referenced via the YAML `!include` tag (which is a data-composition feature, not template composition). Enabling Jinja `{% include %}` lets authors split large template files, share common headers/footers, and build composable templates. The constraint is confinement: includes must not reach outside the template root, must not follow symlinks, and must stay silent about their escape avenues—matching the existing `!include` and `confined_file` patterns that already police filesystem boundaries.

## Usage (caller's view)

A template author enables includes by placing partial files (conventionally named `*.jinja`) in the source directory and referencing them from other source files:

```
template/
├── template.yml
├── template/
│   ├── header.txt
│   ├── main.py
│   └── helpers.py
```

In `main.py`:
```jinja
{% include "header.txt" %}

def greet():
    pass
```

In `header.txt`:
```jinja
# Generated on {{ now() | dateformat }}
# Author: {{ author }}
```

After rendering with `author="Alice"`, the output is:
```
# Generated on 2025-09-28
# Author: Alice

def greet():
    pass
```

Include names are paths relative to the including file's directory:
- `{% include "header.jinja" %}` resolves to `header.jinja` in the same directory
- `{% include "ui/form.jinja" %}` resolves to `ui/form.jinja` relative to the current file's directory
- `{% include "../shared.jinja" %}` resolves to a sibling directory; rejected if it escapes the source root

Variables defined in `template.yml` data, interview questions, and computed values are available in included files without modification to the include syntax. The rendering context is shared across the including file and all transitively included files.

## Shape

The design introduces three key types and integrates them into the existing template compilation and rendering pipeline:

**IncludeLoader** — A guard that resolves include names to file content. Given an include name and the directory of the including file, it:
1. Resolve the name as a relative path from the includer's directory
2. Canonicalize the result to eliminate symlinks
3. Verify the canonical path is within the source directory
4. Return the file text or an error with a specific diagnostic

**IncludeGraph** — Built at load time by traversing all source files and walking their includes, recording which files include which others. Used to detect cycles and ensure all includes exist before rendering starts.

**Tmpl + includes** — The `Tmpl` struct gains an optional `compile_with_includes()` constructor and the `include_paths()` method, allowing callers to opt into include support and inspect which files were included. When a loader is present, MiniJinja's environment is configured with a custom loader function that delegates to `IncludeLoader`.

Integration happens in `template.rs` at load time: after parsing `template.yml` and discovering the source directory, a single `IncludeLoader` is created for that root. The loader is passed to every `Tmpl::compile()` call that processes source-tree files and `files:` rule sources. Includes are NOT available in target path segments (would break confinement logic), expressions (no filename context), or hook commands (need explicit review).

Load-time validation detects:
- **Cycles** — via depth-first traversal with an explicit stack; reported as `"jinja include cycle: a.txt -> b.txt -> a.txt"`
- **Escapes** — via canonicalization and `starts_with()` check; reported as `"jinja include: include path escapes source directory"`
- **Symlinks** — via `is_symlink()` check; reported as `"jinja include: include target is a symlink"`
- **Missing files** — via existence check; reported as `"jinja include: include file not found"`

These diagnostics are distinct from YAML `!include` errors (`"include cycle"`, `"include escapes template root"`), preserving the separation required by the task.

Variable references inside includes are validated by Toha's existing `undeclared_variables()` analysis: each included file is compiled and its variable references are checked against the union of template data, interview ids, and computed ids—the same context available at render time.

## Synthesis decision

*Filled in by the orchestrator.*

## Tradeoffs accepted

- **Eager validation over lazy loading.** All includes are verified at load time, not when rendering. This adds startup latency proportional to the include graph size but catches missing files and cycles early, before the interview begins.
- **Static include names over dynamic paths.** Include names must be string literals in the source; `{% include variable_name %}` is not supported. This prevents path traversal and symlink-escape attacks at render time and keeps the confinement model static.
- **Relative-to-includer semantics over root-relative.** Includes are resolved relative to the including file's directory, not the source root. This is more intuitive for factoring (a file can include its siblings without knowing the full path) but requires name rewriting before each MiniJinja loader call.
- **Text composition, not scope sharing.** Included files are pure text substitution; macros defined in an include are visible only within that include. This avoids the complexity of propagating lexical scope across file boundaries and matches the principle that includes are templates, not modules.
- **Source directory as confinement root, not template root.** Includes are confined to the source directory (e.g., `template/`), not the whole template root. This prevents partials from being emitted as output files (which would require special logic to exclude them from `walk()`), and keeps support material (like data files, scripts) outside the include namespace.
- **No `ignore missing` support.** A missing include is always an error. Optional includes would require a static analysis phase to know whether an include is optional, which the design does not provide.

## Alternatives considered

### Alternative A: Root-relative includes (confinement at template root, not source dir)

Resolve all includes against the template root, allowing includes from outside the source directory. Partials that should not be emitted are added to the `static` or `ignore` globs.

**Why it lost:** Partials are not typically static files; they are Jinja templates that should not be emitted. Adding them to `ignore` is a runtime decision that happens during `walk()`, while `ignore` is conceptually about files that should not appear in the output. The design treats includes as a text composition feature of the source tree, not a general filesystem feature. If someone needs shared data files, they use YAML `!include` in `template.yml`, which is already available and distinct.

### Alternative B: Lazy validation (load includes only at render time)

Skip the load-time graph walk and rely on MiniJinja's loader to produce errors when an include is rendered and not found.

**Why it lost:** Late errors are harder to debug and degrade user experience. A template with a cycle would silently exceed MiniJinja's recursion limit (~50 deep), producing a generic `"recursion limit exceeded"` message instead of naming the chain: `"a.txt -> b.txt -> a.txt"`. Missing includes would surface as `TemplateNotFound` only when the code path that includes them is rendered, which could be during a user interview, not at template author time.

### Alternative C: Macros/functions from includes (shared scope)

Allow macros and functions defined in includes to be visible to the includer, replicating traditional templating language semantics.

**Why it lost:** Requires reimplementing MiniJinja's scope and symbol table. The current design treats includes as text replacement: the includer just gets the rendered output of the include. Macro sharing would require a module system (tracking which file defines which macro, enforcing the def-before-use invariant across files, handling name collisions). The simpler alternative is for users to define all macros inline or copy them into each file. If sharing macros becomes a pain point, the design can be extended later with a dedicated import/module feature.

### Alternative D: MiniJinja's `path_loader` without custom wrapping

Use MiniJinja's built-in `path_loader(root)` directly.

**Why it lost:** `path_loader` uses `safe_join`, which does NOT canonicalize. So a symlink inside the root that points outside the root is not caught. The grounding explicitly flags this: "MiniJinja's `safe_join` ... therefore does NOT stop a symlink inside the root that points outside." Toha's policy is to canonicalize + confine + refuse symlinks. The design implements a custom loader mirroring Toha's existing `!include` handler.

## Open questions and risks

- **Dynamic include semantics with loops and conditions.** The design validates includes statically, but `{% include %}` can appear inside `{% for %}` or `{% if %}` blocks. A missing include inside a `{% if false %}` block will still be caught at load time. Is this the desired behavior? (Conservative answer: yes, fail early. If needed, a future design can relax this to "only validate includes reachable under possible conditions," which is more complex.)
- **Include-related variable references.** If an includer does not use a variable but the include does, the variable is still validated as long as it is defined. This is correct, but relies on the union of all variables in the include tree being valid. Are we comfortable with variables that are defined but unreferenced by the top-level file?
- **Performance of the include graph walk.** If a template has a very large source tree with many includes, the load-time graph walk could be slow. Should we add a cache or early-exit optimization? (Likely no for the first implementation; revisit if telemetry shows slowness.)
- **Include statement inside a template inside a test.** Test fixtures can exercise includes; should we ensure at least one fixture per error type is in the suite? (Yes, per the fixture plan below.)

## Next implementation step

Implement `IncludeLoader::load_partial()` with path canonicalization, confinement check, and symlink rejection, then wire its loader function into `jinja::environment()` so that MiniJinja can resolve include names.
