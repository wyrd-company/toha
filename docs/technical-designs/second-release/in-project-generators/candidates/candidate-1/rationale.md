# In-project generators — rationale (candidate: minimal CLI surface)

## The one decision that shapes everything

A generator is an **invocation pattern**, not a new noun. Toha already applies a
template into a subpath (`canonical_target` handles a not-yet-existing suffix),
already keys per-application identity by the target path, and — through task
1029 — already records each application as a snapshot keyed by `source` +
`target`. The only thing missing to "add another one like the last one" is a way
to **seed the new application's answer defaults** from an earlier application.
That is a defaults problem, and Toha already has a defaults boundary:
`Seed.defaults` / the default bank. So the feature collapses to one flag that
feeds that boundary. Everything else — identity, repetition, per-application
answers — already exists and is reused, not reinvented.

This is the interface-depth bet: hide the entire snapshot/gitoxide/ULID/ref
machinery behind the reader contract task 1029 already exposes, add one CLI
boundary function that turns a `Snapshot` into `IndexMap<Id, RawAnswer>`, and one
`Resolution` method that layers those over the configured defaults. The pure
engine — the thing every route and crate caller shares — sees nothing new. Its
input is still `IndexMap<Id, RawAnswer>`.

## Alternatives considered and rejected

### A new verb (`toha generate` / `toha add`)
Rejected. A verb would duplicate the entire five-step apply spine
(`setup → canonical_target → build_context → resolution → start → Plan::build →
apply_reporting`) or wrap it, and it would need its own route matrix, its own
result documents, and its own spec surface — all to do what `apply` already does
into a subpath. The only genuine delta is "seed defaults from a snapshot," which
is a flag's worth of behaviour. A verb also invites a *second* mental model of
identity ("a generated thing") when the snapshot `source`+`target` pair already
is the identity. More surface, no new capability.

### A `template.yml` generator/`kind` concept
Rejected, and the grounding warns against it explicitly. The schema is a closed
11-key set with `additionalProperties: false`, and "one source, one interview"
is baked into load. A `kind: generator` or a sub-template block would reopen the
template loader, force a migration of every template author's mental model, and
buy nothing: whether a template is "a generator" is a property of *how you call
it*, not of the template. The same `widget` template is a one-shot scaffold the
first time and a generator the second time. Encoding that in the template is a
category error.

### Overloading `--from`
Rejected — this is the sharpest trap. `--from` means "re-render into the *same*
target and three-way merge in place"; its whole contract is preservation of
operator edits. A generator writes a *fresh* application, usually into a *new*
subpath, and must **not** merge. Overloading `--from` to sometimes-merge-
sometimes-not, keyed on whether the target differs, would make one flag carry two
opposite contracts and would make the dangerous case (accidental in-place
overwrite) the easy typo. `--like` is a sibling flag with a disjoint contract:
different source axis, no merge, target free to differ, mutually exclusive with
`--from` (exit 2). Behavior B5's sole-kill ("require `target` to match") exists
precisely to prove the axes stayed separate.

### Replaying snapshot answers (the `--from` replay adapter) instead of seeding defaults
Rejected, and this is the subtlest decision. The 1029 replay adapter *submits*
recorded answers to drive the interview to completion, treats a rejected recorded
value as fatal, and submits batches whole. That is right for an update (you want
the old answers, changed only where the new version forces it). It is wrong for a
generator: the caller wants the old answers as a **starting point** they freely
override, and a value the evolved template now rejects should re-ask, not abort.
The engine already has exactly the right mechanism for "a value you can override
and that re-asks if invalid" — a **default**. Routing snapshot submissions to
`Seed.defaults` gets all of that for free, because `Pending::answer` already
auto-applies a batch's default when unanswered and, for a `Seed`-sourced default,
produces a clear rejection (re-ask / exit 4) when the value is invalid. Reusing
the replay adapter would have re-implemented override and rejection semantics the
default path already has, and coupled the generate axis to 1029's crate-private
adapter.

### Extending `DefaultBankEntry` to hold an ordered fallback (snapshot → configured → template)
Considered — the grounding even floats it — and rejected as unnecessary. The
precedence is *total per id*: a snapshot default totally wins over a configured
default for the same id. So the two never need to coexist in one entry; I resolve
precedence by **subtraction at the boundary** (snapshot ids shadow configured
ids, each id ends with exactly one bank entry). That keeps `DefaultBankEntry`'s
single-occupant invariant, keeps the pure engine byte-identical, and — critically
— keeps the no-`--like` path *exactly* today's `start_with_context` (configured
defaults keep their `Configured` provenance and their distinct invalid-default
message). An ordered-fallback entry would have changed the engine for every
caller to serve a case that never arises, and risked altering the configured-
default provenance that template-defaults' B12 driver-parity test pins down.

