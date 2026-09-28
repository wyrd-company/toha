# Design: Jinja `{% include %}` Confined to Template Root

## Caller usage

### Quickstart

Enable partials by placing template fragments in your source directory and including them with standard Jinja syntax:

```
template/
  index.html          # {% include "partials/header.html" %}
  partials/
    header.html       # reusable HTML header
    footer.html       # reusable HTML footer
```

File `template/index.html`:
```jinja
{% include "partials/header.html" %}
<h1>{{ title }}</h1>
{% include "partials/footer.html" %}
```

File `template/partials/header.html`:
```jinja
<header>
  <h2>{{ page_title }}</h2>
</header>
```

When Toha renders `index.html` with `page_title: "Welcome"`, it compiles and renders `header.html` in context, inserting the result.

### Call sites

**Site 1: Source-tree file with relative-to-includer includes**

```
template.yml:
  name: my-app
  source: template
  data:
    app_name: MyApp

template/
  src/
    app.rs         # {% include "../shared/license.rs" %}
    main.rs
  shared/
    license.rs     # /* Copyright ... */
```

File `template/src/app.rs`:
```jinja
{% include "../shared/license.rs" %}
use crate::main;
fn main() {
  println!("{{ app_name }}");
}
```

File `template/shared/license.rs`:
```jinja
// SPDX-License-Identifier: MIT
// {{ app_name }} - Licensed under MIT
```

Toha resolves `"../shared/license.rs"` relative to `template/src/`, finding `template/shared/license.rs`. The rendered output includes the license comment with `app_name` substituted.

**Site 2: `files` rule with partials outside source tree**

```
template.yml:
  name: scaffold
  files:
    - source: partials/class.template
      path: "src/{{ name }}.java"
      each: "classes as cls"
  data:
    package: com.example

template/
  (source subdirectory)
partials/
  class.template    # {% include "../shared/javadoc.txt" %}
shared/
  javadoc.txt       # /** Standard javadoc header */
```

File `partials/class.template`:
```jinja
{% include "../shared/javadoc.txt" %}
package {{ package }};
public class {{ cls }} {
}
```

Toha resolves includes relative to `partials/class.template`, so `"../shared/javadoc.txt"` finds `shared/javadoc.txt` at the template root. Both files are outside the source subdirectory, so they are never emitted as their own output files.

**Site 3: Shared partial used from multiple files**

```
template/
  routes/
    auth.html      # {% include "../components/button.html" with { text: "Login" } %}
    home.html      # {% include "../components/button.html" with { text: "Get started" } %}
  components/
    button.html    # <button>{{ text }}</button>
```

Each route includes the same button partial with different context. Both includes resolve relative to their includer's directory, so both find `template/components/button.html`.

---

## Data & type sketch

### Load-time include graph

