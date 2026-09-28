$schema: https://refinery.systems/ontology/design
title: "Content injection via anchor-relative insertion with presence/identity guard"
relationships:
  design-for:
    - "toha/0.2.0 (task 1068)"
  supersedes: []
  references:
    - "01-grounding.md"

---

# Anchor-relative insertion with presence/identity guard

## Problem

Toha currently writes only whole files — `Content::Rendered` and `Content::Copied`. Adding a bounded mutation to existing files (content injection) requires a way to:
- locate where content should go in a file Toha does not own (anchor/selection)
- detect when the same content has already been applied (idempotency)
- avoid rewriting the entire file, which destroys formatting and user changes
- make re-application safe even when the user edits around the injected region

The design must honor the hard constraints: the interview engine stays pure; purity survives staging replay; `TargetPath` and symlink guards stay intact; no new permission/timeout/subprocess mechanics; injection must compose with whole-file writes in a single `Plan`.

The grounding surface this on existing callers: a second apply of the same staged interview must change the file exactly once, not twice.

## Usage (caller's view)

### Author perspective: template.yml

```yaml
name: example-app
description: Scaffold an app with dependencies

interview:
  - id: app_name
    type: text
    prompt: "Name of your app?"
    default: "myapp"
  - id: needs_testing
    type: confirm
    prompt: "Add test configuration?"
    default: false

files:
  # Traditional whole-file generation still works
  - each: "values.enabled as item"
    source: "src/config.yml.jinja"
    path: "config/{{item}}.yml"

# NEW: injection rules
injections:
  # Inject into an existing Cargo.toml
  - target: "Cargo.toml"
    anchor:
      literal: "[dependencies]"  # Find this line exactly
    insert: after  # Insert after it
    content-source: "snippets/cargo-deps.txt.jinja"  # Render this file
    guard-id: "toha-cargo-deps"  # Stable identity
    
  # Inject into an existing README
  - target: "README.md"
    anchor:
      regex: "^## Features$"  # Find this section header
    insert: after
    content-source: "snippets/readme-features.md.jinja"
    guard-id: "toha-readme-features"
    when: "{{needs_testing}}"  # Conditional injection
```

### Crate caller (pure function)

```rust
// User code, unchanged:
let completed = interview::Interview::start(template, seed)
    .ask_all(user_input)?
    .completed();

// Plan is pure over template, answers, and target-directory state:
let plan = Plan::build(&template, &completed, &target_dir)?;

// Two calls to apply_once—once to plan, once to actually write:
let plan = plan.build_injections(&target_dir)?;  // reads existing files
let applied = plan.apply(&target_dir, options, runner)?;

// If the user re-stages and re-applies the same answers, files change exactly once:
let applied2 = plan.apply(&target_dir, options, runner)?;  
// Result: same file, no mutation on second apply.
```

### CLI caller

```bash
# Traditional flow, now with injection support
toha init my-template --target ./myproject
# → interview.yml with answers
# → second apply (if re-run) is idempotent

toha apply --from interview.yml --target ./myproject --dry-run
# Output includes injections:
#   → Cargo.toml (inject after [dependencies])
#   → README.md (inject after ## Features)

toha apply --from interview.yml --target ./myproject
# Files written: Cargo.toml, README.md, etc.

toha apply --from interview.yml --target ./myproject
# Files unchanged (idempotent)
```

### Behavior under replay (purity + staging)

```
Session 1:
  - Start interview, answer questions → Completed C1
  - Plan::build(template, C1, target/) → Plan P1 (pure, no file reads yet)
  - P1.build_injections(target/) → reads existing files → Plan P1'
  - apply(P1', target/) → files written

Session 2 (reload interview.yml, replay with same answers):
  - Record::replay() → same Completed C1 (staging is deterministic)
  - Plan::build(template, C1, target/) → Plan P1 again (pure)
  - P1.build_injections(target/) → reads current target files
  - apply(P1', target/) → idempotent: guards already present, skip
```

The purity boundary is clear: `Plan::build` does not read target files; `build_injections` does (and is not replayed). This preserves the interview engine's pure semantics.

---

## Shape

### Data structures

#### New Content variant

```rust
pub enum Content {
    Rendered(String),
    Copied(PathBuf),
    // NEW:
    Injected {
        anchor: Anchor,
        insert: InsertMode,
        rendered_content: String,  // result of rendering content-source
        guard_id: String,          // stable identity of this injection
    },
}
```

