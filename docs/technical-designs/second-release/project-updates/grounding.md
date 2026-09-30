---
relationships:
  informs: project-updates
  references:
    - content-injection
    - headless-recovery
    - hook-results
---

# Grounding — project updates

Code facts are at `epic/second-release` @ `7a966c6`. Paths are relative to the
repository root. The gitoxide evidence comes from the spike in `spike/`
(`cargo run --offline` from that directory; it needs `git` and uses `strace`
when present).

## Apply today

- CLI: `Apply` (`src/main.rs:268-286`) takes one or two operands,
  `-A/--answers`, `-f/--force`, `-d/--dry-run`, `--trust`. `Stage`
  (`src/main.rs:210-230`) takes `-a/--async [FILE]` and `--trust`. `run()`
  (`src/main.rs:1621`) sends a template with `--answers` to `scripted()`
  (`src/main.rs:1347`, dispatched at 1633), refuses `apply PATH --answers`
  (1650-1659), takes the direct person route when nothing is staged (about
  1695), and the staged `apply PATH` route otherwise (about 1760), replaying
  with `replay_with_resolution` (1785).
- The staged-interview refusal is `staged_refusal` (`src/main.rs:704-734`);
  `canonical_target` is `src/staging.rs:164`.
- `Plan::build(template, completed, target)` (`src/plan.rs:391`) produces
  `files`, `edits` (region and JSON value), `conflicts`, `hooks`, and messages.
  `Plan::mutations()` (`src/plan.rs:740`) gives the Whole, Region, and JsonValue
  identities. Hooks and the after-apply message that read hook results are
  deferred (`Planned::AfterHooks`) and render in the apply loop.
- `Plan::apply` / `apply_reporting` (`src/apply.rs:144`, `155`) refuse existing
  files without `force` before writing (162-172), return `NeedsTrust` for
  untrusted hooks (173-175), resolve edits in memory (217), write whole files
  (221-273), commit edited targets (275-281), run hooks (282-346), and render
  `after_apply` (348-354). `ApplyOptions { force, trusted }` (`src/apply.rs:19`).
- Trust: `evaluate_trust` (`src/review.rs:140`) ORed with `--trust`
  (`src/main.rs:1857-1885`); the scripted route checks trust in
  `scripted_completed` (1486-1488); environment access is `apply_environment_grant`
  (118-125). `HookRunner` is the only port (`src/hook.rs:51`).
- Template identity: `ResolvedTemplate { formal_name, commit, folder, … }`
  (`src/cli/resolve.rs:18-33`). A folder template has an empty commit
  (`src/cli/resolve.rs:202`). Template clones are full history
  (`src/source.rs:314`), cached per commit (`src/source.rs:203`).
- The target is never opened as a git repository; gix is used only in
  `src/source.rs`.

## Interview and answers

- `StagedRecord` (`src/staging.rs:24-38`) holds the private target, the formal
  name, commit, `named`, `now`, raw `submissions`, and the invocation context.
  It has no schema file. It is stored at `<state>/<sha256(canonical
  target)>.json`.
- Replay is `StagedRecord::replay*` into `replay_from` (`src/staging.rs:381-452`),
  one `Pending::answer` per submission (440).
- `format` runs in `check_answer` before an answer is stored
  (`src/interview.rs:1192-1209`), so replay must use raw submissions.
- Unknown answer ids are rejected (`src/interview.rs:1945-1947`), so replay must
  filter submissions to the rendered template's question ids.
- `answer_headless` (`src/protocol.rs:521`) returns `Headless::{Completed,
  Pending, Ended}` (505). Identity checking is `parse_and_verify(expected_template,
  text)` (`src/protocol.rs:383`, private) behind `answer_document_headless`
  (491) and `answer_document_once` (466).
- The scripted route emits one result document: `questions` (exit 4), `ended`
  (0), `planned` (0 for a dry run, 3 when untrusted), `applied` (0), and `error`
  with a kind (1, or 5 when ambiguous); builders in `src/protocol.rs:142-319`,
  emission in `src/main.rs:1227-1571`. Apply's exit codes are 0, 1, 3, 4, and 5
  (`docs/specifications/command-line-interface.spec.yml:203-237`); clap exits 2
  for an invalid command line.

## Terminal

`inquire` 0.9.4 is the prompt crate (`Cargo.toml:45`); `MultiSelect` is already
used (`src/cli/terminal.rs:6`, `99-104`), so the snapshot selection list needs a
thin wrapper only.

## gitoxide spike

The spike uses gix 0.87.1 with Toha's features plus `merge` and `status`, which
enable `blob-diff` and `dirwalk` and add `gix-merge` 0.20.1, `gix-imara-diff`,
`gix-status` 0.34.1, and `gix-dir` 0.29.1.

| Need | Result | API |
|---|---|---|
| Target directory clean against `HEAD` | Proven | `repo.status(..).untracked_files(Files).index_worktree_rewrites(None).into_iter([":(top)<target>/"])`; ignores ignored files and changes outside the target |
| Checkout of `HEAD` outside the repository | Proven | `repo.index_from_tree`, `repo.checkout_options(Source::IdMapping)`, `gix::worktree::state::checkout`; no `.git` created, no linked worktree |
| Parentless snapshot commit and ref | Proven | `repo.write_blob`, `repo.edit_tree(empty).upsert(..).write()`, `repo.commit_as(.., "refs/toha/snapshots/<id>", .., tree, [])`; a parentless commit requires the ref to be absent |
| List and delete snapshot refs | Proven | `repo.references()?.prefixed("refs/toha/snapshots/")`, `find_reference(..)?.delete()` |
| Ignore check for captured paths | Proven | `repo.excludes(&index, None, WorktreeThenIdMappingIfNotSkipped)` then `at_path(..).is_excluded()` |
| Metadata outside the merged tree | Proven | Tree `{ snapshot.json, files/ }`; `tree.lookup_entry_by_path("files")`, empty tree when absent |
| Three-way tree merge, eight path cases | Proven | `repo.merge_trees(base, ours, theirs, labels, options)` with snapshot `files/` grafted into `HEAD`'s tree at the target; diff3 set through `gix_merge::tree::Options` (`blob_merge.text.conflict = Keep { style: Diff3, .. }`), `rewrites = None` |
| Worktree and index with conflict stages | Proven | checkout of changed paths with `overwrite_existing`, file removal for deletions, `repo.open_index()`, replace target entries, `sort_entries()`, `remove_tree()`, `index_changed_after_applying_conflicts(.., RemovalMode::Prune)`, `index.write(..)`; `git status` shows `UU` and `DU`, plain `git commit` and `git write-tree` refuse |
| No process started | Proven for the spike fixtures | `strace -f`: one `execve` per operation, no fork, vfork, or process clone |

Gotchas the implementation must keep:

- Decide conflicts with `is_unresolved(TreatAsUnresolved::git())`; a text
  conflict with markers reports `resolution = Ok`.
- `remove_tree()` after replacing index entries, or git commits a stale cached
  tree.
- `git commit -a` does not check for conflicts, after Toha or after `git merge`.
- gitoxide runs an external merge driver when `.gitattributes` names
  `merge=<name>` and `merge.<name>.driver` is configured
  (`gix-merge` `blob/platform/merge.rs:419-422`), and filter drivers (for
  example git-lfs) for `filter=` attributes during conversion and checkout.
  The spike fixtures had none. The design refuses to run either; the
  implementation proves it with a driver fixture under the process guard.
- Without the `parallel` feature, status runs single-threaded.
