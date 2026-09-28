# Final design — Jinja `{% include %}` within the template root

Synthesized design for Toha's second release. Design only:
signatures with `not implemented` bodies, proposed (not applied) contract edits,
described fixtures. No production code, spec, or schema is edited here. Base
evidence is verified against `epic/second-release` at
`4738042a766dc7b2d1d3c76a9c83c41c03e803d2`.

Base candidate: C2 (capability-by-type spine). Selected graft: C1's
compile-time confined closure (precise diagnostics + load-time validation). See
`03-arena-scoring.md` for scores, defects, grafts, and rejections.

## 1. What an author gets (caller's view)

An author factors shared text out of a rendered file into a **partial** and
pulls it in:

```jinja
{% include "partials/frontmatter.md" %}
# {{ title }}
```

Rules the author must know:

- The include name is a `/`-separated path **relative to the template root**.
  It can name a root-level file (`"notice.txt"`), a file in any nested directory
  (`"shared/legal/license.inc"`), or a file with any extension. It is the same
  string in every file that includes it; it is **not** resolved relative to the
  including file.
- The accepted target grammar is a string literal or an ordered literal list of
  strings: `{% include "notice.txt" %}` or
  `{% include ["preferred.txt", "fallback.txt"] %}`. Computed names and lists
  containing a computed or non-string item are rejected. `with context` and
  `without context` are accepted MiniJinja syntax aliases; both use the same
  variables because MiniJinja gives the marker no separate behavior.
- A literal list selects the first candidate that exists, in author order.
  Missing earlier candidates are fallbacks. If no candidate exists, the include
  fails with Toha's Jinja-include diagnostic unless `ignore missing` is present;
  with it, the include emits nothing. An empty literal list also emits nothing,
  matching MiniJinja. `ignore missing` suppresses only the direct all-not-found
  result: escape, symlink, unreadable, non-UTF-8, syntax, nested-include, and
  cycle errors still fail.
- Include targets may live **anywhere inside the template root**. There is no
  required `partials/` prefix or directory allowlist; `partials/` is only an
  authoring convention. Targets placed **outside the source subdirectory** are
  pulled in but
  **never emitted** as their own output files. A partial placed *inside* the
  source subdirectory is emitted like any other source file unless an `ignore`
  glob excludes it.
- A partial sees the **same variables** (answers, `data`, computed values) as
  the file that includes it — one flat global namespace.
- Includes work in **rendered file bodies only**: source-tree files and `files:`
  rule sources. They do nothing in prompts, defaults, `when`/`computed`/`format`/
  `options` expressions, target path segments, apply messages, or hook fields.
  If a Jinja multi-template statement appears on one of those loaderless
  surfaces, compilation reports that it is unavailable outside file bodies.
- **`template.yml` gets no include processing, and neither does any document
  loaded through its YAML `!include` tag.** Those are configuration: Toha parses
  them as structured data, and even the individual string fields it later
  compiles (a prompt, a default, a `files:` `path:`) reject multi-template
  statements — a `{% include %}` written there is not a partial. A partial is
  authored in a file body, not in configuration.
- The include name may be **any file inside the template root**, read as raw
  text. Its role — configuration vs body — is set by *how it is referenced*, not
  by its extension: a generated `.yml`/`.yaml` file in the source tree (or named
  as a `files:` `source:`) is a **file body** and does get includes. There is no
  extension-based rule; the confinement seam is the only gate on a target.
- Absolute names, `..` traversal, backslash separators, and symlinked partials
  are refused.
- `block`, `import`, `from ... import`, and `extends` remain unavailable in file
  bodies. The YAML `!include` tag in `template.yml` is a separate feature and is
  unchanged.

### Call site A — shared front-matter partial (not emitted)

```text
weekly-note/
├── template.yml           # source: template   (default)
├── partials/
│   └── frontmatter.md      # outside source dir → never emitted
└── template/
    └── {{ slug }}.md       # {% include "partials/frontmatter.md" %}
```

`answers = {title: "Weekly Notes", slug: "weekly-notes", status: "draft"}`
renders one file, `weekly-notes.md`, with the front-matter inlined. `partials/`
is outside the source subdirectory, so `walk` never visits it.

### Call site B — a `files:` rule source includes a license header

```yaml
files:
  - each: modules
    source: parts/module.rs.j2      # confined_file path, as today
    path: "src/{{ item | snake }}.rs"
```

`parts/module.rs.j2` contains `{% include "parts/license.txt" %}`. Because a
`files:` source is compiled when `template.yml` loads, a missing
`parts/license.txt` — or a variable reached only through it —
is reported **before any interview runs**.

### Call site C — nested partial chain

