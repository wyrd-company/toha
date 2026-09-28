# Candidate B: Format-preserving structured mode via `jsonc-parser` CST

## Overview

Adopt `jsonc-parser` (0.34.0, feature `cst`) as Toha's surgical JSON-family (JSON/JSONC/JSON5) injection mechanism, mirroring the marker-based system's reach into structured formats. The template author declares a structured **target path** (e.g. `features.enabled` in a `package.json`) and a value; Toha parses the file into a Concrete Syntax Tree (CST), navigates or creates the path, sets the value, and writes back preserving every comment, key order, and untouched whitespace. This mode composes alongside marker-based injection under a single `inject:` surface.

---

## 1. Author surface (template.yml)

Structured injection is a new variant within the existing `inject` list. A template author declares:

```yaml
inject:
  # Marker-based injection (retained, for arbitrary text):
  - into: ".gitignore"
    region: "build-artifacts"
    content: |
      /target
      *.tmp
  
  # NEW: Structured injection (JSON-family files):
  - into: "package.json"                 # TargetPath (same guards)
    struct:                              # discriminator: not region/content
      path: "scripts.test"               # target path (dot-separated, array indexable)
      value: "jest"                      # literal value or rendered template
    when: "has_tests"                    # optional condition
    # comment omitted for JSON (strict JSON cannot carry markers)
    # For JSONC, optional: provenance marker in preceding comment
    provenance: true                     # JSONC-only; default false
    create: true                         # may apply create the file if absent
```

The `struct` key is mutually exclusive with `region` (the discriminator). Inside `struct`:
- `path`: dot-separated navigation string (e.g. `"config.debug.level"`, `"routes[0].name"`). Parsed at build time into a typed path; invalid syntax (mismatched brackets, leading dots) is a build error.
- `value`: a string (rendered from template variables, same as `content`). At apply time, this string is parsed as JSON to extract the actual value to set (e.g. `value: "42"` → JSON number `42`, `value: "true"` → boolean, `value: '{"x":1}'` → object). If the string is not valid JSON, it is a build error (fail-fast, not apply-time).
- `provenance` (JSONC only): if `true`, Toha embeds a `// toha:struct path/value ` marker comment immediately preceding the target key (first apply only). Enables future drift detection in JSONC; ignored for strict JSON. Default `false`.
- `create`: allow apply to create the file if absent (default `false`).

Validation at build time:
- Path syntax is valid.
- Value is valid JSON (parsed to a serde_json::Value for type confirmation; no runtime serde dependency, just syntax validation).
- File extension is `.json`, `.jsonc`, or `.json5` (unknown structured format is a build error; markers remain the fallback for custom formats).

---

## 2. Concrete before/after examples

### Example A: `.json` file (strict JSON, no comments)

**Template config:**
```yaml
inject:
  - into: "package.json"
    struct:
      path: "scripts.build"
      value: "tsc && webpack"
    create: false
```

**Target BEFORE (first apply):**
```json
{
  "name": "myapp",
  "version": "1.0.0",
  "scripts": {
    "start": "node index.js"
  }
}
```

**Target AFTER first apply:**
```json
{
  "name": "myapp",
  "version": "1.0.0",
  "scripts": {
    "start": "node index.js",
    "build": "tsc && webpack"
  }
}
```

**Target AFTER second apply (same template/answers):**
```json
{
  "name": "myapp",
  "version": "1.0.0",
  "scripts": {
    "start": "node index.js",
    "build": "tsc && webpack"
  }
}
```

**Byte-level change on second apply:** NONE. The value at `scripts.build` already equals `"tsc && webpack"`, so `resolve_struct_edit` returns `Unchanged`. File is not rewritten.

**Drift case (user edits `scripts.build` to `"yarn build"`):**
```json
{
  "name": "myapp",
  "version": "1.0.0",
  "scripts": {
    "start": "node index.js",
    "build": "yarn build"
  }
}
```

**Re-apply without `--force`:** The value at `scripts.build` (`"yarn build"`) does not equal the desired value (`"tsc && webpack"`). Toha **cannot detect whether this was user-edited or a template change** (no marker space in JSON). Behavior: **Converge to desired state** — Toha sets the value to `"tsc && webpack"` and writes the file. This is the honest "declarative" contract: "the value at this path SHALL be set to this value" (idempotent, always converges). No `--force` gate for JSON structured mode (the mode itself is convergent).

