# Final design — Jinja `{% include %}` within the template root (task 1076)

Synthesized design for epic 1065 / paired implementation 1061. Design only:
signatures with `not implemented` bodies, proposed (not applied) contract edits,
described fixtures. No production code, spec, or schema is edited here. Base
`epic/second-release` at `cfab3286`.

Base candidate: C2 (capability-by-type spine). Recommended graft: C1's
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

- The include name is a path **relative to the template root**, `/`-separated
  (`"partials/frontmatter.md"`). It is the same string in every file that
  includes it; it is **not** resolved relative to the including file.
- Partials may live **anywhere inside the template root**. Placed **outside the
  source subdirectory** (e.g. a top-level `partials/`) they are pulled in but
  **never emitted** as their own output files. A partial placed *inside* the
  source subdirectory is emitted like any other source file unless an `ignore`
  glob excludes it.
- A partial sees the **same variables** (answers, `data`, computed values) as
  the file that includes it — one flat global namespace.
- Includes work in **rendered file bodies only**: source-tree files and `files:`
  rule sources. They do nothing in prompts, defaults, `when`/`computed`/`format`/
  `options` expressions, target path segments, apply messages, or hook fields.
- **`template.yml` gets no include processing, and neither does any document
  loaded through its YAML `!include` tag.** Those are configuration: Toha parses
  them as structured data, and even the individual string fields it later
  compiles (a prompt, a default, a `files:` `path:`) are loaderless — a
  `{% include %}` written there is not a partial. A partial is authored in a
  file body, not in configuration.
- The include name may be **any file inside the template root**, read as raw
  text. Its role — configuration vs body — is set by *how it is referenced*, not
  by its extension: a generated `.yml`/`.yaml` file in the source tree (or named
  as a `files:` `source:`) is a **file body** and does get includes. There is no
  extension-based rule; the confinement seam is the only gate on a target.
- Absolute names, `..` traversal, and symlinked partials are refused.
- `import` and `extends` remain unavailable. The YAML `!include` tag in
  `template.yml` is a separate feature and is unchanged.

### Call site A — shared front-matter partial (not emitted)

```
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
`parts/license.txt` — or (recommended mode) a variable reached only through it —
is reported **before any interview runs**.

### Call site C — nested partial chain

`template/index.html` → `partials/page.html` → `partials/head.html` →
`partials/meta.html`. Nesting resolves transitively; every name is root-relative,
so a partial names the next by the same path any file would use. A cycle
anywhere in the chain is reported by name (recommended mode) at compile time.

## 2. Module & seam map

One new value object owns the single confinement seam; two file-body call sites
in `plan.rs` construct the include-capable template. Everything else is
unchanged and stays loaderless.

```
Template::load (template.rs)
  └─ root = folder.canonicalize()                         [existing]
  └─ source_dir derived, confined                         [existing]
Plan::build (plan.rs)
  └─ let partials = Partials::rooted(&template.root);      ← built once, from the
  │                                                          single source of truth
  ├─ walk() source-tree file  ─▶ read_render(path, ctx, &partials)
  │                                └─ partials.compile(text, label)? .render(ctx)
  ├─ files: rule source        ─▶ read_render(origin, ctx, &partials)
  │                                          │
  │                       ┌──────────────────┴───────────────────┐
  │                       │   the ONE confinement seam            │
  │                       │   (IncludeRoot::locate / confined_loader)
  │                       │   reject absolute/`..`; refuse symlink │
  │                       │   at any component; canonicalize;      │
  │                       │   starts_with(root); read              │
  │                       └───────────────────────────────────────┘
  ├─ target path segments      ─▶ rendered() ─▶ Tmpl (loaderless)   includes inert
  ├─ apply messages, hooks     ─▶ Tmpl / Expr (loaderless)          includes inert
interview.rs / template.rs field compiles ─▶ Tmpl / Expr (loaderless) includes inert
template.rs !include resolve() ─▶ SEPARATE load-time data feature, own diagnostics
```

Includes are reachable **iff** a caller holds the include-capable template type,
and only `Partials::compile` mints one. Surface scope is a type invariant, not a
guard that can be forgotten.

## 3. Public interfaces (signatures)

New types live in `src/jinja.rs` beside `Tmpl`. `Tmpl` and `Expr` are unchanged
and stay loaderless (so `{% include %}` remains inert on every non-file surface,
exactly as today).

```rust
/// The include boundary for one loaded template. A cheap, cloneable handle to
/// the *selected template root* (the canonical `Template::root`, derived from
/// 1074's `ResolvedTemplate.folder`). It consumes that identity; it defines no
/// new one. It is the ONE seam every include name passes through.
///
/// Invariant: the wrapped root is already canonical (Template::load canonicalizes).
#[derive(Clone, Debug)]
pub struct Partials { root: std::sync::Arc<std::path::PathBuf> }

