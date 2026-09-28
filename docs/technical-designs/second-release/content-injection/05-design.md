# Content injection into existing files — final design

This design adds two bounded mutation modes to Toha's existing plan-and-apply
flow:

- visible managed regions for non-JSON text files; and
- typed-path mutation through embedded `jsonc-parser` for JSON, JSONC, and
  JSON5.

The arena evidence is preserved in `02-arena-rubric.md`, `03-candidates/`, and
`04-synthesis.md`. The focused JSON-family evidence is preserved in
`01a-grounding-addendum-json-family.md`, `02a-arena-json-family.md`, and
`04a-synthesis-json-family.md`. This package describes interfaces and behavior;
the paired implementation owns runtime and canonical specification changes.

## Problem

Every current `PlannedFile` replaces a whole target. Content injection changes
one bounded part of a file that an operator otherwise owns. The design must keep
five properties:

1. Interview evaluation remains pure and every terminal, headless, staged,
   direct, and crate caller follows the same plan.
2. A repeated apply is a byte no-op when the owned content already matches.
3. Target-path, symbolic-link, and `.git` protections apply before every read or
   write.
4. All mutations resolve before the first write, and hooks retain their existing
   trust gate and after-write order.
5. A whole-file rule and bounded mutations can compose in one plan.

One ownership mechanism does not fit every file. Most text formats can carry
visible comment markers and a checksum. JSON-family documents need a syntax-
aware edit because strict JSON has no comments. The selected design uses visible
markers outside the JSON family and a full-fidelity concrete syntax tree for
JSON, JSONC, and JSON5.

## Author experience

### Visible region in a non-JSON text file

```yaml
inject:
  - into: "config/app.toml"
    region: "features"
    content: |
      analytics_enabled = true
      analytics_sample_rate = 0.1
    anchor: { after: "[application]", occurrence: only }
```

First apply places a named region. Later applies find the markers rather than
the bootstrap anchor.

```toml
[application]
# >>> toha:region features >>>
analytics_enabled = true
analytics_sample_rate = 0.1
# <<< toha:end features sha256:a7f3d… <<<
name = "sample-service"
```

Toha owns the marker pair and the bytes between them. The checksum records the
body Toha last wrote. A matching body is unchanged. A template change replaces
only that span. An operator edit inside the span is drift and requires
`--force`; `--force` still cannot invent a missing anchor or repair malformed
markers.

### Typed value in a JSON-family file

```yaml
inject:
  - into: "package.json"
    struct:
      path: "scripts.build"
      value: "tsc"
```

Before:

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js"
  }
}
```

After:

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js",
    "build": "tsc"
  }
}
```

Toha owns the value at `scripts.build`. The second apply changes no bytes. If an
operator changes that owned value, a later apply converges it to `"tsc"`
without `--force`. Strict JSON cannot store a checksum, so the author contract
states this ownership directly. All unrelated keys and source text remain
operator-owned.

JSONC and JSON5 use this same `struct:` mode. Toha does not add marker or
provenance comments to JSON-family documents. Existing comments, key order,
whitespace, quote style, and trailing commas remain in the concrete syntax tree
and are retained outside the node and punctuation range that must change.

### Values and paths

`struct.value` is a JSON-compatible YAML value. String values are normal
templates and remain JSON strings after rendering. Boolean, number, null, array,
and object literals retain their type. Strings nested inside arrays and objects
render recursively. Planning stores the result as `serde_json::Value`, which is
already part of Toha, so apply never infers a type from rendered text.

`struct.path` uses dot-separated object keys and bracketed array indexes:
`routes[0].name`. Backslash escapes `.`, `[`, `]`, and `\\` in a key. The path
must begin with a key. Missing object-key parents are created as objects. Arrays
must already exist and each index must be in range. An existing scalar where
traversal needs an object or array is an error.

Marker rules whose target ends in `.json`, `.jsonc`, or `.json5` are planning
errors. The message directs the author to `struct:` or an existing whole-file
`files:` rule. Whole-file JSON generation remains supported.

## Caller behavior

Existing crate callers keep `Plan::build(..).apply(..)`:

```rust
let plan = Plan::build(&template, &completed, target)?;
match plan.apply(
    target,
    ApplyOptions { force: false, trusted: true },
    &runner,
)? {
    Applied::Written { files, .. } => { /* each changed path appears once */ }
    Applied::NeedsTrust(plan) => { /* unchanged hook trust gate */ }
}
```

CLI behavior:

- `toha apply <PATH>` reports each changed target once. Unchanged regions and
  values produce no report.