#### Anchor definition (in Plan, computed at build_injections time)

```rust
pub enum Anchor {
    // Literal string must exist in file, exactly
    Literal(String),
    
    // Regex pattern; must match 0 or 1 occurrence (configurable)
    // If multiple, behavior set by cardinality_policy
    Regex {
        pattern: String,
        cardinality_policy: CardinalityPolicy,  // FirstMatch, SingleMatch, AllMatches
    },
    
    // 1-indexed line number (rare, fragile, explicit)
    Line(usize),
}

pub enum InsertMode {
    Before,  // Insert before anchor line
    After,   // Insert after anchor line
    Replace, // Replace anchor line with content (removes anchor)
}

pub enum CardinalityPolicy {
    FirstMatch,    // Use first match (unsafe, user edits above)
    SingleMatch,   // Fail if != 1 match (safe, explicit)
    AllMatches,    // Inject before/after every match (may duplicate)
}
```

#### Guard: identity marker in rendered content

```rust
pub struct InjectionGuard {
    // Every injected block carries this marker embedded in a comment
    // Format is language-agnostic: wrap in `<!-- -->`, `//`, `#`, etc.
    // The guard MUST be present in the rendered_content before rendering.
    id: String,  // e.g., "toha-cargo-deps"
    // Computed: hash of (anchor, content_source_path, guard_id) → stable identity
}
```

#### PlannedInjection type

```rust
pub struct PlannedInjection {
    pub path: TargetPath,
    pub anchor: Anchor,
    pub insert: InsertMode,
    pub rendered_content: String,
    pub guard_id: String,
    // source for error reporting:
    pub source: PathBuf,
}
```

#### Updated Plan structure

```rust
pub struct Plan {
    pub files: Vec<PlannedFile>,      // existing (whole-file writes)
    pub injections: Vec<PlannedInjection>,  // NEW
    pub conflicts: Vec<TargetPath>,   // existing
    pub hooks: Vec<PlannedHook>,      // existing
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}
```

### Signatures and flow

#### Plan building (pure; does not read target files)

```rust
impl Plan {
    /// Pure function: render template, compute anchors and content.
    /// Does NOT read target files yet.
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Plan, PlanError> {
        // Existing logic for files, hooks, messages
        // NEW: for each injection rule in template.injections:
        //   - render anchor (literal/regex/line)
        //   - render content-source as a support file
        //   - create PlannedInjection with rendered_content and guard_id
        // Returns Plan with empty injections until build_injections() is called.
        not_implemented()
    }
}
```

#### Plan: read existing files and compute final injection (impure, gated)

```rust
impl Plan {
    /// Impure function: read target files, find anchors, compute final mutations.
    /// Must be called immediately before apply.
    /// This is the seam where existing file state enters; it is NOT replayed.
    pub fn build_injections(
        mut self,
        target: &Path,
    ) -> Result<Plan, InjectionError> {
        for inj in self.injections.iter_mut() {
            let file_path = target.join(inj.path.as_path());
            
            // Guard: check has_symlink_component
            if has_symlink_component(target, &inj.path)? {
                return Err(InjectionError::Symlink(inj.path.clone()));
            }
            
            // Read existing file
            let existing = fs::read_to_string(&file_path).map_err(|e| {
                InjectionError::Io {
                    path: file_path.clone(),
                    source: e,
                }
            })?;
            
            // Find anchor
            let anchor_line = self.find_anchor(&existing, &inj.anchor)?;
            
            // Check guard: is injection already present?
            if self.has_guard(&existing, &inj.guard_id)? {
                // Idempotent: skip this injection (or update if content changed)
                inj.status = InjectionStatus::AlreadyPresent;
                continue;
            }
            
            // Compute final bytes: merge existing file + injected content
            let final_content = self.apply_injection(&existing, anchor_line, inj)?;
            
            // Convert to a pseudo-PlannedFile for apply() to handle consistently
            // OR track in injections and apply handles both files and injections
            inj.final_content = Some(final_content);
            inj.status = InjectionStatus::ToApply;
        }
        Ok(self)
    }
    
