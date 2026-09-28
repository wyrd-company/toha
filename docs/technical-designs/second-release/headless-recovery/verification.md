---
relationships:
  verifies: headless-recovery
  depends-on:
    - confirm-flow
    - error-attribution
---

# Headless recovery design verification

## Basis

- Epic source: `1e663bd661304698ebd10219c8c52d78020a189e`.
- Accepted flow design SHA-256: `54827ce90a185fa24568c79a14707815e225121fbaad7c060154ae17adf5381e`.
- Accepted flow verification SHA-256: `9167bf7a48e3c65aaee4dc1dfb7dbbae62cf8d2188b0d9ff8ffc63bc370f8d52`.
- Integrated error-attribution design SHA-256: `9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b39`.
- Integrated error producer: `8a19269d78f6da0738edb17e56310621f143923d`.
- Arena candidate 1: `27cadb578f57414ad50a602d191eb1ebe9c7166701b599cb8a71ef5d63de363a`.
- Arena candidate 2: `ee0cff26d5a2a7aba831cfdbf35e9a3834db3d3532fd471ae65a2740a223cc2a`.
- Other-family cross-judge: Claude session `84fdcd73-606e-4993-9181-6014e60caf9c`.

## Caller usage against the sketch

| Usage claim | Sketch carrier | Result |
|---|---|---|
| Target-only apply resumes staged state. | Existing route loads `StagedRecord`; `answer_apply` returns `Transition`; `commit_transition` commits it. | Carries. |
| Matching named apply has identical recovery behavior. | The template operand is checked before both spellings enter the same staged transaction. | Carries. |
| Completion applies. | Only `RecoveryOutcome::Completed` carries `Completed`; main retains `Plan::build` and apply. | Carries. |
| Pending exits 4 with summary on terminal and JSON otherwise. | `RecoveryOutcome::Questions` becomes `Outcome::Questions`; `finish` selects one of two representations. | Carries. |
| `stage --async` remains JSON. | It retains `Outcome::Document`/`Saved`; only `Outcome::Questions` tests stdout. | Carries. |
| A wrong template or complete replay does not consume the document. | Identity check and replay precede `read_answers`. | Carries. |

No usage example relies on a public type, option, document field, or state field absent from the sketch.

## Evidence and provenance trace

### Target identity

```text
raw CLI path
  -> staging::canonical_target(&Path) -> CanonicalTarget
  -> Store::load/save/remove(&CanonicalTarget)
  -> protocol::Context::new(&CanonicalTarget, record)
  -> Plan::build(..., &CanonicalTarget)
  -> Plan::apply_reporting(&CanonicalTarget, ...)
```

`StagedTransaction` accepts `&CanonicalTarget`, not `&Path`. No caller can reconstruct or normalize the target inside recovery.

### Configured value origin

```text
ConfigEntry values and source paths
  -> configured_defaults(...)
  -> Resolution { values + origins + warnings }
  -> Resolution::start or StagedRecord::replay_with_resolution
  -> evaluator faults and warnings with original source identity
```

The design passes `Resolution` unchanged before recovery. `Transition`, `commit_transition`, and presentation have no defaults field and no origin reconstruction path.

### Answer transaction

```text
answers file/stdin
  -> read_answers
  -> protocol::parse_answers
  -> RawAnswers
  -> answer_headless (apply) or Pending::answer (continue)
  -> Transition { state, accepted }
  -> copy StagedRecord + append accepted only
  -> commit_transition
```

Rejected input is absent from `accepted`. A persisted submission cannot precede the pure transaction result.

### Question presentation

```text
Batch + Context + effective Rejections
  -> protocol::batch_document (canonical JSON)
  -> QuestionSummary (human projection from the same inputs)
  -> Outcome::Questions
  -> stdout TTY ? summary : existing JSON
```

The terminal renderer does not parse or reinterpret the JSON. The protocol serializer remains the single machine-document authority.

## Accepted flow compatibility

| Accepted result | Recovery handling | Invariant |
|---|---|---|
| `Pending` | Save returned accepted prefix under normal execution; question output. | No plan or target effect. |
| `Completed(Proceed)` | Save accepted prefix for existing record; plan/apply; remove after success. | Only completed values are plannable. |
| `Completed(DryRun)` | Save accepted prefix; build/show plan; retain record. | No target write or hook. |
| `Ended(Stop)` | Save accepted prefix; report ended result. | No completed value, plan, write, or hook. |
| `Ended(Abort)` | Normal execution removes through existing Store; CLI preview performs no mutation. | No plan/write/hook; removal errors surface. |
| Skip | No recovery branch; engine continues with existing default/empty/warning rules. | No second walker or skipped-value policy. |

