# Structured-Document Merge: Content Injection Design

## Problem

Toha today owns whole files: every planned mutation replaces a target file in full. Content injection extends Toha into files it does not own—adding or updating a bounded region of an existing file while leaving the rest to the user. Injection must preserve idempotency (applying twice = applying once), survive staging replay, and express ownership precisely so downstream work (1066, 1031) can reason about re-application.

The tension: arbitrary text injection into user-owned files risks ambiguity (is a marker still present? did the user edit the managed region?), while structured-document modification loses formatting/comments on reserialize. The design chooses where each mode is the right fit.

## Usage (caller's view)

### Template Author (YAML)

```yaml
name: webserver-config
interview:
  - id: server_host
    prompt: What is the server hostname?
    type: text
    default: "localhost"
  - id: db_host
    prompt: What is the database hostname?
    type: text
    default: "db.local"

files:
  # Whole-file generation (existing)
  - source: nginx.conf.j2
    path: config/nginx.conf

# NEW: Injection into existing structured documents
injections:
  - target: config/app.json
    format: json
    path: server.host
    value: "{{ server_host }}"
    when: "{{ render_server_config }}"

  - target: docker-compose.yml
    format: yaml
    path: services.db.environment.DB_HOST
    value: "{{ db_host }}"
    merge: append-to-array  # optional; default is set
```

### CLI User

```bash
# Apply a template; injection targets may already exist
$ toha apply my-config

# Plan view shows injections
$ toha apply my-config --dry-run
  create config/nginx.conf
  inject config/app.json at path server.host
  inject config/docker-compose.yml at path services.db.environment.DB_HOST

# With --force, existing files are updated (including injections)
$ toha apply my-config --force

# Staged interview: idempotency applies to replay
$ toha stage my-config --async
$ toha continue (re-answers) --apply
# Applying the staged result a second time is a no-op
```

### Crate Caller

```rust
use toha::{ Plan, Template, interview };

let template = Template::load("./my-config")?;
let mut interview = interview::start(&template);

// ... drive interview ...
let completed = interview.complete()?;

let plan = Plan::build(&template, &completed, target)?;

// Plan now contains both PlannedFile and PlannedInjection
println!("To write:");
for file in &plan.files {
    println!("  file: {}", file.path);
}
for inj in &plan.injections {
    println!("  inject: {} at {}", inj.path, inj.content.path_expr);
}

plan.apply(target, options, runner)?;
```

## Shape

### Data Structures

#### New Type: Injection Declaration (template.yml domain)

```rust
// In src/template.rs

#[derive(Debug)]
pub struct InjectionRule {
    /// Target file, relative to the target directory. Validated via TargetPath.
    pub target: TargetPath,
    
    /// Format of the target document: json, yaml, toml.
    /// Must be explicit; format is not inferred from extension to avoid silent
    /// misparse of user-owned files.
    pub format: InjectionFormat,
    
    /// Navigation expression. See "Path expressions" below.
    /// Compiled as a Tmpl (string template).
    pub path: Tmpl,
    
    /// Value to inject. Rendered as Jinja, then parsed as JSON (the value
    /// exchange format across all structured documents).
    pub value: Tmpl,
    
    /// How to merge the value:
    /// - Set: replace the value at path (default).
    /// - Append: for arrays, append the value item.
    /// - DeepMerge: for objects, recursively merge.
    pub merge: MergeMode,
    
    /// Conditional: inject only if true.
    pub when: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionFormat {
    Json,
    Yaml,
    Toml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Set,
    Append,
    DeepMerge,
}

// Template gains:
impl Template {
    pub injections: Vec<InjectionRule>,
    // existing fields ...
}
```

#### New Type: Planned Injection (plan.rs domain)

```rust
// In src/plan.rs

#[derive(Debug)]
pub struct PlannedInjection {
    /// Target file path, validated.
    pub path: TargetPath,
    
    /// Content to inject: format, navigation, value, merge strategy.
    pub content: InjectionContent,
    
    /// Source location in template (for error reporting).
    pub source: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InjectionContent {
    pub format: InjectionFormat,
    
    /// The path expression as a string. Rendered at plan time.
    pub path_expr: String,
    
    /// The value as JSON. Rendered at plan time, parsed as JSON.
    pub value: serde_json::Value,
    
    pub merge: MergeMode,
}

// Plan gains:
#[derive(Debug)]
pub struct Plan {
    pub files: Vec<PlannedFile>,
    
    /// NEW: Modifications to existing files.
    pub injections: Vec<PlannedInjection>,
    
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<PlannedHook>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}
```

