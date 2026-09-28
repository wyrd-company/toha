# Synthesis addendum — JSON-family / structured mutation

Extends `04-synthesis.md`. Base pick, scores (self + readonly cross-judge),
grafts, rejections, verification, and the recommendation for the JSON-family
question. Reconciles the focused re-arena (`02a-arena-json-family.md`).

**Status of this record.** Its "base pick", "rejected", "chosen", and "deferred"
verdicts are the *arena's recommendations* feeding Bob's ruling on D6a/D6b/D9
(`05-design.md`). None of it is settled scope, an authorized structured-mode
deferral, or a 1031 narrowing until Bob rules. The implement-now option for the
structured mode remains co-equal on D6b.

## Scores (1–5 per criterion)

Parent self-scores (opus); cross-judge (`ocx-gpt-5-6-luna`) in brackets where it
differs. Full judge report: `03-candidates/json-family-revision/cross-judge-report.md`.

| Criterion | A jaq | B jsonc-parser CST | C markers/JSONC |
|---|:--:|:--:|:--:|
| 1 Format/comment/order preservation | 1 | 5 | 5 |
| 2 Ownership / drift / idempotency | 3 | 4 | 5 |
| 3 Conflict / failure / recovery | 2 | 4 | 5 |
| 4 Interface depth / composition | 3 | 4 | 5 |
| 5 Scope honesty & dependency | 3 | 4 | 5 |
| 6 Evidence quality | 5 | 5 | 5 |
| **Total** | **17** | **26** | **30** |

Self and cross-judge agreed on every total. No disagreement needed
reconciliation: both rank C first (ready to ship, strongest ownership contract,
zero new dependency, proven by Ansible `blockinfile`), B second (the superior
*structured* design, format-preserving, but a heavier addition), A last (jaq
reserializes — the disqualifier).

## Base pick for the 0.2.0 slice (1031): Candidate C

The JSON family ships **through the retained marker base**: JSONC and JSON5
permit `//` comments, so Toha's begin/end markers are valid comment lines and
inject exactly as they do in `.rs`/`.gitignore`/`.yaml`, with the same `sha2`
checksum idempotency and drift refusal. Strict `.json` is a **documented
boundary** — markers would break JSON validity — with named author alternatives
(target a `.jsonc`, manage the whole file with `files:`, or use the deferred
structured mode). This is not a regression: today Toha cannot inject at all, and
the ecosystem's own package.json editor reserializes rather than surgically
editing strict JSON.

## The jaq question, answered on evidence (Candidate A rejected as mechanism)

