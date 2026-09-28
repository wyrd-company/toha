# Cross-judge report: opt-in Jinja hook results

Read-only review. No files changed. Weights: C1–C3 ×3, C4–C5 ×2, C6 ×1, so the maximum is 70.

## 1. Scores

| Candidate | C1 Comp | C2 Phase | C3 Opt-in/Trust | C4 Edges | C5 Depth | C6 Compat | **Weighted** |
|---|---|---|---|---|---|---|---|
| candidate-1 (deferred render, `hooks` map) | 2 (cap) | 3 | 3 | 2 | 3 | 2 | **36** |
| candidate-2 (typed capture, load schedule, `results`) | 2 (cap) | 4 | 4 | 3 | 2 | 2 | **42** |
| candidate-3 (narrow: `when` + after-apply) | 2 (cap) | 4 | 4 | 2 | 3 | 2 | **42** |
| independent-A (id is the switch, capture inferred) | 4 | 4 | 3 | 5 | 4 | 4 | **55** |
| independent-B (result-flow table, explicit capture) | 4 | 5 | 5 | 5 | 2 | 3 | **59** |

- **c1:** It has the right single interleave site. But a skipped hook is described as both `none` and `undefined`. `id` together with `each` is undefined, and `each` may read results. It does not say whether signals can be tolerated. A field that was not captured is a silent `undefined`. It adds a reserved name.
- **c2:** It has the strongest static checks of the parent candidates (uncaptured, forward, self). But `HookSchedule` lives on `Template` apart from `Plan` (see section 2). `results` is a common question id, so the compatibility break is larger than c1's.
- **c3:** The narrow surface keeps dry-run fidelity. But its edge contract is "same as c1", so it takes over c1's gaps. Its stages exploration is dead weight, although it correctly rejects stages. The narrow surface misses the stated objective (see section 5).
- **A:** Most complete runtime contract. `allow-failure` tolerates exit codes only; a signal is still fatal. `allow-failure` must be read somewhere. Failed hooks give a value, not `undefined`. Debug output hides captured text. The opaque `Planned<T>` / `Deferred` type is a good deep type. It loses points because inferred capture acts at a distance, and because `run[0]` and `each` may read results.
- **B:** Best availability guarantee. Results sit in fixed slots, `run[0]` and `each` cannot read them, interview hooks can produce but not read, and the capture declaration is inside the 1069 digest. It loses points for a very wide public surface, capture data repeated in five places, and 14 load rules, some of them heuristic.

## 2. Red-flag screen

| Candidate | Findings |
|---|---|
| c1 | **Pass-through:** `Plan::bind_result(results, id, capture, outcome)` is an associated fn that only inserts into a map. `classify()` is exposed as a separate step instead of being hidden inside load. **Info leakage (mild):** the eager/deferred split lives in both `interview.rs` ("retains deferred ones") and `plan.rs`. Whether interview hooks may read results is ambiguous. |
| c2 | **Info leakage:** the ordering and capture data sit in `HookSchedule` (stored on `Template`) and again in `PlannedHook`/`Plan`. `run_scheduled(&self, …, schedule: &HookSchedule)` needs plan and schedule to agree by index. **Hypothetical seam (naming):** the "schedule" is just list order that has been checked. It does no reordering, so it is a validator result dressed up as a scheduler. |
| c3 | **Hypothetical seam:** `stage` (explored and rejected, so harmless in synthesis). Otherwise none. `eval_deferred_when` and `render_after_apply` are small but own real state. |
| A | **Mild temporal coupling:** capture is set at load from surfaces outside the producer node, so the producer's behavior depends on distant text. `Deferred` holds a clone of `HookNode` plus the whole plan context (acceptable). Otherwise none. `HookResults` is `pub(crate)` and the interleave is private. |
| B | **Info leakage:** capture and slot appear in `Exposure`, `ExposureRef`, `ProducerDecl`, `ResultFlowShape`, `PlannedHook.capture` and `DeferredHook.capture`. **Wide public surface:** `Surface` (15 variants), `FlowPosition`, `ProducerSlot`, `ResultReads`, `HookStep`/`HookBody`, `AfterApply`, `DeferredMessage`, `ToleratedFailure`, all public. **Heuristic rule:** L13 "must reference `.ran`" is conservative and does not prove a guard (B says so itself). |

