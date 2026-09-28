# Unified typed mutation object on `Plan` (mechanism-as-data)

## Problem

Toha's current mutation model treats whole-file operations (`Rendered`, `Copied`) as first-class variants in `Content`, while content injection would add a third variant requiring read-modify-write semantics. This scatters the mutation contract: whole-file operations define conflict semantics via a separate `conflicts` list (existence ⇒ conflict), while injection must redefine what "conflict" means (target must exist, only a bounded region changes). The mechanism for selecting *where* to inject (markers, anchors, structured keys) would live across multiple modules with no unified contract for idempotency, ownership, or replay. Instead, design one **typed mutation enum** that carries all mechanism (anchor, marker, merge strategy) as **data computed at Plan::build**, applied by one uniform read-modify-write executor. This unifies whole-file and injection mutations, makes idempotency and ownership explicit properties of each mutation value (surviving replay), and keeps injection a pure text/data transform.

---

## Usage (caller's view)

### Template author perspective

A template author declares injection using a new `injections` section in `template.yml`. The injection mechanism (marker type, anchor location, structured path) is **data** the author provides; Toha computes idempotency properties from it.

```yaml
template:
  name: web-bootstrap
  
interview:
  - id: framework
    type: select
    prompt: "Choose a web framework"
    options: [next, remix, astro]

files:
  # Ordinary whole-file write (existing behavior)
  - source: src/app.ts
    path: src/app.ts

injections:
  # Inject content into a marked region in an existing file
  - id: inject_config
    source: config/inject-defaults.yaml
    target: config.yaml
    anchor:
      type: marker
      strategy: comment
      start_marker: "# BEGIN generated config"
      end_marker: "# END generated config"
  
  # Inject into a structured file (JSON/YAML key)
  - id: inject_deps
    source: deps.json
    target: package.json
    anchor:
      type: structured
      format: json
      path: /dependencies
      merge: update_keys
```

An author can mix whole-file writes and injections targeting the same apply. They are composed naturally in one `Plan`.

### Crate/CLI caller perspective

From the caller's view, the contract is:

```rust
// Caller provides template + completed interview + target directory
let plan = Plan::build(&template, &completed, &target_dir)?;

// Plan now contains mutations of mixed types:
// - Some targets are replaced wholly (user-facing files Toha owns)
// - Some targets are injected into (existing files, partial ownership)
// All mutations carry idempotent properties computed from template source + context

// Apply idempotently: each mutation knows its ownership scope and how to
// validate re-applicability. Applying twice on unchanged input ⇒ no change.
let applied = plan.apply(&target_dir, options, runner)?;
```

The CLI sees this through `--dry-run` output, which now names the mutation type:
```
create src/app.ts                       # whole-file write
inject config.yaml (markers)            # injection
update package.json (json path)         # structured merge
```

Conflict behavior is now mutation-specific (embedded in `FileMutation`):
- Whole-file: "file exists ⇒ conflict (unless --force)"
- Injection: "target must exist, but region can be present/absent for idempotency"

Exit code logic remains: 0 success, 1 error, 3 needs-trust, 4 pending, 5 ambiguous.

---

## Shape

### Data structures (core mutations and idempotency contract)

```rust
/// Unified mutation enum: all file modifications are variants of this type.
/// The mutation mechanism (how content is placed) is data, not scattered.
#[derive(Debug, Clone)]
pub enum FileMutation {
    /// Replace entire file with rendered text.
    WholeFileRendered {
        text: String,
        idempotent_properties: IdempotentProperties,
    },
    
    /// Replace entire file with byte-for-byte copy.
    WholeFileCopied {
        source: PathBuf,
        idempotent_properties: IdempotentProperties,
    },
    
    /// Inject content into a bounded region of an existing file.
    /// The anchor defines WHERE and HOW to mark/select that region.
    /// Idempotency is encoded as properties of the mutation itself.
    Injection {
        content: String,
        anchor: AnchorSpec,
        idempotent_properties: IdempotentProperties,
    },
}

/// Specifies the anchor/selection mechanism for injection.
/// All mechanisms are data; the executor interprets them uniformly.
#[derive(Debug, Clone)]
pub enum AnchorSpec {
    /// Marker region: text is bounded by start/end delimiters.
    /// Markers are computed from template + context at Plan::build.
    MarkerRegion {
        start_marker: String,
        end_marker: String,
        // Strategy for how to find/handle markers (e.g., comment style)
        marker_strategy: MarkerStrategy,
    },
    
    /// Structured field merge: inject by updating a key/path in JSON/YAML/TOML.
    /// Not implemented in this sketch, but shape reserves it.
    StructuredMerge {
        path: String,  // JSON pointer, YAML path, etc.
        format: StructuredFormat,
        merge_strategy: MergeStrategy,
    },
}

#[derive(Debug, Clone)]
pub enum MarkerStrategy {
    /// Markers are comments; lines starting with `#`, `//`, etc.
    Comment { line_prefix: String },
    /// Markers are XML/HTML comments.
    HtmlComment,
    // Further strategies reserved for future.
}

