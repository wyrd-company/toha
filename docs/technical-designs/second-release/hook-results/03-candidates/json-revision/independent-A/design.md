# Candidate A — `parse: json`: the parsed value is `<id>`

This is a delta on the v1 design (`../../../design.md`). Everything v1 says
still holds for a hook that does not declare `parse`. This candidate adds two
optional producer fields, `parse: json` and `status-id`.

**The reconciliation in one line.** With `parse: json`, `<id>` *is* the parsed
value and nothing else. Execution metadata does not share that name. When a
reader needs metadata, the author names a second, separate result with
`status-id`. That result has the v1 metadata shape and reuses the v1 rules. The
two names cannot collide, because the parsed value has no reserved keys and the
metadata object contains no JSON.

## 1. Caller usage

### 1.1 The common case: read a JSON field

```yaml
hooks:
  - id: pkg
    run: [ pkg-tool, info, --json ]      # prints {"name":"demo","version":"1.4.0","tags":["a","b"]}
    parse: json                          # pkg is the parsed stdout; implies stdout capture
  - run: [ note-tool, "built {{ pkg.name }} {{ pkg.version }}" ]
  - run: [ tag-tool, "{{ pkg.tags[0] }}" ]
    when: "pkg.tags | length > 0"
messages:
  after-apply: "Created {{ pkg.name }} at {{ pkg.version }}."
```

There is no `capture` line. `parse: json` captures stdout by itself. `pkg` is
unconditional and in the default stop mode, so every reader sees a parsed value.
The trust review sees `parse: json` in the hook node.

### 1.2 Every JSON value type

```yaml
hooks:
  - id: meta                             # stdout: {"version":"2.0","features":{"x":true}}
    run: [ meta-tool ]
    parse: json
  - id: targets                          # stdout: ["linux","macos"]
    run: [ list-targets, --json ]
    parse: json
  - id: count                            # stdout: 3
    run: [ count-tool ]
    parse: json
  - id: channel                          # stdout: "stable"   (a JSON string, quoted)
    run: [ channel-tool ]
    parse: json
  - id: enabled                          # stdout: true
    run: [ flag-tool ]
    parse: json
  - id: parent                           # stdout: null
    run: [ parent-tool ]
    parse: json
  - run: [ report-tool,
           "{{ meta.version }}",         # object → key
           "{{ meta.features.x }}",      # nested key
           "{{ targets[0] }}",           # array → index
           "{{ targets | join(',') }}",  # array → filter
           "{{ count + 1 }}",            # number → arithmetic
           "{{ channel | upper }}",      # string → string filters
           "{{ parent is none }}" ]      # null → none
    when: "enabled"                      # bool → truthiness
```

### 1.3 A conditional producer; the reader guards on "defined"

```yaml
interview:
  - id: use_registry
    type: confirm
    prompt: Register the package?
  - hook:
      id: reg
      run: [ registry-tool, add, --json ]
      parse: json
    when: use_registry
hooks:
  - run: [ note-tool, "registered as {{ reg.handle }}" ]
    when: "reg is defined"               # undefined ⇔ reg did not run
```

A JSON-mode `<id>` is **undefined** when its producer did not run. A producer
that ran and printed `null` gives a **defined** `none`. `is defined` is the
guard. It keeps "did not run" apart from every JSON value.

### 1.4 Metadata when it matters: a tolerated failure

```yaml
hooks:
  - id: findings
    run: [ lint-tool, --format, json ]   # exit 1 with a JSON findings list when it finds issues
    parse: json
    status-id: lint                      # lint = {exit_code, stderr}: the v1 shape without stdout
    capture: [ stderr ]                  # only because lint.stderr is read below
    allow-failure: true                  # needs status-id, and lint.exit_code must be read
  - run: [ lint-tool, --fix, . ]
    when: "lint.exit_code != 0"
messages:
  after-apply: >-
    {% if findings is defined %}{{ findings | length }} findings.{% endif %}
    {% if lint.exit_code != 0 %}Lint said: {{ lint.stderr }}{% endif %}
```

`findings` stays "the value". `lint` holds the metadata and follows v1 exactly:
it is always bound, and `lint.exit_code is none` ⇔ did not run. A tolerated
failure that printed no valid JSON leaves `findings` undefined, and
`lint.exit_code` tells the reader why.

### 1.5 Crate / driver call site: unchanged from v1

No call site gains an argument. Parsing is private to `Plan::apply_reporting`.

## 2. Types delta (on v1 §2)

