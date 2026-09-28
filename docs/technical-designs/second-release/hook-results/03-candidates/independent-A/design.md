# Hook results — independent candidate A: "the id is the switch"

Shape: a hook opts in with one field, `id`. Its result is then a plain Jinja
variable of that name. Only later top-level hooks and the after-apply message
can read it. Toha captures exactly the streams the template reads. One more
field, `allow-failure`, is the only way to continue past a nonzero exit.

New author vocabulary: two hook fields (`id`, `allow-failure`) and three result
attributes (`exit_code`, `stdout`, `stderr`). No new reserved name, no namespace
object, no capture list, no phase field.

## 1. Caller usage

### 1.1 Later hook runs on an earlier hook's `exit_code`

```yaml
name: sample-service
hooks:
  - id: lint
    run: ["make", "lint"]
    allow-failure: true            # continue on nonzero; lint.exit_code says what happened
  - run: ["make", "format"]
    when: "lint.exit_code != 0"
  - run: ["make", "test"]
    when: "lint.exit_code == 0"
messages:
  after-apply: |
    {% if lint.exit_code == 0 %}Lint passed.{% else %}Lint failed ({{ lint.exit_code }}); ran make format.{% endif %}
```

The template reads only `lint.exit_code`, so the lint output is not captured.
It still goes to the terminal, as today.

### 1.2 After-apply message and a later hook read `stdout`

```yaml
name: sample-library
hooks:
  - id: version
    run: ["git", "describe", "--tags", "--always"]
  - run: ["git", "notes", "add", "-m", "scaffolded at {{ version.stdout }}"]
messages:
  after-apply: "Created at {{ version.stdout }}."
```

`version` has no `allow-failure`, so a nonzero exit stops and fails the apply,
as today. Because the template reads `version.stdout`, Toha captures stdout and
nothing else. Toha does not print the captured text. It appears only where the
template places it.

### 1.3 An interview hook can produce a result

```yaml
interview:
  - hook:
      id: init
      run: ["git", "init", "--quiet"]
hooks:
  - run: ["git", "add", "-A"]
    when: "init.exit_code == 0"
```

An interview hook can declare an `id` but cannot read a result, because the pure
engine renders it. Interview hooks run before every top-level hook, so their
results are readable in every top-level hook and in after-apply.

### 1.4 Crate / driver call site: unchanged

```rust
let template = Template::load(&root)?;                   // ids, readable surfaces, capture decided here
let plan = Plan::build(&template, &completed, &target)?; // surfaces that read no result render here
match plan.apply(&target, ApplyOptions { force: false, trusted: true }, &ProcessRunner)? {
    // Results are bound inside `apply`, between hooks, and never leave it.
    Applied::Written { files, hooks_run, after_apply } => { /* after_apply is final text */ }
    Applied::NeedsTrust(plan) => { /* nothing ran; list plan.hooks */ }
}
```

No call site gains an argument. The interleave is private to `Plan::apply_reporting`.

## 2. Data / type sketch

```rust
// template.rs
#[derive(Debug, Clone)]            // Clone: Tmpl/Expr become Arc-backed (below)
pub struct HookNode {
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    pub id: Option<Id>,            // opt-in identity; never with `each`
    pub allow_failure: bool,       // literal; true only with a read `id`
    pub capture: Capture,          // derived at load from reading surfaces; not authored
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capture { pub stdout: bool, pub stderr: bool }

// interview.rs
#[derive(Debug, Clone)]
pub struct RenderedHook {
    pub program: RenderedProgram,
    pub cwd: Option<String>,
    pub id: Option<Id>,
    pub allow_failure: bool,
    pub capture: Capture,
}

// plan.rs
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<Planned<PlannedHook>>,     // run order
    pub before_apply: Option<String>,         // never reads a result
    pub after_apply: Option<Planned<String>>,
}
pub enum Planned<T> { Ready(T), AfterHooks(Deferred) }
pub struct Deferred { kind: DeferredKind, scope: Arc<DeferredScope> } // opaque
enum DeferredKind { Hook { index: usize, node: HookNode }, AfterApply(Tmpl) }
struct DeferredScope {
    context: BTreeMap<String, serde_json::Value>, // the one plan-time context
    hook_ids: Vec<Id>,                            // every declared id, run order
}
#[derive(Clone)]
pub struct PlannedHook {
    pub program: PlannedProgram,
    pub cwd: Option<TargetPath>,
    pub template_root: PathBuf,
    pub id: Option<Id>,
    pub allow_failure: bool,
    pub capture: Capture,
}
/// Manual Debug: new fields printed only when not default, so the Debug text
/// (and `ApplyError::Hook` Display) of a hook without `id` is unchanged.
impl fmt::Debug for PlannedHook { /* not implemented */ }

// hook.rs
#[derive(Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Option<String>, // None when not captured
    pub stderr: Option<String>,
}
/// Manual Debug: prints only `success` and `code`, never captured text.
impl fmt::Debug for HookOutcome { /* not implemented */ }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct HookResult {
    pub exit_code: Option<i32>,  // null ⇔ hook did not run
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}
impl HookResult {
    pub(crate) const NOT_RUN: Self = Self { exit_code: None, stdout: None, stderr: None };
    pub(crate) fn from_outcome(o: &HookOutcome) -> Self { unimplemented!() }
}
#[derive(Debug, Default)]
pub(crate) struct HookResults(IndexMap<Id, HookResult>); // on apply's stack only
impl HookResults {
    pub(crate) fn record(&mut self, id: &Id, r: HookResult) { unimplemented!() }
    /// context + one entry per declared id: its result or NOT_RUN.
    pub(crate) fn bind(&self, ctx: &BTreeMap<String, Value>, declared: &[Id])
        -> BTreeMap<String, Value> { unimplemented!() }
}
/// Lossy UTF-8; then all trailing '\r' and '\n' removed.
pub(crate) fn decode(bytes: &[u8]) -> String { unimplemented!() }

// apply.rs — one new variant; Applied unchanged; hooks_run = invocations run
pub enum ApplyError { /* existing… */ #[error("{0}")] Deferred(PlanError) }

// jinja.rs
#[derive(Clone)] pub struct Tmpl { inner: Arc<TmplInner> } // same compiled program, shared
#[derive(Clone)] pub struct Expr { inner: Arc<ExprInner> }
// pub(crate) fn attribute_references(&self) -> &HashSet<String>
//   = undeclared_variables(true), recorded at compile beside `references`.
```

