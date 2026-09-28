# Hook review and trust persistence — candidate 2: approved-review ledger

Structural direction: trust is **not** a field on the install entry. It is a
separate **ledger of approved executable digests per template identity**. The
registry entry keeps only install metadata; a first-class library `Review`
operation returns the reviewable executable surface plus its current digest and
approval status. Because approvals are a *set*, "keep trusting the old revision"
and "approve the new one" both fall out for free.

## Problem

Trust today is `Entry { trusted: bool }` keyed by formal name in a writable
registry (`src/registry.rs:31`, gate at `src/apply.rs:82`). It binds to the
*name*, not the executable content. `templates update` swaps a followed branch to
a new commit and never touches `trusted` (`src/cli/templates.rs:505-566`;
contract `command-line-interface.yml:55-56`), so a changed `script:` body or
`run` line runs on the next `apply` with no review and no `--trust`. `add --trust`
is a blind assertion; no hook content is ever shown. We must (1) let a caller
review executable hook content before it is trusted, (2) drop stale trust when the
executable input changes, (3) define precisely *what* changed input is and *what*
identity binds approval, while keeping the interview engine pure and every adapter
working.

## Usage (caller's view)

### README / quickstart

> Toha runs a template's hooks only when their **exact executable content** has
> been approved. Approval is recorded per template in an *approval ledger*
> separate from the install list. `toha apply NAME` shows the hook programs,
> arguments, and referenced script files; if that content has never been
> approved, or has changed since it was, Toha performs a dry run, shows what
> changed, and exits `3`. Approve it once and it stays approved until the content
> changes again — a documentation-only `templates update` keeps approval; an
> update that touches a hook or a script drops it until you review again.
> `--trust` still runs the hooks for one invocation without recording anything.

### Call site 1 — library caller computes a Review + diff (pure, no UI)

```rust
let template = Template::load(&folder)?;
let ledger = Ledger::load_layers(&system_ledger, &user_ledger)?; // union, read-only
let review = Review::of(&template, &ledger)?;   // reads template.yml + script bytes
match review.status {
    ReviewStatus::Approved  => { /* digest is in the ledger; hooks may run */ }
    ReviewStatus::Unreviewed => present(&review.surface),        // never approved
    ReviewStatus::Changed { ref approved } => {
        // `approved` are the still-approved digests for this identity.
        // A real content diff needs the previously approved surface, which the
        // caller reconstructs from the approved commit when it has one:
        if let Some(old) = review.prior_surface()? {             // git only, else None
            present_diff(&diff_surface(&old, &review.surface));
        } else {
            present(&review.surface);                            // show current as new
        }
    }
}
```

### Call site 2 — the binary's apply gate consults the ledger

```rust
let template = load_template(&resolved)?;                 // content, pinned commit
let ledger   = Ledger::load_layers(&dirs)?;              // system ∪ user
let review   = Review::of(&template, &ledger)?;
let trusted  = trust_flag                                 // --trust: one run, no record
    || ledger.contains(&resolved.formal_name, &review.digest);
match plan.apply(path, ApplyOptions { force, trusted }, &ProcessRunner)? {
    Applied::NeedsTrust(_) => {
        show_plan(&plan);                                 // unchanged dry-run listing
        // names the same command with --trust and the approval command (1071),
        // plus review.status so a caller sees "changed" vs "never reviewed".
        return Outcome::NeedsTrust(guidance::needs_trust(&invocation, installed));
    }
    Applied::Written { .. } => { /* ... */ }
}
```

`Plan::apply` and its `ApplyOptions { force, trusted }` port are **unchanged**:
the digest is computed by the binary/library *before* the gate, the gate still
sees one `bool`. The exit-code 3 contract and the dry-run listing are untouched.

### Call site 3 — `templates update` re-evaluates against the ledger (no ledger code)

```rust
// update swaps content to the newest commit and set_commit(...), as today.
// It writes NOTHING to the ledger. On the next apply, Review::of recomputes the
// digest from the new content:
//   - executable surface unchanged (doc-only update) -> same digest -> Approved
//   - a hook or script changed                       -> new digest  -> Changed
// update MAY report the delta as a courtesy:
let review = Review::of(&Template::load(&install)?, &ledger)?;
if matches!(review.status, ReviewStatus::Changed { .. }) {
    lines.push(format!("updated {formal} (hooks changed; re-approval required)"));
}
```

The hole closes with **zero** special logic in `update`: trust is digest
membership, and changed content has a new digest. Over-invalidation is avoided for
free — a doc-only update yields an identical digest and stays approved.

