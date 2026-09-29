# Design — opt-in Jinja hook results

Task 1078 (design), paired implementation 1063, parent epic 1065. Base epic
`9a6d902`. This is the synthesized design; see `01-grounding.md`,
`03-candidates/`, `04-cross-judge.md`, and `05-synthesis.md` for how it was
reached, and `verification.md` for the falsifiable checks.

## Objective

Expose, opt-in, a finished hook's `exit_code`, `stdout`, and `stderr` to later
Jinja surfaces, so a template can run a later hook conditionally on an earlier
one, pass an earlier hook's output to a later one, or report a result in the
closing message — without changing hook trust and without ever silently placing
command output into generated files or public output.

## 1. Caller usage

### 1.1 A later hook branches on an earlier hook's exit code

```yaml
hooks:
  - id: lint
    run: [ lint-tool, --check, . ]
    allow-failure: true            # a nonzero exit is tolerated; it is read below
  - run: [ lint-tool, --fix, . ]
    when: "lint.exit_code != 0"    # deferred: evaluated after `lint` runs
  - run: [ test-tool ]
    when: "lint.exit_code == 0"
```

`lint` reads no result, so `allow-failure` requires that *something* reads
`lint`'s result — the two `when` expressions do. Without `allow-failure`, a
nonzero `lint` stops and fails the apply, exactly as today.

### 1.2 Pass output to a later hook and to the closing message

```yaml
hooks:
  - id: version
    run: [ git, describe, --tags, --always ]
    capture: [ stdout ]            # piped to Toha, not shown on the terminal
  - run: [ git, notes, add, -m, "scaffolded at {{ version.stdout }}" ]
messages:
  after-apply: "Created at {{ version.stdout }}."
```

`version` is unconditional and in the default stop mode, so at every reader its
`exit_code` is statically `0` when it is read. Because `version.stdout` is read,
`version` must declare `capture: [stdout]`; that declaration lives in the hook
node, so trust review sees it.

### 1.3 A conditional producer; the reader guards on "did it run"

```yaml
interview:
  - id: use_vcs
    type: confirm
    prompt: Initialize version control?
  - hook:
      id: vcs_init
      run: [ vcs, init ]
    when: use_vcs                  # an interview hook: it may produce, never read
hooks:
  - run: [ note-tool, "{{ vcs_init.stdout }}" ]
    when: "vcs_init.exit_code is not none"   # none ⇔ vcs_init did not run
    capture: [ stdout ]            # declared on vcs_init below in real YAML
```

`vcs_init.exit_code is none` means the hook did not run (its `when` was false).
`is not none` is the guard; `== 0` means it ran and succeeded.

### 1.4 Crate / driver call site — unchanged

```rust
let template = Template::load(&root)?;                    // ids, surfaces, capture settled here
let plan = Plan::build(&template, &completed, &target)?;  // result-free surfaces render here
match plan.apply(&target, ApplyOptions { force, trusted }, &ProcessRunner)? {
    Applied::Written { files, hooks_run, after_apply } => { /* after_apply is final text */ }
    Applied::NeedsTrust(plan) => { /* nothing ran; list plan.hooks */ }
}
```

No call site gains an argument. The interleave is private to
`Plan::apply_reporting`; results never leave it.

## 2. Data / type sketch

```rust
// template.rs — authored node gains opt-in identity and capture.
pub struct HookNode {
    pub command: HookCommand,
    pub each: Option<Each>,
    pub when: Option<Expr>,
    pub id: Option<Id>,            // opt-in; None = today's hook exactly
    pub capture: Capture,          // empty unless a stream is read
    pub allow_failure: bool,       // literal; true only with a read id
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Capture { pub stdout: bool, pub stderr: bool }

// hook.rs — outcome carries opt-in raw captured bytes; manual Debug hides them.
pub struct HookOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Option<Vec<u8>>,   // raw bytes; Some iff Capture.stdout
    pub stderr: Option<Vec<u8>>,   // apply strict-decodes after the failure gate
}
impl std::fmt::Debug for HookOutcome { /* success, code only — never bytes */ }

// hook.rs — the result value a reader sees, and the per-apply table.
#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct HookResult {
    pub exit_code: Option<i32>,    // none ⇔ did not run
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}
impl HookResult { pub(crate) const NOT_RUN: Self = /* all none */ unimplemented!(); }
#[derive(Default)]
pub(crate) struct HookResults(IndexMap<Id, HookResult>);  // on apply's stack only

// plan.rs — a surface is Ready (rendered at build) or AfterHooks (retained).
pub enum Planned<T> { Ready(T), AfterHooks(Deferred) }
pub struct Deferred { /* opaque: compiled fields + one plan-time context + declared ids */ }
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<Planned<PlannedHook>>,      // run order
    pub before_apply: Option<String>,          // never reads a result
    pub after_apply: Option<Planned<String>>,
}
#[derive(Clone)]
pub struct PlannedHook {
    pub program: PlannedProgram, pub cwd: Option<TargetPath>, pub template_root: PathBuf,
    pub id: Option<Id>, pub capture: Capture, pub allow_failure: bool,
}
impl std::fmt::Debug for PlannedHook { /* new fields print only when non-default */ }
```

