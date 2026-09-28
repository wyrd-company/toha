# Cross-Judge Report: Content Injection Arena

## Scoring Table

| Criterion | C1 Markers | C2 Anchor+Guard | C3 Struct Merge | C4 Unified Mut |
|-----------|:----------:|:---------------:|:---------------:|:--------------:|
| **1. Structural idempotency** | **5** | 4 | 3 | 2 |
| **2. Ownership contract** | **5** | 3 | 3 | 4 |
| **3. Purity & replay** | **5** | 3 | 3 | 2 |
| **4. Conflict/recovery/safety** | **5** | 3 | 2 | 2 |
| **5. Interface depth & composition** | **4** | 3 | 2 | 3 |
| **6. Format scope honesty** | **5** | 4 | 1 | 2 |
| **Total** | **29/30** | 20/30 | 14/30 | 15/30 |

---

## Verdict Rationale (per criterion)

### 1. Structural Idempotency (no-op second apply guaranteed by construction)

**Candidate 1 (Markers) — 5/5 CONFIRMED**
- Checksum recorded in end-marker `sha256:hex` is the structural invariant. Second apply finds markers, computes `hash(body_on_disk)`, compares to recorded hash; if match → `Unchanged`. Idempotency is by construction, not heuristic. Survives replay.

**Candidate 2 (Anchor+Guard) — 4/5 PLAUSIBLE**
- Guard-id marker must be present in injected content for idempotency. If guard is missing, duplication occurs on re-apply. Idempotency is guaranteed by author discipline (embedding guard), not enforced at build time. Second weakness: two-phase process (build, then build_injections) means between-step changes to target state can alter idempotency check results.

**Candidate 3 (Struct Merge) — 3/5 PLAUSIBLE**
- Reserialization idempotency: second apply re-parses and re-sets value. But YAML/TOML formatters may reindent, reorder keys, or change trailing newlines on round-trip. Risk: bytes differ even though data is identical, violating idempotency guarantee. JSON is safer (stable order, no comments), but design must support YAML/TOML.

**Candidate 4 (Unified Mutation) — 2/5 PLAUSIBLE**
- Checksum approach mirrors Candidate 1, but design is incomplete (many `not_implemented`). Embedding idempotent key marker *inside* injected content creates a moving target: the file contains not just template content but hidden markers. Idempotency key generation not specified; if non-deterministic, staging replay differs. Hazard: lost.

---

### 2. Explicit, Legible Ownership Expressible for 1066/1031

**Candidate 1 (Markers) — 5/5 CONFIRMED**
- On-disk marker lines `toha:region {key}` ... `toha:end {key} sha256:{hash}` are the single source of truth. Ownership contract explicit: "Toha owns byte span between these markers, identified by key; checksum records what was written." 1066 can deterministically: (a) find regions by key, (b) detect drift by checksum mismatch, (c) reason about re-application safety per region. Crystal clear.

**Candidate 2 (Anchor+Guard) — 3/5 PLAUSIBLE**
- Guard-id marks boundary, but guard is embedded in *content-source author's file*, not Toha's control. No explicit on-disk record like markers. Ownership model implicit in `PlannedInjection` struct. 1066 would reverse-engineer: "if guard present, Toha owns content inside." Legible but indirect; no checksum for drift detection.

**Candidate 3 (Struct Merge) — 3/5 PLAUSIBLE**
- "Toha owns value at dotted path `a.b.c`." Ownership implicit in the rule, not observable on-disk. No checksum; no way to detect user drift without diffing (which is lossy due to reformat). 1066 has no tool to detect "has the value been touched?" for safety reasoning. Poor for expressing update contracts.

**Candidate 4 (Unified Mutation) — 4/5 PLAUSIBLE**
- `FileMutation::Injection { idempotent_properties: { ownership_scope: BoundedRegion { start_marker, end_marker, ... } } }` explicitly states what Toha owns. Type-level clarity is strong. But (a) design is incomplete (no impl), (b) embedding hidden idempotent-key marker means bytes written ≠ bytes declared in template, breaking symmetry. Contract is legible in principle but clouded by hidden modifications.

---

### 3. Purity & Replay Preserved (engine pure, staging replay deterministic)

