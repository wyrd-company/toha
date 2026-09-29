# Runner task — git-based project updates (Toha 0.2.0)

You are producing **one candidate design** in architect's parallel exploration.
Read the **architect** skill and `runner-prompt.md` first. This file is the task
and the grounding. You do **not** receive the scoring rubric. Produce the best
design your model can make; do not hedge toward a safe middle — the differences
between candidates are the signal.

Write your whole package **only** inside your assigned isolated directory
(given to you as an absolute path). Do not read or write any other candidate's
directory, and do not touch the repository working tree.

## The problem

Toha generates a project by rendering a template's source subdirectory into a
target directory from an interview's answers. Today that is a one-way act: once
files are written, Toha keeps no record in the target of what it wrote or from
which template version, and a later template change cannot be carried into the
already-generated project.

Design the **project update** capability for Toha 0.2.0: a caller who generated
a project from template T at commit A, using answer set X, can later move that
project to template T at commit B — re-rendering with the same answers — while
reconciling with any edits an operator made to the generated files in between.
The reconciliation is **git-based**: a three-way merge whose base is what Toha
last wrote, whose "theirs" is the operator's current on-disk file, and whose
"ours" is what Toha would write now.

This is a **design-only** task. Do not write runtime code, stubs, or edits to
shared specifications/schemas. Describe proposed contract edits in prose. Keep
all sketches (`not implemented` bodies, pseudocode) inside your design package.

### Decisions your design must resolve

1. **Applied record — shape and ownership.** To update a project you must know
   what was applied. Define the record that persists, *with the project*, the
   source formal name, the resolved commit, the accepted answers, the target,
   and any per-file version/ownership information needed to merge. Compare at
   least these structurally distinct carriers and recommend one:
   - a single project-root file (e.g. a `.toha`-family file committed with the
     project);
   - a git commit trailer on the project's own history;
   - a custom git ref / note.
   The record must travel with the project (survive clone, move, and re-check-out).

2. **Update and merge semantics.** Define precisely, per ownership identity:
   - **old-template/old-answer render** (the base: what Toha last wrote) versus
     **new-template/same-answer render** (ours: what Toha would write now);
   - the three-way merge of base vs operator edits (theirs) vs ours, and how a
     conflict is represented to the operator;
   - behavior when an **operator edited** a Toha-owned file;
   - what happens when an **ownership identity disappears** between template
     versions (a file, a region, or a JSON path that T@A produced and T@B no
     longer produces);
   - idempotency: running update twice with no template or operator change is a
     byte no-op.

3. **Adoption of record-less projects.** A project generated before this feature
   (or by hand) has no applied record. Define how such a project is adopted:
   re-interview to reconstruct answers, then proceed, or refuse with guidance.

4. **Composition with the three approved prerequisite designs** (contracts
   below). Your design consumes their *approved interfaces*; their
   implementations need not exist yet. Do not silently change any of them; if
   your design needs a change to a prerequisite interface, call it out
   explicitly as an open question.

### Required output package (per `rationale-template.md`)

