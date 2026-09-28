# Design — Jinja `{% include %}` as a confined, load-time include closure

Candidate for task 1076 (`epic/second-release`). Design only: signatures with
`not implemented` bodies, proposed (not applied) doc/spec edits, described
fixtures. No production code, spec, or schema is edited here.

## Headline

`{% include %}` becomes available **only where a whole file under the template
root is rendered as a text document** (source-tree files and `files:` rule
sources). Every include target is resolved **relative to the template root**,
**literal string only**, and the whole transitive set of partials is resolved,
confined, read, and registered **at compile time**. No filesystem loader is ever
installed on the render environment: at render, MiniJinja sees only an in-memory
set of partials it can never step outside of. The single confinement seam is one
function, `IncludeRoot::locate`, mirroring the existing `!include` /
`confined_file` precedent (canonicalize + `starts_with(root)` + refuse symlink).

---

# 1. Caller usage first (template author's view)

## Quickstart

A file body rendered by Toha can now factor shared text into a partial and pull
it in:

```jinja
{% include "partials/frontmatter.md" %}
# {{ title }}
```

Rules an author must know:

- The include name is a **path relative to the template root**, using `/`
  separators, e.g. `"partials/frontmatter.md"`. It is the same for every file
  that includes it; it is **not** resolved relative to the including file.
- The name must be a **literal string**. `{% include some_variable %}` is
  rejected. (Candidate lists — `{% include ["a", "b"] %}` — and `ignore missing`
  are supported because their targets are still literals.)
- Partials may live **anywhere inside the template root**. Put them **outside
  the source subdirectory** (e.g. a top-level `partials/`) and they are never
  emitted as their own output files. A partial placed *inside* the source
  subdirectory is emitted like any other source file unless an `ignore` glob
  excludes it.
- Includes work in **file bodies only** — source-tree files and `files:` rule
  sources. They are not available in prompts, defaults, `when`/`format`
  expressions, options, target path segments, apply messages, or hook fields.
- `import` and `extends` remain unavailable. Toha's YAML `!include` (in
  `template.yml`) is a separate feature and is unchanged.

## Call site A — a shared front-matter partial

Layout (author authors this; `source:` defaults to `template`):

```
weekly-note/
├── template.yml
├── partials/
│   └── frontmatter.md          # outside source dir → never emitted
└── template/
    └── {{ slug }}.md
```

`partials/frontmatter.md`:

```jinja
---
title: {{ title }}
status: {{ status }}
---
```

`template/{{ slug }}.md`:

```jinja
{% include "partials/frontmatter.md" %}
# {{ title }}
```

Answers `{title: "Weekly Notes", slug: "weekly-notes", status: "draft"}` render
to `weekly-notes.md`:

```markdown
---
title: Weekly Notes
status: draft
---
# Weekly Notes
```

Output tree contains only `weekly-notes.md`. `partials/` is outside the source
subdirectory, so it is not walked and not emitted.

## Call site B — a `files:` rule source that includes a license header

`template.yml` (excerpt):

```yaml
files:
  - each: modules
    source: parts/module.rs.j2
    path: "src/{{ item | snake }}.rs"
```

`parts/module.rs.j2`:

```jinja
{% include "parts/license.txt" %}
pub mod {{ item | snake }};
```

`parts/license.txt`:

```jinja
// SPDX-License-Identifier: {{ license }}
```

Here the include closure is resolved when `template.yml` loads (the `files:`
source is compiled at load), so a missing `parts/license.txt` or an undefined
`license` reference is reported **before any interview runs**.

## Call site C — a partial that includes another partial (nested)

```
├── partials/
│   ├── page.html        # includes "partials/head.html"
│   └── head.html        # includes "partials/meta.html"
│   └── meta.html
└── template/
    └── index.html       # includes "partials/page.html"
```

Nested includes resolve transitively: rendering `index.html` pulls in `page` →
`head` → `meta`, all confined to the template root, all registered before the
render begins. A cycle anywhere in that chain is reported by name at compile
time, not as a generic recursion error.

---

# 2. Data / type sketch (derived from the usage above)

All new types live in `src/jinja.rs`, beside `Tmpl`.

