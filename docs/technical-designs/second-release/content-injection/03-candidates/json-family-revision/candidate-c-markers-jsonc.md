# Candidate C: Comment-Marker Injection for JSON-Family via JSONC/JSON5

**Champion direction:** No structured engine. JSON-family handled by the existing marker mechanism, which extends uniformly to `.jsonc` and `.json5` via comment-delimited markers. For strict `.json` (no comment support), define the honest documented boundary: marker injection is a non-goal; target a `.jsonc` instead, manage the whole file, or defer to a future structured mode.

---

## 1. Author surface (template.yml)

Injection declarations are unchanged from the base design (`05-design.md`); the difference is *which* file formats are supported and *how*.

```yaml
name: example-service-config
interview:
  - id: enable_metrics
    type: confirm
    prompt: Enable metrics collection?
    default: true
  - id: environment
    type: select
    prompt: Environment
    options: [dev, staging, prod]

files:
  - source: support/app.rs
    path: "src/app.rs"

# Injection rules. Same syntax; behavior differs by target file type.
inject:
  - into: "config/app.jsonc"       # <- JSONC (comments allowed)
    region: "database"
    when: "enable_metrics"
    content: |
      // Database config
      "db_host": "{{ db_host }}",
      "db_pool": 32
    # marker inferred from .jsonc extension -> comment family '//'

  - into: ".gitignore"              # <- arbitrary text (already works)
    region: "artifacts"
    content: |
      /build
      *.log

  - into: "package.json5"           # <- JSON5 (comments allowed)
    region: "scripts"
    content: "\"custom-build\": \"npm run build && npm run dist\""

  - into: "strict-config.json"      # <- STRICT JSON (no comments allowed)
    # This injection CANNOT use comment markers.
    # Author must choose: target a .jsonc version, or manage the whole file.
    # See Scope Recommendation section.
    region: "auth"
    create: false
    # (This entry would raise a build error or runtime refusal.)
```

**Authority:** The extension → comment-family table (reusing the existing `resolve_marker_style` logic from `05-design.md`) determines whether markers can be inserted:

- `.jsonc`, `.json5`: comment-capable → markers work as `//` lines
- `.json`: comment-incapable → markers **cannot** be inserted; build-time refusal or runtime skip, per decision below

---

## 2. Concrete BEFORE/AFTER examples

All examples use JSONC (`.jsonc`) to show the marker mechanism's full behavior.

### Example: First apply (markers inserted)

**template.yml:**
```yaml
inject:
  - into: "config/app.jsonc"
    region: "features"
    content: |
      "analytics_enabled": true,
      "analytics_sample_rate": 0.1
```

**Target BEFORE (config/app.jsonc):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info"
}
```

**Target AFTER (first apply):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d... <<<
}
```

**Key observations:**
- Markers are JSONC comment lines (`//`), so the file remains valid JSONC
- The injected body is placed at end of file (no anchor specified)
- End marker records a SHA256 checksum of the body Toha wrote
- User's existing comments (`"log_level": "info"`) are untouched

### Example: Second apply with same answers (idempotent)

**template.yml:** (unchanged)

**Target BEFORE (same as AFTER above — file not edited by user):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d... <<<
}
```

**Target AFTER (second apply):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d... <<<
}
```

**Result:** No bytes changed. Exit code 0. Region listed as "unchanged" in `--dry-run` output. The sha256 hash in the end marker identifies the body as identical, so `resolve_edit` returns `Resolution::Unchanged`.

### Example: Second apply with changed template (update)

**template.yml (changed):**
```yaml
inject:
  - into: "config/app.jsonc"
    region: "features"
    content: |
      "analytics_enabled": true,
      "analytics_sample_rate": 0.15
      // (sample rate increased from 0.1 to 0.15)
```

**Target BEFORE (from previous apply):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d... <<<
}
```

**Target AFTER:**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.15
  // <<< toha:end features sha256:c9e2b... <<<
}
```

**Result:** Region replaced. Only the bytes between (and including) the markers changed. The checksum updated. User's `"log_level"` line untouched. Exit code 0.

### Example: User drift (edited inside markers)

