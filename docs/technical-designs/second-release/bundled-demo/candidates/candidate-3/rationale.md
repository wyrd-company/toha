# Rationale — Candidate C: embedded-as-source

## Problem

`toha apply toha-demo <dir>` must succeed offline, from any directory, on first
run, with no registry entry, no cache, no network, and no git setup. The demo
exists only at `docs/examples/demo/` and ships in the crate tarball, but it is
not compiled into the binary and `toha-demo` resolves to NotFound. The shape is
constrained by a load-bearing fact from Phase A: **the template root must
physically exist as a real folder at both build and apply time.** `Plan::build`
walks `template.source_dir` on disk, `apply_reporting` re-reads static files with
`fs::copy`, reads per-file permission via `fs::metadata(&file.source)`, and runs
hooks from `template.root.join(path)`. An in-memory or virtual source would force
a rework of `Plan`, `apply`, `Content::Copied`, and the permission path. So the
demo must become a real folder before build/apply and must be reconstructable
between a `stage` process and a later `apply` process from the staged identity
alone. Two more constraints crossed our boundary: the `<TEMPLATE>` classification
order and every current name-resolution outcome must be preserved (including an
installed or aliased `toha-demo`), and the word "embedded" must not leak into the
engine, `Template`, `Plan`, `apply`, `protocol`, or `staging`.

## Usage (caller's view)

The caller types `toha apply toha-demo ./out` (or `stage`, or `apply --dry-run`)
from anywhere, offline, and gets `./out/note.txt`. Stage in one process, continue
and apply in another — byte-identical output. See `design.md` §1 for the
quickstart and the three concrete call sites. Nothing in a caller's mental model
says "embedded": they resolve a name, answer questions, and get files, exactly as
with any other template. Crate callers are untouched.

## Shape

The demo is treated as a **first-class source that materializes into the ordinary
source cache** — `<cache>/sources/toha-demo/<commit>/` — precisely where a git
fetch of the same identity would land. Resolution recognizes the reserved token
and short-circuits `Address` handling to this synthetic source; from that point
on the pipeline sees an ordinary resolved folder and an ordinary
`(formal_name, commit)` identity. This is the load-bearing decision: by reusing
the cache's `<install_key>/<commit>` layout and the existing resume machinery,
the demo inherits cross-process resume, idempotent atomic publish, and race
safety for free, and the rest of the pipeline needs zero changes
(`per boundary-discipline`, `per encode-lessons-in-structure`).

The identity: `formal_name = "toha-demo"` (what you type is the formal name is the
staged identity) and `commit =` a SHA-256 **content digest** of the embedded
tree. Choosing a digest over `TOHA_VERSION` is deliberate: in the as-source
framing the cache path already means "this install, this content", so a
content-addressed commit is the honest value. It makes the resume guard actually
enforce something — `bundled::resume` re-materializes only when the staged commit
equals *this* binary's demo digest, which is exactly the condition under which
re-materialization reproduces byte-identical files. It also maximizes
cross-upgrade resume survival: a version bump that does not touch the demo keeps
the same commit, so staged records keep resuming (`per make-operations-idempotent`,
single-source-of-truth: the digest is derived from the bytes, never stored twice).

Precedence is **fallback-only**: the bundled demo answers only after every
registry layer returns NotFound for exactly `toha-demo`. It sits strictly last in
the preserved classification order, so every current successful outcome is
preserved and the only change is that a previously-failing invocation now
succeeds — no supported resolution is narrowed, so no approval is required.

Materialization writes the cache, which is **not a new capability**: a git fetch
already writes `<cache>/sources/...`. And it never becomes *required* state:
`into_cache` degrades to `into_temp` when the cache is unavailable or read-only,
and resume re-materializes from the embedded bytes regardless — the binary is the
durable source of truth, the cache is only an optimization
(`per separate-before-serializing-shared-state`: the durable source and the
cached copy are separate; the cache is derived, not authoritative).