`template/index.html` → `partials/page.html` → `partials/head.html` →
`partials/meta.html`. Nesting resolves transitively; every name is root-relative,
so a partial names the next by the same path any file would use. A cycle
anywhere in the chain is reported by name at compile time.

### Call site D — root-level and non-`partials/` targets

`template/report.txt` includes `notice.txt` at the template root, then
`shared/legal/license.inc`. Both resolve because each is a confined
root-relative file. Neither path starts with `partials/`, and neither needs a
recognized extension.

## 2. Module & seam map

One new value object owns the single confinement seam. `Template::load` creates
it once and retains it for both file-body paths. A `files:` rule owns its
compiled body from load onward; a walked source-tree file uses the same value
object when it is discovered during planning. Everything else stays loaderless.

```text
Template::load (template.rs)
  └─ root = folder.canonicalize()                         [existing]
  └─ source_dir derived, confined                         [existing]
  └─ partials = Partials::rooted(&root)                    ← owned by Template
  └─ Builder { partials: &partials, ... }
       └─ files: source read once
            └─ partials.compile(text, root-relative source label)
                 ├─ reject block/import/from/extends in the complete AST
                 ├─ build confined literal include closure
                 └─ Builder::refs(body.references(), "files[i].source", each)
            └─ FileRule { body: FileTmpl, ... }             ← survives interview
Plan::build (plan.rs)
  ├─ walk() source-tree file  ─▶ read_render(path, label, ctx, &template.partials)
  │                                └─ partials.compile(text, label)? .render(ctx)
  ├─ files: rule source        ─▶ rule.body.render(ctx)      ← no read/recompile
  │                                          │
  │                       ┌──────────────────┴───────────────────┐
  │                       │   the ONE confinement seam            │
  │                       │   (IncludeRoot::locate / confined_loader)
  │                       │   reject absolute/`..`/`\`; refuse     │
  │                       │   symlink at any component;             │
  │                       │   canonicalize; starts_with(root); read │
  │                       └───────────────────────────────────────┘
  ├─ target path segments      ─▶ rendered() ─▶ guarded Tmpl         includes refused
  ├─ apply messages, hooks     ─▶ guarded Tmpl / Expr                includes refused
interview.rs / template.rs field compiles ─▶ guarded Tmpl / Expr    includes refused
template.rs !include resolve() ─▶ SEPARATE load-time data feature, own diagnostics
```

Includes are reachable **iff** a caller holds `FileTmpl`, and only
`Partials::compile` mints one. `Template` owns `Partials`; `FileRule` owns the
load-time `FileTmpl`. Surface scope and load-before-interview behavior are
ownership invariants, not guards that a driver can forget.

## 3. Public interfaces (signatures)

New types live in `src/jinja.rs` beside `Tmpl`. `Expr` is unchanged. `Tmpl`
keeps its public API and loaderless environment, and adds an AST guard rejecting
all multi-template statements. This guard is required because the dependency
feature is crate-wide and the loaderless environment still registers its current
template under the name `"value"`. `FileTmpl` is the only mode that admits
`include`.

```rust
/// The include boundary for one loaded template. A cheap, cloneable handle to
/// the *selected template root* (the canonical `Template::root`, derived from
/// template resolution's `ResolvedTemplate.folder`). It consumes that identity;
/// it defines no new one. It is the ONE seam every include name passes through.
///
/// Invariant: the wrapped root is already canonical
/// (`Template::load` canonicalizes it).
#[derive(Clone, Debug)]
pub struct Partials { root: std::sync::Arc<std::path::PathBuf> }

impl Partials {
    /// Build from the canonical template root. Does not re-canonicalize.
    pub fn rooted(root: &std::path::Path) -> Self { not_implemented!() }

    /// Compile a file body into an include-capable template. `label` is the
    /// file's root-relative path, used only in diagnostics. See §5 for the
    /// selected static-closure resolution mode.
    pub fn compile(
        &self,
        source: String,
        label: &str,
    ) -> Result<FileTmpl, IncludeError> {
        not_implemented!()
    }
}

/// A compiled file body that MAY resolve `{% include %}` against its root.
/// A distinct type from `Tmpl`: holding one IS the include capability.
pub struct FileTmpl { /* source, env, referenced_ids */ }

// Required by FileRule's existing Debug derivation; mirror Tmpl's manual Debug
// implementation and expose only the source field.
impl std::fmt::Debug for FileTmpl { /* same shape as Tmpl */ }

impl FileTmpl {
    pub fn source(&self) -> &str { not_implemented!() }
    /// References including those reached through partials — see §5.
    pub fn references(&self) -> &std::collections::HashSet<String> {
        not_implemented!()
    }
    pub fn render<S: serde::Serialize>(
        &self,
        ctx: S,
    ) -> Result<String, RenderError> {
        not_implemented!()
    }
}

// Existing owners gain the capability/body fields.
pub struct Template {
    // existing fields ...
    pub partials: Partials,
    pub files: Vec<FileRule>,
}

pub struct FileRule {
    pub each: Each,
    pub source: std::path::PathBuf, // root-relative diagnostic/origin path
    pub body: FileTmpl,             // compiled and validated by Template::load
    pub path: Tmpl,
    pub when: Option<Expr>,
}
```