Confirm is not mentioned in the recovery type sketch because it remains an answer kind. The flow engine, after a confirm value commits, produces the result recovery consumes.

The CLI preview and abort overlap is settled by the existing dry-run promise. Normal execution applies abort's accepted removal effect. Preview reports the same ended outcome without performing the removal, so the command changes nothing.

## Requirement coverage

| Requirement | Design evidence |
|---|---|
| Decide target-only apply answers. | Decision 1, command matrix, usage example, behaviors 1–8. |
| Decide terminal exit-4 presentation while retaining JSON. | Decision 2, presentation table and types, behaviors 16–19. |
| Decide matching-template apply answers. | Decision 3, identity assertion rule, route-parity behaviors 1–3. |
| Cover stop/dry-run/skip/completion. | Recovery outcome table, compatibility matrix, behaviors 10–15 and 22–23. |
| Cover staged/direct/terminal routes. | Command matrix, module map, presentation table, parity proof. |
| Preserve exit 4 and JSON. | `Outcome::Questions` always returns 4; non-terminal uses `batch_document`. |
| Preserve pure interview logic. | Both adapters consume existing engine results; no engine policy or protocol schema changes. |
| Preserve target/provenance contracts. | Field traces above; no raw target or flat defaults cross the seam. |
| No unapproved capabilities. | No removal/restriction, permission/access change, timeout, pin, or subprocess. |

## Failure and persistence verification

The result matrix accounts for every exit after a document can be supplied:

- state/identity/source/replay errors occur before document read;
- parse errors occur before engine submission;
- rejections return original pending for the rejected step;
- staged save errors occur before planning;
- plan/trust/conflict failures retain accepted staged progress;
- file/hook failures use existing partial-target semantics and retain staged progress;
- successful apply removes only after `Applied::Written`;
- cleanup failure is visible after effects and leaves the record;
- stop retains accepted staged progress;
- abort removes under normal execution and surfaces failure;
- CLI preview changes no staged state.

There is no unclassified transition from answers to effects.

## Interface depth and red flags

- **Recovery module:** deep. One `commit_transition` interface hides origin/new state, accepted-prefix persistence, preview policy, every future flow result, and removal behavior.
- **Presentation module:** deep enough because two real adapters vary at the seam and both derive from one source input.
- **Main routing:** remains visible because it owns command selection, trust, planning, and target effects; moving it would relocate established behavior rather than hide the new policy.
- **No shallow pass-through:** the two engine adapters are private result constructors; the design removes candidate wrapper functions that forwarded complete command inputs.
- **No information leakage:** protocol JSON, staged storage, target identity, configured origin, and planning keep one authority each.
- **No temporal decomposition:** recovery groups state-transition knowledge, not load/validate/save phases.

## Arena verification

Two valid candidates are structurally distinct: candidate 1 uses focused recovery and presentation modules; candidate 2 places apply and continue behind one command interface. The candidates share no writable output path. The supplied capsules contain the same task and evidence snapshot and no Git data or scoring material.

The runner sandbox did not enforce filesystem blindness. Complete terminal transcripts show the actual evidence paths: both candidates read capsule task/source/design files and general global authoring skills; neither read repository history, the rubric, judge prompt, scores, task board, worktree, or the other output. `arena-setup.md` records the exact capsule inventory, history boundary, extra reads, process handles, and hashes.

The parent and judge disagreement is reconciled in `synthesis.md`. The final shape closes the judge's two reasons for preferring candidate 2 — shared continue/apply commit policy and completed-only planning — while retaining the parent's narrower ownership seam.

## Canonical-document impact

The proposed CLI and protocol text is internally consistent with the design. The paired implementation owns edits to the command specification, interview protocol, architecture document, and user guides. It introduces no new schema field for recovery; accepted flow document fields remain owned by the flow implementation.

## Verdict

The design can carry its target identity, configured origin, accepted submissions, flow result, persistence intent, canonical question document, and terminal summary without reconstruction or loss. The three product decisions remain pending exact human approval. No implementation dependency may treat publication or this verification as approval.
