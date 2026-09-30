# Common task — design in-project generators for Toha 0.2.0

You are one independent design candidate in an arena. Produce a **design
package** (a `design.md` sketch and a `rationale.md`), not code. Do not edit any
production source, spec, or schema. Write only to the output directory you are
given.

## The feature

Toha generates a project from a template today with `toha apply TEMPLATE PATH`.
**In-project generators** let a caller apply a template **repeatedly into
subpaths of an existing project** — e.g. "add another component", "add a
module" — where **each application has its own answers and its own identity**,
and where a caller has **explicit access to the project's existing snapshots as
optional defaults** for the new application's answers.

Concretely, the design must specify:

1. **Generator CLI (and, if you justify it, template) syntax** for applying a
   template repeatedly at project subpaths.
2. **Per-application answers and identity** — how repeated applications are
   distinguished and how each carries its own answers.
3. **Explicit access to available project snapshots as optional defaults**,
   using the approved snapshot design (below).
4. **Snapshot selection**, **collision / ambiguity / error behavior**,
   **cross-snapshot precedence**, and **compatibility with configured defaults**
   (presets / template-defaults).
5. A **falsifiable fixture** applying the same generator **twice with different
   answers in one project**.

## Fixed policy (already decided by the product owner — do NOT re-open)

- Available snapshots are used as **optional** generator defaults. When a
  snapshot is present, its recorded answers seed the new application's defaults.
- When snapshots are **absent** (non-git target, dirty target, or a clone that
  has not fetched snapshot refs), generator application still works: fall back to
  explicit **configured presets / template-defaults**, then to normal questions.
- An **explicitly required** snapshot reference that cannot be satisfied
  (unknown, ambiguous, or absent) **fails clearly**.
- **No independent second persisted-answers lifecycle.** Prior-application
  answers come from snapshots or configured defaults; nothing new is persisted
  into the project.
- **Never infer semantic identity from matching question ids.** Existing
  per-template `template-defaults` mappings and `{ preset: <name> }` references
  are preserved exactly.

## What you must read first (your inputs)

All paths are in the worktree you are running in
(`/workspaces/worktrees/toha/task1067-generator-design`):

- `docs/technical-designs/second-release/in-project-generators/grounding.md` —
  the traced current behavior and the Preserve/Change/Avoid/Risk constraints.
  **This is authoritative.** Honor every Preserve and Avoid constraint.
- `docs/technical-designs/second-release/project-updates/design.md` — the
  **approved** snapshot producer contract you consume (the `toha::snapshot`
  `Project`/`Snapshot` reader surface, `source`/`target`/`submissions`,
  when snapshots exist). Its implementation is a concurrent task; you consume
  the reader contract only and must not edit snapshot capture, the `--from`
  update merge, the replay adapter, or the `snapshots` commands.
- `docs/technical-designs/second-release/template-defaults/design.md` — the
  approved `presets` / `template-defaults` config model you must stay compatible
  with.

Do **not** read any `rubric.md`, `cross-judge*.md`, `synthesis*.md`, or any
other candidate's directory. Your design must stand on the task and the
grounding alone.

## Hard constraints

- Preserve the pure interview engine and its `Seed.defaults` contract; every
  adapter (terminal, headless, staged, agent, crate) must keep working and
  produce identical results for identical inputs.
- A snapshot default enters the interview only through the engine's existing
  seed/default-bank boundary. If you need a snapshot default and a configured
  default to coexist for one question id, say exactly how (the current
  `DefaultBankEntry` holds one occupant per id).
- Reuse the single per-target apply spine (`setup → canonical_target →
  build_context → start_with_context → Plan::build → apply_reporting`). A `Plan`
  is single-target by contract.
- Slot into the existing exit-code vocabulary (0 success; 1 load/conflict/hook/
  mismatch; 2 invalid CLI; 3 untrusted hooks; 4 questions remain; 5 ambiguous
  template name). The scripted machine route is selected by `--answers`, not
  `--json`.
- Keep the generate axis distinct from the update axis (`apply --from`, same
  target). Do not overload `--from`.
- No new permission/access change, timeout, pinned-version check, or application
  subprocess. If your design would introduce any of these, stop and say so
  explicitly instead of designing it in.
- Design only. Keep all pseudocode and `not implemented` sketches inside the
  design artifacts; do not propose editing shared spec/schema/runtime now
  (describe proposed contract edits in prose for the paired implementation).
- Use generic, non-identifying example values and scenarios.

## Deliverable shape (write both files to your output directory)

### `design.md` — the sketch (caller usage FIRST)

1. **Caller usage (the spec).** Write the caller's usage first, with **two or
   three concrete call sites** across routes (person terminal, scripted
   `--answers`, agent staged), including: a first generator application into a
   subpath; a second application of the **same** generator into a **different**
   subpath defaulting from the first; and the snapshot-absent fallback. Show
   real command lines and, for the scripted route, the result document shape.
2. **Data structures expected** (Rust type sketches with `not implemented`
   bodies / pseudocode).
3. **Public interfaces expected** (function/type signatures), including the
   exact seam where a snapshot's `submissions` become defaults and how
   precedence with configured defaults is realized.
4. **Module and seam diagram** (text), showing what is new vs reused and what is
   consumed from the snapshot reader contract.
5. **Snapshot selection, precedence, collision/ambiguity, and the error /
   results contract** (a table across snapshot counts × routes, and the failure
   cases, mapped to exit codes).
6. **Behaviors to prove** (falsifiable), including the required
   twice-in-one-project fixture, plus a sole-kill for each load-bearing rule.
7. **Compatibility and canonical-document impact**, and **out of scope**.

### `rationale.md` — the reasoning

Name the alternatives you considered and **what you rejected and why** (this is
the highest-signal part). State your key decisions, the tradeoffs, and where your
design pushes back on the existing shape. Include your interface-depth argument
(what complexity you hid behind how small a surface).