`HookProgram`/`HookCommand`/`Each`/`Tmpl`/`Expr` are unchanged; result-reading
fields stay guarded `Tmpl`/`Expr`, never `FileTmpl` (1076). `StagedRecord` is
unchanged: results are per-apply and never persisted.

## 3. Function signatures (seams)

```rust
// template.rs — all inside Template::load, before admission (no new reserved name).
fn settle_hook_results(template: &mut Template) -> Result<(), LoadError>;
//   using the 1075 retained-program reference analysis:
//   - resolve each `<id>.<field>` read to a producer ordered strictly earlier
//   - a producer is an interview hook (any) or a top-level hook listed before
//   - readable surfaces: top-level hook when / run[1..] / args / cwd, and
//     messages.after-apply. Any other reference (files, paths, before-apply,
//     interview fields, interview-hook fields, run[0]/script path, each) errors
//   - a read stream must be captured; a captured stream must be read
//   - allow-failure requires an id whose result is read
//   - id joins the authored-id space: duplicate / seventeen-name collision are
//     the existing errors, unchanged
pub(crate) fn reads_results(node: &HookNode) -> bool;   // → Ready vs Deferred

// plan.rs — build keeps its signature; deferred surfaces are retained.
impl Deferred {
    pub fn preview(&self) -> Vec<String>;               // source form + `<id.field>` placeholders
    pub(crate) fn render_hooks(&self, results: &HookResults) -> Result<Vec<PlannedHook>, PlanError>;
    pub(crate) fn render_message(&self, results: &HookResults) -> Result<Option<String>, PlanError>;
}

// hook.rs — trait signature unchanged; ProcessRunner captures only when asked.
pub trait HookRunner { fn run(&self, hook: &PlannedHook, target: &Path) -> Result<HookOutcome, HookError>; }
//   Capture empty  → command.status()  (today's path, stdio inherited)
//   Capture stdout → stdout piped, stderr inherited, command.output()  (and vice-versa / both)
pub(crate) fn decode(bytes: Vec<u8>) -> Result<String, DecodeError>;  // strict UTF-8, strip trailing \r\n

// apply.rs — apply_reporting keeps its signature; one new error variant.
pub enum ApplyError { /* existing… */
    Deferred(PlanError),                                // deferred render/cwd fault
    HookOutput { index: usize, id: Id, valid_up_to: usize },  // non-UTF-8, no bytes
}
```

Apply loop, after files are written (the only interleave):

```text
results = HookResults::default()
for (index, step) in hooks:
    hook = Ready(h)         → h
         | AfterHooks(d)    → d.render_hooks(&results)?      // ApplyError::Deferred
    symlink-check hook.cwd                                   // just before running
    outcome = runner.run(hook)?                              // HookIo unchanged; raw bytes
    if !outcome.success && !(hook.allow_failure && outcome.code.is_some()):
        return Err(ApplyError::Hook{ index, hook, outcome })  // Display unchanged, no bytes
    if hook.id:                                              // decode strictly after the failure gate
        stdout/stderr = decode(...)?                         // ApplyError::HookOutput on non-UTF-8
        results.record(id, HookResult{ code, stdout, stderr })
after_apply = Ready(s) → Some(s) | AfterHooks(d) → d.render_message(&results)?
// results dropped here
```

## 4. Module / seam diagram

```text
template.rs  Template::load
  compile all Jinja into the 1075 retained program
  id / capture / allow-failure parse (schema literals)
  settle_hook_results(): resolve reads → producers, order, surface, capture,
                         allow-failure-must-be-read   (no new reserved name)
        │ HookNode{ id, capture, allow_failure }  → 1069 parsed-node digest
        ▼
interview.rs  pure engine; interview hooks render here (may produce, never read)
        ▼
plan.rs  Plan::build (one context, no results)
  files / file rules / before-apply / interview hooks → Planned::Ready
  top-level hook reads a result? no → Ready | yes → AfterHooks(Deferred)
  after-apply reads a result?     no → Ready | yes → AfterHooks
        ▼
apply.rs  Plan::apply_reporting  ◀── the only interleave
  conflicts → NeedsTrust gate → symlinks → write files
  for step: [render deferred → symlink cwd → run → stop on failure → decode → record]
  render deferred after-apply
        ▲ HookOutcome{ code, stdout, stderr }
hook.rs  HookRunner::run (Capture), decode(), HookResults   (dropped at return)
```

