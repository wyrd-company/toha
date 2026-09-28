# Grounding — Jinja includes within the template root

Architect Phase A. Traces the actual caller-to-result flow, types, state
ownership, error paths, and the trust seam that a `{% include %}` feature must
integrate with. Evidence is file/symbol anchored against `epic/second-release`
at `4738042a766dc7b2d1d3c76a9c83c41c03e803d2`.

## The question

Toha renders template source files with MiniJinja. Today Jinja `include`,
`import`, and `extends` are disabled: `docs/template-jinja.md` states "Jinja
`import`, `include`, and `extends` are unavailable in rendered templates."
Task 1076 designs turning `{% include %}` on, confined to the selected template
root, without granting arbitrary filesystem access and without disturbing the
separate YAML `!include` feature.

## How rendering works today

### One environment, one template, no loader

Every rendered string is compiled and rendered in isolation by
`src/jinja.rs`:

- `jinja::environment()` (`src/jinja.rs:18`) builds a fresh
  `minijinja::Environment` with the case filters, `toyaml`/`tojson`, `now()`,
  and `dateformat`. It sets no loader.
- `Tmpl::compile(source)` (`src/jinja.rs:95`) creates one environment, calls
  `env.add_template_owned("value", source)`, and records
  `undeclared_variables(false)` as `referenced_ids`. The environment holds
  exactly one template named `"value"`.
- `Tmpl::render(ctx)` (`src/jinja.rs:111`) renders `"value"`.
- `Expr` (`src/jinja.rs:116`) compiles an expression, no loader, no templates.

Consequence: `{% include "x" %}` in any current template resolves the name `x`
against an environment that contains only `"value"` and has no loader, so it
fails today with `TemplateNotFound`. There is no code path from one template
file to another. Turning includes on means giving the environment a **loader**
(or pre-registered templates) that maps an include name to source text, and
deciding the confinement boundary for that mapping.

### The render surfaces (where `Tmpl` is used)

`Tmpl` renders far more than source files. Every surface is a candidate for
"does include apply here?", and the design must answer per surface:

| Surface | Site | Notes |
| --- | --- | --- |
| Source-tree file contents | `plan.rs:384` `read_render` → `rendered` (`plan.rs:146`) → `Tmpl::compile(text).render` | The primary surface. Each file under `source_dir` rendered independently. |
| `files:` rule sources | `plan.rs:221` `read_render(&origin, &local)` | Explicit `files` rules with `each`; same `rendered` path. |
| Target path segments | `plan.rs:168` `rendered(segment,...)` per component | Single path segment; include here is nonsensical and unsafe. |
| Interview text/expr (prompt, default, computed, when, format, options) | `interview.rs`/`template.rs` via `Tmpl`/`Expr` | Field-level strings and expressions. |
| Apply messages (before/after-apply) | `plan.rs:261` `source.render(&ctx)` (stored `Tmpl`) | Rendered from `template.messages`. |
| Hook fields (command, args, cwd, env) | `interview::render_hooks` | Rendered strings feeding hook execution. |

Static files are copied byte-for-byte and never rendered (`plan.rs:381`,
`Content::Copied`), so includes never apply to `static` matches.

### Load-time reference validation and ownership

At load (`template.rs:842`–`846`), each `files` rule's source content is read and
compiled via `b.tmpl(...)` purely to validate that referenced ids are defined
earlier (`Builder::refs`, `template.rs:388`). The compiled value is discarded;
`FileRule` keeps only the source path (`template.rs:171`–`176`). `Plan::build`
then reads and compiles that file again through `read_render` (`plan.rs:221`).
This split is important: giving only the plan-time call an include-capable
compiler cannot provide the promised pre-interview missing-partial and
include-reference failures.

The load-time caller order is concrete: command drivers call `Template::load`
before `Interview::start` (`main.rs:830`–`840`) and before replay
(`main.rs:901`–`909`). The include design must therefore make the loaded
`Template` own the root-bound include capability, make each `FileRule` own its
compiled include-capable body, and have `Builder` validate that body's unioned
references while its `each` binding is in scope. `Plan::build` can render the
stored body without rereading or recompiling it. Source-tree files remain
undiscovered until `walk`, so they are still compiled at plan time through the
same capability owned by `Template`.