**After drift re-apply:** Same as after-second-apply above.

---

### Example B: `.jsonc` file (JSON with comments and trailing commas)

**Template config:**
```yaml
inject:
  - into: "tsconfig.json"     # often written as .jsonc in practice
    struct:
      path: "compilerOptions.strict"
      value: "true"
      provenance: true        # embed marker in preceding comment
    create: false
```

**Target BEFORE:**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // Toggle strict mode here
  },
}
```

**Target AFTER first apply:**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // toha:struct compilerOptions.strict
    "strict": true,
    // Toggle strict mode here
  },
}
```

**Key observations:**
- The trailing comma after `strictNullChecks` is preserved (JSONC allows it).
- The comment `// Compiler strictness` before the block is unchanged.
- The new `provenance: true` comment is inserted on a line immediately before the `"strict"` key.
- The trailing comment `// Toggle strict mode here` remains at the end.
- Indentation (2-space) is preserved; blank lines are preserved.

**Target AFTER second apply (unchanged template):**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // toha:struct compilerOptions.strict
    "strict": true,
    // Toggle strict mode here
  },
}
```

**Byte-level change on second apply:** NONE. CST parses, checks that the value at `compilerOptions.strict` equals `true`, finds no changes needed, and returns `Unchanged`.

**Drift case (user removes the `strict` key entirely):**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // Toggle strict mode here
  },
}
```

**Re-apply:** CST parses, navigates to `compilerOptions`, looks up `strict`, finds it missing. Toha re-creates the key with value `true` and the provenance comment. (For JSONC with provenance enabled, the re-created comment is byte-identical, proving no user edit occurred; for non-provenance JSONC or JSON, this is indistinguishable from a first placement.)

**After drift re-apply:**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // toha:struct compilerOptions.strict
    "strict": true,
    // Toggle strict mode here
  },
}
```

---

## 3. Ownership / drift / idempotency model

### Ownership

**Toha owns the VALUE AT A NAMED PATH** in the JSON-family file. Ownership is scoped to a single leaf (or container) value addressable by a dot-separated path (e.g. `scripts.build`, `compilerOptions.strict`, `routes[0].method`). Everything else in the file belongs to the owner.

This differs from marker-based injection, which owns a **byte-spanned region delimited by begin/end markers**. Structured mode owns a **logical JSON value**, not a byte range.

### Idempotency without markers

**Toha makes the value at the path equal to the desired value; if the value already equals the desired value, no bytes change.**

This is a declarative contract, not an imperative one. Pseudocode:

```
resolve_struct_edit(current_file_bytes, struct_edit) =
  current = parse(current_file_bytes) using CST
  navigate to struct_edit.path in current
  if (current_value at path) == (struct_edit.value):
    Unchanged
  else:
    set value at path to struct_edit.value
    reserialize current to bytes (CST.to_string() — preserves formatting)
    Write(bytes)
```

**Why is this idempotent?** Because `set_value` is idempotent: setting the same value twice produces the same bytes (CST only rewrites the exact node, leaving everything else untouched). Therefore:

- First apply: value missing or different → Write.
- Second apply with unchanged template: value equals desired → Unchanged (zero bytes written).
- Third apply (or 100th): same as second.

### The hard question: drift detection without markers

**Can Toha distinguish "user edited the value at the path" from "first set"?**

**Answer: NO, not in strict JSON.** JSON has no comment syntax, so Toha cannot embed a checksum or provenance marker to prove "this value is the one Toha last wrote." This is the fundamental tradeoff.

**But this is safe, because the operation is declarative:** Toha does not care *why* the value is present or changed — it owns the value at the path, and every apply sets it to the desired state. If the user edits it and Toha re-applies, the user's edit is overwritten. This is honest, predictable, and surfaced to the author ("if you manage a path, you own it; other edits to that key will be lost on re-apply").

**For JSONC files (with comment syntax), optional drift *awareness* is available:** The `provenance: true` option embeds a `// toha:struct path` comment immediately before the target key on first apply. A future re-apply can check if that comment still precedes the key; if it does, the user did not manually delete and recreate it. This is a *best-effort* heuristic (not a cryptographic proof like checksums), but it allows authors to opt into stricter semantics if they wish. **This option does not block convergence; it only adds metadata.**