impl Partials {
    /// Build from the canonical template root. Does not re-canonicalize.
    pub fn rooted(root: &std::path::Path) -> Self { not_implemented!() }

    /// Compile a file body into an include-capable template. `label` is the
    /// file's root-relative path, used only in diagnostics. See §5 for the two
    /// resolution modes (A recommended); the signature is identical for both.
    pub fn compile(&self, source: String, label: &str) -> Result<FileTmpl, IncludeError> {
        not_implemented!()
    }
}

/// A compiled file body that MAY resolve `{% include %}` against its root.
/// A distinct type from `Tmpl`: holding one IS the include capability.
pub struct FileTmpl { /* source, env, referenced_ids */ }

impl FileTmpl {
    pub fn source(&self) -> &str { not_implemented!() }
    /// References including those reached through partials (mode A) — see §5.
    pub fn references(&self) -> &std::collections::HashSet<String> { not_implemented!() }
    pub fn render<S: serde::Serialize>(&self, ctx: S) -> Result<String, RenderError> {
        not_implemented!()
    }
}
```

Signature deltas to existing functions (`plan.rs`), the only call-site change:

```rust
// Plan::build: construct `let partials = Partials::rooted(&template.root);`
//   and thread `&partials` into walk() and the files: loop.
// read_render gains the capability parameter; `rendered()` (path segments) does NOT.
fn read_render(path: &Path, ctx: &impl serde::Serialize, partials: &Partials)
    -> Result<String, PlanError> { not_implemented!() }
