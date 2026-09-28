# Rationale — confirm-controlled interview and apply flow

## Problem

A `confirm` answer must be able to drive a control action — stop, dry-run, or
skip — declared as data on the question, while ordinary confirms keep pure
boolean behavior. The shape is non-obvious because the engine has no surface
today where an answer's *value* alters execution: `enum Interview { Asking,
Complete }` has exactly two outcomes, the wire protocol is closed on
`status ∈ {questions, complete}` with `additionalProperties:false`, and a
confirm can share a batch with the questions after it, so an action cannot
assume it runs before its siblings answer. The design must travel through all
five drivers, stay replay-deterministic from stored submissions only, compose
with the approved 1072 atomic-answer-transaction contract and the origin-bearing
`Resolution` producer, and not depend on the unmerged 1056 runtime.

## Usage (caller's view)

See `design.md §1`: a template declares `on: { when_true|when_false: <action> }`
on a confirm, action ∈ `{stop, dry-run, skip-rest, skip-group}`. Three call
sites — crate (exhaustive `match Interview`), terminal (`drive → Conclusion`),
headless (`answer_headless → Headless::Stopped → stopped_document`) — all observe
the outcome on a type the compiler forces them to handle, never in `main.rs`
glue.

## Shape

A third terminal variant, `Interview::Stopped(Stopped)`, produced by `advance`,
the sole result constructor. The load-bearing decision is that **the three
actions map onto the surface differently by their nature**, not uniformly:

- **stop** is a genuine third terminal outcome — an interview that ended and
  *cannot* be planned. It becomes the `Stopped` variant. Making it a variant
  (not a flag on `Completed`) makes "build a plan from a stopped interview"
  unrepresentable — `per encode-lessons-in-structure`.
- **dry-run** is not a terminal state: "normal planning" needs all answers, so
  the interview must finish. It is a plan-only *decision* derived onto
  `Completed.dry_run` and honored at the apply boundary — the same branch the
  existing `--dry-run` flag uses. It is derived from `(recorded answer, template
  mapping)`, single-source, recomputed on replay, never persisted.
- **skip** is not a terminal state either: it is exactly the existing
  `when:false` skip, triggered by a value instead of a static expression. It
  resolves *inside the walk* by reusing the `skip` flag, `render_default`,
  `warn_if_held`, and `warn_unused`. `skip-rest` sets an `Advance.skip_rest`
  field that propagates to all frames; `skip-group` sets a **frame-local**
  `let mut skip_group` that naturally resets when the frame returns to the
  parent — that locality is what scopes it to the enclosing group with no extra
  bookkeeping and no parallel walker.

All three are decided in **one funnel**: the walk's `Node::Question` arm, when a
confirm is active (past `when`) and its answer is recorded, reached in interview
order. This is the single site that sees the question node and the final
boolean. Drivers, protocol, and staging are thin adapters that route the
terminal variant; none re-implements policy — a small, deep interface `per
minimize-reader-load`. Validation is at load (`per boundary-discipline`): the
closed action set is an enum, and structural rules (on-only-on-confirm,
skip-group-needs-a-group) are rejected at load with corrective messages. The
system deliberately does **not** persist any control state, does not add a
second target normalizer, does not enter `plan.rs`/`apply.rs` on stop, and does
not build a plan for `Stopped`.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept **asymmetry** (stop = variant, dry-run = derived field, skip =
  in-walk) in exchange for each action landing where its capability actually
  differs; a uniform "everything is a variant" surface would force callers to
  handle a `DryRun`/`Skip` terminal that can still be planned or still completes.
- We accept a **derived** `Completed.dry_run` recomputed each replay in exchange
  for zero new persisted state and a single source of truth (the recorded
  answer); this is not a missing cache — persisting it would create a second
  source that could drift from the submissions.
- We accept that a confirm **sharing a batch** with later questions fires its
  stop only on the step that submits it (later siblings may already be answered)
  in exchange for honoring the existing batch semantics rather than inventing a
  pre-batch action pass.
- We accept a **new wire status** (`stopped`) and one **optional derived field**
  (`complete.apply: dry-run`) as additive changes to closed schemas, in exchange
  for a machine-readable terminal outcome; both are documented contract additions.
- We accept **exit `0`** for a stop (a deliberate control outcome, not a
  failure), flagged below as an open question for CI detection.

## Alternatives considered

- **Field on `Completed` instead of a variant** (`Completed { stopped: bool }`).
  Smaller surface, but shallow: it exposes a completed interview that must not be
  planned and relies on every caller checking the flag before `Plan::build`. It
  lets the misuse compile. Rejected — the variant hides that hazard behind the
  type. Lost on interface depth.
- **A single `Action` terminal variant carrying `{stop|dry-run|skip}`.** Uniform
  and tidy, but wrong: dry-run and skip are not terminal (planning needs all
  answers; skip continues the interview), so this would either terminate too
  early or carry non-terminal cases in a terminal type. Rejected as
  structurally dishonest.
- **A parallel post-walk reachability pass to apply skips.** Rejected outright:
  the grounding "Avoid" list forbids a second skip walker, and the existing
  `skip` flag already expresses exactly this with `render_default`/`warn_unused`.
- **Inferring the action from prompt text / a magic answer value.** Rejected —
  actions are declared data by inherited constraint; inference is unauditable.

## Open questions and risks

- Should a confirm **stop** exit `0` (deliberate outcome) or a distinct code so
  CI/automation can detect "the template stopped" without parsing output? The
  headless `stopped` document already carries the machine signal; is a distinct
  terminal exit code also wanted for direct/terminal runs?
- Should the `stopped` wire document include the **answers gathered so far**
  (proposed: yes, for visibility) even though a stop applies nothing — or is that
  surprising for a "nothing happened" outcome?
- For dry-run, do we surface `apply: dry-run` on the `complete` document (helps a
  headless agent anticipate) or keep the wire unchanged and let the apply
  boundary re-derive it silently? The design proposes surfacing it; it is the
  one place a zero-wire-change alternative exists.
- Sequencing vs 1056: confirm-flow's engine change can land first; the paired
  impl consumes whichever `Resolution` surface exists, targeting the
  origin-bearing form. Is landing the engine variant ahead of 1056 acceptable at
  Phase C, or should 1062 wait?
- Skip-scope wording in the template guide must be unambiguous: getting
  whole-vs-group wrong silently changes generated output (grounding Risk). Is
  `skip-rest` / `skip-group` clear enough, or should the scope be an explicit
  sub-key (`skip: { scope: rest|group }`)?

## Next implementation step

Add the `Interview::Stopped(Stopped)` variant and the `confirm_action`/`fire`
funnel in `interview.rs::Advance::walk` + `advance`, with `stop` returning
`Ok(false)` and `advance` constructing `Stopped` before its existing
classification — then let the compiler enumerate every driver match site to
update.