```rust
use std::{collections::{BTreeMap, HashSet}, path::{Path, PathBuf}, sync::Arc};

/// The confinement boundary and name resolver for `{% include %}`.
///
/// A cheap, cloneable handle to the *selected template root* — the canonical
/// `Template::root` derived from 1074's `ResolvedTemplate.folder`. This type
/// consumes that identity; it does not define a new one (per the grounding:
/// the root is already established by resolution/1074).
///
/// It is the ONE seam every include name passes through. `locate` is the only
/// code in the include path that touches the filesystem, and it closes all
/// three escape vectors there (absolute/`..`, escape-by-canonical-target,
/// symlink-inside-root-pointing-out).
#[derive(Clone, Debug)]
pub struct IncludeRoot(Arc<PathBuf>); // invariant: the wrapped path is canonical

/// The transitive set of partials reachable from one file body via literal
/// `{% include %}` targets, resolved and confined once at compile time.
///
/// Built by a static walk of the include graph (mirroring the `!include`
/// `stack: Vec<PathBuf>` in `template.rs::resolve`). Because every target is a
/// literal and every partial is read here, the render environment needs no
/// loader: the closure is complete and in memory before rendering starts.
#[derive(Debug, Default)]
struct IncludeClosure {
    /// root-relative, forward-slash name  →  confined UTF-8 source text.
    /// Keys are exactly the strings authors write in `{% include "..." %}`.
    partials: BTreeMap<String, String>,
    /// Union of `undeclared_variables(false)` across the root body and every
    /// partial — the variables the *whole* file needs, including those reached
    /// only through an include (which per-template analysis misses today).
    referenced_ids: HashSet<String>,
}

/// Everything that can go wrong turning a file body + root into a `Tmpl`.
/// Distinct wording from the YAML `!include` diagnostics so the two features
/// never collide in a message (grounding hard requirement).
#[derive(Debug, thiserror::Error)]
pub enum IncludeError {
    /// MiniJinja syntax/compile error in the body or a partial (unchanged wording).
    #[error("{0}")]
    Template(#[from] minijinja::Error),
    #[error("jinja include not found: {name}, included by {by}")]
    NotFound { name: String, by: String },
    #[error("jinja include escapes template root: {name}, included by {by}")]
    Escape { name: String, by: String },
    #[error("jinja include path is a symlink: {name}, included by {by}")]
    Symlink { name: String, by: String },
    #[error("jinja include is not utf-8: {name}, included by {by}")]
    NotUtf8 { name: String, by: String },
    #[error("jinja include is not readable: {name}: {source}")]
    Unreadable { name: String, #[source] source: std::io::Error },
    #[error("jinja include cycle: {path}")] // e.g. "note.md -> a.md -> note.md"
    Cycle { path: String },
    #[error("jinja include target must be a literal string, included by {by}")]
    Dynamic { by: String },
}
```

`Tmpl` gains one constructor and keeps the same fields; the plain path is
untouched:

```rust
pub struct Tmpl {
    source: String,
    env: Environment<'static>,
    referenced_ids: HashSet<String>,
}

impl Tmpl {
    /// UNCHANGED. No partials registered, no loader set → any `{% include %}`
    /// fails exactly as today. Used by every non-file surface.
    pub fn compile(source: String) -> Result<Self, minijinja::Error> { /* existing */ }

    /// NEW. Resolve the include closure against `root`, confine and register
    /// every partial, then compile the body. `label` is the file's
    /// root-relative path, used only for diagnostics (the initial `by` and the
    /// head of any cycle path).
    ///
    /// Guarantees on return:
    /// - every partial named by a literal `{% include %}` (transitively) is
    ///   registered under its root-relative name;
    /// - no loader is installed, so render performs zero filesystem access;
    /// - `references()` includes variables reached only through an include.
    pub fn compile_in_root(
        source: String,
        label: &str,
        root: &IncludeRoot,
    ) -> Result<Self, IncludeError> {
        let closure = IncludeClosure::build(&source, label, root)?;
        let mut env = environment();
        for (name, text) in &closure.partials {
            env.add_template_owned(name.clone(), text.clone())?;
        }
        env.add_template_owned("value", source.clone())?;
        // Deliberately NO env.set_loader(...): the closure is complete.
        Ok(Self { source, env, referenced_ids: closure.referenced_ids })
    }

    // references(), source(), render() unchanged. render() still renders "value".
}
```