Load-time and plan-time call-site deltas:

```rust
struct Builder<'a> {
    // existing fields ...
    partials: &'a Partials,
}

impl Builder<'_> {
    /// Compile a `files:` body at load. `field` remains `files[i].source` for
    /// LoadError attribution; `label` is its root-relative path for include
    /// diagnostics and cycle paths. Called inside `bound(Some(&each), ...)`, so
    /// the existing reference check sees the local binding.
    fn file_tmpl(
        &mut self,
        source: String,
        label: &str,
        field: &str,
    ) -> Option<FileTmpl> {
        let body = self.partials.compile(source, label)
            .map_err(|error| problem(&mut self.problems, field, error.to_string()))
            .ok()?;
        self.refs(body.references(), field, &[]);
        Some(body)
    }
}

// Template::load creates `partials` immediately after canonicalizing `root`,
// lends it to Builder, stores each successful `file_tmpl` on FileRule, then
// moves `partials` into the returned Template. All of this finishes before any
// driver can call Interview::start or replay. A files: body and its selected
// partial closure are one immutable load snapshot; planning never resolves the
// live files independently.

// Only walked source-tree bodies compile during planning. The caller derives
// `label` from `path.strip_prefix(&template.root)` and normalizes `/` separators.
// `rendered()` (path segments) does NOT gain these parameters.
fn read_render(
    path: &Path,
    label: &str,
    ctx: &impl serde::Serialize,
    partials: &Partials,
)
    -> Result<String, PlanError> { not_implemented!() }

// The `files:` loop renders the load-owned body.
let content = Content::Rendered(
    rule.body.render(&local).map_err(|error| PlanError::Render {
        path: template.root.join(&rule.source),
        message: error.to_string(),
    })?,
);
```

At load, `IncludeError` becomes the existing `Problem` at `files[i].source`; at
plan, it becomes `PlanError::Render` at the walked source path. Render errors for
a stored `FileRule::body` retain the rule's root-relative source as their
origin. No new error variant leaks to the CLI or crate boundary.

## 4. Data shapes

```rust
/// Distinct wording from the YAML `!include` diagnostics (hard requirement).
/// Every message says "jinja include" (or "partial"); none reuses the
/// `!include` strings ("include escapes template root", "include cycle",
/// "include file not found", "unsupported include extension").
pub enum IncludeError {
    // Syntax error, unchanged wording.
    Template(minijinja::Error),
    // One candidate: "jinja include not found: {name}, included by {by}"
    // Multiple: "jinja include candidates not found: {names}, included by {by}"
    NotFound { candidates: Vec<String>, by: String },
    // "jinja include escapes template root: {name}, ..."
    Escape { name: String, by: String },
    // "jinja include path is a symlink: {name}, ..."
    Symlink { name: String, by: String },
    // "jinja include is not utf-8: {name}, ..."
    NotUtf8 { name: String, by: String },
    // "jinja include is not readable: {name}, included by {by}: {io}"
    Unreadable { name: String, by: String, source: std::io::Error },
    // "jinja include cycle: a.md -> b.md -> a.md"
    Cycle { path: String },
    // "jinja include target must be a literal string or literal list
    // of strings, ..."
    Dynamic { by: String },
    // "jinja {statement} is unavailable in {surface}, found in {by}"
    UnsupportedStatement {
        statement: &'static str,
        surface: &'static str,
        by: String,
    },
}
```

The selected static-closure mode adds one private structure, the confined
closure (root-relative name → confined UTF-8 source, plus the unioned
`referenced_ids`), built by a static walk of the literal include graph with an
explicit canonical-path stack — the same shape as `template.rs::resolve` for
`!include`. No `template.yml` schema shape changes: partials are ordinary files
plus inline `{% include %}`.

One private recursive AST walker has two explicit modes:

- `LoaderlessField`, called by `Tmpl::compile`, rejects `Include`, `Block`,
  `Import`, `FromImport`, and `Extends` anywhere in the AST. The public
  `Tmpl::compile` signature remains `Result<Tmpl, minijinja::Error>`; the guard
  returns a syntax error such as `jinja include is unavailable outside file
  bodies` with MiniJinja's filename/span attribution.
