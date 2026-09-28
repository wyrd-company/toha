# Grounding — opt-in Jinja hook results

Task 1078 (design), paired implementation 1063. Role: architect. Phase A.

Base: epic `9a6d902a97c225069cd87d3299a5f0b2703683df`. This artifact traces the
actual code and the approved predecessor contracts a hook-results design must
compose with. It records evidence, inference, searched-source gaps, and
Preserve / Change / Avoid / Risk constraints. It resolves no design; that is
the arena's job.

## Objective (problem this feature solves)

A template can run hooks — commands executed after files are written. Today a
hook produces only pass/fail; nothing a template author writes can read what a
hook did. Authors want a **real conditional flow**: run a later hook only when
an earlier one succeeded, pass an earlier hook's output to a later one, or
report a hook's result in the closing message. This feature exposes, **opt-in**,
a finished hook's `exit_code`, `stdout`, and `stderr` to **later** Jinja
surfaces, without changing hook trust and without silently placing command
output into generated files or public output.

## How — traced caller-to-result flow (file:symbol evidence)

### Hook data model, load → render → plan → run

- **Authored node.** `template::HookNode { command: HookCommand, each:
  Option<Each>, when: Option<Expr> }`; `HookCommand { program: HookProgram, cwd:
  Option<Tmpl> }`; `HookProgram::Run(Vec<Tmpl>) | Script { path: PathBuf, args:
  Vec<Tmpl> }` (`src/template.rs:155-170`). **A hook has no id today.** Exposing
  `<hook>.exit_code` requires adding an opt-in identity field to the node.
- **Rendered form.** `interview::RenderedHook { program: RenderedProgram, cwd:
  Option<String> }`; `RenderedProgram::Run(Vec<String>) | Script { path, args }`
  (`src/interview.rs:105-116`). Produced by `render_hook` / `render_hooks`
  (`src/interview.rs:124-170`), which render each `Tmpl` against a
  `BTreeMap<String, Value>` context and evaluate `each` via `Each::contexts`
  (`src/template.rs`, `Each::contexts`).
- **Planned form.** `plan::PlannedHook { program: PlannedProgram, cwd:
  Option<TargetPath>, template_root: PathBuf }`; `PlannedProgram::Run(Vec<String>)
  | Script { path, args }` (`src/plan.rs:109-129`).
- **Outcome.** `hook::HookOutcome { success: bool, code: Option<i32> }`
  (`src/hook.rs:11-15`). **No stdout/stderr is captured.** `ProcessRunner::run`
  builds a `std::process::Command` and calls `command.status()`
  (`src/hook.rs:52`), which **inherits** the child's stdio to the terminal.
  Capturing output requires `command.output()` and a documented decode rule.

### When Jinja renders — the load-bearing timing fact

**Every hook field is rendered at plan-build time, before any file is written
and before any hook runs.** `Plan::build` computes one `ctx =
context_from_answers(&completed.answers, &template.data, &completed.now)`
(`src/plan.rs:209`) and renders, in this order: source-tree files and `files:`
rules (`src/plan.rs:225-264`, `walk`), interview hooks already rendered into
`completed.hooks` then `plan_hook`ed (`src/plan.rs:265-267`), top-level
`template.hooks` — evaluating `when`/`each` and rendering `run`/`args`/`cwd` here
(`src/plan.rs:268-295`, via `render_hooks`), and finally the before/after-apply
messages (`src/plan.rs:296-309`). The plan carries fully rendered
`PlannedHook`s.

Hooks run **last**, in `Plan::apply_reporting`: after conflict/symlink checks
and after all files are written (`src/apply.rs:110-159`), the loop
`for (index, hook) in self.hooks.iter().enumerate()` calls `runner.run(hook,
target.as_path())`; on `!outcome.success` it returns `ApplyError::Hook` —
**stopping remaining hooks and failing the apply** (`src/apply.rs:160-176`).
Untrusted with hooks present returns `Applied::NeedsTrust(self)` before any
write (`src/apply.rs:84-86`).

**Consequence:** no render surface can read a hook result today, because all
rendering finishes before the first hook runs. Exposing results to a later
surface requires **deferring that surface's rendering until after the producing
hook completes**, i.e. interleaving execution and rendering inside the apply
effect loop. Files are written before hooks run, so files cannot read results
without reordering writes after execution — which would also route command
output into files. The surfaces that already sit at/after hook execution are:
**later hooks' `run`/`args`/`cwd`/`when`/`each`** and **the `after-apply`
message** (`src/plan.rs:296-309`, shown "after the last hook",
`docs/template-hooks.md:151`).