**Candidate 1 (Markers) — 5/5 CONFIRMED**
- `Plan::build(template, completed, target_listing)` pure—renders templates, no target file reads. `PlannedEdit` is pure output. `resolve_edit(current_bytes, edit)` pure function (no side effects, idempotent). Staging replay: same `PlannedEdit` values, so second apply over modified target reaches identical result (by checksum check). Interview engine isolation preserved.

**Candidate 2 (Anchor+Guard) — 3/5 PLAUSIBLE**
- `Plan::build` pure. But `build_injections(target)` impure, *not replayed* (grounding allows reads at plan/apply time, but this step is not part of interview replay). Staging replay: same `build()` but `build_injections()` reads current target state—may differ from original session. Purity boundary is explicit (good), but two-stage process is less pure than Candidate 1.

**Candidate 3 (Struct Merge) — 3/5 PLAUSIBLE**
- `Plan::build` pure (renders Jinja, parses value as JSON, no target reads). Apply reads and parses target document. Idempotency relies on stable reserialization, which is not guaranteed for YAML/TOML. Staging replay: same completed, same plan, but re-apply may produce different bytes. Interview engine is pure, but injection output is not a pure function of inputs due to format-specific serialization quirks.

**Candidate 4 (Unified Mutation) — 2/5 PLAUSIBLE**
- Unclear where idempotency computation happens (build vs. apply—design says "build," but sketches show checks at apply). Embedding hidden idempotent key marker violates pure-function contract: output content ≠ declared template content. Replay would embed fresh markers if generation is non-deterministic. Critical purity hazard.

---

### 4. Coherent Conflict/Failure/Recovery WITHOUT Weakening Safety Guards

**Candidate 1 (Markers) — 5/5 CONFIRMED**
- Error taxonomy clear: `AnchorMissing` (exit 1), `AnchorAmbiguous` (exit 5), `Drift` (exit 1, names `--force`), `MalformedRegion` (exit 1), `TargetMissing` (exit 1 unless create:true). `--force` overrides drift only; never fabricates structure (anchor errors persist). Symlink and `.git` guards preserved. Per-file atomicity: each `fs::write` direct; no corruption of user-owned files (managed region is explicit boundary). Refusal semantics: drift without `--force` → list drifted regions, zero writes. Sound.

**Candidate 2 (Anchor+Guard) — 3/5 PLAUSIBLE**
- Anchor errors clear (not found, cardinality). Guard drift detected only if guard is present; missing guard produces silent duplication (not a caught error). Two-stage process (build_injections before apply) makes failure recovery awkward: user must fix target and re-invoke before retrying. Symlink checks preserved. Coherence weakened by guard embedding dependency.

**Candidate 3 (Struct Merge) — 2/5 PLAUSIBLE**
- Parse/navigation errors fail safely (no write). But **no drift detection**: if user edits value at path, re-apply silently overwrites without warning. This **weakens no-silent-overwrite guarantee** for files Toha partially owns. Ownership boundary is implicit (the path), not explicit or recoverable. Failure on missing path is clear, but recovery path (user must manually restore) is unclear. Asymmetric: whole-file writes refuse on conflict, injections silently overwrite. Incoherent.

**Candidate 4 (Unified Mutation) — 2/5 PLAUSIBLE**
- Conflict logic computed from mutation properties, but implementation sketched (`not_implemented`). Embedding hidden idempotent key marker is itself a silent change: file differs from template without user knowledge. Symlink checks preserved. Error cases not fully defined. Recovery not coherent with whole-file semantics.

---

### 5. Interface Depth & Small Surface; Reuses TargetPath; Composes Whole-File + Injection

**Candidate 1 (Markers) — 4/5 CONFIRMED**
- Public surface: `Plan.edits: Vec<PlannedEdit>` field, `resolve_edit(current, edit)` pure function, optional `FileMutation` view (derived, not public). Reuses `TargetPath` newtype. Whole-file and injection compose: files written first, then edits (well-ordered). Marker policy centralized in `resolve_edit` and marker-family table (implied module). Complexity hidden behind small public API. `PlannedEdit` is sibling to `PlannedFile` (not Content variant), keeping "reads target" and "holds content" separate—good boundary discipline.