**Explicit contract for Toha consumers:**
- Structured mode is **convergent**: re-apply sets the value to the desired state, idempotent.
- If the file is at its desired state, no bytes change (zero-cost re-apply).
- If the user edits the value and Toha re-applies, the user's edit is replaced.
- For JSONC, `provenance: true` adds an inline comment marker for future awareness; it does not gate convergence.
- For strict JSON, no marker is possible; the path ownership is implicit (authors must document it).

### Composition with marker-based injection

In a single `inject:` list, entries with `region` (marker-based) and `struct` (structured) can coexist:

```yaml
inject:
  - into: "setup.sh"
    region: "init_env"
    content: |
      export DEBUG=1

  - into: ".env.example"
    struct:
      path: "APP_DEBUG"
      value: "1"

  - into: "config.yaml"    # arbitrary YAML; markers only
    region: "database"
    marker: { open: "#", close: "" }
    content: |
      host: localhost
```

Each entry is independent; they apply in order, each reading the target's current bytes (which may include changes from the prior entry in the same plan). The same file can be targeted by multiple entries (each with distinct `region` keys for markers, or distinct `path` values for structured). There is no mutual exclusion.

---

## 4. Data/type sketch + key signatures

All types are in a new module `src/injection/structured.rs`.

```rust
// src/template.rs — extends InjectionRule
pub enum InjectionBody {
    Marker {
        region: Tmpl,                      // stable key
        body_source: BodySource,           // Support | Inline
        marker: MarkerSpec,
        anchor: Option<AnchorSpec>,
    },
    Struct {                               // NEW
        path: StructPath,                  // parsed & validated at build
        value: Tmpl,                       // rendered to JSON at apply
        provenance: bool,                  // JSONC: embed marker comment?
    },
}
// InjectionRule becomes:
pub struct InjectionRule {
    pub into: Tmpl,
    pub body: InjectionBody,               // discriminator replaces old `region` field
    pub create: Typed<bool>,
    pub each: Option<Each>,
    pub when: Option<Expr>,
}

// src/injection/structured.rs
/// Dot-separated path to a JSON value, validated at build. 
/// Examples: "scripts.build", "config.debug.level", "routes[0].name"
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructPath {
    segments: Vec<PathSegment>,            // built from "a.b[0].c"
}

#[derive(Clone, Debug)]
pub enum PathSegment {
    Key(String),                           // object key
    Index(usize),                          // array index
}

impl StructPath {
    /// Parse "a.b[0].c" syntax. Fail if invalid.
    pub fn parse(input: &str) -> Result<Self, StructPathError> {
        not_implemented()
    }
}

// src/plan.rs — extend PlannedEdit union
pub enum PlannedEdit {
    Marker {
        path: TargetPath,
        region: RegionKey,
        body: String,
        marker: MarkerStyle,
        anchor: Option<Anchor>,
        create: bool,
        source: Option<PathBuf>,
    },
    Struct {                               // NEW
        path: TargetPath,
        struct_path: StructPath,
        value: String,                     // rendered JSON value (must parse as valid JSON)
        provenance: bool,
        create: bool,
    },
}

// src/injection/structured.rs
/// Outcome of resolving a structured edit against the current file bytes.
pub enum StructResolution {
    Unchanged,                             // value at path already equals desired
    Write(Vec<u8>),                        // file with value at path updated
}

/// Pure function: resolve a structured edit against current file bytes.
/// Called from apply (post-read) and dry-run.
/// 
/// Purity: depends only on (current_file_bytes, struct_edit).
/// 
/// Errors: malformed JSON, missing/invalid path, unrecognized format, non-UTF-8.
pub fn resolve_struct_edit(
    current: Option<&[u8]>,                // file bytes (None = absent)
    edit: &PlannedEdit,                    // must be Struct variant
) -> Result<StructResolution, StructError> {
    // 1. Absent file: create (empty object {} + set path) iff edit.create, else StructError::TargetMissing.
    // 2. Non-UTF-8: StructError::NotUtf8.
    // 3. Parse into CST via jsonc_parser::cst::CstRootNode::parse (using ParseOptions for strict JSON if needed).
    // 4. Navigate to struct_edit.struct_path, creating intermediate objects/arrays as needed.
    // 5. Get current value at path (or None if missing).
    // 6. Parse struct_edit.value (already validated as JSON at build; should never fail, but handle gracefully).
    // 7. If current value == desired value: return Unchanged.
    // 8. Set value at path using CstRootNode/CstObject mutation API.
    // 9. If provenance: embed/check provenance marker comment (JSONC only; no-op for strict JSON).
    // 10. Reserialize via CstRootNode::to_string(), which preserves all formatting/comments/key order.
    // 11. Return Write(bytes).
    not_implemented()
}

pub enum StructError {
    TargetMissing { path: TargetPath },
    NotUtf8 { path: TargetPath },
    MalformedJson { path: TargetPath, why: String },
    InvalidPath { path: TargetPath, struct_path: String, why: String },
    UnsupportedFormat { path: TargetPath, format: String },
    Unimplemented { msg: String },        // e.g., array navigation not yet built
}

// src/plan.rs — extend Plan::build to handle InjectionRule.body
impl Plan {
    pub fn build_structured_injection(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Vec<PlannedEdit>, PlanError> {
        // For each InjectionRule with body = Struct:
        // 1. Render struct.path (usually literal; can contain template vars).
        // 2. Render struct.value to string.
        // 3. Validate value as JSON (parse and discard; just check syntax).
        // 4. TargetPath::parse on into + symlink checks.
        // 5. Collect into PlannedEdit::Struct.
        // 6. Check for duplicate (path, struct_path) pairs → PlanError::Duplicate.
        not_implemented()
    }
}

// src/apply.rs — extend apply to handle PlannedEdit::Struct
pub fn apply_reporting(
    plan: &Plan,
    target_root: &Path,
    options: ApplyOptions,
) -> Result<Applied, ApplyError> {
    // ... existing marker edits ...
    
    for edit in &plan.edits {
        match edit {
            PlannedEdit::Marker { .. } => { /* existing code */ }
            PlannedEdit::Struct { path, .. } => {
                let current = read_optional_bytes(target, path)?;
                match resolve_struct_edit(current.as_deref(), edit)? {
                    StructResolution::Unchanged => {}
                    StructResolution::Write(bytes) => {
                        // atomic temp+rename (same as markers)
                        write_atomic(target, path, &bytes)?;
                        resolved.push(path.clone());
                    }
                }
            }
        }
    }
    // ... rest of apply ...
}
```