## The one confinement seam

```rust
impl IncludeRoot {
    /// `root` is already canonical (from `Template::load`, template.rs:710).
    pub fn new(root: &Path) -> Self {
        Self(Arc::new(root.to_path_buf()))
    }

    /// Resolve a literal include name to a confined, canonical, regular file
    /// under the root. The ONLY filesystem-touching step for includes.
    ///
    /// Order (fail closed, clearest message first):
    ///   1. reject absolute paths and `\` → Escape.
    ///   2. reject any `..` component up front → Escape (clear message before IO).
    ///   3. reject a symlink at ANY path component → Symlink
    ///      (mirrors plan.rs::has_symlink_component and the source-walk policy
    ///      of refusing symlinks rather than following them; this is what stops
    ///      a symlink INSIDE the root that points OUTSIDE it).
    ///   4. canonicalize; Err → NotFound.
    ///   5. `!actual.starts_with(root)` → Escape (defense in depth).
    ///   6. `!actual.is_file()` → NotFound.
    fn locate(&self, name: &str, by: &str) -> Result<PathBuf, IncludeError> {
        // TODO: implement steps 1–6 exactly as documented; reuse the
        // component-walk shape of plan.rs::has_symlink_component for step 3.
        let _ = (name, by);
        unimplemented!("IncludeRoot::locate")
    }
}
```

## The static closure walk (diagnostics + reference union)

```rust
impl IncludeClosure {
    /// Walk the literal include graph from `source`, confining and reading each
    /// partial. Cycle detection uses an explicit stack of canonical paths, the
    /// same shape as template.rs::resolve for `!include`.
    fn build(source: &str, label: &str, root: &IncludeRoot) -> Result<Self, IncludeError> {
        let mut closure = IncludeClosure::default();
        let mut stack: Vec<PathBuf> = Vec::new();      // canonical paths, cycle guard
        let mut labels: Vec<String> = vec![label.into()]; // for a readable cycle path
        // TODO: recurse:
        //   - literal_include_targets(text) -> Vec<String> (see below)
        //     (dynamic target encountered → IncludeError::Dynamic{ by })
        //   - for each name not already in closure.partials:
        //       path = root.locate(name, by)?;
        //       if stack.contains(&path) → IncludeError::Cycle {
        //           path: labels.iter().chain([name]).join(" -> ") }
        //       text = read confined file (Unreadable / NotUtf8 on failure)
        //       closure.partials.insert(name, text)
        //       recurse into text with by=name, pushing/popping stack+labels
        //   - closure.referenced_ids |= undeclared_variables(false) of this text
        let _ = (source, label, root, &mut closure, &mut stack, &mut labels);
        unimplemented!("IncludeClosure::build")
    }
}

/// Extract literal `{% include %}` targets from one template body.
///
/// Implemented on MiniJinja's own parser (feature `unstable_machinery`):
/// `minijinja::machinery::parse(source, ...)` → AST, then collect
/// `ast::Stmt::Include` nodes. For each:
///   - `Include.name == Expr::Const(String)`            → one literal target
///   - `Include.name == Expr::List([Const(String), ..])`→ candidate list
///   - anything else (variable, filter, concat, ...)    → Err(Dynamic)
/// Reusing the vendor's parser guarantees the walk sees exactly what the
/// renderer would, and avoids a hand-rolled Jinja tokenizer.
fn literal_include_targets(source: &str, by: &str) -> Result<Vec<String>, IncludeError> {
    let _ = (source, by);
    unimplemented!("literal_include_targets via minijinja::machinery::parse")
}
```

---

# 3. Module / seam map (how it threads in)

Three files change; the confinement lives in exactly one function.

