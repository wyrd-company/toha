---
relationships:
  depends-on: project-updates
  informs: project-updates
---

# Verification — project updates

This file checks `design.md` and `design.yml` against the task, the approved
prerequisite designs, and the code at `epic/second-release` @ `b97da39`.

## Prerequisite revisions consumed

| Prerequisite | Artifact | SHA-256 at base | Last commit |
|---|---|---|---|
| Content injection | `content-injection/05-design.md` | `33f2f943e9a1b0165c690d1779499b8ea1c69130eb10f30e11f71ab7b0229eec` | `8a9ad067b185fa1db35834796025ff32ff157fe6` |
| Caller routes and answers identity | `headless-recovery/design.md` | `a7e081784604e78c8349baa9f33fdb456c615074c2319d7f2eb6d1724b409ed5` | `2c4a197c82a64605c1659ddf2aceea6863e526da` |
| Hook results | `hook-results/design.md` | `c3be45a767bcb98046d7130f89bf09a9ecd42575216700d88a4f1b9bac4b8d96` | `973b50ee7a9b4d6be3a9840411cc5a16bff27aa1` |

The candidates read `05-design.md` at SHA-256
`b01b442917737c457dc92d581833b9f67c1f37abddc8638440bacf965380a006`, which is
unchanged from `2c4a197` to `b97da39`. The accepted as-built alignment at
`8a9ad06` adds two statements and changes no interface or ownership rule:

- A region body is an inline `content` template or a `source` support file,
  and both render at plan time. Update consumes the rendered
  `PlannedRegionEdit.body`, so the origin does not matter.
- A `Json` target is validated with `serde_json::from_str` before any CST edit
  (`src/inject.rs:779-790`). Update inherits this through the JSON resolver.
  The design's crate-private JSON retraction applies the same check (behavior
  9 and its sole-kill guard).

The recovery and hook-result designs are unchanged across all three commits.

## Task decisions

| Task requirement | Where the design resolves it |
|---|---|
| Record shapes compared; formal name, resolved commit, answers, target, versions; travels with the project | "The applied record": carrier comparison (`design.yml` `record-carrier`), content table. **Target** is the directory that holds `.toha/`, derived rather than stored so that a moved project stays correct. **Version** is the resolved commit per applied template, the only template version identity (`grounding.md`). The generation instant pins time-dependent renders. |
| Commit-trailer and custom-ref alternatives assessed against current evidence | `design.yml` `record-carrier`; `candidates/candidate-1.md` Decision 1; prior art in `grounding-delta.md` (both comparable tools use a project-root file) |
| Old-template/old-answer and new-template/same-answer rendering | "The merge base", "Answers" |
| Project update merge behavior and user-edit conflicts | "Per-identity rules", "Conflicts" |
| Adoption of projects without records by interviewing again | "Adoption" |
| Injection mutation and idempotency | Region and JSON value tables; "Order of effects"; behaviors 3, 6–9 |
| Hook-result availability and effects | Hooks run in the existing loop after files; results are not recorded; `hooks-on-update` decision; behaviors 15–16 |
| Staged recovery without conflating applied records with staged interviews | Record is project-resident and holds a completed apply; update and adopt never stage and refuse while an interview is staged; behavior 17 |
| Interfaces, data shapes, failure and recovery contracts | "Public interfaces", "Data structures", "Errors and results", "Order of effects" (crash recovery by record-last) |
| Fixture moving A to B with an edited-file merge conflict | Behavior 1 (`update-merge-conflict`) and the fixture harness shape under "Compatibility and canonical documents" |

## Constraints

| Constraint | Result |
|---|---|
| Interview engine pure and UI-free | No engine change. Removed-question filtering happens before replay, in the project module. |
| Every adapter keeps working | Terminal, headless, staged, direct, and crate apply keep their signatures and routes. Apply adds the record write. Behavior 23 compares person, scripted, and crate updates. |
| No production stubs, runtime, or shared canonical edits | Only files under `docs/technical-designs/second-release/project-updates/` change on this branch. |
| Generic, non-identifying examples | Sample template `forge:catalog/receipt@stable`, target `./sample-service`, generic file names. |
| Application subprocess | None. Only `gix::merge::blob::builtin_driver::text::merge` is called. The `gix-merge` driver platform, which can start external drivers through `gix-command`, is never used. Behavior 24 and its sole-kill guard enforce this. |
| New permission or access rule | None. Fetching a template commit uses the existing gitoxide fetch that address and registry templates use. Environment values keep the existing trust rule. |
| New or changed timeout | None. |
| Pinned-version check | None. The record's commit is an identity parsed for syntax. No toha version is recorded or compared. |
| Supported-capability restriction | **One**: template output under `.toha/applied/` fails at planning. Disclosed for separate approval (`reserve-record-path`). The refusals of update and adopt while an interview is staged, or while Toha markers remain, apply only to the new commands and remove no existing capability. |
| New effect on existing commands | Every apply writes `.toha/applied/<name>.json` into the project (`apply-always-records`). |

