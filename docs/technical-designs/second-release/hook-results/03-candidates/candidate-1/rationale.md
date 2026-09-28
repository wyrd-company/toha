# Candidate 1 rationale — Deferred-render results program

## Why this shape

The one true constraint the runtime imposes is timing: all Jinja renders before
any hook runs. The smallest structural change that lifts it is to **retain the
few fields that reference a result and render them later**, inside the one place
that already sequences hooks — the apply loop. Nothing else moves. Order stays
the existing linear hook order, which authors already understand ("hooks run in
list order; interview hooks first"). The reserved `hooks` map is the only new
context value, and it is populated by the effect loop, not the engine — so the
context design's "engine reads nothing ambient" holds unchanged.

## How it honors each hard constraint

- **1075 context.** Result references are static names found by the same
  `undeclared_variables` analysis already unioned into the retained program;
  they contribute zero environment needs and are analyzed before admission. The
  engine performs no ambient read; `hooks` is effect-loop output. `hooks` is a
  new reserved name added to the collision set; it is not one of the seventeen
  and not `preset`/`presets`. No target normalizer, resolver, or replay change.
- **1077 confirm-flow.** The interleave is inside `Plan::apply`; stop/abort never
  reach it; dry-run runs no hooks so results are empty and nothing is applied.
- **1076 includes.** Deferred hook fields hold guarded `Tmpl`, never `FileTmpl`;
  no include surface is added.
- **1069 trust.** New fields ride the parsed-node digest; no new executable file
  reference; guard test required.
- **Opt-in / no leakage.** A hook with no `id`/`capture` is byte-identical to
  today. Files/before-apply referencing `hooks` is a load error, so stdout/stderr
  can never reach generated files. `allow-failure` is a separate explicit opt-in.

## Alternatives considered

- **Capture-all implicitly (no `capture` list).** Rejected: capturing stdout for
  every hook changes `status()` to `output()` universally and risks buffering
  large output; explicit `capture` keeps the default path (`status()`) untouched.
- **Per-hook top-level names (`fmt_exit_code`).** Rejected: pollutes the id
  namespace and multiplies collision rules; one `hooks` map is cleaner.
- **Reorder writes after hooks so files can read results.** Rejected: routes
  command output into generated files (violates the inherited constraint) and
  breaks the "each file reported before any hook" contract.

## Sharpest tradeoff

Referencing an uncaptured field yields runtime `undefined` rather than a
load-time error — opt-in is enforced for *exposure* (results only bind when
captured) but a typo like `hooks.fmt.stdout` when only `exit_code` was captured
is silently `undefined`, caught only by the author testing. Candidate 2 trades
this away with load-time field checking at the cost of a heavier declaration.

## Synthesis decision

<!-- filled by the architect -->
