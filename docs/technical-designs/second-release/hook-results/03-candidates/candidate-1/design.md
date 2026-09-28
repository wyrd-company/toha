# Candidate 1 — Deferred-render results program (implicit order)

Structural stance: keep the existing linear hook order; split each hook field
and the after-apply message into eager and deferred surfaces at load; the apply
effect loop binds each captured result into one reserved `hooks` map and renders
deferred surfaces just-in-time.

## 1. Caller usage first

Conditional later hook reading an earlier hook's exit code:

```yaml
hooks:
  - id: fmt
    run: [ cargo, fmt, --check ]
    allow-failure: true          # explicit: a nonzero here does not stop the apply
    capture: [ exit_code ]
  - run: [ cargo, fmt ]
    when: "hooks.fmt.exit_code != 0"   # deferred: rendered after fmt runs
```

After-apply message reading captured stdout:

```yaml
hooks:
  - id: version
    run: [ git, describe, --tags ]
    capture: [ stdout ]
messages:
  after-apply: "Tagged {{ hooks.version.stdout | trim }}."   # deferred
```

Crate/driver call site — results bind during apply, not at build:

```rust
// Plan::build stays pure; deferred surfaces are retained, not rendered.
let plan = Plan::build(&template, &completed, &target)?;      // unchanged signature
// apply drives the results program: run hook -> bind result -> render next deferred surface
let applied = plan.apply(&target, options, &runner)?;         // unchanged signature
```

## 2. Data / type sketch

```rust
// template.rs — authored node gains opt-in identity + capture.
pub struct HookNode {
    pub id: Option<Id>,                 // opt-in; None = today's behavior
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    pub capture: CaptureSet,            // empty unless opted in
    pub allow_failure: bool,           // explicit nonzero tolerance; default false
}
#[derive(Default)]
pub struct CaptureSet { pub exit_code: bool, pub stdout: bool, pub stderr: bool }

// hook.rs — outcome carries opt-in captured streams.
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Option<String>,        // Some only when captured
    pub stderr: Option<String>,        // Some only when captured
}

// plan.rs — a hook step carries its retained deferred fields and capture request.
pub struct PlannedHook {
    pub program: DeferredProgram,      // literal args + retained deferred templates
    pub cwd: DeferredCwd,
    pub template_root: PathBuf,
    pub id: Option<Id>,
    pub capture: CaptureSet,
    pub allow_failure: bool,
}

// A single reserved results value threaded through the effect loop.
pub struct HookResults(BTreeMap<String, HookResult>);   // Serialize -> `hooks`
pub struct HookResult { exit_code: i64, stdout: Option<String>, stderr: Option<String> }
```

`RenderedProgram`/`RenderedHook` gain a retained-template variant so a field that
references a result is carried un-rendered into the plan.

## 3. Function signatures (seams)

```rust
// jinja.rs — the reserved results namespace name.
pub const HOOK_RESULTS_NAME: &str = "hooks";

// interview.rs / plan.rs — classify each hook field at build.
fn classify(field: &Tmpl, results_refs: &HashSet<String>) -> Surface; // Eager | Deferred

// load-time validation (template.rs::Template::load): ordering + collision.
fn validate_hook_results(template: &Template) -> Result<(), LoadError>;
//   - each result reference resolves to an id declared earlier in hook order
//   - referenced field is in that hook's CaptureSet
//   - files, path segments, before-apply, interview fields never reference `hooks`
//   - duplicate hook id, or id colliding with the seventeen / question / data /
//     each-binding / `hooks` itself -> aggregate LoadError

// apply.rs — the results program: run, bind, render-next.
impl Plan {
    fn render_deferred(&self, tmpl: &RetainedTmpl, results: &HookResults)
        -> Result<String, ApplyError>;                       // not implemented
    fn bind_result(results: &mut HookResults, id: &Id, capture: &CaptureSet,
        outcome: &HookOutcome);                              // not implemented
}

// hook.rs — runner captures only when asked.
pub trait HookRunner {
    fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>;
}
```

## 4. Module / seam diagram

```
template.rs   Template::load  ── compiles all hook fields into retained program;
              validate_hook_results (ordering, capture-set, surface, collision)
                    │ retained deferred templates + CaptureSet
interview.rs  renders eager interview-hook fields; retains deferred ones
                    │
plan.rs       Plan::build  ── eager fields rendered; deferred retained in PlannedHook
                    │ Plan { hooks: Vec<PlannedHook>, after_apply: Surface }
apply.rs      apply loop (the ONLY interleave):
                for hook in hooks:
                    render hook's deferred when/each/args/cwd vs HookResults
                    runner.run -> HookOutcome
                    if !success && !allow_failure -> stop+fail (unchanged default)
                    bind_result(HookResults, id, capture, outcome)
                render after_apply vs HookResults
hook.rs       ProcessRunner: command.output() when capture non-empty, else status()
```

The interleave lives in exactly one place (`apply.rs`); `HookResults` is the one
owned state. Everything else is load-time classification and validation.

## 5. Error / results contract

- **Nonzero.** Default unchanged: nonzero stops remaining hooks and fails apply.
  Only `allow-failure: true` lets a nonzero hook continue; its result still binds
  (so a later `when` can branch). Reading `.exit_code` never by itself weakens
  stop — the two are independent opt-ins.
- **Absent / unrun / skipped.** A hook whose `when` is false or whose `each` is
  empty never binds a result, so `hooks.<id>` is Jinja `none`; `hooks.<id>.stdout`
  is `undefined`. A hook after a stop never runs. Templates guard with
  `hooks.<id> and hooks.<id>.exit_code == 0`.
- **Duplicate id.** Load error (aggregate, names both locations).
- **Encoding.** Captured stdout/stderr decoded UTF-8 **lossily**
  (`String::from_utf8_lossy`); documented. No trailing-newline stripping — authors
  use `| trim`.
- **Dry-run.** No hooks run, so `HookResults` is empty; deferred surfaces render
  against `none`. The dry-run listing shows each planned invocation with its
  eager args rendered and marks deferred args as "resolved at apply".
- **Stop / abort.** `Ended{Stop|Abort}` never builds a plan; a stopped apply
  renders no further deferred surface and no after-apply. Full suppression.
- **Load ordering / surface.** Forward, self, unknown-id, or wrong-surface
  reference is a load error.

## 6. Trust coverage (1069)

`id`, `capture`, `allow-failure` are parsed hook-node fields → covered by the
existing parsed-node digest automatically; no review-code change. **No new field
names an in-tree executable file** (the executable surface is still
`run`/`script`/`args`), so no new byte-coverage is needed. Require a guard test
in impl 1063 that fails if a future hook-result field introduces a new
executable-file reference without a coverage decision. Hook trust requirements
unchanged; staged `apply --trust` stays hook-only.