    fn find_anchor(&self, content: &str, anchor: &Anchor) -> Result<usize, InjectionError> {
        match anchor {
            Anchor::Literal(s) => {
                let lines: Vec<&str> = content.lines().collect();
                lines.iter().position(|line| line.contains(s))
                    .ok_or(InjectionError::AnchorNotFound)
            }
            Anchor::Regex { pattern, cardinality_policy } => {
                let re = Regex::new(pattern)?;
                let lines: Vec<&str> = content.lines().collect();
                let matches: Vec<usize> = lines.iter()
                    .enumerate()
                    .filter(|(_, line)| re.is_match(line))
                    .map(|(i, _)| i)
                    .collect();
                    
                match cardinality_policy {
                    CardinalityPolicy::SingleMatch => {
                        if matches.len() == 1 {
                            Ok(matches[0])
                        } else {
                            Err(InjectionError::AnchorCardinality {
                                expected: 1,
                                found: matches.len(),
                            })
                        }
                    }
                    CardinalityPolicy::FirstMatch => Ok(matches[0]),
                    CardinalityPolicy::AllMatches => {
                        // Special case: handled in apply_injection
                        // For now, return first
                        Ok(matches[0])
                    }
                }
            }
            Anchor::Line(line_num) => {
                if *line_num > 0 && *line_num <= content.lines().count() {
                    Ok(line_num - 1)
                } else {
                    Err(InjectionError::LineOutOfBounds(*line_num))
                }
            }
        }
    }
    
    fn has_guard(&self, content: &str, guard_id: &str) -> Result<bool, InjectionError> {
        // Guard format: <!-- toha:guard_id -->
        // (language-agnostic; content-source can use comment syntax as needed)
        let guard_marker = format!("toha:{}", guard_id);
        Ok(content.contains(&guard_marker))
    }
    
    fn apply_injection(
        &self,
        existing: &str,
        anchor_line: usize,
        inj: &PlannedInjection,
    ) -> Result<String, InjectionError> {
        let mut lines: Vec<&str> = existing.lines().collect();
        
        let guard_start = format!("<!-- toha:{} (start) -->", inj.guard_id);
        let guard_end = format!("<!-- toha:{} (end) -->", inj.guard_id);
        let guarded_content = format!("{}\n{}\n{}", guard_start, inj.rendered_content, guard_end);
        
        match inj.insert {
            InsertMode::After => {
                lines.insert(anchor_line + 1, &guarded_content);
            }
            InsertMode::Before => {
                lines.insert(anchor_line, &guarded_content);
            }
            InsertMode::Replace => {
                lines[anchor_line] = &guarded_content;
            }
        }
        
        Ok(lines.join("\n"))
    }
}
```

#### Apply logic (handles both whole-file and injections)

```rust
impl Plan {
    pub fn apply_reporting(
        self,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        on_written: &mut dyn FnMut(&TargetPath),
    ) -> Result<Applied, ApplyError> {
        // Existing conflict checks for whole-file writes
        // NEW: injections do NOT conflict (target must exist; that's expected)
        
        let mut written = Vec::new();
        
        // Apply whole files first (traditional)
        for file in &self.files {
            // ... existing logic
            written.push(file.path.clone());
        }
        
        // Apply injections
        // Injections that have status == AlreadyPresent are skipped
        // Injections with status == ToApply are written
        for inj in &self.injections {
            if inj.status == InjectionStatus::AlreadyPresent {
                continue;
            }
            let path = target.join(inj.path.as_path());
            let final_content = inj.final_content.as_ref()?;  // computed by build_injections
            fs::write(&path, final_content).map_err(|e| ApplyError::Io {
                path,
                source: e,
            })?;
            on_written(&inj.path);
            written.push(inj.path.clone());
        }
        
        // Run hooks
        // ...
        
        Ok(Applied::Written { files: written, hooks_run, after_apply })
    }
}
```

#### Template schema (new)

```yaml
# In template.yml:
injections:
  - id: optional_name  # for error messages
    target: "path/to/file"
    anchor:
      # Choose one:
      literal: "text to find"
      # OR
      regex: "^## Section$"
      # OR
      line: 42
    
    # How to position injected content
    insert: "before" | "after" | "replace"  # default: after
    
    # Template file to render and inject
    content-source: "snippets/content.txt.jinja"
    
    # Identity marker (stable across re-applies)
    guard-id: "my-injection-id"
    
    # Conditional injection
    when: "{{some_condition}}"
```

#### Error and recovery contract

```rust
pub enum InjectionError {
    #[error("anchor not found in {0}")]
    AnchorNotFound,
    
