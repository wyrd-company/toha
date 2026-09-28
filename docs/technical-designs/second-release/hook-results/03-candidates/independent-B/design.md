# Hook results — candidate independent-B: statically checked result flow

Shape: a hook opts in with an `id`. Its id joins the template's single
authored-id space and becomes a **Jinja root** that later surfaces read as
`<id>.<field>`. `Template::load` builds a **result-flow table** (producers,
their capture and failure mode, and every consumer read resolved to a producer
slot) and rejects every author mistake that can be decided from the text.
Apply fills a fixed-size results table in hook order. A consumer renders only
against producers whose slot is below its own position.

## 1. Caller usage

### 1.1 Conditional later hook reading an earlier hook's `exit_code`

```yaml
hooks:
  - id: check
    run: [ lint-tool, --check, . ]
    on-failure: continue          # explicit opt-in: nonzero does not stop apply
  - run: [ lint-tool, --fix, . ]
    when: "not check.ok"          # deferred: evaluated after `check` runs
  - run: [ report-tool, --code, "{{ check.exit_code }}" ]
```

`check` exits nonzero → apply continues (declared), the fix hook runs, and
`Applied.tolerated` lists `check`. With the default `on-failure: stop`, a
nonzero `check` stops and fails the apply as today.

### 1.2 After-apply message reading `stdout`

```yaml
hooks:
  - id: version
    run: [ tool, --version ]
    capture: [ stdout ]           # piped to Toha, not shown on the terminal
messages:
  after-apply: |
    Generated with {{ version.stdout }}.
```

`version` is unconditional and in stop mode, so at every consumer `ran` and
`ok` are statically true. No guard is required.

### 1.3 Conditional producer — guard required

```yaml
interview:
  - id: use_vcs
    type: confirm
    prompt: Initialize version control?
  - hook:
      id: vcs_init
      run: [ vcs, init ]
      capture: [ stdout, stderr ]
    when: use_vcs
hooks:
  - run: [ note-tool, "{{ vcs_init.stdout }}" ]
    when: vcs_init.ran            # required (L13): producer may be skipped
messages:
  after-apply: "{% if vcs_init.ran %}{{ vcs_init.stderr }}{% endif %}"
```

### 1.4 Load errors an author sees (aggregated `LoadError.problems`)

```text
hooks[1].run[1]: hook result read before hook `check` runs: check
messages.before-apply: hook results are readable only in top-level hook when/run[1..]/args/cwd and messages.after-apply: version
hooks[2].run[1]: hook `version` does not capture stderr; add `capture: [stderr]`
hooks[0].capture: captured stderr of hook `version` is never read
hooks[0].on-failure: failure of hook `check` is tolerated but never checked; read `check.ok` or `check.exit_code`
hooks[3].args[0]: read hook results as `<id>.<field>`: vcs_init
```

### 1.5 Driver call site — results are bound inside apply

```rust
// src/main.rs — call shape unchanged; the interleave is inside apply.
match plan.apply_reporting(
    &target,
    ApplyOptions { force, trusted: registry_trusted || trust },
    &ProcessRunner,
    &mut |file| println!("{file}"),
) {
    Ok(Applied::Written { after_apply, tolerated, .. }) => {
        // after_apply was rendered after the last hook, against the results.
        // tolerated carries ids and exit codes only — never output bytes.
        let lines = tolerated.iter().map(ToString::to_string)
            .chain(after_apply)
            .collect::<Vec<_>>();
        Outcome::Written(lines)
    }
    Ok(Applied::NeedsTrust(_)) => { /* unchanged */ }
    Err(error) => Outcome::Error(error.to_string()), // Display never holds output
}
```

Results exist only inside `Plan::apply_reporting`; no caller handles a results map.

## 2. Data / type sketch

### 2.1 Authored node (`src/template.rs`)

