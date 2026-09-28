# Synthesis — content injection

Phase C–F record. Base pick, scores (self + cross-judge), grafts, rejections,
dropouts, and verification. The final design is `05-design.md`.

## Runners and dropouts

Four foreground candidates, cross-family, each championing a structurally
distinct mechanism (see `02-arena-rubric.md`). All four produced complete
packages; **no dropouts**. Cross-judge: readonly, `ocx-gpt-5-6-luna` (family
distinct from the parent opus); report preserved at
`03-candidates/cross-judge-report.md`.

## Scores (1–5 per criterion)

Self-scores (parent, opus) with the cross-judge in brackets where it differs.

| Criterion | C1 markers | C2 anchor+guard | C3 structured | C4 unified |
|---|:--:|:--:|:--:|:--:|
| 1 Structural idempotency | 5 | 4 | 3 | 2 |
| 2 Ownership / mutation contract | 5 | 3 | 3 | 4 |
| 3 Purity & replay | 5 | 3 | 3 | 2 |
| 4 Conflict / failure / recovery | 5 | 3 | 2 | 2 |
| 5 Interface depth / small surface | 5 [4] | 3 | 2 | 3 |
| 6 Scope honesty on formats | 5 | 4 | 1 | 2 |
| **Total** | **30 [29]** | 20 | 14 | 15 |

The self/judge split on C1 criterion 5 (5 vs 4) is not a disagreement worth
reconciling: both rank C1 first by a wide margin. The one place my read is
stricter than the judge's is on **C2 and C4 correctness hazards**, below — my
lower purity scores reflect defects the judge also flagged.

## Base: Candidate 1 (managed-region markers)

C1 wins on every criterion. Its load-bearing move is a **pure function
`resolve_edit(current_bytes, edit) -> Unchanged | Write(full_bytes)`** whose
`Write` arm always recomputes a body checksum recorded in the end marker.
Because the checksum is recomputed over the emitted body, resolving C1's own
output again yields `Unchanged` — so idempotency is a proven structural
consequence, not a heuristic, and it survives staging replay because
`PlannedEdit` is a pure function of `(Template, Completed)` and the only target
read happens at apply/dry-run. Ownership is the byte span between a keyed
begin/end marker pair; the checksum distinguishes "Toha's last body" (safe to
replace) from "user edit" (drift). This is exactly the mutation contract 1066
and 1031 need.

A future maintainer extends C1 most easily: one `template.yml` list, one
`Plan.edits` vector, one pure function, one derived `FileMutation` view. The
whole-file path is untouched, so no existing fixture regresses.

## Grafts (source → what was folded in, by hand)

1. **From C4 → the derived mutation contract is enriched, not the internal
   representation.** C4's instinct to give downstream ONE typed contract is
   right; its execution (collapsing `Content`/`PlannedFile` into a single
   `FileMutation` enum, with build-time byte offsets) is not. Graft: keep C1's
   `edits` sibling as the internal representation, and expose
   `Plan::mutations() -> FileMutation` as a **derived read-model** enriched with
   C4's `OwnershipScope` vocabulary (`EntireFile` vs `Region { path, key }`) and
   the region checksum, so 1066 reads a first-class ownership contract without
   the blast radius or replay hazard of C4's collapse.

2. **From C3 → structured-merge is a named, reserved, out-of-scope mode with an
   explicit ownership model.** C3's "Toha owns the value at a named path"
   ownership is genuinely the right model *for structured documents* and worth
   preserving. Graft: reserve a `BodySource`-free structured variant in the
   design's type space and document the "owns the value at path" ownership, but
   place it **out of scope for 1031** behind an explicit caveat that it
   reserializes (destroying comments/formatting/key order), so it is never on
   the critical path and never silently rewrites a user-owned file by default.
   Explicit-`format` discipline (never inferred) is adopted for that future
   mode.

3. **From C2 → cardinality/anchor vocabulary and the exact-bytes discipline.**
   C2's anchor cardinality naming is clear; folded into C1's `Occurrence`
   (`First`/`Last`/`Only`, `Only` ⇒ ambiguous error). C2's optional `replace`
   insert mode and `line` anchor are recorded as reserved options, not first-cut
   surface. C2's `.lines().join("\n")` implementation exposed a hazard worth
   binding as a verification requirement (below).

## Rejections (highest-signal record)