- `toha apply --dry-run <PATH>` reports `inject <path> (<owner>)` for a missing
  region or value and `update <path> (<owner>)` for a changed one.
- Marker drift without `--force` reports each `path (region)` and writes
  nothing. JSON value convergence is not drift and does not require `--force`.
- Missing or ambiguous marker anchors, malformed markers or JSON, invalid JSON
  traversal, and missing targets fail before any file write.

## Planned data

Injection is a sibling of `PlannedFile`. A whole-file plan already carries all
bytes to write; an injection must read and transform the target at apply time.
Keeping those concepts separate preserves the meaning of `Content`.

```rust
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub edits: Vec<PlannedEdit>,
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<PlannedHook>,
    pub before_apply: Option<String>,
    pub after_apply: Option<String>,
}

pub enum PlannedEdit {
    Region(PlannedRegionEdit),
    JsonValue(PlannedJsonEdit),
}

pub struct PlannedRegionEdit {
    pub path: TargetPath,
    pub region: RegionKey,
    pub body: String,
    pub marker: MarkerStyle,
    pub anchor: Option<Anchor>,
    pub create: bool,
    pub source: Option<PathBuf>,
}

pub struct PlannedJsonEdit {
    pub path: TargetPath,
    pub json_path: JsonPath,
    pub desired: serde_json::Value,
    pub format: JsonFormat,
    pub create: bool,
}

pub enum JsonFormat { Json, Jsonc, Json5 }

pub struct JsonPath(Vec<JsonPathSegment>);
pub enum JsonPathSegment { Key(String), Index(usize) }
```

`RegionKey` accepts lowercase ASCII letters, digits, `_`, and `-`. It is unique
per target. `MarkerStyle` is resolved from the non-JSON target extension or an
explicit override during planning. `JsonFormat` is resolved from the JSON-family
extension and selects one named `jsonc_parser::ParseOptions` bundle:

| `ParseOptions` field | `Json` | `Jsonc` | `Json5` |
|---|:---:|:---:|:---:|
| `allow_comments` | false | true | true |
| `allow_loose_object_property_names` | false | false | true |
| `allow_trailing_commas` | false | true | true |
| `allow_missing_commas` | false | false | true |
| `allow_single_quoted_strings` | false | false | true |
| `allow_hexadecimal_numbers` | false | false | true |
| `allow_unary_plus_numbers` | false | false | true |
| `allow_bare_decimal_point_numbers` | false | false | true |
| `allow_non_finite_numbers` | false | false | true |
| `allow_extended_string_escapes` | false | false | true |

The implementation constructs every bundle explicitly instead of inheriting
the dependency's all-enabled default. The `Json5` bundle is a JSON5-compatible
superset because it also permits missing commas; it is not strict JSON5 grammar
validation.

Two JSON rules with the same `(target, json_path)` are duplicates. An ancestor
and descendant pair, such as `compilerOptions` and
`compilerOptions.strict`, is also rejected because applying one changes the
meaning of the other. Distinct paths and distinct marker regions may share a
target when the file format permits them.

## Pure resolvers

Both modes return a complete candidate file and perform no I/O.

```rust
pub enum EditResolution {
    Unchanged,
    Write(Vec<u8>),
    Drift { forced: Vec<u8> },
}

pub fn resolve_region_edit(
    current: Option<&[u8]>,
    edit: &PlannedRegionEdit,
) -> Result<EditResolution, RegionError>;

pub fn resolve_json_edit(
    current: Option<&[u8]>,
    edit: &PlannedJsonEdit,
) -> Result<EditResolution, JsonEditError>;
```

### Region resolver

The region resolver:

1. Requires the target unless `create: true`.
2. Requires UTF-8 text.
3. Finds one exact begin/end marker pair for the region key.
4. Returns `Unchanged` when the body already equals the desired body.
5. Returns `Write` when the body still matches the recorded checksum and the
   template body changed.
6. Returns `Drift { forced }` when the operator changed the owned bytes.
7. Uses the bootstrap anchor only when the markers do not exist.
8. Splices byte ranges without normalizing bytes outside the region.

Every `Write` and `forced` candidate contains a fresh checksum. Therefore
resolving bytes from a successful previous apply with the same edit returns
`Unchanged`.

### JSON-family resolver

The JSON resolver:

1. Requires the target unless `create: true`; a created target starts as `{}`.
2. Requires UTF-8 and parses through `jsonc_parser::cst` with the options for
   `JsonFormat`.