**Candidate 2 (Anchor+Guard) — 3/5 PLAUSIBLE**
- Public surface: `Content::Injected` variant (adds Content match branch everywhere), `build_injections()` method (new API surface), `PlannedInjection` struct. Two-stage API is new (users must call both build and build_injections). Guard policy scattered: template author embeds guards in content-source files, apply checks them. Reuses `TargetPath`. Composition: natural (files first, then injections after build_injections), but extra stage increases surface. Interface is wider than Candidate 1.

**Candidate 3 (Struct Merge) — 2/5 PLAUSIBLE**
- Public surface: `Plan.injections` list, `InjectionRule`, `InjectionContent` types. No new top-level function; apply handles inline. But structured-merge logic (parse, navigate path, merge, serialize) is monolithic in apply. No modular seam for future structured-document variants. Path navigation policy not isolated. Interface is simpler (fewer new names), but policy is less modular and harder to extend.

**Candidate 4 (Unified Mutation) — 3/5 PLAUSIBLE**
- Public surface: `FileMutation` enum, `AnchorSpec` enum, `IdempotentProperties` struct, `apply_mutation()` function. All mutations dispatch through one function (coherent). Reuses `TargetPath`. Vision is clean, but execution incomplete (many stubs). If finished, depth would be appropriate; currently, too incomplete to assess.

---

### 6. Scope Honesty on Formats (text vs. structured; one mechanism or several; strategy seam justified; jaq/structured-merge bounded)