## Shape

### Data structures

```rust
// review.rs — pure, UI-free
pub struct ApprovalDigest([u8; 32]);        // sha256; Display = lowercase hex

/// The reviewable executable surface of ONE hook. Derived by one canonical
/// serialization of the parsed hook node — NOT a hand-copied field list — so a
/// future hook field (e.g. Jinja "hook results", design 1078) enters the surface
/// and the digest automatically, and cannot silently escape review.
pub struct HookSurface {
    node: template::HookNode,               // full parsed AST: run/script/args/cwd/when/each/…
    scripts: Vec<InTreeFile>,               // bytes of each referenced in-tree file
}
pub struct InTreeFile { pub rel: PathBuf, pub bytes: Vec<u8> }

pub enum ReviewStatus {
    Approved,                               // current digest ∈ ledger[identity]
    Unreviewed,                             // ledger has no digest for identity
    Changed { approved: Vec<ApprovalDigest> }, // ledger has other digests, not this one
}
pub struct Review {
    pub surface: Vec<HookSurface>,          // every DECLARED hook, answer-independent
    pub digest: ApprovalDigest,
    pub status: ReviewStatus,
    approved_commit: Option<String>,        // provenance for prior_surface(), git only
}
impl Review {
    pub fn of(template: &Template, ledger: &Ledger) -> Result<Review, ReviewError>;
    pub fn prior_surface(&self) -> Result<Option<Vec<HookSurface>>, ReviewError>; // reconstruct from approved_commit
}
pub struct SurfaceDiff { /* per-hook: Added | Removed | Changed{old,new} */ }
pub fn diff_surface(old: &[HookSurface], new: &[HookSurface]) -> SurfaceDiff;
```

```rust
// ledger.rs — a set of approved digests per formal name, per layer
pub struct Ledger { approvals: IndexMap<String, IndexSet<Approval>> }
pub struct Approval { pub digest: ApprovalDigest, pub commit: Option<String> }
impl Ledger {
    pub fn load_layers(system: &Path, user: &Path) -> Result<Ledger, LedgerError>; // union; local NEVER contributes
    pub fn contains(&self, formal: &str, digest: &ApprovalDigest) -> bool;
    pub fn approve(&self, formal: &str, a: Approval) -> Ledger;   // returns new value (immutable, like RegistryFile)
    pub fn revoke(&self, formal: &str) -> Ledger;                 // 1071 "untrust"
    pub fn write_atomic(&self, path: &Path) -> Result<(), LedgerError>; // USER layer only
    pub fn import_legacy_trust(&self, formal: &str, current: ApprovalDigest) -> Ledger; // migration
}
```

`Entry` loses `trusted`; it holds install metadata only
(`name/source/ref/commit/path/aliases`). `ResolvedTemplate.trusted` and
`Listed.trusted`/`Resolved.trusted` are removed — resolve no longer decides trust
because trust needs the *content* digest, which is known only after the template
loads. `FieldPresence.trusted` and the `trusted |= old.trusted` re-add merge are
deleted; sparse round-trip for `aliases` is retained unchanged.

### Access patterns traced

- **"is this identity+digest approved?"** → `Ledger.contains(formal, digest)`:
  one `IndexMap` lookup + `IndexSet` membership. O(1). This is the apply gate and
  the staged-resume decision.