3. Traverses `JsonPath`, creating missing object-key parents and rejecting an
   invalid scalar or array traversal.
4. Compares the current typed value with `desired`.
5. Returns `Unchanged` when they are equal.
6. Sets or inserts the value through the CST and returns `Write` otherwise.

It never returns `Drift`: the declared path is convergent ownership. A strict
JSON edit emits strict JSON. JSONC and JSON5 edits preserve their existing
comments and syntax. Replacing a value may normalize that owned value's own
lexical spelling; source outside the changed node and required punctuation range
is retained.

## Pure planning and ordered apply

`Plan::build` remains pure over the template, completed answers, and target
listing. It renders region bodies and typed JSON values, parses keys and paths,
chooses marker or JSON format, rejects JSON-family markers, and records edit
intent. It does not read target bytes.

Apply resolves all work before the first write:

```text
recompute whole-file conflicts
check hook trust and every target path/symlink
group whole-file content and edits by target path
for each target in deterministic plan order:
  start with planned whole-file bytes, otherwise read current bytes once
  fold that target's edits over one in-memory byte buffer
  collect one final changed image or one error
if any conflict, unforced region drift, parse/path error, or trust gate: stop
atomically replace each changed target once; report each path once
run hooks in their existing order
```

A whole-file rule supplies the starting image before bounded edits for the same
path. Multiple bounded edits see preceding edits in source order. One target is
written once, even when several rules change it. A later planning failure cannot
leave earlier changes on disk.

Atomic replacement is per target: write a complete sibling temporary file,
preserve the applicable mode, then rename it into place. A file-system failure
during the commit loop can leave earlier targets committed, matching the current
partial-across-files behavior. The design does not claim a multi-file
transaction.

## Ownership and downstream mutation contract

The plan is the single source of truth for owned targets. Project update and the
paired implementation consume this derived view:

```rust
pub enum FileMutation<'a> {
    Whole {
        path: &'a TargetPath,
    },
    Region {
        path: &'a TargetPath,
        region: &'a RegionKey,
    },
    JsonValue {
        path: &'a TargetPath,
        json_path: &'a JsonPath,
    },
}

impl Plan {
    pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>>;
}
```

The ownership rules are:

- `Whole`: Toha owns and replaces the complete file.
- `Region`: Toha owns the marker pair and enclosed byte span. The checksum
  distinguishes a safe replay from operator drift.
- `JsonValue`: Toha owns the typed value at the path. Replay converges that
  value and preserves the rest of the document. There is no drift refusal.

Project update must retain these identities in old-plan/new-plan comparison.
It may reapply a region only when its checksum still describes the owned body,
and it treats a changed owned JSON value as convergent input rather than a
whole-file conflict. Removal of an ownership identity and three-way project
merge behavior remain decisions of the project-update design; this design does
not silently choose them.

## Errors and results

Planning errors:

- invalid or duplicate region key;
- marker style cannot be resolved;
- marker rule targets `.json`, `.jsonc`, or `.json5`;
- invalid typed-path syntax;
- duplicate or ancestor/descendant JSON paths in one target;
- structured rule targets a non-JSON-family extension;
- a rendered JSON-compatible value cannot be represented as JSON.

Apply errors:

- target missing when `create: false`;
- non-UTF-8 target;
- missing or ambiguous anchor;
- malformed or duplicate marker pair;
- region drift without `--force`;
- malformed JSON-family source under the selected format;
- invalid object/array traversal or out-of-range array index;
- target path contains a symbolic link or enters `.git`;
- I/O failure while reading, staging, or replacing a file.

`Applied::Written.files` contains every changed path once. Unchanged paths never
appear. `--force` applies only to region drift; it does not alter JSON value
ownership, make invalid paths valid, repair malformed input, or create a target
unless the rule declares `create: true`.

## Source fidelity of `jsonc-parser`

The selected dependency is `jsonc-parser` with its `cst` and `serde_json`
features. Its CST keeps comments and whitespace as tokens and displays the
original source plus the requested edit
(`jsonc-parser/src/cst/mod.rs:1-5,1356`, revision `c7d4cf5`). The
`serde_json` feature supplies CST-to-`serde_json::Value` conversion for the
owned-value equality check. The crate has no reverse
`From<serde_json::Value>` implementation, so Toha owns this conversion:

```rust
fn json_value_to_cst_input(
    value: &serde_json::Value,
) -> jsonc_parser::cst::CstInputValue
```