Driver change: `main.rs` dry-run/trust listing matches on `Planned`. The engine,
staging, replay, configured resolution, canonical-target identity, and context
projection do not change.

## 5. Error / results contract

### Invariants

1. A result is read as `<id>.exit_code`, `<id>.stdout`, `<id>.stderr` — an object
   with exactly those three fields. `<id>` is an ordinary identifier in the
   authored-id space; there is **no new reserved name**.
2. **Static availability.** A surface may read `<id>` only if it always runs
   after the producer: a top-level hook `when`/`run[1..]`/`args`/`cwd` listed
   after the producer, or `messages.after-apply`. Every other reference is a
   load error.
3. **"Did not run" is a value, never undefined.** Every read `<id>` is bound in
   every reading surface; a skipped/unreached producer is
   `{exit_code: none, stdout: none, stderr: none}`. `exit_code is none` ⇔ did
   not run.
4. **Capture equals reads.** `capture: [stdout, stderr]` is declared on the
   producer. A read stream that is not captured is a load error; a captured
   stream that is never read is a load error. An uncaptured stream inherits the
   terminal as today. Toha never prints captured text.
5. **Default failure unchanged.** A nonzero exit or any signal stops remaining
   hooks and fails the apply. `allow-failure: true` (which requires a read `id`)
   tolerates a nonzero **exit code only**; a signal stays fatal.
6. **Results last one apply.** Never stored in `Plan`, `Applied`, `ApplyError`,
   or `StagedRecord`.
7. **No leakage.** No `Debug`/`Display` of `HookOutcome`, `PlannedHook`, or
   `ApplyError` contains captured text.
8. A hook without `id` parses, plans, runs, handles stdio, reports errors, and
   lists in dry-run exactly as today.

### Load errors (aggregated `LoadError`)

| Condition | Message |
| --- | --- |
| bad `id`, or `id` equal to a data/question/computed/group/hook id | existing `invalid identifier` / `duplicate id: <id>` |
| `id` equal to one of the seventeen reserved names | the approved 1075 collision message, unchanged |
| `id` together with `each` | `a hook with each cannot declare id` |
| `capture`/`allow-failure` without `id` | `capture requires id` / `allow-failure requires id` |
| `allow-failure: true`, result never read | `the result of <id> is never read; read <id>.exit_code in a later hook or in messages.after-apply` |
| read of a hook result before its producer runs / self / unknown id | `hook result read before hook \`<id>\` runs: <id>` |
| read on a disallowed surface (file body/path/rule, before-apply, interview field, interview hook, run[0]/script path, each) | `hook results are readable only in a later top-level hook when/run[1..]/args/cwd and messages.after-apply: <id>` |
| stream read but not captured | `hook \`<id>\` does not capture <stream>; add \`capture: [<stream>]\`` |
| stream captured but never read | `captured <stream> of hook \`<id>\` is never read` |
| unknown result field | `unknown hook result field: <id>.<field>` |

### Apply outcomes

| Situation | Result |
| --- | --- |
| nonzero, no `allow-failure` | `ApplyError::Hook`; later hooks and after-apply do not run |
| nonzero, `allow-failure` | recorded (`exit_code = Some(n)`); continue |
| signal (no code) | `ApplyError::Hook`, even with `allow-failure` |
| cannot start | `ApplyError::HookIo` (unchanged) |
| deferred `when` false at apply | not run; result stays not-run (`none`) |
| deferred render fault | `ApplyError::Deferred(PlanError::Render)` with the existing `hooks[i].<field>` text |
| deferred `cwd` symlink / invalid | `ApplyError::Symlink` / `ApplyError::Deferred(PlanError::Path)` at that step |
| non-UTF-8 captured stream | `ApplyError::HookOutput{index,id,valid_up_to}`, fatal, no bytes |
| trailing newlines | all trailing `\r`/`\n` removed (shell `$(…)`); empty output is `""` |

### Dry-run, trust, stop/abort, replay