```rust
/// A validated include graph, built once per template at load.
pub struct IncludeGraph {
    /// Map: rendered path (relative to template root) → source text
    partials: BTreeMap<PathBuf, String>,
    
    /// Adjacency for cycle detection: file → files it includes
    edges: BTreeMap<PathBuf, HashSet<PathBuf>>,
    
    /// All variable references reachable through the include graph
    all_referenced_ids: HashSet<String>,
    
    /// Template root; all includes must resolve inside it
    root: PathBuf,
}

impl IncludeGraph {
    /// Build the include graph by walking the source tree and `files` rules.
    /// Validates confinement (no escapes, no symlinks) and cycles.
    /// Returns the graph or LoadError with precise diagnostics.
    pub fn load(
        root: &Path,
        source_dir: &Path,
        rules: &[FileRule],
        now: &jiff::Zoned,
    ) -> Result<Self, LoadError> {
        not_implemented!()
    }

    /// Return the text of a partial, if it was validated and loaded.
    pub fn get(&self, path: &Path) -> Option<&str> {
        self.partials.get(path).map(|s| s.as_str())
    }

    /// All ids that are referenced (directly or through includes).
    pub fn referenced_ids(&self) -> &HashSet<String> {
        &self.all_referenced_ids
    }
}

/// The confining loader handed to MiniJinja.
/// Resolves include names relative to the including file's directory.
pub struct ConfinedLoader {
    graph: Arc<IncludeGraph>,
    /// Current file being rendered; tracks for relative resolution.
    /// Caller pushes/pops this as it renders each file.
    current_file: RefCell<Option<PathBuf>>,
}

impl ConfinedLoader {
    pub fn new(graph: Arc<IncludeGraph>) -> Self {
        Self {
            graph,
            current_file: RefCell::new(None),
        }
    }

    /// Set the file currently being rendered, for relative include resolution.
    pub fn set_current(&self, file: Option<PathBuf>) {
        *self.current_file.borrow_mut() = file;
    }

    /// The MiniJinja loader callback. Resolves `name` relative to the current file.
    pub fn load_template(&self, name: &str) -> Result<Option<String>, minijinja::Error> {
        let current = self
            .current_file
            .borrow()
            .as_ref()
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "include outside of file render context",
                )
            })?
            .clone();

        // Rewrite the include name relative to the current file's directory.
        let includer_dir = current
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        let resolved = includer_dir.join(name);

        // Normalize the path: reject `..` escapes, absolute paths, symlinks.
        let normalized = self.normalize_path(&resolved)?;

        // Look up in the preloaded graph.
        match self.graph.get(&normalized) {
            Some(text) => Ok(Some(text.to_string())),
            None => Ok(None), // MiniJinja converts None to TemplateNotFound.
        }
    }

    fn normalize_path(&self, path: &Path) -> Result<PathBuf, minijinja::Error> {
        // Reject absolute paths.
        if path.is_absolute() {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!(
                    "include path must be relative: {}",
                    path.display()
                ),
            ));
        }

        // Canonicalize to resolve `.` and `..`.
        let candidate = self.graph.root.join(&path);
        let Ok(actual) = candidate.canonicalize() else {
            // Path doesn't exist on disk, but the graph may still have it.
            // Try normalizing manually.
            return self.normalize_without_canonicalize(path);
        };

        // Confine: must be inside template root.
        if !actual.starts_with(&self.graph.root) {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!(
                    "include escapes template root: {}",
                    path.display()
                ),
            ));
        }

        // Reject symlinks.
        if actual.is_symlink() {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!(
                    "include symlink not allowed: {}",
                    path.display()
                ),
            ));
        }

        Ok(actual.strip_prefix(&self.graph.root).unwrap().to_path_buf())
    }

    fn normalize_without_canonicalize(&self, path: &Path) -> Result<PathBuf, minijinja::Error> {
        // Simple path normalization: resolve `.` and `..` components.
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err(minijinja::Error::new(
                            minijinja::ErrorKind::InvalidOperation,
                            format!(
                                "include path escapes template root: {}",
                                path.display()
                            ),
                        ));
                    }
                }
                std::path::Component::Normal(name) => parts.push(name.to_owned()),
                std::path::Component::CurDir => {}
                _ => {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        format!(
                            "invalid include path: {}",
                            path.display()
                        ),
                    ));
                }
            }
        }
        let result: PathBuf = parts.iter().collect();
        
        // Even if the file doesn't exist, the path structure is valid.
        // Return it, and the lookup will fail later if it's not in the graph.
        Ok(result)
    }
}
```

### Integration points

**In `src/jinja.rs`:**

