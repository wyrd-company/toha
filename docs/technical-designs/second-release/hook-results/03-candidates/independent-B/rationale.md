# Rationale — candidate independent-B

## Why this shape

Lens: static safety. Every result read is resolved at `Template::load` to a
producer slot with a known capture set, failure mode, and conditionality. So
the questions an author gets wrong — does it exist yet, is it captured, can it
be absent, is this failure checked, can this surface see it — are answered
before admission, with authored paths, in one aggregated `LoadError`.

- **Id as root in the single id space** instead of a namespace root such as
  `hooks.<id>`. The existing duplicate rule and the 1075 reserved-name rule
  apply with no new reservation. A namespace root would need a new reserved
  name, which breaks existing templates that use it as an answer id; `toha_*`
  would alter the reserved set.
- **Literal-field-only reads (L8)** let the table be exact: every read names
  a producer and a field, so L7/L9/L10/L11/L12/L13 are decidable.
- **Fixed slot count** (no results in `each`, no `id` on `each`): the dry-run
  lists every slot apply will consider, and a producer is one invocation.
- **Ready/Deferred split**: hooks that read no result stay byte-identical;
  only readers move into the apply loop. The interleave lives in one function.
- **Runner receives only `PlannedHook`**: an unrendered hook cannot reach a
  runner by type.

## Alternatives considered

1. `hooks.<id>.field` namespace root — rejected (new reservation; weaker
   typo detection unless path analysis anyway).
2. Render everything after apply (move all hooks to apply) — rejected: changes
   every template, loses plan-time errors and dry-run argv.
3. Lossy UTF-8 decode — rejected: silently corrupts values passed to later
   hooks.
4. `on-failure` allowed without `id` — rejected: tolerated failure no one can
   read is silent weakening.
5. Runtime error on reading an unrun producer's fields — rejected: minijinja
   field access cannot error cleanly; L13 moves the check to load.
6. List-valued results for `each` producers — deferred: adds a second value
   shape; can be added later as `<id>.runs` without breaking this contract.

## Tradeoffs

- Strictness rejects some valid templates (whole-value `tojson`, unread
  capture, `.ran` in a node the author knows always runs, `each` producers,
  output-derived program).
- L13 is conservative: a reference to `.ran` does not prove guarding.
- Captured streams are hidden from the terminal; stated in docs.
- Deferred `cwd` symlink check moves to just before that hook, after writes.
- Non-UTF-8 capture fails the apply after files are written, same class as a
  hook failure.
- No size limit on captured output. A limit is a new policy and needs user
  approval.
- Requires the 1075 analyzer to report literal-vs-other use of a root; if it
  cannot, `undeclared_variables(true)` is the fallback and needs a table test
  of accepted/rejected forms.

## Hard constraints

- **1075**: result names are analyzed in the same retained-program pass at
  load, before admission; engine reads nothing new; no environment surface;
  seventeen names and collision rule untouched (hook ids pass through them);
  no `preset(s)`; target via `CanonicalTarget::as_path()` only; `Resolution`
  untouched; replay record-owned; staged `--trust` hook-only.
- **1077**: results exist only in `apply_reporting`, reached only on
  `Plan{apply:true}` without dry-run; Stop/Abort have no plan.
- **1076**: result-reading fields are guarded `Tmpl`/`Expr`; hook fields are
  not include surfaces; files cannot read results (L6).
- **1069**: new fields are literal and name no file; digest covers them;
  guard test required.
- **Opt-in / no leakage**: no `id` → identical behavior; output only where
  named; stop-and-fail default; `continue` is explicit, requires `id`, must
  be checked (L12), and is reported.
- **Design only**: no runtime, schema, or spec edits.

## Synthesis decision
