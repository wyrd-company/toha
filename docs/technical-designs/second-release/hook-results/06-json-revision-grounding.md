# Grounding addendum — parse stdout as JSON into `<id>`

Bob's revision request (verbatim intent): add an **opt-in** mode that parses a
hook's stdout as JSON and exposes the **parsed value directly at `<id>`**, not
silently only at `<id>.stdout`. This is a shape change to the v1 design
(`design.md` at HEAD `a3f1708`, unapproved), so Phase B is re-run for the
representation question. The v1 three decisions remain **open** — Bob's addition
does not approve them.

## What changes vs v1

v1 exposes a result as an object `<id> = {exit_code, stdout, stderr}`. The
revision must let the author choose, per hook, to have `<id>` carry the *parsed
JSON* of stdout so that `<id>.version` / `<id>[0]` / a scalar `<id>` work
directly. The open design question is how the parsed value and the execution
metadata (`exit_code`, raw `stdout`, `stderr`) coexist without collision.

## Grounding facts (this repo, HEAD a3f1708)

- **No new dependency.** `serde_json = "1"` is already in `Cargo.toml:32` and is
  used across the crate (`src/jinja.rs`, `src/interview.rs`). JSON parsing uses
  the existing `serde_json::from_slice`/`from_str`. No third-party adoption or
  approval is needed; disclose this rather than implying a new dependency.
- **The context already carries `serde_json::Value`.** `jinja::context_from_answers`
  returns `BTreeMap<String, serde_json::Value>` (`src/jinja.rs:196`) and every
  answer becomes a `Value` via `Answer::to_json`. A result is serialized into
  that map. So binding `<id>` to a parsed `serde_json::Value` is the *same
  mechanism* as any other context value — minijinja renders objects, arrays,
  and scalars uniformly. A parsed JSON object's keys become `<id>.<key>`; an
  array is indexable; a scalar is the value itself.
- **Decode already exists in the v1 design.** stdout is captured as bytes and
  decoded strict-UTF-8 (fatal on non-UTF-8). JSON parse is a second step *after*
  decode: parse the decoded text, fatal on malformed JSON — same failure class
  as non-UTF-8 and today's non-UTF-8 file error (`src/plan.rs:179`).
- **Static analysis unaffected.** Whether `<id>` is a metadata object or a
  parsed value, the *name* `<id>` (and any `<id>.<path>`) is the same static
  reference the 1075 retained-program analysis already sees at load; the value's
  shape is a runtime fact. Parsing changes no admission need.
- **Trust/digest.** The parse-mode field is a parsed hook-node field → covered by
  the 1069 digest automatically. Parsing stdout introduces no executable-file
  reference and derives no program from output; `run[0]` still cannot read a
  result.

## Decisions the revision must settle (arena targets)

1. **Representation reconciliation.** With parsed JSON at `<id>`, where do
   `exit_code`/raw `stdout`/`stderr` live? (mode-switch that drops metadata vs a
   companion that preserves it vs a merged object).
2. **Explicit mode/syntax.** How the author opts in (e.g. `parse: json` on the
   producer), and its relationship to `capture`.
3. **All JSON value types.** object → `<id>.key`; array → `<id>[i]`; string/
   number/bool/null → scalar `<id>`. Define each.
4. **Malformed / empty / non-UTF-8.** malformed JSON and empty stdout behavior;
   non-UTF-8 stays fatal.
5. **Nonzero / allow-failure interaction.** Is stdout parsed when the hook
   failed but is tolerated? What is `<id>` then?
6. **Phase / unrun / dry-run.** an unrun producer's parsed `<id>`; dry-run has no
   parse.
7. **Trust / digest coverage.** confirm auto-coverage; guard test unchanged.
8. **Canonical / paired-1063 scope.** described, not applied.