- `FileBody`, called by `Partials::compile`, admits `Include` and rejects the
  four sibling statements as unavailable in file bodies.

`Partials::compile` runs one AST inspection parse per body with
`minijinja::machinery::parse`; the `FileBody` walk then performs two jobs before
that body is registered in the render environment:

1. It rejects every `Stmt::Block`, `Stmt::Import`, `Stmt::FromImport`, and
   `Stmt::Extends`, including those nested in an unreachable branch, macro,
   loop, or capture. It recurses through every statement-body vector, following
   the same exhaustiveness shape as MiniJinja's own AST visitor.
2. For each `Stmt::Include`, it accepts only `Expr::Const(String)` or
   `Expr::List` containing only string constants. It keeps author order and the
   `ignore_missing` flag in an `IncludeSpec` used by the closure walk.

For one `IncludeSpec`, the closure checks candidates in order. A missing path is
recorded and advances to the next candidate. The first candidate that resolves
to a confined, non-symlink, regular UTF-8 file is selected, registered under the
author's exact string, and walked transitively; later candidates are not read.
An existing candidate that escapes, is a symlink, unreadable, invalid UTF-8, or
contains an invalid nested body fails immediately rather than falling back. If
all candidates are missing, `ignore_missing` emits no edge and succeeds; without
it, `NotFound` contains the ordered candidates. An empty literal list emits no
edge in either form, matching MiniJinja's empty sequence behavior.

## 5. The decisions this design resolves

1. **Supported surfaces — rendered file bodies only.** Source-tree files and
   `files:` rule sources. Excluded, each with
   reason:
   - **Configuration documents — `template.yml` and every document loaded
     through its YAML `!include` tag.** These are never a render surface. Toha
     parses `template.yml` as data (`serde_norway::from_str`, `template.rs:734`)
     and expands `!include` by deserializing the referenced file by extension and
     splicing the parsed value into the config tree (`template.rs:305`–`327`) —
     no Jinja on that path. The include capability is a property of a render
     surface, so it cannot reach configuration by construction. A field's origin
     (inline or `!include`-spliced) does not matter; both are data.
   - **Interview fields and configuration string values** —
     prompts/defaults/`when`/`computed`/`format`/`options`/`regex`, `files:`
     `path:`, apply-message and hook strings. Toha does compile these to
     templates, but through `Tmpl`/`Expr` (`template.rs:399`,
     `template.rs:421`; `jinja.rs:95`/`128`). `Tmpl` is loaderless and its AST
     guard rejects every multi-template statement before compiling the internal
     `"value"` template; `Expr` cannot contain statements. The interview engine
     also must stay a pure, filesystem-free
     function across the headless/staged/crate drivers; a loader there breaks
     that. Apply messages are excluded to keep the file-body rule crisp.
   - **Target path segments** (`plan.rs:168`) — a segment is a filename fragment;
     injecting multi-line partial text is nonsensical and a path/newline hazard.
   - **Hook fields** — feed process execution; an unaudited injection surface.

   Enforced **structurally**: `Template::load` stores an include-capable
   `FileTmpl` only on each `FileRule`; the source-tree `read_render` receives
   `&template.partials`; every other site holds guarded `Tmpl`/`Expr`, and
   configuration documents are pure data before any template exists. The two
   AST modes prevent the crate-wide feature from turning an internal template
   name into a capability. This is a type/role invariant.

   **Target selection has no prefix or extension allowlist.** An include name resolves,
   through the one confinement seam, to any real file inside the template root,
   read as raw UTF-8 text and inlined. Naming a configuration document
   (`template.yml`, an `!include`-loaded fragment) as a partial target just
   inlines its literal bytes into the body — it does **not** parse or "process"
   that document, so the invariant above (configuration receives no include
   processing) still holds. A blanket extension ban is deliberately **not**
   introduced: it would wrongly block a legitimate `.yml`/`.yaml` partial shared
   into a generated YAML body, and it would confuse role with extension. The
   confinement seam (in-root, `/` separators, no `..`/absolute/symlink) is the
   only target gate.
   Root-level names and nested names outside `partials/` are equally valid.
2. **Include root — the whole template root.** Confinement/resolution root =
   `template.root`, the identity selected-template resolution already
   establishes and canonicalizes; `Partials::rooted` consumes it. Chosen over
   `source_dir` so a
   partial can live as support material outside `source_dir` and never be
   emitted — no `ignore` ceremony. `ignore`/`static` are untouched and govern
   emission only; `static` files are still copied byte-for-byte and never
   rendered.
