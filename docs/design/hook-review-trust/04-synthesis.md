# Synthesis: hook-script review and trust persistence

Phase B/C of the architect workflow for task 1069. This artifact records the
red-flag screening, the two independent score sets (a read-only cross-judge from
a different model family and the architect's own read), the explicit
reconciliation of their disagreement, and the base/graft/reject decision that
produces the final design (`05-design.md`).

Candidate evidence is preserved verbatim under `03-candidates/`. The rubric and
the three structural directions are in `02-arena-rubric.md`.

## Runners and dropout

Three structurally distinct candidates were produced by isolated runners
(`inherit-parent`), each writing to its own path with no shared writable output.
All three completed and are viable; there was no dropout, which clears the "at
least two structurally distinct viable candidates" bar with margin.

| # | Direction | Identity bound to approval |
|---|-----------|----------------------------|
| 1 | Content-addressed digest **on the registry entry** | digest of executable surface, co-located with install metadata |
| 2 | Approved-digest **ledger decoupled** from the entry | a set of approved digests per identity, in its own store |
| 3 | **Commit-bound** trust + executable-surface digest fallback | resolved git commit (primary) or surface digest (folder) |

## Red-flag screening (before scoring)

Screened for shallow module, information leakage, temporal decomposition, and
pass-through, per `design-red-flags.md`.

- **Candidate 1.** No shallow module (the digest/diff module is deep behind a
  small surface). One mild information-leakage smell: the effective-trust compare
  `seal.digest == live` is written inline at three call sites (apply gate, `list`,
  `update`) rather than behind one function, so the trust policy is stated in
  more than one place. No temporal decomposition; no pass-through.
- **Candidate 2.** No material flags. Trust representation is fully removed from
  the registry entry and hidden behind `Ledger.contains`, so leakage is lowest of
  the three. The separate ledger file/subsystem is ownership-grouped (trust
  state), not a temporal split. Cost is total surface, not a red flag.
- **Candidate 3.** No shallow module. One temporal-decomposition smell in
  `templates update`: read old tree → digest → read new tree → digest → compare →
  decide → write, which re-derives the surface across steps and depends on the old
  tree still being on disk before the swap. The dual `HookRevision::{Commit,
  Digest}` identity is justified (git O(1) vs folder) but adds type surface.

No candidate was rejected at screening; Candidate 1's inline-compare and
Candidate 3's update temporality are the items the synthesis must resolve.

## Score set A — read-only cross-judge (different model family)

Scored 1–5 per criterion; see `02-arena-rubric.md` for criteria.

| Criterion | C1 | C2 | C3 |
|-----------|----|----|----|
| 1. Invalidation precision | 5 | 5 | 5 |
| 2. Capability preservation | 4 | 5 | 5 |
| 3. Engine purity / UI-free | 5 | 5 | 5 |
| 4. Forward-compat (1078 fields) | 5 | 5 | 3 |
| 5. Interface depth & locality | 4 | 5 | 4 |
| 6. Contract & migration surface | 4 | 5 | 2 |
| **Total** | **27** | **29** | **23** |

Judge verdict: base on **Candidate 2** for the cleanest module boundaries (trust
fully decoupled from the registry), explicit surfacing of the capability
narrowing, and a complete legacy migration path. It penalized Candidate 1 for not
flagging the narrowing and for co-locating trust with the registry, and penalized
Candidate 3 for a missing migration path, a temporal `update`, and a
forward-compat gap where a future field that references a *new file* needs the
surface reader taught.

## Score set B — architect independent read

Scored 1–5 per criterion after reading each candidate end to end.

| Criterion | C1 | C2 | C3 |
|-----------|----|----|----|
| 1. Invalidation precision | 5 | 5 | 4 |
| 2. Capability preservation | 5 | 5 | 5 |
| 3. Engine purity / UI-free | 5 | 5 | 5 |
| 4. Forward-compat (1078 fields) | 5 | 4 | 5 |
| 5. Interface depth & locality | 3.5 | 4.5 | 5 |
| 6. Contract & migration surface | 5 | 3.5 | 4.5 |
| **Total** | **28.5** | **27** | **28.5** |

Architect verdict: Candidate 1 and Candidate 3 tie ahead of Candidate 2. Weighted
toward the minimal contract surface for a pre-v1 tool and the "effective trust is
always the live compare — nothing to sync" property of Candidate 1, which removes
all update-time trust bookkeeping. Docked Candidate 1 on locality for the repeated
inline compare (the screening item). Docked Candidate 2 on contract/migration
because it introduces a whole new schema, file, layer-merge path, and migration
step, and a set-of-digests capability nobody has asked for.

## Reconciliation of the disagreement

The two sets agree that all three candidates satisfy invalidation precision,
engine purity, and adapter/capability preservation. They diverge on two axes:

1. **Locality (criterion 5): does trust belong co-located on the entry, or in a
   decoupled store?** The judge scores the decoupled ledger highest for zero
   leakage; the architect scores it lower because the decoupling buys a new
   subsystem. Both are right about the facts. The resolution is that leakage is a
   property of *where the trust decision is stated*, not of *where the approved
   fingerprint is stored*. Storing an approved-content fingerprint next to the
   installed-content pointer (`commit`, `path`, `aliases`) is cohesive — both are
   durable facts about one installed template. The leakage the judge correctly
   dislikes in Candidate 1 is the *inline compare repeated at three call sites*,
   not the storage location. That is fixable without a separate file: put the
   decision behind one function. So the entry-stored digest keeps its minimal
   contract *and* reaches zero policy-leakage by grafting a single trust
   chokepoint.

2. **Migration surface (criterion 6): how much does legacy `trusted: bool`
   migration weigh?** The judge treats a missing migration path as near-fatal
   (Candidate 3 = 2). The project is pre-v1 with no users and a single small
   registry file; per the repository's own "ceremony scales with blast radius"
   rule, migrating a boolean that nobody depends on is trivial — a legacy
   `trusted: true` simply reads as "no matching digest → needs review on next
   apply", which is the safe direction. So the migration advantage the judge
   assigns to Candidate 2 is small, and Candidate 2's *added* schema/file/layer
   surface counts against it, not for it, at this stage.

Netting both: the judge's decisive argument for Candidate 2 (locality) is real
but can be captured by a one-function graft onto Candidate 1; the judge's
secondary argument (migration) is over-weighted for a pre-v1 tool. Candidate 1's
minimal contract and no-sync property are the larger durable wins. The
architect's concern about Candidate 1 (locality) is conceded and fixed by the
graft rather than by adopting the heavier shape.

## Decision: base, grafts, rejections

**Base: Candidate 1** — content-addressed trust digest stored on the registry
entry; effective trust is always the live compare of the approved digest to the
freshly computed digest, so `templates update` needs no trust bookkeeping.

**Grafts:**

- **From Candidate 3 — a single trust chokepoint.** Introduce
  `evaluate_trust(approved: Option<&ReviewDigest>, live: &ReviewDigest) -> Trust`
  as the *only* producer of "trusted" from stored state. This removes Candidate
  1's repeated inline compare, so the trust policy is stated once. `Trust` is a
  two-variant enum; equality is the whole policy.
- **From Candidate 2 — explicit write discipline and the narrowing flag.** State
  as a typed invariant that the apply path never writes trust; only `templates`
  commands do. Surface the "trust no longer survives an executable-surface
  change on update" behavior as an explicit Bob decision (capability narrowing),
  not an assumption.
- **From Candidate 1 and 3 — whole-parsed-node digest.** Digest the raw canonical
  serialization of the parsed hook *node value* (sorted keys), not a
  `#[derive(Serialize)]` of a Rust struct, so a future opt-in field (design 1078)
  enters the digest with no review-code change and no allow-list to forget.

**Rejections:**

- **Candidate 2's separate ledger file + set-of-digests.** Rejected as premature
  surface for a pre-v1 tool: a new schema, a new file, layer-merge, and a
  migration step, plus a multi-revision-approval capability nobody has requested.
  Recorded as a future extension if rollback/multi-revision approval is ever
  wanted; the single-digest entry can grow into a set without breaking callers.
- **Candidate 3's dual commit/digest identity and pre-swap-tree update
  reconciliation.** Rejected: the commit-primary path exists to save a few script
  hashes on a load the apply already performs, so its performance argument is
  weak, and the reconciliation is temporal and depends on the old tree being on
  disk before the swap. A uniform content digest is simpler and removes the
  ordering dependency.

## Verification

- **Caller usage vs. sketch.** The base's three call sites (library review, apply
  gate, `templates update`) are re-expressed in `05-design.md` against the grafted
  chokepoint; the apply gate reduces to `--trust || matches!(evaluate_trust(..),
  Trusted)` and `list`/`update` call the same function, so the sketch and usage
  agree after the graft.
- **Requirements vs. design.** Review-before-trust, review-before-update-keeps-
  trust, precise changed-input definition, all adapters, engine purity, 1078
  auto-coverage, and the 1069/1071 policy-vs-syntax split are each traced to a
  design element in `05-design.md`.
- **Falsifiable invalidation matrix.** {no change, doc-only change, hook-line
  change, script-byte change} → {trusted, trusted, needs-review, needs-review} is
  the falsifiable test the design must pass; enumerated in `05-design.md`.
- **Compatibility with predecessors.** 1069 is a design root with no
  predecessor-design prerequisite, so there is no approved predecessor revision to
  contradict. It must not settle design 1071's trust-management command syntax;
  the design exposes the library operation and names where 1071 plugs in.
- **Reframe check.** The result holds under the reconciliation; no re-run of the
  candidate stage is warranted. The single unresolved structural choice — whether
  to hash literal in-tree paths named by `run:`/`args` — is a bounded capability
  decision surfaced to Bob, not a shape that changes the module map.