```rust
pub struct Tmpl {
    source: String,
    env: Environment<'static>,
    referenced_ids: HashSet<String>,
    /// Only Some when this template is rendered with a confined loader.
    loader: Option<Arc<ConfinedLoader>>,
}

impl Tmpl {
    /// Compile with a confined loader for includes.
    pub fn compile_with_loader(
        source: String,
        loader: Arc<ConfinedLoader>,
        current_file: PathBuf,
    ) -> Result<Self, Error> {
        let mut env = environment();
        env.set_loader({
            let loader = loader.clone();
            move |name| loader.load_template(name)
        });
        env.add_template_owned("value", source.clone())?;
        let referenced_ids = env.get_template("value")?.undeclared_variables(false);
        
        Ok(Self {
            source,
            env,
            referenced_ids,
            loader: Some(loader),
        })
    }

    /// Compile without a loader (for interview fields, where includes are disabled).
    pub fn compile(source: String) -> Result<Self, Error> {
        // Existing behavior.
        let mut env = environment();
        env.add_template_owned("value", source.clone())?;
        let referenced_ids = env.get_template("value")?.undeclared_variables(false);
        Ok(Self {
            source,
            env,
            referenced_ids,
            loader: None,
        })
    }

    pub fn render<S: Serialize>(&self, ctx: S) -> Result<String, RenderError> {
        if let Some(loader) = &self.loader {
            loader.set_current(None); // Clear after render.
        }
        self.env.get_template("value")?.render(ctx)
    }
}
```

**In `src/plan.rs`:**

The `Plan::build` method receives the `IncludeGraph`. Before rendering any file, it validates that all referenced ids (including those reached through includes) are defined:

```rust
impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Self, PlanError> {
        let ctx = context_from_answers(&completed.answers, &template.data, &completed.now);

        // Load the include graph once, validating confinement and cycles.
        let graph = Arc::new(
            IncludeGraph::load(
                &template.root,
                &template.source_dir,
                &template.files,
                &completed.now,
            )
            .map_err(|e| PlanError::Render {
                path: template.root.clone(),
                message: e.to_string(),
            })?
        );

        // Validate that all transitively referenced ids are defined.
        let mut seen = HashSet::new();
        for id in &template.data.keys().map(|id| id.as_str().to_owned()).collect::<HashSet<_>>() {
            seen.insert(id.clone());
        }
        for ref_id in graph.referenced_ids() {
            if !seen.contains(ref_id) && !is_global(ref_id) {
                return Err(PlanError::Render {
                    path: template.root.clone(),
                    message: format!(
                        "id is not defined by an earlier node: {} (reached through include)",
                        ref_id
                    ),
                });
            }
        }

        let loader = Arc::new(ConfinedLoader::new(graph));
        let mut plan = Self {
            files: vec![],
            conflicts: vec![],
            hooks: vec![],
            before_apply: None,
            after_apply: None,
        };

        walk_with_loader(
            &template.source_dir,
            &template.source_dir,
            target,
            template,
            &ctx,
            &mut plan,
            &loader,
        )?;

        // ... rest of plan building ...
    }
}

fn read_render_with_loader(
    path: &Path,
    ctx: &impl serde::Serialize,
    loader: &Arc<ConfinedLoader>,
) -> Result<String, PlanError> {
    let bytes = fs::read(path).map_err(|source| PlanError::Io {
        path: path.into(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| PlanError::Render {
        path: path.into(),
        message: "file is not UTF-8; add it to static".into(),
    })?;

    // Set current file for relative include resolution.
    loader.set_current(Some(path.to_path_buf()));

    let result = Tmpl::compile_with_loader(text, loader.clone(), path.to_path_buf())
        .and_then(|t| t.render(ctx))
        .map_err(|e| PlanError::Render {
            path: path.into(),
            message: e.to_string(),
        });

    loader.set_current(None);
    result
}
```

---

## Error contract

All diagnostics are Jinja-specific and distinct from `!include` messages:

| Error | Context | Example |
| --- | --- | --- |
| `include escapes template root: ../etc/passwd` | Loader | When include path resolves outside template root. |
| `include symlink not allowed: partials/bad.html` | Loader | When include target is a symlink. |
| `include path must be relative: /etc/passwd` | Loader | When include path is absolute. |
| `include path escapes template root: ../../x` | Loader | When `..` components escape root. |
| `tried to include non-existing template "missing.html"` | MiniJinja | When included file is not in the graph. |
| `include cycle: x.html → y.html → x.html` | Load time (IncludeGraph) | When include graph has a cycle. |

---

## Fixtures

### Positive: relative-to-includer includes

```
template/
  src/
    app.rs         # {% include "../shared/license.rs" %}
  shared/
    license.rs     # License header
```