## 3. Load-bearing contract violations

| Candidate | Violation |
|---|---|
| c1, c2, c3 | **YES (1075).** 1075 `jinja-context/design.md:733`: "For current contexts, **only the seventeen exact public names are reserved**." The synthesis correction at `synthesis.md:100` exists "so this design does not silently forbid unrelated existing … identifiers". An unconditional `hooks`/`results` reservation breaks that sentence. It turns existing answer or data ids into load errors, and it clashes with the legacy staged-load rule (`design.md:744-746`). The grounding line "must carry its own reservation rule" does not override the approved text. **C1 capped at 2.** Fixes: use a bare id in the existing id space (A/B), or reserve the name only when a hook declares an `id`. |
| A | None. **Risk:** inferred capture changes a hook's stdio (terminal to pipe) when `messages.after-apply` changes, and that message is outside the 1069 digest. Stdio changes with no re-approval, and the program's behavior can change through `isatty`. This is not a textual violation. |
| B | None. **Risk:** L8 needs the 1075 analyzer to report "literal field read vs other use" (`RootReads`). That changes an approved component's output. It is only acceptable as an additive change. Minijinja may not expose that level of detail; B admits the `undeclared_variables(true)` fallback. |

Pass on all five: dry-run and stop/abort suppression (1077), guarded `Tmpl`/`Expr` and no include surface (1076), digest coverage and a guard test (1069).

Shared risk: c1, c2, A and c3's stage sketch allow `run[0]` to be built from process output. Today's 1069 byte coverage already skips non-literal `run[0]`, so this is not new, but it widens the gap. B bans it.

## 4. Ranked verdict and grafts

**Rank: B > A >> c2 ≈ c3 > c1.** Base the synthesis on B's phase model and trust posture, with A's surface size and runtime contract.

**Graft**

| From | Idea | Reason |
|---|---|---|
| A + B | Hook `id` is a bare Jinja root in the existing authored-id space: the existing duplicate rule and unchanged seventeen rule, no new reservation. | Removes the 1075 violation and the compatibility break. |
| A + B | Reject `id` together with `each`. | Keeps one value shape per result; a list shape can be added later. |
| B | `run[0]`/`script` path and `each` cannot read results; `when`, `run[1..]`, `args`, `cwd` and after-apply can. | Slot count is fixed at plan time, so dry-run lists every invocation, and no program is derived from output. |
| B | Interview hooks can produce results but never read them. | Keeps the engine pure (A agrees). |
| B | Explicit `capture: [stdout, stderr]` on the producer. Error if a stream is read but not captured (B's L10; c2's `UncapturedField`) and if it is captured but never read (L11). | The stdio change lives in the producer node, so it is inside the 1069 digest (see section 5). |
| A + B | The failure opt-in requires an `id` and requires a read (A's "never read" error, B's L12). | Blocks silent weakening. |
| A | The failure opt-in tolerates exit codes only; signals stay fatal. | More conservative, and it makes `exit_code == none` ⇔ "did not run", so `.ran`/`.ok` are not needed. |
| A | The result object has exactly `exit_code`, `stdout`, `stderr`. A hook that did not run gives all three as `none`, never `undefined`. | Matches the grounding's field list and gives a defined absent-result value without B's L13 heuristic. |
| A | Opaque `Planned<T> = Ready(T) \| AfterHooks(Deferred)` with `Deferred::render_hooks` / `render_message`. `HookResults` stays `pub(crate)` and is dropped when apply returns. | A smaller public surface than B's `HookStep`/`HookBody`/`DeferredHook`/`AfterApply`/`DeferredMessage`, hiding the same complexity. |
| A | Manual `Debug` for `HookOutcome`/`PlannedHook`: no captured text; new fields print only when not default. | Existing `ApplyError` text stays byte-identical, and nothing leaks into public output. |
| A + B | Schema-driven 1069 guard test that classifies every hook-node key. | Required by 1069, and it fails on any unclassified key. |
| A + B | A deferred `cwd` symlink check runs just before that hook. A deferred render error is an `ApplyError` with the existing `hooks[i].<field>` text. | Defines these failure modes. |
| B | Strict UTF-8 decode (see section 5). | Values can flow into later argv. |
| c3 | Put the surface-breadth choice to Bob as an explicit decision with a recommendation. | A capability boundary; Bob approves it. |

