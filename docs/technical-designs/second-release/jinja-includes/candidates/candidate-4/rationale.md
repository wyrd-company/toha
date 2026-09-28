# Rationale: Jinja `{% include %}` Confined to Template Root

## Problem

Toha users cannot factor repeated template content into reusable partials. Template authors must inline duplicated text, raising maintenance cost and making templates harder to read. The Jinja `{% include %}` statement is disabled today to prevent arbitrary filesystem access; the design must turn it on while preserving confinement to the selected template root.

The existing `!include` precedent (YAML data composition at load time) and `confined_file()` (path validation) establish the confinement model. The design must reuse that policy—canonicalize, confine with `starts_with(root)`, refuse symlinks—but adapted to Jinja's render-time evaluation and the two very different operational contexts: data composition vs. text rendering.

## Usage (caller's view)

Template authors place partials anywhere in the template root:

```
template/
  index.html         # {% include "partials/header.html" %}
  partials/
    header.html      # reusable content
shared/              # outside source tree, never emitted
  license.txt
```

Include paths are relative to the including file's directory:

- `{% include "partials/footer.html" %}` from `template/index.html` → `template/partials/footer.html`
- `{% include "../license.txt" %}` from `template/partials/header.html` → `template/license.txt`
- `{% include "../../etc/passwd" %}` is rejected: escapes root

Variables are shared with the includer's context. The included text is rendered and inserted inline.

Partials outside the source subdirectory are never emitted as their own output files, so a `partials/` directory at the template root can hold include-only fragments that don't map to outputs.

## Shape

A confining loader for MiniJinja with two components:

**1. IncludeGraph (load-time validation)**

Built once when the template loads, before any interview begins:
- Walks the source tree and `files` rules to discover all potentially-includable files.
- Validates each path: canonicalize, confine to root, reject symlinks.
- Detects include cycles via explicit stack (mirroring `!include`).
- Preloads all partial text into memory.
- Collects all variable references (AST-level) reachable through the include graph.

This is eager validation: errors surface before any render, and the cost is paid once per template, not per file rendered.

**2. ConfinedLoader (render-time enforcement)**

Handed to MiniJinja's `set_loader` callback:
- Takes an include name, resolves it relative to the current file's directory.
- Normalizes the path: rejects absolute paths, `..` escapes, symlinks.
- Looks up the preloaded text in the graph.
- If not found, returns `None`, and MiniJinja converts it to `TemplateNotFound`.

The current file is tracked via thread-local state as the plan renders each file. This allows relative-to-includer semantics without modifying include names in the template.

**Supported surfaces:** Includes are available in source-tree files and `files` rule sources only. Not in interview fields (prompt, default, options, etc.), not in target path segments (unsafe), not in messages or hook fields (unaudited without explicit design).

**Include root:** The whole `template.root`, not just `source_dir`. This allows partials to live outside the source tree and remain private (never emitted). It aligns with how `!include` works and how templates use `files` rules on support files.

**Relative semantics:** Resolved relative to the including file's directory, not a fixed root. This is more intuitive and allows partials to be moved/reorganized without changing include statements. The loader rewrites names: `{% include "x" %}` from `src/app.html` becomes a lookup for `src/x`.

**Cycle detection:** Static graph walking at load time, not MiniJinja's degraded recursion-limit message. Errors name the files involved: `include cycle: a.html → b.html → a.html`.

**Variable validation:** Walk the include graph at load time, collect all referenced ids transitively, and validate them all against defined ids in the interview. Late-bind variable errors (variables used only in an included file) are caught at load time, not render time.

**Interface depth:** Public surface is minimal. `IncludeGraph::load()` is called from `Template::load()`. The loader is internal; callers work only with the existing `Tmpl` API. The `current_file` tracking is hidden in the loader. This keeps confinement logic in one place and prevents callers from bypassing it.

Per boundary-discipline, the loader is the sole enforcement seam; all path validation and confinement happens there. Wire types (raw `PathBuf`, `Arc<ConfinedLoader>`) are internal to `plan.rs`. Callers see only the effects: includes work, and errors are precise.

## Synthesis decision

*Filled in by the orchestrator.*

## Tradeoffs accepted

- **Static validation overhead:** The graph is walked and preloaded at load time, validating every file that could be included. This costs I/O and memory. We accept this in exchange for early error detection (cycles, escapes, missing variables) before any interview runs, aligning with Toha's design principle of catching errors early.