- **"what changed since last approval?"** → `Review::of` sets `status` from set
  membership (no content needed); a *textual* diff calls `prior_surface()` (reads
  the approved commit's `template.yml` + scripts for git templates) then
  `diff_surface`. Both pure; the ledger stores digests, never content.

### Module / seam map

```
              library (toha crate) — pure, UI-free, no writes on the apply path
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ template::HookNode  ── single source of every hook field ───┐             │
 │        │ canonical serialize + read script bytes            ▼             │
 │        └────────────────────────►  review::HookSurface ──► ApprovalDigest │
 │ review::Review::of(&Template, &Ledger) -> Review   (status via set)       │
 │ review::diff_surface(old,new) -> SurfaceDiff                              │
 │ ledger::Ledger { load_layers, contains, approve, revoke, write_atomic }   │
 │ apply::Plan::apply(.., ApplyOptions{trusted:bool}, &dyn HookRunner)  ◄── unchanged port
 └──────────────────────────────────────────────────────────────────────────┘
        ▲ present + write ledger                    ▲ present + supply `trusted`
   terminal driver (interactive)          headless / staged driver / crate caller
   1071 owns the approve/untrust command SYNTAX; it calls Ledger.approve/revoke.
```

### Error / results contract

- `ReviewError`: `Io { path, source }`, `Parse { path, message }` — reading
  `template.yml` or a `script:`/argv-referenced in-tree file; boundary-parsed to
  domain types.
- `LedgerError`: `Io`, `Parse`, `Schema { path, instance_path, message }` —
  same shape as `RegistryError`, atomic write via tempfile + persist.
- Gate result unchanged: `Applied::NeedsTrust(Plan)` → binary exits `3`.

### Invariants encoded in types

- **Digest covers all declared executable input.** `HookSurface` wraps the whole
  parsed `HookNode`; a new field on `HookNode` flows into the canonical
  serialization and the digest with no digest-code edit. There is no per-field
  allow-list to forget. (Meets requirement 5.)
- **Approval binds to content, not name.** Membership is `(formal_name, digest)`.
  A name with no matching digest is untrusted regardless of registry state.
- **The apply path never writes the ledger.** `Ledger` write methods are called
  only by explicit `templates` commands. Legacy migration is imported into the
  *in-memory* ledger on read and persisted only on the next command write, so
  concurrent `apply` runs never race a write (single-writer discipline).
- **Local layer cannot grant trust:** `load_layers` takes only system + user
  paths; there is no local ledger. Preserved by construction.
- **Digest is answer-independent:** `Review::of` reads `template.hooks` and
  interview hook nodes directly (all *declared* hooks, not the `when`/`each`
  filtered plan subset), so a later answer set that activates a dormant hook
  cannot bypass review.

### What the interview engine must NOT gain

No prompt/review port, no ledger handle, no UI. `render_hooks` and the engine
stay pure. Review is computed outside the engine (in `review.rs` + the binary),
exactly where trust is decided today. `HookRunner` remains the only library port.

## Tradeoffs accepted

- **Two files instead of one.** Trust moves to `approvals.yml` beside
  `templates.yml`. More surface, but a clean separation: install state and trust
  state have independent lifecycles and independent writers.
- **Ledger stores a commit for provenance.** To offer a *textual* diff we keep
  `commit` next to each digest. It is provenance only — membership uses the
  digest — but it is a light coupling to install state. Alternative (store full
  surface content) rejected as bloat; git already has the bytes.
- **Set grows unbounded.** Every approved revision stays approved (enables
  rollback). A once-approved, later-known-bad revision stays trusted if content
  reverts. Pruning is deferred (open question).
- **Trust decision moves later** (after template load) than today (at resolve).
  Necessary: the digest needs content. Net simplification of `resolve`.

## Alternatives considered

- **Trust as a content hash *field on the entry* (`trusted_digest: Option<Sha>`
  replacing `trusted: bool`).** Rejected. It keeps trust and install state in one
  record, so a single field can hold only one approved revision — no "still trust
  the old one" without a second field, and `update`'s `set_commit` sits one line
  away from the trust field, inviting the same "carry it along" bug the grounding
  documents. Interface depth is worse: the wire type (`Entry`) leaks trust onto
  every registry read and merge, and the sparse `FieldPresence` machinery must
  grow a third tracked field. A decoupled set hides trust behind `Ledger.contains`
  and lets `update` stay trust-agnostic.
- **Whole-tree or per-commit digest.** Rejected by the grounding's Avoid: any
  file change (docs) forces re-review. The declared-surface digest re-reviews
  exactly when a hook or a referenced script changes.

## Open questions and risks (for the human)

1. **`run: [interpreter, in-tree-file]` bytes escape.** A `run` hook can name an
   in-tree file as an argument (e.g. `run: [python, scripts/setup.py]`) whose
   bytes are not a declared `script:`. The declared-surface digest hashes the
   argv *template strings* but not that file's body, so editing it escapes
   review. Options: (a) also hash bytes of any argv token that resolves to an
   existing in-tree path (best-effort; recommended); (b) accept the gap, relying
   on the commit pin; (c) require such files be declared `script:`. Which policy?
2. **Future bundled template that ships hooks.** Design 1074's bundled demo ships
   no hooks, so trust never engages. A *future* bundled template with hooks has no
   git commit and no registry entry — what identity keys its approval, and is it
   trusted-on-ship (vendor-signed) or does it flow through the same review as any
   folder template? Needs a decision before such a template exists.
3. **Ledger ownership, layering, and migration.** Confirm: user ledger at
   `$XDG_DATA_HOME/toha/approvals.yml`, system ledger admin-provisioned, union
   membership, local never contributes. Migration: on read, a legacy
   `Entry.trusted: true` imports as an approval of the *current on-disk digest*
   (`import_legacy_trust`) — in memory only until the next command write. Keep the
   `trusted` schema field accepted-for-import but stop writing it, or drop it
   outright (breaking)? Recommend accept-for-import for one release.
4. **`add --trust` semantics (capability change — needs approval).** Today it
   records name-trust blindly. Proposed: `add --trust` approves the *freshly
   cloned content's* digest into the ledger (still one headless command, no
   prompt, so the headless capability is preserved), but trust now drops on a
   content-changing update. This is the intended change (requirement 2), yet it
   *narrows* the old "trust forever by name" behavior — flagged for explicit
   approval per the durable preference on narrowing supported capability.
