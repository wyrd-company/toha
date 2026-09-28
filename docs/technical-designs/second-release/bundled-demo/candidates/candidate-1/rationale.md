# Rationale — Candidate 1 (Direction A: reserved-name resolution branch + on-demand materialization)

## Problem

`toha`'s demo template lives at `docs/examples/demo/` and ships in the crate
tarball, but it is not compiled into the binary and not resolvable by name, so
`toha apply toha-demo <dir>` on a clean machine fails with "template not found".
We must make it resolvable and runnable by name, offline, from any directory, on
first run — including the staged path where a `stage` in one process resumes in a
later process and produces identical output. The shape is non-obvious because of
constraints that crossed our boundary from Phase A: `Template::load`
canonicalizes its folder and `Plan::build`/`apply` walk it, re-read static files,
read per-file permissions, and run hooks from it, so the source **must** be a
real directory at both build and apply time — an in-memory source would force a
rewrite of `Plan`, `apply`, and the permission path. The `<TEMPLATE>`
classification order (git URL → `host:` → folder → alias → short → formal) and
*every* current name-resolution outcome are fixed contract; an installed or
aliased `toha-demo` must keep resolving to itself. The word "embedded" may not
leak into the pure engine, `Plan`, `apply`, `protocol`, or `staging`. And the
identity `(formal_name, commit)` is the shared surface that staging resume and
downstream designs 1073/1075 consume, so it must be stable and reconstructable
offline from the staged record alone. The registry schema pins `commit` to 40-
or 64-char lowercase hex and forbids extra fields on an entry.

## Usage (caller's view)

The caller types only `toha-demo` and needs no setup:

```console
$ toha apply toha-demo ./out                     # generate on first run, offline
$ toha apply --dry-run toha-demo ./out           # preview only (no `preview` subcommand)
$ toha stage toha-demo ./out                      # interview now…
$ toha continue ./out    # or: toha apply ./out   # …resume in a later process, offline
```

`toha-demo` is a reserved name: if you install or alias a template as
`toha-demo`, yours wins; the bundled demo answers only when nothing else does.
The batch/result documents carry `context.template = "toha-demo"` and
`context.commit = <64-hex>`, the identity downstream designs key on. Nothing in
the caller's view mentions embedding, a cache path, or a synthetic address. Full
call sites are in `design.md` §1.

## Shape

One deep seam. The only value resolution hands to the rest of the binary is the
existing five-field `ResolvedTemplate`; it gains **no field and no variant**. A
new CLI-only module `src/cli/bundled.rs` is the sole place that knows the demo is
embedded: it holds the `include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo")`,
computes `commit()` as a sha256 over the tree, and materializes the tree to a
real, content-addressed folder. Three small edits wire it in: the
`Address::Name` arm of `resolve_template` calls into `bundled` only on the exact
input that fails today (`registry.resolve` returns `NotFound("toha-demo")`); the
same fallback is mirrored in `formal_name` so `apply toha-demo <dir>` still
recognizes its own staged interview; and a branch at the top of `resume_template`
reconstructs the demo from `(formal_name, commit)` alone. Below the seam, the
flow is byte-for-byte the existing path (`Template::load` → pure interview →
`Plan::build` → `apply`), which is what keeps "embedded" out of the engine.

Load-bearing decisions:

- **Materialize into the cache namespace git already uses**
  (`~/.cache/toha/sources/toha-demo/<commit>/`). This needs no new capability:
  toha already writes template sources there for git fetches; we only fill a
  slot from embedded bytes instead of a clone. Content-addressed ⇒ immutable and
  reused across processes; materialization is idempotent and race-safe via
  tempdir-then-rename (per make-operations-idempotent), exactly like `cached`.
- **Identity is `formal_name = "toha-demo"` (stable) + `commit = sha256(tree)`
  (content version).** The digest ties resume determinism to the bytes that
  determine output — a version bump that leaves the demo alone keeps staged
  interviews resumable; a demo change correctly invalidates them. This is a
  content match like git's existing `fetched.commit != commit` guard, not a
  pinned-version check. The 64-hex form also satisfies the registry `commit`
  pattern, keeping the identity uniform with git-backed commits.
