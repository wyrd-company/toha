# Candidate A: Embedded `jaq` Structured Mode

Design fragment for JSON-family structured injection in Toha 0.2.0 content-injection feature. Champion direction: adopt the `jaq` crate (jaq-core + jaq-json, embedded library, no subprocess) as a mechanism for structured mutation of JSON-family files.

## 1. Author Surface

A new optional `structured` sub-block within each `inject` rule, specifying a jq-style filter for the target file. Mutually exclusive with the marker-based `content`/`source` body.

```yaml
inject:
  - into: "package.json"
    region: "dev-dependencies"
    structured:
      format: json                           # explicit format (json, jsonc, json5)
      filter: '.devDependencies."toha" = "^0.2.0"'
      # filter is a jq program that produces a new root value
      # if filter produces null, the whole file is left untouched
    when: "include_dev_dependencies"

  - into: "src/config.jsonc"
    region: "api-endpoint"
    structured:
      format: jsonc
      filter: '.api.endpoint = "{{ api_url }}"'
      create: true                          # allow creating if missing
    anchor: { after: "// === CONFIG SECTIONS ===", occurrence: only }
    # Note: anchor is IGNORED for structured mode; structural format-merge 
    # replaces the WHOLE file, not a bounded region.
```

**Key surface decisions:**

- `filter` is a jq-language string; authors must learn jq syntax for non-trivial merges.
- `format` is mandatory for structured mode; unknown extensions are a build error.
- `create: true` creates the file with an empty object `{}` or array `[]` (inferred from filter); unknown structure is an error.
- Structured mode is **mutually exclusive** with the marker-based `content`/`source` in the same rule; choosing one syntax automatically activates it.
- Structured mode **ignores anchors** (the filter runs over the whole document; there is no bounded region).
- `when`/`each` conditions work normally; each iteration applies the filter independently.

## 2. Concrete BEFORE/AFTER Examples

### Example 1: `.json` file with formatted structure (NO comments)

**Template config:**
```yaml
inject:
  - into: "src/config.json"
    region: "api-config"
    structured:
      format: json
      filter: '.api.timeout = 5000 | .api.retries = 3'
```

**BEFORE (user-created file with hand-chosen key order and spacing):**
```json
{
  "server": {
    "port": 3000,
    "host": "localhost"
  },
  "api": {
    "baseUrl": "http://localhost:8080"
  }
}
```

**AFTER first apply:**
```json
{
  "server": {
    "port": 3000,
    "host": "localhost"
  },
  "api": {
    "baseUrl": "http://localhost:8080",
    "timeout": 5000,
    "retries": 3
  }
}
```

**Key observation:** jaq's pretty-printer (`jaq-json/src/write.rs:202-260`, `format_val!` macro) regenerates all whitespace from scratch. The source spacing (which was 2 spaces) is preserved *by chance* because that's the default, but key order within `api` is regenerated: new keys are appended, but existing key order is deterministic because `jaq-json/src/lib.rs:117` uses `IndexMap` which preserves insertion order in memory.

**AFTER second apply (same template, same answers):**
```json
{
  "server": {
    "port": 3000,
    "host": "localhost"
  },
  "api": {
    "baseUrl": "http://localhost:8080",
    "timeout": 5000,
    "retries": 3
  }
}
```

**Idempotency:** Unchanged. No bytes differ; the write is skipped.

---

### Example 2: `.jsonc` file with comments (COMMENTS LOST)

**Template config:**
```yaml
inject:
  - into: "tsconfig.json"
    region: "compiler-options"
    structured:
      format: jsonc
      filter: '.compilerOptions.strict = true | .compilerOptions.target = "ES2020"'
```

**BEFORE (user file with developer comments and deliberate spacing):**
```jsonc
{
  // TypeScript compiler options for the project
  "compilerOptions": {
    // Module resolution strategy
    "moduleResolution": "node",
    "lib": ["ES2020", "DOM"],

    // Strictness checks — enable to catch more errors
    "esModuleInterop": true
  },
  "include": ["src/**/*.ts"]
}
```

**AFTER first apply:**
```jsonc
{
  "compilerOptions": {
    "moduleResolution": "node",
    "lib": [
      "ES2020",
      "DOM"
    ],
    "esModuleInterop": true,
    "strict": true,
    "target": "ES2020"
  },
  "include": [
    "src/**/*.ts"
  ]
}
```