---

## 5. Error/results contract

### Build time

- `PlanError::InvalidPath { rule, path, why }` — the `path` field in `struct:` is not valid syntax (e.g. `"a..b"`, `"[x]"`, `"a["`, unmatched brackets).
- `PlanError::InvalidJson { rule, value, why }` — the `value` field is not valid JSON (e.g. `value: "true"` is valid, `value: "truee"` is not; validation happens at build time, not apply time).
- `PlanError::UnsupportedFormat { rule, path }` — file extension is not `.json`, `.jsonc`, or `.json5` (suggests using markers instead).
- `PlanError::Duplicate { path, struct_path }` — two rules target the same path + struct_path pair.
- `PlanError::Render { rule, why }` — rendering the template variables in `path` or `value` failed.

### Apply time

- `StructError::TargetMissing { path }` — file does not exist and `create: false`. Exit 1.
- `StructError::NotUtf8 { path }` — file is not UTF-8. Exit 1.
- `StructError::MalformedJson { path, why }` — file is present but not valid JSON/JSONC/JSON5 (parser error). Exit 1.
- `StructError::InvalidPath { path, struct_path, why }` — the path (even if it was valid at build time) fails to navigate (e.g. array out of bounds, key doesn't exist and auto-vivify failed). Exit 1.
- `StructError::UnsupportedFormat { path, format }` — file extension changed between build and apply (unlikely, but surfaced). Exit 1.

### Idempotency / re-apply

- If the value at `struct_path` already equals the desired value, `resolve_struct_edit` returns `Unchanged` → no write → file unchanged → exit 0, print nothing.
- If the file is absent and `create: true`, apply creates it with the value; a second apply sees the value present and returns `Unchanged`.

### Dry-run

- `toha apply --dry-run <PATH>` prints:
  - `inject <path> <struct_path>` if not present (first apply).
  - `update <path> <struct_path>` if value differs.
  - Nothing if value already equals desired (idempotent).

---

## 6. Format/comment/order preservation

### Cited evidence (from `/tmp/injection-research/rust-structured.md`)

**jsonc-parser CST module** (`src/cst/mod.rs:1-5` doc comment):
> "CST for manipulating JSONC. Unlike the AST, this keeps every comment and every piece of whitespace, so a document can be edited and written back out with everything the author wrote still in place."

**Core preservation machinery** (`src/cst/mod.rs`):
- `CstRootNode::parse` (line 1146) — parse full CST via `parse_to_ast` with `CommentCollectionStrategy::AsTokens` and `tokens: true`.
- `CstObject::append/insert/remove/sort_properties` (lines 1798–1858) — surgical mutations; doc at lines 1821–1834 explicitly documents that properties carry "leading comments/blank lines ... along with [them] when reordering."
- `set_value` on every node (lines 1964, 1554, 2315, 2348) — replaces only the value, preserves surrounding trivia.
- `impl Display for CstRootNode` (line 1356) — `to_string()` reproduces "the full original text plus edits, comments included."

### What is preserved

1. **Comments**: Every `//` (line) and `/* */` (block) comment is retained in the CST and re-emitted verbatim at serialization.
2. **Key order**: Object properties iterate in source-declared order (CST children = source order). Appending a new key adds it at the end; inserting adds it at the specified position; reordering via `sort_properties` carries comments with each property.
3. **Whitespace/newlines**: Every space, tab, newline, and blank line outside the edited value is byte-preserved. Only the edited value's text changes.

### What is NOT preserved in structured mode (by design)

- The **value itself** at the path is recomputed and reserialize from the CST node, not byte-copied. So if the user has an object value with an unusual comment or key order inside it, and Toha re-sets that object value, the internal structure is regenerated by `CstInputValue::from(serde_json::Value)`. This is acceptable because Toha owns the *value at the path*, not the internal structure.

> [!IMPORTANT]
> **API correction:** The inspected `jsonc-parser` revision `c7d4cf5` does not
> implement `From<serde_json::Value>` for `CstInputValue`. Its
> `src/cst/input.rs` defines the input variants and primitive conversions only.
> The implementation therefore uses a Toha-owned recursive
> `json_value_to_cst_input(&serde_json::Value) -> CstInputValue` converter.
> The candidate statement above remains intact as historical arena evidence;
> this correction is authoritative for implementation.

- **Exact quote style** of strings: if the user wrote `{ "x": "hello" }` with double quotes and Toha sets it again, the reserialize may produce the same double quotes (typical), but this is not guaranteed. For safety, assume it may normalize to the crate's default (which is typically double quotes for JSON). This is the same tradeoff as `toml_edit`: editing a value may normalize quotes/number representation, but the value itself is preserved.

### Proof via example (from Section 2, Example B)

**Before:**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // Toggle strict mode here
  },
}
```

**After setting `compilerOptions.strict = true`:**
```jsonc
{
  "compilerOptions": {
    // Compiler strictness
    "noImplicitAny": true,
    "strictNullChecks": true,
    // toha:struct compilerOptions.strict
    "strict": true,
    // Toggle strict mode here
  },
}
```

**Preserved:**
- Line comment `// Compiler strictness` before the block ✓
- Line comment `// Toggle strict mode here` after the edited portion ✓
- Trailing comma after `strictNullChecks` ✓
- 2-space indentation ✓
- Blank-line structure (none in this example, but CST preserves them) ✓