### Reference analysis (static, compile-time)

`jinja::Tmpl::compile` and `Expr::compile` record `undeclared_variables(false)`
as `referenced_ids` (`src/jinja.rs:95-113`, `127-145`). This is the static
name-reference set. The approved context design already unions these across the
**complete retained render program**, including "every interview message and
hook Jinja field" and "every top-level hook and before/after apply message"
(1075 `design.md:242-248`). So a hook-result name reference is a **static,
load-time** fact available to conservative analysis before admission — even
though the value arrives at runtime.

### Persistence / replay

`staging::StagedRecord { target, template, commit, named, now, submissions:
Vec<IndexMap<String,Value>> }` stores **submissions (answers), not hook
results** (`src/staging.rs:20-29`). Replay reconstructs the interview from
submissions via `replay` / `replay_with_defaults` / `replay_with_resolution`
(`src/staging.rs:295-323`) and **never re-runs hooks**; hooks execute only in a
fresh `Plan::apply`. Hook results are therefore **per-apply**, not persisted:
they do not ride the versioned context wire and impose no new replay rule.

### Trust seams

- Trust gate: untrusted + any hooks → `NeedsTrust` dry run (`src/apply.rs:84`);
  `apply --trust` / registry approval grant execution.
- Review digest (1069): over the **parsed hook-node value** (all fields) plus
  bytes of every executed in-tree file (`script:` targets; literal in-tree
  `run`/`args` paths). A new hook-node field enters the digest **automatically**
  by construction.
- Target identity: `staging::canonical_target(&Path) -> Result<CanonicalTarget,
  StagingError>`; `CanonicalTarget::as_path()` is the only projection; no
  unchecked constructor, no `From<PathBuf>`, no `TargetError`
  (`src/staging.rs:41-155`, 1075 `design.md:154-158`).

## Why — ownership / layering constraints

The feature changes **when a hook field is rendered** (some hook fields move
from plan-build to the apply effect loop) and adds a **new opt-in result-data
category** to the render context. It does not change who owns the interview
engine, target identity, configured defaults, trust, or the fixed-seventeen
context.

- **Preserve.**
  - The pure engine reads no ambient state (1075 `design.md:19-20`). Hook
    results are produced by the **effect loop** (`Plan::apply`), not the engine;
    they are not an engine ambient read.
  - Hooks run only at the single write/hook site; stop/abort are structurally
    unplannable; dry-run suppresses the site (1077 `design.md:161-193`,
    `315-316`). Hook-result rendering must live inside that site and be fully
    suppressed on stop/abort/dry-run.
  - Default failed-hook behavior: a nonzero hook stops remaining hooks and fails
    the apply (`src/apply.rs:160-176`, `docs/template-hooks.md:28`). Any change
    to this must be explicit and opt-in — no silent weakening.
  - The fixed-seventeen reserved names and collision rule; no name is `preset`/
    `presets` (1075 `design.md:172-208`, `732-746`). A result namespace must not
    collide and must carry its own reservation/availability rule.
  - `CanonicalTarget`/`StagingError` producer seam; configured `Resolution`
    route; record-owned replay with no live/ambient read (1075 contracts 4–6).
  - Includes stay file-body-only and inert; hook fields are **not** an include
    surface (1076 `04-final-design.md:52-56`, `392`). Result-reading hook fields
    hold guarded `Tmpl`/`Expr`, never `FileTmpl`.
  - Do not silently place stdout/stderr into generated files or public output
    (inherited feature constraint). Result-reading surfaces exclude files and
    before-apply by construction.
- **Change.**
  - Hooks gain an **opt-in identity** and an **opt-in capture** declaration.
  - Some hook fields and the after-apply message render **after** the hooks they
    reference, against an accumulating results value.
  - `HookOutcome`/runner may capture stdout/stderr when a hook opts in.
- **Avoid.**
  - A late engine context read, a second normalizer/resolver, a new environment
    surface, a new trust/permission/timeout/pinned-check/subprocess policy.
  - Reordering file writes after hooks (would leak output into files and break
    the "each file reported before any hook" contract, `src/apply.rs:66-72`,
    tested at `src/apply.rs:257-274`).
