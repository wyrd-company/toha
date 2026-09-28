# Rationale — candidate A

## Why this shape

The lens is the simplest possible authoring experience and the smallest public
surface. The common cases are "run B only if A passed" and "show A's output
at the end". Each needs one line on the producer (`id`) and a plain expression
on the reader (`a.exit_code == 0`, `{{ a.stdout }}`). Everything else is
derived from facts Toha already has at load:

- **Order comes from position.** Top-level hooks already run in list order, and
  after interview hooks. A result can be read only by surfaces placed after
  the producer. There is no phase field and no dependency declaration.
- **Capture comes from references.** `undeclared_variables(true)` already tells
  load which attributes a template reads. There is no `capture:` list.
- **Availability reuses the existing check.** `Builder::refs`/`seen` already
  rejects a name used before it is defined. Hook ids enter `seen` at their run
  position. The only extra is a precise message.
- **The interleave is hidden.** No crate signature changes except the
  `Planned<T>` wrapper and one `ApplyError` variant.

## Settled decisions

| Decision | Choice |
| --- | --- |
| Identity / opt-in | `id` on the hook (top-level or interview). It is the only opt-in. |
| Namespace / collision | The bare id, in the existing authored-id space. It uses the existing duplicate-id rule and the unchanged 1075 reserved-seventeen rule. No new reserved name, no `preset(s)`. |
| Readable surfaces | Later top-level hooks (`when`, `each`, `run`, `args`, `cwd`) and `messages.after-apply`. Not files, before-apply, interview fields, or interview hooks. |
| Phase model | A surface that reads a result is `AfterHooks`: it renders in `apply` just before use. All other surfaces render at plan time, as today. Load checks this statically. |
| Capture scope | Per stream, inferred from reads. A stream nobody reads inherits the terminal. |
| Decode | Lossy UTF-8; all trailing `\r`/`\n` removed. |
| Nonzero | Stop and fail by default. `allow-failure: true` needs an id that is read, and tolerates exit codes only, not signals. |
| Absent / unrun | An object whose three fields are `none`. `exit_code` is none if and only if the hook did not run. |
| Dry-run / stop / abort | Apply never runs, so no results exist. Dry-run lists reading hooks in source form. |
| Trust | The new fields name no file. The digest covers them. A schema-driven guard test is required. |
| Compatibility | A hook without `id` is unchanged: its Debug and Display text are pinned by manual Debug impls. |

## Alternatives considered

1. **Namespace object (`hooks.lint.exit_code`, or a `toha_`-style name).**
   Rejected. It adds a new reserved name, which touches the collision contract,
   and makes the common expression longer. A bare id reuses the existing
   duplicate-id rule for free. Cost: hook ids share the namespace with answers.
2. **Explicit `capture: [stdout]` field.** Rejected for the lens. It is one
   more thing to write, and it is redundant with what the template reads. An
   author could also capture a stream and never read it (hidden output for
   nothing) or read one they did not capture (a new error class).
3. **Always capture when `id` is set.** Simpler to explain, but a long `make`
   hook branched only on `exit_code` would go silent in the terminal.
4. **Results as a list for `id` + `each`.** Deferred. Rejecting the pair at load
   keeps the value shape to a single object. Accepting it later is additive.
5. **`allow-failure` without `id`, or with an unread id.** Rejected. A tolerated
   failure that nothing reads would be the silent weakening the brief forbids.
6. **Echo (tee) captured output to the terminal.** Rejected. It needs threads,
   and it puts the text into public output a second time. The template decides
   where the text appears.
7. **Put results in `Applied` for crate callers.** Deferred for the smallest
   surface. The after-apply message is the output channel. Adding it later is
   additive.
8. **Defer all hook rendering to apply.** Rejected. Dry-run and NeedsTrust
   listings would lose rendered argv for every existing hook.

## Tradeoffs and risks

- **Inferred capture (sharpest).** Adding `{{ build.stdout }}` to after-apply
  stops `build`'s stdout from reaching the terminal, with no edit to `build`.
  `after-apply` is not in the 1069 hook digest, so this stdio change does not
  trigger re-approval. The executed surface is unchanged, but a program that
  checks isatty may behave differently when piped. The rule fits in one
  sentence in the docs. The arena should judge whether that is enough.
- **Captured output is hidden on failure.** A failing hook whose stdout is
  captured shows only its exit status. Debugging needs `allow-failure` plus an
  after-apply report, or removing the read.
- **Capture has no size limit.** A limit would be a new policy that needs your
  approval, so none is proposed. Memory use is unbounded for very large output.
- **Lossy decode** can hide binary output. This is acceptable for text use.
- **An `id` of the form `__toha_now`** is a valid `Id`. This gap already exists
  for every authored id and is not introduced here.
- **Deferred render faults** happen after files are written. The same is true
  today for a failing hook.
- **Public-type breaks:** `Plan.hooks` and `Plan.after_apply` get new types, and
  `HookOutcome` and `PlannedHook` get new fields. This is acceptable before v1.
  The only driver affected is `main.rs::plan_lines`.

## Hard-constraint compliance

- **1075:** hook-id references are compiled and checked in `Template::load` as
  part of the complete retained program. Deferred renders use the retained
  compiled `Tmpl`/`Expr` (shared via Arc), never recompiled. The engine does no
  ambient read: results exist only in the effect loop. No new environment
  surface. The seventeen reserved names and the collision rule are untouched.
  No `preset(s)` name. `CanonicalTarget::as_path()` is consumed as today.
  `Resolution` is untouched. Replay is unchanged. Staged `--trust` still only
  authorizes hook execution.
- **1077:** results and deferred renders exist only inside `Plan::apply`, the
  single write/hook site. Stop and Abort cannot produce a plan. Dry-run never
  applies.
- **1076:** the reading fields are guarded `Tmpl`/`Expr`, never `FileTmpl`.
  Hook fields are still not an include surface.
- **1069:** no trust change. The new fields cannot name a file. The guard test
  is specified.
- **Opt-in / no leakage:** without `id`, behavior is identical. Captured text
  goes only to surfaces the author wrote to read it. Error Debug and Display
  exclude it. Nonzero tolerance is explicit and must be read.
- **Design only:** no runtime, spec, or schema edits.

## Synthesis decision