**Candidate 1 (Markers) — 5/5 CONFIRMED**
- **One mechanism:** text-based markers work on *any* file. No format restriction. Arbitrary text (README, .gitignore, source code, YAML with comments) all supported.
- **Strategy seam justified:** per-extension comment-family table (hash, --, //, /\* \*/, <!-- -->, semicolon, percent). Explicit override via `marker` field. Seam is small, justified by real variation in comment syntax.
- **No jaq dependency.** No structured-merge mode on critical path. Honest: text-based injection is general; structured merge reserved for future optional extension.
- Scope boundary: arbitrary text files; markers visible in source; author can use `comment:` to override inference. Clear and honest.

**Candidate 2 (Anchor+Guard) — 4/5 PLAUSIBLE**
- One mechanism: text-based guards (author wraps in comments). Conceptually language-agnostic, but author must know comment syntax per file. No structured-merge mode. No jaq. Scope: arbitrary text. Guard embedding is distributed (author responsibility), slightly less honest than Candidate 1's centralized marker logic.

**Candidate 3 (Struct Merge) — 1/5 CONFIRMED (FAILED)**
- **Scope narrowed to structured formats only (JSON, YAML, TOML).** Arbitrary text (README, .gitignore, source code) excluded. This **violates the grounding's core constraint**: "jaq is at best a mechanism for one *structured-merge* injection mode... not a general injection solution." Candidate 3 makes structured-merge the primary (only) design, closing off text injection. Grounding explicitly forbids "letting a jq dependency [or mechanism] dictate the core contract."
- **Reserialization loses formatting.** Comments, key order, whitespace destroyed. Design acknowledges this ("users should keep... under their own control") but offers no solution. Dishonest: scope promise (work with any file) not met.
- **High risk:** users will ask "why can't I inject into README?" Future version will add text-based injection, forcing design revision. This is a transitional design, not final.

**Candidate 4 (Unified Mutation) — 2/5 PLAUSIBLE**
- Reserves both `MarkerRegion` and `StructuredMerge` variants, but only `MarkerRegion` sketched. `StructuredMerge` marked "not implemented"; scope boundaries undefined. Honest about incompleteness (reserves space), but scope is ambiguous. Does not clarify whether structured merge will use jaq or a custom implementation. Leaves the grounding unresolved.

---

## Recommendation

### BASE CANDIDATE: Candidate 1 (Managed-region markers)

**Rationale:**

Candidate 1 is the only design that achieves high confidence on all six criteria simultaneously. It is complete, coherent, and directly addresses the grounding's hard constraints:

1. **Idempotency by construction.** Checksum-in-marker is unforgeable; second apply is guaranteed no-op if data unchanged.
2. **Ownership explicit on-disk.** Marker lines are the contract; no implicit rules; 1066 can reason deterministically.
3. **Purity preserved.** Plan::build pure, bytes read only at apply time (allowed), interview engine isolation maintained, staging replay works identically.
4. **Safety guards unweakened.** Clear error taxonomy, symlink/git/force semantics coherent, no silent overwrites of user-owned files.
5. **Interface tight.** One field, one pure function, no scattered policy. Complexity hidden behind boundary discipline.
6. **Scope honest.** Text-based, works on any file, one mechanism, no jaq dependency, comment-family strategy seam justified.

### Ideas Worth Grafting from Losers

**From Candidate 2 (Anchor+Guard):**
- **Guard concept as ownership boundary** (implicit in Candidate 1's markers; no change needed; superior via checksums).
- **Two-phase planning for future 1066 work** (separate pure plan from impure target state capture). *Note for 1066 design, not critical path; do not graft into base.*

**From Candidate 3 (Struct Merge):**
- **Structured-document extension (future optional mode, 1.0+).** Candidate 3 shows the shape for JSON/YAML/TOML with dotted-path navigation. *Do not graft into base; scope honesty forbids it. Reserve as optional extension if user demand justifies post-launch.*

**From Candidate 4 (Unified Mutation):**
- **Explicit `FileMutation` contract export via `Plan::mutations() -> impl Iterator<Item = FileMutation>`** (already sketched in Candidate 1; makes 1066 consumption cleaner). *Optional polish; not critical.*

---

## Correctness Hazards and Mitigations

### Candidate 1
**No critical hazards.** Design is sound. Minor risk: if body line matches marker syntax (e.g., user hand-writes `// toha:region foo`), parsing breaks. Mitigate: reject at build if rendered body contains a marker line matching this region's key (design already notes this as open question).

### Candidate 2
**Hazard: Guard embedding is author discipline.** If guard omitted, idempotency fails (duplication on re-apply). Mitigate: (a) document as required, (b) template lint rule in 1031, (c) example content-source files include guards.

**Hazard: Target state change between build and build_injections.** If user modifies target between the two calls, idempotency results differ. Mitigate: document that build_injections must be called immediately before apply (not stored for later).

### Candidate 3
**Hazard: Reserialization instability.** YAML/TOML may reformat on round-trip, breaking idempotency guarantee. Mitigate: (a) document round-trip idempotency limitations, (b) use JSON for critical injections (stable serialization), (c) do not use for files with comments (already excluded due to formatting loss).

**Hazard: Silent overwrite of user edits at path.** User edits value, re-apply overwrites without warning. No drift detection. Mitigate: (a) document that Toha owns the value at the path (user should not edit), (b) consider adding a checksum in a comment field (not shown in design; possible extension), (c) this weakens no-silent-overwrite guarantee; high risk of user confusion.

### Candidate 4
**Hazard: Embedding hidden idempotent key marker.** Output content differs from declared template, violating transparency. Staging replay may embed different markers if key generation is non-deterministic. Mitigate: (a) document that hidden markers are added (breaks symmetry), (b) ensure key generation is deterministic (stable hash), (c) risk: users discover markers in generated files and are confused.

**Hazard: Idempotency computation timing unclear.** Design says "build time" but implementation shows "apply time." If computed at apply time, target state changes between plan and apply break idempotency. If computed at build time, target state reads at build violate purity. Mitigate: clarify in implementation (1031), ensure consistency with grounding purity contract.

---

## Dropouts

**None.** All four candidates are viable designs; none are fundamentally broken. Candidate 3 is narrowest in scope (structured formats only), but it is a sound design for that scope. Candidate 4 is incomplete but coherent in vision. Candidate 1 and Candidate 2 are both solid; Candidate 1 is superior on completeness and scope.

---

## Final Summary

| Candidate | Verdict | Use Case |
|-----------|---------|----------|
| **Candidate 1 (Markers)** | **ADOPT AS BASE** | General text injection; any file type; default choice for 1068. |
| **Candidate 2 (Anchor+Guard)** | Good alternative | If two-stage planning is preferred for future 1066 integration. Guard concept is sound but less robust than checksums. |
| **Candidate 3 (Struct Merge)** | Reserve for 1.0+ | Structured documents (JSON/YAML/TOML) with acceptable formatting loss. Do NOT use as primary design (violates scope honesty). |
| **Candidate 4 (Unified Mutation)** | Incomplete | Vision is clean (one dispatcher for all mutations); execution incomplete. Revisit if interface unification becomes critical. |
