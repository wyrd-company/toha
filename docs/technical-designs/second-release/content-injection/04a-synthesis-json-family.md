# Synthesis addendum — JSON-family structured mutation

Extends `04-synthesis.md`. This record reconciles the focused JSON-family arena
in `02a-arena-json-family.md` with the selected product boundary.

## Scores (1–5 per criterion)

Parent self-scores and the readonly cross-judge agreed on every total. The full
judge report is
`03-candidates/json-family-revision/cross-judge-report.md`.

| Criterion | A jaq | B jsonc-parser CST | C markers/JSONC |
|---|:--:|:--:|:--:|
| 1 Format/comment/order preservation | 1 | 5 | 5 |
| 2 Ownership / drift / idempotency | 3 | 4 | 5 |
| 3 Conflict / failure / recovery | 2 | 4 | 5 |
| 4 Interface depth / composition | 3 | 4 | 5 |
| 5 Scope honesty & dependency | 3 | 4 | 5 |
| 6 Evidence quality | 5 | 5 | 5 |
| **Total** | **17** | **26** | **30** |

Candidate C scored highest as a marker mechanism because its visible boundary
and checksum give it the strongest drift proof. Candidate B scored highest as a
structured mechanism and is the selected JSON-family design. The product choice
uses the stronger mechanism for each format family instead of forcing one
mechanism across both.

## Selected combination

- **Non-JSON text:** the base managed-region marker design. Toha owns the marked
  byte span and refuses an edited span without `--force`.
- **JSON family (`.json`, `.jsonc`, `.json5`):** Candidate B,
  `jsonc-parser`'s `cst` module, in the paired implementation. Toha owns the
  value at a typed path and converges that value to the declared value.
- **JSON-family markers:** rejected. JSONC and JSON5 could carry comments, but
  one JSON-family contract avoids a file-extension-dependent ownership model.
- **`jaq`:** rejected as the structured mechanism. Its reserialize model drops
  source fidelity that the selected CST retains.

This means visible content markers remain part of the product, but no Toha
marker is added to a JSON-family document. Existing user comments in JSONC and
JSON5 remain source text that `jsonc-parser` preserves.

## Candidate B contract

The embedded `jsonc-parser` CST owns **one JSON value at one typed path**. It
preserves comments, property order, whitespace, quote style, and punctuation
outside the node and punctuation range that the edit must change. A value that
already equals the desired typed value returns `Unchanged`, so repeated apply is
a byte no-op.

Strict JSON has no place for a durable ownership checksum. The selected contract
is therefore declarative convergence: a later apply replaces an operator edit
at the owned path without requiring `--force`. This is not silent whole-file
ownership. The author opted into ownership of that exact path; all unrelated
source text remains operator-owned and is preserved.

JSONC and JSON5 do not receive a Toha provenance comment. Their existing
comments are preserved, but the ownership contract is the same typed-path
contract as strict JSON.

## Path and value shape

The author surface remains the approved `struct:` discriminator:

```yaml
inject:
  - into: "package.json"
    struct:
      path: "scripts.build"
      value: "tsc"
```

`path` is parsed during planning into key and array-index segments. Keys use dot
notation, array indexes use brackets, and backslash escapes `.`, `[`, `]`, and
`\\` inside a key. The path must begin with a key. Missing object-key parents
are created as objects; arrays must already exist and an index must be in range.
An existing scalar where traversal needs an object or array is an error.

`value` is a JSON-compatible YAML value. Strings are ordinary templates and
remain JSON strings after rendering. Boolean, number, null, array, and object
literals keep their JSON type; strings nested inside arrays and objects render
recursively. Planning stores the result as `serde_json::Value`, so apply never
guesses a type from text.

## Conflict and composition rules

- Duplicate `(target, typed path)` rules are planning errors.
- Ancestor/descendant paths in one plan are planning errors because their order
  would change the meaning of ownership.
- Distinct, non-overlapping paths in one file are folded in source order into
  one in-memory file image and committed with one atomic replacement.
- A whole-file rule for the same target supplies the starting bytes; JSON
  mutations then apply to that in-memory result.
- Any planning, parse, path, drift, or trust failure is found before the first
  file write. Atomic replacement is per target; an operating-system failure
  during the commit loop retains the existing partial-across-files behavior.

## Why `jaq` remains rejected

`jaq` discards `#` comments at lex time
(`jaq-json/src/read.rs:10-19`, revision `f167ad4`) and regenerates output through
its printer (`jaq-json/src/write.rs:202-260`). It is a query and transform
engine. Editing one value through it would make the entire source document a
generated result. The same disqualifier applies to `serde_json` and `json5-rs`.

`jsonc-parser` retains comments and whitespace as CST tokens and writes the
original source plus the requested edit (`jsonc-parser/src/cst/mod.rs:1-5,1356`,
revision `c7d4cf5`). This is the selected embedded dependency; no command or
subprocess is involved.

## Verification delta

The paired implementation proves all of these conditions:

1. Strict JSON, JSONC, and JSON5 each accept a typed-path edit and are byte-no-op
   on the second apply.
2. JSONC and JSON5 comments, ordering, whitespace, and untouched values survive
   an insert and a replacement.
3. String, boolean, number, null, array, and object values retain their type.
4. A changed owned value converges without `--force`; unrelated text never
   changes.
5. Missing object parents are created; invalid traversal, absent/out-of-range
   arrays, malformed documents, duplicate paths, and overlapping paths fail
   before any write.
6. A marker rule targeting `.json`, `.jsonc`, or `.json5` is rejected during
   planning with guidance to use `struct:` or whole-file generation.
7. Multiple edits to one JSON-family file and a whole-file-plus-structured plan
   produce one final in-memory image and one atomic target replacement.

The focused arena evidence remains intact. The final design uses Candidate B
for the JSON family and Candidate C's marker mechanism for other text formats.