```rust
// template.rs
pub struct HookNode {
    /* v1: command, each, when, id, capture, allow_failure */
    pub parse: Parse,                  // default Text = v1 behavior
    pub status_id: Option<Id>,         // only with Parse::Json; joins the authored-id space
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Parse { #[default] Text, Json }
// Capture is unchanged. Under Parse::Json the loader sets Capture.stdout = true;
// an author-written `capture: [stdout]` together with `parse: json` is a load error.

// plan.rs
pub struct PlannedHook {
    /* v1 fields */
    pub parse: Parse,
    pub status_id: Option<Id>,
}   // Debug: new fields print only when non-default (as v1)

// hook.rs: HookOutcome and HookRunner unchanged from v1.
// The result table now binds names to plain JSON values, which is the same
// mechanism as every other context value (jinja::context_from_answers).
#[derive(Default)]
pub(crate) struct HookResults(IndexMap<Id, serde_json::Value>);   // on apply's stack only
impl HookResults {
    // v1 (Parse::Text): id → {exit_code, stdout, stderr}; NOT_RUN pre-seeded.
    // Parse::Json: id → parsed value, inserted only when defined;
    //              status_id → {exit_code, stderr}; NOT_RUN pre-seeded.
    pub(crate) fn record(&mut self, hook: &PlannedHook, outcome: &HookOutcome,
                         parsed: Option<serde_json::Value>);
}

pub(crate) fn parse_json(text: &str) -> Result<serde_json::Value, JsonFault>; // serde_json::from_str

// apply.rs: v1 HookOutput widens from one fault to three; no bytes in any.
pub enum ApplyError { /* v1… */
    HookOutput { index: usize, id: Id, fault: OutputFault },
}
pub enum OutputFault {
    NotUtf8 { valid_up_to: usize },           // v1
    Empty,                                    // parse: json, stdout was only whitespace
    NotJson { line: usize, column: usize },   // parse: json, serde_json syntax/EOF position
}
```

`HookResult` (v1) remains the Text-mode shape. The status object is the same
struct serialized with `stdout` omitted.

## 3. Seams delta

```rust
// template.rs: settle_hook_results (v1) gains these rules:
//  - parse / status-id only with id
//  - status-id only with parse: json; status-id ≠ id; status-id is subject to
//    the existing invalid-identifier / duplicate / 1075 collision checks
//  - under parse: json, a read of `<id>`, `<id>.<anything>` or `<id>[…]` counts
//    as reading stdout. No field check applies to <id>, because its shape is known only at run time.
//  - `<status-id>.<field>` must be exit_code or stderr
//  - allow-failure with parse: json requires status-id, and <status-id>.exit_code must be read
//  - v1 capture-equals-reads applies to stderr. stdout is implied by parse: json
```

Apply loop delta (v1 §3). Only the lines marked `+` are new:

```text
outcome = runner.run(hook)?
stdout/stderr = decode(...)?                        // NotUtf8: fatal, even when the failure is tolerated
+ parsed = match (hook.parse, outcome):
+   (Text, _)                  → n/a
+   (Json, success)            → Some(parse_json(stdout)?)          // Empty / NotJson: fatal
+   (Json, tolerated nonzero)  → parse_json(stdout).ok()            // never fatal; None → undefined
+   (Json, untolerated / signal) → not parsed; the v1 ApplyError::Hook follows
if hook.id: results.record(hook, outcome, parsed)
if !outcome.success && !(hook.allow_failure && outcome.code.is_some()):
    return Err(ApplyError::Hook{…})                 // v1, unchanged
```

When a deferred surface renders, `HookResults` is merged over the plan-time
context, as in v1. A JSON-mode `<id>` that is not in the table is absent from
the context. Jinja therefore sees it as undefined.

## 4. Error / results contract for `parse: json`

### Invariants (added to v1 §5)

1. **The value is `<id>`.** With `parse: json`, `<id>` is the parsed stdout as a
   plain JSON value. It is not an object that wraps the value. No key is
   reserved, so `<id>.exit_code` is simply the JSON key `exit_code` if the
   output has one.
2. **Metadata is opt-in and separately named.** `status-id: <name>` binds
   `<name> = {exit_code, stderr}` with v1 semantics. `<name>` is always bound,
   and `exit_code is none` ⇔ did not run. Raw stdout is not exposed in JSON
   mode. `{{ <id> | tojson }}` gives a canonical re-serialization.
3. **"Did not run" is undefined, never a JSON value.** `<id> is defined` is
   false exactly when the producer did not run, or when a tolerated failure
   printed no valid JSON. `status-id` tells these two cases apart. JSON `null`
   is a defined `none`.
4. **Success means valid JSON.** On exit 0, stdout must be one JSON document.
   An empty or malformed document stops the apply.
5. **Tolerated failure never fails on content.** On a tolerated nonzero exit,
   stdout is parsed if it is valid JSON. If not, `<id>` is undefined.
6. **v1 carries over.** Readable surfaces, static availability, the stop-and-fail
   default, signal-is-fatal, strict UTF-8, per-apply lifetime, and no leakage
   are all as in v1. A hook without `parse` is exactly v1.

### JSON value mapping (serde_json → minijinja, the existing context path)

