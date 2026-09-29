# Design candidate B — JSON hook results at `<id>`, metadata at a declared name

Delta on `../../../design.md` (v1). Everything in v1 holds unless this document
changes it. Lens: full, unambiguous access to both the parsed value and the
execution metadata, with no possible collision between JSON keys and metadata.

## Summary

- `parse: json` on a producer binds the **parsed stdout** at `<id>`. `<id>` is the
  JSON value itself: an object, an array, or a scalar.
- The execution metadata is **never** merged into `<id>`. If the author needs it,
  they declare a second identifier, `metadata: <name>`. That name is bound to
  `{exit_code, stdout, stderr, parsed}`.
- Two names, two values, so there is no collision: `<id>.exit_code` is always the
  JSON key `exit_code`, and `<name>.exit_code` is always the exit code.
- `metadata` is **required** when the producer can finish in any state other than
  "ran, exit 0, parsed". That means the producer has a `when` or it has
  `allow-failure: true`. In those states `<id>` is `none`. Without metadata the
  reader could not tell "did not run" or "could not parse" from JSON `null`.
- A hook without `parse` behaves exactly as v1. A hook without `id` behaves
  exactly as it does today.

## 1. Caller usage

### 1.1 Object: read keys directly

```yaml
hooks:
  - id: toolchain
    run: [ toolchain-info, --format, json ]
    capture: [ stdout ]            # stdout is piped to Toha, as in v1
    parse: json                    # <id> is the parsed stdout
  - run: [ setup-tool, --channel, "{{ toolchain.channel }}" ]
    when: "toolchain.features.formatter"
messages:
  after-apply: "Toolchain {{ toolchain.version }} is ready."
```

stdout is `{"version":"1.4.2","channel":"stable","features":{"formatter":true}}`.
`toolchain.version` renders `1.4.2`. `toolchain` is unconditional and in the
default stop mode, so at every reader it ran, exited 0, and parsed. No metadata
is needed or allowed to be unread.

### 1.2 Array: index and iterate inside a Jinja expression

```yaml
hooks:
  - id: packages
    run: [ list-packages, --json ]
    capture: [ stdout ]
    parse: json
  - run: [ build-tool, --package, "{{ packages[0].name }}" ]
    when: "packages | length > 0"
messages:
  after-apply: "Built {{ packages | map(attribute='name') | join(', ') }}."
```

`each` still cannot read a result (v1: `each` is eager). Iteration is available
only inside one rendered field, as above.

### 1.3 Scalar: string, number, boolean, null

```yaml
hooks:
  - id: widget_count
    run: [ count-widgets, --json ]  # prints: 42
    capture: [ stdout ]
    parse: json
  - run: [ shard-tool, --shards, "{{ (widget_count / 10) | round | int }}" ]
    when: "widget_count > 10"
```

`widget_count` is the number `42`. It is not `{"value": 42}` and it is not the
text `"42"`.

### 1.4 Tolerated failure: JSON report plus exit code

```yaml
hooks:
  - id: report
    run: [ lint-tool, --format, json, . ]
    capture: [ stdout ]
    parse: json
    allow-failure: true            # requires `metadata` in JSON mode
    metadata: report_run           # {exit_code, stdout, stderr, parsed}
  - run: [ lint-tool, --fix, . ]
    when: "report_run.exit_code != 0 and report_run.parsed and report.fixable > 0"
messages:
  after-apply: >-
    {% if report_run.exit_code == 0 %}Lint is clean.
    {% elif report_run.parsed %}Lint found {{ report.problems | length }} problems.
    {% else %}Lint failed with exit code {{ report_run.exit_code }}.{% endif %}
```

On a tolerated nonzero exit Toha still tries to parse stdout, because many tools
print a JSON report and exit nonzero. If the parse succeeds, `report` is the
report and `report_run.parsed` is `true`. If stdout is empty or not JSON,
`report` is `none`, `report_run.parsed` is `false`, and the apply continues.
The raw text is in `report_run.stdout`.

### 1.5 Conditional producer: guard on metadata, not on `<id>`

```yaml
interview:
  - id: use_registry
    type: confirm
    prompt: Register the project?
  - hook:
      id: registration
      run: [ registry-tool, register, --json ]
      capture: [ stdout ]
      parse: json
      metadata: registration_run   # required: this producer has a `when`
    when: use_registry
hooks:
  - run: [ note-tool, "registered as {{ registration.handle }}" ]
    when: "registration_run.exit_code is not none"
```

`registration is none` is **not** a "did not run" test, because the JSON could
be `null`. `registration_run.exit_code is none` is the "did not run" test, as in
v1.

### 1.6 A JSON key with a metadata name