- **No registry entry.** The demo is never written to any `templates.yml`, so the
  registry stays a truthful record of installed templates and we avoid
  fabricating a `source`/`path` the schema documents but the demo can't honestly
  supply.
- **Encoded invariant: source == embedded == materialized.** `include_dir!` reads
  the source at compile time, so there is no second in-repo copy to drift (per
  encode-lessons-in-structure); a falsifiable test asserts the materialized tree
  is byte-identical to `docs/examples/demo/`, and a fixture test ties demo
  content to documented output.

Validation lives at the resolution boundary (`bundled::resume` rejects a
commit mismatch with a clear offline error); everything the pure code trusts is
already-materialized types. Interface depth: a large capability — embedding,
content-addressed materialization, a stable offline identity, cross-process
resume — sits behind a public surface that grows by **zero types**; two existing
functions gain internal branches and one private module appears. That is the
deepest interface of the candidates: the demo is indistinguishable from a by-name
template to every consumer, differing only in needing no setup.

## Synthesis decision

*(left for the orchestrator)*

## Tradeoffs accepted

- We accept a per-content `commit` that changes when the demo changes, in
  exchange for exact-output resume determinism; a staged interview cannot resume
  across a build whose demo bytes differ, degrading to a clear "stage again"
  message rather than silently producing different files.
- We accept that the reserved resolution name `toha-demo` differs from the demo
  template's own short name `demo` (its `template.yml` is embedded verbatim to
  keep one source), in exchange for not editing production/spec files and honoring
  the single-source constraint.
- We accept materializing to the cache (bytes on disk) rather than serving files
  from memory, in exchange for leaving `Plan`/`apply`/permissions untouched — the
  real-folder constraint from Phase A makes an in-memory source a false economy.
- We accept fallback-only precedence (a marginal "the name always means the
  demo" guarantee is given up) in exchange for preserving every current
  name-resolution outcome without needing approval.

## Alternatives considered

- **Synthetic in-memory registry entry for `toha-demo`.** Inject a fake `Listed`
  into the merged `Registry` so `registry.resolve` returns it. Rejected on
  interface depth and honesty: it pushes a fabricated `source`/`path` into a
  structure whose schema and prose say those are a real repo URL / canonical
  path, forces the trust/layer model to special-case a non-installed entry, and
  spreads "this one is special" across `registry.rs`, `resolve.rs`, and
  `trustable`, exposing the specialness to more callers rather than hiding it.
- **Reserved-name-wins precedence** (bundled shadows any installed/aliased
  `toha-demo`). Rejected: it narrows a supported resolution outcome — a product
  decision requiring approval — for little gain; presented as the non-recommended
  option in `design.md` §3.
- **Materialize under a new dedicated directory** (e.g. a `bundled/` sibling of
  `cache`). Rejected: it introduces a new on-disk location and arguably a new
  access surface needing justification, whereas the existing `sources/` cache
  already has exactly the right semantics (per-source, per-commit, immutable).

## Open questions and risks

- Should `templates add`/`alias` ever reserve the *formal key* `toha-demo` to
  close the purely theoretical case where a user installs a template whose formal
  key is literally that token? Reserving it would narrow a capability (approval
  needed); the recommendation is to leave it, since address-derived formal keys
  never equal the bare token. Is that acceptable?
- Is a per-content `commit` digest (not `TOHA_VERSION`) the identity you want
  downstream 1073/1075 to key configuration on — with `formal_name` as the stable
  key and `commit` as the content version — or do those designs need a
  version-stable `commit`?
- The commit-mismatch resume error currently surfaces through `resolve_error`
  (`Outcome::Error`, exit 1). Should it instead route through the existing
  `replay_failed`/"cannot be resumed" guidance (also exit 1) for message
  consistency with the spec's staged-resume-failure clause?
- Confirm the reserved name string: `toha-demo` (matches the task and the demo's
  spirit), with the demo's internal short name staying `demo`.

## Next implementation step

Write `src/cli/bundled.rs` with `RESERVED_NAME`, the
`include_dir!` embed, `commit()` (sha256 over the sorted-manifest of the tree),
and `materialize()` (content-addressed tempdir-then-rename into
`cache/sources/toha-demo/<commit>/`), then wire the `Address::Name` fallback in
`resolve_template` and prove it with a clean-environment
`assert_cmd` test of `apply toha-demo <tmp>`.