```rust
pub struct HookNode {
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    /// `None` = today's hook, exactly.
    pub exposes: Option<Exposure>,
    /// Result reads of every field of this node, resolved at load. Empty →
    /// the node renders at plan time as today.
    pub reads: ResultReads,
}
/// Constructed only by the load builder; its invariants hold for every value.
pub struct Exposure {
    id: Id,                 // single authored-id space
    slot: ProducerSlot,     // execution-order position in the flow table
    capture: Capture,
    on_failure: OnFailure,
    conditional: bool,      // own `when` or any enclosing group `when`
}
impl Exposure {
    pub fn id(&self) -> &Id { unimplemented!() }
    pub fn slot(&self) -> ProducerSlot { unimplemented!() }
    pub fn capture(&self) -> Capture { unimplemented!() }
    pub fn on_failure(&self) -> OnFailure { unimplemented!() }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capture { pub stdout: bool, pub stderr: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnFailure { #[default] Stop, Continue }
/// Dense, private constructor; exists only for declared producers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProducerSlot(u32);

#[derive(Debug, Default)]
pub struct ResultReads {
    by_field: Vec<(FieldPath, BTreeMap<ProducerSlot, FieldSet>)>,
}
impl ResultReads {
    pub fn is_empty(&self) -> bool { unimplemented!() }
    /// All strictly before the reading node (load invariant).
    pub fn producers(&self) -> impl Iterator<Item = ProducerSlot> + '_ { std::iter::empty() }
    pub fn reads_in_when(&self) -> bool { unimplemented!() }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultField { Ran, Ok, ExitCode, Stdout, Stderr }
pub type FieldSet = u8; // bitset of ResultField

/// Built once at load, execution order: interview hooks then top-level hooks.
pub struct ResultFlow {
    producers: Vec<ProducerDecl>,   // indexed by ProducerSlot
    after_apply_reads: ResultReads,
}
pub struct ProducerDecl { id: Id, capture: Capture, on_failure: OnFailure, conditional: bool }

pub struct Template { /* existing fields */ pub result_flow: ResultFlow }
```

`HookProgram`, `HookCommand`, `Each`, `Tmpl`, `Expr` are unchanged.
Result-reading fields stay guarded `Tmpl`/`Expr`; never `FileTmpl`.

### 2.2 Rendered (`src/interview.rs`)

```rust
pub struct RenderedHook {
    pub program: RenderedProgram,
    pub cwd: Option<String>,
    pub exposes: Option<ExposureRef>, // copied static data, not an engine read
}
#[derive(Debug, Clone, Copy)]
pub struct ExposureRef { pub slot: ProducerSlot, pub capture: Capture, pub on_failure: OnFailure }
```

Interview hooks can produce but never read (L6), so the engine renders them
as today.

### 2.3 Planned (`src/plan.rs`)

```rust
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<HookStep>,             // was Vec<PlannedHook>
    pub before_apply: Option<String>,
    pub after_apply: Option<AfterApply>,  // was Option<String>
    result_flow: ResultFlowShape,         // producer count + capture per slot
}
/// Slot count is fixed at plan build: results decide whether a slot runs and
/// fill its arguments, never how many slots exist (`each` cannot read results).
#[derive(Debug, Clone)]
pub struct HookStep { pub exposes: Option<ExposureRef>, pub body: HookBody }
#[derive(Debug, Clone)]
pub enum HookBody { Ready(PlannedHook), Deferred(DeferredHook) }

/// The concrete invocation — the only thing a runner receives.
#[derive(Debug, Clone)]
pub struct PlannedHook {
    pub program: PlannedProgram,
    pub cwd: Option<TargetPath>,
    pub template_root: PathBuf,
    pub capture: Capture,
}
/// Holds plan-time context (answers, data, now, 1075 snapshot projection,
/// each item) — no live read — and cloned compiled fields.
#[derive(Debug, Clone)]
pub struct DeferredHook {
    fields: DeferredFields,               // compiled when/run[1..]/args/cwd
    base: BTreeMap<String, serde_json::Value>,
    program_name: String,                 // run[0] or script path: never deferred
    template_root: PathBuf,
    capture: Capture,
    reads: ResultReads,
}
#[derive(Debug, Clone)]
pub enum AfterApply { Ready(String), Deferred(DeferredMessage) }
```

