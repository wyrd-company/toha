# Design: Jinja `{% include %}` within the template root

## Usage (template author's view)

### Quickstart

Enable Jinja includes by placing partial files in your source directory (default `template/`) and referencing them from other source files.

```
template/
├── template.yml
├── template/
│   ├── config.jinja
│   ├── header.jinja
│   └── main.py
```

In `main.py`:
```jinja
{% include "header.jinja" %}
# Main configuration
{{ config_text }}
```

In `header.jinja`:
```jinja
# Auto-generated on {{ now() | dateformat }}
```

### Call site 1: Simple partial reuse

**File structure:**
```
template/
├── template.yml
├── template/
│   ├── readme.jinja
│   ├── license.txt
│   └── setup.py
```

`setup.py`:
```jinja
{% include "readme.jinja" %}
# License below

{% include "license.txt" %}
```

`readme.jinja`:
```jinja
# {{ project_name }}

{{ description }}
```

**Result after rendering with `project_name="my-lib"`, `description="A library"`:**
```
# my-lib

A library
# License below

MIT License...
```

### Call site 2: Nested directory structure

**File structure:**
```
template/
├── template.yml
├── template/
│   ├── docker/
│   │   ├── Dockerfile
│   │   └── entrypoint.sh
│   ├── docker-compose.yml
│   └── docker/helpers.jinja
```

`docker-compose.yml` (in template root, not docker/ subdirectory):
```jinja
version: '3'
services:
  app:
    image: {{ image_name }}
    {% include "docker/helpers.jinja" %}
```

`docker/helpers.jinja`:
```jinja
volumes:
  - ./data:/app/data
environment:
  - DEBUG={{ debug }}
```

**Rendered output:**
```yaml
version: '3'
services:
  app:
    image: my-app:latest
    volumes:
      - ./data:/app/data
    environment:
      - DEBUG=true
```

### Call site 3: Relative includes (nested partials)

**File structure:**
```
template/
├── template.yml
├── template/
│   ├── main.py
│   ├── ui/
│   │   ├── forms.jinja
│   │   ├── validation.jinja
│   │   └── theme.jinja
```

`ui/forms.jinja`:
```jinja
{% include "theme.jinja" %}

# Form helpers
```

`main.py` (in template/ root):
```jinja
{% include "ui/forms.jinja" %}

# Main logic
```

**Result:** When `main.py` includes `ui/forms.jinja`, that file can include `theme.jinja` (relative to its own directory), resolving as `ui/theme.jinja`.

## Shape

### Module boundary and seam

A new `IncludeLoader` type and a `load_partial()` function in `src/jinja.rs` handle include resolution and confinement:

```rust
// src/jinja.rs

/// Confines include resolution to a root directory, canonicalizes paths,
/// and refuses symlinks, mirroring the !include and confined_file policy.
pub struct IncludeLoader {
    /// The template root: all includes must resolve within it.
    root: PathBuf,
    /// Cache of canonicalized paths (for cycle detection and symlink safety).
    cache: std::sync::Mutex<std::collections::HashMap<PathBuf, String>>,
}

impl IncludeLoader {
    /// Creates a loader for the given template root.
    /// `root` must be absolute and canonicalized.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Resolves an include name, returning the canonicalized path and source text.
    /// Enforces confinement: rejects absolute paths, `..` escape, symlinks.
    /// If already loaded, returns cached content; cycle detection lives at load time,
    /// not here.
    pub fn load_partial(
        &self,
        name: &str,
        source_dir: &Path,
    ) -> Result<String, IncludeError> {
        // not implemented: see behavior section
    }

    /// Pre-loads all includes reachable from a given source file,
    /// building an include graph for cycle detection and reference validation.
    /// Called once per Tmpl at compile time (load phase), not at render.
    pub fn validate_graph(
        &self,
        root_file: &Path,
        visited: &mut HashSet<PathBuf>,
        stack: &[PathBuf],
    ) -> Result<IncludeGraph, LoadError> {
        // not implemented
    }
}

pub struct IncludeGraph {
    /// Map from source file path to set of included file paths.
    includes: std::collections::HashMap<PathBuf, Vec<PathBuf>>,
    /// All files reachable in the include tree.
    all_files: HashSet<PathBuf>,
}

pub enum IncludeError {
    /// Include name is absolute or starts with `..`
    EscapeAttempt { name: String },
    /// Resolved path exists but is not a regular file, or is a symlink
    SymlinkOrDir { path: PathBuf },
    /// Resolved path is outside the root
    OutsideRoot { path: PathBuf, root: PathBuf },
    /// File not found (does not exist)
    NotFound { name: String, source_file: PathBuf },
    /// IO error reading the file
    IoError { path: PathBuf, source: std::io::Error },
}
```