- **Risk.**
  - Deferred rendering must remain deterministic and replay-stable; a value must
    exist before it is read (static ordering guarantee).
  - Non-UTF-8 stdout/stderr needs a defined decode rule.
  - Dry-run has no results; deferred surfaces need defined absent-result
    behavior that still lists planned invocations.

## Consumed approved predecessor revisions (exact)

- **Context (1075):** integrated `9a6d902`; design SHA-256
  `2711043ef216d965bd05c8d8e477f2f0f4d2bca020b936c7f405d06b537565e9`;
  verification `c8c7a7fa8f259edcf138305d08bcf00db2b89efecee22f6f22976633367d17df`.
  Complete retained render program + conservative analysis before admission;
  fixed-five snapshot; seventeen reserved names + collision rule; producer-owned
  `CanonicalTarget`/`StagingError`; origin-bearing `Resolution` vs flat `Seed`;
  record-owned replay, no live read; staged `apply --trust` is hook-only and
  does not elevate the environment snapshot.
- **Includes (1076):** `b87a5482b97e81524272bee1e526698c862754db`. Includes are
  file-body-only, inert text; hook fields are excluded as an unaudited injection
  surface. Capability-by-type (`FileTmpl` vs guarded `Tmpl`/`Expr`); single
  confinement seam; literal-only targets.
- **Confirm-flow (1077):** integrated `d33245065f80e3ed9a8c75ea6526e8a9f9386f21`.
  `Completed::step() -> Step::Plan { apply }`; `Ended{Stop|Abort}` is
  structurally unplannable; dry-run builds/shows the plan but suppresses the
  sole write/hook site; `apply = matches!(step, Plan{apply:true}) &&
  !cli_dry_run`. No trust rule here.
- **Producer / error-attribution:** integrated `8a19269d78f6da0738edb17e56310621f143923d`;
  producer checkpoint `067c8d2e...` / design SHA-256 `72a56479...`. Sole
  `canonical_target` factory returning `StagingError`; `as_path()` only; no
  second normalizer; `TargetError` is stale wording.
- **Hook-review trust (1069):** `docs/design/hook-review-trust/05-design.md`.
  Digest is over the parsed hook-node value (all fields) + executed in-tree file
  bytes; a new field is covered automatically. **Open coverage decision handed
  to 1078:** a new hook field that names a *new* in-tree executable file is not
  byte-covered by the `script:`/literal-`run` rule; 1069 recommends a guard test
  that fails when such a field is added without a coverage decision
  (`05-design.md:352-356`, `290-291`).

## Decisions this design must settle (arena outcome targets)

1. **Identity + opt-in shape.** How a hook declares an id and opts into exposing
   results; result namespace (must not collide with the seventeen or be
   `preset`/`presets`); duplicate-id handling (load error).
2. **Readable surfaces + phase model.** Which surfaces read results (later
   hooks' fields, after-apply) and the phase/ordering model that makes a value
   available before it is read, with static validation. Files/before-apply
   excluded.
3. **Capture scope.** `exit_code` / `stdout` / `stderr`; opt-in per field or
   per hook; decode rule for non-UTF-8 output; trailing-newline handling.
4. **Nonzero interaction.** Whether and how a hook can tolerate a nonzero exit
   for branching — explicit and opt-in, preserving the default stop-and-fail
   with no silent weakening. Missing/unrun/absent result behavior.
5. **Dry-run + stop/abort.** Absent-result behavior that still lists planned
   invocations; full suppression on stop/abort/dry-run.
6. **Trust coverage.** Confirm the new fields carry no new executable-file
   reference, so 1069's parsed-node digest covers them; require the guard test
   that fails if a future hook-result field introduces a new executable-file
   reference without a coverage decision. Introduce no new trust policy.
7. **Compatibility.** Existing hooks (no id, no capture) behave exactly as
   today; pure interview logic and all supported terminal/headless/staged/
   direct/crate paths preserved.

## Searched-source gaps

- The confirm-flow design specifies no trust rule; the trust contract lives in
  `src/apply.rs` (`NeedsTrust`) and 1069. Treated as authoritative from those.
- No existing code captures process stdout/stderr; the decode rule is new and
  must be designed, not inherited.