- **C2's two-phase `build_injections` impure step + mutable plan status.**
  Rejected in favor of C1's pure `resolve_edit(current_bytes, edit)` invoked at
  apply/dry-run. The two-phase design mutates the plan (`inj.final_content`,
  `inj.status`) and adds a caller-visible step; C1 keeps the plan immutable and
  the read at the write boundary, a cleaner purity story.
- **C2's guard-as-substring authored into the content by the template author.**
  Rejected: idempotency then depends on author discipline (a missing/renamed
  guard silently duplicates). C1 has *Toha* write the markers, so the guarantee
  is structural.
- **C2's `.lines().collect().join("\n")` region editing.** Rejected as an
  implementation shape: it normalizes CRLF/LF and can drop or add a trailing
  newline, which would change bytes outside Toha's region on first apply and can
  break idempotency of the surrounding, user-owned content. The region editor
  must splice byte ranges and preserve the rest of the file verbatim (bound as a
  verification item).
- **C3's structured merge as the core mechanism.** Rejected: it cannot touch
  arbitrary text (README, `.gitignore`, source, commented CI YAML), and it
  reserializes — a disguised whole-file rewrite of a file Toha does not own,
  which weakens the no-silent-overwrite guarantee. Kept only as the reserved,
  out-of-scope mode above.
- **C4's collapse of `Content`/`PlannedFile` into one `FileMutation` enum as the
  internal representation, and its `IdempotencyCheck::WholeFile { expected_content }`.**
  Rejected: the whole-file idempotency-by-content-match changes today's
  existence⇒conflict semantics and would regress existing fixtures
  (`conflict`, `conflict-force`); and folding both mutation kinds into one enum
  enlarges the blast radius for no gain over a derived view.
- **C4's build-time byte-offset ownership (`BoundedRegion { start_byte, end_byte }`).**
  Rejected: byte offsets computed at build are invalidated by any change to the
  target between build and apply and do not survive replay. Selection is by
  marker at apply time; offsets, if surfaced at all, are derived at read time.

## Verification (Phase F)

- **Caller usage vs sketch.** Existing `Plan::build(..).apply(..)` callers are
  unchanged; the crate surface grows by one `Plan` field, one `template.yml`
  list, one derived `mutations()` view, and one pure `resolve_edit`. The
  `apply_reporting` insertion point (resolve edits after the whole-file conflict
  recompute, before writes, folding drift into the `--force` gate) preserves the
  all-or-nothing "write nothing on conflict / needs-trust" ordering. Holds.
- **Requirements/constraints vs design.** anchors/selection (anchor bootstrap +
  marker steady-state) ✓; idempotency (checksum, proven) ✓; conflict behavior
  (injection is not an existence conflict; *drift* is the conflict, gated by
  `--force`) ✓; ownership/mutation contract (span between markers + checksum +
  derived `FileMutation`) ✓; failure/recovery (error taxonomy, exit codes,
  atomic write) ✓; purity/replay (pure `resolve_edit`, engine reads no target)
  ✓; `TargetPath`/symlink/`.git` reuse ✓; no new port/timeout/permission/
  subprocess/pinned-check ✓; compose with whole-file writes (files first, then
  edits; same-path write-then-inject allowed) ✓.
- **Dependency check.** The region checksum reuses the **existing** `sha2 = "0.10"`
  dependency (`Cargo.toml:43`, used today for staged file names). No new crate;
  the C1 open question is resolved.
- **Falsifiable scenarios** (become 1031 fixtures): (1) *twice-apply changes the
  file once* — the acceptance fixture, sole assertion on idempotency; (2)
  first-placement at an anchor and at EOF; (3) template body change replaces only
  the region; (4) user drift inside markers refuses without `--force`, exit 1,
  overwrites with `--force`; (5) missing anchor exit 1, ambiguous (`Only`) exit
  5; (6) whole-file write + injection into the same file in one plan; (7)
  arbitrary text targets (`.gitignore`, a `.rs` source) exercising comment-style
  inference; (8) staged-replay double-apply equals single apply.
- **Predecessor compatibility.** 1068 is a design root with no predecessor
  design; nothing to reconcile. Downstream 1066/1029 consume the mutation
  contract named in `05-design.md`.

No problem surfaced that the arena missed; the base holds under scrutiny. Two
refinements beyond C1 are folded into the final design: reuse existing `sha2`
(no new dep), and an atomic temp-file-plus-rename write for injection targets
(a user-owned file must not be left half-written), recorded as decisions D7/D8.