### 2.4 Outcome and result value (`src/hook.rs`, new `src/hook_result.rs`)

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Option<Vec<u8>>, // Some iff requested
    pub stderr: Option<Vec<u8>>,
}
impl std::fmt::Debug for HookOutcome { /* success, code, byte lengths only */ }

#[derive(Clone, PartialEq, Eq)]
pub enum HookResult {
    /// Plan-time when false, group skipped, node unreached, deferred when false.
    Skipped,
    Ran { ok: bool, exit_code: Option<i32>, stdout: Option<String>, stderr: Option<String> },
}
/// One slot per declared producer, all Skipped until written; each written once.
pub struct Results { slots: Vec<HookResult> }
impl Results {
    pub(crate) fn new(shape: &ResultFlowShape) -> Self { unimplemented!() }
    pub(crate) fn record(&mut self, slot: ProducerSlot, result: HookResult) { unimplemented!() }
    /// Roots for exactly the producers `reads` names; only captured fields.
    pub(crate) fn view(&self, flow: &ResultFlowShape, reads: &ResultReads)
        -> BTreeMap<String, serde_json::Value> { unimplemented!() }
}
// Jinja value: {ran, ok, exit_code: int|none, stdout?: str|none, stderr?: str|none}
// Skipped → ran=false, ok=false, others none.
pub enum Stream { Stdout, Stderr }
/// Strict UTF-8, then remove exactly one trailing "\n" or "\r\n".
pub(crate) fn decode(stream: Stream, bytes: Vec<u8>) -> Result<String, DecodeError> { unimplemented!() }
pub struct DecodeError { pub stream: Stream, pub valid_up_to: usize }
```

### 2.5 Apply result (`src/apply.rs`)

```rust
pub enum Applied {
    Written { files: Vec<TargetPath>, hooks_run: usize, after_apply: Option<String>,
              tolerated: Vec<ToleratedFailure> },
    NeedsTrust(Plan),
}
pub struct ToleratedFailure { pub id: Id, pub exit_code: Option<i32> }
// Display: "hook `check` exited with 1; continued (on-failure: continue)"
```

## 3. Function signatures

```rust
// template.rs — load
impl Builder {
    /// id via Builder::id (dup + 1075 reserved rule) and into answer_ids; L3, L4.
    fn exposure(&mut self, map: &Map<String, Value>, path: &str,
                has_each: bool, conditional: bool) -> Option<Exposure>;
    /// From the 1075 retained-program analysis output (RootReads): literal
    /// field reads of hook-id roots, or L6/L7/L8/L9/L10/L14.
    fn result_reads(&mut self, refs: &RootReads, surface: Surface, path: &str,
                    position: FlowPosition) -> BTreeMap<ProducerSlot, FieldSet>;
}
/// accepts_results(): TopHookWhen, TopHookRunArg(>=1), TopHookScriptArg, TopHookCwd, AfterApply.
pub enum Surface { InterviewAny, SourcePath, SourceBody, FileRulePath, FileRuleBody,
    FileRuleWhen, FileRuleEach, BeforeApply, TopHookEach, TopHookProgram,
    TopHookWhen, TopHookRunArg, TopHookScriptArg, TopHookCwd, AfterApply }
pub struct FlowPosition { visible_below: ProducerSlot }
/// Whole-template: L11, L12, L13.
fn check_flow(flow: &ResultFlow, consumers: &[(&str, &ResultReads)], ps: &mut Vec<Problem>);
```

Load order: interview (register producers; reject any read), files, top-level
hooks in list order (resolve reads against producers so far, *then* register
own id → self-read is L7), messages, `check_flow`. `RootReads` is the 1075
analyzer's output extended to classify each use of a root as literal field
read or other use — not a second analyzer.

```rust
// plan.rs
impl Plan { pub fn build(template: &Template, completed: &Completed, target: &CanonicalTarget)
    -> Result<Self, PlanError>; } // unchanged signature