`jaq` is **rejected as the structured mechanism.** It discards `#` comments at
lex time (`jaq-json/src/read.rs:10-19`, rev `f167ad4`) and regenerates all output
from a pretty printer (`jaq-json/src/write.rs:202-260`); it is a jq-language
filter/query engine, not an editor. Using it to edit a user-owned JSON-family
file is a disguised whole-file rewrite that destroys comments and normalizes
formatting/order — the exact no-silent-overwrite hazard the grounding forbids —
and it imposes a query language for what is usually a single-value set. The same
disqualifier applies to every reserialize approach (`serde_json`, `json5-rs`;
npm's package.json editor mitigates only indent/newline, never comments). This
confirms and hardens the base grounding's read with cited source, and supersedes
the assumption in prior D6 that structured merge would be a jaq/serde path.

## The structured mode, when Toha adds one: Candidate B (jsonc-parser CST)

If/when Toha ships a structured JSON-family mode, the mechanism is **`jsonc-parser`'s
`cst` module** (rev `c7d4cf5`, crate `0.34.0`, feature `cst`), the format-
preserving JSON-family analogue of `toml_edit` and the same pattern VS Code uses.
It owns **the value at a named path** (not a byte region), preserves every
comment/key/whitespace outside the touched node (`cst/mod.rs:1-5,1356`), and is a
byte no-op when the value already equals the desired value (idempotent). Its
honest limit: strict JSON has no comment slot for a checksum, so a re-apply
cannot distinguish "user edited the managed value" from "first set" — the model
is declarative convergence (Toha re-sets exactly the path it owns, touching
nothing else); JSONC can optionally carry an inline provenance comment for
awareness.

## Grafts (source → what is folded in)

1. **From B → the reserved structured interface and its ownership model.** The
   deferred structured mode is decided down to its shape: a `struct: { path,
   value }` discriminator inside the same `inject:` list (sibling to `region`),
   path-based ownership ("owns the value at a path"), declarative convergence for
   idempotency, an optional JSONC `provenance` comment, and a pure resolver
   `resolve_struct_edit(current_bytes, edit) -> Unchanged | Write` mirroring the
   marker resolver. This is reserved in the design's type space so a fast-follow
   lands non-breaking, exactly as the base synthesis reserved a structured mode —
   but now with the mechanism named (jsonc-parser CST) instead of left open.
2. **From C → the base, entire.** Markers over JSONC/JSON5 plus the documented
   strict-JSON boundary is the 0.2.0 answer.
3. **From A → nothing adopted.** jaq is retained only as a rejected alternative
   with cited reasoning, and as a note that a future filter-transform mode (if
   ever wanted for machine-generated output where reserialize is acceptable)
   would be a distinct, opt-in, explicitly-formatted concern — never the default
   editor for user-owned files.

## Rejections

- **jaq / any reserialize as the structured editor** — destroys comments/order/
  formatting of a user-owned file (cited above). Rejected.
- **Shipping the structured mode in 0.2.0/1031** — the arena and cross-judge
  independently *recommend against* it: it adds a second mechanism, a new
  dependency, a distinct ownership contract, and new error/dry-run semantics to
  the first-ever injection slice, for an ergonomic gain over markers that
  JSONC/JSON5 already deliver. Recommendation is to sequence it as a fast-follow
  with the interface seam reserved; the implement-now option stays co-equal on
  D6b for Bob to choose.
- **Requiring JSONC for all JSON (never adding structured)** — too narrow: it
  leaves strict-`.json` value management permanently to whole-file mode. The
  recommended (pending D6b) structured mode keeps that door open without burdening
  0.2.0.

## Verification (Phase F, delta)

- **Requirements vs design.** JSON-family injection addressed (markers for
  JSONC/JSON5; proposed boundary for strict JSON; a recommended structured
  mechanism and sequencing). jaq evaluated with cited source and recommended
  against. Structured mutations reconsidered rather than assumed out of scope —
  the reconsideration produced a *recommended mechanism* (jsonc-parser CST) plus a
  reserved interface, feeding the revised D6/D9 for Bob's ruling (not a settled
  supersession).
- **Purity/replay.** The deferred structured resolver is specified as a pure
  function of `(current_bytes, edit)` like the marker resolver; the engine still
  reads no target bytes. Holds.
- **No-silent-overwrite.** The base marker refusal is unchanged. The rejection of
  jaq is precisely to protect this guarantee. The deferred structured mode's
  convergence is bounded to the exact owned path (never the rest of the file),
  which is the structured analogue of the marker span; this is called out as the
  decision Bob must ratify before that mode is built.
- **Dependency.** No new dependency is added in 1031 (markers reuse existing
  `sha2`). `jsonc-parser` is adopted only when the deferred structured mode is
  built; its adoption is justified (used by dprint/deno, actively maintained, no
  subprocess — an embedded crate) and recorded now so the fast-follow does not
  re-litigate it.
- **Falsifiable scenarios (delta for 1031).** Add: injection into a `.jsonc`
  target places valid `//` markers and is twice-apply idempotent; a strict
  `.json` marker injection is refused at build time naming the boundary and the
  alternatives. The structured-mode scenarios (byte-preserving set into `.json`
  and `.jsonc`, convergence, JSONC provenance) belong to the fast-follow design's
  fixtures, not 1031.

The base holds and is strengthened: the marker mechanism now has explicit
JSON-family coverage and precedent, and the structured question is resolved with
a decided mechanism rather than an open reservation.
