# Grounding — content injection into existing files

Phase A of the architect workflow for design 1068 (paired implementation 1031),
epic 1065 (Toha 0.2.0). Traces the actual caller-to-result flow, the types, the
state ownership, the error and recovery paths, and the trust seams that the
injection design must honor. Evidence is by file and symbol at epic head
`cfab3286`.

## What "content injection" must add

Today Toha owns whole files. Every planned mutation replaces a target file in
full. Content injection is the first mutation that writes *into a file Toha does
not own* — adding or updating a bounded region of an existing file while leaving
the rest of that file to its owner. The observable acceptance for this slice
(task 1068) is: a template that injects into an existing file, applied twice,
changes that file exactly once.

The design must resolve, per task 1068:

- **anchors / selection** — how a template names *where* content goes in a
  target file it did not create;
- **idempotency** — a second apply against an unchanged result is a no-op;
- **conflict behavior** — injection targets an existing file by definition, so
  the current "target exists ⇒ conflict unless `--force`" rule cannot govern it;
- **ownership of file mutations** — precisely what bytes Toha manages versus
  what the user owns, expressed so the git-based-update design (1066) and the
  paired implementation (1031) can consume it;
- **failure / recovery** — missing or ambiguous anchor, drifted managed region,
  partial write.

## The mutation model today (whole-file only)

`src/plan.rs`

- `Plan { files: Vec<PlannedFile>, conflicts: Vec<TargetPath>, hooks: Vec<PlannedHook>, before_apply: Option<String>, after_apply: Option<String> }` (`plan.rs:88`).
- `PlannedFile { path: TargetPath, content: Content, source: PathBuf }` (`plan.rs:96`).
- `Content::{ Rendered(String), Copied(PathBuf) }` (`plan.rs:102`) — a planned file
  is *either* rendered text held in memory *or* a byte-for-byte copy from a
  source path. Both are **whole-file** results.
- `Plan::build(template, completed, target) -> Result<Plan, PlanError>`
  (`plan.rs:178`) is pure over the template folder plus the target directory
  *listing*. It renders every source-tree file (`walk`, `plan.rs:331`) and every
  `files` rule (`plan.rs:200`), then `add`s each as a `PlannedFile`.
- `Plan::add` (`plan.rs:272`) rejects a duplicate target
  (`PlanError::Duplicate`), rejects a symlink component
  (`has_symlink_component`, `plan.rs:71`), and — the load-bearing line for this
  design — records the target in `self.conflicts` **iff `destination.exists()`**
  (`plan.rs:295`). Existence *is* conflict.

`src/apply.rs`

- `Plan::apply_reporting(target, options, runner, on_written)` (`apply.rs:66`) is
  the sole writer. Order of guards:
  1. recompute conflicts (any planned file whose destination now exists),
     `apply.rs:73`;
  2. if `!force && !conflicts.is_empty()` → `Err(ApplyError::Conflicts)`
     (`apply.rs:79`);
  3. if `!trusted && !hooks.is_empty()` → `Ok(Applied::NeedsTrust(self))` — writes
     nothing (`apply.rs:82`);
  4. symlink-component check for every file and every hook cwd (`apply.rs:85`);
  5. for each file: `create_dir_all(parent)`, then `fs::write(text)` or
     `fs::copy(source)` — **full replace** — then set the unix exec bit from the
     source mode, then `on_written(path)` (`apply.rs:104`);
  6. run hooks in order; first failure stops and fails the apply (`apply.rs:154`).
- `Applied::{ Written { files, hooks_run, after_apply }, NeedsTrust(Plan) }`
  (`apply.rs:18`).
- `ApplyOptions { force: bool, trusted: bool }` (`apply.rs:13`).
- **No atomicity across files.** Each `fs::write` is direct; a mid-plan I/O
  failure leaves already-written files in place (test
  `a_failed_write_reports_only_the_files_written`, `apply.rs:322`). Recovery
  today is "re-run"; whole-file writes are naturally re-runnable.

### Consequence for injection

`Content` has no variant that expresses "modify part of an existing file", and
`apply` has no path that reads a target file's current bytes. Injection needs a
third planned-mutation shape whose `apply` step **reads the existing target,
computes the new full bytes, and writes them** — and whose conflict semantics
are *not* the whole-file existence rule, because the target is expected to
exist.

## How an author declares generated files (where injection slots in)

`src/template.rs`, `docs/specifications/template-format.yml`

- The domain `Template` carries `files: Vec<FileRule>` (`template.rs:49`), plus
  `source_dir`, `ignore`/`static_files` glob sets, `hooks`, `messages`.
- `FileRule { each: Each, source: PathBuf, path: Tmpl, when: Option<Expr> }`
  (`template.rs:153`). A rule renders the support file at `source` once per item
  of `each`, to the rendered `path` (target-relative). `source` is confined to
  the template root by `confined_file` (`template.rs:434`), which canonicalizes
  and checks `starts_with(root)`.
- Load path: `Template::load` (`template.rs:708`) resolves `!include` tags
  (`resolve`, `template.rs:253`), validates against the embedded
  `template-format.schema.yml` (`SCHEMA`, `template.rs:243`), then the `Builder`
  builds domain types and runs semantic checks. **A new injection declaration is
  a new schema shape plus a new `Builder` branch plus a new domain type** — the
  design owns proposing these but the paired implementation (1031) owns editing
  the canonical schema/spec.
- Jinja typing is fixed at load: string fields → `Tmpl`, `when`/`computed`/
  `format` → `Expr`, typed scalars → `Typed<T>` (`architecture.yml`
  `typed-jinja-values`). An injection's anchor/marker/content fields inherit this
  discipline.

