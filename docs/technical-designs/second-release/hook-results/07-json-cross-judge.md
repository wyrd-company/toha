
# Cross-judge report: parse stdout as JSON into `<id>`

## 1. Scores

Weights are 3/3/3/2/2/1, so the maximum is 70.

| Criterion (weight) | A | B | Reason |
| --- | ---: | ---: | --- |
| 1 Composition fidelity (3) | 5 | 5 | Both add fields that are hashed into the 1069 parsed-node digest. Neither adds a reserved name. The companion name joins the authored-id space and uses the existing collision checks. Replay, staging and the surfaces are unchanged. Neither breaks a load-bearing predecessor sentence. |
| 2 Phase + availability (3) | 4 | 5 | Both keep the v1 ordering. A lets a statically available name be **undefined** at run time, which breaks v1 invariant 3. An unguarded read through it becomes a render error after files are written. B keeps "always bound". |
| 3 Opt-in + trust (3) | 5 | 5 | Both are opt-in. Stop-and-fail stays the default. `allow-failure` must be read in both. No new trust, timeout or subprocess policy. `serde_json` is disclosed as already present. |
| 4 Failure/edge completeness (2) | 4 | 5 | Both cover empty, malformed, BOM, multiple documents, non-UTF-8, signal, dry-run and not-run. A cannot tell the reader *why* a tolerated failure did not parse, because raw stdout is dropped. B has a complete 8-row state table. |
| 5 Interface depth (2) | 4 | 4 | A is leaner: `HookResults(IndexMap<Id, Value>)`, and one `HookOutput` widened with an `OutputFault`. B adds a `ResultEntry` enum and a second output-fault variant (`HookJson`) next to `HookOutput`. B's `bind()` is a good single seam. |
| 6 Compat + canonical (1) | 5 | 5 | Both describe the schema, spec, guide and CLI changes and apply none of them. B also lists `verification.md` checks. |
| **Weighted total** | **63** | **68** | |

## 2. Red-flag screen

| Flag | A | B |
| --- | --- | --- |
| Shallow module / pass-through | none | none |
| Information leakage | none | Minor: the rule "which failures are fatal" appears in both the pseudo-code and a prose note, and the two disagree (see below). |
| Temporal decomposition | none | none |
| Hypothetical seam | none (`status-id` has real variation) | none |
| Two sources of truth | none (raw stdout deliberately dropped) | `<metadata>.stdout` and `<id>` hold the same stream. It is useful when `parsed=false` and redundant otherwise. |

Neither candidate violates a load-bearing contract. Defects and gaps:

- **B's pseudo-code has the wrong order** (B design.md:222-239). It parses before the failure check. An untolerated nonzero exit with bad JSON would then report `HookJson` instead of the v1 `ApplyError::Hook`. The prose at :237 says to check failure first. The synthesis must state the order: failure check first, then decode, then parse. A has it right (A design.md:184-191).
- **B's "metadata required ⇔ `when` or `allow-failure`" rule is incomplete.** An interview-hook producer can also be skipped by an enclosing group or branch condition, not only by its own `when`. The rule must be "the producer may not run", computed from the retained program. A's `is defined` guard has no such rule, so the same gap only moves to run time there.
- **A breaks v1 invariant 3 ("never undefined")** for the JSON value. A states this openly at A rationale.md:369.
- **Both** assume the minijinja undefined mode stays lenient. Neither checks what happens on a read through `none` (B's not-run value, as in `registration.handle`). This needs a fixture in the 1063 implementation.

## 3. Reconciliation decisions

### a. Metadata channel → B's shape, A's field name

- B's `{exit_code, stdout, stderr, parsed}` is better. When a tolerated failure prints non-JSON (for example error text on stdout), B still gives the author the raw text. A discards it, so the author sees an undefined `<id>` with no explanation.
- A's case against two sources of truth only applies when the parse succeeded. There, `<id> | tojson` is enough and raw stdout is simply unused.
- For the field name, graft **A's `status-id`**. `-id` says the value is an identifier in the authored-id space, like `id`. `metadata: report_run` reads like a value, not a name.
- Keep B's rule that a declared name must be read. Keep A's load error for `<status-id>.stdout` only if raw stdout is dropped; with B's shape it does not apply.

### b. "Did not run" → B (`none` plus a required status name)

- B keeps v1 invariant 3. Every read name is bound, and `exit_code is none` ⇔ did not run, the same test as v1.
- A's `undefined` adds a second guard idiom (`is defined`). It also turns a forgotten guard into a render fault after files are written. A calls that "failing loudly", but it is a partial apply.
- B's rule to require a status name when the producer can finish without a value puts that ambiguity at load time. Apply the fix from §2: base the rule on "may not run", not on "has `when`".

### c. `<id>.exit_code` on a JSON hook: load error (B) or plain key read (A) → B

The analyzer can distinguish the two. minijinja 2.24 `meta.rs:147-182`, with `undeclared_variables(true)`, emits `lint.exit_code` for a `GetAttr` chain that ends in a variable. For `GetItem` it emits only `lint`, plus names inside the subscript expression. So `<id>.exit_code` and `<id>['exit_code']` are distinct in the output.

- Today Toha calls `undeclared_variables(false)` (`src/jinja.rs:98,130`). v1 already needs `nested=true` for its "unknown hook result field" and "stream read but not captured" checks. B adds no new analyzer requirement beyond what v1 already needs.

Known blind spots, where B's check misses:

- A result aliased in an args template: `{% set x = lint %}{{ x.exit_code }}`.
- A `for` loop variable.
- A chain rooted in a non-variable expression.

In those cases the read falls through to a plain JSON-key read, which is exactly A's behavior. B therefore degrades to A and is never worse. Its only cost is a false positive on JSON that really has `exit_code`, `stdout` or `stderr` keys. The subscript form is the escape, and the error message names it.

A misses the most likely migration mistake silently: add `parse: json` to a v1 hook and `lint.exit_code != 0` becomes `undefined != 0`, which is true. The synthesis should record that the check is best-effort, with the aliasing blind spot listed.

### d. Tolerated-failure parsing → B

- Both are lenient: parse is tried and a failure is not fatal. A also has a status channel, but it is optional in general and required only with `allow-failure`.
- B makes the result explicit: `parsed=false`, `<id>` is `none`, and raw stdout is kept. `report_run.parsed` is a real disambiguator, because `<id>` = `none` could otherwise be JSON `null`.

### e. Syntax: implied capture (A) or required `capture: [stdout]` (B) → B, marginally

- B leaves v1 invariant 4 ("capture equals reads"; `capture` is the only place that controls piping) without an exception. A adds "except stdout under `parse`" to that invariant and a new load error for the redundant form.
- A saves one YAML line. B keeps one source of truth for "stdout is not shown on the terminal" and keeps the 1069 guard classification simpler.
- This is a two-way door and a low-weight choice. It is worth a line in the decisions put to Bob.

## 4. Verdict

**B ranks first (68), A second (63).** The synthesis should be based on B.

### Graft from A

| Idea | Reason |
| --- | --- |
| Field name `status-id` instead of `metadata` | So that the name reads as an authored identifier, like `id`. |
| One `HookOutput { fault: OutputFault }` with `NotUtf8 / Empty / NotJson{line,column}` instead of B's separate `HookJson` | So that all stdout-content faults share one variant and one "no bytes in Display" guard. |
| Apply-loop order from A design.md:184-191 (untolerated or signal: not parsed, v1 `Hook` error first) | So that the real failure is never hidden behind a parse fault. |
| Explicit "BOM / several documents → NotJson" rows | So that every edge case has a falsifiable row. |
| The `enabled` (bool as `when`) and `channel \| upper` examples | So that the guide shows every JSON type in one place. |

### Graft from B

| Idea | Reason |
| --- | --- |
| Status shape `{exit_code, stdout, stderr, parsed}` | So that a tolerated non-JSON failure can be explained. |
| `<id>` = `none` when not run or not parsed; never undefined | So that v1 invariant 3 holds and there is one guard idiom. |
| Status name required when the producer may not run or has `allow-failure`, **computed from the retained program** (fixed per §2) | So that `none` never has two meanings without a disambiguator. |
| Attribute-form `<id>.exit_code/.stdout/.stderr` load error, subscript as the escape, marked best-effort | So that the likely migration mistake fails at load; it degrades to A's behavior. |
| `parse: json` requires `capture: [stdout]` | So that `capture` stays the only place that controls piping. |
| Invariant 7: a parsed string is data, not a template | So that nobody adds re-rendering later. |
| The 8-row state table and the `verification.md` checks | So that implementation 1063 has falsifiable targets. |

### Reject

| Idea | Reason |
| --- | --- |
| A: undefined as "did not run" | It breaks v1 invariant 3 and moves a guard omission to after files are written. |
| A: raw stdout dropped in JSON mode | A tolerated failure with non-JSON output cannot be diagnosed. |
| A: "no field check on `<id>`" | It loses the only load-time catch for the migration mistake, and the analyzer supports the check. |
| A: `capture: [stdout]` + `parse` as a load error | It adds an exception to v1's capture invariant for one saved line. |
| B: separate `HookJson` variant | It duplicates `HookOutput`; merge the two. |
| B: metadata-required trigger = "has `when`" | It misses group or branch-skipped interview producers. |
| B: pseudo-code parse-before-failure-check | It conflicts with B's own prose and v1's error precedence. |

### Assumptions and risks for the architect

- **Assumption:** 1075/1063 switch the reference analysis to `undeclared_variables(true)`. v1 already needs this. It is not done in the code today.
- **Risk:** what a read through `none` does (`none.key`) under Toha's undefined mode is not verified by either candidate. Add a fixture.
- **Risk:** serde_json without `arbitrary_precision` turns integers larger than u64 into f64, which can change their value. Both candidates accept this. Keep it disclosed.
- **Decision for Bob:** implied capture vs required capture (3e). It is a two-way door with a small preference for B.
