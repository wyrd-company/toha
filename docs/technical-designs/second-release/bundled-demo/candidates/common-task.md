# Design task — Bundled offline toha-demo (Toha 0.2.0)

Toha is a Rust crate + binary scaffolding tool. Repository root: `/workspaces/worktrees/toha/design-bundled-demo`.
The current integration head is epic branch `epic/second-release` (already checked out in this worktree).

## The goal

Make `toha`'s small inspectable demo template resolvable and runnable **by name**,
**offline**, from **any directory**, on **first run**, with **no registry entry, no
cache, no network, no git setup**. Today the demo lives only at `docs/examples/demo/`
and ships inside the crate tarball (Cargo `include`); it is *not* compiled into the
binary and `toha apply toha-demo <dir>` in a clean environment fails with
"template not found". Close that gap.

The first-run path must cover, offline and from an arbitrary cwd:
- `toha apply toha-demo <dir>`
- `toha apply --dry-run toha-demo <dir>` (this is the "preview" path; there is no `preview` subcommand)
- `toha stage toha-demo <dir>` then later `toha continue` / `toha apply <PATH>` in a **separate process**, offline, producing identical output.

## Decisions your design must resolve

1. **Packaging/embedding** and one maintained source: the embedded demo must derive
   from exactly one source (`docs/examples/demo/`) so the embedded copy, the example,
   tests, and docs cannot drift. An enforcing, falsifiable check must exist.
2. **Discoverability, selected-template identity/root, and collisions.** Define how a
   bare `toha-demo` reaches the bundled demo without silently changing the meaning of
   an existing installed/aliased/discovered `toha-demo`. State the precedence rule
   explicitly (which wins, and why). If the design would *narrow* supported name
   resolution, that is a product decision — present it as an option with a
   recommendation, do not assume it.
3. **Selected-template identity** (`formal_name`, `commit`, root) must be stable across
   runs of one build and reconstructable offline from the staged identity alone, so a
   staged interview resumes in a later process. This identity is also the shared
   coordination surface consumed by the downstream defaults design (1073) and
   Jinja-context design (1075) — specify it.
4. **Verification & synchronization**: artifact/package verification, source↔embedded
   sync check.

## Hard constraints (from the feature contract)

- Preserve the `<TEMPLATE>` classification order (git URL → host: → folder → alias →
  short → formal) and **every** current name-resolution outcome, including an installed
  or aliased `toha-demo`.
- Preserve the pure interview engine and every supported path: terminal, `--async`,
  `continue`, staged, `apply --answers`, and crate callers.
- Preserve the trust boundary (hooks need trust; folders/discovered/local are
  untrusted), path safety (no write outside target, none into `.git`), and
  no-overwrite-without-`--force`.
- **Do NOT introduce** any capability needing separate approval: narrowing supported
  resolution, a new permission/access change, a new/changed timeout, a pinned-version
  check, or an application subprocess. Stay inside the existing capability envelope. If
  your shape seems to need one, flag it as an open question instead of assuming it.
- Do not leak the word/concept "embedded" into the pure engine, `Plan`, `apply`,
  `protocol`, or `staging`.
- Keep everything as a **design**: pseudocode, `not implemented` sketches, and proposed
  contract edits described in the design package. Do NOT commit runtime stubs or alter
  production code. Do NOT edit shared specification/schema files.

## What you must read first

- `/tmp/arena-bundled-demo/architect-SKILL.md` — the workflow you are inside. Read in full.
- `/tmp/arena-bundled-demo/runner-prompt.md` — the discipline you apply.
- `/tmp/arena-bundled-demo/rationale-template.md` — the shape of your rationale prose.
- `/tmp/arena-bundled-demo/grounding.md` — the Phase A traced model (DO NOT re-ground;
  build on it). It cites the exact files/symbols.

Then read the real code it points at, in the repository root above:
`src/cli/resolve.rs` (`resolve_template`, `resume_template`, `load_context`),
`src/source.rs` (`Address::parse`, `formal_name`, `install_key`), `src/registry.rs`
(`resolve`, the layer model), `src/cli/skills.rs` (the `include_dir!` + `export()`
embedding precedent), `src/staging.rs` (`StagedRecord`, `replay_with_defaults`),
`src/template.rs` (`Template::load`), `src/main.rs`, `Cargo.toml`, `build.rs`
(`TOHA_VERSION`), and the specs under `docs/specifications/`.

## Deliverable (write into your assigned isolated output directory)

Produce a complete candidate design package as markdown files in your output dir:

- `design.md` — caller's usage FIRST (README/quickstart + two or three concrete call
  sites), then data/type sketches, function signatures (Rust, with `not implemented`
  bodies / doc comments stating intent + invariants), a module/seam map (ASCII
  diagram), the error/results contract, and the resolution precedence rule stated
  explicitly. Cover all three first-run paths and the cross-process resume.
- `rationale.md` — shaped per `rationale-template.md` (Problem, Usage, Shape, Tradeoffs
  accepted, Alternatives considered, Open questions and risks, Next implementation
  step). Leave "Synthesis decision" empty (the orchestrator fills it).

Design the BEST version of *your* assigned direction. Do not hedge toward the other
candidates' shapes — divergence is the signal. State the explicit precedence rule,
the exact `formal_name`/`commit` identity scheme, and the source→embedded sync check.
Name where materialization writes and why that needs no new capability approval.
Use only generic, non-identifying example values.
