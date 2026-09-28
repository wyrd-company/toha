# Synthesis — opt-in Jinja hook results

Inputs: five candidates (three parent-exploration `candidate-1/2/3`; two blind
independent `independent-A`, `independent-B`), the withheld rubric
(`02-arena-rubric.md`), and the independent read-only cross-judge
(`04-cross-judge.md`, a fresh model instance blind to this reasoning).

## Own scoring (reconciled with the cross-judge)

| Candidate | C1 | C2 | C3 | C4 | C5 | C6 | Weighted /70 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| candidate-1 | 2* | 3 | 3 | 2 | 3 | 2 | 36 |
| candidate-2 | 2* | 4 | 4 | 3 | 2 | 2 | 42 |
| candidate-3 | 2* | 4 | 4 | 2 | 3 | 2 | 42 |
| independent-A | 4 | 4 | 3 | 5 | 4 | 4 | 55 |
| independent-B | 4 | 5 | 5 | 5 | 2 | 3 | 59 |

*C1 capped at 2 for the three parent candidates by a real 1075 violation
(below). My scores match the cross-judge's; I independently confirmed the
violation against the approved text and agree with the ranking **B > A >> c2 ≈
c3 > c1**.

## Reconciled disagreement — the decisive finding

The three parent-exploration candidates reserve a new `hooks`/`results`
namespace. The approved context design is explicit: "For current contexts, only
the seventeen exact public names are reserved" (1075 `design.md:733`), and its
synthesis correction exists so the design "does not silently forbid unrelated
existing … identifiers." An unconditional new reserved name breaks that sentence
and turns an existing answer or data id named `hooks`/`results` into a load
error, and it collides with the legacy staged-load available/reserved set. The
grounding line "a result namespace must carry its own reservation rule" is *not*
license to add an unconditional reserved name.

Both blind independent candidates avoided this by making the hook **`id` a bare
identifier in the existing authored-id space**, so the result is read as
`<id>.<field>` with no new reserved name — the existing duplicate rule and the
unchanged seventeen-name rule apply as-is. This is the base.

## Base and grafts

**Base:** `independent-B`'s phase model and trust posture, wearing
`independent-A`'s smaller public surface and runtime contract.

| # | Graft | Source | Reason |
| --- | --- | --- | --- |
| G1 | Hook `id` is a bare identifier in the existing authored-id space; result read as `<id>.<field>`; no new reserved name. | A + B | Removes the 1075 violation and the compatibility break. |
| G2 | Reject `id` together with `each`. | A + B | One value shape per result; a list shape can come later. |
| G3 | Readable in later top-level hook `when`, `run[1..]`, `args`, `cwd`, and `messages.after-apply`. NOT `run[0]`/`script` path, NOT `each`, NOT files/paths/before-apply/interview fields, NOT interview hooks reading. | B | `run[0]`/`each` stay eager → slot count fixed, dry-run lists every invocation, no program derived from output. |
| G4 | Interview hooks may produce a result but never read one. | B (A agrees) | Keeps the pure engine free of results. |
| G5 | Explicit `capture: [stdout, stderr]` on the producer. Load error if a stream is read but not captured, and if a stream is captured but never read. | B / c2 | Keeps the stdio change (terminal→pipe) inside the producer node, so it is in the 1069 parsed-node digest; the two errors keep capture exactly equal to reads. |
| G6 | `allow-failure: true` requires an `id` and requires that the result be read. | A + B | Blocks silent weakening of the default stop-and-fail. |
| G7 | `allow-failure` tolerates a nonzero **exit code only**; a signal (no code) stays fatal. | A | Makes `exit_code is none ⇔ did not run`, so no `.ran`/`.ok` fields are needed. |
| G8 | Result object is exactly `exit_code`, `stdout`, `stderr`. A hook that did not run yields all three `none`, never `undefined`. | A | Matches the task's named surface and the grounding's field list; gives a defined absent value without a heuristic guard rule. |
| G9 | Opaque `Planned<T> = Ready(T) \| AfterHooks(Deferred)`; `Deferred::render_hooks`/`render_message`. `HookResults` is `pub(crate)` and dropped when apply returns. | A | Hides the interleave behind a small surface; smaller than B's `HookStep`/`HookBody`/`DeferredHook`/`AfterApply`/`DeferredMessage` public types. |
| G10 | Manual `Debug` for `HookOutcome`/`PlannedHook`: no captured text; new fields print only when non-default. | A | Existing `ApplyError` text stays byte-identical; no output leaks into public text. |
| G11 | Schema-driven 1069 guard test classifying every hook-node key as executable-referencing or not; fails on any unclassified key. | A + B | Required by 1069's open coverage decision. |
| G12 | Deferred `cwd` symlink check runs just before that hook; a deferred render error is an `ApplyError` carrying the existing `hooks[i].<field>` text. | A + B | Defines the deferred failure modes. |
| G13 | Strict UTF-8 decode; non-UTF-8 output is a fatal `ApplyError` naming stream + `valid_up_to`, carrying no bytes. | B | Decoded text can flow into later argv; lossy replacement would silently corrupt it (violates no-silent-weakening). |
| G14 | Trailing `\r`/`\n` are all removed (shell `$(…)` behavior). | A | Matches author expectation for command output. |
| G15 | The surface-breadth choice is put to Bob as an explicit disclosed decision with a recommendation. | c3 | It is a capability boundary; Bob approves it. |