Expected: includes resolve relative to `src/app.rs`'s directory, finding `shared/license.rs`.

### Positive: partial outside source tree

```
template/ (empty source tree)
partials/
  header.txt       # Common header
files:
  - source: template.txt
    path: output.txt
```

`template.txt` contains `{% include "../partials/header.txt" %}`. Partial lives outside source tree and is never emitted as its own file.

### Negative: include escapes root

```
template/
  src/
    app.html       # {% include "../../etc/passwd" %}
```

Expected: error "include escapes template root".

### Negative: include is symlink

```
template/
  src/
    app.html       # {% include "../bad.html" %}
  bad.html -> (symlink to /etc/passwd)
```

Expected: error "include symlink not allowed".

### Negative: include cycle

```
template/
  a.html           # {% include "b.html" %}
  b.html           # {% include "a.html" %}
```

Expected: error "include cycle: a.html → b.html → a.html" detected at load time.

### Negative: missing include

```
template/
  app.html         # {% include "missing.html" %}
```

Expected: render-time error "tried to include non-existing template missing.html".

---

## Proposed documentation edits

### `docs/template-jinja.md`

Change:

> Jinja `import`, `include`, and `extends` are unavailable in rendered templates.

To:

> Jinja `import` and `extends` are unavailable in rendered templates. Jinja `{% include %}` is available in source-tree files and `files` rule sources. Include paths are resolved relative to the including file's directory and must stay within the selected template root. Use Toha's `!include` in `template.yml` to load support data files; it is separate from Jinja's `include` statement and operates on YAML/JSON/TOML structured data at load time.

Add a new section "Jinja includes":

> ### Jinja includes
>
> You can factor repeated template content into reusable partials and include them using standard Jinja syntax:
>
> ```
> template/
>   index.html          # {% include "partials/header.html" %}
>   partials/
>     header.html
> ```
>
> Include paths are resolved relative to the directory of the file containing the include statement. So `{% include "../shared/footer.html" %}` from `src/app.html` looks for `shared/footer.html` at the template root.
>
> Included content must remain within the selected template root. Absolute paths, `..` escapes, and symlinks are rejected. Cycles (a includes b includes a) are detected at load time.

### `docs/specifications/template-format.yml`

Update the Jinja section:

> Jinja evaluation. Toha evaluates Jinja with minijinja. A field whose value type is string is a template: a string value is rendered, and the result is the field value. [...existing...] A macro is visible only in the value that defines it; `import` and `extends` are not available; `include` is available in source-tree files and `files` rule sources only.
>
> Jinja includes. Source-tree files and `files` rule sources may contain `{% include "path" %}` to include text from another file within the template root. Include paths are resolved relative to the including file's directory. Absolute paths, `..` escapes, and symlinks are forbidden. Cycles are detected at load time. Interviews, messages, and target paths do not support includes.

Add to validation criteria:

> , and every include path resolves within the template root with no absolute paths, symlink escapes, or cycles

---

## Module and seam map

```
src/jinja.rs
├─ environment()               [unchanged; no loader]
├─ Tmpl::compile()             [unchanged; for interview fields]
├─ Tmpl::compile_with_loader() [new; for file surfaces with ConfinedLoader]
└─ Tmpl::render()              [manages loader context]

src/template.rs
├─ Template::load()            [calls IncludeGraph::load()]
├─ IncludeGraph::load()        [new; validates graph at load time]
├─ IncludeGraph::get()         [new; preloaded partials lookup]
├─ IncludeGraph::referenced_ids() [new; transitive validation]
└─ ConfinedLoader              [new; MiniJinja loader callback]

src/plan.rs
├─ Plan::build()               [receives Arc<IncludeGraph>; validates refs]
├─ walk_with_loader()          [new; renders files with loader context]
└─ read_render_with_loader()   [new; sets current_file for relative resolution]
```

The trust seam is `Template::load()`: the selected template root is canonicalized once, and all include paths are confined relative to it. The `ConfinedLoader` enforces that boundary on every render.
