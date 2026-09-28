# Candidate 2 rationale — Typed capture bindings + load-time schedule

## Why this shape

The include design (1076) taught the epic a durable lesson: push enforcement
into an owning boundary and prove each rejected shape at load. This candidate
applies it to hook results. Every result reference is resolved and type-checked
at `Template::load` against the producer's declared capture set, so the classes
of author error that Candidate 1 leaves to runtime `undefined` — a typo, a field
that was never captured, a forward reference — become named load errors. The
apply loop is then a dumb consumer of a proven-safe `HookSchedule`; all
ordering/typing lives in one deep function, `schedule_hooks`.

## How it honors each hard constraint

Same composition as Candidate 1 (static analysis pre-admission; effect-loop
binding; `results` is a new reserved name, not one of the seventeen, not
`preset`/`presets`; producer/replay/resolver seams untouched; includes not
extended; parsed-node digest coverage; opt-in; files/before-apply excluded at
load). The difference is *where* correctness is proven: at load, field by field.

## Alternatives considered

- **Runtime-none for uncaptured fields (Candidate 1).** Simpler declaration but
  weaker guarantee; a mis-typed field is invisible until the author runs it.
- **Full DAG reordering of hooks.** Rejected: authors expect list order to be
  run order (side effects between hooks); a scheduler that *reorders* would
  surprise. The schedule here is the linear order *validated* as topologically
  sound, not a reordering.
- **`capture: [exit_code, stdout]` list vs `capture: { exit_code: true }` map.**
  The map reads as field-by-field opt-in and extends cleanly; a list is terser.
  Left as a synthesis choice.

## Sharpest tradeoff

Heavier authored surface: two new declarations (`capture` map + `on-failure`)
and a load-time model that must track which fields each id captured. The
type-checking cannot prove a hook *runs* (only that its field *would* exist), so
runtime-`none` guarding for skipped producers is still required — the load-time
strictness does not remove the runtime absent case, it only removes the
uncaptured-field case. For a first release the extra ceremony may exceed what
templates need.

## Synthesis decision

<!-- filled by the architect -->