- **Usage (caller's view) first**: the README/quickstart a user reads, plus two
  or three concrete call sites (CLI command lines and one crate call site).
- Data/type sketch derived from the usage.
- Function signatures and the module/seam map (which modules, what they touch).
- Error/results contract (exhaustive).
- Rationale prose: problem, shape, tradeoffs, alternatives considered, open
  questions/risks, next implementation step.
- **The three-way merge engine choice.** "git-based" needs a merge
  implementation. State exactly what performs the merge and name it. Toha
  already depends on `gix` (gitoxide). A pure-Rust in-process library or gix's
  own merge capability is strongly preferred. **Application code must not spawn
  a CLI or subprocess** (e.g. shelling out to `git merge-file`) unless as a last
  resort; if you propose one, flag it loudly as requiring explicit operator
  approval. Link any third-party dependency you propose.

## Grounding — current architecture (evidence in `grounding.md`)

- One Cargo package `toha`: pure interview engine + drivers (terminal, JSON
  protocol, headless answers, crate). Modules: `template`, `interview`, `plan`,
  `apply`, `protocol`, `source`, `registry`, `config`, `staging`, `jinja`
  (private), binary shell.
- `Plan::build(template, completed, target) -> Result<Plan, PlanError>` renders
  every file and path, reading only the template folder and the target
  directory listing. `PlannedFile { path: TargetPath, content:
  Content::{Rendered(String), Copied(PathBuf)} }`. `Plan.conflicts` holds
  existing target files.
- `Plan::apply(self, target, ApplyOptions { force, trusted }, runner) ->
  Result<Applied, ApplyError>` refuses to write over an existing file without
  `--force` (today's whole-project protection), returns
  `Applied::NeedsTrust(Plan)` when hooks exist and trust is absent, else writes
  files then runs hooks.
- **Staged interviews are stored in the XDG *state* directory, keyed by target
  path** — the target path, template formal name, **resolved commit**, whether
  the argument resolved by name, the frozen `now()` instant, and every accepted
  answers submission in order. Replay reloads the template at the recorded
  commit and re-runs the submissions through the pure engine. This staged record
  does **not** travel with the project; do **not** conflate it with the applied
  record you are designing.
- The registry records each installed template's formal name, short name,
  source, ref, **commit**, location, aliases, and trust. A resolved template's
  commit is therefore already available.
- `templates update` today moves an *installed template* to the newest commit of
  its branch. It does **not** update generated projects. Your feature is the new,
  distinct project-side capability.
- Trust: hooks run only after the caller approves the template's executable
  surface (a digest of hook nodes + executed file bytes), or with `--trust`. An
  update that changes the executable surface withdraws trust until re-approved.
- Fixtures: `tests/fixtures/<name>/` holds `template/`, `answers.json`,
  `expected/` tree, and `expect.yml` (exit code, messages, recorded hooks). The
  harness drives the engine and applies into a temp dir; a second harness
  replays through staging.

## Prerequisite contract 1 — content injection (design 1068, approved)

Injection adds bounded mutation modes alongside whole-file rules. The plan is the
single source of truth for owned targets and exposes a derived ownership view:

```rust
pub enum FileMutation<'a> {
    Whole    { path: &'a TargetPath },
    Region   { path: &'a TargetPath, region: &'a RegionKey },
    JsonValue{ path: &'a TargetPath, json_path: &'a JsonPath },
}
impl Plan { pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>>; }
```

Ownership semantics your update design must retain (do not collapse into
whole-file):

- **Whole**: Toha owns and replaces the complete file.
- **Region**: Toha owns a visible marker pair and the enclosed byte span in a
  non-JSON text file. A `sha256` checksum in the end marker records the body
  Toha last wrote; a matching body is a safe replay, an operator edit inside the
  span is **drift** and refuses without `--force`. Markers are placed at a
  bootstrap anchor on first apply and found by key afterward.
- **JsonValue**: Toha owns the typed value at a dot/bracket path in a
  JSON/JSONC/JSON5 file (parsed through embedded `jsonc-parser`; no subprocess).
  Ownership is **convergent**: a later apply converges an operator-changed owned
  value back to the declared value **without** `--force`, and never drifts;
  unrelated keys, comments, key order, and whitespace are preserved.

The injection design explicitly leaves to *you*: "Removal of an ownership
identity and three-way project merge behavior remain decisions of the
project-update design; this design does not silently choose them." Injection uses
atomic per-target replacement; all mutations resolve before the first write.

## Prerequisite contract 2 — caller routes & answers identity (design 1070, approved)

- Three caller kinds — script, agent, person — each with its own command route.
  The route selects modality; no command guesses the caller from terminal
  status. Routes: script `apply T P --answers F` (one-shot, refuses if staged);
  agent `stage T P --async`, `continue P F`, `apply P`; person `stage T P`,
  `continue P`, `apply T P`.
- An external **answers document** is exactly `{ "template": "<formal name>",
  "answers": { ... } }`. It has **no target and no commit member**: "A matching
  document can answer another target or another resolved commit of the same
  formal template identity." Identity is compared by exact string equality
  against the route's established formal name.
- `ResolvedTemplate.formal_name` and `StagedRecord.template` are the identity
  authorities. `StagedRecord.submissions` holds only the accepted inner answer
  maps; the envelope is never stored.
- Explicitly **out of scope** for 1070 (i.e. available to you as design space):
  binding documents to a target or commit; adding identity to staged
  submissions. The applied record you design is a *distinct* project-resident
  artifact from both the answers document and the XDG staged record.

## Prerequisite contract 3 — opt-in hook results (design 1078, approved)

- A hook may opt into an `id` and expose `exit_code`, `stdout`, `stderr` (and,
  with `parse: json`, a parsed value) to *later* Jinja surfaces within the same
  apply.
- **Hook results last exactly one apply.** They are never stored in `Plan`,
  `Applied`, `ApplyError`, or `StagedRecord`, and never persisted. An update
  therefore re-runs hooks under the existing trust gate; it cannot rely on any
  stored hook output. A hook without `id` behaves exactly as today.

## Constraints (all candidates)

- Keep the interview engine pure and UI-free; every adapter (terminal, headless,
  async, crate) keeps working. Preserve staged/direct/crate/terminal routes.
- Toha is pre-1.0 with no dependent users: **no back-compat burden**. Prefer the
  right shape over a migration path.
- Do **not** introduce a new permission/access rule, a new or changed timeout, a
  pinned-version check, or an application subprocess without flagging it loudly
  as requiring separate explicit operator approval.
- Use generic, non-identifying example values and scenarios. Do not use the
  implementation's own domain as a sample domain.
- Portable paths only (`~/` or repo-relative) in anything you write.

Deliver the package in your assigned directory as `rationale.md` (the prose
package per `rationale-template.md`), with the type/signature sketch inline in
fenced Rust blocks and the module/seam diagram as a fenced text block.