fn plan_hook(hook: &RenderedHook, template: &Template) -> Result<PlannedHook, PlanError>;
fn defer_hook(node: &HookNode, index: usize, local: BTreeMap<String, Value>, template: &Template)
    -> Result<DeferredHook, PlanError>;
impl HookStep { pub fn preview(&self) -> Vec<String>; } // `<id.field>` placeholders + deferred `when` source

// apply.rs
impl DeferredHook {
    /// Ok(None) = deferred when false. Parses cwd, checks symlink.
    pub(crate) fn resolve(&self, results: &Results, flow: &ResultFlowShape,
        target: &CanonicalTarget) -> Result<Option<PlannedHook>, ApplyError>;
}
impl DeferredMessage {
    pub(crate) fn render(&self, results: &Results, flow: &ResultFlowShape) -> Result<String, ApplyError>;
}
impl Plan {
    /// Unchanged signature. Per step: resolve → run → conform capture → decode →
    /// record → Stop: Err(Hook) | Continue: tolerated. Then render after-apply.
    pub fn apply_reporting(self, target: &CanonicalTarget, options: ApplyOptions,
        runner: &dyn HookRunner, on_written: &mut dyn FnMut(&TargetPath))
        -> Result<Applied, ApplyError>;
}

// hook.rs — trait signature unchanged; contract: stdout.is_some() == hook.capture.stdout (same for stderr).
pub trait HookRunner { fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>; }
```

`ProcessRunner`: no capture → `status()` as today. Any capture → stdin
inherited, captured streams `Stdio::piped()`, others inherited,
`spawn()?.wait_with_output()?`. `RecordingRunner` gains scripted outcomes.

## 4. Module / seam diagram

```text
template.yml ─ parse, !include, schema (id/capture/on-failure literals)
    ▼
template.rs Builder: Builder::id (one id space; dup + 1075 reserved) · exposure (L3,L4)
    · 1075 retained-program analysis → RootReads · result_reads (L6–L10,L14) · check_flow (L11–L13)
    ▼
Template { hooks: [HookNode{exposes, reads}], result_flow }
    ├─► interview.rs (pure engine): renders interview hooks as today; copies ExposureRef
    ▼
plan.rs Plan::build   ← dry-run ends here; Stop/Abort never reach it
    reads empty → HookBody::Ready · else → HookBody::Deferred · AfterApply Ready|Deferred
    ▼
apply.rs Plan::apply_reporting   ← the only interleave site
    conflicts → trust gate (NeedsTrust unchanged) → symlinks → write files
    for step: resolve → hook.rs run (capture) → hook_result.rs decode/record → Stop|Continue
    render deferred after-apply
    ▼
Applied::Written { after_apply, tolerated }   (Results dropped)
```

Ownership: `template.rs` declaration + all static rules; `hook_result.rs`
value, decode, `Results`; `plan.rs` Ready/Deferred split + preview;
`apply.rs` only site where results exist; `hook.rs` capture. `staging.rs`
untouched.

## 5. Error / results contract

### 5.1 Load (aggregated, authored path)

| Rule | Condition | Message |
| --- | --- | --- |
| L1 | invalid id, or equal to any authored id (data, question, computed, group, hook) | existing `invalid identifier` / `duplicate id: <id>` |
| L2 | id is one of the seventeen reserved names | the 1075 message, unchanged |
| L3 | `id` with `each` | `a hook with each cannot declare id` |
| L4 | `capture`/`on-failure` without `id` | `capture requires id` / `on-failure requires id` |
| L5 | bad capture item/duplicate/empty; bad on-failure | schema error |
| L6 | hook-id read in interview, source path/body, file rule, before-apply, top-level `each`, `run[0]` | `hook results are readable only in top-level hook when/run[1..]/args/cwd and messages.after-apply: <id>` |
| L7 | read of a producer at or after the consumer (incl. self) | `hook result read before hook \`<id>\` runs: <id>` |
| L8 | any root use but a literal field read (bare, `is defined`, dynamic subscript, alias, whole-value filter/function arg, loop) | `read hook results as \`<id>.<field>\`: <id>` |
| L9 | field not in ran/ok/exit_code/stdout/stderr | `unknown hook result field: <id>.<field>` |
| L10 | stream read but not captured | `hook \`<id>\` does not capture <s>; add \`capture: [<s>]\`` |
| L11 | captured stream never read | `captured <s> of hook \`<id>\` is never read` |
| L12 | `continue` and no read of ok/exit_code | `failure of hook \`<id>\` is tolerated but never checked` |
| L13 | conditional producer; consumer reads exit_code/stdout/stderr with no ran/ok ref in that node | `hook \`<id>\` may not run; reference \`<id>.ran\` or \`<id>.ok\`` |
| L14 | local binding named as a hook id | `local name shadows hook result: <id>` |