Invariants in types/structure: `trusted=false`/`named=false` make the demo
untrusted like a folder source and unable to be registry-trusted; the length-
prefixed digest makes the identity collision-resistant; `bundled::resume`
returning `Option` encodes "not mine — fall through" so the existing resume paths
are literally unchanged. Validation lives at the resolution boundary; the seam is
one module (`src/cli/bundled.rs`) plus three call sites.

Interface depth: the public surface added is four items (`RESERVED`, `commit`,
`resolve`, `resume`) behind which sits embedding, content-addressing, cache-first
idempotent materialization with atomic publish, and a read-only-cache fallback.
Every downstream module stays ignorant of all of it. That is a large amount of
complexity concentrated behind a small, deep seam — the property the rubric's C4
rewards. Deliberately not done: no new subcommand, no lib-level API, no change to
`StagedRecord`, `Plan`, `apply`, or the engine.

## Synthesis decision

*(left for the orchestrator)*

## Tradeoffs accepted

- We accept writing the OS cache on first run (in exchange for free cross-process
  resume, idempotency, and race safety from the existing `cached()` machinery).
  It is the same directory git fetch already writes, so no new capability.
- We accept a per-process temp dir left behind when the cache is unwritable (in
  exchange for first run working with a read-only/absent cache and no threaded
  lifetime guard through the pipeline). The OS temp reaper cleans it.
- We accept a content-digest commit that changes when the demo content changes,
  breaking resume of interviews staged against a *different* demo version across
  an upgrade (in exchange for a meaningful resume guard and byte-identical
  reproduction). It degrades cleanly to "cannot resume; start over".
- We accept that the reserved token is matched by an exact string compare, not a
  registry mechanism (in exchange for keeping the trigger specific and the
  fallback strictly last).

## Alternatives considered

- **Embedded virtual filesystem (no materialization).** Serve the demo's files
  from memory during `Plan`/`apply`. Rejected: it exposes the embedding to
  `Plan`, `apply`, `Content::Copied`, and the permission path — a shallow,
  leaky interface that spreads the concept across the pipeline and contradicts
  the load-bearing constraint. Hides little, exposes much.
- **Synthetic registry entry (inject a `Discovered`/system layer entry pointing
  at a materialized folder).** Rejected: it changes name-resolution semantics
  (the demo would surface in `templates list`, participate in ambiguity, and
  could shadow a real `toha-demo` depending on layer), enlarging blast radius
  and risking C2. The as-source seam keeps resolution semantics intact.
- **`commit = TOHA_VERSION`.** Rejected as the primary identity: it bypasses the
  fetch-guard's meaning, changes on every release even when the demo does not
  (needlessly breaking resume), and is not content-addressed, so the cache path
  no longer means "this content". Digest dominates on interface honesty.
- **Reserved-name-wins precedence.** Rejected as default: it narrows a supported
  outcome (installed `toha-demo`) and needs approval; offered as product option B.

## Open questions and risks

- Should precedence be fallback-only (recommended) or reserved-name-wins? The
  latter narrows resolution and needs your approval — which do you want?
- Is a leaked per-process temp dir acceptable when the cache is unwritable, or
  should the design thread a `TempDir` guard through so it is cleaned on exit
  (at the cost of a wider signature)?
- Is the cross-upgrade resume degradation (staged demo interview stops resuming
  when a later binary changes the demo content) acceptable as "abort and start
  over", or should staged records carry enough to reconstruct the *old* demo?
- Should `bundled::commit()` be the full 64-hex SHA-256, or truncated to 40 hex
  to visually match git commits in `context`/`--async` documents consumers read?

## Next implementation step

Write `src/cli/bundled.rs` with `DEMO`, `RESERVED`, `commit()` (digest), and
`write_tree` (generalized from `skills::export`), plus the
`embed_equals_source` and `commit_derives_from_source` unit tests, before wiring
the three `resolve.rs` call sites.
