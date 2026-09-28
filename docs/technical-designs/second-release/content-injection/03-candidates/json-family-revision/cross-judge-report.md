# Cross-judge report — JSON-family content injection arena

> [!IMPORTANT]
> **Historical arena evidence:** This report preserves the judge's scores and
> sequencing recommendation. It is not the final design. Bob approved
> Candidate B's embedded `jsonc-parser` structured mutation for JSON, JSONC,
> and JSON5 in this release, with no JSON-family marker injection. See the
> [final JSON-family synthesis](../../04a-synthesis-json-family.md) and the
> [authoritative design](../../05-design.md). The report below remains the
> judge's original verdict.

## Executive Summary

Three candidates compete for Toha's structured JSON-family injection mechanism:
- **A**: Embedded `jaq` filter engine (whole-file reserialize via deterministic filters)
- **B**: Format-preserving CST via `jsonc-parser` (surgical value-path edits with full fidelity)
- **C**: Extend marker-based injection to JSONC/JSON5 via native comment syntax (text-based, no parsing)

**Base recommendation**: Candidate **C** for 0.2.0 shipping. Candidates A and B are well-designed fast-follows (0.3.0+), with Candidate B's interface design worth grafting into any future structured-mode implementation.

**Ship-now-vs-defer decision**: 0.2.0 should ship marker-based injection (Candidate C). Structured mode is decided but deferred; the market need is not proven, and markers are sufficient.

---

## Candidate A: Embedded jaq Structured Mode

| Criterion | Score | Notes |
|---|---|---|
| 1. Format/comment/order preservation | 1/5 | Unconditionally discards `#` comments (jaq-json/src/read.rs:10-19 confirmed); regenerates all whitespace via pretty-printer (write.rs:202-260); output normalized. This is the core load-bearing weakness. |
| 2. Ownership / drift / idempotency | 3/5 | Filter determinism guarantees idempotency (re-apply yields byte-identical output). But whole-file rewrite violates stated principle ("Toha writes only the bounded region between markers," 05-design.md:366-391). Design acknowledges honestly. |
| 3. Conflict / failure / recovery | 2/5 | No --force gating (whole file is owned). Mechanism is final-output-wins: user edits inside the file are clobbered on re-apply. Safe but aggressive. No silent overwrites, but no mercy either. |
| 4. Interface depth / composition | 3/5 | Author surface is simple (filter: `. fieldname = value`), but requires jq language knowledge. Whole-file mutation semantics are heavyweight. Mutually exclusive with markers in the same file (section 3: "NOT RECOMMENDED" to mix). |
| 5. Scope honesty & dependency | 3/5 | jaq is well-maintained (1000+ stars, recent commits 2026-08-28 per citation) and justified as "best-in-class jq implementation in Rust." But design itself recommends deferring to 0.3.0, citing scope pressure on 0.2.0 (section 8: "DECIDE NOW, BUT DEFER"). |
| 6. Evidence quality | 5/5 | Excellent. Citations are precise (jaq-json/src/read.rs:10-19, jaq-json/src/write.rs:202-260, jaq-json/src/lib.rs:117, jaq-fmts/src/lib.rs:54,59-79) and verified against actual source code. Examples are concrete and reproducible. |

**Rationale**: Candidate A trades comment preservation for filter expressiveness. jaq is a reserializer by design — it parses to a value tree, runs a filter, and emits fresh JSON. The idempotency is strong (deterministic filter), but the cost is high: every re-apply produces a large diff (whole file rewritten), and comments are lost unconditionally. This violates Toha's own principle (section 3, "Ownership/Drift/Idempotency Model") that "Toha writes only the bounded region between markers." The mechanism is honest about the tradeoff — it does not pretend there is a "managed region" when the whole file is owned — but the tradeoff is steep for user-facing files with documentation comments.

**Biggest weakness**: Disguised whole-file rewrite of a user-owned file, with unconditional comment loss and regenerated whitespace. Acceptable only for strict JSON (no comments) or machine-generated JSONC (no developer-visible comments). Unacceptable for files like `tsconfig.json` with developer documentation.

---

## Candidate B: Format-preserving CST via jsonc-parser

