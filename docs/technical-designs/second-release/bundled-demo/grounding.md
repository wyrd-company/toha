# Bundled offline toha-demo — Phase A grounding

Traced model of the subsystems the bundled-demo design touches, with file:symbol
evidence. Produced by the `how` trace over resolution, registry, source,
staging, plan, apply, hooks, interview, and jinja, plus a `why` reading of the
resolution/trust ownership the design changes.

## Overview

Toha resolves a `<TEMPLATE>` argument to a `ResolvedTemplate`, loads that folder
into a `Template`, runs the pure interview engine, builds a `Plan`, and applies
it. The demo template exists as `docs/examples/demo/` and is listed in the crate
`include` set, so it ships in the published crate tarball — but it is **not**
compiled into the binary and **not** resolvable by name. `toha apply toha-demo`
in a clean environment fails with `template not found`. Closing that gap, offline
and from any directory, without registry/cache/git setup, is the task.

## Key concepts

- **`Address`** (`src/source.rs:8`) — `parse(arg, hosts, cwd, home)` classifies a
  `<TEMPLATE>` into `Git`, `Folder`, or `Name`. Order is a documented contract
  (`command-line-interface.yml`): `://`/`git@` git URL → `host:` shorthand →
  `.`/`/`/`~`/drive folder → otherwise `Name`. A bare `toha-demo` is `Name`.
- **`ResolvedTemplate`** (`src/cli/resolve.rs:16`) — `{ formal_name, commit,
  folder, trusted, named }`. This is the epic's shared *selected-template
  identity/root* surface consumed by demo (1074), defaults (1073), and Jinja
  context (1075).
- **Registry** (`src/registry.rs`) — layered `templates.yml`: `System`, `User`,
  `Local` (aliases only), plus `Discovered` (folders scanned from
  `templates_paths`). `resolve(name)` matches alias → short name → formal key.
  Precedence: local over user over system; discovery adds folders not already
  present.
- **`Template`** (`src/template.rs:42`) — parsed from a folder; retains
  `root: PathBuf` and `source_dir: PathBuf` and root-relative file/script paths.
  It does **not** copy body-file content into memory at load.
- **Trust** — `ApplyOptions.trusted` gates hooks (`src/apply.rs:82`). It flows
  from `ResolvedTemplate.trusted`, which is registry trust for named user/system
  entries, and is false for folders, discovered templates, and local-layer
  entries. `--trust` overrides per run.
- **Staging** (`src/staging.rs`) — `StagedRecord { target, template(=formal),
  commit, named, now, submissions }`. Resume reconstructs the folder from
  `(template, commit)`; replay re-runs the engine and does **no** content
  fingerprint check.

## How it works (traced flow)

1. `resolve_template(arg, …)` (`resolve.rs:175`) parses the address.
   - `Folder` → returns the canonical folder as both `formal_name` and `folder`,
     untrusted, unnamed.
   - `Git` → registry entry if present, else `fetch_new` clones into
     `cache/sources/<install_key>/<commit>`.
   - `Name` → `registry.resolve(name)` → entry; **`NotFound` today for
     `toha-demo`**.
2. `Template::load(&resolved.folder)` (`template.rs:709`) reads
   `root/template.yml`, resolves `!include` at load (confined to root), and
   records `source_dir` (default `root/template`).
3. `Interview::start(&template, seed)` (`interview.rs:1266`) runs the pure engine
   over `template.interview`; **no filesystem access** — the folder is irrelevant
   once parsed.
4. `Plan::build(&template, &completed, path)` (`plan.rs:179`) **walks
   `template.source_dir` on disk** and reads each body file: rendered files are
   held in memory (`Content::Rendered`), static files store the on-disk path
   (`Content::Copied`), and `files` rules read `template.root.join(source)`.
5. `apply_reporting` (`apply.rs`) writes rendered text from memory, but **re-reads
   static files with `fs::copy(source, …)`**, reads **per-file permission via
   `fs::metadata(&file.source)`**, and runs **script hooks from
   `template.root.join(path)`**.
6. Staging: `record()` stores `formal_name`/`commit`/`named`. `resume_template`
   (`resolve.rs:224`) rebuilds the folder: empty `commit` ⇒ `formal` is an
   absolute path; else a registry entry with matching commit; else a git fetch at
   that commit (with a `fetched.commit != commit` guard).

## The load-bearing constraint

**The template root must physically exist as a real folder at both build time and
apply time.** Build walks the source dir; apply re-reads static files, reads
permission metadata per file, and executes script hooks from the root. An
in-memory or virtual filesystem source would require reworking `Plan`, `apply`,
`Content::Copied`, and the permission path. Therefore the bundled demo must be
**materialized to a real folder** before build/apply, and that folder must persist
(or be re-materialized idempotently) between a `stage` process and a later
`apply`/`continue` process.

## Embedding precedent

`src/cli/skills.rs` embeds a directory with
`include_dir!("$CARGO_MANIFEST_DIR/skills")` and its `export()` (`skills.rs:110`)
materializes an embedded tree to disk with create-new writes and blocking checks.
This is the direct model: embed `docs/examples/demo/` the same way and reuse the
materialization pattern.

## Where things live

- Resolution: `src/cli/resolve.rs` (`resolve_template`, `resume_template`,
  `load_context`).
- Address parsing/identity: `src/source.rs` (`parse`, `formal_name`,
  `install_key`).
- Registry/layers/resolve: `src/registry.rs`.
- Embedding precedent: `src/cli/skills.rs` (`include_dir!`, `export`).
- Template parse: `src/template.rs`. Plan/apply: `src/plan.rs`, `src/apply.rs`.
  Hooks: `src/hook.rs`. Staging: `src/staging.rs`.
- Contracts: `docs/specifications/command-line-interface.yml`,
  `template-registry.yml`, `template-format.yml`. Canonical design:
  `docs/technical-designs/architecture.yml`.
- Demo source: `docs/examples/demo/` (`template.yml` has two text questions,
  `title` and `topic`; `template/note.txt`; no hooks). Version: `build.rs` →
  `TOHA_VERSION`.

## Constraints (Preserve / Change / Avoid / Risk)

**Preserve**
- The `<TEMPLATE>` classification order and every current name-resolution
  outcome, including an installed or aliased `toha-demo`.
- The pure interview engine and the terminal / `--async` / `continue` / staged /
  `apply --answers` / crate paths.
- The trust boundary: hooks need trust; folders/discovered/local are untrusted.
- Path safety (no write outside target, none into `.git`) and
  no-overwrite-without-`--force`.
- One maintained source for the demo (`docs/examples/demo/`).

**Change**
- Add a way for a reserved name to resolve to a binary-embedded template,
  materialized to a real folder, fully offline.
- Extend `resume_template` so a staged bundled-demo interview reconstructs its
  folder from identity alone, offline.
- Pin the bundled demo's `formal_name` and `commit` as the shared identity for
  downstream designs.

**Avoid**
- Any capability that would need separate approval: narrowing supported
  resolution, new permission/timeout/pinned-check, or an application subprocess.
- Leaking "embedded" into the pure engine, `Plan`, `apply`, `protocol`, or
  staging.
- A second copy of the demo content that can drift from the source.

**Risk**
- Materialization races between concurrent processes writing the same folder.
- Identity instability (a `commit` that changes per build breaks a resume across
  an upgrade — acceptable if it degrades to a clear "cannot resume, start over"
  message via existing `replay_failed` guidance).
- Collision policy is a product decision (Bob): fallback-only vs reserved-name.