**Target BEFORE (user manually edited inside the region):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": false,
  // User turned this off locally
  "analytics_sample_rate": 0.05
  // <<< toha:end features sha256:a7f3d... <<<
}
```

**Apply attempt (no --force):**

Toha:
1. Locates the region by exact marker lines (`// >>> toha:region features >>>` and `// <<< toha:end features ...`)
2. Extracts on-disk body: `"analytics_enabled": false,\n// User turned this off locally\n"analytics_sample_rate": 0.05\n`
3. Compares sha256 of that body against the recorded hash (`a7f3d...`)
4. Hashes do NOT match → drift detected
5. Refuses to overwrite, prints `region drifted: config/app.jsonc (features)`, suggests `toha apply --force`
6. Exit code 1

**Apply with --force:**

Same steps 1-4; at step 5, because `--force` is set, computes fresh sha256 over the new template body and rebuilds the region markers. File is written.

**Result AFTER (--force applied):**
```jsonc
{
  "app_name": "my-service",
  "port": 8080,
  // User's own config
  "log_level": "info",
  // >>> toha:region features >>>
  "analytics_enabled": true,
  "analytics_sample_rate": 0.1
  // <<< toha:end features sha256:a7f3d... <<<
}
```

User's edits were overwritten (the whole point of `--force`). No silent data loss — the drift was named and refusal was explicit.

---

## 3. Ownership and drift model

**Ownership boundary:**

Toha owns the byte span *between and including* the begin and end marker lines. Everything else in the file belongs to the file owner (the user). Precisely:

```
{line_0}
{line_1}
...
// >>> toha:region {key} >>>  <- Toha's byte 0 (begin marker)
{body_line_0}
{body_line_1}
...
// <<< toha:end {key} sha256:{hex} <<<  <- Toha's byte N (end marker)
{line_N+1}
...
```

**Idempotency guarantee:**

When `resolve_edit` computes `Write(bytes)`, the new bytes include a freshly-computed sha256 checksum of the body. Therefore, when the same edit is applied to the freshly-written bytes, the extracted on-disk body matches the template body, and `resolve_edit` returns `Resolution::Unchanged`. Double-apply without file owner edits = no-op. This holds across staging replay because `PlannedEdit` is a pure function of `(Template, Completed)`.

**Drift detection:**

At resolve time:
1. Locate the region by exact marker-line matches
2. Extract the on-disk body between the markers
3. Compute sha256 of that body
4. Compare against the sha256 recorded in the end marker
5. Match → safe to re-apply; mismatch → user edit (drift)

**JSON-aware concerns (comment order, key order, formatting):**