#### Conflict and Ownership Model

```rust
// In plan.rs: updated Plan::build logic

impl Plan {
    /// Build a plan from a completed interview.
    /// Pure over (template, completed, target directory state).
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Plan, PlanError> {
        let mut plan = Plan {
            files: Vec::new(),
            injections: Vec::new(),
            conflicts: Vec::new(),
            hooks: Vec::new(),
            before_apply: None,
            after_apply: None,
        };

        // 1. Whole-file entries (existing logic)
        // 2. Injection entries (new)
        for inj_rule in &template.injections {
            if !inj_rule.when.eval(&ctx).unwrap_or(false) {
                continue;
            }

            let target_path = TargetPath::parse(&inj_rule.target.to_string())?;
            let path_expr = inj_rule.path.render(&ctx)?;
            let value_str = inj_rule.value.render(&ctx)?;
            let value = serde_json::from_str(&value_str)?;

            plan.injections.push(PlannedInjection {
                path: target_path,
                content: InjectionContent {
                    format: inj_rule.format,
                    path_expr,
                    value,
                    merge: inj_rule.merge,
                },
                source: inj_rule_source_path,
            });

            // Injections do NOT enter the conflicts list.
            // Target MUST exist; non-existence is an apply-time error.
            // (Distinct from whole-file semantics.)
        }

        // 3. Compute conflicts (files only; injections have different semantics)
        for file in &plan.files {
            if target.join(file.path.as_path()).exists() {
                plan.conflicts.push(file.path.clone());
            }
        }

        Ok(plan)
    }
}
```

**Ownership semantics:**
- Whole-file `PlannedFile`: Toha owns the entire target file. User has no prior claim.
- `PlannedInjection` at path `P`: Toha owns the value at path `P`. The user owns the rest of the document.
  - If the document does not exist at apply time → error (no implicit whole-file creation from injection).
  - If the document exists, Toha navigates to `P`, sets the value, and reserializes.
  - If the document is malformed → error (apply-time diagnosis).
  - User may edit around the injected value; re-applying re-sets the exact value.

### Signatures: Template Load

```rust
// In src/template.rs (Builder branch, new)

impl Builder {
    fn build_injections(
        &mut self,
        injections_value: &Value,
        root: &Path,
    ) -> Result<Vec<InjectionRule>, LoadError> {
        let mut rules = Vec::new();
        let arr = injections_value.as_array().ok_or(/* error */)?;
        for inj_obj in arr {
            let target_str = self.require_string(inj_obj, "target")?;
            let target = TargetPath::parse(target_str)
                .map_err(|e| self.error("target", e))?;

            let format_str = self.require_string(inj_obj, "format")?;
            let format = match format_str {
                "json" => InjectionFormat::Json,
                "yaml" => InjectionFormat::Yaml,
                "toml" => InjectionFormat::Toml,
                _ => return self.error("format", "expected json, yaml, or toml"),
            };

            let path_tpl = inj_obj.get("path")
                .ok_or(/* error */)?
                .as_str()
                .ok_or(/* error */)?;
            let path_expr = Tmpl::compile(path_tpl)
                .map_err(|e| self.error("path", e))?;

            let value_tpl = inj_obj.get("value")
                .ok_or(/* error */)?
                .as_str()
                .ok_or(/* error */)?;
            let value_expr = Tmpl::compile(value_tpl)
                .map_err(|e| self.error("value", e))?;

            let merge_str = inj_obj.get("merge").and_then(|v| v.as_str());
            let merge = match merge_str {
                Some("set") | None => MergeMode::Set,
                Some("append") => MergeMode::Append,
                Some("deep-merge") => MergeMode::DeepMerge,
                Some(s) => return self.error("merge", format!("unknown mode: {s}")),
            };

            let when = inj_obj.get("when").and_then(|v| v.as_str());
            let when_expr = when.map(|w| Expr::compile(w))
                .transpose()
                .map_err(|e| self.error("when", e))?;

            rules.push(InjectionRule {
                target,
                format,
                path: path_expr,
                value: value_expr,
                merge,
                when: when_expr,
            });
        }
        Ok(rules)
    }
}
```

