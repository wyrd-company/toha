# Verification — opt-in Jinja hook results

Verifies the synthesized `design.md` against caller usage, task requirements,
predecessor contracts, failure cases, and compatibility. Each row is falsifiable
by the paired implementation (1063).

## Caller usage traces back to the sketch

| Usage (design §1) | Sketch elements exercised | Holds? |
| --- | --- | --- |
| 1.1 later hook `when: lint.exit_code != 0` | `settle_hook_results` resolves `lint` to an earlier producer; `Planned::AfterHooks` for the two `when`s; apply records `lint`'s `exit_code`; `allow-failure` tolerates nonzero | Yes — reader is later, `allow-failure` is read |
| 1.2 `{{ version.stdout }}` in a later arg and after-apply | `capture: [stdout]` on producer; both readers are allowed surfaces; `render_hooks`/`render_message` render against `HookResults` | Yes — stream read ⇒ must be captured (declared) |
| 1.3 `when: vcs_init.exit_code is none` on a conditional producer | interview hook produces; skipped ⇒ `HookResult::NOT_RUN`; `exit_code is none` guard | Yes — not-run is a value, not undefined |
| 1.4 crate call site unchanged | `Plan::build`/`apply` signatures unchanged; interleave private | Yes — no caller argument added |

## Requirements (task Decisions and scope)

| Requirement | Where satisfied | Falsifiable check |
| --- | --- | --- |
| Define hook ids and opt-in syntax exposing `<id>.exit_code/stdout/stderr` | §1, §2 `HookNode.id`/`capture`; §5 invariant 1 | A hook with `id` + `capture` exposes exactly the three fields; a hook without `id` exposes nothing |
| Choose evaluation phase and later readable surfaces with a real conditional flow | §3 `Planned`/`Deferred`; §5 invariant 2 | Fixture: later hook runs/skips on an earlier hook's `exit_code` |
| Preserve default failed-hook behavior (stop remaining, fail apply) | §5 invariant 5; apply loop | Sole-kill: remove the `allow_failure` guard ⇒ a nonzero un-tolerated hook must still stop (test fails if it continues) |
| Opt-in capture/nonzero interaction without silent weakening | §5 G6/G7; `allow-failure` requires a read | Fixture: `allow-failure` without a read is a load error; capture alone never changes stop |
| No duplicate ids | §5 load errors (existing duplicate rule) | Two hooks with `id: x` ⇒ `duplicate id: x` |
| Missing/unrun results | §5 invariant 3 | Skipped producer ⇒ `exit_code none`; reader guards work |
| Encoding | §5; `decode` strict UTF-8 fatal | Non-UTF-8 ⇒ `ApplyError::HookOutput`, no bytes |
| Dry runs | §5 dry-run | Dry-run runs no hook, lists every invocation with placeholders |
| Preserve trust requirements and script-review coverage | §5 trust; 1069 digest + guard test | Adding a hook-node field changes the digest (1069 field-coverage guard); guard test fails on an unclassified key |
| Do not silently place stdout/stderr in generated files or public output | §5 invariants 4, 7; readable surfaces exclude files/before-apply | A file/before-apply reference to a result is a load error; no Debug/Display carries bytes |
| Namespace/context/include/confirm-flow composition + falsifiable tests | §5 composition; below | See predecessor table |

## Predecessor contract compatibility

| Contract (source) | This design | Verified against |
| --- | --- | --- |
| 1075: only the seventeen reserved names; existing ids not forbidden | `id` is an ordinary authored id; no new reserved name | `design.md:733`, `synthesis.md:100`; sole-kill: an existing template with a hook `id: version` still loads |
| 1075: complete retained program + conservative analysis before admission; engine no ambient read | Result names analyzed at `Template::load`, zero env need; values bound in the apply effect loop | `design.md:242-248`, `:19-20` |
| 1075: `CanonicalTarget`/`StagingError` producer seam | consumed unchanged; no second normalizer, no `TargetError` | `design.md:154-158` |
| 1075: configured `Resolution` vs flat `Seed`; record-owned replay, no live read | untouched; results per-apply, never persisted, never on the wire | `design.md:589-598`, `589` |
| 1075: staged `apply --trust` hook-execution-only | unchanged; results do not touch the environment snapshot | `design.md:546` |
| 1076: includes file-body-only; hook fields not an include surface; capability-by-type | result-reading fields hold guarded `Tmpl`/`Expr`, never `FileTmpl`; no include added | `04-final-design.md:52-56`, `392` |
| 1077: hooks only at the write/hook site; stop/abort unplannable; dry-run suppresses | interleave inside `Plan::apply_reporting`; stop/abort build no plan; dry-run runs nothing | `design.md:161-193`, `315-316` |
| 1069: parsed-node digest covers new fields; open coverage decision | `id`/`capture`/`allow-failure` auto-covered; no new executable-file reference; guard test required | `05-design.md:158-163`, `352-356` |

