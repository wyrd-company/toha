# Rationale — Candidate 2 (Direction B: synthetic lowest-precedence "Bundled" registry layer)

## Problem

Toha's demo template ships in the crate tarball but is not compiled into the
binary, so `toha apply toha-demo <dir>` fails with "template not found" in a
clean, offline, arbitrary-cwd environment. We must close that gap for all three
first-run paths (`apply`, `apply --dry-run`, and staged `stage`→`continue`/`apply`
across processes) without changing any existing name-resolution outcome. The
shape is non-obvious because several existing invariants cross our boundary and
must survive intact: the `<TEMPLATE>` classification order (git → `host:` →
folder → name); the name-resolution precedence (alias → short → formal, with
higher registry layers winning on the formal key); the trust boundary (hooks need
user/system trust or `--trust`; folders/discovered/local are untrusted); and the
hard physical constraint the Phase A trace surfaced — `Plan::build` walks the
template's source dir on disk and `apply` re-reads static files, reads per-file
permission metadata, and runs hooks from the template root, so **the template
root must physically exist as a real folder** at both build and apply time, in
both the stage process and a later resume process. An in-memory source would
force reworking `Plan`, `apply`, and the permission path; the demo must therefore
be materialized to a real, persistent folder.

## Usage (caller's view)

Callers type `toha apply toha-demo ./out` (or `--dry-run`, or `stage` then
`continue` in a second shell) from any directory, offline, on first run, and get
the demo — unless they have installed, aliased, or discovered their own
`toha-demo`, in which case theirs answers and the bundled demo steps aside.
`toha templates list` shows the bundled row so the reserved name is discoverable.
Rust crate callers see no new surface. The full README, the three call sites, and
the cross-process resume are in [`design.md` §1](design.md).

## Shape

Model the embedded demo as a **new registry layer below `System`**. The binary
owns the embed (`include_dir!(docs/examples/demo)`) and hands the library one
synthetic `Listed` entry to seed at the lowest precedence via
`Registry::seed_fallback`, which inserts **only when the formal key is absent** —
so precedence is realized by insert-if-absent, and any higher-layer `toha-demo`
shadows the bundled one. The bundled entry materializes its embedded content
lazily to `cache/bundled/toha-demo/<commit>/` (mirroring the git-source cache
layout) with the temp-dir + create-new + atomic-rename pattern the git cache and
`skills::export` already use, so it is idempotent and race-safe across processes.

Load-bearing decisions:

- **Fallback-only is emergent, not special-cased.** Resolution keeps its exact
  shape (alias → short → formal); the bundled entry can only be reached through
  the final formal-key lookup, and only when nothing higher holds the key. This
  is the whole thesis of the direction: collision behaviour falls out of layer
  precedence.
- **Bundled is formal-only, encoded once.** A single predicate
  `Listed::formal_only()` and one iterator `Registry::by_name()` exclude the
  bundled layer from alias matching, short-name matching, and alias-uniqueness
  checks. This is the invariant that lets us add a template *named* `toha-demo`
  without (a) creating a new short-name ambiguity for bare `demo`, or (b) making
  the merge reject a user's `alias X toha-demo` as "alias equals a formal name".
  Encoding it in one iterator (per encode-lessons-in-structure) means the rule
  cannot drift across its four call sites.
- **Unified identity.** The bundled demo resolves to an ordinary
  `ResolvedTemplate { formal_name:"toha-demo", commit:<sha256 hex>, folder:<cache
  path>, trusted:false, named:true }`. `commit` is a build-time content digest,
  contract-shaped (64-hex satisfies the documented commit pattern). Downstream
  designs (1073 defaults, 1075 Jinja) consume this with **no bundled-specific
  branch** — the payoff of routing everything through the registry.
- **Offline cross-process resume for free.** Because the seeded entry is present
  every run and its `commit` matches the staged `commit`, `resume_template` finds
  it by `entry()` lookup, materializes idempotently, and never reaches the
  git-fetch branch. Reconstruction is offline and cwd-independent.

Validation lives at the resolution boundary (the seam in `resolve.rs`); the pure
engine, `Plan`, `apply`, and `staging` are untouched and never learn the word
"embedded", per boundary-discipline. Invariants are in types where possible
(`Layer::Bundled`, `trusted:false` by construction) and in the one predicate
otherwise.

**Interface depth.** The public surface added is one enum variant
(`Layer::Bundled`) and one library method (`seed_fallback`), plus a binary-only
`bundled` module and two guarded call sites. Behind that small surface sits the
entire embed → digest → lazy race-safe materialization → fallback precedence →
offline cross-process resume machinery. Everything else in the system is
unchanged. The complexity exposed to callers (and to the rest of the codebase) is
near zero; the complexity hidden is the whole feature — high depth.