**What was lost:**
- **All comments are gone.** jaq's lexer (`jaq-json/src/read.rs:10-19`) unconditionally scans and **discards** `#` comments; they never enter the `Val` tree.
- **Original spacing is lost.** The array `["ES2020", "DOM"]` was written on one line by the user; the output reformats it across multiple lines per jaq's `PrettyPrinter`.
- **Key order is renormalized.** New keys (`strict`, `target`) are appended to `compilerOptions`; old keys retain insertion order, but the entire output structure is regenerated.

**AFTER second apply:**
```jsonc
{
  "compilerOptions": {
    "moduleResolution": "node",
    "lib": [
      "ES2020",
      "DOM"
    ],
    "esModuleInterop": true,
    "strict": true,
    "target": "ES2020"
  },
  "include": [
    "src/**/*.ts"
  ]
}
```

**Idempotency:** Converges to the same bytes. The filter is deterministic, so re-applying produces byte-identical output.

---

### Example 3: Drift / user edits after jaq reserialize

**Scenario:** User manually edits the `compilerOptions` inside the reserialize boundaries after first apply.

**File after apply + user edits:**
```jsonc
{
  "compilerOptions": {
    "moduleResolution": "node",
    "lib": [
      "ES2020",
      "DOM"
    ],
    "esModuleInterop": true,
    "strict": true,
    "target": "ES2020",
    "allowJs": true  // <-- USER ADDED THIS
  },
  "include": [
    "src/**/*.ts"
  ]
}
```

**Second apply with same template:**

Since structured mode is a **whole-file transform**, there is no bounded "managed region" to check for drift in the marker sense. The question becomes: **is the on-disk file equal to the desired jaq-filter output, yes or no?**