Because the injected body is inserted verbatim as text between markers, not parsed/reserialize:
- Comments *inside* the injected body are preserved (they're just text)
- The injected content's whitespace/formatting is preserved exactly
- Key order inside the injected body is preserved
- **But the surrounding file is unmodified** — if the user reordered keys in the rest of the file, that is outside Toha's region and is untouched

The file as a whole may not be valid JSON at intermediate edits (e.g., missing commas if an injection was partial or manual edit nearby). The design assumes the author and user maintain file-level validity together.

---

## 4. Data/type sketch (not implemented)

Reuses the base design's `PlannedEdit`/`resolve_edit` contract; the only detail specific to JSON-family is the comment inference and the test of whether markers can even be inserted.

```rust
// src/template.rs — new during build (comment family resolution)

fn resolve_marker_style(path: &TargetPath, spec: &MarkerSpec)
    -> Result<MarkerStyle, PlanError>
{
    // Infer from path extension, or use explicit `marker` override.
    // For JSON-family:
    let ext = path.extension().unwrap_or("").to_lowercase();
    match (spec, ext.as_str()) {
        // Explicit override always wins
        (MarkerSpec::Raw { open, close }, _)
            => Ok(MarkerStyle { open: open.clone(), close: close.clone() }),
        
        // Inferred from extension
        (MarkerSpec::Infer, "jsonc" | "json5" | "json")
            => {
                // .json5 and .jsonc both support // comments
                if ext == "json" {
                    Err(PlanError::CannotInferComment {
                        path: path.clone(),
                        reason: "strict .json has no comment syntax; \
                                 target a .jsonc or .json5 file instead, \
                                 or use create: false and manage the whole file"
                        .to_string(),
                    })
                } else {
                    // .jsonc and .json5
                    Ok(MarkerStyle { open: "//".into(), close: "".into() })
                }
            }
        
        // All other extensions follow existing logic (already in base design)
        (MarkerSpec::Infer, ext) => {
            // Delegate to existing family table
            resolve_from_extension_table(ext)
        }
    }
}

// src/plan.rs — build-time check, reuses existing logic

pub fn add_edit(&mut self, edit: PlannedEdit) -> Result<(), PlanError> {
    // Existing checks (symlink, duplicate key per path) already in base design
    
    // NEW: For marker injection into .json (strict), refuse at build time
    if edit.path.extension().unwrap_or("").to_lowercase() == "json"
        && edit.marker.close == "" // line-comment style
    {
        return Err(PlanError::StrictJsonCannotUseMarkers {
            path: edit.path.clone(),
            region: edit.region.clone(),
        });
    }
    
    // ... rest of existing add_edit logic
    Ok(())
}

// src/apply.rs — unchanged from base design

pub enum Resolution { Unchanged, Write(Vec<u8>) }

pub fn resolve_edit(current: Option<&[u8]>, edit: &PlannedEdit)
    -> Result<Resolution, EditError>
{
    // All logic from 05-design.md applies unchanged.
    // The file's bytes are bytes; markers are comment lines; JSONC/JSON5
    // parsers accept `//` comments natively.
    // Pure text splice, no JSON parsing.
    
    todo!("see base design for full implementation contract")
}
```

**Mutation contract (for 1066):**

```rust
pub enum FileMutation<'a> {
    Whole  { path: &'a TargetPath },
    Region { path: &'a TargetPath, region: &'a RegionKey },
}
impl Plan {
    pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>> {
        self.files.iter().map(|f| FileMutation::Whole { path: &f.path })
            .chain(self.edits.iter().map(|e| FileMutation::Region { 
                path: &e.path, 
                region: &e.region 
            }))
    }
}
```

---

## 5. Error and results contract

**New error cases specific to JSON-family:**

```rust
pub enum PlanError {
    // ... existing variants ...
    
    /// Strict .json file cannot use comment-delimited markers.
    StrictJsonCannotUseMarkers { path: TargetPath, region: RegionKey },
}

pub enum EditError {
    // ... existing variants from base design ...
    