`undeclared_variables` is a static per-AST analysis (`minijinja
template.rs:425`); it does **not** follow includes. A file that pulls variables
in through a partial therefore needs the closure builder to union references
from the selected transitive include graph before `Builder::refs` runs.

### Configuration documents are parsed as data, never rendered

`template.yml` and every document pulled into it are **configuration**, not a
render surface. The parse path never touches Jinja:

- `Template::load` (`template.rs:727`) reads `template.yml` and parses it with
  `serde_norway::from_str` into a `serde_norway::Value` (`template.rs:734`) — a
  data tree, not a template.
- `resolve` (`template.rs:271`) walks that data tree and expands the YAML
  `!include` tag by **reading the referenced file and deserializing it by
  extension** — YAML/JSON/TOML (`template.rs:305`–`320`) — then splicing the
  parsed value back into the tree (`template.rs:327`, `*value = next`) and
  recursing into it. Nothing on this path is compiled or rendered as a template;
  `!include` composes *structured data*.
- The resolved tree is converted to JSON, schema-validated, and deserialized
  into `RawTemplate` (`template.rs:746`–`759`). Still no Jinja.

Only afterwards are **individual string fields** selectively turned into
templates: `Builder::tmpl` → `Tmpl::compile` (`template.rs:399`) and
`Builder::expr` → `Expr::compile` (`template.rs:421`). Those compile through the
same loaderless `jinja::environment()` as every render surface above. Today,
`multi_template` is disabled and the statement grammar is absent. Enabling that
feature crate-wide would make “no loader” an insufficient boundary because the
environment registers its current template as `"value"`. The design therefore
adds a loaderless AST mode that rejects every multi-template statement before
compilation; a `{% include %}` in a prompt, default, apply message, or `files:`
`path:` is unavailable outside file bodies and never becomes a partial.

Consequence for this design: the include capability is a property of the
**render surface**, and configuration documents are not one. A field's origin —
inline in `template.yml` or spliced in through a YAML `!include` — makes no
difference: both are data, and both yield loaderless field templates. The
existing `tests/fixtures/includes` fixture is exactly this shape
(`data: !include data/a.yml`, plus `.json`/`.toml`), and it confirms `!include`
carries parsed values into `data`, not text into a renderer.

A file's **role** is set by how it is referenced, not by its extension. The same
`.yml` file is configuration when it is `template.yml` or reached through
`!include`; it is a **file body** when it sits in the walked source tree
(`plan.rs:384`) or is named as a `files:` `source:` (`plan.rs:221`), and then it
is rendered like any other body. So "includes only in file bodies" is a
statement about *role*, and it must not become a blanket ban on any extension:
a generated `.yml`/`.yaml` body is still a body.

An include target has no required directory or filename shape. Its name may
identify a root-level file such as `notice.txt`, a nested file such as
`shared/legal/license.inc`, or a file with any extension. `partials/` is only a
useful authoring convention. The template-root confinement checks are the full
target-selection policy; a directory-prefix allowlist would narrow the approved
capability without adding confinement.

## The two existing confinement precedents (reuse, keep separate)

### YAML `!include` — `resolve()` in `template.rs:271`

Replaces a `!include <path>` tag inside `template.yml` with parsed file
content. Its confinement model is the template it must **not** be confused with,
and the exact shape to mirror:

- `root.join(name).canonicalize()` then `actual.starts_with(root)` else
  `"include escapes template root"` (`template.rs:288`–`296`).
- Cycle detection via an explicit `stack: Vec<PathBuf>` of canonical paths;
  re-entry → `"include cycle"` (`template.rs:297`–`300`).
- Extension dispatch: `.yml`/`.yaml`→YAML, `.json`→JSON, `.toml`→TOML, else
  `"unsupported include extension"` (`template.rs:305`–`320`).