**Deliberately not done.** No in-memory/virtual filesystem source (would rework
`Plan`/`apply`). No new directory concept (reuses `cache`). No new error type
(reuses `ResolveError` and the existing `replay_failed`/exit-1 degrade). No trust
grant (a below-System layer confers no trust by the spec's own wording). No
reserved-name-wins seizure of the name.

## Synthesis decision

*(left for the orchestrator)*

## Tradeoffs accepted

- We accept **four internal touchpoints for one rule** (resolve's two scans,
  `check_aliases`, the `templates alias` guard all route through `by_name()`) in
  exchange for the bundled demo being a genuine registry layer — listed,
  unified, and downstream-transparent — rather than a resolver side-channel.
- We accept a **content-digest `commit` that changes when the demo changes**,
  which means a staged interview cannot resume across an upgrade that edited the
  demo, in exchange for identical-output guarantees and a self-enforcing sync
  check. It degrades to the existing "cannot resume, start over" path.
- We accept **materializing to `cache`** (which a `cache` clear will remove and a
  later run re-materializes) in exchange for reusing an existing, documented,
  no-approval directory and the proven git-cache write pattern.
- We accept **teaching the registry that one layer is formal-only**, a small
  information leak of the bundled concept into `registry.rs`, in exchange for not
  leaking it into the engine/`Plan`/`apply`/`staging` — we push the leak to the
  one module whose whole job is name resolution.
- We accept the bundled row **appearing in unfiltered `toha templates list`**,
  a small additive change to the listing output, in exchange for honest
  discoverability consistent with the layer model.

## Alternatives considered

- **Resolve-time fallback with no registry layer** (bundled handled only inside
  `resolve_template` when `registry.resolve` returns `NotFound`): simpler, fewer
  touchpoints, and no `by_name` exclusion. Rejected for *this* direction because
  it is not a layer — it hides the demo from listing, splits resolution into "the
  registry" plus "a special case", and forces `resume_template` to grow its own
  parallel bundled branch. It exposes *less* structure but hides the demo from the
  one surface (listing) that should show it; lower depth for the unification goal.
- **Reserved-name-wins** (bundled above alias/short for its key): a real product
  option, presented in [`design.md` §4]. Rejected as the default because it breaks
  an existing installed/aliased `toha-demo` — a narrowing of supported resolution
  requiring explicit approval. It hides nothing extra and exposes a surprising
  seizure of a user's name.
- **Embed as a phantom System entry** (reuse `Layer::System` instead of adding
  `Layer::Bundled`): avoids the enum variant, but System is admin-provisioned and
  *confers trust*; a demo masquerading as System would be trusted and would
  pollute `--system` listing and field-inheritance. Rejected: wrong trust and
  wrong provenance.
- **Digest = `TOHA_VERSION` instead of a content hash**: stable string, no build
  walk. Rejected: it neither fingerprints content (a demo edit within one version
  would not change the identity, weakening the sync check) nor gives a natural
  falsifiable freshness test.

## Open questions and risks

- Should a user who installs a template whose **formal** name cannot be
  `toha-demo` (formal names are derived) but whose **short** name is `toha-demo`
  be able to shadow the bundled demo, as the fallback rule allows — or is that a
  surprising way to lose the demo? (The design allows it; confirm it is desired.)
- Is materializing under `cache` acceptable given a `cache` purge silently removes
  the folder (re-created on next run)? Or should the bundled demo live under a
  more durable base? (No new directory is introduced by the `cache` choice.)
- When a resume fails because the demo content changed across an upgrade, is the
  existing git-flavoured message ("staged git template has invalid formal name")
  acceptable, or should a bundled-aware "cannot resume, start over" message be
  added to `guidance`?
- Should `toha templates list --bundled` exist as a filter, or is the unfiltered
  row plus the JSON `layer:"bundled"` sufficient? (Design recommends no new flag.)
- Confirm that adding `Layer::Bundled` and a `"bundled"` layer value in listing
  JSON is within the listing-output contract without a spec amendment, since the
  spec enumerates only local/system/user for the *filter* group.

## Next implementation step

Add `Layer::Bundled`, `Listed::formal_only`, `Registry::by_name`, and
`Registry::seed_fallback` to `src/registry.rs`, route the two `resolve` scans and
`check_aliases` through `by_name`, and prove fallback-only precedence with a unit
test (seeded bundled `toha-demo` resolves by formal key, loses to an alias and to
a short-name match, and does not change bare `demo`) before touching the binary
embed.
