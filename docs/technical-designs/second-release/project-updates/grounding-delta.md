---
relationships:
  depends-on:
    - content-injection
    - headless-recovery
    - hook-results
  informs: project-updates
---

# Grounding delta — refreshed base

`grounding.md` traces the base the arena candidates read: `2c4a197`. The
candidates keep that pinned input. This file records what the refreshed design
base, `epic/second-release` @ `b97da39`, adds, and the facts that synthesis uses
beyond the pinned grounding.

## Content-injection runtime on the base

The base now contains the content-injection runtime. Its producer symbols agree
with the approved design (`content-injection/05-design.md`, SHA-256
`b01b442917737c457dc92d581833b9f67c1f37abddc8638440bacf965380a006`, unchanged):

| Approved contract | Base symbol |
|---|---|
| `Plan.edits: Vec<PlannedEdit>` | `src/plan.rs:103-114` |
| `PlannedEdit::{Region, JsonValue}` | `src/inject.rs:28-31` |
| `PlannedRegionEdit { path, region, body, marker, anchor, create, source }` | `src/inject.rs:45-58` |
| `PlannedJsonEdit { path, json_path, desired, format, create }` | `src/inject.rs:62-68` |
| `EditResolution::{Unchanged, Write, Drift { forced }}` | `src/inject.rs:376-385` |
| `resolve_region_edit`, `resolve_json_edit` (pure) | `src/inject.rs:454`, `src/inject.rs:724` |
| `FileMutation::{Whole, Region, JsonValue}` | `src/plan.rs:759-773` |
| `Plan::mutations()` | `src/plan.rs:740-756` |

The base adds helpers that are outside the approved producer interface. This
design does not depend on them as contract: `EditReport` and `report_*` dry-run
classification (`src/inject.rs:389`, `src/inject.rs:466`, `src/inject.rs:735`),
`PlannedEdit::path` (`src/inject.rs:34`), and `JsonPath::overlaps`
(`src/inject.rs:317`). `FileMutation` derives `Clone, Copy, PartialEq, Eq`, so
old-plan and new-plan identities compare directly.

Apply resolves every edit before the first write and commits edited targets
through a sibling temporary file and rename (`src/apply.rs:350-470`,
`tempfile::NamedTempFile::persist`). Whole-file rules keep the existing conflict
gate (`src/apply.rs:151-160`).

The recovery design (`headless-recovery/design.md`) is approved at the base, and
its runtime is not implemented yet. This design consumes the approved caller
routes and the `{template, answers}` identity contract, not the current
command behavior.

## Merge engine available in an existing dependency

`gix` 0.87.1 is a dependency (`Cargo.toml`). Its optional `merge` feature adds
`gix-merge` ^0.20.1, together with `blob-diff` and `attributes`, and re-exports
the blob merge as `gix::merge::blob`, which has a built-in text driver
(`gix-0.87.1/Cargo.toml` `[features] merge`; `gix-0.87.1/src/merge.rs:2,108`).
The feature is not enabled at the base, and `gix-merge` is not in `Cargo.lock`.
`imara-diff` 0.1.8 is already in `Cargo.lock` through `gix-diff`.

## Prior art

Two established tools implement project update from a template:

- **copier** records `_src_path`, `_commit`, and the answers in the project-root
  file `.copier-answers.yml`. Its update regenerates the old version, computes
  the operator's diff, regenerates the new version, and reapplies the diff. It
  starts `git` subprocesses (`git apply`, `git merge-file`) to do this
  (`copier/_main.py:383,1388-1690`).
- **cruft** records `template`, `commit`, and `context` in the project-root file
  `.cruft.json`, and applies the old-to-new template diff with a `git apply`
  subprocess (`cruft/_commands/update.py:70-229`).

Both put the record in a project-root file that is committed with the project.
Both use a git subprocess to merge. That merge method is what this design must
avoid without explicit approval.