#[derive(Debug, Clone)]
pub enum StructuredFormat {
    Json,
    Yaml,
    Toml,
}

#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// Update/insert keys; preserve unmentioned keys.
    UpdateKeys,
    /// Replace the entire value at this path.
    Replace,
}

/// Properties that encode idempotency and ownership.
/// Computed purely at Plan::build from (Template, Completed, anchor spec).
/// Part of the mutation contract for 1066 (git-based updates) to consume.
#[derive(Debug, Clone)]
pub struct IdempotentProperties {
    /// Stable identifier for this mutation, computed from:
    /// - template source file path
    /// - anchor specification
    /// - hash of content template
    /// Used to detect drift and allow replay.
    pub idempotent_key: String,
    
    /// What bytes does Toha own in the target file?
    pub ownership_scope: OwnershipScope,
    
    /// How to validate that this mutation can be applied idempotently.
    pub idempotency_check: IdempotencyCheck,
}

#[derive(Debug, Clone)]
pub enum OwnershipScope {
    /// Whole file: Toha is sole owner.
    EntireFile,
    
    /// Bounded region: Toha owns bytes between markers/delimiters.
    /// User owns everything outside. Coordinates are byte offsets
    /// in the expected file layout (computed at Plan::build, validated at apply).
    BoundedRegion {
        start_byte: usize,
        end_byte: usize,
        // Markers for re-finding the region during re-apply
        start_marker: String,
        end_marker: String,
    },
}

#[derive(Debug, Clone)]
pub enum IdempotencyCheck {
    /// For whole-file writes: content must be identical or file absent.
    /// Applying twice on unchanged input leaves file unchanged.
    WholeFile {
        expected_content: String,
    },
    
    /// For injections: region is either present (with idempotent key marker inside)
    /// or absent. Applying twice when region is already present is a no-op.
    MarkerRegion {
        start_marker: String,
        end_marker: String,
        // Embedded in the injected content: a hidden marker that survives
        // user edits, allowing drift detection and re-application.
        idempotent_key_marker: String,
    },
}

/// Refactored planned file: mutation is now the core, not content alone.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub path: TargetPath,
    pub mutation: FileMutation,
    /// Source file in template folder (for traceability, mode preservation)
    pub source: PathBuf,
}

/// Refactored Plan: no separate conflicts list.
/// Conflict semantics are now properties of each mutation.
#[derive(Debug)]
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub hooks: Vec<PlannedHook>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}
```

### Signatures (Plan building and applying)

```rust
impl Plan {
    /// Build a plan: pure over (template, completed, target directory listing).
    /// Reads target directory to detect conflicts and compute idempotency checks.
    /// Computes anchor specs and idempotent properties for all injections.
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &Path,
    ) -> Result<Self, PlanError> {
        // not implemented: build file mutations, detecting conflicts
        // For each injection rule: compute anchor from template + context,
        // read target file's partial content to compute idempotency checks,
        // build IdempotentProperties value that encodes the contract.
        // For whole-file: compute IdempotentProperties with ownership=EntireFile.
    }

    /// Apply a plan: read-modify-write executor interpreting mutations uniformly.
    /// Checks symlinks, computes new conflicts, applies mutations in order.
    pub fn apply_reporting(
        self,
        target: &Path,
        options: ApplyOptions,
        runner: &dyn HookRunner,
        on_written: &mut dyn FnMut(&TargetPath),
    ) -> Result<Applied, ApplyError> {
        // not implemented
        // 1. Compute conflicts from mutations (each mutation reports its conflicts)
        // 2. Check --force, --trusted, symlinks (as today)
        // 3. For each mutation: apply_mutation(target, mutation, on_written)
        //    All mutations use the same executor path.
    }
}