An unread `id` with no capture and stop mode is legal.

### 5.2 Runtime

| Situation | Behavior |
| --- | --- |
| stop mode, nonzero/signal | `ApplyError::Hook{index,id,hook,outcome}`; later hooks and after-apply skipped — as today |
| continue mode, nonzero/signal | `ok=false`, `exit_code` (none on signal); continue; `tolerated` entry |
| no `id` | exactly today |
| spawn failure | `ApplyError::HookIo`, always fatal |
| non-UTF-8 capture | `ApplyError::HookOutput{index,id,stream,valid_up_to}`, always fatal, no bytes |
| runner capture contract broken | `HookError::CaptureContract` |
| deferred render error | `ApplyError::Render{index,field,expression,message}`; stops |
| deferred cwd invalid/symlink | `ApplyError::Path`/`Symlink` at that slot before running |
| deferred when false | slot skipped; producer `Skipped`; not in `hooks_run` |
| producer skipped at plan / unreached | `Skipped`: ran=false, ok=false, others none (render empty) |
| deferred after-apply error | `ApplyError::Render{field:"messages.after-apply"}` |

Capture: `ran`/`ok`/`exit_code` always for an `id`; streams only if listed.
Captured streams do not reach the terminal. Leakage: only surfaces that name a
field receive it; no Debug/Display/`Applied` carries bytes; results dropped at
return.

### 5.3 Dry-run, trust, stop/abort, replay

Dry-run: builds plan, runs nothing; Ready slots as today; Deferred slots with
`<id.field>` placeholders and deferred `when` source; after-apply not shown.
Untrusted → `NeedsTrust(plan)` before writes, same preview. Stop/abort: no
`Completed` → no `Plan` → unreachable. Staging: `StagedRecord` unchanged;
results per-apply; staged `apply --trust` hook-execution-only.

### 5.4 Trust

`id`, `capture`, `on-failure` name no file and cannot name a new in-tree
executable; the 1069 parsed-node digest covers them. Result reads are banned
in `run[0]`, so no program is derived from output. Guard test
`tests/hook_node_fields.rs`: read `hook-command`/`hook-node` property names
from the schema, compare to a table classifying each as `ExecutesFile`
(`script`, literal `run[0]`) or `NoFileReference` (`args`, `cwd`, `when`,
`each`, `id`, `capture`, `on-failure`, `run[1..]`); fail on any
unclassified key, citing the 1069 coverage decision.

### 5.5 Compatibility and canonical impact (described, not applied)

No `id` → empty flow, all Ready, `status()` runner, identical output. Crate
API changes (pre-v1): `Plan.hooks: Vec<HookStep>`, `Plan.after_apply:
Option<AfterApply>`, `PlannedHook.capture`, `HookOutcome.stdout/stderr`,
`Applied::Written.tolerated`. Schema: `hook-command` gains `id` (Id pattern),
`capture` (enum array, unique, min 1), `on-failure` (enum stop/continue,
default stop). `docs/template-hooks.md`: section "Use a hook's result". Apply
output contract: one line per tolerated failure before after-apply.