### Signatures: Apply

```rust
// In src/apply.rs

impl Plan {
    pub fn apply_reporting(
        self,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        on_written: &mut dyn FnMut(&TargetPath),
    ) -> Result<Applied, ApplyError> {
        // 1. Recompute conflicts for whole-file writes only.
        let mut conflicts = self.conflicts.clone();
        for file in &self.files {
            if target.join(file.path.as_path()).exists() && !conflicts.contains(&file.path) {
                conflicts.push(file.path.clone());
            }
        }

        // 2. Check force gate on whole-file conflicts only.
        if !options.force && !conflicts.is_empty() {
            return Err(ApplyError::Conflicts(conflicts));
        }

        // 3. Check trust for hooks.
        if !options.trusted && !self.hooks.is_empty() {
            return Ok(Applied::NeedsTrust(self));
        }

        // 4. Symlink checks: files, hooks, AND injection targets.
        for file in &self.files {
            if has_symlink_component(target, &file.path)? {
                return Err(ApplyError::Symlink(file.path.clone()));
            }
        }
        for inj in &self.injections {
            if has_symlink_component(target, &inj.path)? {
                return Err(ApplyError::Symlink(inj.path.clone()));
            }
        }
        for hook in &self.hooks {
            if let Some(cwd) = &hook.cwd {
                if has_symlink_component(target, cwd)? {
                    return Err(ApplyError::Symlink(cwd.clone()));
                }
            }
        }

        let mut written = Vec::new();

        // 5. Apply whole-file writes FIRST.
        for file in &self.files {
            let path = target.join(file.path.as_path());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| ApplyError::Io {
                    path: parent.into(),
                    source,
                })?;
            }
            match &file.content {
                Content::Rendered(text) => {
                    fs::write(&path, text).map_err(|source| ApplyError::Io {
                        path: path.clone(),
                        source,
                    })?
                }
                Content::Copied(source) => {
                    fs::copy(source, &path).map_err(|source| ApplyError::Io {
                        path: path.clone(),
                        source,
                    })?;
                }
            }
            written.push(file.path.clone());
            on_written(&file.path);
        }

        // 6. Apply injections SECOND.
        // Idempotency: parse, modify, serialize.
        // If the value is already set, serialization is identical.
        for inj in &self.injections {
            self.apply_injection(target, inj).map_err(|e| {
                ApplyError::InjectionFailed {
                    path: inj.path.clone(),
                    reason: e,
                }
            })?;
            written.push(inj.path.clone());
            on_written(&inj.path);
        }

        // 7. Run hooks.
        // ... (existing logic) ...

        Ok(Applied::Written {
            files: written,
            hooks_run: hook_count,
            after_apply: self.after_apply,
        })
    }

    /// Apply one injection: read, parse, modify, serialize, write.
    fn apply_injection(&self, target: &Path, inj: &PlannedInjection) -> Result<(), String> {
        let full_path = target.join(inj.path.as_path());

        // Read existing file (must exist for injection).
        let file_bytes = fs::read(&full_path)
            .map_err(|e| format!("failed to read {}: {}", inj.path, e))?;

        // Parse according to format.
        let mut doc = match inj.content.format {
            InjectionFormat::Json => {
                serde_json::from_slice(&file_bytes)
                    .map_err(|e| format!("invalid JSON: {}", e))?
            }
            InjectionFormat::Yaml => {
                serde_yaml::from_slice(&file_bytes)
                    .map_err(|e| format!("invalid YAML: {}", e))?
            }
            InjectionFormat::Toml => {
                // TOML parsing: limited support. Tables are complex; kept simple.
                let s = String::from_utf8(file_bytes)
                    .map_err(|e| format!("TOML must be UTF-8: {}", e))?;
                toml::from_str(&s)
                    .map_err(|e| format!("invalid TOML: {}", e))?
            }
        };

        // Navigate to path and apply merge.
        self.merge_at_path(&mut doc, &inj.content.path_expr, &inj.content.value, inj.content.merge)
            .map_err(|e| format!("merge at path '{}': {}", inj.content.path_expr, e))?;

        // Serialize back.
        let new_bytes = match inj.content.format {
            InjectionFormat::Json => {
                serde_json::to_vec_pretty(&doc)
                    .map_err(|e| format!("re-serialize JSON: {}", e))?
            }
            InjectionFormat::Yaml => {
                serde_yaml::to_vec(&doc)
                    .map_err(|e| format!("re-serialize YAML: {}", e))?
            }
            InjectionFormat::Toml => {
                let s = toml::to_string_pretty(&doc)
                    .map_err(|e| format!("re-serialize TOML: {}", e))?;
                s.into_bytes()
            }
        };

        // Write back.
        fs::write(&full_path, new_bytes)
            .map_err(|e| format!("write {}: {}", inj.path, e))?;

        Ok(())
    }

    /// Navigate a dotted path in a JSON value and apply a merge.
    /// Paths: "a.b.c" navigates a → b → c.
    /// Arrays: "a[0].b" is not yet supported; indices in square brackets are future work.
    fn merge_at_path(
        &self,
        doc: &mut serde_json::Value,
        path_expr: &str,
        value: &serde_json::Value,
        mode: MergeMode,
    ) -> Result<(), String> {
        // TODO: pseudocode for dotted path navigation.
        // - Split on ".", handle empty components.
        // - At each step, navigate the JSON structure.
        // - At the final step, apply the merge mode.
        //
        // For Set: doc[final_step] = value.clone()
        // For Append: doc[final_step].as_array_mut()?.push(value.clone())
        // For DeepMerge: recursive object merge.
        //
        // Invariant: all intermediate steps must exist.
        // If a step is missing, return an error naming it.

        not_implemented!(
            "merge_at_path: navigate path '{}', apply {:?}, value type {:?}",
            path_expr,
            mode,
            value.get_type()
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    // ... existing variants ...

    #[error("injection into {path} failed: {reason}")]
    InjectionFailed { path: TargetPath, reason: String },
}
```