- Fixtures: `tests/fixtures/err-include-cycle` (`error_contains: ["include
  cycle"]`), `tests/fixtures/err-include-escape` (`["include escapes"]`).

This is a **data** feature (structured config composition), evaluated at load,
producing parsed values. Jinja `{% include %}` is a **text** feature, evaluated
at render, producing rendered text. The task requires they stay separate; the
diagnostics must not collide (distinct message wording).

### `confined_file()` — `template.rs:452`

Validates every `template.yml` path reference (file sources, script paths):

- Reject absolute → `"file path must be relative to template root"`.
- `root.join(value).canonicalize()`; missing → `"file does not exist"`.
- `!actual.starts_with(&self.root) || !actual.is_file()` →
  `"file must stay inside template root and be a file"`.
- Returns the path **relative to root** (`strip_prefix(&self.root)`).

### Source-tree symlink rejection — `plan.rs:368`

`walk` rejects any symlink entry in the source tree outright:
`"source symlink not supported"`. Target-path symlink components are rejected at
`plan.rs:287` (`has_symlink_component`). So the codebase's standing policy is:
**canonicalize and confine, and refuse symlinks rather than follow them.**

## MiniJinja include grammar and feature gates (minijinja 2.24.0)

Toha declares `minijinja` with `default-features = false` and currently enables
only `builtins`, `serde`, `json`, `macros`, and `loop_controls` (`Cargo.toml:30`).
The current `cargo tree -e features -i minijinja` has neither feature required by
the selected design:

- `unstable_machinery` exposes `machinery::parse` and the AST (`lib.rs:253`–`267`).
- The separate `multi_template` feature exposes the parser arms and AST variants
  for `block`, `extends`, `include`, `import`, and `from ... import`
  (`parser.rs:865`–`874`; `ast.rs:67`–`76`). It also enables include execution
  (`vm/mod.rs:830`). `unstable_machinery` does not imply `multi_template`.

The implementation therefore needs both features. Because `multi_template`
widens MiniJinja's grammar as a group, the include-capable compiler must walk
every statement, including nested statement bodies, and reject `block`,
`extends`, `import`, and `from ... import`. This preserves the approved change:
only `include` becomes available on file bodies. Merely omitting names from the
pre-registered closure is insufficient because an import or extends can name a
template that an include has already caused the closure to register.

The feature is crate-wide, so the loaderless `Tmpl::compile` path also begins to
parse these statements. That environment registers the current template as the
publicly guessable name `"value"` (`jinja.rs:98`), so “no loader” alone is not a
complete capability boundary: `{% include "value" %}` or a sibling statement
could reach the current template. The type boundary therefore needs two AST
modes. Ordinary `Tmpl` rejects every multi-template statement; `FileTmpl`
admits `include` and rejects `block`, `extends`, `import`, and
`from ... import`. `Expr` cannot contain statements and needs no change.

The parsed `Include` has `name: Expr` and `ignore_missing: bool`
(`ast.rs:357`–`364`). MiniJinja accepts `with context` and `without context` for
syntax compatibility but deliberately gives both no separate meaning
(`parser.rs:1185`–`1202`, `1240`–`1245`). At execution it treats a sequence as
ordered candidates, requires each tried candidate to be a string, selects the
first template found, and suppresses only the all-not-found result when
`ignore_missing` is true (`vm/mod.rs:838`–`864`, `900`–`915`). These semantics
can be reproduced by the static closure without a render-time loader: select
and register the first confined candidate that exists; missing earlier
candidates are fallbacks, while an error opening an existing candidate is not.

## MiniJinja loader internals (minijinja 2.24.0)

Read from the vendored crate; these shape what a confining loader must add.

- `Environment::set_loader(f)` where `f: Fn(&str) -> Result<Option<String>,
  Error>` (`environment.rs:234`). Called lazily on `get_template`, include,
  import, extends. `Ok(None)` → `TemplateNotFound`.