- **Dry-run** builds the plan and runs nothing; `run[0]`/`each` are eager, so
  every planned invocation is listed, with `<id.field>` placeholders for a
  deferred arg and the deferred `when` shown in source form. After-apply is not
  shown (unchanged).
- **NeedsTrust** still keys on `!plan.hooks.is_empty()`; deferred steps count.
- **Stop/abort** produce no `Completed`, so no `Plan` and nothing runs (1077).
- **Staged `apply --trust`** applies a fresh plan; results exist only inside that
  apply; `StagedRecord` and all replay routes are unchanged and perform no live
  read (1075).

### Trust coverage and the 1069 guard test

`id`, `capture`, and `allow-failure` are parsed hook-node fields → covered by the
1069 parsed-node digest automatically; no review-code change. **No new field
names an in-tree executable** (the executable surface is still `run`/`script`/
`args`, and `run[0]`/`script` path cannot read a result, so no program is derived
from output). Impl 1063 adds a schema-driven guard test that reads every
hook-node key and classifies it as `ExecutableReference` (`run`, `script`) or
`NoExecutableReference` (`args`, `cwd`, `when`, `each`, `id`, `capture`,
`allow-failure`); an unclassified key fails the test, citing the 1069 coverage
decision. Hook trust requirements are unchanged; staged `apply --trust` stays
hook-execution-only.

## 5A. JSON results mode — opt-in `parse: json`

A producer may opt in to parse its stdout as JSON so the parsed value is read
**directly at `<id>`** (not only as raw text at `<id>.stdout`). See
`06-json-revision-grounding.md`, `07-json-cross-judge.md`, and
`08-json-synthesis.md`.

### Caller usage

```yaml
hooks:
  - id: pkg
    run: [ pkg-tool, info, --json ]   # prints {"name":"demo","version":"1.4.0","tags":["a"]}
    parse: json                       # requires id and capture: [stdout]
    capture: [ stdout ]
  - run: [ note-tool, "built {{ pkg.name }} {{ pkg.version }}" ]
    when: "pkg.tags | length > 0"
messages:
  after-apply: "Created {{ pkg.name }} at {{ pkg.version }}."
```

Metadata, when needed, goes to a separately named result:

```yaml
hooks:
  - id: findings
    run: [ lint-tool, --format, json ]  # exit 1 with a JSON list when it finds issues
    parse: json
    capture: [ stdout ]
    status-id: lint                     # lint = {exit_code, stdout, stderr, parsed}
    allow-failure: true                 # requires status-id; lint.exit_code must be read
  - run: [ lint-tool, --fix, . ]
    when: "lint.exit_code != 0"
messages:
  after-apply: >-
    {% if lint.parsed %}{{ findings | length }} findings.{% else %}Lint: {{ lint.stdout }}{% endif %}
```

### Contract

1. **The value is `<id>`.** With `parse: json`, `<id>` is the parsed stdout as a
   plain JSON value: object → `<id>.key` / `<id>['a-key']`, array → `<id>[i]`,
   string/number/bool → the scalar, `null` → `none`. A parsed string is **data,
   not a template** (never re-rendered). serde_json integers above `u64` become
   floats (no `arbitrary_precision`) — disclosed.
2. **Opt-in and explicit.** `parse: json` requires `id` and `capture: [stdout]`
   (capture stays the single control for "stdout is piped, not shown"). A hook
   without `parse` is exactly the v1/today behaviour.
3. **Metadata is a separate name.** `status-id: <name>` binds `<name> =
   {exit_code, stdout, stderr, parsed}` — an ordinary identifier in the
   authored-id space, no new reserved name. `parsed` is true iff stdout parsed
   as JSON this run. `status-id` is **required** when the producer may not run
   (its own `when`, an enclosing group/branch `when`, or a skippable interview
   position — computed from the retained program) or has `allow-failure`;
   otherwise it is optional.
4. **`<id>` is `none`, never undefined**, when the producer did not run or a
   tolerated failure did not parse. `<status-id>.exit_code is none` ⇔ did not
   run; `<status-id>.parsed` ⇔ stdout parsed; JSON `null` is a parsed `none`
   (`parsed` true). This preserves the v1 "always bound, never undefined" rule.
5. **Apply order (failure first):** run → if untolerated nonzero or a signal,
   the v1 `ApplyError::Hook` fires and nothing is parsed → decode stdout
   strict-UTF-8 (`NotUtf8` fatal) → parse. On exit 0, empty or malformed stdout
   is fatal. On a tolerated nonzero exit, parse is lenient: on failure `<id>` is
   `none`, `parsed` is false, the raw text stays in `<status-id>.stdout`, and the
   apply continues.