3. **Relative / nested semantics — root-relative, verbatim.** MiniJinja looks
   names up verbatim (`vm/mod.rs:840`), so a fixed root needs no rewriting: the
   author's string is the lookup key. Nested includes are trivial and
   unambiguous. Relative-to-includer is rejected (needs per-includer name
   rewriting; the arena's attempt used a non-`Sync` stateful loader).
4. **Literal grammar, fallback, and missing include.** A string literal and an
   ordered literal list of strings are accepted; computed values and mixed lists
   are rejected. A list selects the first confined candidate that exists.
   Single-name failure uses `jinja include not found: {name}, included by {by}`;
   multiple-name failure uses `jinja include candidates not found: {names},
   included by {by}`. `ignore missing` converts only the all-candidates-missing
   result to empty output. It does not suppress confinement, file, parse, nested,
   or cycle failures. `with context` and `without context` are accepted no-op
   syntax markers, matching MiniJinja; every partial sees the includer's context.
5. **Recursion / cycle** — the static closure detects re-entry and reports the
   root-relative cycle path before rendering.
6. **Load-time `files:` ownership and reference validation.** `Template::load`
   creates and owns `Partials`; `Builder::file_tmpl` compiles each `files:` body
   inside the existing `each` binding scope, reports errors at
   `files[i].source`, unions references across its selected literal include
   graph, and stores the `FileTmpl` on `FileRule`. Every driver completes this
   before starting or replaying an interview. Source-tree bodies compile later
   because `walk` discovers them at plan time. The stored body and its selected
   partial closure are the exact bytes read at load; a file mutation during an
   interview cannot split validation from rendering. A resume loads a fresh
   `Template` from the selected template identity, as it does today.
7. **Only include from MiniJinja's multi-template grammar.** Both
   `multi_template` and `unstable_machinery` are required. The capability
   boundary rejects `block`, `import`, `from ... import`, and `extends` across
   the complete AST before registering the closure, so enabling the vendor
   feature cannot expose sibling capabilities through an already registered
   partial.

## 6. Approved choices

- **Static compile-time confined closure.** Include targets are literal strings
  or ordered literal lists of strings;
  the transitive file set is resolved, confined, read, and pre-registered as
  in-memory named templates at compile time. No loader is installed on the
  render environment, so rendering performs no filesystem access. This provides
  named cycle paths, author-time missing/escape/symlink errors, and load-time
  reference validation. Dynamic include expressions remain unsupported. The
  implementation enables MiniJinja's `multi_template` feature for include
  parsing/execution and `unstable_machinery` to inspect the AST; the latter API
  has no semver guarantee. The AST walk rejects the other `multi_template`
  statements before closure registration.
- **Apply messages remain loaderless.** Jinja includes are available only to
  rendered file bodies. The loaderless `Tmpl` AST mode rejects every
  multi-template statement before compilation.
- **Every symlink in an include path is refused.** This matches the source walk's
  refusal at `plan.rs:368` and keeps one safe rule for every include depth.

These choices introduce no timeout, permissions/access change, pinned runtime
version check, or subprocess integration (see §9 disclosure).

## 7. Proposed contract edits (described, NOT applied — owned by implementation)

- **`docs/template-jinja.md`** — replace the "Supported Jinja features" closing
  sentence ("Jinja `import`, `include`, and `extends` are unavailable …") with a
  statement that `{% include %}` is available in rendered **file bodies only**,
  root-relative literal names, confined to the template root (absolute/`..`/
  symlink refused), partials outside the source subdirectory not emitted,
  partials share the includer's variables, ordered literal candidate lists and
  `ignore missing` follow §5, `block`/`import`/`from ... import`/`extends` remain
  unavailable, and the YAML `!include` tag is separate. **State plainly that `template.yml`
  and documents loaded through its YAML `!include` receive no Jinja include
  processing** (they are configuration parsed as data), and that a file's role —
  body vs configuration — follows how it is referenced, not its extension (a
  generated `.yml`/`.yaml` body still gets includes). Add a short "Partials"
  subsection with Call site A.
- **`docs/specifications/template-format.yml`** — revise the "Jinja evaluation"
  paragraph to state `{% include %}` availability on file bodies, root-relative,
  confined (mirrors `!include`/file-path confinement), literal-only with ordered
  literal-list fallback and `ignore missing`, separate diagnostics from the YAML
  `!include` tag (whose paragraph is unchanged), and to state that
  configuration documents (`template.yml` and its YAML `!include` targets) are
  parsed as data and are not a Jinja render surface.
- **`docs/specifications/template-format.schema.yml`** — **no change** (includes
  add no `template.yml` keys). Deliberate interface-depth point: capability added
  without widening the config contract.
- **`Cargo.toml`** — add both `multi_template` and `unstable_machinery` to the
  existing `minijinja` feature list. `default-features = false` remains.

## 8. Behaviors to prove (falsifiable) + sole-kill guidance

Exercised through the fixture harness (the caller interface), following the
`tests/fixtures/<name>/` shape, plus one runtime-constructed symlink test.

**Positive** (`exit: 0`, `expected/` tree):

- `include-basic` (Call site A): partial outside source is inlined, variable
  flows in, `partials/` is **not** emitted.
- `include-nested` (Call site C): transitive composition; partials not emitted.
- `include-root-level`: a file body includes root-level `notice.txt`. Proves a
  target needs no directory prefix.
- `include-nested-outside-partials`: a file body includes
  `shared/legal/license.inc`. Proves nested targets outside `partials/` and
  arbitrary extensions are accepted.
- `include-files-source` (Call site B): `files:` rule source includes a partial.
- `include-list-first-existing`: both literal candidates exist; only the first is
  rendered. Proves author-order selection.
- `include-list-fallback`: the first literal candidate is absent and the second
  exists; the second is rendered. Proves a missing earlier name is fallback, not
  unconditional load failure.
- `include-ignore-missing`: a missing literal target with `ignore missing`
  renders empty text. A companion case uses an all-missing literal list.
- `include-empty-list`: an empty literal list renders empty text, matching
  MiniJinja's sequence behavior.
- `include-partial-under-source-ignored`: a partial inside `source_dir` excluded
  by `ignore` is included but not emitted.
- `include-yaml-body`: a **generated `.yaml` file body** (a `.yaml` source-tree
  file) that `{% include %}`s a partial resolves it and emits rendered YAML.
  Proves the capability is by role, not extension — there is no YAML ban. A
  companion `.yaml` partial fragment is inlined into it.

**Negative** (`exit: 1`, `error_contains`), each with distinct wording:

- `err-jinja-include-missing` → `["jinja include not found"]`.
- `err-jinja-include-escape` (`../outside` and an absolute variant) →
  `["jinja include escapes"]`.
- `err-jinja-include-backslash` → `["jinja include escapes"]`; the template
  namespace always uses `/`, independent of host path syntax.
- `err-jinja-include-symlink` (a symlink **inside** the root pointing outside —
  the vector `safe_join` misses) → `["jinja include path is a symlink"]`.
- `err-jinja-include-cycle` → `["jinja include cycle"]`; assert the named path
  `a -> b -> a`.
- `err-jinja-include-dynamic` (`{% include chosen %}`) →
  `["must be a literal string"]`.
- `err-jinja-include-list-dynamic`: a literal list containing a computed or
  non-string item → `["must be a literal string or literal list of strings"]`.
- `err-jinja-include-candidates-missing`: every item in a non-ignored literal
  list is absent → `["jinja include candidates not found"]` with names in author
  order.
- `err-jinja-include-ignore-escape`: `ignore missing` plus `../outside` still
  fails with `["jinja include escapes"]`; the modifier is not a broad catch.
- `err-jinja-block`, `err-jinja-import`, `err-jinja-from-import`, and
  `err-jinja-extends`: each statement appears in a file body. The three
  name-bearing forms point to an existing, otherwise valid in-root template, so
  they would work if the registered closure leaked to the sibling capability.
  Each fails with `jinja {statement} is unavailable in file bodies`.

**Direct load-order tests** (call the library boundary and stop there):

- `files_include_missing_fails_template_load` constructs a template whose
  `files:` body includes an absent partial, calls only `Template::load`, and
  asserts a `LoadError` problem at `files[0].source` naming the body and target.
- `files_include_reference_fails_template_load` constructs a `files:` body whose
  selected partial references an id that is neither earlier nor its `each`
  binding, calls only `Template::load`, and asserts
  `id is not defined by an earlier node`. Neither test constructs an
  `Interview` or calls `Plan::build`.
- `files_body_snapshot_is_stable_after_load` loads a `files:` body and its
  partial, changes both live files, then completes and plans with the already
  loaded `Template`; output comes from the load-owned `FileTmpl` snapshot.

**Inertness guards** (prove the seam holds by *type*/*role*, not by luck):

- A source **file name** `{% include "x" %}.txt` and an **apply message** /
  **interview default** each containing `{% include %}` fails with
  `jinja include is unavailable outside file bodies`, proving the guarded
  loaderless `Tmpl` path cannot acquire the capability.
- `include-config-field-inert`: a `template.yml` **string field** (e.g. a
  `messages.after-apply` or a `default`) that contains `{% include
  "partials/x.md" %}` where `partials/x.md` **exists in the root**. It still does
  **not** resolve the partial — it errors with `jinja include is unavailable
  outside file bodies`, never a root lookup. Existence of the
  target is the sole-kill point: if the field ever gained the include capability,
  the partial would resolve and this fixture would emit inlined text instead of
  erroring. Proves configuration is not a render surface.
- `include-yaml-include-field-inert`: the same `{% include %}` string is
  delivered into a `template.yml` field **through a YAML `!include`-loaded
  fragment** (e.g. `default: !include frag.yml` whose value is the include
  string). It behaves identically to the inline case — `!include` composes data,
  so the field is still a loaderless template. Proves `!include`-loaded
  configuration receives no Jinja include processing, exactly as inline
  configuration does.
- `include-loaderless-self-name-inert`: a guarded `Tmpl` body uses
  `{% include "value" %}` and is rejected as unavailable outside file bodies.
  This catches reliance on no-loader alone, because `Tmpl` registers itself
  under that internal name.
- `err-jinja-import-loaderless-self`: a guarded `Tmpl` attempts
  `{% import "value" as helpers %}` and is rejected. The existing template name
  makes this a sole-kill for the loaderless sibling-statement guard.

**Distinct-diagnostics guard:** assert no Jinja-include message equals a YAML
`!include` message; the existing `err-include-cycle`/`err-include-escape`
fixtures still pass unchanged.

**Sole-kill agreements the implementer must prove (per task-execution):**

- The include **name↔lookup-key agreement** (author string = registered/looked-up
  key). Mutate the key derivation; a named positive fixture must fail.
- The MiniJinja **feature agreement** is the pair `multi_template` +
  `unstable_machinery` with defaults still disabled. Remove `multi_template`;
  `include-basic` must cease to build or pass. Remove `unstable_machinery`; the
  exhaustive AST boundary must cease to build. The repository gate records the
  named feature pair from `cargo tree -e features -i minijinja`.
- Remove each `Block`/`Import`/`FromImport`/`Extends` match arm independently;
  its matching `err-jinja-*` fixture must fail by name while the others remain
  green. Place one sibling statement in an unreachable nested branch; a
  top-level-only walker must fail that named case.
- Remove the `LoaderlessField` mode's `Include` arm;
  `include-loaderless-self-name-inert` must fail. Remove its `Import` arm;
  `err-jinja-import-loaderless-self` must fail. The in-root-target inertness
  fixtures remain the surface-policy guards; the self-name cases close the
  internal-environment bypass.
- Bypass `Builder::file_tmpl` and restore the loaderless `b.tmpl` validation for
  `files:` sources; both `files_include_missing_fails_template_load` and
  `files_include_reference_fails_template_load` must fail by name. Rendering the
  same body later in `Plan::build` is not evidence for this agreement.
- Restore an independent `read_render(&origin, ...)` in the `files:` planning
  loop; `files_body_snapshot_is_stable_after_load` must fail by name. Validation
  and rendering must consume the same `FileTmpl` snapshot.
- Change candidate traversal to fail on the first missing name;
  `include-list-fallback` must fail. Reverse the order;
  `include-list-first-existing` must fail. Treat `ignore_missing` as false;
  `include-ignore-missing` must fail. Make it suppress every error;
  `err-jinja-include-ignore-escape` must fail.
- The **confinement seam**: invert `starts_with(root)`, drop the symlink refusal,
  and drop the absolute/`..` rejection each in turn; the matching negative
  fixture must fail by name. The symlink test must hold a symlink **inside** the
  root (the exhaustiveness shape), not a top-level one.
- Drop the backslash rejection; `err-jinja-include-backslash` must fail by name.
- The **distinct-wording claim**: mutate a Jinja-include message to the
  `!include` wording; the distinct-diagnostics guard must fail.
- The **surface-inertness claim** (a prose claim — sweep it): mutate a
  loaderless site to use `Partials::compile`; an inertness guard must fail. Do
  this for a body-adjacent site (path segment / apply message) **and** for a
  configuration field: giving `Builder::tmpl` (`template.rs:399`)
  the include capability must fail `include-config-field-inert` (and, since
  `!include` feeds the same field compilation, `include-yaml-include-field-inert`
  by the same mutation) — proving configuration exclusion is enforced, not
  incidental.
- The **role-not-extension claim**: `include-yaml-body` must pass unchanged; a
  mutation that filters partial targets or bodies by `.yml`/`.yaml` extension
  must fail it — proving no blanket YAML ban crept in.
- The **no-prefix-allowlist claim**: add a `partials/`-only target restriction;
  `include-root-level` and `include-nested-outside-partials` must each fail by
  name. This is the sole-kill against treating the example directory as policy.
- Sever the reference **union**; `files_include_reference_fails_template_load`
  must fail.

## 9. Out of scope / compatibility / disclosure

**Out of scope.** `block`, `import`, `from ... import`, `extends`; dynamic include
targets and include lists containing dynamic or non-string items;
**Jinja include processing of `template.yml` and any document loaded through its
YAML `!include` tag** (configuration is parsed as data, never rendered); includes
on interview fields, expressions, path segments, hook fields, and apply
messages; any change to YAML `!include`, `static`, `ignore`, or
path/target confinement; any `template.yml` schema key; any extension-based rule
on which files may be bodies or partials.

**Compatibility.** Templates with no `{% include %}` render identically
(closure/loader is inert). Pure interview logic and all
terminal/headless/staged/direct/crate paths are preserved — the include
capability lives only in file-body values, never in the interview engine.
Staged-interview replay is unaffected: `Template::load` reconstructs
`Partials` and every `files:` body from the selected root before replay; no
include state is serialized.

**Access-policy disclosure (separate, per task).** This design grants **no** new
filesystem reach: includes resolve **only** inside the selected template root,
which is already Toha's confinement boundary for source files, `files:` rule
sources, YAML `!include`, and hook scripts.
There is **no** new timeout mechanic, **no** permissions/access change, **no**
pinned runtime version check in non-test code, and **no** application subprocess
integration. The approved static closure enables the `minijinja`
`multi_template` build feature and the `unstable_machinery` build feature (a
no-semver-guarantee crate API). The AST gate admits only `include` from the
multi-template grammar. These are dependency/maintenance risks, not operator
capability changes.

## 10. Verification (Phase F)

Checked against `minijinja` 2.24.0 source and the Toha tree:

- Configuration is data, not a render surface: `Template::load` parses
  `template.yml` with `serde_norway` (`template.rs:734`) and `resolve` expands
  YAML `!include` by deserializing referenced files and splicing parsed values
  (`template.rs:305`–`327`) — no Jinja on that path. Config string fields compile
  through `Tmpl`/`Expr` (`template.rs:399`/`421`, `jinja.rs:95`/`128`). The
  crate-wide feature makes statement grammar visible to `Tmpl`, whose
  `LoaderlessField` AST mode rejects it before the internal `"value"` template
  is compiled. So "includes only in file bodies" holds by construction, and it holds
  identically for inline and `!include`-loaded configuration. Confirmed against
  the existing `tests/fixtures/includes` (`data: !include …`) shape.
- `Template::load` currently reads each `files:` source and compiles it through
  `Builder::tmpl` only for reference validation (`template.rs:842`–`846`), then
  discards that compiled value; `Plan::build` rereads it at `plan.rs:221`.
  Drivers complete `Template::load` before `Interview::start` or replay
  (`main.rs:830`–`840`, `901`–`909`). The declared `Template.partials` and
  `FileRule.body` owners replace that discard/recompile split and make
  pre-interview closure/reference failure implementable.
- Confinement precedent reused exactly: `template.rs::resolve` (canonicalize +
  `starts_with` + cycle stack), `confined_file` (`template.rs:452`),
  `has_symlink_component` (`plan.rs`) — the design adds nothing novel to the
  security model, only a new caller.
- `safe_join`/`path_loader` do NOT canonicalize (`loader.rs:180`), so the custom
  seam is necessary to close the in-root-symlink vector. Confirmed.
- `set_loader` requires `Send + Sync + 'static` (`environment.rs:234`). The
  chosen static closure installs no loader; the rejected relative-to-includer
  `RefCell` loader would not satisfy that boundary.
- With Toha's `default-features = false`, neither required feature is present in
  the current feature tree. `unstable_machinery` exposes the parser/AST
  (`lib.rs:253`–`267`) but does not enable `Include`; `multi_template` separately
  gates the `Include`, `Block`, `Import`, `FromImport`, and `Extends` AST/parser
  arms (`ast.rs:67`–`76`, `parser.rs:865`–`874`) and include execution
  (`vm/mod.rs:830`). The contract therefore names both features and rejects the
  sibling variants before registration.
- `ast::Include` carries `name: Expr` and `ignore_missing` (`ast.rs:357`–`364`).
  MiniJinja tries sequence candidates in order, advances only on
  `TemplateNotFound`, and suppresses only the exhausted not-found result when
  `ignore_missing` is true (`vm/mod.rs:838`–`915`). The selected static behavior
  preserves that order and failure boundary while limiting candidate
  expressions to literal strings.
- Every requirement and constraint maps to a section here; failure cases and
  falsifiable fixtures are enumerated in §8. The selected-template
  `ResolvedTemplate` root is consumed, not redefined. Includes remain inert text
  and do not intersect with executable trust.