- `path_loader(dir)` (`loader.rs:207`) uses `safe_join` (`loader.rs:180`).
  **`safe_join` rejects any segment starting with `.` (so `..`, `.git`,
  dotfiles) and any segment containing `\`, but it does NOT canonicalize.** It
  therefore does **not** stop a symlink inside the root that points outside the
  root: `fs::read_to_string` would follow it. So `path_loader` alone does not
  satisfy the inherited "included content must stay within the selected template
  root" + no-symlink-escape policy. A custom loader mirroring
  `!include`/`confined_file` (canonicalize + `starts_with` + reject symlink) is
  required.
- Include name resolution is **not relative to the including template**.
  MiniJinja looks the include name up verbatim in the environment/loader
  namespace (`vm/mod.rs:840`). "Relative to the including file" semantics would
  require the design to rewrite names against the includer's directory.
- Recursion: `INCLUDE_RECURSION_COST = 10` against a default limit of
  `MAX_RECURSION = 500` (`vm/mod.rs:43`, `environment.rs:53`). So ~50 nested
  includes → `"recursion limit exceeded"` (`vm/context.rs:369`). A direct
  self-include cycle surfaces as this generic message after 50 iterations, not
  as a named cycle. Include failures wrap as `ErrorKind::BadInclude`
  (`vm/mod.rs:895`); missing includes as `TemplateNotFound` "tried to include
  non-existing template" (`vm/mod.rs:902`). Precise cycle diagnostics require
  static graph walking; lazy loading gives only the generic recursion message.
  This is a distinguishing design axis.
- `{% include %}` supports `ignore missing` and an ordered list of candidate
  names (`vm/mod.rs:838`–`915`). The final design preserves those literal forms
  while continuing to reject computed names.

## Trust and coordination seam

- Jinja includes are inert template **text**: rendering an included file runs
  no hooks, no scripts, no process. So includes introduce **no** executable
  trust intersection beyond sharing the same selected template root.
- Selected-template resolution owns the identity/root:
  `ResolvedTemplate { formal_name, commit, folder, approval, named }`. The
  `folder` is the template root; `Template::load(folder)` canonicalizes it to
  `template.root` (`template.rs:728`) and derives `source_dir`
  (`template.rs:760`). The include confinement boundary is derived from the
  same `template.root`/`source_dir` this design consumes; it does not define a
  new identity.

## Preserve / Change / Avoid / Risk

**Preserve.**

- Pure interview logic and all driver paths (terminal/headless/staged/direct/
  crate); rendering stays a pure function of template + answers.
- YAML `!include` as a distinct load-time data feature with its own diagnostics.
- Configuration parsing as a data-only path: `template.yml` and every document
  reached through its YAML `!include` are deserialized (`serde_norway`), never
  Jinja-rendered; the include capability never reaches them, and their string
  fields stay loaderless.
- The canonicalize-and-confine + refuse-symlink policy already applied to
  sources, targets, `!include`, and `confined_file`.
- `static` files never rendered; `template.yml` never rendered as a source.

**Change.**

- The MiniJinja environment used for renderable **file** surfaces must gain a
  confining loader (or pre-registered partials) keyed to the template root.
- `docs/template-jinja.md`'s "include … unavailable" statement and the
  `template-format.yml` spec Jinja section — described as proposed edits owned
  by implementation, not made here. No schema shape changes.

**Avoid.**

- Pointing a raw `path_loader`/filesystem loader at any directory without the
  canonicalize+confine+symlink guard (grants arbitrary read via symlink).
- Collapsing Jinja include diagnostics into the `!include` wording.
- Enabling includes on surfaces where they are unsafe or meaningless (target
  path segments) or unaudited (hook fields) without deliberate decision.
- Introducing a new configuration/timeout/permission knob (would trigger
  separate approval) unless a candidate deliberately proposes one and flags it.

**Risk.**

- Reference validation gap: variables reached only through an include are not
  validated by today's per-template `undeclared_variables`; late render errors
  possible unless the design walks includes.
- Cycle diagnostics: without static graph walking, cycles degrade to the
  generic recursion-limit message.
- Include-root choice (`source_dir` vs whole template root) changes whether a
  shared partial can live outside the emitted tree; interacts with `ignore`
  (a partial under `source_dir` would otherwise be emitted as its own file).