    #[error("anchor matched {found} times, expected {expected}")]
    AnchorCardinality { expected: usize, found: usize },
    
    #[error("line {0} out of bounds")]
    LineOutOfBounds(usize),
    
    #[error("target path contains symlink: {0}")]
    Symlink(TargetPath),
    
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    
    #[error("regex error: {0}")]
    Regex(String),
    
    #[error("{path}: managed region drifted; guard present but content differs")]
    ManagedRegionDrifted { path: TargetPath },
}
```

**Behavior on recovery:**

- **Anchor not found**: fail fast with clear error (user edits template anchor or fixes file).
- **Anchor cardinality mismatch**: fail fast; user chooses cardinality_policy or edits file.
- **Managed region drifted**: detect when guard is present but content differs; by default, re-apply the new content (overwrite user's local edit in the managed region). Rationale: guard is an ownership mark; content inside is Toha's domain.
- **Symlink**: reject at build_injections time (same as PlannedFile).
- **I/O failure during build_injections**: recorded in InjectionError; apply() is not attempted.

## Tradeoffs accepted

- **We accept loss of user formatting within the injected region in exchange for clear ownership.**
  The guard markers frame what Toha owns. User edits inside the guards are intentional overrides of managed content; if content re-renders differently, the new version replaces the old (no three-way merge). This is simpler than diffing or preserving formatting and avoids silent errors.

- **We accept that re-application requires reading the existing file in exchange for idempotency and purity.**
  The `build_injections` step is impure and not replayed; it sits between pure plan construction and apply. This is necessary to detect guards and compute final bytes correctly. The trade is acceptable because `build_injections` is deterministic given the current file state—replay is not needed for correctness, only for re-running answers files from a past session.

- **We accept single-pass regex matching (first match by default) in exchange for simplicity.**
  Anchors are expected to be unique per file. If a user wants to inject in multiple places, they write multiple anchor rules with distinct patterns or cardinality policies. This avoids complex multi-match logic and keeps the surface small.

- **We accept that guard markers must be embedded in the content-source template in exchange for language-agnostic injection.**
  The content-source file is responsible for wrapping guarded content with markers (in comments). This keeps injection logic out of the plan engine and lets authors control comment syntax per file type.

- **We accept no formatter integration in exchange for no new permissions.**
  Injected content is inserted as-is. If the user wants auto-formatting post-injection, they run it themselves or as a hook (which already gates on trust). This avoids subprocess execution and formatter dependency.

- **We accept that ordering between whole-file writes and injections to the same target is sequential (whole-files first, then injections) in exchange for determinism.**
  If an injection rule and a file rule target the same path, the whole-file write happens first; the injection then operates on the written file. This avoids complex merge logic and is unambiguous.

## Alternatives considered

1. **Three-way merge (old content, injected content, user edits).**
   - Merge tools like `diff3` or `git merge-base` could resolve conflicts between managed region and user edits.
   - **Rejected**: Requires external tool (violates no-subprocess constraint) or complex merge logic. Three-way merge is fragile when user edits are semantically invalid (e.g., broken YAML in a YAML injection). The simpler rule (guard marks ownership, user edits inside are overrides, re-render replaces) is more predictable.

2. **Heuristic idempotency via text matching (no explicit guard).**
   - On re-apply, search for the injected text in the file; if found, skip. If not found but anchor exists, inject again.
   - **Rejected**: Fragile when user edits the injected content slightly (e.g., removes a comment, changes formatting). The guard is explicit and stable; heuristics hide silent failures.

3. **Whole-file merge for injections (jaq-based structured merge).**
   - Use jaq to do deep merges on JSON/TOML/YAML, preserving structure and comments where possible.
   - **Rejected** (per grounding): jaq does not preserve comments in YAML and is only suitable for structured formats. Arbitrary text targets (README, .gitignore, Makefile) are outside its reach. Injection should be a general text mechanism, not specialized for JSON/YAML.

4. **Anchor relative to line count or fixed offsets (no text matching).**
   - Anchor is "line 42" or "offset 1000 bytes". User must be careful not to move target content.
   - **Rejected**: Fragile; line count shifts easily with user edits. Text-based anchors are more resilient.

5. **Markers written by Toha, not by the content-source author.**
   - Plan logic emits `<!-- toha:guard-id (start) -->` and `<!-- toha:guard-id (end) -->` around the rendered content.
   - **Rejected**: Imposes HTML comment syntax on all files. The content-source author should choose comment syntax appropriate to the file type (e.g., `#` for shell, `//` for C, `<!--` for HTML). Flexibility here reduces surprise.