- **Relative-to-includer semantics add complexity:** Rewriting include names requires the loader to track the current file and resolve paths relative to its directory. We accept this complexity in exchange for intuitive semantics: partials can be organized in directories and moved without updating includes elsewhere.

- **No `ignore missing` support (for now):** MiniJinja supports `{% include "x" ignore missing %}` to silently skip missing includes. We reject this for now to keep diagnostics clear and to avoid the temptation to use it to hide errors. A future iteration can add it with explicit design.

- **Partials share the includer's context:** No parameter passing or isolated scopes (as in some template engines). We accept this in exchange for simplicity and familiarity with Jinja. If a partial needs different data, compute it in the parent and pass it as a variable.

## Alternatives considered

**Alternative 1: Lazy loading with MiniJinja's built-in `path_loader`**

Use `minijinja::path_loader(template_root)` and rely on `safe_join` to confine paths. Simpler implementation; no preloading.

Why it lost: `safe_join` does not canonicalize and does not reject symlinks. A symlink inside the root pointing outside would escape. Cycle detection degrades to MiniJinja's generic "recursion limit exceeded" after ~50 iterations, not a named cycle. Variable references reached only through includes are not validated at load time; they surface late as render errors. These diagnostics gaps conflict with Toha's design of catching errors early and giving precise messages.

**Alternative 2: Fixed-root resolution**

All includes are resolved from the template root, not relative to the includer. `{% include "partials/footer.html" %}` always means `partials/footer.html` at the root, regardless of where the include statement is.

Why it lost: Less intuitive. A partial in `src/components/` that includes `src/utils/helper.html` must use the full path `src/utils/helper.html` every time, even from different locations. If components are reorganized, all includes must be updated. Relative-to-includer semantics allow partials to be self-contained and moveable, which is the benefit of factoring code in the first place.

**Alternative 3: Enable includes everywhere (interview fields, messages, hook fields)**

Extend the loader to all renderable surfaces: interview prompts, message text, hook commands, target path segments.

Why it lost: Target paths are unsafe; a computed path segment could escape the target directory. Interview/message/hook fields are unaudited; no decision has been made to enable Jinja features there without security review. Scope creep: the design would need to specify behavior for each surface separately (when are variables validated? which filters are available?). The chosen design is narrower: includes for file content only, the primary use case.

**Alternative 4: Inline cycle detection via MiniJinja's recursion limit**

Accept MiniJinja's default behavior: cycles degrade to "recursion limit exceeded" after 50 iterations. No static graph walking.

Why it lost: Error messages are generic and don't name the offending files. Cycles are caught only at render time, after setup costs. Toha's pattern is to validate at load time (the `!include` precedent) and give precise diagnostics. Static validation is the established norm.

## Open questions and risks

1. **Should we support `ignore missing`?** Some templates might want to include optional partials. The cost is adding a feature to the grammar and specifying its behavior. Should it be a render-time setting or per-include? Defer for now; simple "missing is an error" is clearer.

2. **How much does preloading cost on large templates?** If a template has hundreds of files and only a few are included, we're reading and parsing text we won't use. Profiling is needed. A lazy variant (load files on demand) could reduce I/O; cycle detection would still require graph walking upfront.

3. **Relative-to-includer means nested templates can't use absolute names.** If deep nesting becomes common, paths become fragile (long relative paths like `../../../../../../x.html`). An escape hatch (a fixed root alias, e.g., `{% include "@/x.html" %}`) could help. Design this later if nesting gets deep.

4. **MiniJinja's `undeclared_variables` per-AST analysis won't follow includes.** We walk the graph manually and collect refs. What if an include is inside a conditional (`{% if x %}{% include "y.html" %}{% endif %}`), and a variable is used only in `y.html` but only when `x` is true? We validate unconditionally (conservatively). This is correct but may flag false positives if a conditional include is intentionally used to avoid undefined variables. Documenting the limitation is enough for now.

5. **Permission/timeout/config knobs?** The design requires none. Cycle detection is O(V + E) on the include graph, not expensive. The preloading is standard I/O. No new capabilities to gate.

## Next implementation step

Implement `IncludeGraph::load()`: walk the source tree and `files` rules, validate confinement (canonicalize, `starts_with`, reject symlinks) for each file, detect cycles via explicit stack, preload all text, and collect transitive variable references by parsing each file's AST.
