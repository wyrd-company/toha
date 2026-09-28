# Design: hook-script review and trust persistence

The final design for task 1069 (paired implementation 1033), Toha 0.2.0. It is a
design artifact: it describes the target system and the contract edits the
implementation will make. It does not change canonical specifications, schemas, or
runtime code. Grounding is in `01-grounding.md`; the candidate exploration, scores,
and the base/graft/reject reconciliation are in `02-arena-rubric.md`,
`03-candidates/`, and `04-synthesis.md`.

## Problem

Trust today is a whole-template `trusted: bool` on the registry entry, bound to a
template's **name** in a writable registry (`src/registry.rs`, gate at
`src/apply.rs:82`). `templates add --trust` sets it blind, showing no hook
content, and `templates update` re-clones a followed branch to a new commit while
leaving `trusted` untouched (`src/cli/templates.rs:505-566`;
`command-line-interface.yml:55-56`). So an update that rewrites a `script:` body or
a hook `run:` line keeps stale trust, and the next `apply <name>` runs the changed
code with no review and no `--trust`. Approval must instead bind to the **reviewed
executable content**: review must be possible before a trusted install and before
an update keeps trust, the coverage must survive future hook-syntax growth, and
every adapter plus interview-engine purity must be preserved.

## Usage (caller's view)

### README / quickstart

> Toha runs a template's hooks only after you approve the exact code that would
> run. Approval is recorded as a fingerprint of the template's *executable
> surface* — its hook definitions and the bytes of any `script:` file. Review the
> hooks with `toha apply <template> <dir> --dry-run`, approve for one run with
> `--trust`, or record approval with `toha templates add <addr> --trust`. When a
> later `templates update` changes any hook or script, the fingerprint changes,
> the recorded approval no longer matches, and the next apply lists the hooks as
> *changed since approval* and asks you to review and approve again. A
> documentation-only update changes nothing executable, so approval stands.

### Call site A — library caller reviews a template (crate)

```rust
let template = Template::load(&folder)?;              // post-!include, parsed
let surface  = HookSurface::of(&template)?;           // pure, UI-free
let live     = surface.digest();                      // ReviewDigest

match evaluate_trust(stored_approval.as_ref(), &live) {
    Trust::Trusted    => { /* approved content unchanged: hooks may run */ }
    Trust::NeedsReview => match stored_approval {
        Some(_) => present_changed(&surface),          // approved once, changed since
        None    => present(&surface),                  // never approved
    },
}
```

### Call site B — the binary's apply gate (`main.rs`), replacing the `bool` seam

```rust
let template = load_template(&resolved)?;
let live     = HookSurface::of(&template)?.digest();
let trusted  = trust                                   // --trust one-run flag
    || matches!(evaluate_trust(resolved.approval.as_ref(), &live), Trust::Trusted);
// apply.rs is unchanged: it still takes a decided bool and returns NeedsTrust.
plan.apply_reporting(path, ApplyOptions { force, trusted }, &ProcessRunner, &mut on_written)?;
```

On the not-trusted path the existing exit-3 flow lists the hooks; when an approval
exists but no longer matches, the listing marks them *changed since approval*.

### Call site C — `templates update` re-evaluates trust with no bookkeeping

```rust
// after the clone is swapped in and set_commit runs, as today:
let live = HookSurface::of(&Template::load(&install)?)?.digest();
if let Some(approved) = entry.approval.as_ref() {
    if matches!(evaluate_trust(Some(approved), &live), Trust::NeedsReview) {
        lines.push(format!("updated {formal} (hooks changed; approval lapsed)"));
    }
}
// The stored approval is NOT rewritten. Effective trust is always the live compare;
// there is nothing to carry, drop, or sync.
```

## Shape

### Data structures