/// Apply one mutation uniformly; interpreter dispatches on mutation variant.
fn apply_mutation(
    target: &Path,
    planned: &PlannedFile,
    on_written: &mut dyn FnMut(&TargetPath),
) -> Result<(), ApplyError> {
    let path = target.join(planned.path.as_path());

    // Symlink check (already done at plan level, but checked again per-file)
    if has_symlink_component(target, &planned.path)? {
        return Err(ApplyError::Symlink(planned.path.clone()));
    }

    match &planned.mutation {
        FileMutation::WholeFileRendered {
            text,
            idempotent_properties,
        } => {
            // Check idempotency: if file exists, must match expected.
            validate_whole_file_idempotency(&path, text, idempotent_properties)?;
            
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(&path, text)?;
            copy_permissions(&planned.source, &path)?;
            on_written(&planned.path);
        }

        FileMutation::WholeFileCopied {
            source,
            idempotent_properties,
        } => {
            // Read copied source to validate idempotency check
            let copied_content = fs::read_to_string(source)?;
            validate_whole_file_idempotency(&path, &copied_content, idempotent_properties)?;
            
            fs::create_dir_all(path.parent().unwrap())?;
            fs::copy(source, &path)?;
            copy_permissions(source, &path)?;
            on_written(&planned.path);
        }

        FileMutation::Injection {
            content,
            anchor,
            idempotent_properties,
        } => {
            // Read-modify-write: find the injection point, insert/update content.
            apply_injection(&path, content, anchor, idempotent_properties)?;
            on_written(&planned.path);
        }
    }

    Ok(())
}

/// Validate that a whole-file write is idempotent.
fn validate_whole_file_idempotency(
    path: &Path,
    expected_content: &str,
    properties: &IdempotentProperties,
) -> Result<(), ApplyError> {
    match &properties.idempotency_check {
        IdempotencyCheck::WholeFile {
            expected_content: stored,
        } => {
            if path.exists() {
                let existing = fs::read_to_string(path)?;
                if existing != *stored {
                    // File exists but differs from expected: conflict
                    return Err(ApplyError::ConflictingFile {
                        path: path.to_owned(),
                        reason: "existing file differs from expected".into(),
                    });
                }
            }
            // File absent or matches expected: idempotent, safe to write.
            Ok(())
        }
        _ => unreachable!("whole-file must have WholeFile idempotency check"),
    }
}

/// Apply an injection mutation: read existing, find anchor, modify, write.
fn apply_injection(
    path: &Path,
    content: &str,
    anchor: &AnchorSpec,
    properties: &IdempotentProperties,
) -> Result<(), ApplyError> {
    match anchor {
        AnchorSpec::MarkerRegion {
            start_marker,
            end_marker,
            marker_strategy,
        } => {
            // Read existing file
            let existing = if path.exists() {
                fs::read_to_string(path)?
            } else {
                String::new()
            };

            // Find or create the marked region
            let (new_content, was_present) =
                inject_by_markers(&existing, content, start_marker, end_marker)?;

            // Check idempotency: if region was present, must be unchanged
            if was_present {
                match &properties.idempotency_check {
                    IdempotencyCheck::MarkerRegion {
                        idempotent_key_marker,
                        ..
                    } => {
                        // Verify the idempotent_key_marker is present inside the old region
                        // If not, region has drifted; may be error or warning per design
                    }
                    _ => unreachable!(),
                }
            }

            // Write the new content
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, new_content)?;
            Ok(())
        }

        AnchorSpec::StructuredMerge {
            path: json_path,
            format,
            merge_strategy,
        } => {
            // not implemented: structured merge path
            // Read JSON/YAML/TOML, update key at path, reserialize
            Err(ApplyError::NotImplemented(
                "structured merge".into(),
            ))
        }
    }
}

/// Find or create a marker region, inject content.
fn inject_by_markers(
    existing: &str,
    content: &str,
    start_marker: &str,
    end_marker: &str,
) -> Result<(String, bool), ApplyError> {
    // not implemented: full logic
    // 1. Search for start_marker and end_marker in existing
    // 2. If both found: replace content between them (idempotent if content matches)
    // 3. If neither found: append markers + content to end
    // 4. If only start found: error (malformed, drift)
    // Return (new_content, was_region_present)
    todo!()
}
```

### Module seams and orchestration

**Key seams:**
- `Plan::build` computes `FileMutation` and `IdempotentProperties` for all files (whole-file and injection)
- `apply_mutation` dispatcher interprets all mutation variants uniformly
- `IdempotentProperties` encodes the contract that downstream (1066 updates, 1031 implementation) consumes

**No new modules needed.** The mutation enum lives in `plan.rs` alongside `Plan` and `PlannedFile`. The anchor specs live in `plan.rs` as data. The `apply` path in `apply.rs` gains `apply_mutation` helper (one function, not scattered).

**Composition of whole-file and injection in one Plan:**
- Both are `FileMutation` variants
- Applied in source order by the same `apply_mutation` dispatcher
- Ordering determinism: mutations are applied in `plan.files` order
- If two mutations target the same file, the last one wins (whole-file write replaces, injection modifies). Atomicity: mutations are independent file operations, each atomic. If mutation N fails, mutations 1..N-1 are committed (as today with whole-file).

### Error and result contract

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("conflicting files:{}", conflict_lines(.0))]
    Conflicts(Vec<TargetPath>),  // Computed from mutation properties
    
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    
    #[error("target path contains symlink: {0}")]
    Symlink(TargetPath),
    
    #[error("{path}: {reason}")]
    ConflictingFile { path: PathBuf, reason: String },
    
    #[error("injection failed in {path}: {reason}")]
    InjectionFailed { path: PathBuf, reason: String },
    
    // Hook errors as today
    #[error("hook {index} failed: {outcome:?}")]
    Hook { index: usize, outcome: HookOutcome },
    
    #[error("hook {index} failed: {source}")]
    HookIo { index: usize, source: HookError },
}
```

