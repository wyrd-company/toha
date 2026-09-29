---
relationships:
  depends-on:
    - content-injection
    - headless-recovery
    - hook-results
  informs: project-updates
---

# Grounding — git-based project updates

Phase A grounding for design 1066 (paired implementation 1029), epic 1065. Base
`epic/second-release` @ `2c4a197`. Evidence cites `file:line` in the worktree at
that commit. This traces the real caller-to-result flow a project-update feature
must consume, the state ownership it must extend, and the approved prerequisite
contracts it composes with.

## How generation works today (caller to result)

1. **Resolve the template.** `cli::resolve` produces `ResolvedTemplate {
   formal_name, commit, folder, approval, named, aliases, source }`
   (`src/cli/resolve.rs:17-33`). `formal_name` is derived from a source
   `Address` (`src/source.rs:118-164`); a git fetch returns `Fetched { commit,
   reference_kind }` with a 40-char commit hash (`src/source.rs:26-29,341-344`).
   The registry persists the resolved commit as `Entry.commit: Option<String>`
   (`src/registry.rs:26`) and caches sources under `.../sources/{key}/{commit}`
   (`src/source.rs:203-208`).

2. **Interview.** `Interview::start(template, seed)` runs the pure engine
   (`docs/technical-designs/architecture.yml`, `interview-engine`). Answers form
   one flat `Answers` map keyed by `Id`. Configured defaults enter through an
   origin-bearing `Resolution`.

3. **Plan.** `Plan::build(template, completed, target) -> Result<Plan,
   PlanError>` (`src/plan.rs:381-571`) renders every file and path, reading only
   the template folder and the target directory listing. `Plan { files:
   Vec<PlannedFile>, conflicts: Vec<TargetPath>, hooks, before_apply,
   after_apply, result_seed }` (`src/plan.rs:95-106`); `PlannedFile { path:
   TargetPath, content: Content::{Rendered(String), Copied(PathBuf)}, source }`
   (`src/plan.rs:118-128`). A **conflict today is exactly "the target file
   already exists"**: `Plan::add` pushes to `conflicts` when `destination.exists()`
   (`src/plan.rs:595-597`), rechecked live in apply (`src/apply.rs:120-127`).

4. **Apply.** `Plan::apply(self, target, ApplyOptions { force, trusted },
   runner)` (`src/apply.rs:102-109`) delegates to `apply_reporting`. Trust gate:
   returns `Applied::NeedsTrust(Plan)` when hooks exist and `!trusted`
   (`src/apply.rs:131-133`). Conflict gate: `ApplyError::Conflicts` when
   `!force` and conflicts exist (`src/apply.rs:128-130`). Writes each file
   (`src/apply.rs:162-211`), then runs hooks (`src/apply.rs:220-271`). Result:
   `Applied::Written { files, hooks_run, after_apply }` (`src/apply.rs:19-27`).

## State ownership and persistence today

- **Nothing is written into the target project to record what Toha wrote.** The
  apply write loop writes only `PlannedFile` content (`src/apply.rs:162-211`).
  There is no manifest, lock, or dotfile. `HookResults` are dropped when apply
  returns and never reach `Plan`, `Applied`, `ApplyError`, or `StagedRecord`
  (`src/hook.rs:213-215`).

- **The staged record lives in XDG state, not with the project.**
  `StagedRecord { target, template, commit, named, now, submissions:
  Vec<IndexMap<String,Value>>, context }` (`src/staging.rs:23-38`) is stored
  content-addressed as `Sha256(canonical_target).json` under `dirs.state`
  (`.../toha/staged`, `src/main.rs:376-384`; `Store::path_for`
  `src/staging.rs:205-208`), written atomically via temp+rename
  (`src/staging.rs:246-296`). It **already carries the resolved `commit`**
  (`src/staging.rs:27`) and the accepted `submissions`. **It is deleted on
  successful apply** (`src/main.rs:1388-1394`) and on abort (`src/main.rs:950,985`).
  Replay reloads the template at `commit` and re-runs `submissions` through the
  pure engine (`src/staging.rs:381-452`).

- **`CanonicalTarget`** is the sole target identity, produced only by
  `canonical_target(&Path)` (`src/staging.rs:149-196`), and crosses storage,
  plan, and apply. It absolutizes, lexically normalizes, canonicalizes the
  nearest existing ancestor, and re-appends the missing suffix.

- **There is no template `version` field.** `Template` (`src/template.rs:45-83`)
  and `RawTemplate` (`src/template.rs:425-437`) carry no version key. **The
  resolved git commit is the only version identity.**

- **`.toha.yml` in the current directory is already the local *config* file**
  (`src/config.rs:164`, `src/main.rs:143`). An applied record placed at the
  project root must not reuse that name.