```rust
// src/review.rs — NEW library module. Pure; no I/O beyond reading script bytes.

/// The reviewable executable surface of a template, in a stable order:
/// interview hook nodes (interview order) then top-level hooks (list order).
pub struct HookSurface { hooks: Vec<HookView> }

struct HookView {
    origin:  HookOrigin,             // Interview { index } | TopLevel { index }
    node:    CanonicalNode,          // canonical (sorted-key) encoding of the PARSED hook node,
                                     //   post-!include, PRE-render — a Value, not a serde-derive
    scripts: Vec<ScriptDigest>,      // every file this node executes (see "changed input")
}
struct ScriptDigest { rel: RelPath, sha256: [u8; 32] }

/// A fingerprint of the whole executable surface. Constructed only by `digest()`
/// or parsed from disk against `^sha256:[0-9a-f]{64}$`; not hand-forgeable.
pub struct ReviewDigest(String);     // "sha256:<64 hex>"

pub struct HookSurfaceDiff { /* added / removed / changed HookViews for presentation */ }

impl HookSurface {
    pub fn of(template: &Template) -> Result<Self, ReviewError>;  // reads script bytes
    pub fn digest(&self) -> ReviewDigest;                          // deterministic, answer-independent
    pub fn diff(&self, prior: &HookSurface) -> HookSurfaceDiff;
}

/// The ONLY producer of "trusted" from stored state. Equality is the whole policy.
/// (Graft from candidate 3: a single chokepoint, so the trust rule is stated once.)
pub enum Trust { Trusted, NeedsReview }
pub fn evaluate_trust(approved: Option<&ReviewDigest>, live: &ReviewDigest) -> Trust {
    match approved {
        Some(a) if a == live => Trust::Trusted,
        _ => Trust::NeedsReview,
    }
}
```

Registry datum (replaces the bool — one source of truth, nothing to sync):

```rust
pub struct Entry { /* name, source, ref, commit, path, aliases (unchanged) */
    pub approval: Option<ReviewDigest>,   // was: pub trusted: bool
}
```

`FieldPresence.trusted` becomes `.approval`; sparse round-trip, layer-merge
precedence (local < user < system, higher-omitted inherits lower), and `add`'s
inherit-on-re-add all carry over in mechanism. `add`'s
`entry.trusted |= old.trusted` becomes `entry.approval = entry.approval.or(old.approval)`
— safe, because an inherited digest grants trust only while the live content still
matches it. `ResolvedTemplate` carries `approval: Option<ReviewDigest>` in place
of `trusted: bool`; `Listed`/`Resolved` likewise, for named non-local entries only.

### What counts as changed executable input (precise)

The digest input, and nothing else, is:

1. Each hook node's canonical encoding — **all** fields (`run` / `script` / `args`
   / `cwd` / `when` / `each`, and any future field), in surface order; and
2. The bytes (as `sha256`) of every file the surface executes, framed with its
   template-relative path:
   - every declared `script:` target; and
   - **(pending Bob decision, see Decision 2)** every `run:`/`args` token that is a
     *literal* in-tree path (no Jinja) resolving inside the template root.

Everything else in the template — generated files, questions, messages, docs — is
excluded, so a doc-only or unrelated update leaves the digest, and the approval,
intact. The identity bound to approval is this content digest, per entry; it is
independent of formal name, branch, and commit.

The `node` is the canonical encoding of the **parsed hook node value** (sorted
keys), not a `#[derive(Serialize)]` of a Rust struct. This is the load-bearing
choice for forward-compatibility: a future opt-in field (design 1078's Jinja hook
results) is present in the parsed node by construction, so it enters the digest
with no change to review code and no allow-list to maintain. `Template::load`
retains the post-`!include` hook nodes as the single source the digest reads;
`additionalProperties: false` keeps the node set to exactly the known schema at any
version.

### Module / seam map

```
                approval digest (stored)              executable-surface digest (live)
 templates.yml ───────────────► registry.rs ──► resolve.rs (ResolvedTemplate.approval)
                                                        │
 template.yml ──load(+!include)──► template.rs ──► review.rs  HookSurface::of / digest / diff
   (parsed hook nodes)                  │                     (pure, UI-free)
                                        ▼                          │
                     main.rs apply gate: trusted = --trust || evaluate_trust(approval, live) == Trusted
                                        │           (the ONE trust chokepoint; list & update call it too)
                                        ▼
                            apply.rs  (bool in → NeedsTrust out; UNCHANGED)
                                        │
                                        ▼
                            HookRunner (the one library port; UNCHANGED)
```

### Error / results contract

- `ReviewError` = I/O reading a `script:` (or in-tree `run` argument) file:
  missing, unreadable, or a path that escapes the template root. A template whose
  surface cannot be read cannot be trusted.
- `digest()` is infallible once the surface is built.
- `apply.rs` keeps `Applied::{Written, NeedsTrust}` and exit code **3**; `--trust`,
  the `--dry-run` listing (exit 0), and target-path printing are unchanged.
- `templates list`'s trust column becomes an *effective* value: for each entry,
  `matches!(evaluate_trust(entry.approval.as_ref(), &live), Trusted)`, computed by
  loading the entry's installed template; an unreadable install reads as not
  trusted.