```yaml
  - run: [ echo-tool, "{{ job['exit_code'] }}" ]   # the JSON key "exit_code" of `job`
```

In JSON mode, `<id>.exit_code`, `<id>.stdout`, and `<id>.stderr` in attribute
form are load errors. They are the likely mistake when someone adds `parse: json`
to an existing v1 hook. Subscript form `<id>['exit_code']` reads the JSON key
without ambiguity. See §5 load errors and the rationale.

### 1.7 Crate / driver call site — unchanged

`Template::load`, `Plan::build`, and `Plan::apply` keep their v1 signatures. No
call site gains an argument.

## 2. Type delta

```rust
// template.rs — the authored node gains two literal fields.
pub struct HookNode {
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    pub id: Option<Id>,
    pub capture: Capture,
    pub allow_failure: bool,
    pub parse: Parse,              // NEW; Parse::Text = v1 exactly
    pub metadata: Option<Id>,      // NEW; Some only with Parse::Json
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Parse { #[default] Text, Json }

// plan.rs — PlannedHook carries the same two fields; Debug prints them only
// when non-default (v1 rule for new fields).
pub struct PlannedHook { /* v1 fields… */ pub parse: Parse, pub metadata: Option<Id> }

// hook.rs — HookOutcome is unchanged from v1 (code + opt-in streams).
// HookResult (v1 metadata object) is unchanged. JSON mode adds one entry kind.
#[derive(Clone, PartialEq, serde::Serialize)]
pub(crate) struct JsonMetadata {
    pub exit_code: Option<i32>,    // none ⇔ did not run
    pub stdout: Option<String>,    // decoded text as in v1 (trailing \r\n stripped)
    pub stderr: Option<String>,    // Some iff capture includes stderr and it ran
    pub parsed: bool,              // true ⇔ <id> holds the parsed stdout
}
#[derive(Clone, PartialEq)]
pub(crate) enum ResultEntry {
    Text(HookResult),                                  // v1: <id> → {exit_code, stdout, stderr}
    Json { value: serde_json::Value,                   // <id> → value (Null when !parsed)
           metadata: Option<(Id, JsonMetadata)> },     // <name> → JsonMetadata
}
pub(crate) struct HookResults(IndexMap<Id, ResultEntry>);   // per apply, as v1

impl HookResults {
    /// Insert every bound name into a render context. Text: one name.
    /// Json: `<id>` and, if declared, `<metadata>`. Not-run entries are bound too.
    pub(crate) fn bind(&self, ctx: &mut BTreeMap<String, serde_json::Value>);
}
```

`serde_json::Value` is the same type that `jinja::context_from_answers` already
puts in the context (`src/jinja.rs`), so minijinja renders the parsed value with
no new code path: an object gives `<id>.key` and `<id>['key']`, an array gives
`<id>[i]`, and a scalar is the value itself.

**No new dependency.** Parsing uses `serde_json::from_str`. `serde_json = "1"` is
already in `Cargo.toml` and in use.

## 3. Seams

```rust
// template.rs — settle_hook_results (v1) gains these checks, still inside load.
//   - parse: json requires id and capture ⊇ [stdout]
//   - metadata requires parse: json; the name joins the authored-id space
//     (existing invalid-identifier / duplicate / 1075 reserved-name errors)
//   - metadata required ⇔ producer has `when` OR allow-failure: true
//   - a declared metadata name must be read
//   - allow-failure in JSON mode is "read" only if <metadata>.exit_code is read
//   - <id>.exit_code / .stdout / .stderr (attribute form) on a JSON producer is a load error
//   - <id>.<anything else> on a JSON producer is allowed (runtime shape);
//     v1 "unknown hook result field" still applies to text producers and to
//     <metadata>.<field> (fields: exit_code, stdout, stderr, parsed)

// hook.rs — one step after v1 decode.
pub(crate) enum JsonFault { Empty, Malformed { line: usize, column: usize } }
pub(crate) fn parse_json(text: &str) -> Result<serde_json::Value, JsonFault>;
//   empty or whitespace-only text → Empty; serde_json error → Malformed{line,column}.
//   The serde_json message text is not kept (no stdout bytes in errors).

// apply.rs — one new variant.
pub enum ApplyError { /* v1… */
    HookJson { index: usize, id: Id, fault: JsonFault },
}
```

Apply loop delta (only the record step changes):

```text
outcome = runner.run(hook)?
stdout/stderr = decode(...)?                        // v1: non-UTF-8 fatal, always
if hook.parse == Json:
    tolerated = !outcome.success && hook.allow_failure && outcome.code.is_some()
    match parse_json(stdout):
        Ok(v)                 → value = v, parsed = true
        Err(f) if tolerated   → value = Null, parsed = false        // continue
        Err(f)                → return Err(ApplyError::HookJson{index, id, fault: f})
    results.record(id, Json{ value, metadata: (name, {code, stdout, stderr, parsed}) })
else:
    v1 record
v1 failure check (nonzero without allow-failure / signal → ApplyError::Hook)
```