```
Template::load (template.rs)
  └─ root = folder.canonicalize()                     [existing]
  └─ IncludeRoot::new(&root)  ── derived from root, NOT stored (single source
                                 of truth is Template::root; construct on demand)
  └─ files[i].source content compiled via
        b.tmpl_in_root(content, source_label, &root)  [NEW call, load-time]
            └─ Tmpl::compile_in_root
                 └─ IncludeClosure::build
                      └─ IncludeRoot::locate   ◄── the ONE confinement seam
     (load-time reference validation: b.refs(t.references(), ...) now sees
      variables reached through includes, so files: sources are validated
      across the closure)

Plan::build (plan.rs)
  └─ let include_root = IncludeRoot::new(&template.root);
  └─ walk(...) source-tree files:
        read_render(&source, ctx, &include_root, &relative_label)  [plan-time]
            └─ Tmpl::compile_in_root(text, label, root)?.render(ctx)
  └─ files: rule sources:
        read_render(&origin, &local, &include_root, &rule_source_label)
  └─ target path segments: rendered(segment, ...) → Tmpl::compile  [UNCHANGED: no includes]
  └─ apply messages, hooks: existing Tmpl/Expr                     [UNCHANGED: no includes]

interview.rs / template.rs field compiles (prompt/default/when/format/options):
  └─ b.tmpl / b.expr → Tmpl::compile / Expr::compile               [UNCHANGED: no includes]
```

Signature deltas to existing functions (plan.rs):

```rust
// was: fn rendered(text, ctx, source) -> Result<String, PlanError>   (path segments; unchanged)
// was: fn read_render(path, ctx) -> Result<String, PlanError>
fn read_render(
    path: &Path,
    ctx: &impl serde::Serialize,
    root: &IncludeRoot,
    label: &str,
) -> Result<String, PlanError> {
    // TODO: read bytes → UTF-8 (existing "add it to static" message) →
    //       Tmpl::compile_in_root(text, label, root)
    //         .and_then(|t| t.render(ctx))
    //         .map_err(|e| PlanError::Render { path: path.into(), message: e.to_string() })
    unimplemented!()
}
```

And a sibling of `Builder::tmpl` in template.rs used only for `files:` source
content:

```rust
impl Builder {
    /// Like `tmpl`, but compiles the file body against the include root so the
    /// reference check (`self.refs`) covers variables reached through includes.
    fn tmpl_in_root(&mut self, content: String, path: &str, root: &IncludeRoot) -> Option<Tmpl> {
        // TODO: match Tmpl::compile_in_root(content, path, root) {
        //   Ok(t)  => { self.refs(t.references(), path, &[]); Some(t) }
        //   Err(e) => { problem(&mut self.problems, path, e.to_string()); None }
        // }
        unimplemented!()
    }
}
```

`PlanError` and `LoadError`/`Problem` are unchanged: `IncludeError` is folded in
by `Display` (`e.to_string()`) exactly as `minijinja::Error` is today, so no new
error variant leaks to the CLI or crate boundary.

---

# 4. Full error / results contract

Every diagnostic string this feature can emit. All are worded distinctly from
the YAML `!include` messages (`"include escapes template root"`, `"include
cycle"`, `"include file not found: {name}"`, `"unsupported include extension"`,
`"include cannot be read"`) so the two features never collide.