### Invariants encoded in types

- **No stored bool can drift from content.** Trust is *only* produced by
  `evaluate_trust`, always comparing a stored digest to a freshly computed one.
  There is no settable `trusted: bool`, so the stale-trust state is
  unrepresentable.
- **The apply path never writes approval.** Approval is written only by `templates`
  commands (`add --trust`, and the review/approve command design 1071 owns).
  Concurrent `apply` runs never race a write.
- **A new hook field cannot silently escape review.** `HookSurface::of` reads the
  whole parsed node value; a property test asserts that mutating *any* field of any
  hook node changes the digest.
- **The digest is answer-independent.** It reads declared hooks (all interview and
  top-level nodes), not the `when`/`each`-filtered plan subset, so an answer set
  that later activates a dormant hook cannot bypass review.
- **Local layer cannot grant trust.** The local-registry schema rejects the
  `approval` field, as it rejected `trusted`.

### What the interview engine must NOT gain

No review or prompt port, no trust or digest knowledge, no UI. It stays
answer → question. Review and digest live in `review.rs` and the binary; trust is
decided at the gate; drivers present. `HookRunner` remains the only library port.

## Behaviors to prove (falsifiable)

- **Invalidation matrix.** For a trusted template, apply after each of
  {no change, doc-only change, hook-line change, script-byte change} yields
  {run, run, needs-review (exit 3), needs-review (exit 3)}. This is the central
  falsifiable test.
- **Update keeps/drops trust.** `templates update` across a doc-only commit keeps
  approval (next apply runs hooks); across a commit that changes a hook or script
  lapses approval (next apply exits 3 and lists the change).
- **Field-coverage guard.** Adding a field to a hook node in a fixture changes the
  digest; a test fails if it does not — proving 1078-style growth is covered.
- **Answer-independence.** Two apply runs with different answers over the same
  template produce the same digest.
- **Adapter parity.** Headless `--answers` with `--trust` runs hooks; without it
  exits 3 with the same listing; staged resume across processes re-computes the
  gate at the staged commit; a crate caller reaches the same decision via
  `HookSurface` + `evaluate_trust`.
- **Sole-kill (for implementation).** Removing the `evaluate_trust` equality (always
  return `Trusted`) fails the invalidation-matrix tests; narrowing the digest to
  exclude script bytes fails the script-byte-change test; digesting a serde-derive
  instead of the parsed value fails the field-coverage guard.

## Compatibility

- **Predecessors.** 1069 is a design root with no predecessor-design prerequisite;
  there is no approved predecessor revision to contradict.
- **Coordination (1074, bundled demo).** The bundled demo ships no hooks, so trust
  never engages for it; this design adds no bundled-trust mechanism. A *future*
  bundled template that ships hooks is an open question (Decision 3).
- **Downstream (1071, 1075).** 1071 owns the trust-management *command syntax*; it
  calls the library review/approve operation this design exposes and writes
  `entry.approval`. 1075 (Jinja context) and 1074 consume the same
  selected-template identity; this design keys approval on content, orthogonal to
  that identity, so it does not constrain them.
- **Legacy on-disk state.** Pre-v1, no users. A legacy `trusted: true` entry has no
  `approval` digest, so it reads as needs-review on the next apply — the safe
  direction — with no migration step (Decision 4 confirms this is intended).

## Canonical-document impact (described, not applied — owned by implementation 1033)

These edits are specified here and made by the implementation in the same change,
never in this design package.

- **`template-registry.schema.yml`** — replace `trusted: { type: boolean, default:
  false }` on a registry entry with
  `approval: { type: string, pattern: '^sha256:[0-9a-f]{64}$' }`. The
  local-registry `$def` continues to forbid the field.
- **`command-line-interface.yml` — Trust paragraph** — the hooks run when the
  caller has approved the template's *current executable surface* in the user or
  system registry, or when `--trust` is given; approval is a digest of the hook
  nodes plus the bytes of every executed in-tree file and authorizes the hooks
  only while that digest equals the installed content's digest; local-path and
  address/folder invocations run hooks only through `--trust`; the exit-3 dry-run
  listing marks hooks *changed since approval* when an approval exists but no
  longer matches.
- **`command-line-interface.yml` — Updates paragraph** — replace "The formal name,
  aliases, and trust of an updated template are unchanged." with wording that keeps
  name and aliases unchanged and states that approval continues to authorize the
  hooks only while the executable surface is unchanged; an update that changes the
  executable surface makes the template untrusted until approved again.