Order matters: on a nonzero exit **without** `allow-failure`, the v1
`ApplyError::Hook` is reported, not a JSON fault. The parse result is not needed
because nothing later runs. Implementation checks failure first for that case.

## 4. Error / results contract (JSON mode)

### Invariants

1. **Two names, no merge.** `<id>` is the parsed JSON value. Metadata exists only
   at the declared `metadata` name. Toha never adds a key to, or wraps, the
   parsed value.
2. **Every JSON type is bound as-is.**

   | stdout | `<id>` | Jinja access |
   | --- | --- | --- |
   | object | map | `<id>.key`, `<id>['key-with-dash']`, `<id> \| items` |
   | array | sequence | `<id>[0]`, `<id> \| length`, `for x in <id>` |
   | string | string | `{{ <id> }}` renders the string, unquoted |
   | number | integer or float | `<id> > 10`; serde_json default: i64/u64 exact, otherwise f64 |
   | `true`/`false` | boolean | `when: "<id>"` |
   | `null` | `none` | `<id> is none` — means "JSON null" **only** when `<metadata>.parsed` is true or the producer needs no metadata |

   Object key order follows serde_json's default map (sorted). A duplicate key
   keeps the last value. A leading byte-order mark is malformed JSON.
3. **`<id>` is never undefined.** It is `none` when the producer did not run or
   (tolerated failure only) did not parse. v1 invariant 3 holds.
4. **Unambiguous states.** Each producer state has exactly one representation:

   | Producer state | `<id>` | `<metadata>` |
   | --- | --- | --- |
   | did not run (`when` false, or never reached) | `none` | `{exit_code: none, stdout: none, stderr: none, parsed: false}` |
   | exit 0, valid JSON | value | `{exit_code: 0, stdout: "<text>", stderr, parsed: true}` |
   | exit 0, empty or malformed | — | — apply fails, `ApplyError::HookJson` |
   | nonzero, tolerated, valid JSON | value | `{exit_code: n, stdout, stderr, parsed: true}` |
   | nonzero, tolerated, empty or malformed | `none` | `{exit_code: n, stdout, stderr, parsed: false}` |
   | nonzero, not tolerated | — | — apply fails, `ApplyError::Hook` (v1) |
   | signal | — | — apply fails, `ApplyError::Hook` (v1, even with allow-failure) |
   | non-UTF-8 stdout or stderr | — | — apply fails, `ApplyError::HookOutput` (v1) |

   The states where `<id>` is `none` for a reason other than JSON `null` are
   possible only when metadata is required. So a producer without metadata
   always has `<id>` = the parsed value at every reader.
5. **Capture stays the single list of piped streams.** `parse: json` does not
   pipe anything by itself; it requires `stdout` in `capture`. Reading `<id>`
   counts as reading stdout for the v1 "captured stream must be read" rule.
   `<metadata>.stdout` gives the raw decoded text; `<metadata>.stderr` requires
   `stderr` in `capture`, as in v1.
6. **Readable surfaces, trust, persistence, and leakage are v1.** Both names are
   readable only on the v1 surfaces (later top-level hook `when`/`run[1..]`/
   `args`/`cwd`, and `messages.after-apply`). Results last one apply. No
   `Debug`/`Display` holds stdout, stderr, or parsed content.
7. **A parsed string is data, not a template.** Rendering `{{ <id> }}` inserts
   the text. Jinja syntax inside JSON strings is not evaluated. Values reach a
   later hook as argv elements, as in v1 (no shell).

### Load errors (added to the v1 table)

| Condition | Message |
| --- | --- |
| `parse` without `id` | `parse requires id` |
| `parse: json` without `stdout` in `capture` | ``hook `<id>` parses stdout as JSON; add `capture: [stdout]` `` |
| `parse` value other than `json` | schema error: `parse must be json` |
| `metadata` without `parse: json` | ``metadata requires parse: json; read `<id>.exit_code` directly`` |
| `metadata` name invalid / duplicate / reserved | existing `invalid identifier` / `duplicate id: <name>` / 1075 collision message |
| `parse: json` with `when` or `allow-failure`, no `metadata` | ``hook `<id>` may finish without a parsed value; add `metadata: <name>` and read `<name>.exit_code` `` |
| `metadata` declared, never read | ``metadata `<name>` of hook `<id>` is never read`` |
| `allow-failure: true` in JSON mode, `<name>.exit_code` never read | ``the exit code of `<id>` is never read; read `<name>.exit_code` in a later hook or in messages.after-apply`` |
| `<id>.exit_code` / `.stdout` / `.stderr` on a JSON producer | ``hook `<id>` parses stdout as JSON; its exit code and streams are at `metadata`; use `<id>['<field>']` for a JSON key`` |
| unknown metadata field | `unknown hook result field: <name>.<field>` (fields: `exit_code`, `stdout`, `stderr`, `parsed`) |
| `<id>` or `<name>` read on a disallowed surface, or before the producer | the v1 messages, naming whichever name was read |

