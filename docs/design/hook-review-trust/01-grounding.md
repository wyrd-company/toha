# Grounding: hook-script review and trust persistence

Design package for task 1069 (paired implementation 1033), Toha 0.2.0.
This is a design artifact. It describes the current system and proposed
contract edits. It does not change canonical specifications or runtime code.

## Traced model: how trust decides whether hooks run today

### The executable surface

A template can execute code through **hooks** (`docs/template-hooks.md`,
`docs/specifications/template-format.yml` "Hooks"):

- `run: [program, arg, ...]` — executes `program` directly, no shell, with the
  rendered arguments. Program is any executable on `PATH`.
- `script: path/inside/template` with optional `args` — executes an executable
  file **inside the template root**. The path is relative to the folder holding
  `template.yml` and must stay inside it.
- Both forms accept `when` (a guard expression), `each` (`<list> as <item>`),
  and `cwd` (a target-relative directory). `run`, `args`, and `cwd` carry Jinja
  substitutions rendered from answers/computed values at plan time.
- Hooks appear at top-level `hooks:` and as interview `- hook:` nodes. Interview
  hooks run first (interview order), then top-level hooks (list order).

Domain types (`src/plan.rs:107-127`): `PlannedHook { program: PlannedProgram,
cwd: Option<TargetPath>, template_root: PathBuf }`, `PlannedProgram::{Run(Vec<
String>), Script { path: PathBuf, args: Vec<String> }}`. The `Script` path is
resolved to `template.root.join(path)` at `plan_hook` (`src/plan.rs:306-330`).

### The trust gate

Trust is a single boolean, whole-template, all-or-nothing.

- Registry entry (`src/registry.rs:19-32`): `Entry { ..., trusted: bool }`,
  serialized in `templates.yml`. `FieldPresence` tracks whether `trusted` was
  written, so a sparse entry round-trips (`src/registry.rs:40-44`, `248-295`).
- Apply gate (`src/apply.rs:82-84`): `if !options.trusted && !self.hooks.
  is_empty() { return Ok(Applied::NeedsTrust(self)) }`. If trusted, **every**
  hook runs; if not, **none** run and the caller shows a dry run.
- `ApplyOptions { force, trusted }` (`src/apply.rs:13-17`). `trusted` is set by
  the binary to `registry_trusted || trust` (`src/main.rs:1056`), where `trust`
  is the `--trust` one-run flag and `registry_trusted` comes from the resolved
  entry.

### Where `registry_trusted` comes from (identity currently bound to trust)

- Resolution by **name** from user/system registry carries `trusted`
  (`src/cli/resolve.rs:72-81`, `entry()`), and only a name-resolved,
  non-local template is "trustable" (`src/main.rs:439-443`, `trustable`).
- A template given by **git address or folder** to `apply`/`stage` is forced
  `trusted = false` (`src/cli/resolve.rs:194-195`, `184-190`): address/folder
  invocations are trusted only through `--trust`
  (`command-line-interface.yml:38-50`).
- Local-layer aliases never grant trust; the local registry schema rejects a
  `trusted` field (`src/registry.rs:659-673`, `template-registry.yml:33-36`).
- Discovered templates are never trusted (`src/registry.rs:463`).
- Staged resume recomputes trust only when the argument resolved by name
  (`src/cli/resolve.rs:224-274`, `resume_template`; `architecture.yml`
  "staging").

So the identity bound to approval today is **the template's formal name in a
writable/system registry** — not any property of the executable content.

### How trust is granted, and how it persists across updates (the hole)

- `templates add --trust` records `trusted: true` on the new entry
  (`src/cli/templates.rs:285-392`, field set at `389`). No hook content is ever
  shown; `--trust` is a blind assertion.
- Re-adding keeps trust: `RegistryFile::add` does `entry.trusted |= old.trusted`
  (`src/registry.rs:190`; test `readding_keeps_aliases_and_trust`).
- `templates update` re-clones a followed branch to its newest commit, swaps the
  install directory in place, and calls `set_commit` — but **never touches
  `trusted`** (`src/cli/templates.rs:505-566`). The contract states this
  explicitly: "The formal name, aliases, and trust of an updated template are
  unchanged" (`command-line-interface.yml:55-56`).

**Consequence:** an update that changes a `script:` body or a hook `run` line
keeps the old trust. The next `apply <name>` runs the changed hooks with no
review and no `--trust`. This is the defect the design must close.

### Adapters that must keep working (interview stays UI-free)

The interview engine is pure (`architecture.yml` "interview-engine",
"drivers"): drivers call the engine; there is no prompt port. Trust is decided
outside the engine, in `plan`/`apply` and the binary. Every adapter reaches the
same gate:

- Interactive terminal apply (`src/main.rs` apply flow, `terminal.rs::drive`).
- `apply --dry-run` (`src/main.rs:1019-1039`): lists hooks, names `--trust` and
  `templates add --trust`, exits 0.
- `apply --answers FILE|-` headless (`src/main.rs:840-873`,
  `protocol::answer_headless`).
- Staged resume across processes/modalities (`resume_template`, `StagedRecord`).
- Crate callers: `Plan::apply(target, ApplyOptions, &dyn HookRunner)`.
- Exit code 3 = `NeedsTrust` (`architecture.yml` "outcomes-and-exit-codes";
  `src/main.rs:364-367`).

The library port is `HookRunner` (`src/hook.rs:23-25`), the only port; adapters
present, the library computes.

## Constraints carried into the design (Preserve / Change / Avoid / Risk)

**Preserve**
- Interview engine purity and all adapters (interactive/headless/staged/crate).
- Existing headless `add --trust` / `--trust` grant flows (no new interactive
  gate that a script cannot pass) — narrowing these is a supported-capability
  change requiring explicit approval.
- `--trust` one-run semantics; local layer cannot grant trust; discovered and
  address/folder invocations untrusted by default.
- Exit-code contract (3 = needs trust) and the dry-run listing contract.
- `HookRunner` as the one port; validation-at-boundary, trust-types-inside.
- Registry round-trip sparseness (`FieldPresence`).

**Change**
- Trust must bind to the reviewed **executable content**, not to the name alone.
- `templates update` must not carry stale trust when the executable content
  changed (`command-line-interface.yml:55-56` and `template-registry.yml:30`
  need edits).
- A review surface must show executable hook content and its changes.

**Avoid**
- Re-review on doc-only or unrelated updates (over-invalidation).
- Trust logic leaking across registry/plan/apply/cli (information leakage).
- Putting UI in the interview engine.
- A digest so broad (whole tree / commit) that any change forces re-review, or
  so narrow it misses a changed `script:` body.

**Risk**
- `run: [interpreter, in-tree-file]` executes an in-tree file whose bytes are
  not a declared `script:`; a content digest over declared surface will not see
  its body change. Needs an explicit policy decision.
- Hook-result syntax expansion (design 1078, opt-in Jinja hook results) adds
  fields to a hook node; review coverage and the digest must extend to new
  fields automatically or the expansion silently escapes review.
- Selected-template identity (formal name / root) intersects bundled-demo
  design 1074 and Jinja-context design 1075; the identity that binds approval
  must be the same identity those consume.

## Prerequisite designs

Per the epic board, 1069 is a design root with **no predecessor-design
prerequisite**; it precedes 1071 (trust management for installed templates) and
1075 (Jinja context). 1069 owns trust **policy** and the identity/digest that
binds approval; 1071 owns the trust-management **command syntax**. This design
must not settle 1071's command surface.
