# Candidate 3 — Explicit stages explored; minimal reporting surface

Structural stance: interrogate whether making execution order a first-class
authored concept (named stages) buys a cleaner availability guarantee, and
whether the readable surface should be *broader* (arbitrary later Jinja) or
*narrower* (a single after-all-hooks reporting surface) than Candidates 1–2.
Conclusion up front: explicit stages add authored complexity without a
proportional guarantee; the honest shape is a **narrow** one — results readable
in the after-apply message plus a later hook's `when` gate — with the surface
choice surfaced to Bob as an explicit decision rather than pre-cut.

## 1. Caller usage first

Narrow shape (recommended by this candidate):

```yaml
hooks:
  - id: test
    run: [ make, test ]
    capture: [ exit_code ]
    allow-failure: true
  - run: [ make, coverage ]
    when: "hooks.test.exit_code == 0"    # gate only — no deferred arg rendering
messages:
  after-apply: "Tests {{ 'passed' if hooks.test.exit_code == 0 else 'failed' }}."
```

Explicit-stages shape (explored, not recommended):

```yaml
hooks:
  - id: probe
    stage: check          # a named stage
    run: [ detect-tool ]
    capture: [ stdout ]
  - stage: main           # later stage reads earlier stage's results
    run: [ "{{ hooks.probe.stdout | trim }}", build ]
```

## 2. Data / type sketch

```rust
// Narrow shape: same node additions as Candidate 1 but readable surface is
// restricted to `when` and after-apply; deferred arg/cwd rendering is NOT offered.
pub struct HookNode {
    pub id: Option<Id>,
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,          // MAY read `hooks`
    pub capture: CaptureSet,
    pub allow_failure: bool,
    // pub stage: Option<StageName>, // explicit-stages variant only
}
pub struct HookResults(BTreeMap<String, HookResult>);   // Serialize -> `hooks`
pub struct HookResult { exit_code: i64, stdout: Option<String>, stderr: Option<String> }
```

## 3. Function signatures (seams)

```rust
pub const HOOK_RESULTS_NAME: &str = "hooks";

// Narrow: only two surfaces are deferred — a hook's `when` and after-apply.
// run/args/cwd stay eager (rendered at build), so the plan's invocation listing
// is fully known before apply (better dry-run fidelity).
fn validate_hook_results(template: &Template) -> Result<(), LoadError>;
//   - `when` and after-apply may read `hooks`; run/args/cwd/files/paths/
//     before-apply/interview may not (WrongSurface load error)
//   - `when` reads only earlier hooks' results

impl Plan {
    fn eval_deferred_when(&self, when: &Expr, results: &HookResults) -> Result<bool, ApplyError>;
    fn render_after_apply(&self, tmpl: &Tmpl, results: &HookResults) -> Result<String, ApplyError>;
}
```

## 4. Module / seam diagram

```
template.rs   Template::load ── compile fields; validate_hook_results
                    │  (only `when` + after-apply may read `hooks`)
plan.rs       Plan::build ── run/args/cwd rendered eagerly (full listing known);
              a hook's `when` retained iff it reads `hooks`
apply.rs      loop: eval retained `when` vs HookResults; run; bind; ...;
              render after-apply vs HookResults
hook.rs       ProcessRunner: output() iff capture non-empty
```

## 5. Error / results contract

Same as Candidate 1 for nonzero, absent, duplicate, encoding, stop/abort. The
distinguishing points:

- **Dry-run fidelity.** Because `run`/`args`/`cwd` stay eager, every planned
  invocation's command line is fully known and shown in the dry run; only the
  `when` gate and after-apply depend on results (shown as conditional). This is a
  cleaner dry-run story than deferring arg rendering.
- **Readable-surface decision (surfaced, not pre-cut).** The narrow shape
  restricts result reading to gating (`when`) and reporting (after-apply). A
  later hook cannot interpolate an earlier hook's `stdout` into its own `args`.
  This is a *smaller* new capability than Candidates 1–2 offer. It must be a
  Bob decision, with disclosure that it excludes deferred arg/cwd interpolation.
- **Explicit stages.** If chosen, `stage` names introduce an ordering the author
  must keep consistent with list order; duplicate/forward stage references are
  load errors. Assessed as redundant with list order.

## 6. Trust coverage (1069)

Identical to Candidates 1–2: parsed-node fields auto-covered; no new
executable-file reference; guard test required.