## 3. Function signatures

```rust
// template.rs (all inside Template::load, before admission)
fn hook_ids(&mut self, interview: &[Value], hooks: &[Value]) -> IndexMap<Id, HookSite>; // raw pre-pass
fn hook_identity(&mut self, map: &Map<String, Value>, path: &str, each: bool)
    -> Option<(Option<Id>, bool)>;          // rejects id+each, allow-failure without id
fn refs(&mut self, refs: &HashSet<String>, path: &str, local: &[&str]); // + "not readable here"
fn settle_results(&mut self, interview: &mut [Node], hooks: &mut [HookNode],
    after_apply: Option<&Tmpl>);            // sets Capture; rejects unread allow-failure
pub(crate) fn reads_results(node: &HookNode, ids: &HashSet<String>) -> bool;

// plan.rs
impl Plan { pub fn build(template: &Template, completed: &Completed,
    target: &CanonicalTarget) -> Result<Self, PlanError>; } // unchanged signature
impl Deferred {
    pub fn argv(&self) -> Vec<String>;      // source text, for listing
    pub fn cwd(&self) -> Option<&str>;
    pub(crate) fn render_hooks(&self, results: &HookResults, template_root: &Path)
        -> Result<Vec<PlannedHook>, PlanError>;   // when → each → render; reuses render_hooks + plan_hook
    pub(crate) fn render_message(&self, results: &HookResults)
        -> Result<Option<String>, PlanError>;     // blank → None, as today
}

// hook.rs — HookRunner::run signature unchanged.
// ProcessRunner: default Capture → command.status() as today; otherwise stdin
// inherits, each captured stream piped, the other inherits, command.output().
impl RecordingRunner { pub fn with_output(index: usize, code: i32, stdout: &str, stderr: &str) -> Self; }

// apply.rs — apply_reporting signature unchanged.
```

Apply loop after files are written:

```text
results = HookResults::default()
for (index, step) in hooks:
  ready = Ready(h) → [h] | AfterHooks(d) → d.render_hooks(&results)?     // ApplyError::Deferred
  for hook in ready:
    symlink-check hook.cwd                  // Ready cwds are still checked before any write
    outcome = runner.run(hook)?             // HookIo unchanged
    if hook.id: results.record(id, from_outcome(&outcome))
    if !outcome.success && !(hook.allow_failure && outcome.code.is_some()):
        return Err(Hook{index, hook, outcome})
after_apply = Ready(s) → Some(s) | AfterHooks(d) → d.render_message(&results)?
```

## 4. Module / seam diagram

```text
template.rs  Template::load
  raw pre-pass: hook ids + positions ─┐
  compile all Jinja (1075 program)    ├─ refs(): availability per surface position
  id / allow-failure parse            │
  settle_results(): Capture, allow-failure-must-be-read
        │ HookNode{id, allow_failure, capture}  (raw node → 1069 digest)
        ▼
interview.rs  pure engine; interview hooks render here (cannot read results)
        ▼
plan.rs  Plan::build (one context, no results)
  files / file rules / before-apply → Ready only (reading = load error)
  interview hooks → Ready
  top-level hook reads a result? no → Ready | yes → AfterHooks(Deferred)
  after-apply reads a result?    no → Ready | yes → AfterHooks
        ▼
apply.rs  Plan::apply_reporting  ◀── the only interleave
  conflicts → NeedsTrust → write files → [render? → run → record]* → after-apply
        ▲ HookOutcome{code, stdout, stderr}
hook.rs  HookRunner::run (Capture), decode(), HookResults
```

Driver change: `main.rs::plan_lines` only (match on `Planned`). The engine,
staging, replay, resolution, target identity, and context projection do not
change.

## 5. Error / results contract