### Applied Result

```rust
// In src/apply.rs (updated)

#[derive(Debug)]
pub enum Applied {
    Written {
        files: Vec<TargetPath>,  // Both whole-file and injection targets.
        hooks_run: usize,
        after_apply: Option<String>,
    },
    NeedsTrust(Plan),
}
```

### Dry-Run Output

```
$ toha apply --dry-run
  create config/nginx.conf
  inject config/app.json at path 'server.host' (format: json)
  inject config/docker-compose.yml at path 'services.db.environment.DB_HOST' (format: yaml)
```

### Conflict Resolution

**Whole-file + injection to same target in one plan:**

```rust
// If the plan has both PlannedFile and PlannedInjection for the same target:
// 1. Detect during Plan::build.
// 2. Raise PlanError::InvalidMixture (file and injection on same target).

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    // ...
    #[error("cannot both write and inject {path}: choose one")]
    InvalidMixture { path: TargetPath },
}
```

This prevents the ambiguity: whole-file writes are intended to fully replace, while injections modify in place.

## Tradeoffs Accepted

- **Reserialization loses formatting.** Injecting into JSON, YAML, or TOML destroys comments, key order, and whitespace. This is inherent to structured merge via parsing. For files where formatting is critical, users should keep them under their own control and not declare injections into them. The design accepts this loss in exchange for the correctness and idempotency of structured merge.

- **Array indexing not supported.** Dotted paths like `a.b.c` are supported; array indexing like `a[0].b` is left for future work. This limits injection expressiveness but keeps the initial implementation simple and focused on the common case (object keys).

- **No implicit file creation.** If an injection target does not exist, apply fails. Injection assumes the file already exists; Toha does not create it. This prevents silent surprises and makes the injection's ownership clear: Toha modifies an existing file, not synthesizes a new one from scratch.

- **Format must be explicit.** The `template.yml` author states the format (`json`, `yaml`, `toml`); it is not inferred from file extension. This prevents misparse of user-owned files and makes the injection's intent legible.

- **One injection mode per format.** A given structured-document injection uses one of {Set, Append, DeepMerge}. Finer control (e.g., conditional append, or conditional deep-merge) requires multiple injections or conditional top-level logic—a deliberate simplification to avoid nesting complexity.

## Alternatives Considered

### Alternative 1: Text-based injection with markers