| Condition | Message | Where surfaced |
| --- | --- | --- |
| Literal target does not exist | `jinja include not found: {name}, included by {by}` | load (files source) / plan (source file) |
| Resolves outside root (`..`, absolute, canonical target escapes) | `jinja include escapes template root: {name}, included by {by}` | same |
| Any path component is a symlink | `jinja include path is a symlink: {name}, included by {by}` | same |
| Partial is not valid UTF-8 | `jinja include is not utf-8: {name}, included by {by}` | same |
| Partial cannot be read (IO) | `jinja include is not readable: {name}: {io error}` | same |
| Cycle in the literal include graph | `jinja include cycle: {a -> b -> a}` | same |
| Non-literal (`{% include var %}`) | `jinja include target must be a literal string, included by {by}` | same |
| Syntax error in body or partial | (MiniJinja's own message, unchanged) | same |
| Undefined variable reached only via an include, in a `files:` source | `id is not defined by an earlier node: {name}` (existing `Builder::refs` wording) | load |

`{by}` is the root-relative path of the including file (the source-tree file's
path under the source dir, or the `files[i].source` path). The cycle path lists
labels from the entry file through the repeated node.

Success contract: `Tmpl::compile_in_root` returns a `Tmpl` whose `render`
produces the fully composed text and whose `references()` is the closure union.
Rendering is a pure function of body + partials + context, unchanged for every
template that uses no includes (closure is then just the body itself).

---

# 5. Proposed edits (NOT applied)

### `docs/template-jinja.md` (replace lines 120–122)

Current:

> ...Jinja `import`, `include`, and `extends` are unavailable in rendered
> templates. Use Toha's `!include` in `template.yml` to load support data
> files; it is separate from Jinja's `include` statement.

Proposed:

> Jinja `{% include %}` is available in **file bodies** — source-tree files and
> `files:` rule sources. An include name is a path relative to the template
> root (`{% include "partials/header.md" %}`), must be a literal string, and
> must resolve to a regular file inside the template root; absolute paths,
> parent traversal, and symlinks are rejected. Put shared partials outside the
> source subdirectory so they are not emitted as their own output files.
> Includes are not available in field strings, expressions, target path
> segments, apply messages, or hook fields. Jinja `import` and `extends` remain
> unavailable. Toha's `!include` in `template.yml` is a separate feature for
> loading support data files; it is unrelated to Jinja's `include` statement.

### `docs/specifications/template-format.yml` (revise the Jinja note at line 44)

Current: `... import, include, and extends are not available.`

Proposed: state that `{% include %}` is available on rendered file bodies only,
resolved relative to the template root, literal-target-only, confined to the
template root (absolute, `..`, and symlink rejected), with partials outside the
source subdirectory excluded from output; `import` and `extends` remain
unavailable; the YAML `!include` tag (lines 20–23) is unchanged and separate.

### `docs/specifications/template-format.schema.yml`

**No change.** Includes live in rendered file *text*, not in the `template.yml`
structure the JSON Schema validates. This is a deliberate interface-depth point:
the feature adds capability without widening the config contract.

### `Cargo.toml`

Add the `unstable_machinery` feature to the existing `minijinja` dependency so
`literal_include_targets` can reuse the vendor parser instead of a hand-rolled
scanner. Flagged as a risk (unstable API, no semver guarantee) in the rationale.

---

# 6. Fixtures to add (positive + negative)

Following the `tests/fixtures/<name>/` shape (`template/`, `answers.json`,
`expect.yml`, optional `expected/`).

**Positive**

- `include-basic` — Call site A. `partials/frontmatter.md` outside the source
  dir; `template/{{ slug }}.md` includes it. `expect.yml: exit 0`; `expected/`
  has the composed file and no `partials/`.
- `include-nested` — Call site C. `index.html` → `page` → `head` → `meta`.
  Asserts transitive composition and that partials are not emitted.
- `include-files-source` — Call site B. A `files:` rule whose source includes
  `parts/license.txt`. Exercises load-time compile + `read_render` agreement.
- `include-partial-under-source-ignored` — a partial living *inside* the source
  dir, excluded by an `ignore` glob; confirms it is included but not emitted.

**Negative** (each `expect.yml: exit 1` with `error_contains`)

- `err-jinja-include-missing` — `{% include "partials/nope.md" %}` →
  `["jinja include not found"]`.
- `err-jinja-include-escape` — `{% include "../outside.md" %}` with an
  `outside.md` beside the template → `["jinja include escapes"]`.
- `err-jinja-include-symlink` — `partials/evil.md` is a symlink to a file
  outside the root → `["jinja include path is a symlink"]` (covers the
  symlink-inside-root-pointing-out vector `safe_join` would miss).
- `err-jinja-include-cycle` — `a.md` includes `b.md` includes `a.md` →
  `["jinja include cycle"]`.
- `err-jinja-include-dynamic` — `{% include chosen %}` → `["must be a literal
  string"]`.
- `err-jinja-include-reference` — a `files:` source whose partial references an
  id defined by no earlier node → `["is not defined by an earlier node"]`
  (proves closure reference validation at load).

These exercise all three escape vectors (absolute/`..`, canonical-target escape,
symlink), plus missing, cycle, dynamic, and load-time reference validation —
through the caller interface (the CLI/fixture runner), not internal APIs.