    // All existing variants (TargetMissing, NotUtf8, AnchorMissing, etc.)
    // apply unchanged. No new error type needed for JSON-specific issues
    // because there is no JSON parsing in the resolve path.
}
```

**CLI behavior (extends base design):**

- `toha apply <path>`: Refusal on build (strict `.json` + markers) prints `error: cannot inject into strict JSON file <path> (region <region>); target a .jsonc or .json5 file instead, or manage the whole file with files:` and exits 1.
  
- `toha apply --dry-run <path>`: Same refusal at build time; dry-run does not mask build errors.
  
- Injection into `.jsonc` / `.json5`: Behaves exactly like any other text-file injection (existing base design rules apply).

**Atomicity:**

Each file write goes to a temp file and is atomically renamed (atomic, per base design). No partial write visible to the user.

---

## 6. Format/comment/order preservation table

| Aspect | Preserved | Destroyed | Notes |
|--------|-----------|-----------|-------|
| Comments *inside* the injected body | Yes — text is verbatim | N/A | The body is raw text between markers; comments within it are preserved as text |
| Comments *outside* the region (rest of file) | Yes — untouched bytes | N/A | Only the marked span is replaced; all other content is left alone |
| Whitespace inside the body | Yes — verbatim | N/A | Text splice, no reformatting |
| Whitespace outside the region | Yes — untouched | N/A | Marker mechanism, not parsing |
| Key order inside the body | Yes — verbatim | N/A | Text splice, not a reserialize |
| Key order outside the region | Yes — untouched | N/A | Only the region changes |
| Formatting of the file as a whole | No — only region changes | Possible but not Toha's fault | If the file's JSON structure changes outside Toha's region, the author/user maintain validity; Toha does not reformat anything |
| Trailing commas / JSON5 syntax | Yes — preserved verbatim | N/A | Text mechanism: all source syntax is preserved |

**For strict `.json` files:** The question is moot because injection is not supported; markers cannot be inserted into strict JSON syntax without breaking validity. The file owner must choose a different mutation strategy.

---

## 7. Dependency justification

**Decision:** No new dependency. Reuse the existing marker mechanism (from the base design) without extending it for structured JSON parsing.

**Why not adopt jsonc-parser's CST mode?**

The research evidence (`/tmp/injection-research/rust-structured.md`, lines 53–96) documents that `jsonc-parser 0.34.0`'s `cst` module (`src/cst/mod.rs`) is format-preserving and supports surgical edits. However:

1. **Adds a new dependency** for a narrow use case. `jsonc-parser` is a well-regarded crate (500+ stars, active, no security risks in shallow clone), but it specializes in JSON-family. The marker mechanism already covers arbitrary text (`.rs`, `.yaml`, `.toml`, `.gitignore`, etc.).

2. **Only solves JSON-family; marker mechanism solves all text.** If Toha adopts `jsonc-parser` for `{json,jsonc,json5}` files, it still needs the marker mechanism for everything else. That is two injection codepaths, two test suites, two error modes.

3. **Structured merge is conceptually distinct.** The marker mechanism owns boundaries by text markers; a structured merge owns boundaries by JSON path (e.g., `doc.features.analytics`). These are different ownership models. Candidate-3 (structured via jaq) was properly rejected as a separate concern. `jsonc-parser` raises the same architectural question: is structured merge in scope for 0.2.0, or reserved for a future design?

4. **Overkill for simple, predictable cases.** Many JSON injections are small: a new key-value pair, a comment-style config entry. The marker mechanism is lighter and testable at the byte level, with no parsing overhead.

5. **The real weakness (strict `.json`) remains unsolved.** Even with `jsonc-parser`, strict JSON has no comment syntax, so markers still cannot be inserted. The honest design choice is to document that boundary and let the author/user choose: target `.jsonc`, manage the whole file, or defer to structured mode. `jsonc-parser` does not change this tradeoff.

**Evidence summary:**
- jaq (`rust-structured.md` lines 5–50): reserializes (drops comments, key order), not format-preserving. Rejected for user-owned files.
- jsonc-parser (`rust-structured.md` lines 53–96): format-preserving, surgical edits via CST. Well-regarded. But adds a dependency for JSON-only, and reserves structured merge for later anyway.
- toml_edit (`rust-structured.md` lines 99–120): established reference for format-preserving TOML. Proves the pattern works. But TOML is not the question here.
- Scaffold (`cross-lang.md` lines 124–212): the only scaffolding tool that injects into files uses **text/line-based insertion** (`strings.Contains` + `before`/`after`), not structural merge. No JSON/YAML awareness. Pure textual insertion is the prior art.

**Recommended scope for 0.2.0:** Marker-only. Declare structured mode (if ever needed) as a future, separate design, with the interface reserved as shown in section 8.

---

## 8. Scope recommendation

**Recommendation: Ship marker injection in 0.2.0; reserve structured-merge mode for a future design.**

**Reasoning:**

1. **Marker injection is complete for 0.2.0** and solves the common case:
   - Template authors inject into `.jsonc`, `.json5`, `.gitignore`, source files, CI YAML — all working with the same marker mechanism.
   - Idempotency, drift detection, and ownership are proven structural facts.
   - No new dependencies; reuses existing infrastructure.

2. **Strict `.json` is a documented non-goal.** The honest design says:
   - If your target is strict JSON, either:
     - Rename it to `.jsonc` (functionally equivalent for most use cases, JSON parsers accept it)
     - Use whole-file `files:` to manage it completely
     - Wait for a future structured-merge design
   - This is not a regression; today Toha cannot inject at all.

3. **Reserved interface for future structured mode:**
   If a later design adopts `jsonc-parser` or similar, the interface can extend without breaking:
   ```yaml
   inject:
     - into: "package.json"
       region: "scripts"
       structured: true          # <- NEW (reserved key, rejected in 0.2.0)
       path: ".scripts.custom-build"  # <- JSON path (reserved, rejected in 0.2.0)
       value: "npm run build"
   ```
   The marker-based entries remain unchanged. Build logic can refuse `structured: true` in 0.2.0 and enable it later.

4. **Follow Toha's principle of ceremony-scales-with-blast-radius.** Marker injection is minimal, proven, orthogonal. Structured merge is a separate concern, larger blast radius, should be designed independently.

---

## 9. Rationale, alternatives, and tradeoffs

### Rationale for champion direction

The marker mechanism + JSON-family comment syntax is **minimalist and composable**:

- JSONC and JSON5 already support `//` comments; Toha's existing marker syntax (`// >>> toha:region ... >>>`) is **literally just a comment line** in these formats. No parsing, no special handling.
- The file remains valid according to the file's own language rules (`jsonc-parser` in the browser or any JSON5 runtime will parse it).
- The same markers work in `.rs` (`//`), YAML (`#`), `.gitignore` (`#`), TypeScript (`//`), etc. — one mechanism, multiple formats.
- No new dependency, no query language, no structural schema.

