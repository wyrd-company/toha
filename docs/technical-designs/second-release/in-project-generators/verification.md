# Verification — in-project generators

Phase F. The synthesized design (`design.md`) is checked against the caller
usage, every task decision and constraint, the failure cases and the mandated
fixture, the consumed producer revisions, and the cross-judge's contract flags.

## Caller usage reconciled against the sketch

- Every call site in `design.md` §Caller usage resolves through the sketched
  types: the person `--like latest` run uses `latest(candidates(...))`; the
  scripted `--like <prefix>` run uses `find_required`; the agent `stage --like`
  pins `StagedRecord.seed`; the fallback runs omit the flag and never construct a
  `SnapshotSeed`. The `applied`/`questions` `seed` member is produced only when a
  seed applied. No call site needs a type the sketch does not expose, and no
  snapshot/gitoxide type appears past `src/cli/like.rs`.

## Task decisions and constraints, each checked against the design

- **Repeated applications at subpaths, own answers, own identity** — an apply
  per subpath, identity derived as `(source, target)`; the twice-in-one-project
  fixture and behaviors 5/17 prove distinct applications. ✔
- **Explicit access to snapshots as optional defaults (policy A)** — `--like`
  seeds; absent → configured → template → questions; required-absent → exit 1;
  behaviors 2, 8, 14, 15. ✔
- **No independent second persisted-answers lifecycle** — only a pinned snapshot
  id in the staged record; answers re-read from git; behavior 19. ✔
- **Never infer identity by shared question id** — seeding is confined to one
  `source`; cross-source refused; behavior 4. Configured `template-defaults` /
  `{ preset: <name> }` unchanged. ✔
- **Generator CLI/template syntax, selection, collision/ambiguity, cross-snapshot
  precedence, compatibility with configured defaults** — §Selection and the
  error table; one snapshot seeds (newest of source); `--like`/`--from` mutually
  exclusive; precedence total per id with configured defaults; behaviors 2, 3,
  10, 16. ✔
- **Falsifiable fixture: same generator twice, different answers, one project** —
  the mandated fixture, with a sole-kill per load-bearing rule. ✔
- **Preserve pure engine + all adapters** — one additive private bank variant and
  one seam; no-`--like` path byte-identical; route parity; behaviors 10, 21. ✔
- **Reuse the single-target apply spine; `Plan` single-target** — behavior 18. ✔
- **Slot into exit 0–5; scripted route by `--answers`** — the error table maps
  every case into 0–5; snapshot ambiguity is exit 1, template-name ambiguity
  stays exit 5. ✔
- **No new permission/timeout/pinned-check/subprocess; no capability removed** —
  §Out of scope and `design.yml` disclosures; plain apply/stage unchanged. ✔
- **Design only; no premature shared spec/schema/runtime edits** — all proposed
  contract edits are described for the paired implementation (task 1030);
  `template-format.schema.yml` is unchanged. ✔
- **Generic, non-identifying examples** — `gh:example/widget`, `alpha`/`beta`,
  generic ids. ✔

## Failure cases and the fixture, traced

The error table's every row maps to an exit code and a written/nothing-written
outcome; the mandated fixture asserts the second application inherits the
unspecified answers, the first is unchanged, both snapshots exist with equal
`source` and different `target`, and the three routes agree. The 21 behaviors
each carry a sole-kill; wrong-kind (8) vs constraint-invalid (9) vs missing (12)
are proven separately, per the cross-judge's required revision.

## Compatibility with the consumed producer revisions

The design consumes project-updates **revision 3** (Bob-approved) with
completeness correction **1110**, integrated at the epic base
`e0d38d41d5a1c0e9eb4736ca21571b449938e0e6`; producer `design.md` SHA-256
`0a780e0fcb440b4e632751cf34cd812fd4a98929afc11e3d1fad50ebb83d0b8b`, `design.yml`
SHA-256 `782759dd173a314b1cd8f1165d911b2f54447b9360be6d0c4f423bce7c92e4b2`
(as recorded on the task). Only the `toha::snapshot::{Project, Snapshot}` reader
(`source`, `target`, `submissions`, `id`, `created`, `snapshots`, `find`,
`likely_bases`) is used; every write path (capture, merge, replay, snapshots/init)
is left to task 1029. The generate apply relies on the producer's own capture, so
the two tasks compose: 1029 writes the snapshot, this design reads it.

## Cross-judge contract flags — disposition

1. **Update-replay mischaracterization** — fixed: `synthesis.md` and `design.md`
   state the reason not to reuse the update adapter is ownership and update
   semantics, not a false "fatal on rejects". Resolved.
2. **Whole-run "never writes a ref" would disable capture** — fixed: "reader-only"
   is scoped to seed resolution; the generate apply preserves producer capture
   (behavior 6). Resolved.
3. **Invented source-filtered `snapshots list`** — fixed: selection uses the
   repository-wide reader filtered by source (behavior 17); discovery is the
   in-flow person picker; the required-absent message names the source that has
   none, not a target-scoped list command. Resolved; reader-scope coordination
   surfaced to 1029.
4. **`likely_bases` determinism** — rejected for the base: auto/`latest` is the
   newest snapshot of the source by ULID (total), not the producer's likely-base
   mark. Resolved.
5. **Sub-minimum prefixes** — fixed: examples and behavior 16 use full ULIDs or
   ≥6-char prefixes. Resolved.
6. **Dirty conflated with snapshot-unavailable** — fixed: a dirty target still
   reads snapshots; only new capture is skipped (behavior 15). Resolved.
7. **Declared-generator view not reconstructable for updates** — moot: the
   template-declared concept is out of scope; the base is a flag over plain
   templates, whose updates are ordinary `apply --from`. Resolved.

Additionally, the cross-judge's required revisions are all reflected: three seed
dispositions separated (8/9/12); a pure dependency-accepting selection surface
with prompting in the CLI adapter (G4); capture preserved (6); newest-selection
and picker ordering defined (§Selection); repository-wide discovery (17);
dedicated selection/ambiguity/no-id-coupling/purity guards (4, 11, 12, 16, 20,
21); consumed revisions named (above); and policy-A satisfaction stated
explicitly (opt-in `--like`, optional/overridable, fallback, clear required-absent
failure).

## Residual risks carried to Phase C

- **Invocation surface (flag vs verb)** and **flag name** and the
  **out-of-scope declared-generators** call are `design.yml` decisions-for-approval
  for Bob.
- **The `Project::snapshots()` reader scope** (repository-wide vs target-scoped)
  is a read-only coordination point with task 1029; the design states the
  dependency and that satisfying it edits no producer write path.