**Reject**

| From | Idea | Reason |
|---|---|---|
| c1, c2, c3 | `hooks` / `results` namespace. | Breaks the 1075 "only seventeen" rule and existing ids. |
| c1 | Allowing `id` without `capture`/`allow-failure` checks and letting an uncaptured field be `undefined`. | Silent author error. |
| c2 | `HookSchedule` stored on `Template` apart from the plan. | Info leakage; the plan already carries the order. |
| c3 | `stage` field. | A second ordering that repeats list order and adds no power. |
| A | Inferred capture. | Stdio changes at a distance and outside the digest. |
| A | `each` / `run[0]` reading results. | Dry-run shows hooks that may not run, and a program could be built from output. |
| B | `.ran` / `.ok` fields. | Redundant once signals are fatal and "not run" is `exit_code: none`. |
| B | L13 "conditional producer must be guarded". | Heuristic, does not prove a guard, and rejects valid templates. |
| B | L8 "literal field reads only" in its full form (needs the analyzer extension). | Use `undeclared_variables(true)` dotted paths instead. A whole-value use (`tojson(x)`) is fine: explicit capture already bounds what exists. |
| B | `Applied::Written.tolerated` plus a new CLI output line. | A new public output contract. The must-be-read rule already prevents silent tolerance, and after-apply is the report channel; this can be added later. |
| B | 15-variant `Surface` enum and `FlowPosition` as public API. | Make them private to the load builder. |

Naming (low stakes): use `allow-failure: true` (boolean, as in c1, c3, A) rather than `on-failure: stop|continue`. The enum has no second real variation, so it would be a hypothetical seam.

Trailing newline: remove all trailing `\r`/`\n`, like shell `$(…)` (A's rule), because authors expect `$(…)` behavior. B's "strip exactly one" is also acceptable; pick one and put it in the spec.

## 5. Pivotal decisions

**Surface breadth. Recommendation: broad minus `run[0]` and `each` (B's cut), not narrow.**
- Narrow (c3) does not meet the grounding objective, which says "pass an earlier hook's output to a later one". With narrow, that case needs a `script:` workaround. That makes narrow a cut to the stated capability, not a safe first slice.
- c3's real concerns are dry-run fidelity and output-derived programs. B's exclusions answer both:
  - `run[0]` stays eager, so the program is always known before apply and 1069 coverage does not change.
  - `each` stays eager, so the slot count is fixed and dry-run lists every slot, with `<id.field>` placeholders for deferred args.
- Remaining risk to disclose: output passed into `run: [sh, -c, "{{ x.stdout }}"]` is output-to-shell. The author writes it, but the text comes from the producer's environment. Put this in the docs.
- Still a Bob decision. Present it as (a) broad-minus-`run[0]`/`each`, recommended; (b) narrow. Tell him that (b) excludes arg/cwd interpolation.

**Capture. Recommendation: explicit (B / c2), with the read-but-not-captured error and the captured-but-never-read error.**
- The one argument that decides it: capture changes where a stream goes (terminal or pipe). Explicit capture keeps that change in the producer's hook node, so it is in the 1069 parsed-node digest and trust review sees it. Inferred capture (A) lets an edit to after-apply silently change a trusted hook's stdio with no re-approval, and `isatty`-sensitive programs can change behavior.
- The cost is one short list. The two load errors make it exactly equal to the reads, so it can never drift.

**Non-UTF-8. Recommendation: fatal (B), with a documented error that has no bytes (`stream`, `valid_up_to`).**
- Under the broad surface, decoded text can become argv for later commands. Lossy U+FFFD replacement silently corrupts those values, which conflicts with "no silent weakening".
- The cost is an apply failure after files are written. That is the same class as today's hook failure, and B already notes it.
- Only if Bob picks the narrow surface (text goes only to `when` and the message) is lossy acceptable.

**Decisions for Bob:** the surface breadth; that the result names are bare ids in the answer id space (hook ids share it with answers); and the decode policy.

**Assumption:** 1075's "only the seventeen" sentence is load-bearing. The grounding's "must carry its own reservation rule" was read as permission for a rule that does not add an unconditional reserved name.