### Invariants

1. A result is the variable named by the hook's `id`: an object with exactly
   `exit_code`, `stdout`, `stderr`.
2. **Static availability.** A surface can reference id `x` only if it always
   runs after `x`: top-level hooks listed after `x`, or `messages.after-apply`.
   Any other reference is a load error.
3. **"Not run" is a value, never undefined.** Every declared id is bound in
   every reading surface. A skipped, unreached, or skipped-group hook is
   `{exit_code: none, stdout: none, stderr: none}`. `exit_code` is none if and
   only if the hook did not run.
4. **Capture is exactly what is read.** Toha pipes stdout (stderr) if and only
   if some reading surface references `x.stdout` (`x.stderr`). If the template
   uses `x` in any other way, Toha pipes both. A stream Toha does not capture
   inherits the terminal. Toha never prints captured text.
5. **Default failure is unchanged.** A nonzero exit or a signal stops remaining
   hooks and fails the apply. `allow-failure: true` tolerates a nonzero exit
   code only.
6. **Results last for one apply.** They are never stored in `Plan`, `Applied`,
   `ApplyError`, or `StagedRecord`.
7. **No leakage.** No Debug or Display of `HookOutcome`, `PlannedHook`, or
   `ApplyError` contains captured text.
8. A hook without `id` parses, plans, runs, handles stdio, reports errors, and
   appears in dry-run exactly as today.

### Load errors (aggregated `LoadError`)

| Condition | Message |
| --- | --- |
| bad `id` | `invalid identifier` (existing) |
| `id` equals a data, question, computed, group, or hook id | `duplicate id: x` (existing) |
| `id` equals one of the seventeen reserved names | approved 1075 collision message, unchanged |
| `id` with `each` | `a hook with each cannot have an id` |
| `allow-failure` without `id` | `allow-failure requires an id` |
| `allow-failure` not a boolean literal | `wrong literal type: expected boolean` |
| `allow-failure: true`, id never read | `the result of x is never read; read x.exit_code in a later hook or in messages.after-apply` |
| early read (file body/path/rule, before-apply, any interview field, interview hook, own fields, earlier hook) | `x is a hook result and is not readable here; read it in a top-level hook listed after x or in messages.after-apply` |
| `each` binding equals a hook id | `each binding x hides the hook result x` |

### Apply outcomes

| Situation | Result |
| --- | --- |
| nonzero, no `allow-failure` | `ApplyError::Hook`, Display unchanged, no captured text; later hooks and after-apply do not run |
| nonzero, `allow-failure` | recorded; continue |
| signal (no code) | `ApplyError::Hook`, even with `allow-failure` |
| cannot start | `ApplyError::HookIo` (unchanged) |
| reading `when` false at apply | not run; result stays not-run |
| reading surface render fault | `ApplyError::Deferred(PlanError::Render)` with the existing `template error in hooks[i].<field>` text |
| invalid deferred `cwd` | `ApplyError::Deferred(PlanError::Path)` |
| deferred `cwd` symlink | `ApplyError::Symlink` (existing), checked just before that hook |
| non-UTF-8 output | lossy (U+FFFD), never an error |
| trailing newlines | all trailing `\r`/`\n` removed (shell `$(…)`); empty output is `""` |

### Dry-run, trust, stop/abort

- **Dry-run** never calls `apply`, so no hook runs, no result exists, and no
  reading surface renders. `plan_lines` lists a result-reading hook in source
  form: `hook ["make", "test"] cwd . after earlier hook results`. The listing
  includes every hook that might run, and may include hooks that will not.
- **NeedsTrust** still depends on `plan.hooks.is_empty()`. Deferred entries
  count as hooks.
- **Stop/Abort** produce no `Completed`, so no `Plan` and nothing runs.
- **Staged `apply --trust`** applies a fresh plan. Results exist only inside
  that apply, and `StagedRecord` does not change.

### Trust coverage and guard test

- `id` is an identifier and `allow-failure` is a boolean. Neither can name an
  in-tree executable. Both are in the 1069 parsed-node digest by construction.
  `capture` is derived, not authored: it changes where a stream goes, not what
  runs.
- Guard test `tests/hook_fields_coverage.rs`: read the hook properties from
  `template-format.schema.yml`. Assert they equal a table that classifies each
  key as `ExecutableReference` (`run`, `script`) or `NoExecutableReference`
  (`args`, `cwd`, `each`, `when`, `id`, `allow-failure`). An unclassified key
  fails with `classify hook field <key> for trust byte coverage`.

### Canonical doc/schema impact (described, not applied)

- `template-format.schema.yml`: add `id` and `allow-failure` to the top-level
  and interview hook objects. `allow-failure` requires `id`; `id` excludes
  `each`.
- `template-format.yml`: add a "Hook results" paragraph stating invariants 1–5.
- `command-line-interface.yml`: the dry-run and trust listings show the source
  form of a result-reading hook. Toha does not print a stream the template
  reads.
- `docs/template-hooks.md`: add a "Use a hook's result" section using the 1.1
  and 1.2 examples.
- `interview-protocol.yml`: no change.