---

## Synthesis decision

This is candidate-4, designed in isolation. It represents one exploration of the design space: **unify all mutations into one enum carrying mechanism-as-data, make idempotency and ownership explicit properties of mutations, apply uniformly**. Other candidates may explore:
- Keeping `Content` and adding a third variant + scattered marker logic (less unified)
- Separating mutation types per file (`MutationPlan` vs `PlannedFile`) with per-type handlers (more complex seams)
- Lazy idempotency checks at apply time vs. eager computation at build time (different purity trade-off)

This candidate prioritizes **interface depth and unification**: a small public surface (`FileMutation`, `AnchorSpec`, `IdempotentProperties`) that hides the complexity of marker selection, structured merge, and idempotency validation behind a uniform executor. The mechanism is data, not logic.

---

## Tradeoffs accepted

- **Marker strategy as an enum, not a heuristic.** We compute start/end markers from template at `Plan::build` and carry them as data. This is deterministic and replay-safe, but requires the author to be explicit about marker format (e.g., "# BEGIN" for comments). A heuristic that infers markers from file extension would be simpler for authors but violates purity (output would depend on untraced file content).
- **Idempotent key embedded in injected content.** To detect drift (user edits the managed region), we embed a hidden marker inside the injected content itself. This is transparent to the file and survives re-apply, but adds complexity: the content the user sees is slightly different from the template-declared content (the marker is appended). Future work can refine (e.g., use byte-offset markers instead).
- **Whole-file and injection ordering.** If one mutation writes a whole file and another injects into it, the order matters. We apply in source order; the last mutation wins. This is simple but could surprise an author. Alternative: detect this and error; alternative: parallelize independent files. We chose source order for simplicity and compatibility with whole-file behavior.
- **Idempotency computed at Plan::build, not apply.** We read the target file at plan time to compute idempotency checks (e.g., what the region currently contains). This makes idempotent properties deterministic and cacheable, but means a file edited between plan and apply may cause idempotency to fail. Alternative: defer checks to apply time (more flexible but harder to report problems early).

---

## Alternatives considered

**Alternative 1: Keep `Content` enum, add `Injection` variant (scatter marker/anchor logic).**
- Add `Content::Injected { content: String, anchor: ??? }` alongside `Rendered` and `Copied`.
- Injection conflict and idempotency logic scattered across `plan.rs` (building the variant) and `apply.rs` (handling it).
- No unified `IdempotentProperties` contract; each variant defines its own rules.
- **Why it lost:** Doubles the number of code paths. The public surface (`Plan`, `PlannedFile`, `Content`) does not hide the complexity of markers and anchors — that complexity leaks into the template schema and the apply dispatcher. Interface depth increases; cohesion decreases.

**Alternative 2: Separate `MutationPlan` from `PlannedFile` (type-level distinction).**
- Keep `PlannedFile` for whole-file writes; introduce `PlannedInjection` with its own contract.
- Each lives in its own struct; `Plan` holds `Vec<PlannedFile>` and `Vec<PlannedInjection>`.
- Apply dispatcher has two branches: one for files, one for injections.
- **Why it lost:** Doubles the plan structure. Composing whole-file and injection in one plan becomes awkward (two lists, separate conflict logic). The contract is harder to extend (adding a third mutation type means adding a third struct). Unification is lost.

**Alternative 3: Defer idempotency checks to apply time (lazy computation).**
- `IdempotentProperties` is minimal; the real checks happen when applying.
- Simpler to build at Plan::build; more flexible if the target changes between plan and apply.
- **Why it lost:** Violates the grounding requirement that idempotency survive staging replay. A staged interview is replayed from inputs, not from the original target state. If we compute idempotency at apply time and the target has changed, replay will fail differently than the original apply. We need determinism: idempotency must be a property of the mutation value itself, computed early and stably.