**Shape:** Mark managed regions with comments like `<!-- toha:start id=server_host -->` / `<!-- toha:end -->`. Toha reads the text, finds markers, and replaces the region between them.

**Why it lost:** Markers are file-type-specific; there is no universal comment syntax (HTML, JavaScript, YAML, Python, Bash, JSON all differ). This forces the template author to author many variants or accept file-specific logic scattered across the codebase. Additionally, if the user edits within a marked region, re-application risks clobbering their work. Marker-based recovery requires heuristics (detecting drift, diffing), which violate the pure, side-effect-free contract and complicate staging replay. Structured merge avoids this by owning only the value at a named path, not a text region.

### Alternative 2: jaq as the primary mechanism

**Shape:** Embed `jaq` (pure-Rust jq clone) as a dependency and use jq filters like `.server.host` or `.[0] |= . + 1` for all injections, not just JSON.

**Why it lost:**
1. **Dependency weight:** jaq is embeddable but is a non-trivial query engine. The brief's grounding notes that jaq reserializes (does not preserve formatting), so it does not solve the formatting problem; it only expands the scope. Adding a dependency on the critical path requires explicit approval and shifts maintenance burden.
2. **Scope creep:** jq's power is appealing but introduces a new language users must learn. A dotted path is immediately familiar; `.a.b.c` is not familiar to non-jq users.
3. **Not required:** The design achieves idempotency, ownership, and idempotency without jq. Jaq is a mechanism for one (optional, advanced) mode; the base design does not need it.

The design intentionally bounds structured merge as a mode and does NOT require jaq adoption. If a future project wants advanced filtering, jaq can be added as an opt-in mode; the core design stands without it.

### Alternative 3: Marker + auto-detection

**Shape:** Combine text markers with auto-detect of document format. For structured documents (JSON, YAML, TOML), use markers like `# toha:injected-by=server_host` inside the value (e.g., as a comment in the JSON object).

**Why it lost:** Markers inside structured values are malformed or limited in expressiveness. A JSON object cannot hold a comment (`{"key": "value" /* comment */}` is not valid JSON). Markers must live outside the value, which pushes us back toward the text-region model and its attendant complexity. Structured merge cleanly separates Toha's concern (the value at a path) from the document structure.

## Open Questions and Risks

1. **Dotted-path expressiveness:** Is a dotted-path syntax sufficient for common cases (config objects, environment tables)? Array indexing is left for future work. If early users frequently encounter arrays (e.g., `services[0].name`), the syntax must be extended. Question: should the design reserve the syntax (e.g., `[0]` in paths) to avoid future breakage, or keep it open?

2. **Round-trip semantics:** When JSON is serialized and re-parsed, does the result byte-for-byte match the input (given no data changes)? For JSON, yes. For YAML and TOML, it depends on the library's defaults (indentation, key order, trailing newlines). Should the design specify these, or is best-effort acceptable? Risk: user code that is sensitive to byte-exact comparisons (e.g., git diffs) will see spurious changes after injection.

3. **Partial failures:** If an injection fails midway through apply (e.g., merge_at_path discovers the path does not exist), is the partially-written file a problem? Today, whole-file writes are atomic per file (if write fails, the old file remains); injections read, modify in memory, then write. If the write fails after the modification, the old bytes on disk are not affected. Is this sufficient, or does the design need a read-modify-write atomicity guarantee?

4. **TOML tables and arrays:** TOML has rich semantics for inline tables and arrays. The initial design treats TOML as serde_json values. Does this handle edge cases (e.g., tables with `[table]` vs `{table = {}}`), or are complex TOML documents out of scope?

5. **Circular coordinate:** If a user manually edits the injected value at a path, and the template re-applies with the same path and a different value, does the injection silently overwrite the user's edit? This is the intended behavior (Toha owns the value at the path). Should the design warn, or is this expected? Risk: user confusion if they forget Toha manages a value and edit it manually.

## Next Implementation Step

Implement `merge_at_path` in `apply.rs`: parse the dotted-path expression, navigate the JSON value step-by-step, and apply the merge mode (Set / Append / DeepMerge) at the final step, with error diagnostics naming which path component is missing or malformed. Test against a fixture that injects into a realistic config file (JSON or YAML) and verifies idempotency (apply twice, file changes once).