## Failure-case matrix (falsifiable)

| Case | Expected | Sole-kill |
| --- | --- | --- |
| nonzero, no `allow-failure` | `ApplyError::Hook`; later hooks + after-apply skipped | remove stop guard ⇒ continues ⇒ test fails |
| nonzero, `allow-failure` | tolerated; `exit_code = Some(n)` readable | narrow tolerance to also swallow signals ⇒ signal test fails |
| signal, `allow-failure` | fatal `ApplyError::Hook` | — |
| read stream not captured | load error `does not capture <stream>` | drop the capture check ⇒ runtime `undefined` ⇒ load-error test fails |
| captured stream never read | load error `captured <stream> … never read` | — |
| forward/self reference | load error `read before hook <id> runs` | order the check after own-id registration incorrectly ⇒ self-read passes ⇒ test fails |
| read on file/before-apply/interview/run[0]/each | load error `readable only in …` | admit `run[0]` ⇒ program-from-output test fails the surface check |
| non-UTF-8 capture | `ApplyError::HookOutput`, no bytes | switch to lossy ⇒ byte-free-error test fails |
| deferred `cwd` symlink | `ApplyError::Symlink` at that step | move the check before the loop ⇒ deferred-cwd test fails |
| no `id` (existing hook) | byte-identical to today | add `capture` inference ⇒ stdio changes ⇒ Debug/text test fails |

## Interface-depth check (Ousterhout / red flags)

- One interleave site (`Plan::apply_reporting`); one owned state (`HookResults`,
  `pub(crate)`, dropped at return). No `HookSchedule`-on-`Template` duplication
  (rejected), no public `Surface` enum (private to load), no pass-through
  `bind_result` exposed (rejected). `Planned<T>` hides the eager/deferred split.
- No shallow module, information leakage, temporal decomposition, pass-through,
  or hypothetical seam survives synthesis (see `05-synthesis.md` rejections).

## JSON results mode (§5A) — added checks

| Case | Expected | Sole-kill |
| --- | --- | --- |
| object/array/string/number/bool/null stdout under `parse: json` | `<id>` is the parsed value of that type; `<id>.key` / `<id>[i]` / scalar / `none` | one fixture per type |
| exit 0, empty or malformed stdout | fatal `HookOutput::{Empty,NotJson}`, no bytes | make parse lenient on success ⇒ empty/malformed test fails |
| tolerated nonzero, unparseable stdout | `<id>` `none`, `<status-id>.parsed` false, `<status-id>.stdout` raw text; apply continues | drop the `parsed` flag ⇒ null-vs-unparsed test fails |
| untolerated nonzero with bad JSON | v1 `ApplyError::Hook` (failure checked before parse) | parse before failure check ⇒ wrong error surfaces |
| `<id>.exit_code` on a `parse: json` hook | best-effort load error naming the subscript escape and `status-id` | remove the `GetAttr`/`GetItem` distinction ⇒ migration-mistake test fails; aliased root still degrades to a key read |
| `status-id` omitted when the producer may not run | load error (trigger computed from the retained program) | trigger on "has `when`" only ⇒ group-skipped-producer test fails |
| read through a `none` `<id>` (`<id>.key`) | defined behaviour under minijinja's undefined mode | fixture required in 1063 |
| existing hook / v1 `parse`-free hook | byte-identical to v1 | add `parse: json` inference ⇒ v1 hook changes behaviour |

`serde_json` is already a dependency; no new third-party adoption. Integers above
`u64` become floats (disclosed).

## Open items carried to Phase C

- The three decisions in `design.md:§7` require Bob's ruling before impl 1063
  starts; the recommendation is recorded for each.
- Capture has no size limit; a limit would be a new policy and is explicitly out
  of scope unless Bob asks for one (disclosed).