## Composition with prerequisite contracts

- **Content injection.** Update consumes `Plan::mutations()` and the
  `FileMutation::{Whole, Region, JsonValue}` identities at `src/plan.rs:740-773`,
  and the pure resolvers at `src/inject.rs:454` and `src/inject.rs:724`. Region
  drift outside update still refuses. JSON values stay convergent with no
  three-way merge. Retraction and merged-region bodies are the removal and merge
  behaviors that injection assigns to this design (`05-design.md:383-388`). They
  are crate-private helpers, so the public producer interface is unchanged. The
  identity join sorts on a private key built from `TargetPath::as_path`,
  `RegionKey::as_str`, and `JsonPath::segments`, so no injection type gains a
  trait.
- **Caller routes and answers identity.** Update answers documents keep the
  `{template, answers}` envelope and the exact-equality gate. The update route's
  expected identity is the selected application's `template`, which adds a row
  for a new route and changes no existing row. `parse_and_verify` belongs to the
  recovery runtime, which is not implemented at the base. The paired
  implementation already sequences after that runtime, so the dependency graph
  needs no change. Adding a target or commit binding to answers documents stays
  out of scope. The binding lives in the applied record.
- **Hook results.** Base renders run no hook. File bodies cannot read hook
  results, so the base does not depend on hooks. The next template's hooks run
  in the existing loop, and results are dropped at return.

## Code facts the design depends on

| Claim | Evidence |
|---|---|
| Answers are formatted before they reach `Completed.answers` | `src/interview.rs:1209-1223` (`format_answer`) |
| Staging stores accepted raw submissions and replays them | `src/staging.rs:31`, `src/staging.rs:381-452`, `src/main.rs:768` |
| The engine rejects an answer id that is not a question | `src/interview.rs:1952` |
| A render reads seventeen reserved context names, including host facts and a trust-gated environment snapshot | `src/context.rs:37-56` |
| Edited targets are committed by temporary file and rename | `src/apply.rs:419-470` |
| `gix` 0.87.1 `merge` feature adds `gix-merge` ^0.20.1 | `gix-0.87.1/Cargo.toml` `[features] merge` |
| `builtin_driver::text::merge(out, input, labels, current, ancestor, other, options) -> Resolution` with `ConflictStyle::{Merge, Diff3, ZealousDiff3}` | `gix-merge-0.20.1/src/blob/builtin_driver/text/{function.rs:246, mod.rs:8-95}`, `src/blob/mod.rs:19-27` |
| `.toha.yml` is the local configuration file | `src/config.rs:164` |

## Usage agrees with the sketch

- `ProjectRecord::open(&CanonicalTarget)`, `application`, `begin_update`,
  `answer_headless` on `Begun.interview` and `Begun.answers`, `Begun::plan`,
  `UpdatePlan::{changes, apply}`, and `Updated::{UpToDate, Written,
  NeedsTrust}` appear with the same signatures in "Caller usage" and "Public
  interfaces".
- Refusal messages name commands in canonical form, options first:
  `toha update --on-conflict markers ./sample-service`.
- `adopt --at 3f9a1c2` accepts a commit-ish and records the resolved full commit,
  because the record's `commit` must be 40 hex.

## Pre-mortem

| Failure | Guard in the design |
|---|---|
| Replay produces different answers, so the base never verifies and every update falls to fingerprint mode | Raw submissions, a filter for removed questions, and the recorded instant; behaviors 12–14; a flow-heavy fixture |
| A forged or stale record deletes operator files | Retract needs a verified base; behavior 5 |
| An old commit is gone upstream | Fingerprint mode never fails and never deletes; `adopt --at --replace` sets a new baseline; behavior 10 |
| Conflict markers are committed and merged again as content | Marker preflight; behavior 18 |
| A crash leaves files at B and the record at A | Record-last commit point; behavior 15 |
| Updates rewrite date-bearing files every time | `generated` instant; behavior 14 |
| The merge engine starts an external driver | Direct built-in text call; behavior 24 |
| Two templates in one project fight over a file | Per-template record files and `OwnershipCollision`; behavior 20 |

## Known gaps

- The `https://refinery.systems/ontology/technical-design` schema is not
  available in the workspace. `design.yml` follows the top-level keys of the
  repository's other technical designs (`architecture.yml`,
  `hook-results/design.yml`).
- `grounding.md` has one 81-character line. It is kept byte-identical because it
  is the recorded, hash-pinned candidate input. Candidate files and the judge
  output are preserved verbatim as evidence. The judge copy has a lint directive
  prepended, and the raw output hash is
  `561fa505411cd5bcd07163024c94c9a45d2f5d43746d8e2c507c1c663f4e8a05`.
- The exact `gix-merge` call is verified against the published 0.20.1 source,
  not compiled. The paired implementation's first step compiles it behind the
  `merge` feature.