- **Option A (conservative/no-rewrite-on-edits):** Compute the jaq-filter output. If it differs from the on-disk file, treat the difference as "drift" (user edits outside the managed part, or pre-existing structure changed). With `--force=false`, refuse the apply. With `--force=true`, rewrite the whole file with the filter output (discarding the user's `allowJs` addition).

- **Option B (aggressive/final-filter-output-wins):** Always rewrite if the filter output differs. The user's edits are simply lost; the filter is the source of truth.

**Recommendation in this design: Option B (final-filter-output-wins).** Reason: the whole-file rewrite is already the core contract; pretending there is a "managed region" when the filter touches arbitrary keys is misleading. The trade-off is explicit: using structured jaq mode means the filter output is authoritative, and user hand-edits to the file are temporary and will be overwritten on next apply.

---

## 3. Ownership / Drift / Idempotency Model

### Ownership

**The hard truth:** Structured merge via jaq is a **disguised whole-file rewrite** of a user-owned file.

- The file is owned by the user (it exists before Toha's first apply).
- Toha writes the entire file after running the jq filter.
- Comments, spacing, and relative key order are not preserved; they are reconstructed from scratch.
- This violates the stated principle in the base design (`05-design.md:366-391`) that "Toha writes only the bounded region between markers" — here Toha writes the whole file, reserializing it in the process.

**When is this acceptable?**

1. **For strict JSON (`.json`).** JSON has no comments, so the only loss is spacing/order. If the user's file does not rely on specific indentation patterns or key order, a reserialize is low-risk.
2. **For JSON files with indent/style that can be detected and threaded back** (npm's `package.json` pattern, `/tmp/injection-research/cross-lang.md:207-276`). npm's `json-parse-even-better-errors` sniffs the original file's indent and newline, stores them on the parsed object, and re-applies them on reserialize — a pragmatic compromise.
3. **For JSONC / JSON5 with *no developer-visible comments*.** If developers use `.jsonc` syntax (trailing commas, unquoted keys) but don't include comments that document the structure, reserialize is low-risk.

**When is it NOT acceptable?**

1. **For `.jsonc`/`.json5` files with comments that document intent.** Discarding comments is data loss. A developer's comment explaining "this regex matches emails" is gone after Toha's reserialize.
2. **For files where key order is semantically significant.** Some tools (e.g., ESLint config chaining) depend on rule priority via key order. Reserialize may change the order.
3. **For files that must survive diffs and version control.** A whole-file reserialize produces a large diff even if only one field changed, making review and blame harder.

### Idempotency Contract

**Re-apply converges because the jq filter is deterministic.**

```
idempotency_property: resolve_structured(current_bytes, filter) == 
                      resolve_structured(resolve_structured(current_bytes, filter), filter)
```

The filter is a pure function over a `Val` tree; applying it twice yields the same result because the filter logic doesn't depend on external state or file state — only on the structure of the input.

**No drift detection in the marker sense.** Since the whole file is the "managed region," there is no isolated checksum or marker to detect whether the user edited it. Options:

- **Whole-file byte diff:** On re-apply, compute the desired jaq output. If it differs from the on-disk file, assume something changed (user edits or filter changed). Report the diff and refuse without `--force`.
- **No diff, final-output-wins:** Always rewrite if the filter produces a different result; user edits are clobbered.

This design recommends the second (final-output-wins) as more explicit: the jaq filter is the source of truth for that file's structure.

### Composition with Markers (Hybrid)

Structured mode and marker-based mode could coexist for **different regions of the same file**, but this design recommends **against it within a single file**:

```yaml
# NOT RECOMMENDED:
inject:
  - into: "package.json"
    region: "scripts"
    content: '"test": "jest"'       # marker-based

  - into: "package.json"
    region: "deps"
    structured:
      filter: '.dependencies."lodash" = "^4.17.0"'  # jaq-based
```

**Why?** Mixing introduces ambiguity:

- The marker region owns only its bounded span and coexists peacefully with user edits elsewhere.
- The jaq filter owns the entire file structure after it runs.
- After first apply with markers + jaq in the same file, the file has both marker comments (from the marker rule) and whole-file reserialize (from jaq). A second apply becomes unclear: does jaq see the marker comments it just wrote, or are they already discarded during its lex pass?

**Simpler contract:** One file uses **either** markers **or** structured jaq, not both. If a template needs both (e.g., "inject a script entry via marker, and update dependencies via jaq"), split into two files or use only jaq and accept the whole-file reserialize.

---

## 4. Data / Type Sketch

```rust
// src/template.rs — new domain types

pub struct StructuredMerge {
    pub format: JsonFamily,           // enum: Json, Jsonc, Json5
    pub filter: String,               // jq filter program (validated at build)
    pub create: Typed<bool>,          // create file if missing? (default false)
}

pub enum JsonFamily { Json, Jsonc, Json5 }

pub enum BodySource {
    Marker { content: BodySource, marker: MarkerSpec },  // existing marker-based
    Structured(StructuredMerge),                         // NEW: jaq-filter based
}

// OR: separate field to avoid ambiguity
pub struct InjectionRule {
    pub into: Tmpl,
    pub region: Tmpl,
    pub body: Option<BodySource>,              // marker-based (content or source)
    pub structured: Option<StructuredMerge>,   // jaq filter-based
    // ... remaining fields: anchor, marker, when, each
}
// Build-time validation: exactly one of `body` or `structured` must be present.

// --- Plan types ---

pub enum PlannedMutation {
    Marker(PlannedEdit),        // existing marker-based edit
    Structured(PlannedStructured),  // NEW
}

pub struct PlannedStructured {
    pub path: TargetPath,
    pub region: RegionKey,      // for reporting/grouping, though the whole file is owned
    pub filter: jaq_core::Filter,  // compiled jq program
    pub format: JsonFamily,
    pub create: bool,
}

pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub edits: Vec<PlannedMutation>,  // mixed markers + structured
}

// --- Apply / resolve ---

pub enum Resolution {
    Unchanged,
    Write(Vec<u8>),
}

pub fn resolve_structured(
    current: Option<&[u8]>,
    edit: &PlannedStructured,
    jaq_filter: &jaq_core::Filter,
) -> Result<Resolution, StructuredError> {
    // 1. If file is absent:
    //    - If !create, return Err(StructuredError::TargetMissing)
    //    - Else, start with empty {} for objects or [] for arrays
    //       (inferred from filter result type / user hint)
    //
    // 2. Parse input with the appropriate parser (json, jsonc, or json5):
    //    - Unknown format with format == Json => strict-JSON only
    //    - format == Jsonc => jsonc-parser in lax mode (DISCARDS comments)
    //    - format == Json5 => json5-rs (DISCARDS non-structural syntax)
    //
    // 3. Apply jaq filter over the parsed jaq_json::Val:
    //    let input_val = parse_to_jaq_val(...)?;
    //    let output_val = jaq_filter.apply(input_val)?;
    //
    // 4. Serialize output via jaq-json::write::write_buf:
    //    let output_bytes = jaq_json::to_vec(&output_val)?;
    //
    // 5. Compare: if output_bytes == current, return Unchanged.
    //    Else return Write(output_bytes).
    //
    // No checksum, no marker search, no drift detection — just recompute and compare.
    todo!("not implemented")
}

pub enum StructuredError {
    TargetMissing { path: TargetPath, region: RegionKey },
    NotUtf8 { path: TargetPath },
    ParseError { path: TargetPath, why: String },
    FilterError { path: TargetPath, region: RegionKey, why: String },
    InvalidFilter { why: String },  // at build time, jaq compile failure
}
```

**Build-time:**

```rust
// During Plan::build, for each structured rule:
pub fn build_structured_edit(
    rule: &InjectionRule,
    completed: &Completed,
    target: &Path,
) -> Result<PlannedStructured, PlanError> {
    // 1. Render rule.region to get the stable key (RegionKey)
    let region_key = render_region_key(&rule.region, completed)?;
    
    // 2. Render and validate rule.structured.filter as a jq program
    let filter_src = render_template(&rule.structured.filter, completed)?;
    let compiled_filter = jaq_core::compile(&filter_src)
        .map_err(|e| PlanError::InvalidFilter(e.to_string()))?;
    
    // 3. Resolve target path (TargetPath::parse, check symlinks, `.git`)
    let path = TargetPath::parse(&rendered_into, target)?;
    
    // 4. Check for duplicates: no two rules to the same (path, region) pair
    
    Ok(PlannedStructured {
        path,
        region: region_key,
        filter: compiled_filter,
        format: rule.structured.format,
        create: rule.structured.create.resolve(completed)?,
    })
}
```

---

## 5. Error / Results Contract

### Build-time errors

- `InvalidFilter { why: String }` — jaq filter does not compile (syntax error). Exit 1, names the filter and error.
- `PlanError::Render` — region key or filter string fails to render. Existing error path reused.
- `PlanError::Path` — target path fails parse. Existing error path reused.
- `PlanError::Duplicate` — two structured edits to the same (path, region). Existing error path reused.

### Apply-time errors

- `TargetMissing` — file does not exist and `create: false`. Exit 1, names file and region.
- `NotUtf8` — file exists but is not valid UTF-8. Exit 1.
- `ParseError` — input is not valid JSON/JSONC/JSON5 (per format). Exit 1, names file, format, parse error.
- `FilterError` — jaq filter runtime error (e.g., applying `.foo` to a null value if filter is strict). Exit 1, names file and region.

### Dry-run

- `toha apply --dry-run <PATH>` extends vocabulary with:
  - `inject <path> (<region>)` — first structured apply (file was absent, just created).
  - `update <path> (<region>)` — structured apply that changed the file.
  - Unchanged structured applications print nothing (idempotent).

### `--force` and `--dry-run` interaction

- `--force` does **not** apply to structured mode; there is no "drift" concept (the filter is always authoritative).
- If `--force` is passed and a structured edit fails (parse error, filter error), it still fails; `--force` only pardons whole-file conflicts.
- Structured mode never refuses an apply based on user edits (it simply overwrites the whole file).

### Atomicity

Each structured apply writes to a sibling temp file and is renamed into place (atomic, same as markers). A crash mid-apply leaves the temp file but not a half-written user-owned file.

---

## 6. Format / Comment / Order Preservation — Honest Assessment

| Aspect | Preserved? | Evidence |
|---|---|---|
| **Comments** | **NO — unconditionally lost.** | jaq-json lexer (`jaq-json/src/read.rs:10-19`) scans `#` comments and **discards them**; never enter the `Val` tree. No CST, no trivia tracking. |
| **Whitespace / indentation** | **Partially. Regenerated.** | jaq's pretty-printer (`jaq-json/src/write.rs:202-260`, `format_val!` macro) emits fresh JSON from the `Val` tree using fixed indent rules (`Pp` struct, `indent` field). Default 2 spaces; configurable but output is fully reconstructed, not byte-for-byte preserved from input. |
| **Key order (insertion order)** | **Preserved in memory; output normalized.** | `jaq-json` uses `IndexMap` (`jaq-json/src/lib.rs:117`) which preserves insertion order. But output key order is deterministic per the filter + the in-memory order at write time; if the filter modifies keys or values are added programmatically, order follows jaq's construction order, not the original file's. |
| **Trailing commas / JSON5 syntax** | **NO — lost on parse if format != json5.** | jaq treats JSON5/JSONC syntax leniently during parse (accepts trailing commas, unquoted keys, single quotes) because `jaq-json`'s reader extends JSON5-style input. But on output, `format_val!` emits strict JSON (no trailing commas, double quotes, etc.) regardless of input syntax. |
| **Overall byte similarity** | **NO — expect large diffs.** | Even a single-field change produces a whole-file reserialize. Line breaks, spacing, and comment blocks are regenerated. A file may *semantically* be 99% identical but *textually* completely different. |

**Verdict:** jaq is a **reserializer**. It is not format-preserving in the sense of `jsonc-parser`'s CST (`jaq-json/src/lib.rs:46-67` vs. `/tmp/injection-research/rust-structured.md:62-79`) or `toml_edit` (`/tmp/injection-research/rust-structured.md:99-121`). Use it only when whole-file reserialize is acceptable.

---

## 7. Dependency Justification

### Crate: `jaq` (jaq-core + jaq-json, version 3.1.1 / 2.0.3)

**Where it sits:** On the apply path only, never in the pure interview engine or build-time pure functions (filter compilation happens at build, not engine execution). It is an embedded library, not a subprocess.

**Why adopt vs build:**

- **Well-regarded.** jaq is a mature Rust jq clone, has 1000+ GitHub stars, is regularly maintained (last commit 2026-08-28, `/tmp/injection-research/rust-structured.md:7`), and is notably used in Rust tooling.
- **Actively maintained.** The workspace crates (jaq-core, jaq-json, jaq-std) have consistent versioning and recent releases. No security issues in a shallow clone scan.
- **Best-in-class jq implementation in Rust.** No other Rust crate offers a complete, embedded jq interpreter with multi-format support. Building would duplicate a large, non-trivial compiler/interpreter.

**Cost-benefit:**

- **Benefit:** Out-of-the-box jq language support; authors can leverage their jq knowledge.
- **Cost:** Medium-size dependency (~100kB binary growth); adds jaq-core's parser/interpreter complexity. Structured mode is opt-in (templates can still use only markers), so the cost is paid only when structured rules are present.
- **Justification:** The cost is acceptable because structured merge is an optional feature, not the core, and jaq is the ecosystem standard for jq-language evaluation in Rust.

**No subprocess.** jaq-core is embedded; no `std::process::Command` spawn. Complies with the "no application subprocess/CLI integration without explicit approval" constraint.

---

## 8. Scope Recommendation

### Should this ship in Toha 0.2.0 (paired impl 1031) now, or be decided but deferred?

**RECOMMEND: DECIDE NOW, BUT DEFER IMPLEMENTATION to 0.3.0 or later.**

**Reasoning:**

1. **Conceptual readiness:** The design is solid and can be reviewed/approved now. The implementation is straightforward (compile jaq filter at build, apply it at resolve-time, byte-compare). No unknown unknowns.

2. **But time/scope pressure:** 0.2.0 is the "first slice" and already ships the full marker-based system (which works for all text files, including JSON). Structured jaq mode is value-add, not essential. Adding it now risks:
   - Scope creep in 1031 (implementation paired with markers, same code review/test cycle).
   - Delaying marker-mode shipping if jaq integration discovers issues.
   - Embedding a large dependency (jaq-core) when markers alone are sufficient.

3. **Structured mode is a clear win later, but markers suffice now.** A template author can use the marker-based injection on `package.json`, `tsconfig.json`, etc. right now. Structured mode is a polish (no reserialize, no marker comments in the file), not a blocker.

4. **Risk of commitment.** Shipping structured mode in 0.2.0 commits the project to supporting jaq for all time (upgrade costs, API stability, bug fixes). Deferring lets the community use markers first, validate the core design, and adopt jaq as a 0.3 opt-in refinement.

5. **Interface boundary is clear.** The `StructuredMerge` / `PlannedStructured` types are orthogonal to markers. Shipping now means carrying both in the schema and codebase; deferring means adding them in a follow-up design 1068b/1031b.

### If deferred, what interface reservation is needed?

In `template-format.schema.yml` and `PlannedMutation`, reserve a branch for `StructuredMerge` / `PlannedStructured` but mark it as "future" or gate it behind a feature flag so early adopters don't accidentally rely on it.

```rust
pub enum PlannedMutation {
    Marker(PlannedEdit),
    #[cfg(feature = "experimental-structured")]
    Structured(PlannedStructured),
}
```

This keeps the design decision on record without shipping the code.

---

## 9. Rationale, Alternatives, Tradeoffs

### The Core Tradeoff: Format Preservation vs. Simplicity

| Axis | Marker-Based | Structured jaq |
|---|---|---|
| **Comment preservation** | Yes (text-based markers in comments) | No (reserialize) |
| **Formatting preservation** | Yes (marker replaces only a region) | No (entire file rebuilt) |
| **Key order preservation** | Yes (untouched parts unchanged) | Partial (in-memory order preserved, output normalized) |
| **User file impact** | Minimal (only marked region rewritten) | Maximum (whole file rewritten) |
| **Diff size on re-apply** | Small (only the changed region) | Large (can be entire file) |
| **Language complexity** | Simple (plain text body) | Medium (jq filter syntax) |
| **Applicability** | Any text file (`.rs`, `.toml`, `.md`, `.json`, `.jsonc`) | Structured formats only (JSON/JSONC/JSON5/TOML/YAML) |
| **Idempotency guarantee** | Structural (marker hash + regex anchor) | Functional (jq filter is deterministic) |

**Honest assessment of the biggest weakness:**

Using structured jaq mode means **accepting a disguised whole-file rewrite of a user-owned file**. This is the load-bearing problem that this candidate must confront. The justification for accepting it is:

1. **For strict JSON (no comments).** Comments don't exist, so no data loss. Indent/formatting loss is acceptable if the file is machine-generated (e.g., npm's `package.json` approach; cite `/tmp/injection-research/cross-lang.md:207-276`).

2. **For JSONC/JSON5 without developer-visible comments.** If the file uses JSONC syntax (trailing commas, unquoted keys) but not comment documentation, reserialize is low friction.

3. **Explicit tradeoff.** The author chooses structured mode; they accept the rewrite. The alternative is markers, which preserves the file.

**When the tradeoff fails:**

The mechanism breaks down for files that depend on preserved comments, formatting, or key order (e.g., ESLint configs, `tsconfig.json` with documentation comments, any `.jsonc` file that is version-controlled and code-reviewed). In these cases, markers are superior: they change only the target region, leaving developer intent intact.

**Alternative: jsonc-parser CST mode?**

A future candidate could adopt `jsonc-parser`'s `cst` module (`/tmp/injection-research/rust-structured.md:53-96`) for format-preserving JSON family editing. This would:

- Preserve comments, spacing, key order (full fidelity, like `toml_edit`).
- Require a per-format CST parser + mutation API (more implementation work).
- Be applicable to JSONC/JSON5, but strict JSON needs no CST (markers are simpler).

This candidate does NOT pursue that path; it accepts the reserialize cost as the price of simplicity.

---

## Summary

Structured merge via embedded `jaq` is a **pragmatic, well-justified mechanism for JSON-family injection IF the user accepts whole-file reserialize.** The jq filter language is expressive and lowers the learning curve for authors with jq experience. Idempotency is guaranteed by filter determinism. 

The mechanism's **single biggest weakness** is that it is a **disguised whole-file rewrite of a user-owned file**, with all the consequences: loss of comments, reformatted spacing, and potentially altered key order. This is acceptable for strict JSON and machine-generated JSONC; it is not acceptable for developer-facing JSONC files that carry documentation comments or rely on key order.

**Scope recommendation:** Approve the design now for future implementation, but defer shipping to 0.3.0. 0.2.0 delivers the full marker-based system, which is sufficient for all text files. Structured jaq mode is a polish that can be added as an opt-in feature after markers are validated in the wild.

---

## Signatures / Evidence

- **jaq architecture:** `/workspaces/references/toha/jaq/jaq-core/src/val.rs:55`, `/workspaces/references/toha/jaq/jaq-json/src/lib.rs:46-67,133,301,476-480`
- **jaq comment handling:** `/workspaces/references/toha/jaq/jaq-json/src/read.rs:10-19`
- **jaq reserialize:** `/workspaces/references/toha/jaq/jaq-json/src/write.rs:202-260,302,321,796-800`
- **jaq IndexMap preservation:** `/workspaces/references/toha/jaq/jaq-json/src/lib.rs:117`
- **Format support:** `/workspaces/references/toha/jaq/jaq-fmts/src/lib.rs:54,59-79`
- **npm precedent (reserialize + preserve-indent):** `/tmp/injection-research/cross-lang.md:207-276`, `/workspaces/references/toha/npm-package-json/lib/index.js:239-264`, `/workspaces/references/toha/json-parse-even-better-errors/lib/index.js:3-4,99-113`
- **jsonc-parser CST (format-preserving alternative):** `/tmp/injection-research/rust-structured.md:53-96`, `/workspaces/references/toha/jsonc-parser/src/cst/mod.rs:1-5,6-33,1146,1202,1212`
- **Comparison table:** `/tmp/injection-research/rust-structured.md:124-135`