The converter recursively maps null, Boolean, number, string, array, and object
values. Numbers use `serde_json::Number::to_string()`, and objects retain the
map's iteration order when building `CstInputValue::Object`
(`jsonc-parser/src/cst/input.rs:1-102`, revision `c7d4cf5`). The same edit
pattern is used by VS Code's `node-jsonc-parser`, which returns minimal
offset/length/content edits.

`jsonc-parser` is an embedded Rust library. The design adds no command,
subprocess, permission, timeout, or runtime version check. Existing `sha2`
remains the region checksum dependency.

## Tradeoffs accepted

- Visible markers in non-JSON text make ownership inspectable and enable
  checksum drift refusal.
- JSON-family path ownership has no visible marker and converges an edited owned
  value. This permits strict JSON while retaining all unrelated source text.
- Two resolvers add implementation cost, but give each format family the
  stronger ownership mechanism.
- Marker anchors are bootstrap-only. Later selection uses the marker key.
- Structured paths create missing object parents but never extend arrays, which
  avoids inventing values to reach an index.
- Injection uses atomic replacement because partial writes to operator-owned
  files are not acceptable.

## Alternatives considered

- **`jaq` or another parse/transform/print engine:** rejected. It regenerates
  the document and loses comments or formatting.
- **Markers in JSONC and JSON5, refusal for strict JSON:** rejected as the
  JSON-family product boundary. It would give the same feature family two
  ownership models based only on extension.
- **Whole-file JSON generation only:** retained as an existing option, but it
  cannot provide bounded ownership of a value in an operator-owned document.
- **Anchor plus presence guard:** rejected because repeats can duplicate after
  nearby edits and no stable owned boundary exists.
- **A `Content::Injected` variant:** rejected because it hides a target-read
  requirement behind a type whose current variants already carry full bytes.
- **A separate injection command:** rejected because it creates a second apply
  pass and breaks one-plan ordering.

## Canonical artifact impact

The paired implementation updates these authoritative documents in the same
change as runtime behavior:

- `docs/specifications/template-format.yml` and its schema: `inject`, its
  `region` and `struct` discriminators, typed JSON values, path grammar,
  format boundary, ownership, idempotency, and create behavior.
- `docs/specifications/command-line-interface.yml`: dry-run vocabulary,
  region-drift output, and the bounded meaning of `--force`.
- `docs/specifications/interview-protocol.yml`: only if the written-path result
  shape needs clarification; no new interview state or UI belongs here.
- `docs/technical-designs/architecture.yml`: `PlannedEdit`, pure resolvers,
  grouped resolve-before-write, and the derived `FileMutation` view.
- User guide and generic examples: one visible-region example and one
  JSON-family structured example.

## Behaviors to prove

The paired implementation must prove these observable cases:

1. A visible region applied twice changes its target only once.
2. First placement at an anchor and at end-of-file uses the correct offset.
3. A template-body change preserves every byte outside the marked span.
4. Region drift refuses without `--force`; forced apply replaces only the
   region; force never fabricates placement.
5. Missing anchor exits as an error and `occurrence: only` rejects ambiguity.
6. Comment inference works on generic text targets and an unknown extension
   without an override fails loudly.
7. A marker rule for `.json`, `.jsonc`, or `.json5` is rejected with `struct:`
   and whole-file alternatives.
8. Strict JSON insert and replacement are valid, typed, and byte-no-op on the
   second apply.
9. JSONC and JSON5 inserts and replacements preserve comments, key order,
   whitespace, trailing commas, untouched values, and surrounding syntax.
10. String, boolean, number, null, array, and object desired values retain their
    JSON type.
11. A changed owned JSON value converges without `--force`; no unrelated source
    text changes.
12. Missing object parents are created; scalar traversal, missing/out-of-range
    arrays, malformed input, duplicate paths, and overlapping paths fail before
    any write.
13. Whole-file plus bounded edits on one target use the whole-file result as the
    starting image and commit one final replacement.
14. Multiple non-overlapping JSON paths in one target commit once and report the
    path once.
15. An error in the final planned edit leaves every target unchanged.
16. Staged replay, headless apply, direct apply, terminal apply, and crate apply
    produce the same mutation plan and result.
17. Existing whole-file JSON generation still works.

## Implementation order

Implement the typed plan shapes and both pure resolvers first. Then add grouped
resolve-before-write orchestration and atomic replacement. Add template/schema
parsing, CLI reporting, canonical documentation, and the end-to-end fixtures
against the public `Plan::build().apply()` path. The project-update design
consumes the final `FileMutation::{Whole, Region, JsonValue}` contract.