6. **One output fault type.** `ApplyError::HookOutput { index, id, fault }` with
   `OutputFault::{ NotUtf8 { valid_up_to }, Empty, NotJson { line, column } }`.
   No message carries stdout bytes; `NotJson` carries only a position.
7. **Best-effort migration guard.** Writing `<id>.exit_code`, `<id>.stdout`, or
   `<id>.stderr` on a `parse: json` hook is a load error naming the subscript
   escape (`<id>['exit_code']`) and the `status-id`. It reuses
   `undeclared_variables(true)` (which v1 already needs) to tell `GetAttr` from
   `GetItem`; aliasing, loop variables, and non-variable roots fall through to a
   plain key read (disclosed, not silently wrong).

### Load errors (added to §5)

| Condition | Message |
| --- | --- |
| `parse`/`status-id` without `id` | `parse requires id` / `status-id requires id` |
| `status-id` without `parse: json` | `status-id requires parse: json` |
| `parse: json` without `capture: [stdout]` | `parse: json requires capture: [stdout]` |
| `status-id` duplicate / reserved / invalid / equal to `id` | existing `duplicate id` / 1075 collision / `invalid identifier` |
| `allow-failure` on a `parse: json` hook without a read `status-id` | `add status-id: <name> and read <name>.exit_code` |
| `<status-id>` field not in `exit_code`/`stdout`/`stderr`/`parsed` | `unknown hook result field` |
| attribute `<id>.exit_code`/`.stdout`/`.stderr` on a `parse: json` hook | `hook <id> parses stdout as JSON; use <id>['exit_code'] for a JSON key or <status-id>.exit_code for metadata` |

### Dry-run, trust, dependencies

- Dry-run parses nothing; the listing shows `parse: json`. Stop/abort/replay
  unchanged; parsed values live one apply only.
- `parse` and `status-id` are parsed hook-node fields → covered by the 1069
  digest automatically; neither names an executable; `run[0]`/`script` still
  cannot read a result, so no program is derived from output.
- **No new dependency:** `serde_json = "1"` is already in `Cargo.toml`, and the
  render context is already `serde_json::Value`.
- **Validation target for impl 1063:** a fixture for a read *through* a `none`
  `<id>` (`<id>.key`) under minijinja's undefined mode.

## 6. Compatibility and canonical impact (described, owned by impl 1063)

- **Existing templates:** a hook with no `id` is byte-identical to today
  (`status()` runner, inherited stdio, same Debug/Display, same dry-run).
- **Crate API (pre-v1, no back-compat owed):** `Plan.hooks:
  Vec<Planned<PlannedHook>>`, `Plan.after_apply: Option<Planned<String>>`,
  `PlannedHook`/`RenderedHook`/`HookNode` gain `id`/`capture`/`allow_failure`,
  `HookOutcome` gains `stdout`/`stderr`, new `ApplyError` variants.
- **Schema (`template-format.schema.yml`):** add `id` (identifier),
  `capture` (array of `stdout`/`stderr`, unique, min 1), `allow-failure`
  (boolean), `parse` (enum `[json]`), and `status-id` (identifier) to the
  top-level and interview hook objects; `capture`/`allow-failure`/`parse`/
  `status-id` require `id`; `parse: json` requires `capture: [stdout]`;
  `status-id` requires `parse: json`; `id` excludes `each`.
- **Spec (`template-format.yml`):** add a "Hook results" paragraph stating
  invariants 1–5 and the readable surfaces.
- **Guide (`docs/template-hooks.md`):** add a "Use a hook's result" section from
  the 1.1–1.3 examples.
- **CLI (`command-line-interface.yml`):** dry-run/trust listings show the source
  form of a result-reading hook; Toha does not print a stream a template reads.
- **Interview protocol:** no change.

## 7. Decisions for Bob (Phase C — see the deck)

All five are open; Bob's JSON request approves none of the first three.

1. Readable-surface breadth: BROAD-minus-`run[0]`/`each` (recommended) vs NARROW.
2. Hook `id` shares the answer-id space (no new reserved name).
3. Decode policy: strict UTF-8, and (on exit 0) malformed JSON, fatal with a
   byte-free error (recommended).
4. JSON mode shape (§5A): `parse: json` makes `<id>` the parsed value; metadata
   goes to an optional/required `status-id`. Approve the exact shape.
5. JSON capture: `parse: json` requires an explicit `capture: [stdout]`
   (recommended) vs implying it. Two-way door.