- **`template-registry.yml`** — replace the `trusted: true` sentence with a
  description of `approval` as the approved executable-surface digest; change "is
  not trusted" to "carries no approval" in Discovery/Ownership.

## Out of scope

- Trust-management command syntax (review / approve / untrust / list-trust) — owned
  by design 1071; this design only exposes the library operation and names the
  plug-in point (`entry.approval` write + `HookSurface`/`evaluate_trust`).
- The opt-in Jinja hook-result fields themselves — design 1078; this design only
  guarantees they are covered automatically.
- Multi-revision / rollback approval (a *set* of approved digests) — recorded as a
  future extension; the single-digest field can grow into a set without breaking
  callers.

## Decisions needed from Bob (options + recommendation)

1. **Capability narrowing — trust no longer survives an executable-surface change
   on update.** Today `templates update` keeps `trusted` across any update; this
   design drops effective trust when the hook nodes or script bytes change. This
   narrows the "trust forever by name" behavior and is the core fix.
   - A. Approve the narrowing (recommended — it is the defect this task exists to
     close; doc-only updates still keep trust, so precision is preserved).
   - B. Keep trust across all updates (rejects the task's purpose).
   Recommendation: **A**.

2. **`run: [interpreter, in-tree-file]` bytes coverage.** A `run` hook can execute
   an in-tree file named as an argument (e.g. `run: [python, scripts/x.py]`) whose
   bytes are not a declared `script:`. Options:
   - A. Additionally hash any `run`/`args` token that is a *literal* in-tree path
     (no Jinja); accept a residual gap for Jinja-substituted paths, which cannot be
     resolved before render (recommended — closes the common escape with a bounded,
     declared-surface rule).
   - B. Cover only declared `script:` targets; document the `run`-arg gap.
   Recommendation: **A**. This bounds what the digest reads; it is disclosed as a
   capability-shaping choice below.

3. **Future bundled template that ships hooks.** Today's bundled demo ships no
   hooks (confirmed with design 1074). If a future bundled template ships hooks, it
   has no registry entry to hold an approval. Options: a built-in approval shipped
   with the binary, or `--trust` each run. Recommendation: **defer** — decide when
   such a template is proposed; nothing in this design blocks either.

4. **Legacy `trusted: true` migration.** Pre-v1, no users. Options: re-approve on
   next apply (no migration; recommended), or write a one-time migration.
   Recommendation: **re-approve on next apply**.

## Permissions / access and related disclosures

Called out separately because deck or design inclusion alone does not grant
approval:

- **Supported-capability change (requires approval):** Decision 1 narrows the
  existing "trust persists across updates" capability. No other supported capability
  is removed; all existing grant flows (`add --trust`, `--trust`) and all adapters
  keep working with no new interactive gate a script cannot pass.
- **Filesystem read scope (permissions/access):** `HookSurface::of` reads the bytes
  of executed in-tree files (declared `script:` targets, and under Decision 2, literal
  in-tree `run`/`args` paths). This is a read within the template root only; it adds
  no write, network, or out-of-root access. Approve as part of Decision 2.
- **No new timeout mechanic, no pinned-version check, and no application-subprocess
  integration** are introduced by this design.

## Open questions and risks

- Should `templates list` compute live digests for the trust column (loads every
  installed template) or show "approved" vs "approved (changed)" without a full
  recompute? (Cost vs. accuracy; recommend compute, it is a small parse plus a few
  hashes.)
- Should a now-mismatched approval be pruned on update, or kept so trust re-matches
  if content reverts? (Recommend keep; harmless and convenient.)
- A future hook field that references a *new* executable file (not covered by the
  `script:`/literal-`run` rule) would need the surface reader taught to pull those
  bytes; the field-coverage guard proves the *field* is in the digest but not that a
  newly referenced file's *bytes* are. Recommend a test that fails when a new
  file-referencing field is added without a coverage decision.

## Next implementation step

Add `src/review.rs` with `HookSurface`, `ReviewDigest`, `digest()` over the
canonical encoding of the parsed hook nodes plus executed-file byte hashes, and the
`evaluate_trust` chokepoint; have `Template::load` retain the parsed nodes; add the
property test that any hook-field mutation changes the digest. Land types and
signatures with `not implemented` bodies first so the seam is reviewable before the
gate is wired.