### Apply outcomes (added to the v1 table)

| Situation | Result |
| --- | --- |
| exit 0, stdout empty or whitespace-only | ``ApplyError::HookJson{fault: Empty}`` — ``hook `<id>` (hooks[i]) printed no JSON; print `null` for no value`` |
| exit 0, malformed JSON (includes serde_json's nesting limit) | ``ApplyError::HookJson{fault: Malformed}`` — ``hook `<id>` (hooks[i]) printed invalid JSON at line L, column C`` |
| tolerated nonzero, parse fails | continue; `<id>` = `none`, `<name>.parsed` = `false` |
| deferred render reads a missing key, `<id>.a` | minijinja lenient undefined, as for any context value (renders empty; `is defined` works) |
| deferred render reads through a missing key, `<id>.a.b` | `ApplyError::Deferred(PlanError::Render)` (v1) |

There is no size cap on stdout and no new timeout. Both are as in v1.

### Dry-run, trust, replay

- **Dry-run** runs nothing and parses nothing. A deferred field shows the v1
  placeholder form with the path as written, for example `<toolchain.channel>`,
  `<packages[0].name>`, `<report_run.exit_code>`. The trust listing shows
  `parse: json` and `metadata` in source form with the hook.
- **Static analysis** is unchanged in kind. The names `<id>`, `<id>.<path>`, and
  `<name>.<field>` are static references that the 1075 retained program already
  sees. The shape of `<id>` is a runtime fact. The one new static check (the
  `<id>.exit_code` attribute-form error) needs the analysis to tell attribute access from constant
  subscript access. minijinja's AST keeps `GetAttr` and `GetItem` apart; impl
  1063 confirms that the 1075 analysis keeps the distinction.
- **Trust.** `parse` and `metadata` are parsed hook-node fields, so the 1069
  parsed-node digest covers them automatically. Changing `parse` or `metadata`
  changes the digest and re-prompts trust. Neither field names an executable.
  Parsing derives no program from output, and `run[0]`/`script` still cannot read
  a result. The 1063 guard test classifies both as `NoExecutableReference`.
- **Stop/abort, staging, replay:** unchanged from v1. Parsed values are never
  persisted.

## 5. Compatibility and canonical impact (described, owned by impl 1063)

- **Existing templates and v1 hooks:** a hook without `parse` is exactly v1. A
  hook without `id` is exactly today.
- **Crate API (pre-v1):** `HookNode`/`PlannedHook`/`RenderedHook` gain `parse`
  and `metadata`. `ApplyError` gains `HookJson`. `Parse` is a new public enum.
- **Schema (`template-format.schema.yml`):** add `parse` (enum `[json]`) and
  `metadata` (identifier) to the top-level and interview hook objects. `parse`
  requires `id`; `metadata` requires `parse`. The conditional-metadata rule and
  `capture ⊇ [stdout]` are load checks, because they depend on `when`,
  `allow-failure`, and reads.
- **Spec (`template-format.yml`):** extend "Hook results" with invariants 1–5 of
  §4 and the state table.
- **Guide (`docs/template-hooks.md`):** add "Read a hook's JSON output" from
  examples 1.1–1.5.
- **CLI (`command-line-interface.yml`):** dry-run placeholder and trust-listing
  text as above. Toha prints no parsed content.
- **`verification.md`:** add falsifiable checks: one fixture per JSON type; the
  eight-row state table; the `<id>.exit_code` load error; digest change on
  `parse`/`metadata`; no stdout bytes in `HookJson` Display.
- **Interview protocol:** no change.

## 6. Decisions for Bob

1. **Metadata at a declared name** (recommended) vs a fixed derived name vs a
   merged object. See rationale.
2. **Metadata required for conditional or tolerated JSON producers**
   (recommended), so that `none` at `<id>` never has two meanings without a
   disambiguator.
3. **Tolerated nonzero: parse is tried; failure is not fatal** (recommended) vs
   never parse on nonzero vs always fatal on bad JSON.
4. **Empty stdout on exit 0 is fatal** (recommended) vs empty → `none`.
5. **`<id>.exit_code|stdout|stderr` attribute form on a JSON producer is a load
   error** (recommended; subscript form is the escape).