### Integration with `Tmpl`

Modify `Tmpl::compile()` to optionally accept an `IncludeLoader`. When set, MiniJinja's environment gets a custom loader that delegates to `IncludeLoader::load_partial()`:

```rust
// src/jinja.rs

pub struct Tmpl {
    source: String,
    env: Environment<'static>,
    referenced_ids: HashSet<String>,
    // NEW: track which includes this template uses
    include_paths: Vec<PathBuf>,
}

impl Tmpl {
    /// Compiles a template with optional include support.
    /// If `loader` is Some, set MiniJinja's environment loader to delegate to it.
    /// The loader is set up to resolve names relative to `source_file_dir`
    /// (for relative-to-includer semantics).
    pub fn compile_with_includes(
        source: String,
        loader: Option<&IncludeLoader>,
        source_file_dir: PathBuf,
    ) -> Result<Self, Error> {
        // not implemented
    }

    pub fn include_paths(&self) -> &[PathBuf] {
        &self.include_paths
    }
}
```

### Load-time include-graph validation

At template load (in `template.rs:Template::load()`), before rendering begins, walk the include graph of all source files and `files:` sources to:
1. Detect cycles (via stack during traversal)
2. Validate that all included files exist and are not symlinks
3. Collect all referenced files for validation

```rust
// src/template.rs (new function)

/// Walks the Jinja include graph of a template,
/// validating confinement and cycle safety.
/// Called once at load time.
fn validate_jinja_includes(
    source_dir: &Path,
    template_root: &Path,
    loader: &IncludeLoader,
    problems: &mut Vec<Problem>,
) {
    // not implemented: traverse source tree + files rules, build graph
}
```

### Render surfaces: where includes apply

**YES (includes enabled):**
- Source-tree file contents (`plan.rs:384`)
- `files:` rule sources (`plan.rs:221`)
- Apply messages (before/after) (`plan.rs:261`)
- Interview fields: `prompt`, `description`, `placeholder` (question text surfaces only)

**NO (includes NOT available):**
- Target path segments (nonsensical; would break path escaping invariants)
- `when`, `format`, `computed` expressions (these are expressions, not templates; no filename context)
- `validate.regex` (not a template)
- Hook fields (`run`, `args`, `script path`) — hooks need explicit trust review and template author intent is unclear
- `template.yml` file itself (never rendered; confuses config with output)
- `static` files (copied as-is, never rendered)

**Rationale:** Restrict to surfaces where the semantic is "render a file fragment." Exclude expressions (no filename context), paths (confinement-unsafe), and hook commands (need explicit review). This preserves the principle that rendering is side-effect-free text composition.

### Relative-to-includer semantics

Include names are resolved relative to the **source file's directory**, not the template root. This enables natural factoring:

```jinja
{% include "header.jinja" %}      {# relative to this file's dir #}
{% include "ui/form.jinja" %}     {# relative to this file's dir #}
{% include "../shared.jinja" %}   {# parent dir; REJECTED if escapes root #}
```

**Implementation:** When setting MiniJinja's loader, wrap each include resolution to:
1. Resolve the name against the including file's directory
2. Canonicalize the result
3. Check `starts_with(source_dir)`
4. Verify not a symlink
5. Return the text

This matches the user's intuition: "include the file at this path relative to me."

### Cycle detection with precise diagnostics

Rather than relying on MiniJinja's generic recursion limit, statically walk the include graph at load time:

```rust
// src/template.rs (new function)

fn detect_include_cycles(
    root_file: &Path,
    source_dir: &Path,
    loader: &IncludeLoader,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    problems: &mut Vec<Problem>,
) {
    if stack.contains(&root_file) {
        let cycle = stack.iter()
            .skip_while(|p| **p != root_file)
            .chain(std::iter::once(&root_file))
            .map(|p| p.strip_prefix(source_dir).unwrap_or(p).display())
            .collect::<Vec<_>>()
            .join(" -> ");
        problems.push(Problem {
            path: root_file.display().to_string(),
            message: format!("jinja include cycle: {}", cycle),
        });
        return;
    }

    // Traverse includes of this file
    // For each included file:
    //   if not visited, recurse
    //   if in stack, cycle found
}
```