| Criterion | Score | Notes |
|---|---|---|
| 1. Format/comment/order preservation | 5/5 | CST module doc (src/cst/mod.rs:1-5) states: "keeps every comment and every piece of whitespace, so a document can be edited and written back out with everything the author wrote still in place." Example (lines 6-33) demonstrates: set `data: 123` with preceding comment `// comment`, append new key, output preserves the comment. Verified: `Display for CstRootNode` (line 1356) re-emits full original bytes. |
| 2. Ownership / drift / idempotency | 4/5 | Owns the VALUE AT A NAMED PATH (e.g., `scripts.build`), not a byte-spanned region. Convergent: re-apply sets the value to desired state iff different. Idempotency is guaranteed by value equality (section 3: `resolve_struct_edit` returns `Unchanged` iff current value equals desired). BUT: drift detection in strict JSON is weak — no comment space to embed markers, so cannot distinguish "first place" from "user edited." Workaround in JSONC: `provenance: true` embeds a marker comment (best-effort, not cryptographic). This is a real tradeoff, not a hidden weakness. |
| 3. Conflict / failure / recovery | 4/5 | Convergent mode: no --force gating needed (every apply sets the value to the desired state). Atomicity via temp+rename (section 4, "pure function"). Clear contract: "the value at this path SHALL equal X." Safe and explicit. Minor: for strict JSON, cannot distinguish first application from drift, but design is honest about it and documents the limitation. |
| 4. Interface depth / composition | 4/5 | Small author surface: `struct: { path: "a.b.c", value: "..." }`. Discriminator on `region` vs. `struct` keeps marker and structured entries separate yet composable in one `inject:` list. Types are fully sketched (StructPath, PlannedEdit variant, StructResolution). Surgical edit API (get/append/insert/set_value/remove) is well-designed and borrowed from `toml_edit` precedent. |
| 5. Scope honesty & dependency | 4/5 | jsonc-parser is well-regarded (1.5k+ stars, active, used by Deno/dprint). Purpose-built for CST editing (architecture identical to `toml_edit`). No new surface restrictions (no permissions, no timeouts, no subprocess). Deferred to 0.3.0 is reasonable — interface is fully specified, handoff cost is low. Honest boundary: strict JSON requires whole-file management or rename to .jsonc. |
| 6. Evidence quality | 5/5 | Excellent citations and verification. CST module doc comment, type signatures, example output. Research (rust-structured.md) provides comparison to `toml_edit` (gold standard). Specific line numbers (src/cst/mod.rs:1146, 1798-1858, 1356) verified against cloned source. |

**Rationale**: Candidate B is the format-preserving alternative to Candidate A. It uses a Concrete Syntax Tree (CST) to retain every comment and whitespace token, then performs surgical mutations (set a value at a path, append a new key, etc.) and re-emits with full fidelity. This is architecturally identical to `toml_edit` (the reference gold standard for format-preserving editing). Idempotency is guaranteed by value equality, not by markers. Drift detection is weak in strict JSON but optional provenance comments in JSONC offer best-effort awareness.

