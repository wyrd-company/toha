# Synthesis — parse stdout as JSON into `<id>`

Inputs: v1 `design.md`; the JSON-revision grounding (`06-...`); two blind
independent candidates (`03-candidates/json-revision/independent-A`, `-B`,
rubric withheld, isolated outputs, each blind to the other); and the independent
read-only cross-judge (`07-json-cross-judge.md`). Both blind runners
independently converged on the core reconciliation (parsed value at `<id>`,
metadata at a separately author-declared identifier). The v1 three decisions
remain **open** — Bob's addition does not approve them.

## Own scoring (reconciled with the cross-judge)

| Candidate | C1 | C2 | C3 | C4 | C5 | C6 | Weighted /70 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| json-A (`status-id`, `<id>` undefined when unrun) | 5 | 4 | 5 | 4 | 4 | 5 | 63 |
| json-B (declared `metadata`, `<id>` none, load-guard) | 5 | 5 | 5 | 5 | 4 | 5 | 68 |

I agree with the ranking and the base. My scores match; I independently confirmed
that A's `undefined`-when-unrun breaks v1 invariant 3 (a statically available
name becomes undefined at run time, turning a forgotten guard into a
files-already-written render fault), and that B keeps "always bound, one guard
idiom."

## Base and grafts

**Base:** json-B (parsed value at `<id>`; metadata at a declared name; `<id>` is
`none` never undefined; a best-effort attribute-form load guard).

| Graft | Source | Reason |
| --- | --- | --- |
| Field name `status-id` (an identifier, like `id`) | A | Reads as a name, not a value (`metadata: report_run` reads like a value). |
| One `ApplyError::HookOutput { fault: OutputFault }` with `NotUtf8{valid_up_to} \| Empty \| NotJson{line,column}` | A | All stdout-content faults share one variant and one "no bytes in Display" guard; drop B's separate `HookJson`. |
| Apply-loop order: **failure check first**, then decode, then parse | A | An untolerated nonzero/signal must surface the v1 `Hook` error, never a parse fault (B's pseudo-code had parse first, contradicting its own prose). |
| Explicit BOM / several-documents → `NotJson` rows, and the all-JSON-types example | A | Every edge has a falsifiable row; the guide shows every type once. |
| Status shape `{exit_code, stdout, stderr, parsed}` (raw stdout kept; `parsed` bool) | B | A tolerated non-JSON failure stays diagnosable; `parsed` disambiguates JSON `null` from "did not parse". |
| `<id>` = `none` when the producer did not run or a tolerated failure did not parse; never `undefined` | B | Preserves v1 invariant 3 and one guard idiom (`<status-id>.exit_code is none` / `<status-id>.parsed`). |
| `status-id` **required when the producer may not run** (own `when`, an enclosing group/branch `when`, or a skippable interview position) **or has `allow-failure`**, computed from the retained program | B (fixed per cross-judge §2) | Otherwise a `none` `<id>` is ambiguous with no disambiguator. The trigger is "may not run", not merely "has `when`". |
| Attribute-form `<id>.exit_code`/`.stdout`/`.stderr` on a JSON hook is a **best-effort** load error; the subscript form `<id>['exit_code']` is the escape for a real JSON key | B | Catches the likely migration mistake (flip a v1 hook to `parse: json`, `lint.exit_code != 0` silently becomes `undefined != 0`). The analyzer distinguishes `GetAttr` from `GetItem` (minijinja `meta.rs`, `undeclared_variables(true)` — which v1 already needs). Degrades gracefully: aliasing, loop variables, and non-variable roots fall through to a plain key read (disclosed). |
| `parse: json` requires `capture: [stdout]` | B | `capture` stays the single control for "stdout is piped, not shown"; no exception to v1 invariant 4. (Bob decision 3e: implied vs required — recommend required.) |
| Invariant: a parsed string is **data, not a template** (never re-rendered) | B | Forecloses a future re-render foot-gun. |
| The 8-row apply-state table and `verification.md` checks | B | Falsifiable targets for impl 1063. |

## Rejections

| Rejected | Source | Reason |
| --- | --- | --- |
| `<id>` undefined when unrun | A | Breaks v1 invariant 3; moves a guard omission to after files are written. |
| Raw stdout dropped in JSON mode | A | A tolerated non-JSON failure cannot be diagnosed. |
| "No field check on `<id>`" | A | Loses the only load-time catch for the migration mistake; the analyzer supports the check. |
| `capture: [stdout]` + `parse` as a load error | A | Adds an exception to v1's capture invariant to save one line. |
| Separate `HookJson` error variant | B | Duplicates `HookOutput`; merged. |
| Metadata-required trigger = "has `when`" | B | Misses group/branch-skipped interview producers; use "may not run". |
| Parse-before-failure-check order | B | Contradicts v1 error precedence and B's own prose. |

## Corrections baked in (from the cross-judge)

1. **Apply order:** run → failure check (untolerated nonzero or signal → v1
   `ApplyError::Hook`, nothing parsed) → decode strict-UTF-8 (`NotUtf8` fatal) →
   parse (exit 0: `Empty`/`NotJson` fatal; tolerated nonzero: lenient, failure →
   `<id>` `none`, `parsed` false, continue).
2. **`status-id` trigger** is "the producer may not run" (from the retained
   program) or `allow-failure`, not "has `when`".
3. **Attribute-form guard is best-effort**; the aliasing/loop/non-variable-root
   blind spots are disclosed, and each degrades to a plain key read.
4. **serde_json integer precision:** values above `u64` become `f64` (no
   `arbitrary_precision`); disclosed.
5. **Read-through-`none`** (`<id>.key` when `<id>` is `none`) under minijinja's
   undefined mode needs a 1063 fixture; recorded as a validation target.
6. **Analyzer:** the attribute checks reuse `undeclared_variables(true)`, which
   v1 already requires; no new analyzer capability.

## Synthesized JSON mode (one paragraph)

A producer opts in with `parse: json` (which requires `id` and `capture:
[stdout]`). Its `<id>` then carries the parsed stdout as a plain JSON value —
object keys as `<id>.key`, arrays as `<id>[i]`, scalars directly, `null` as
`none`. Execution metadata moves to an optional author-declared `status-id:
<name>` (an ordinary identifier, no new reserved name) bound to `{exit_code,
stdout, stderr, parsed}`; `status-id` is **required** when the producer may not
run or tolerates failure, so a `none` `<id>` is never ambiguous. On exit 0,
empty or malformed stdout stops the apply with a byte-free `HookOutput` fault; a
tolerated nonzero exit parses leniently and, on failure, leaves `<id>` `none`
with `parsed: false` and the raw text in `<status-id>.stdout`. Writing
`<id>.exit_code`/`.stdout`/`.stderr` on a JSON hook is a best-effort load error
that names the subscript escape and the `status-id`. `serde_json` is already a
dependency; nothing else changes from v1.

## Decisions surfaced to Bob (Phase C — all still open)

1–3. The **v1 three** (readable-surface breadth; hook `id` shares the answer-id
space; strict-UTF-8 decode) remain open; Bob's JSON request approves none of
them.
4. **JSON mode shape** (this synthesis): `parse: json` → `<id>` is the parsed
   value; metadata at an optional/required `status-id`. Approve the exact shape.
5. **Capture for JSON mode:** `parse: json` requires an explicit `capture:
   [stdout]` (recommended) vs implying it. Two-way door.