Error message format (matching `!include` style but distinct):
- `"jinja include cycle: template/main.py -> template/ui/forms.jinja -> template/main.py"`

### Reference validation at load

After walking the graph, use MiniJinja's `undeclared_variables()` (per-template AST analysis) on each source file **with includes statically resolved**. Variables referenced in includes are validated when their containing file is compiled. This avoids the reference-gap mentioned in grounding: every variable is validated by the time we're ready to render.

**Alternative:** If a variable is used only in an included file, today's per-template analysis would miss it. The design validates this by ensuring all includes are compiled and their references checked.

### Error contract (diagnostics)

All error messages for Jinja includes are prefixed with `"jinja include:"` to distinguish from `!include` (which uses `"include"`):

| Scenario | Message |
| --- | --- |
| Absolute path or `..` escape | `{file}: jinja include: include name cannot be absolute or escape root: {name}` |
| File outside root after canonicalization | `{file}: jinja include: include path escapes source directory: {name}` |
| Symlink | `{file}: jinja include: include target is a symlink: {path}` |
| Not a regular file (is directory) | `{file}: jinja include: include target is not a file: {path}` |
| File not found | `{file}: jinja include: include file not found: {name}` |
| Cycle detected | `{file}: jinja include cycle: {cycle}` |
| IO error | `{file}: jinja include: cannot read file: {path}: {err}` |

### Module map and thread-through

```
src/jinja.rs
├── IncludeLoader (new type)
├── IncludeError (new enum)
├── IncludeGraph (new type)
├── IncludeLoader::new()
├── IncludeLoader::load_partial()
├── IncludeLoader::validate_graph()
├── Tmpl::compile_with_includes() (new)
└── environment() [modified: if loader present, set_loader]

src/template.rs
├── Template::load() [modified: after parse, validate includes]
├── validate_jinja_includes() (new function)
├── detect_include_cycles() (new function)
└── LoadError [unchanged; reuses Problem list]

src/plan.rs
├── walk() [unchanged; uses rendered() which uses Tmpl]
├── rendered() [unchanged; calls Tmpl::render]
└── read_render() [unchanged]
```

The loader is created once when a Template is loaded (`Template::load()`) and passed to each `Tmpl::compile_with_includes()` call. By the time rendering begins in `plan.rs`, all includes are validated and the loader is safe to use at render time.

## Proposed documentation edits

### `docs/template-jinja.md`

Replace the line:
```
Jinja `import`, `include`, and `extends` are unavailable in rendered templates.
```

With:
```
Jinja `import` and `extends` are unavailable in rendered templates. Jinja 
`{% include %}` is available within source files and `files:` rule sources, 
confined to the template root; use relative paths from the including file's 
directory. Include names are resolved relative to the including file, so 
`{% include "header.jinja" %}` includes the file at the same level, and 
`{% include "ui/form.jinja" %}` includes from a `ui/` subdirectory. 
All includes are validated at template load time; cycles and escapes 
are detected and rejected with specific diagnostics.
```

Add a new section **Jinja includes**:
```markdown
## Jinja includes

Template files can include other template files to reuse fragments. Include 
names are relative paths resolved from the including file's directory.

```jinja
{% include "header.jinja" %}
{% include "styles/theme.jinja" %}
```

Includes are available in source files and `files:` rule sources. They are 
NOT available in target path segments, interview expressions, or hook commands.

All included files must exist within the source directory (or `source:` 
configured directory). Absolute paths and `..` escape attempts are rejected. 
Symlinks are not followed. Cycles are detected at load time and reported 
with the chain of includes involved.

Use `!include` in `template.yml` to include data files (YAML, JSON, TOML); 
it is separate from Jinja `{% include %}`.
```

### `docs/specifications/template-format.yml`

In the "Jinja evaluation" section, replace:
```
A macro is visible only in the value that defines it; `import`, `include`, 
and `extends` are not available.
```

