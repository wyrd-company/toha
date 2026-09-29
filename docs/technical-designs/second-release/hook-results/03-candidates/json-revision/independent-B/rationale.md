# Rationale — candidate B

## The problem in one sentence

When `<id>` becomes the parsed JSON, the name `<id>` can no longer also be the
v1 metadata object. So the design must say where `exit_code`, raw `stdout`, and
`stderr` go. It must also say how a reader tells "JSON `null`" from "did not
run" and from "could not parse".

## Chosen reconciliation: two names, declared by the author

`<id>` is the parsed value, as is. The metadata is at a second identifier that
the author declares with `metadata: <name>`. It is required only when the
producer can end without a parsed value, which is when it has a `when` or
`allow-failure`.

Why:

- **No collision is possible.** Parsed keys and metadata live in different
  values. A JSON object with a key named `exit_code`, `stdout`, or `parsed` is
  still read exactly. No key is shadowed, renamed, or reserved.
- **The representation is explicit in the template.** A reader of
  `template.yml` sees both names at the producer. Trust review sees both, and
  the 1069 digest covers both.
- **No new reserved name.** The metadata name joins the authored-id space and
  uses the existing duplicate and 1075 collision checks. v1 decision 2 (hook ids
  share the answer-id space) extends with no new rule.
- **`none` never has two meanings without a disambiguator.** A producer that
  always runs and must parse has only one state at every reader, so `<id>` is
  the value. A producer that can end in another state must declare metadata,
  and `<name>.exit_code` / `<name>.parsed` say which state it is. This is the
  JSON-mode form of v1's "`exit_code is none` ⇔ did not run".
- **It reuses the v1 machinery.** The metadata object is the v1 result object
  plus one field, `parsed`. Capture rules, readable surfaces, decode, and the
  failure policy are unchanged.

## Alternatives considered

| Alternative | Why not chosen |
| --- | --- |
| **A. Mode switch that drops metadata.** `<id>` = parsed; `exit_code`/`stderr` not available. | Loses the exit code for `allow-failure` and loses "did not run" for conditional producers. The JSON-report-with-nonzero-exit case (example 1.4) cannot be written. Fails the lens. |
| **B. Merged object.** `<id>` = parsed object plus `exit_code`/`stdout`/`stderr` keys. | Collides with JSON keys of the same names. It does not work for arrays or scalars at all (there is nowhere to put the keys). Needs a precedence rule that silently hides data. |
| **C. Reserved sub-key**, for example `<id>.__toha` or `<id>._meta`. | Still a collision (less likely, not impossible). Fails for arrays and scalars. Needs a wrapper object type, which breaks "the parsed value is exactly `<id>`". |
| **D. Fixed derived name**, for example `<id>_result`. | Implicit: the name appears nowhere in the template, so a reader must know the rule. Can collide with an author's own id, so it needs a new collision error for a name nobody wrote. |
| **E. One reserved namespace**, for example `hooks.<id>.exit_code`. | Uniform and collision-free with JSON, but adds an eighteenth reserved name. That reopens the approved 1075 list and breaks any template with an answer id `hooks`. Reasonable second choice. |
| **F. Parsed at a sub-key**, `<id>.json`. | Keeps v1 fully, but Bob asked for the value at `<id>`, not at a sub-path. |
| **G. Custom minijinja object** that acts as the JSON but hides metadata for a filter such as `<id> \| exit_code`. | Collision-free, but the value is no longer a plain `serde_json::Value`. It is a new render path, `tojson`/`items` behavior must be re-proved, and the metadata is invisible in `template.yml`. |

## Tradeoffs we accept

- **Two names per producer when metadata is needed.** Slightly more YAML. In
  exchange, the template says what each name holds.
- **`metadata` is sometimes required.** An author who writes `parse: json` with
  `when` gets a load error until they add it. The message says what to add. We
  judge a forced guard better than a silent `none` that can mean three things.
- **`<id>.exit_code` attribute form is a load error in JSON mode.** A JSON
  producer that prints a key named `exit_code` must be read as
  `<id>['exit_code']`. This is a small cost for catching the likely migration
  mistake: adding `parse: json` to a v1 hook would otherwise turn
  `lint.exit_code != 0` into `undefined != 0`, which is true, with no error.
  This relies on the 1075 analysis keeping attribute and subscript access apart.
  That is an assumption; impl 1063 must confirm it.
- **`parse: json` requires `capture: [stdout]`.** This is redundant, because
  parse implies capture. We keep it so that `capture` stays the single source of
  truth for which streams are piped instead of inherited.
- **Tolerated nonzero is lenient about JSON; exit 0 is strict.** A tool that
  exits 0 has promised JSON, so bad output is a fault. A tool that failed may
  print an error text instead, and `allow-failure` exists to continue. The
  `parsed` flag keeps that leniency visible.
- **Empty stdout on exit 0 is fatal, not `none`.** Empty is not JSON. Treating
  it as `null` would add a second meaning to `none`. A tool with no value should
  print `null`.

## Assumptions

- The 1075 retained-program analysis can tell `GetAttr` from constant `GetItem`
  (needed only for the attribute-form load error).
- serde_json default features (sorted object keys, no arbitrary precision) are
  acceptable. Integers beyond u64 become f64.
- Minijinja lenient undefined remains the render policy for missing keys.

## Risks

- A number beyond u64 or a float with many digits can change value when
  rendered. This is the serde_json default. Using the `arbitrary_precision`
  feature would change the behavior of existing answers too, so it is out of
  scope.
- Large JSON output stays in memory for the apply. As in v1, there is no size
  cap. Adding one is a new limit and needs Bob's approval.
- Two names per producer grow the authored-id space. Collisions are caught at
  load by existing checks.

## And then what

The likely next request is `each: <id>` over a parsed array, so that one hook
runs per item. v1 makes `each` eager and forbids result reads there. This design
does not change that, and it does not block it: the parsed array is already a
plain `serde_json::Value` at a static name.

## Synthesis decision