---

## Open questions and risks

1. **Idempotent key marker embedding:** How visible should the embedded marker be? Should it be a byte-offset comment, an invisible Unicode marker, or something else? The design assumes comments (e.g., `<!-- idempotency-key: hash -->` for HTML), but this is opaque to the file and adds bytes the author did not write. A structured approach (e.g., storing the key separately in a manifest) would avoid content modification but adds I/O.

2. **Drift detection and recovery:** If the user edits the managed region and the idempotent key marker is lost, how does Toha detect drift? The design flags it as a conflict, but should re-apply be attempted with `--force`? Or is it an unrecoverable error? This should be documented and tested.

3. **Whole-file write to the same target as injection:** If a template declares `files: [{ source: a, path: config }]` (whole-file) and `injections: [{ target: config, ... }]`, what happens? The design applies in source order, so whole-file write runs first, then injection modifies it. Is this intentional or an error? Should we detect and warn?

4. **Structured merge implementation:** The design reserves `AnchorSpec::StructuredMerge` but marks it "not implemented." Implementing it requires either adopting a dependency (e.g., jaq, ruled out by grounding) or writing a small structured-document editor. This is deferred to 1031 (implementation), but the design should commit to the contract (what formats, what merge strategies).

5. **File permissions and executable mode:** Injections modify files in place. Should `copy_permissions` also preserve mode for injected files? Today, mode is copied from the template source; for injections, the source is the template file, not the target. If the target is executable and the injection is text, should it stay executable? The design currently does not address this.

6. **Multi-target composition:** Can an injection target the same file as another injection? Yes (append multiple regions). Can a whole-file write target an existing injection target afterward? Yes (replaces whole file). Is this tested? This should be explicitly tested in 1031.

---

## Next implementation step

Implement `Plan::build` to construct `FileMutation` enums and compute `IdempotentProperties` for all file rules (whole-file and injections), handling both marker-based and structured anchors as reserved variants. This is the critical path: the entire apply executor and public contract downstream depend on this value being correct.

---

## Proposed template.yml snippet

```yaml
template:
  name: service-scaffold
  description: "Scaffold a microservice with config injection"

interview:
  - id: service_name
    type: text
    prompt: "Service name"
    required: true
  
  - id: has_env_config
    type: confirm
    prompt: "Add environment variables?"
    default: true

files:
  - source: src/main.rs
    path: src/main.rs
  - source: Cargo.toml
    path: Cargo.toml

injections:
  # Inject dependencies into an existing Cargo.toml
  - id: add_deps
    when: "{{ has_env_config }}"
    source: support/cargo-deps.toml
    target: Cargo.toml
    anchor:
      type: marker
      strategy: comment
      start_marker: "# [BEGIN] managed dependencies"
      end_marker: "# [END] managed dependencies"
  
  # Inject environment-variable defaults into a .env file
  # (target already exists; this region is managed)
  - id: add_env_defaults
    when: "{{ has_env_config }}"
    source: support/env-defaults
    target: .env
    anchor:
      type: marker
      strategy: comment
      start_marker: "# BEGIN {{ service_name }} config"
      end_marker: "# END {{ service_name }} config"

hooks:
  - run: [cargo, check]
    when: "{{ has_env_config }}"
```

The author is explicit about markers (no inference); injection targets are existing files that Toha does not own wholly. The template engine renders markers (e.g., `{{ service_name }}` in the end marker). Anchors are data; Toha interprets them uniformly at apply time.

---

## Rust type and signature sketches (summary)

**Core enum:**
```rust
pub enum FileMutation {
    WholeFileRendered { text: String, idempotent_properties: IdempotentProperties },
    WholeFileCopied { source: PathBuf, idempotent_properties: IdempotentProperties },
    Injection { content: String, anchor: AnchorSpec, idempotent_properties: IdempotentProperties },
}

pub enum AnchorSpec {
    MarkerRegion { start_marker: String, end_marker: String, marker_strategy: MarkerStrategy },
    StructuredMerge { path: String, format: StructuredFormat, merge_strategy: MergeStrategy },
}

pub struct IdempotentProperties {
    pub idempotent_key: String,
    pub ownership_scope: OwnershipScope,
    pub idempotency_check: IdempotencyCheck,
}
```

**Apply path:**
```rust
fn apply_mutation(target: &Path, planned: &PlannedFile, on_written: &mut dyn FnMut(&TargetPath)) -> Result<(), ApplyError>
```

All three mutation types flow through this one function. No scattered logic.