### Alternatives considered

**A1: Adopt jsonc-parser's CST mode.**
- Pros: Format-preserving for JSON-family; surgical edits at the value level.
- Cons: Adds dependency; only solves JSON-family; structured merge is a separate concern better deferred; strict `.json` still unsolved.
- Verdict: Rejected. Reserves structured mode as a future, separate design.

**A2: Structured merge via jaq.**
- Pros: Supports JSON, YAML, TOML, XML, etc. via a query language.
- Cons: Reserializes (drops comments, key order, formatting). Not format-preserving. For user-owned files, reserialize is a disguised whole-file rewrite — worse than marker injection. Already rejected in base design grounding.

**A3: Whole-file-only (no injection at all).**
- Pros: Simpler; no marker comments visible in user files.
- Cons: Does not solve the user's need: inject a region into an existing file. Regression from expected capability.

**A4: Different injection mechanism per format.**
- Pros: Could optimize for each format (markers for text, CST for JSON, git-diff for others, etc.).
- Cons: Blast radius; multiple code paths; inconsistent author experience. Rejected for complexity.

### Biggest weakness (confront honestly)

**Strict JSON is off-limits.**

The hard truth: a `.json` file with no comment syntax cannot accept comment-delimited markers without breaking JSON validity. Example:

```json
{
  "version": "1.0",
  // >>> toha:region config >>>
  "enabled": true
  // <<< toha:end config sha256:... <<<
}
```

This is **not valid JSON** — comments violate the spec. A strict JSON parser rejects it. So:

- Authors targeting strict `.json` cannot use the marker mechanism.
- They must choose: rename to `.jsonc` (same data, comments allowed), manage the whole file with `files:`, or wait for a structured-merge design.

**How big a limitation?**

In practice, **not huge**:
1. Most projects that generate JSON also use `.jsonc` or `.json5` for hand-edited config (e.g., `tsconfig.json` is often valid JSONC with comments).
2. `package.json` is strict, but generated from a `.jsonc` source or managed whole-file.
3. Many teams already recommend `.jsonc` for config even though the official spec is strict.
4. The design is honest: it documents this boundary and names the alternatives.

**The research evidence** (`/tmp/injection-research/cross-lang.md` lines 208–277) shows even npm's own `package-json` tool (which targets strict `.json`) does **full reserialize** with `JSON.stringify` — not a surgical edit. It threads back indent/newline symbols via heuristics, but loses all comments (of which there are none in strict JSON). Even the ecosystem's de-facto standard doesn't try to do surgical edits to strict JSON; it reserializes.

**Mitigation:** In 0.2.0, document the boundary clearly:
- Injection via markers works for `.jsonc`, `.json5`, and all text files.
- Strict `.json` is not supported for marker injection; use whole-file mode or rename the file.
- Future designs may add structured merge (out of scope).

This is not a regression (today there is no injection at all), and it is a clear contract the author can reason about at template authoring time.

---

## Conclusion

**Candidate C champions minimalism and composition.** By extending the text-marker mechanism to JSONC/JSON5 via their native comment syntax, Toha gains JSON-family injection **without new dependencies, without parsing overhead, without a second code path.** The marker mechanism already handles arbitrary text; comments in JSONC/JSON5 are just text, so markers are just comments. The file remains valid in each language's native parser.

The cost is honesty: strict JSON is documented as out of scope for marker injection. This is a **real tradeoff**, not a weakness hidden. Authors have clear alternatives (rename to `.jsonc`, manage whole-file, or wait for structured mode). The simplicity of 0.2.0 gains you the bandwidth to design structured merge separately, on its own terms, without Toha's core injection mechanism carrying extra weight.