### Auto-prompting on every plain `apply` into a project with matching snapshots
Rejected. It would hijack plain `apply` — a one-shot scaffold into a fresh
directory would suddenly start asking "seed from a snapshot?" whenever the repo
happened to contain a like-sourced snapshot. Seeding is opt-in via `--like`; the
person picker fires only on a bare `--like`, never unbidden.

### Selector designs
- **`--seed-from <id>`** (runner-up naming). Clearer about mechanism and visually
  parallel to `--from`. Rejected *because* it is parallel to `--from`: a
  `--from` / `--seed-from` pair invites the exact confusion the grounding warns
  about. `--like` reads as natural intent ("another one like that") and is
  visually distinct, which is the safer disambiguator between the two axes.
- **Always require an explicit id, no `latest`.** Rejected: the dominant case is
  "like the last one," and forcing the caller to look up a ULID for it is hostile
  on every route. `latest` (newest snapshot of this source) covers it
  deterministically and non-interactively, so scripts and agents get the ergonomic
  path too.
- **`latest` = 1029's likely-base.** Rejected as the *definition*; likely-base is
  an ancestry heuristic meant for choosing an update base, not "the most recent
  application." For seeding I define `latest` as the newest snapshot of the source
  (max ULID) — unambiguous and independent of git history shape. Likely-base is
  reused only to *preselect* in the person picker, where a heuristic hint is
  appropriate.

## Key decisions and tradeoffs

- **Identity is derived, not declared.** A generator application's identity is
  the snapshot `source` + `target` pair that already exists; two subpaths are two
  snapshots. No application id, registry, or count. Tradeoff: there is no
  first-class "list the applications of this generator" object — but that is
  precisely `snapshots list` filtered by source, which 1029 already provides.
- **Persist a selector, not answers.** The staged record gains `seed:
  Option<SnapshotId>` so resume rebuilds the same defaults by re-reading git. It
  stores a pointer, never prior-application answers, so it honours "no second
  persisted-answers lifecycle." Tradeoff: if the referenced snapshot is removed
  between `stage` and `apply`, resume fails clearly (exit 1) — correct, because
  the seed source genuinely vanished.
- **Reader-only, and the axes stay apart.** `--like` consumes `Project`/`Snapshot`
  and never capture, merge, `--from`, or the `snapshots` commands. `StagedRecord.
  seed` is orthogonal to 1029's `StagedRecord.base`, and the flags are mutually
  exclusive, so the two concurrent tasks compose without field or code-path
  conflict.
- **Fresh instant.** A generator is a new identity, so it takes a normal `now`
  and its own `generated`; it does not inherit the seed snapshot's frozen instant
  (behaviour B6). Only answer defaults flow from the snapshot — nothing else.
- **Snapshot source must match.** Seeding across sources would be implicit answer
  reuse by matching ids, which the grounding forbids. `--like` requires
  `snapshot.source == template source identity` (by `source`, never the `@ref`-
  bearing formal name), and refuses otherwise.

## Where this pushes back on the existing shape

Only one library seam is added (`Resolution::start_with_seed`), and it is defined
so that the empty-seed case is literally the current `start_with_context`. The
pushback is deliberately minimal: the design refuses to touch the pure engine,
`DefaultBankEntry`, `Seed`, `Plan`'s single-target contract, the exit-code
vocabulary, or the template schema. The one boundary it does add
(`src/cli/like.rs`) is the single place snapshot types live, mirroring the
codebase's own rule — parse external input once at its boundary into domain types
— so `toha::snapshot` stays as contained on the generate axis as
`docs/.../project-updates` keeps gitoxide contained on the update axis.

## Interface-depth argument

Public surface added: one flag (`--like [SELECTOR]`), one optional result member
(`seed.from`), one optional staged member (`seed`), one crate-private boundary
function (`resolve_like` + `fold_submissions`), and one `Resolution` method. That
surface hides: the entire snapshot store (refs, no-parent commits, ULIDs,
schema validation, invalid-snapshot refusal), the selection/ambiguity logic, the
submissions-to-defaults fold, and the total precedence across snapshot /
configured / template / route. The precedence — the genuinely hard part — never
reaches the engine: it is resolved by subtraction before the bank is built, so
the engine keeps its single-occupant invariant and every existing adapter keeps
producing identical results for identical inputs. Complexity in; a flag and one
method out.
