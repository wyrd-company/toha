# Arena common task — revision: parse stdout as JSON into `<id>`

The scoring rubric is not given.

## The change

The v1 design exposes a hook result as an object `<id> = {exit_code, stdout,
stderr}`. Bob asks for an **opt-in** mode that parses a hook's stdout as JSON and
exposes the **parsed value directly at `<id>`** — so `<id>.version`, `<id>[0]`,
or a scalar `<id>` work directly — **not** silently only at `<id>.stdout`.

Design that revision as a delta on the v1 design. The hard question you must
settle: with the parsed JSON at `<id>`, how do the execution metadata
(`exit_code`, raw `stdout`, `stderr`) coexist without collision or ambiguity?

## Read first (read-only; do not edit any repo file)

- `docs/technical-designs/second-release/hook-results/design.md` — the v1 design you revise.
- `docs/technical-designs/second-release/hook-results/06-json-revision-grounding.md` — grounding for this change (serde_json is already a dependency; the context already carries serde_json::Value).
- Do NOT read the sibling runner's output directory.

## Hard constraints (unchanged from v1, plus)

- Opt-in and explicit; a hook without the new field behaves exactly as v1/today.
- Never place output into generated files or public output; readable surfaces
  unchanged from v1 (later hooks + after-apply). Default stop-and-fail preserved.
- Compose with 1075/1076/1077/1069 as v1 does; the parse-mode field rides the
  1069 parsed-node digest; no new executable reference; no new dependency
  (serde_json already present — say so; no third-party link needed unless you
  adopt something new, which you should not).
- Static reference analysis unchanged (the name `<id>`/`<id>.path` is static; the
  value shape is runtime).
- Design only; no runtime or shared-canonical edits.

## Settle explicitly

Representation reconciliation (metadata vs parsed at `<id>`); explicit
mode/syntax and its relation to `capture`; ALL JSON value types (object → keys,
array → index, string/number/bool/null → scalar); malformed JSON; empty stdout;
non-UTF-8; nonzero + allow-failure (is stdout parsed on a tolerated failure, and
what is `<id>` then?); unrun/skipped producer; dry-run; trust/digest coverage.

## Produce (write ONLY to your assigned output dir)

`design.md` (caller `template.yml` + Jinja examples FIRST, then types/signatures
delta, then the full error/results contract for the JSON mode) and
`rationale.md` (why this reconciliation; alternatives; tradeoffs; empty
`## Synthesis decision` at the end). Plain manager-readable where prose; concrete
config + Jinja examples. Print a 5-line summary at the end.