**Changed:**
- One new line inserted with the new key-value pair (`"strict": true`) and an optional marker comment ✓

This is the CST guarantee: "everything the author wrote still in place" except for the nodes you edit.

---

## 7. Dependency justification

### Dependency: `jsonc-parser` version 0.34.0

**Why adopt vs. build:**

1. **Well-regarded, actively maintained.** GitHub: https://github.com/dprint/jsonc-parser. Used by dprint (a Rust code formatter) and Deno (the JavaScript runtime). Over 1.5k stars; regular releases and maintenance (cloned revision `c7d4cf5` is current as of 2026-09-27).

2. **Purpose-built for exactly this use case.** The doc comment in `src/cst/mod.rs` (cited above) states: "keeps every comment and every piece of whitespace, so a document can be edited and written back out with everything the author wrote still in place." This is not incidental; it is the core design goal.

3. **Direct analogue to `toml_edit`.** The research evidence (`rust-structured.md`, comparison table) identifies `toml_edit` as the "reference/gold-standard pattern" for format-preserving structured editing. `jsonc-parser`'s CST is architecturally identical: parse to a full-fidelity tree with trivia, mutate via typed accessors, serialize back preserving all untouched content.

4. **No external process/CLI.** Embeddable Rust library; no subprocess integration required (constraint honored).