## Purity and the replay contract (hard constraint)

`src/interview.rs`, `src/staging.rs`, memory
`toha-vision-decisions-loops-answer-ux-file-generation-update-da6f11ad`

- The interview engine is a pure value state machine: `Interview::{ Asking(Pending), Complete(Completed) }` (`interview.rs:43`). `Completed { answers, messages, last_messages, hooks, now, .. }` (`interview.rs:60`) is the whole input to `Plan::build` besides the template and target.
- Staging stores **inputs, not plans**: target path, formal name, resolved
  commit, frozen `now`, and each accepted answers submission. `Record::replay`
  (`staging.rs:128`) reloads the template at the commit, restarts with the same
  `Seed { now, defaults }` (`interview.rs:37`), and replays the raw submissions.
  Because the engine is pure, replay reaches the same `Completed`.
- **Therefore injection output must be a pure function of `(Template, Completed,
  target-directory state at apply time)`.** Reading the existing target file's
  bytes is allowed at plan/apply time (the plan already reads the target listing
  for conflicts); it must not leak into the engine. The idempotency guarantee
  must survive replay: applying a staged interview twice must equal applying it
  once, exactly as a fresh double-apply would.

## Safety seams injection must not weaken

- `TargetPath::parse` (`plan.rs:18`) rejects empty, absolute, backslash, drive
  (`C:`), `..` escapes, and any component named `.git`; injection targets go
  through the *same* newtype. No new path type.
- `has_symlink_component` (`plan.rs:71`) is checked at build and again at apply
  for every file and hook cwd; an injected target must be checked the same way
  before its bytes are read or written.
- Trust: hooks gate on `options.trusted`; injection itself performs no code
  execution, but if injection ever runs a program (e.g. a formatter) it inherits
  the trust seam. Default design keeps injection a pure text/data transform with
  no new port.
- `--force` (spec "Overwriting", `command-line-interface.yml:91`) governs
  *overwriting an existing file*. Injection is not an overwrite; the design must
  say how `--force`, the conflict list, `--dry-run` output ("create file.txt",
  fixture `tests/fixtures/dry-run/expect.yml`), and the exit codes (0 success, 1
  error, 3 needs-trust, 4 pending, 5 ambiguous) extend without weakening the
  no-silent-overwrite guarantee.

## The referenced candidate: jaq (inspected, adoption not assumed)

`jaq` (crates.io/crates/jaq; github 01mf02/jaq) is a pure-Rust `jq` clone.
`jaq-core` is the embeddable library; it evaluates jq programs over JSON-like
values (a `ValT` trait) and supports the data formats YAML, TOML, CBOR, and XML.
Two facts decide its role here:

- It is a **transformer that reserializes**: it parses a document to a value,
  applies a filter, and prints the value. It does **not** preserve the input's
  comments, key order, or byte formatting. Structured merge via jaq changes the
  whole document's formatting, which for a user-owned file is itself a
  destructive whole-file rewrite in disguise.
- It applies **only to structured formats**. Arbitrary text targets (README,
  `.gitignore`, `.env`, source files, CI YAML *with comments*) are out of its
  reach.

So jaq is at best a mechanism for one *structured-merge* injection mode over
JSON/TOML where formatting loss is acceptable — not a general injection
solution, and not required. The design should treat structured merge as an
optional, clearly-bounded mode and not let a jq dependency dictate the core
contract.

## Constraints carried into the design (Preserve / Change / Avoid / Risk)

- **Preserve.** Pure interview engine and staging replay determinism; the
  `TargetPath`/symlink/`.git` safety newtype; no-silent-overwrite of files Toha
  does not manage; the one-`HookRunner`-port shape; plan-before-write; every
  driver (terminal/headless/staged/crate) keeps working; spec-as-contract.
- **Change.** `Plan`/`Content` gains a shape for in-file modification; `apply`
  gains a read-modify-write path; `template.yml` gains an injection declaration;
  `--dry-run` and conflict/exit reporting learn the new mutation; the
  file-mutation contract is written down for 1066/1031.
- **Avoid.** A heuristic idempotency that depends on the injected text still
  matching after the user edits around it; a jq dependency on the critical path;
  a new timeout, permission, subprocess, or pinned-check mechanism (each needs
  separate explicit approval); widening `--force` to silently rewrite
  user-owned files; scattering marker/anchor policy across modules.
- **Risk.** Marker comments differ by file type (there is no universal comment
  syntax); an existing file may already contain hand-written content the author
  wants to target without markers; a managed region the user edits by hand is a
  drift the design must define behavior for; ordering between whole-file writes
  and injections into the same target within one apply.

## Downstream consumers (coordinate through the board)

- **1031** (paired implementation) consumes the exact approved interfaces, data
  shapes, and falsifiable validation.
- **1066 / 1029** (git-based template updates) consumes the **mutation
  contract**: the model of what Toha owns in each target file, so an update or
  replay can reason about re-application. 1068 → 1066 is an approved-contract
  prerequisite edge; injection is sequenced before updates precisely so the
  applied-record/replay design accounts for the complete file-mutation model.
- Parallel design roots own isolated concerns: 1069 hook-review/trust, 1074
  bundled-demo/identity, 1076 Jinja-includes/root boundaries. The shared surface
  this design must name explicitly is **`Plan` and the file-mutation types**
  (`Content`/`PlannedFile` and any new sibling) — the interface 1076's include
  work and 1066's update work both touch. Coordination is through board
  artifacts, not code.