5. **Ledger pruning.** Should `approve` cap or prune old digests, or keep the full
   set forever (current proposal)?

## Adapter outcomes

- **Interactive terminal:** on `NeedsTrust`, the driver presents `review.surface`
  (and `diff_surface` when `prior_surface()` is available), then the user re-runs
  with `--trust` (one run) or runs the approve command (1071) which calls
  `Ledger.approve`. No new engine port.
- **Headless `--answers`:** `--trust` still grants for the run and records
  nothing (preserved). The `NeedsTrust`/dry-run document carries `review.digest`
  and `review.status` so a script can decide non-interactively. `add --trust`
  approves the current digest (see open question 4).
- **Staged resume across processes:** `resume_template` computes trust as
  `ledger.contains(formal, Review::of(&template, &ledger)?.digest)` against the
  content pinned to the staged commit. Closes the staged variant of the hole too.
- **Crate / library callers:** `Plan::apply(.., ApplyOptions{trusted}, ..)`
  unchanged; callers derive `trusted` from `Review` + `Ledger`, both pure.

## Proposed schema edits (shapes, not applied)

- **`template-registry.schema.yml`:** remove `trusted` from `#/$defs/entry`
  (accept-for-import handled in code, not schema — or keep with
  `deprecated: true`). No other entry change.
- **New `template-approvals.schema.yml`:**
  ```yaml
  $defs:
    approval: { type: object, additionalProperties: false, required: [digest],
      properties: { digest: { type: string, pattern: '^[0-9a-f]{64}$' },
                    commit: { type: string, pattern: '^[0-9a-f]{40}([0-9a-f]{24})?$' } } }
    approvals: { type: object, additionalProperties: false,
      properties: { approvals: { type: object, additionalProperties:
        { type: array, items: { $ref: '#/$defs/approval' }, uniqueItems: true } } } }
  ```

## Proposed CLI-spec text edits (shapes, not applied)

- **Trust paragraph (`command-line-interface.yml:38`):** "The hooks of a template
  run when the current executable surface of its hooks is approved in the user or
  system approval ledger, or when `--trust` is given. The executable surface is
  the declared `run`/`script`/`args`/`cwd`/`when`/`each` of every hook plus the
  bytes of every referenced in-tree file; approval binds to that content's digest
  under the template's formal name. `--trust` runs the hooks once without
  recording approval. When the surface is unapproved or has changed, `apply`
  performs a dry run, lists the hooks, states whether the content is new or
  changed, names the same command with `--trust`, and — for a name in the user or
  system registry — names the approval command, then exits `3`."
- **Updates paragraph (`command-line-interface.yml:55-56`):** replace "The formal
  name, aliases, and trust of an updated template are unchanged." with "The formal
  name and aliases of an updated template are unchanged. Trust is bound to the
  hooks' executable surface: an update that leaves the surface unchanged keeps
  approval, and an update that changes it requires re-approval before the hooks
  run again."
- **`template-registry.yml` Entries/Ownership:** drop "`trusted: true` lets the
  hooks…"; state trust is recorded in the approval ledger (own spec), written by
  the trust commands and `add --trust`, never by the local layer.

## Next implementation step

Add `review.rs` with `HookSurface`, `ApprovalDigest`, and `Review::of` over the
already-parsed `template::HookNode` (canonical serde serialization + script
bytes), with `not implemented` bodies for `prior_surface`/`diff_surface`. Unit
test the invariant that adding a field to `HookNode` changes the digest, using a
fixture template. Then wire `Ledger.load_layers`/`contains` into the binary gate,
replacing `registry_trusted`/`trustable`, behind the unchanged `ApplyOptions`.