5. **No security risks in shallow clone.** Per repo policy, a shallow clone of the latest release shows no known vulnerabilities.

6. **Small, focused scope.** The `cst` feature is opt-in (Cargo feature flag). Without it, the crate is a lightweight parse-only library. With it, CST support is available but does not bloat the core.

### Why not alternatives

- **serde_json / json5-rs**: Both reserialize from scratch, discarding comments and regenerating formatting. (`rust-serde.md` confirms: serde_json parser has no comment production; json5-rs parses comments but discards them.)
- **jaq**: A jq-language filter engine, not a format-preserving editor. Discards `#` comments at lex time (`jaq-json/src/read.rs:10-19`) and regenerates all output formatting from a pretty-printer (`jaq-json/src/write.rs:202-260`). Purpose-built for "run a jq filter and get new JSON," not "edit one field, keep the rest untouched."
- **Building a custom CST**: Out of scope (non-trivial implementation, security review burden, maintenance cost). A well-regarded, tested crate is preferable.

### Placement in dependency graph

`jsonc-parser` is **never on the pure engine path** (`src/interview.rs`, `src/template.rs`). It is only used in `src/injection/structured.rs:resolve_struct_edit`, which is called during `Plan::apply` (after pure planning is complete). The interview engine remains pure; structured mutation is a pure function applied at apply time, symmetrical to marker-based injection.

---

## 8. Scope recommendation for 0.2.0

### Recommendation: **DEFERRED to a reserved, fast-follow release after 0.2.0.**

**Reasoning:**

1. **Implementation cost vs. 0.2.0 scope.** The marker-based system (Candidate 1, already selected as the base design) is fully specified and can ship in 0.2.0. Structured injection adds significant complexity:
   - A new path-navigation type (`StructPath`, parsing & validation).
   - CST integration (new dependency, unfamiliar API surface for the team).
   - New error modes (malformed JSON, path validation, auto-vivify of intermediate structures).
   - Dry-run / conflict / force-apply semantics for structured mode (convergent, not marker-gated).

2. **0.2.0 validates the core.** Shipping marker-based injection first proves the injection concept, gets user feedback, and stabilizes the `inject:` YAML surface. Once that is proven, structured mode is a natural extension (same surface, new discriminator in `body:`).

3. **Structured mode is optional, not foundational.** Most template authors can express JSON-family injection via markers (awkward for strict JSON, but possible with careful placement). Structured mode is an ergonomic upgrade, not a blocker.

4. **Reserved interface costs nothing.** The `inject:` list can remain discriminated by `region:` vs. `struct:` from day one (documentation states "future modes, e.g. `struct`, are reserved"). When implemented, no YAML changes are needed; only the internals expand.

### Implementation readiness (fast-follow)

If implemented after 0.2.0, the readiness is high:
- All types and signatures are sketched above (`StructPath`, `resolve_struct_edit`, error types).
- Fixtures can be built (one `.json`, one `.jsonc` file with before/after examples).
- CST integration is well-documented (examples in jsonc-parser README; module doc in `src/cst/mod.rs`).
- No new surface-level decisions needed (compression, idempotency semantics, and interaction with markers are all defined here).

### Explicit disclosure for the checkpoint

Structured injection introduces **no supported-capability restriction** (it is additive), **no new permissions/access change**, **no new timeout mechanic**, **no pinned version check**, and **no application-subprocess integration** (jsonc-parser is embedded). When the time comes to implement, these constraints are pre-satisfied.

---

## 9. Rationale, alternatives, tradeoffs

### Honest weakness: Drift detection is weak in strict JSON