## Rejections

| Rejected | Source | Reason |
| --- | --- | --- |
| `hooks`/`results` reserved namespace | c1/c2/c3 | Breaks 1075 "only seventeen" and existing ids. |
| Uncaptured field → runtime `undefined` | c1 | Silent author error; replaced by G5 load errors. |
| `HookSchedule` stored on `Template` apart from the plan | c2 | Information leakage; the plan already carries run order. |
| `stage` field | c3 | A second ordering that duplicates list order for no new power. |
| Inferred capture | A | Stdio changes at a distance and outside the 1069 digest; replaced by explicit `capture` (G5). |
| `each`/`run[0]` reading results | A | Dry-run would show hooks that may not run; a program could be built from output. |
| `.ran`/`.ok` fields | B | Redundant once signals are fatal and not-run is `exit_code: none` (G7/G8); also exceeds the task's named `exit_code/stdout/stderr` surface. |
| L13 "conditional producer must be guarded" | B | Heuristic; does not prove a guard; rejects valid templates. |
| L8 "literal field reads only" in full form | B | Needs an analyzer extension; use `undeclared_variables(true)` dotted paths. A whole-value use is bounded already by explicit capture. |
| `Applied::Written.tolerated` + new CLI output line | B | A new public output contract; after-apply is the report channel. Deferred; the must-be-read rule (G6) already blocks silent tolerance. |
| Public 15-variant `Surface` enum, `FlowPosition` | B | Keep private to the load builder. |
| `on-failure: stop\|continue` enum | B | No second real variation → hypothetical seam; use boolean `allow-failure`. |

## Synthesized shape (one paragraph)

A hook opts in by declaring `id` (a bare identifier in the existing authored-id
space) and, when it exposes output, `capture: [stdout, stderr]`. Its result is
read by later surfaces as `<id>.exit_code`, `<id>.stdout`, `<id>.stderr`.
Readable surfaces are later top-level hook `when`/`run[1..]`/`args`/`cwd` and
`messages.after-apply`; every other surface referencing a hook id is a load
error. `Template::load` compiles all fields into the 1075 retained program,
resolves each result reference to an earlier-ordered producer and its captured
fields, and rejects forward/self/unknown/uncaptured/unread references before
admission — the analysis is static and adds zero environment need. `Plan::build`
renders result-free fields as today and retains result-reading fields as
`AfterHooks(Deferred)`. `Plan::apply_reporting` is the one interleave: after
files are written it runs each hook, records `{exit_code, stdout, stderr}` into a
`pub(crate) HookResults` dropped at return, and renders each deferred surface
just before its step. `allow-failure: true` (requiring a read `id`) tolerates a
nonzero exit code only; a signal stays fatal, so `exit_code is none` means the
hook did not run. Non-UTF-8 output is a fatal decode error. Existing hooks (no
`id`) are byte-identical to today.

## Decisions surfaced to Bob (Phase C)

1. **Readable-surface breadth** — recommend BROAD-minus-`run[0]`/`each` (later
   hook `when`/`run[1..]`/`args`/`cwd` + after-apply) over NARROW (`when` +
   after-apply only). Broad meets the stated objective ("pass an earlier hook's
   output to a later one"); narrow forces a `script:` workaround. Disclosure:
   broad lets a producer's output become a later command's argument (the author
   writes it; the text originates from the producer's environment). This bounds
   a **new** capability; it excludes files/before-apply because writes precede
   hooks (output would land in files). No existing capability is removed.
2. **Result names share the answer-id space** — a hook `id` is an ordinary
   identifier; a duplicate against a question/data/computed/group id is the
   existing load error. No new reserved name.
3. **Decode policy** — recommend strict UTF-8 with a fatal, byte-free error.