## Open questions and risks

1. **How do we handle language-specific comment syntax elegantly?**
   - Today, the content-source author must wrap guarded content in comments. Do we want a filter or macro to help? E.g., `{% guard 'toha-deps' %}...{% endguard %}` that auto-wraps based on file type?
   - Recommendation: Start with manual wrapping (content-source includes guards); a macro can follow in a later iteration if authors request it.

2. **What happens when the user manually removes guards but leaves the injected content?**
   - The guide will explain that guards mark Toha's ownership. Removing a guard is an explicit signal: "I own this now, don't re-inject." Re-applying will detect no guard and inject again, resulting in duplication.
   - Is this the right behavior, or should we detect "orphaned" injected content (e.g., via a hash of the original content)?
   - Recommendation: Document clearly that guards are ownership markers. A hash-based recovery mode can follow if telemetry shows users are removing guards by mistake.

3. **How do we handle partial failures (e.g., injection succeeds, then a hook fails)?**
   - Today, apply does not roll back; already-written files stay written. Injection follows the same model: if an injection is written and a later hook fails, the injection is not undone.
   - Should we offer a mechanism to re-apply injections if hooks fail? (Similar to "re-run this injection only".)
   - Recommendation: Match whole-file write semantics for now. Selective re-apply can be a future feature.

4. **Dry-run output for injections: what should we show?**
   - For whole files, we show "create file.txt". For injections, do we show "inject into Cargo.toml after [dependencies]" or "modify Cargo.toml"?
   - Recommendation: Show "inject into <path> using <anchor>", matching the detail of hook output.

5. **How do we handle re-renders of the content-source that change the guard ID or position?**
   - If a template author edits the injection rule and changes `guard-id`, the old guard remains in the file and the new guard is added (duplication).
   - Should we support deprecation or cleanup of old guards?
   - Recommendation: This is a template author mistake; document that guard-id should not change. A linter can check for orphaned guards later.

6. **Does injection work with symlink'd target files?**
   - The `has_symlink_component` check applies; a symlink in the path is rejected. But what if the target file itself is a symlink?
   - Recommendation: Reject symlinks in the target path as we do for files. The safety seam is preserved.

## Next implementation step

Write a minimal acceptance fixture: a template with a single injection rule targeting an existing file (e.g., Cargo.toml), with a literal anchor and a rendered content-source. Verify that applying the fixture twice produces idempotent results (file changes once, guard is present on second apply).

---

## Appendix: template.yml injection declaration syntax

```yaml
name: example-app
description: Add dependencies to an existing project

interview:
  - id: add_testing
    type: confirm
    prompt: "Add test framework?"
    default: false

files:
  # Traditional whole-file rules remain unchanged
  - each: "['config.yml']"
    source: "config.yml.jinja"
    path: "{{item}}"

# NEW: injection rules
injections:
  # Example 1: Inject into Cargo.toml (Rust project)
  - id: rust-test-deps  # optional, for error reporting
    target: "Cargo.toml"
    anchor:
      literal: "[dev-dependencies]"
    insert: after
    content-source: "snippets/cargo-test-deps.toml"
    guard-id: "toha-test-deps"
    when: "{{add_testing}}"

  # Example 2: Inject into README.md (Markdown)
  - id: readme-setup
    target: "README.md"
    anchor:
      regex: "^## Installation"
    insert: after
    content-source: "snippets/readme-setup.md.jinja"
    guard-id: "toha-readme-setup"

  # Example 3: Inject with conditional + rendering
  - id: ci-config
    target: ".github/workflows/ci.yml"
    anchor:
      regex: "jobs:"
    insert: after
    content-source: "snippets/ci-job.yml.jinja"
    guard-id: "toha-ci-{{add_testing | lower}}"
    when: "{% if add_testing %}true{% else %}false{% endif %}"
```

**Content-source example: `snippets/cargo-test-deps.toml`**

```toml
# toha:toha-test-deps (start)
[dev-dependencies]
insta = "1.36"
proptest = "1.4"
# toha:toha-test-deps (end)
```

Note: The guard markers are literal strings in the content-source and are not rendered. They are part of the injected content verbatim. The plan logic searches for these markers to detect idempotency.