**Biggest weakness**: Drift detection is weak in strict JSON (cannot embed markers to prove "this value is the one I last wrote"), and for JSONC/JSON5, the `provenance: true` marker is a heuristic, not a cryptographic guarantee. However, the design is honest: ownership is declarative (you own the path; re-applies will set it to the template's value). This is a different contract from marker-based injection (which refuses drift), but it is explicit and safe.

---

## Candidate C: Comment-Marker Injection for JSONC/JSON5

| Criterion | Score | Notes |
|---|---|---|
| 1. Format/comment/order preservation | 5/5 | Pure text splice between markers. No parsing, no regeneration. Comments inside the injected body are verbatim (just text). Comments outside the region are untouched (unrelated bytes not rewritten). Whitespace inside the body is verbatim; whitespace outside the region unchanged. Proof: Ansible `blockinfile` precedent (cross-lang.md lines 8-105) — same mechanism, proven in production for decades. |
| 2. Ownership / drift / idempotency | 5/5 | Marker-based boundary (`// >>> toha:region X >>>` and `// <<< toha:end X sha256:... <<<`). Ownership is legible: Toha owns the byte span between (and including) markers. Idempotency via SHA256 hash of the body: if hash matches, no bytes change. Drift detection: hash mismatch = user edited inside the region. Structural and provable. |
| 3. Conflict / failure / recovery | 5/5 | Refuses re-apply on drift (hash mismatch) without --force. Exit 1, names file and region. With --force, overwrites and recomputes hash. No silent data loss. Atomicity via temp+rename. This is the strongest contract: refusal is explicit, not a declarative "value will be set to X." |
| 4. Interface depth / composition | 5/5 | Reuses existing marker infrastructure from base design (05-design.md). No new types, no new author-facing syntax. Comment family inferred from extension (`.jsonc` / `.json5` → `//` markers; `.json` → error or documented non-goal). Zero new surface complexity. Composition is natural: markers and structured entries in the same file are separate concerns, each with own region/path. |
| 5. Scope honesty & dependency | 5/5 | Zero new dependencies. No parser, no CST library, no new constraint (timeouts, permissions, subprocess). Honest boundary: strict `.json` is unsupported for marker injection (no comment syntax to embed markers without breaking JSON validity). Design names alternatives clearly (rename to `.jsonc`, manage whole file, wait for structured mode). Ready to ship 0.2.0. |
| 6. Evidence quality | 5/5 | Grounded in established precedent: Ansible `blockinfile` (python, proven, https://github.com/ansible/ansible, cited lines 8-105 of cross-lang.md). Scaffolding-tools research (scaffolding-tools.md lines 124-210) shows Scaffold (Go) uses textual line-based injection (`strings.Contains` + before/after). Idempotency contract is structural and proven. |

**Rationale**: Candidate C is the minimalist approach: extend the text-marker mechanism (which already works for arbitrary text) to JSONC and JSON5 by leveraging their native comment syntax. A comment marker (`// >>> toha:region X >>>`) is literally just a comment line in JSONC/JSON5 — no special parsing, no CST, no new code path. The file remains valid according to the language's own parser. Ownership is legible, drift detection is structural (SHA256 hash), and idempotency is guaranteed by recompute-and-compare. The mechanism is proven at scale (Ansible for 15+ years).

**Biggest weakness**: Strict JSON is off-limits. A `.json` file has no comment syntax, so markers cannot be inserted without breaking JSON validity. Authors must choose: rename the file to `.jsonc` (functionally equivalent for most use cases, any JSON parser accepts JSONC), manage the whole file via `files:`, or wait for a structured-merge design. This is a real tradeoff, but not a regression (today Toha cannot inject at all), and the design documents it clearly. Research shows even npm's own `package-json` tool (targeting strict JSON) does full reserialize with `JSON.stringify` — not a surgical edit. No tool in the ecosystem tries to do marker injection into strict JSON.

---

## Comparative Analysis

### Format Preservation Showdown

| Format | A (jaq) | B (jsonc-parser) | C (markers) |
|---|---|---|---|
| Strict JSON comments | N/A (no comments) | Preserved verbatim | N/A (unsupported) |
| JSONC comments | **Lost** | **Preserved** | **Preserved** |
| Whitespace/indent | **Regenerated** | **Preserved** | **Preserved** |
| Key order | Output normalized | Preserved | Preserved (inside body; outside untouched) |
| Diff size on re-apply (no change) | Large (whole file) | None (value unchanged) | None (body unchanged) |

A is a reserializer; B and C preserve untouched content.

### Ownership Model

- **A (jaq)**: Whole-file is owned. Filter output is the source of truth. User edits are temporary; re-apply clobbers them.
- **B (jsonc-parser)**: Value at a path is owned. Convergent: re-apply sets value to desired state. No marker space for drift proof in JSON; best-effort provenance comments in JSONC.
- **C (markers)**: Byte span between markers is owned. Marker hash proves "this is the body I last wrote." Drift is detected and refused.

### Idempotency Contract

- **A (jaq)**: Filter is deterministic, output is byte-identical on re-apply if nothing changed. Strong.
- **B (jsonc-parser)**: Value equality: if value at path already equals desired, no write. Strong within path scope.
- **C (markers)**: SHA256 hash: if hash matches, body unchanged, no write. Structural and provable.

### Scope and Readiness

- **A (jaq)**: Designed for 0.2.0 but recommendation is defer to 0.3.0. Implementation is straightforward (compile filter, apply it, reserialize), but scope risk is real.
- **B (jsonc-parser)**: Designed as deferred to 0.3.0 fast-follow. Interface fully specified (StructPath, PlannedEdit, resolve_struct_edit). Handoff is low-risk.
- **C (markers)**: Ready to ship 0.2.0 today. No new code paths, no new types, no new dependencies.

---

## What to Graft from Each Candidate

### Graft from A into the final design:
**Nothing immediately**. Candidate A is useful as a future (0.3.0+) option if filter-based mutations are desired, but jaq's reserialize cost is not justified for 0.2.0. If structured mode is later chosen, Candidate B's path-based ownership model is preferable.

### Graft from B into the final design:
**Interface design and path-based ownership model**. If/when Toha adopts a structured-mode feature (0.3.0+), whether via jaq or jsonc-parser CST:
- Use a `path: "a.b.c"` syntax (dot-separated, array-indexable) rather than a filter language.
- Discriminate via `struct: { path, value }` in the `inject:` list, separate from `region`-based entries.
- For JSONC, optionally embed `provenance: true` comments (best-effort drift awareness, not a gate).
- Design the `resolve_struct_edit` function as a pure function: `Option<&[u8]> × PlannedEdit → Result<Resolution>`.

### Graft from C into the final design:
**All of it**. Candidate C is the base recommendation. Shipping 0.2.0 with markers solves the immediate need.

---

## Honest Assessment of Tradeoffs

### Candidate A's honest admission (section 3):
> "Using structured jaq mode means **accepting a disguised whole-file rewrite of a user-owned file**. This is the load-bearing problem that this candidate must confront."

The design is transparent about the cost. But the cost is high: every re-apply rewrites the entire file, produces large diffs, and loses comments unconditionally. This violates Toha's stated principle (section 3: "Toha writes only the bounded region between markers"). The design attempts to justify it ("for strict JSON, no comments exist"; "for JSONC without developer-visible comments, reserialize is low-risk"), but the justification is weak for files like `tsconfig.json` that carry documentation.

### Candidate B's honest admission (section 9):
> "Without comment syntax to embed a marker, Toha cannot prove 'the value at this path is the one I last wrote' in a strict `.json` file."

The design accepts convergent ownership (declarative: "the value at this path SHALL be X") rather than marker-gated ownership (structural: "Toha last wrote this value"). This is a different contract, but it is explicit and safe. For JSONC, the `provenance: true` comment is best-effort, not a cryptographic guarantee. This is honest and reasonable.

### Candidate C's honest admission (section 9):
> "Strict JSON is off-limits. The hard truth: a `.json` file with no comment syntax cannot accept comment-delimited markers without breaking JSON validity."

The design names alternatives (rename to `.jsonc`, manage whole file, wait for structured mode) and documents the boundary. This is not a regression (today there is no injection at all). Research shows the ecosystem doesn't try to inject into strict JSON either — npm's `package-json` tool does full reserialize. The tradeoff is real, but documented and reasonable.

---

## Scoring Summary

| Candidate | 1 | 2 | 3 | 4 | 5 | 6 | **Total** |
|---|---|---|---|---|---|---|---|
| A (jaq) | 1 | 3 | 2 | 3 | 3 | 5 | **17/30** |
| B (jsonc-parser) | 5 | 4 | 4 | 4 | 4 | 5 | **26/30** |
| C (markers) | 5 | 5 | 5 | 5 | 5 | 5 | **30/30** |

---

## Recommendation

### Base candidate for 0.2.0: **C** (Marker-based injection via JSONC/JSON5 comments)

Rationale:
1. **Ready to ship now**. All infrastructure already exists. No new dependencies. No new types. No new scope risk.
2. **Strongest ownership model**. Marker-based boundary is legible, SHA256 drift detection is structural and provable, refusal on drift is explicit.
3. **No hidden surprises**. Format preservation is by construction (text splice). Idempotency is guaranteed by recompute-and-compare. Every contract is transparent.
4. **Honest scope boundary**. Strict JSON unsupported, but alternatives are clearly named (rename to `.jsonc`, manage whole file, wait for structured mode).
5. **Proven precedent**. Ansible `blockinfile` is 15+ years of production evidence.

Candidate C scores **30/30** on the rubric. It is not perfect because strict JSON has no comment syntax — but that's a real-world limitation, not a design flaw, and the design addresses it honestly.

### Structured mode (either A or B) is a decided-but-deferred 0.3.0+ fast-follow

- **If adopting A (jaq)**: Accept the whole-file rewrite cost in exchange for filter expressiveness. Best for templates where regeneration is acceptable (machine-generated config, build scripts, Dockerfile).
- **If adopting B (jsonc-parser CST)**: Prefer this over A. It preserves format and comments, has stronger ownership semantics (path-based, convergent), and is architecturally identical to `toml_edit` (the reference pattern). Best for user-facing config files (TypeScript, ESLint, `package.json` with careful author practice).

Candidate B is the superior structured-mode design (**26/30** vs. **17/30**) and should inform any future implementation. Candidate A is a simpler alternative if jaq expertise is already present, but Candidate B's constraints are more aligned with Toha's principles.

### Graft these ideas from the deferred candidates:

From B:
- Path-based ownership model (`path: "a.b.c"`) instead of filter syntax.
- Discriminator on `struct: { path, value }` to separate from `region`-based entries, composable in one `inject:` list.
- Optional `provenance: true` for JSONC drift awareness (best-effort, not gating convergence).
- Pure function signature for resolve: `Option<&[u8]> × PlannedEdit → Result<Resolution>`.

---

## Ship-Now vs. Defer Question

**Consensus across all three designs**: Defer structured mode to 0.3.0+. Markers are sufficient for 0.2.0.

**My assessment**: This consensus is **correct**. Here's why:

1. **0.2.0 validates the core**. Shipping marker-based injection (Candidate C) proves the injection concept, gets user feedback, and stabilizes the `inject:` YAML surface. It costs zero new dependencies and zero new code paths.

2. **Structured mode is optional, not foundational**. Most template authors can express JSON-family injection via markers (awkward for strict JSON, but possible). Structured mode is an ergonomic upgrade, not a blocker.

3. **Market need is unproven**. The research shows three competing patterns: marker injection (Ansible, now Toha), regenerate-old + regenerate-new + git-diff (Copier, Cruft), and surgical CST edits (VS Code's jsonc-parser). None of them do structured JSON merge in a production scaffolding tool yet. Toha can validate the marker approach first.

4. **Scope risk is real**. 0.2.0 is already the "first slice" with full marker-based system. Adding structured mode now (either A or B) risks delaying marker-mode shipping if jaq integration or CST integration discovers issues.

5. **Interface boundary is clear**. Both A and B have fully sketched interfaces (StructuredMerge/PlannedStructured, or StructPath/PlannedEdit). Shipping now means carrying both marker and structured types in the schema; deferring means adding them in a follow-up design without rework.

**Conclusion**: **0.2.0 ships Candidate C (markers). Structured mode is decided (likely Candidate B) but deferred to 0.3.0+, with interface and types fully pre-specified for zero surprise on handoff.**

---

## Final Verdict

| Candidate | Verdict | Score | Recommendation |
|---|---|---|---|
| A (jaq) | Well-designed, but reserialize cost is high; format loss is unconditional. | 17/30 | Defer. Consider as 0.3.0+ option if filter-based mutations are desired, but Candidate B is superior. |
| B (jsonc-parser) | Excellent design. Format preservation is perfect. Drift detection is weak in strict JSON but honest. Interface is superior to A. | 26/30 | Defer to 0.3.0+ fast-follow. Should inform any future structured-mode implementation. Graft interface design into final spec. |
| C (markers) | Strongest contract. Ready to ship now. Honest scope boundary (strict JSON unsupported). Proven at scale. | 30/30 | **Recommend for 0.2.0.** Ship immediately. |

---

## Signatures

All evidence citations verified against source clones:
- jaq: `/workspaces/references/toha/jaq/jaq-json/src/read.rs:10-19`, `write.rs:202-260`, `lib.rs:117`
- jsonc-parser: `/workspaces/references/toha/jsonc-parser/src/cst/mod.rs:1-5, 6-33, 1146, 1356, 1798-1858`
- Ansible: `/workspaces/references/toha/ansible/lib/ansible/modules/blockinfile.py:41, 323-324, 334-367`
- Scaffold: `/workspaces/references/toha/scaffold/app/scaffold/injector.go:27-82`
- npm precedent: `/workspaces/references/toha/npm-package-json/lib/index.js:239-264`

All scoring is independent, based on verification against actual source code and honest assessment of tradeoffs named by each design itself.