With:
```
A macro is visible only in the value that defines it; `import` and `extends` 
are not available. Jinja `{% include %}` is available in source file contents 
and `files:` rule sources, confined to the source directory (`template/` by 
default or the directory named by `source`). Include names are resolved 
relative to the including file's directory. All includes are validated at 
load time; escapes and symlinks are rejected, and cycles are detected and 
reported with the chain involved.
```

## Fixtures to add

### Positive cases

1. **`basic-jinja-include`** — Simple include in a source file
   - `template/`
     - `template.yml` (no interview, one data value)
     - `template/main.txt` (includes `header.txt`)
     - `template/header.txt` (uses data value)
   - Expected output: combined text
   - Validates: basic include + variable scope

2. **`include-nested-dirs`** — Includes from nested directories
   - `template/`
     - `template/main.py` (includes `ui/form.jinja`)
     - `template/ui/form.jinja` (includes `theme.jinja`)
     - `template/ui/theme.jinja` (literal text)
   - Expected output: combined text in correct order
   - Validates: relative-to-includer semantics

3. **`include-in-files-rule`** — Include in a `files:` rule source
   - `template/`
     - `template.yml` (one `files` rule pointing to `scaffold.jinja`)
     - `template/scaffold.jinja` (includes `base.jinja`)
     - `template/base.jinja` (template text)
   - Expected output: rendered file from `files` rule, includes resolved
   - Validates: includes work in `files:` sources

### Negative cases

1. **`err-jinja-include-escape`** — Attempt `..` escape
   - `template/`
     - `template/main.txt` (includes `../secret.txt`)
   - Expected error: `"jinja include: include name cannot be absolute or escape root"`
   - Validates: escape prevention

2. **`err-jinja-include-symlink`** — Include is a symlink
   - `template/`
     - `template/main.txt` (includes `evil.txt`)
     - `template/evil.txt` → symlink to `/etc/passwd`
   - Expected error: `"jinja include: include target is a symlink"`
   - Validates: symlink rejection

3. **`err-jinja-include-outside-root`** — Include at path that escapes after canonicalization
   - `template/`
     - `template/main.txt` (includes `../template.yml`)
   - Expected error: `"jinja include: include path escapes source directory"`
   - Validates: post-canonicalization confinement

4. **`err-jinja-include-not-found`** — Include file does not exist
   - `template/`
     - `template/main.txt` (includes `missing.txt`)
   - Expected error: `"jinja include: include file not found"`
   - Validates: existence check

5. **`err-jinja-include-cycle`** — Cycle: A includes B includes A
   - `template/`
     - `template/a.txt` (includes `b.txt`)
     - `template/b.txt` (includes `a.txt`)
   - Expected error: `"jinja include cycle: template/a.txt -> template/b.txt -> template/a.txt"`
   - Validates: cycle detection

6. **`err-jinja-include-in-path`** — Attempt to use include in target path segment
   - `template/`
     - `template.yml` (files rule with path containing `{% include %}`)
   - Expected error: `"MiniJinja TemplateNotFound"` (include not available)
   - Validates: confinement to allowed surfaces

## Non-goals and deliberately excluded

- **Lazy loading:** All includes are validated eagerly at load time, not when rendered. This catches bugs early.
- **`ignore missing`:** Not exposed. A missing include is always an error.
- **Multiple candidates (`include ["a", "b"]`):** Not exposed.
- **Dynamic include paths:** `{% include variable_name %}` is not supported. Include names are static literals in the source; rendering variables in the path would require path traversal at render time and is unsafe.
- **Macros/functions from includes:** Included content is text, not compiled in the same scope as the includer. Macro definitions in includes would require reimplementing MiniJinja's scope; instead, users can define macros inline or use Jinja's `{% macro %}` within a single file.

## Implementation notes

- **Thread safety:** The `IncludeLoader` uses a `Mutex<HashMap>` to cache canonicalized paths. The cache is read-heavy and immutable after load time, so contention is minimal.
- **Serialization:** Staged interviews (saved state) do not need to serialize the loader; the loader is reconstructed when the interview resumes from the template.
- **Backward compatibility:** Including no `{% include %}` statements is unchanged. Existing templates render identically.
- **Fidelity:** The design uses Toha's own `Source: &Path` + canonicalize pattern, not MiniJinja's `path_loader` or `safe_join`, to maintain full control over confinement and symlink handling.
