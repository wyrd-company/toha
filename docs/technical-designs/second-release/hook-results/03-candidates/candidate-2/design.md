# Candidate 2 — Typed capture bindings + load-time dependency schedule

Structural stance: the capture declaration and a load-time dependency validator
are the deep core. Every result reference is resolved and type-checked at load
against the producing hook's declared capture set, producing a validated
run+render schedule. A reference to an uncaptured field is a load error, not a
runtime `undefined` — opt-in is enforced statically.

## 1. Caller usage first

```yaml
hooks:
  - id: build
    run: [ make, build ]
    capture:
      exit_code: true          # each captured field is declared by name
    on-failure: continue       # explicit; default is `stop`
  - run: [ make, package ]
    when: "results.build.exit_code == 0"
  - id: rev
    run: [ git, rev-parse, HEAD ]
    capture:
      stdout: true
messages:
  after-apply: "Built {{ results.rev.stdout | trim }}"
```

```rust
// Load resolves the schedule; build stays pure; apply consumes the schedule.
let template = Template::load(path)?;                 // validates the schedule
let plan = Plan::build(&template, &completed, &target)?;
let applied = plan.apply(&target, options, &runner)?; // runs schedule, binds typed results
```

## 2. Data / type sketch

```rust
// template.rs
pub struct HookNode {
    pub id: Option<Id>,
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    pub capture: Capture,          // declared field-by-field
    pub on_failure: OnFailure,     // Stop (default) | Continue
}
pub struct Capture { pub exit_code: bool, pub stdout: bool, pub stderr: bool }
pub enum OnFailure { Stop, Continue }

// The load-time artifact: a validated schedule the plan/apply consume.
pub struct HookSchedule { steps: Vec<ScheduledHook> }     // linear, topologically sound
struct ScheduledHook { index: usize, id: Option<Id>, deferred: Vec<DeferredField>, capture: Capture }

// Typed result value.
pub struct HookResult { pub exit_code: i64, pub stdout: Option<String>, pub stderr: Option<String> }
pub struct Results(BTreeMap<String, HookResult>);          // Serialize -> `results`

// hook.rs
pub struct HookOutcome { pub success: bool, pub code: Option<i32>,
    pub stdout: Option<String>, pub stderr: Option<String> }
```

## 3. Function signatures (seams)

```rust
pub const HOOK_RESULTS_NAME: &str = "results";

// The deep core: resolve every reference to (id, field), type-check against the
// producer's Capture, check surface + strict precedence; build the schedule.
fn schedule_hooks(template: &Template) -> Result<HookSchedule, LoadError>;
//   errors: UnknownHookId, UncapturedField, ForwardReference, SelfReference,
//           WrongSurface (file/path/before-apply/interview), DuplicateId,
//           ReservedCollision (seventeen / question / data / each / `results`)

// apply.rs — consume the schedule; render deferred fields as their turn arrives.
impl Plan {
    fn run_scheduled(&self, target: &CanonicalTarget, runner: &dyn HookRunner,
        schedule: &HookSchedule) -> Result<Results, ApplyError>;  // not implemented
}

pub trait HookRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>;
}
```

## 4. Module / seam diagram

```
template.rs   Template::load ── compiles hook fields (retained program);
              schedule_hooks() -> HookSchedule (resolve+typecheck+order)  ◄── deep core
                    │ HookSchedule stored on Template
plan.rs       Plan::build ── carries HookSchedule + retained deferred fields
                    │
apply.rs      run_scheduled(): for step in schedule:
                  render step.deferred vs Results
                  runner.run -> HookOutcome
                  if !success && on_failure==Stop -> stop+fail
                  Results.insert(id, typed HookResult from Capture ∩ outcome)
                render after-apply vs Results
hook.rs       ProcessRunner: output() iff Capture non-empty
```

Depth concentrates in `schedule_hooks`: one function turns authored text into a
proven-safe schedule; `apply` becomes a straight consumer with no ordering logic.

## 5. Error / results contract

- **Nonzero.** Default `on-failure: stop` = today's behavior. `on-failure:
  continue` binds the result and proceeds. Explicit; capture alone never changes
  stop behavior.
- **Absent / unrun.** A skipped/empty-`each` producer binds no result; because
  its consumers' references were validated against a *possibly-absent* producer,
  `results.<id>` is `none` at runtime and templates must guard. (Load proves the
  field *would* exist if the hook runs; it cannot prove the hook runs.)
- **Uncaptured field.** Load error — the distinguishing property. `results.build.stdout`
  when only `exit_code` is captured fails at load with `UncapturedField`.
- **Duplicate id / collision / forward / self / wrong surface.** Load errors.
- **Encoding.** UTF-8 lossy decode; documented. No newline stripping.
- **Dry-run.** No run; `Results` empty; deferred surfaces render against `none`;
  listing shows eager args and marks deferred.
- **Stop / abort.** Structurally unplannable; full suppression.

## 6. Trust coverage (1069)

`id`/`capture`/`on-failure` are parsed-node fields → auto-covered by the digest.
No new executable-file reference; guard test required in impl. Trust unchanged.
