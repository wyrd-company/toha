# Arena common task — opt-in Jinja hook results

Given to every candidate runner. The scoring rubric is **not** given.

## The problem

Toha templates define **hooks**: commands run after files are written. Today a
hook yields only pass/fail (`HookOutcome { success, code }`); no Jinja surface
can read what a hook did, because **all Jinja renders at plan-build time, before
any file is written and before any hook runs** (`Plan::build`, `src/plan.rs`),
while hooks run last in `Plan::apply` (`src/apply.rs`).

Design an **opt-in** feature that exposes a finished hook's **`exit_code`,
`stdout`, and `stderr`** to **later** Jinja surfaces, enabling a real
conditional flow — e.g. run a later hook only when an earlier one succeeded, or
report a hook's output in the closing message.

## Grounding (read first, do not repeat it)

Read `docs/technical-designs/second-release/hook-results/01-grounding.md` in the
repo for the full traced model and the exact predecessor contracts. Key facts:

- `template::HookNode`/`HookCommand`/`HookProgram` (`src/template.rs:155-170`) —
  **a hook has no id today.** `interview::RenderedHook`
  (`src/interview.rs:105-170`), `plan::PlannedHook` (`src/plan.rs:109-129`),
  `hook::HookOutcome` + `ProcessRunner::run` using `command.status()`
  (`src/hook.rs:11-57`, no stdout/stderr capture).
- All hook fields render in `Plan::build` (`src/plan.rs:209-309`); hooks run last
  in `Plan::apply_reporting` (`src/apply.rs:160-176`), nonzero stops remaining
  hooks and fails the apply; untrusted + hooks → `Applied::NeedsTrust`.
- `StagedRecord` stores submissions, not results (`src/staging.rs:20-29`); replay
  never re-runs hooks.

## Hard constraints (violating any is disqualifying)

- **Compose with the approved context design (1075).** Static reference analysis
  of any new Jinja names stays part of the complete retained render program at
  `Template::load`, before stage admission; the pure engine performs no ambient
  read; add no new environment surface. Do not touch the seventeen reserved
  `toha_*` names or the collision rule; do not name a value `preset`/`presets`.
  Consume `canonical_target`/`CanonicalTarget::as_path()`; add no second
  normalizer or `TargetError`. Do not flatten or add a second resolver to the
  configured `Resolution` route. Replay stays record-owned with no live read.
  Staged `apply --trust` stays hook-execution-only.
- **Compose with confirm-flow (1077).** Hooks run only at the single write/hook
  site (`Completed::step() -> Step::Plan { apply: true }` and not `--dry-run`);
  `Ended{Stop|Abort}` is structurally unplannable; dry-run builds/shows the plan
  but runs nothing.
- **Compose with includes (1076).** Hook fields are **not** an include surface;
  result-reading fields hold guarded `Tmpl`/`Expr`, never `FileTmpl`.
- **Trust (1069).** Do not change hook trust requirements. New hook-node fields
  are covered automatically by the parsed-node digest. State whether any new
  field can name a *new* in-tree executable file; if not, say so and require a
  guard test that fails when a future field introduces such a reference without
  a coverage decision.
- **Opt-in + no leakage.** Exposure is opt-in; existing hooks (no id, no capture)
  behave exactly as today. Never silently place stdout/stderr into generated
  files or public output. Preserve the default nonzero-stops-and-fails behavior;
  any tolerance of nonzero must be explicit and opt-in — no silent weakening.
- **Design only.** No runtime edits, no shared canonical spec/schema edits.
  Sketches and proposed contract edits stay in the design artifact. Use generic,
  non-identifying examples.

## What to produce (write to your isolated output directory only)

Two files in your assigned `/tmp/arena-hook-results/candidate-<n>/`:

- `design.md` — in this order:
  1. **Caller usage first** — two or three concrete `template.yml` snippets a
     template author writes (a conditional later hook reading an earlier hook's
     `exit_code`; an after-apply message reading `stdout`), plus one crate/driver
     call site showing where results are bound during apply.
  2. **Data/type sketch** — new/changed Rust types (authored node, rendered,
     planned, outcome, result value, results context) with `not implemented`
     bodies where needed.
  3. **Function signatures** — the seams that render deferred surfaces, run
     hooks, bind results, and validate ordering at load.
  4. **Module/seam diagram** — which module owns what; where the interleave
     lives.
  5. **Error/results contract** — nonzero, absent/unrun, duplicate id, encoding,
     dry-run, stop/abort, load-time ordering/collision errors.
- `rationale.md` — why this shape; alternatives considered; tradeoffs; how it
  honors each hard constraint; explicit `Synthesis decision` section left blank
  for the architect.

## Settle these decisions explicitly

Identity + opt-in syntax; result namespace + collision; readable surfaces +
phase/ordering model with a static availability guarantee (a value exists before
it is read); capture scope + non-UTF-8 decode + trailing newline; nonzero
interaction (explicit opt-in, no silent weakening) + absent/unrun; dry-run +
stop/abort suppression; trust digest coverage + guard test; backward
compatibility and canonical doc/schema impact (described, not applied).

Design it as a whole shape. Prefer hiding the interleave complexity behind a
small public surface. State every invariant in the interface.