```

`IncludeError` folds into `PlanError::Render`/`LoadError` by `Display`
(`e.to_string()`), exactly as `minijinja::Error` does today — no new error
variant leaks to the CLI or crate boundary.

## 4. Data shapes

```rust
/// Distinct wording from the YAML `!include` diagnostics (hard requirement).
/// Every message says "jinja include" (or "partial"); none reuses the
/// `!include` strings ("include escapes template root", "include cycle",
/// "include file not found", "unsupported include extension").
pub enum IncludeError {
    Template(minijinja::Error),                        // syntax error, unchanged wording
    NotFound  { name: String, by: String },            // "jinja include not found: {name}, included by {by}"
    Escape    { name: String, by: String },            // "jinja include escapes template root: {name}, ..."
    Symlink   { name: String, by: String },            // "jinja include path is a symlink: {name}, ..."
    NotUtf8   { name: String, by: String },            // "jinja include is not utf-8: {name}, ..."
    Unreadable{ name: String, source: std::io::Error },// "jinja include is not readable: {name}: {io}"
    Cycle     { path: String },                        // "jinja include cycle: a.md -> b.md -> a.md"
    Dynamic   { by: String },                          // mode A only: "jinja include target must be a literal string, ..."
}
```

Mode A adds one private structure, the confined closure (root-relative name →
confined UTF-8 source, plus the unioned `referenced_ids`), built by a static
walk of the literal include graph with an explicit canonical-path stack — the
same shape as `template.rs::resolve` for `!include`. No `template.yml` schema
shape changes: partials are ordinary files plus inline `{% include %}`.

## 5. The decisions this design resolves

1. **Supported surfaces — rendered file bodies only.** Source-tree files and
   `files:` rule sources (the two `read_render` sites). Excluded, each with
   reason:
   - **Configuration documents — `template.yml` and every document loaded
     through its YAML `!include` tag.** These are never a render surface. Toha
     parses `template.yml` as data (`serde_norway::from_str`, `template.rs:715`)
     and expands `!include` by deserializing the referenced file by extension and
     splicing the parsed value into the config tree (`template.rs:287`–`309`) —
     no Jinja on that path. The include capability is a property of a render
     surface, so it cannot reach configuration by construction. A field's origin
     (inline or `!include`-spliced) does not matter; both are data.
   - **Interview fields and configuration string values** —
     prompts/defaults/`when`/`computed`/`format`/`options`/`regex`, `files:`
     `path:`, apply-message and hook strings. Toha does compile these to
     templates, but through the *loaderless* `Tmpl`/`Expr` (`template.rs:381`,
     `template.rs:403`; `jinja.rs:95`/`128` install no loader), so a
     `{% include %}` there resolves against an environment holding only `"value"`
     and errors with MiniJinja's own `TemplateNotFound` — it never resolves a
     partial. The interview engine also must stay a pure, filesystem-free
     function across the headless/staged/crate drivers; a loader there breaks
     that. Apply messages are excluded to keep the rule crisp (see decision Y).
   - **Target path segments** (`plan.rs:168`) — a segment is a filename fragment;
     injecting multi-line partial text is nonsensical and a path/newline hazard.
   - **Hook fields** — feed process execution; an unaudited injection surface.

   Enforced **structurally**: only the two `read_render` file-body sites are
   handed the include-capable `FileTmpl`; every other site holds a loaderless
   `Tmpl`/`Expr`, and configuration documents are pure data before any template
   exists. This is a type/role invariant, not a guard that can be forgotten.

   **Partial-target selection needs no adjustment.** A partial name resolves,
   through the one confinement seam, to any real file inside the template root,
   read as raw UTF-8 text and inlined. Naming a configuration document
   (`template.yml`, an `!include`-loaded fragment) as a partial target just
   inlines its literal bytes into the body — it does **not** parse or "process"
   that document, so the invariant above (configuration receives no include
   processing) still holds. A blanket extension ban is deliberately **not**
   introduced: it would wrongly block a legitimate `.yml`/`.yaml` partial shared
   into a generated YAML body, and it would confuse role with extension. The
   confinement seam (in-root, no `..`/absolute/symlink) is the only target gate.
2. **Include root — the whole template root.** Confinement/resolution root =
   `template.root`, the identity resolution/1074 already establishes and
   canonicalizes; `Partials::rooted` consumes it. Chosen over `source_dir` so a
   partial can live as support material outside `source_dir` and never be
   emitted — no `ignore` ceremony. `ignore`/`static` are untouched and govern
   emission only; `static` files are still copied byte-for-byte and never
   rendered.
3. **Relative / nested semantics — root-relative, verbatim.** MiniJinja looks
   names up verbatim (`vm/mod.rs:840`), so a fixed root needs no rewriting: the
   author's string is the lookup key. Nested includes are trivial and
   unambiguous. Relative-to-includer is rejected (needs per-includer name
   rewriting; the arena's attempt used a non-`Sync` stateful loader).
4. **Missing include** — Toha wording `jinja include not found: {name},
   included by {by}`, not MiniJinja's raw `tried to include non-existing
   template`.
5. **Recursion / cycle** — resolved by the resolution-mode decision (X) below.
6. **Load-time reference validation** — resolved by decision (X): mode A unions
   references across the graph and validates include-only variables at load;
   mode B accepts render-time surfacing (deterministic because the namespace is
   one flat global set).

## 6. Decisions needed from Bob (present with recommendation)

**X — Resolution mode (the load-bearing one).**
- **(A) Static compile-time confined closure — RECOMMENDED.** Include targets
  are literal strings; the transitive partial set is resolved, confined, read,
  and pre-registered as in-memory named templates at compile time; **no loader
  is installed on the render environment** (zero render-time filesystem access).
  Buys: a **named cycle path** at compile time, missing/escape/symlink reported
  at author time, and **load-time reference validation** across the include
  graph. Costs: no dynamic `{% include some_var %}`; and it enables the
  `minijinja` **`unstable_machinery`** feature to reuse the vendor parser for
  literal-target extraction — an API the crate documents as **"no semver
  guarantees"** (`minijinja/src/lib.rs:193`). This is a maintenance risk, not an
  access-policy change (see the separate disclosure). Recommended because it
  matches Toha's standing "fail at author time with a precise message" posture
  and the `!include` precedent, and gives the best diagnostics the task asks for.
- **(B) Render-time confined loader.** One `Environment::set_loader` closure
  (the single seam) resolves each include lazily; supports dynamic targets;
  needs **no** unstable feature. Costs: a cycle degrades to MiniJinja's generic
  "recursion limit exceeded" (~50 levels), translated to a partial-cycle message
  **without** the cycle path; a variable reached only through a partial surfaces
  at **render**, not load. Confinement is identical and equally sound in both.

*Recommendation: A.* If the `unstable_machinery` dependency is unacceptable,
B is the fallback and the rest of the design is unchanged (same `Partials`
spine, same confinement seam, same root/semantics/wording).

**Y — Apply messages.** Exclude from includes (RECOMMENDED, keeps "file bodies
only" crisp) vs include (they are file-ish operator text). Cost of adding later
is one `Partials::compile` call site.

**Z — Symlink policy for includes.** Refuse **all** symlinks in the include
path (RECOMMENDED — matches `walk`'s outright source-symlink refusal,
`plan.rs:368`, and is the simplest safe rule) vs allow an in-root symlink that
resolves inside the root (matches `confined_file`, `template.rs:451`).

None of X/Y/Z introduces a new timeout, permission/access, pinned runtime
version check, or subprocess integration (see §9 disclosure).

## 7. Proposed contract edits (described, NOT applied — owned by 1061)

- **`docs/template-jinja.md`** — replace the "Supported Jinja features" closing
  sentence ("Jinja `import`, `include`, and `extends` are unavailable …") with a
  statement that `{% include %}` is available in rendered **file bodies only**,
  root-relative literal names, confined to the template root (absolute/`..`/
  symlink refused), partials outside the source subdirectory not emitted,
  partials share the includer's variables, `import`/`extends` still unavailable,
  and the YAML `!include` tag is separate. **State plainly that `template.yml`
  and documents loaded through its YAML `!include` receive no Jinja include
  processing** (they are configuration parsed as data), and that a file's role —
  body vs configuration — follows how it is referenced, not its extension (a
  generated `.yml`/`.yaml` body still gets includes). Add a short "Partials"
  subsection with Call site A.
- **`docs/specifications/template-format.yml`** — revise the "Jinja evaluation"
  paragraph to state `{% include %}` availability on file bodies, root-relative,
  confined (mirrors `!include`/file-path confinement), separate diagnostics from
  the YAML `!include` tag (whose paragraph is unchanged), and to state that
  configuration documents (`template.yml` and its YAML `!include` targets) are
  parsed as data and are not a Jinja render surface.
- **`docs/specifications/template-format.schema.yml`** — **no change** (includes
  add no `template.yml` keys). Deliberate interface-depth point: capability added
  without widening the config contract.
- **`Cargo.toml`** — mode A only: add the `unstable_machinery` feature to the
  existing `minijinja` dependency. Flagged in decision X.

## 8. Behaviors to prove (falsifiable) + sole-kill guidance

Exercised through the fixture harness (the caller interface), following the
`tests/fixtures/<name>/` shape, plus one runtime-constructed symlink test.

**Positive** (`exit: 0`, `expected/` tree):
- `include-basic` (Call site A): partial outside source is inlined, variable
  flows in, `partials/` is **not** emitted.
- `include-nested` (Call site C): transitive composition; partials not emitted.
- `include-files-source` (Call site B): `files:` rule source includes a partial.
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
- `err-jinja-include-symlink` (a symlink **inside** the root pointing outside —
  the vector `safe_join` misses) → `["jinja include path is a symlink"]`.
- `err-jinja-include-cycle` → `["jinja include cycle"]` (mode A: assert the
  named path `a -> b -> a`; mode B: assert the translated recursion message).
- mode A only: `err-jinja-include-dynamic` (`{% include chosen %}`) →
  `["must be a literal string"]`.
- `err-jinja-include-reference` (mode A): a `files:` source whose partial
  references an id defined by no earlier node → `["is not defined by an earlier
  node"]` (proves closure reference validation at load).

**Inertness guards** (prove the seam holds by *type*/*role*, not by luck):
- A source **file name** `{% include "x" %}.txt` and an **apply message** /
  **interview default** each containing `{% include %}` render as today (no
  partial resolution): assert MiniJinja's own not-found behavior
  (`TemplateNotFound`, *not* Toha's `jinja include not found`), proving the
  loaderless `Tmpl` path is untouched.
- `include-config-field-inert`: a `template.yml` **string field** (e.g. a
  `messages.after-apply` or a `default`) that contains `{% include
  "partials/x.md" %}` where `partials/x.md` **exists in the root**. It still does
  **not** resolve the partial — it errors with MiniJinja's generic
  `TemplateNotFound`, never Toha's `jinja include` diagnostics. Existence of the
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

**Distinct-diagnostics guard:** assert no Jinja-include message equals a YAML
`!include` message; the existing `err-include-cycle`/`err-include-escape`
fixtures still pass unchanged.

**Sole-kill agreements the implementer must prove (per task-execution):**
- The include **name↔lookup-key agreement** (author string = registered/looked-up
  key). Mutate the key derivation; a named positive fixture must fail.
- The **confinement seam**: invert `starts_with(root)`, drop the symlink refusal,
  and drop the absolute/`..` rejection each in turn; the matching negative
  fixture must fail by name. The symlink test must hold a symlink **inside** the
  root (the exhaustiveness shape), not a top-level one.
- The **distinct-wording claim**: mutate a Jinja-include message to the
  `!include` wording; the distinct-diagnostics guard must fail.
- The **surface-inertness claim** (a prose claim — sweep it): mutate a
  loaderless site to use `Partials::compile`; an inertness guard must fail. Do
  this for a body-adjacent site (path segment / apply message) **and** for a
  configuration field: giving `Builder::tmpl`/`expr` (`template.rs:381`/`403`)
  the include capability must fail `include-config-field-inert` (and, since
  `!include` feeds the same field compilation, `include-yaml-include-field-inert`
  by the same mutation) — proving configuration exclusion is enforced, not
  incidental.
- The **role-not-extension claim**: `include-yaml-body` must pass unchanged; a
  mutation that filters partial targets or bodies by `.yml`/`.yaml` extension
  must fail it — proving no blanket YAML ban crept in.
- Mode A: sever the reference **union**; `err-jinja-include-reference` must fail.

## 9. Out of scope / compatibility / disclosure

**Out of scope.** `import`, `extends`; dynamic include targets (mode A);
**Jinja include processing of `template.yml` and any document loaded through its
YAML `!include` tag** (configuration is parsed as data, never rendered); includes
on interview fields, expressions, path segments, hook fields, and (pending
decision Y) apply messages; any change to YAML `!include`, `static`, `ignore`, or
path/target confinement; any `template.yml` schema key; any extension-based rule
on which files may be bodies or partials.

**Compatibility.** Templates with no `{% include %}` render identically
(closure/loader is inert). Pure interview logic and all
terminal/headless/staged/direct/crate paths are preserved — the include
capability lives only in `plan.rs` file-body rendering, never in the interview
engine. Staged-interview replay is unaffected: the loader/closure is
reconstructed from `template.root` at plan time and holds no serialized state.

**Access-policy disclosure (separate, per task).** This design grants **no** new
filesystem reach: includes resolve **only** inside the selected template root,
which Toha already reads wholesale when it walks and renders the source tree.
There is **no** new timeout mechanic, **no** permissions/access change, **no**
pinned runtime version check in non-test code, and **no** application subprocess
integration. The only approval-adjacent item is decision X option A enabling the
`minijinja` `unstable_machinery` **build feature** (a no-semver-guarantee crate
API) — a dependency/maintenance risk, disclosed for a nod, not an operator
capability change.

## 10. Verification (Phase F)

Checked against `minijinja` 2.24.0 source and the Toha tree:
- Configuration is data, not a render surface: `Template::load` parses
  `template.yml` with `serde_norway` (`template.rs:715`) and `resolve` expands
  YAML `!include` by deserializing referenced files and splicing parsed values
  (`template.rs:287`–`309`) — no Jinja on that path. Config string fields compile
  through loaderless `Tmpl`/`Expr` (`template.rs:381`/`403`, `jinja.rs:95`/`128`).
  So "includes only in file bodies" holds by construction, and it holds
  identically for inline and `!include`-loaded configuration. Confirmed against
  the existing `tests/fixtures/includes` (`data: !include …`) shape.
- Confinement precedent reused exactly: `template.rs::resolve` (canonicalize +
  `starts_with` + cycle stack), `confined_file` (`template.rs:434`),
  `has_symlink_component` (`plan.rs`) — the design adds nothing novel to the
  security model, only a new caller.
- `safe_join`/`path_loader` do NOT canonicalize (`loader.rs:180`), so the custom
  seam is necessary to close the in-root-symlink vector. Confirmed.
- `set_loader` requires `Send + Sync + 'static` (`environment.rs:234`) — the
  chosen `Partials`/closure design satisfies it; the rejected relative-to-includer
  `RefCell` loader does not.
- `unstable_machinery` exposes `ast::Include { name: Expr, ignore_missing }`
  (`ast.rs:361`), so mode A's literal-vs-dynamic detection is real; the feature
  is "no semver guarantees" (`lib.rs:193`) — the basis for decision X.
- Every requirement/constraint in 1076 maps to a section here; failure cases and
  falsifiable fixtures are enumerated in §8; compatibility with 1074's
  `ResolvedTemplate` root (consumed, not redefined) and non-intersection with
  1069's trust seam (includes are inert text) are recorded.