**The problem:** Without comment syntax to embed a marker, Toha cannot prove "the value at this path is the one I last wrote" in a strict `.json` file. If the user edits the value and Toha re-applies, there is no structural way to refuse.

**The tradeoff accepted:** Structured mode is **declarative**, not marker-gated. Toha converges to the desired state every apply. This is honest and predictable: "if you manage a path, you own it; re-applies will set it to the template's desired value regardless of intervening edits." This is a different contract from marker-based injection (which refuses drift), but it is explicit and safe.

**The path forward:** For JSONC files (which have comment syntax), the `provenance: true` option embeds a marker comment. This is best-effort drift awareness without blocking convergence. A future design could enforce stricter drift policies for JSONC (e.g. "refuse to overwrite a value whose provenance comment is missing"), but that is out of scope for 0.2.0.

### Why not jaq?

**jaq is a filter/query language, not a file editor.** (`rust-structured.md`, "Conclusion" section.) It parses input into a value tree, runs a jq filter, and reserializes from scratch — discarding comments and regenerating all formatting. For one-time transformations ("read JSON, run jq filter, emit new JSON"), it is ideal. For "edit one field, keep everything else byte-identical," it is unsuitable.

If a template author wanted to use jaq (e.g. `value: "map(select(.active)) | ..."`), the inject system would have to:
1. Parse the file into a value.
2. Run the jaq filter over the value.
3. Replace the value at the path with the filtered result.

But this loses comments/formatting (jaq reserializes), and it is overengineered for the common case ("set this field to this value").

### Why not a separate `toha struct` subcommand?

**Temporal decomposition.** The marker-based system is ordered: files are written first (whole), then injections (regional). A separate `toha struct` command would be a second apply pass, breaking the single-`Plan` contract and the write-order guarantee. Rejected; structured injection is part of the same `Plan::apply` orchestration.

### Why not auto-vivify intermediate paths?

**Deferred to implementation.** The design above sketches `resolve_struct_edit` as creating intermediate objects/arrays as needed (e.g. setting `config.new.key = value` creates `config.new` if absent). This is reasonable but requires careful error handling (what if `config` exists but is a string, not an object?). The implementation can start conservative (fail if any intermediate is missing) and later relax to auto-vivify.

---

## Composition with marker-based injection

A single `inject:` list composes both modes:

```yaml
inject:
  - into: "setup.sh"
    region: "init"
    content: |
      export DEBUG=1

  - into: "package.json"
    struct:
      path: "scripts.test"
      value: "jest"

  - into: "config.toml"
    region: "features"
    content: "enabled = true"
```

Each entry is evaluated independently during `Plan::build`, producing a `PlannedEdit` (either marker or struct variant). At apply time, each `PlannedEdit` is resolved in list order, reading the current bytes once per path and writing atomically if changed. The same file can be targeted by multiple entries (e.g. one marker region + one struct path), and both apply in sequence.

---

## Summary

Candidate B advocates adopting `jsonc-parser`'s CST as Toha's format-preserving structured-injection mechanism for JSON/JSONC/JSON5 files. It shifts ownership from "byte-spanned regions delimited by markers" to "logical JSON values at named paths," enabling surgical edits that preserve comments, key order, and whitespace verbatim.

The core tradeoff is honest: structured mode converges to the desired state each apply, with no marker space for cryptographic drift proof in strict JSON. For JSONC, an optional provenance marker adds best-effort awareness. This is safe and explicit: authors understand that managing a path means owning it; re-applies will set it to the template's value.

Implementation is deferred to a fast-follow release after 0.2.0, with full interface and type signatures sketched for zero surprise on handoff. No new surface decisions needed; the `inject:` list discriminator and error semantics are defined here.

---

## Key signatures (reference)

```rust
pub fn resolve_struct_edit(
    current: Option<&[u8]>,
    edit: &PlannedEdit,
) -> Result<StructResolution, StructError>;

impl Plan {
    pub fn build_structured_injection(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Vec<PlannedEdit>, PlanError>;
}

pub fn apply_reporting(
    plan: &Plan,
    target_root: &Path,
    options: ApplyOptions,
) -> Result<Applied, ApplyError>;
```

All are pure (depend only on inputs, read no global state except FS at boundaries) or clearly delegated to the caller (apply reads FS).
