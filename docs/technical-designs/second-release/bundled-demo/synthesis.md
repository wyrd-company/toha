# Bundled offline toha-demo — synthesis

The final design in `design.md` is Candidate 1 (Direction A) as the base, with
three grafts and a set of explicit rejections. Each graft names its source and
reason; each rejection names what it discards and why.

## Base

**Candidate 1 (reserved-name resolution branch + on-demand materialization).**
Chosen for interface depth: the whole feature sits behind one new CLI-only module
(`src/cli/bundled.rs`) and internal branches in three existing resolution
functions. `ResolvedTemplate` gains no field or variant, no registry entry is
created, and no module below the resolution seam learns the word "embedded". The
commit is derived at runtime from the embedded bytes, so it cannot drift from the
embed. It scored 30/30 with no red-flag hits and preserves every current
name-resolution outcome by construction (the bundled branch is reachable only on
the exact `NotFound("toha-demo")` that fails today).

## Grafts

- **G1 — Option-returning `bundled::resume`, placed before the git-parse
  fallback (from C3).** C1 intercepted resume with an early
  `if is_reserved(formal) { return … }` at the top of `resume_template`. C3's
  `bundled::resume(formal, commit, dirs) -> Option<Result<…>>` is cleaner: it
  returns `None` for a non-bundled record so every existing resume path
  (empty-commit, registry-entry-match) stays literally unchanged, and is placed
  immediately before the git-parse fallback — the exact point where a bundled
  record would otherwise die at the `Address::Git` guard. Adopted. Reason: same
  behaviour, strictly smaller edit to existing control flow.
- **G2 — precise length-prefixed canonical digest (from C3).** Both C1 and C3
  hash a sorted manifest of the embedded tree; C3 length-prefixes *both* the
  path and the contents (`len(path) ‖ path ‖ len(bytes) ‖ bytes`, files sorted by
  path) so no rename/split of files can produce a colliding digest. Adopted as
  the digest definition. Reason: collision-resistance rigor at no cost.
- **G3 — presentation-only discoverability row in `templates list` (re-shaped
  from C2).** C2's strongest idea is that a resolvable `toha-demo` should be
  visible, so a user can see why the name resolves. C2 achieves it by making the
  demo a registry layer (the rejected part). The graft keeps the idea and drops
  the mechanism: `templates list` (unfiltered) appends one honest, read-only row
  built from the `bundled` module — it is **not** a registry entry, does **not**
  participate in resolution, aliasing, or ambiguity, and is absent from
  `--system/--user/--local` (which read files). This touches only
  `src/cli/templates.rs` presentation, keeping C1's clean resolution seam.
  Included as a decision (D3) with a recommendation to show it.

## Rejections

- **C2's `Layer::Bundled` registry variant + `seed_fallback` + `by_name`/
  `formal_only` rewrites.** Rejected: information leakage into the library, the
  largest blast radius, a serialized `Layer::Bundled` contract change, and a
  `named=true` inconsistency (a named entry that can never be trusted). The
  entire `by_name`/`formal_only` machinery exists only to solve a collision that
  C2 itself creates by minting a formal-key entry. C1/C3 preserve the same
  outcomes — including a user's `alias X toha-demo` and a user template whose
  short name is `toha-demo` — with no registry change at all, because no bundled
  entry exists to conflict with.
- **C2's build-time `env!("TOHA_BUNDLED_DEMO_DIGEST")` commit.** Rejected in
  favour of the runtime `commit()` (LazyLock over the embedded bytes). A runtime
  digest is computed from the same bytes `include_dir!` embeds, so it cannot go
  stale relative to the embed; the build-time env var can. A freshness test is
  still kept as belt-and-suspenders (V-check), but it is not load-bearing for
  correctness.
- **C3's `into_temp()` as unconditional default behaviour.** Not adopted as a
  silent default: it writes to OS temp (a runtime write location outside the
  documented cache) and leaks the directory (`dir.keep()`, relying on the OS
  reaper). The *resilience idea* is preserved as an explicit decision (D2) for
  Bob, not baked in.
- **C3's optional 40-hex SHA truncation.** Rejected: full 64-hex already
  satisfies the commit pattern; truncation only reduces collision resistance.
- **C3's new `ResolveError::message` helper.** Rejected in favour of the existing
  private `ResolveError::text` (`src/cli/resolve.rs:31`); expose it `pub(crate)`
  if the `bundled` module needs it rather than adding a second constructor.

## Decisions surfaced to Bob (Phase C)

Recorded in full in `design.md` §Decisions:

- **D1 — Collision precedence:** fallback-only (recommended) vs
  reserved-name-wins (narrows resolution → needs approval).
- **D2 — Cache-unwritable behaviour:** fail with a clear error (recommended,
  keeps writes inside the documented cache) vs degrade to a temp dir (more
  resilient, introduces a temp write location — a disclosable capability
  question).
- **D3 — Discoverability:** show one read-only bundled row in unfiltered
  `templates list` (recommended) vs keep it invisible.
- **D4 — Identity for downstream 1073/1075:** confirm `formal_name="toha-demo"`
  (stable key) + `commit=<content digest>` (content version) is the coordination
  surface those designs key on.

## Verification

See `verification.md`: caller usage reconciled against the sketch, every
requirement/constraint checked against the design, failure cases mapped to
falsifiable tests, code seams verified against the real tree, and compatibility
with predecessors confirmed (this design has no predecessor-design prerequisite).