| stdout | `<id>` | Typical read |
| --- | --- | --- |
| object | map | `<id>.key`, `<id>["a-key"]`, `<id>.a.b`, `<id> \| items` |
| array | sequence | `<id>[0]`, `<id> \| length`, `{% for x in <id> %}` |
| string | string (unquoted) | `{{ <id> }}`, `<id> \| upper` |
| number | integer (i64/u64) or float (f64) | `<id> + 1`, `<id> > 2` |
| true/false | bool | `when: "<id>"` |
| null | none (defined) | `<id> is none` |

Surrounding whitespace, including a trailing newline, is allowed. Object keys
iterate in sorted order (serde_json's default map). For a duplicate key, the
last one wins (serde_json's behavior). A missing key under the default lenient
undefined behavior is undefined: it renders empty and is false in `when`. An
attribute read *through* an undefined `<id>` (unrun producer, no guard) is a
render error at that step.

### Load errors (added)

| Condition | Message |
| --- | --- |
| `parse` without `id` | `parse requires id` |
| `parse` value other than `json` | existing schema enum error |
| `status-id` without `parse: json` | ``status-id requires parse: json; without it, `<id>` already carries exit_code and stderr`` |
| `status-id` equal to `id`, or duplicate / reserved / invalid | existing `duplicate id` / 1075 collision / `invalid identifier` |
| `capture: [stdout]` with `parse: json` | ``parse: json already captures stdout; remove stdout from capture on hook `<id>` `` |
| `allow-failure: true` with `parse: json` and no `status-id` | ``allow-failure on hook `<id>` needs a status-id; add `status-id: <name>` and read `<name>.exit_code` `` |
| `<status-id>.stdout` | ``hook `<id>` parses stdout as JSON; read `<id>` or `<id> \| tojson` `` |
| `<status-id>.<other>` | existing `unknown hook result field: <name>.<field>` |
| `<status-id>.stderr` read, stderr not captured | v1 `does not capture stderr` message |
| JSON-mode `<id>` or `status-id` never read | v1 `never read` message (for `parse: json`, the stream named is stdout) |

### Apply outcomes (JSON mode)

| Situation | `<id>` | `<status-id>` | Apply |
| --- | --- | --- | --- |
| exit 0, valid JSON | parsed value | `{exit_code: 0, stderr}` | continue |
| exit 0, stdout empty / whitespace | — | — | `ApplyError::HookOutput{fault: Empty}`: ``hook `<id>` printed no JSON on stdout (parse: json)`` |
| exit 0, malformed / several documents / BOM | — | — | `HookOutput{fault: NotJson{line,column}}`: ``hook `<id>` stdout is not valid JSON at line L, column C`` |
| any exit, non-UTF-8 stdout | — | — | v1 `HookOutput{fault: NotUtf8}` |
| nonzero, tolerated, valid JSON | parsed value | `{exit_code: n, …}` | continue |
| nonzero, tolerated, empty / malformed | undefined | `{exit_code: n, …}` | continue |
| nonzero, not tolerated | not recorded | not recorded | v1 `ApplyError::Hook`; nothing parsed |
| signal | — | — | v1 `ApplyError::Hook`, even with allow-failure |
| deferred `when` false / not reached | undefined | `{exit_code: none, stderr: none}` | continue |

No error message contains stdout text. `NotJson` carries only serde_json's
line and column. The serde_json message is not forwarded.

### Dry-run, trust, replay

- **Dry-run** runs nothing and parses nothing. A deferred argument shows the
  source form with placeholders, for example `<pkg.version>`, `<pkg[0]>`, or
  `<pkg>`. `parse: json` appears in the hook listing next to `id`.
- **Trust / digest.** `parse` and `status-id` are parsed hook-node fields, so
  the 1069 parsed-node digest covers them automatically. Neither field names an
  executable. The guard test classifies them as `NoExecutableReference`. No
  program is derived from output, because `run[0]`/`script` still cannot read a
  result.
- **Staged / replay / stop / abort:** unchanged from v1. Parsed values live for
  one apply only.
- **Dependencies:** none new. `serde_json = "1"` is already a crate dependency
  (`Cargo.toml`), and the Jinja context is already `serde_json::Value`.

## 5. Canonical impact (described; owned by impl 1063)

- **Schema:** add `parse` (enum `[json]`) and `status-id` (identifier) to the
  top-level and interview hook objects. Both require `id`. `status-id`
  requires `parse`.
- **Spec:** add a "JSON results" paragraph that states invariants 1–5 and the
  value mapping table.
- **Guide:** add §1.1, §1.3, and §1.4 as "Read a hook's JSON output".
- **CLI:** the dry-run listing shows `parse: json`. The `HookOutput` messages
  above are added.

## 6. Decisions for Bob

1. The metadata channel is an opt-in `status-id` (recommended), not a fixed
   companion name and not a merged object.
2. "Did not run" is undefined in JSON mode (recommended), not `none`.
3. Tolerated-failure parse is lenient (recommended), not strict.