## Trust seam

Hooks run only under approval of the template's executable surface (a digest of
hook nodes + executed file bytes) or `--trust`; an update that changes that
surface withdraws trust until re-approved
(`docs/specifications/command-line-interface.yml`, "Trust"). A project update
re-renders and re-applies, so it re-enters this gate exactly as a fresh apply
does. Hook results are per-apply only (`src/hook.rs:213-215`), so update cannot
carry any stored hook output forward.

## Fixture harness

`tests/fixtures/<name>/` holds `template/`, `answers.json`, `expect.yml`
(`exit`, `options{force,trust,dry_run}`, hooks, messages, `now`), an optional
`existing/` pre-tree, and an `expected/` post-tree
(`tests/support/mod.rs:13-72`). `every_fixture_through_library`
(`tests/fixtures.rs:280-309`) copies `existing/` into a temp target, runs, and
asserts the tree equals `expected/`. A second harness replays through staging.
The `conflict` fixture proves the unchanged-on-conflict contract. The
version-A-to-B edited-file merge fixture this design requires is a new shape:
it needs a *base* tree (what A wrote), an *edited* tree (operator changes), and a
*B template*, none of which the current single-`existing/` shape expresses.

## Approved prerequisite contracts consumed

### Content injection (1068) — ownership identities

`Plan::mutations() -> impl Iterator<Item = FileMutation<'_>>` with
`FileMutation::{ Whole { path }, Region { path, region }, JsonValue { path,
json_path } }` (design `content-injection/05-design.md:329-347`). Ownership:
`Whole` replaces the whole file; `Region` owns a visible marker pair + enclosed
span, a `sha256` checksum distinguishing safe replay from operator **drift**
(refuses without `--force`); `JsonValue` owns a typed value at a path,
**convergent** (a later apply converges an operator change back without
`--force`, never drifts, preserves unrelated source). The injection design
explicitly defers to this design: "Removal of an ownership identity and
three-way project merge behavior remain decisions of the project-update design"
(`content-injection/05-design.md:357-362`).

### Caller routes & answers identity (1070) — authoritative revision at epic HEAD

Epic HEAD `2c4a197` is "docs(design): route answers by caller kind" — the v3
caller-route + identity contract (`headless-recovery/design.md`). An external
answers document is exactly `{ template, answers }` with **no target and no
commit member**; "A matching document can answer another target or another
resolved commit of the same formal template identity"
(`headless-recovery/design.md:145-152`). `ResolvedTemplate.formal_name` and
`StagedRecord.template` are the identity authorities; `StagedRecord.submissions`
holds only accepted inner answer maps (`headless-recovery/design.md:154-173`).
Binding a document to a target or commit and adding identity to staged
submissions are **out of scope for 1070** and therefore design space here
(`headless-recovery/design.md:623-632`). The applied record designed here is a
distinct project-resident artifact from both the answers document and the XDG
staged record; the two must not be conflated.

### Opt-in hook results (1078) — per-apply lifetime

Hook results (`exit_code`, `stdout`, `stderr`, and a parsed value under `parse:
json`) are readable by later Jinja surfaces within one apply and are **never
persisted** — not in `Plan`, `Applied`, `ApplyError`, or `StagedRecord`
(`hook-results/design.md:249-254`). Update re-runs hooks under the existing
trust gate and cannot depend on any stored hook output.

## Why this changes layering (Preserve / Change / Avoid / Risk)

This feature introduces the first Toha artifact that persists **inside the
target project** and the first non-whole-file reconciliation of operator edits.

- **Preserve.** The pure interview engine and every driver route
  (terminal/headless/async/staged/direct/crate). `Plan::build` reads only the
  template folder and target listing. `CanonicalTarget` as the sole target
  identity. The trust gate. The injection ownership identities. The XDG staged
  record's shape and lifetime — the applied record is separate.
- **Change.** Add a project-resident applied record; add a re-render-against-a-
  recorded-baseline path; add three-way reconciliation of operator edits per
  ownership identity; extend the fixture harness to express base/edited/new
  trees.
- **Avoid.** Reusing `.toha.yml` (local config). Conflating the applied record
  with the XDG staged record. Collapsing Region/JsonValue ownership into
  whole-file merge. Persisting hook output. Any subprocess, new permission/
  access rule, new timeout, or pinned-version check without explicit disclosure
  and approval.
- **Risk.** The three-way merge engine choice (in-process `gix` merge vs a
  pure-Rust merge crate vs a subprocess — the last requires explicit approval).
  Answer-set evolution: a recorded answer may no longer validate against T@B
  (mirrors the existing staged-replay rejection at `src/staging.rs:428-452`).
  Record trust: the applied record is operator-editable on disk, so update must
  treat it as untrusted input parsed at the boundary.
