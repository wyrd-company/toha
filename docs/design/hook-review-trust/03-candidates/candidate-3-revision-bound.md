# Hook review and trust persistence — candidate 3: revision-bound trust

## Problem

Trust today is a whole-template boolean bound to a template's **name** in a
writable registry, not to the code that would run. `templates update` moves an
installed git template to a new commit and calls `set_commit` but never touches
`trusted`, so a changed `script:` body or `run:` line runs on the next
`apply <name>` with no review and no `--trust`. Approval must instead bind to the
**revision** whose executable surface was reviewed, and stop granting trust once
that surface changes — for git templates and for folder templates alike — while
keeping the interview engine pure and every adapter working.

## Usage (caller's view)

### Quickstart (README fragment)

```
$ toha templates add --trust gh:acme/web           # records trust at the fetched commit
added acme/web
$ toha apply acme/web ./out                         # commit matches approval -> hooks run
...
$ toha templates update acme/web                     # branch advanced, hooks unchanged
updated acme/web                                     # trust carried to the new commit
$ toha templates update acme/web                     # branch advanced, a hook body changed
updated acme/web, review required                    # trust dropped
$ toha apply acme/web ./out                          # approval != installed revision
create ...
hook ["./setup.sh"] cwd .
--- setup.sh (executable, will run) ---
#!/bin/sh
curl https://... | sh                                # the reviewer sees the new body
this template needs trust; re-run with --trust, or: toha templates add --trust acme/web
$ echo $?
3
```

Trust is granted at a revision. It survives a doc-only update (the executable
surface is unchanged, so trust advances to the new commit) and drops the moment
the executable surface changes. Review shows the hook nodes and the bytes of
every in-tree file they can run; on update it shows the change.

### Call site A — apply gate (binary, replaces `registry_trusted` plumbing)

```rust
let resolved = resolve_template(&arg, &config, &registry, dirs, &cwd)?;
let template = load_template(&resolved)?;
// Trust is only consulted when hooks exist; no hooks -> the gate passes anyway.
let trusted = if template.has_hooks() {
    let installed = HookRevision::of(&resolved, &template)?; // git: Commit; folder: Digest
    matches!(evaluate_trust(resolved.approved.as_ref(), &installed), Trust::Trusted)
} else {
    false
};
let plan = Plan::build(&template, &completed, path)?;
plan.apply_reporting(path,
    ApplyOptions { force, trusted: trusted || trust_flag }, // --trust still a blind override
    &ProcessRunner, &mut |f| println!("{f}"))?;
```

### Call site B — `templates update` re-evaluates trust against the new commit

```rust
next = next.set_commit(&key, new_commit.clone())?;
let old = HookSurface::read(&install.join(sub), &old_doc)?;   // old tree, still on disk pre-swap
let new = HookSurface::read(&clone.join(sub), &new_doc)?;     // freshly fetched tree
let was_trusted = entry.approved.as_deref() == Some(&HookRevision::Commit(old_commit).token());
next = if was_trusted && old.digest() == new.digest() {
    next.set_approved(&key, HookRevision::Commit(new_commit.clone()))? // carry: surface unchanged
} else {
    next.clear_approved(&key)?                                          // drop: re-review
};
lines.push(format!("updated {key}{}", if was_trusted && old.digest()==new.digest() {""} else {", review required"}));
```

### Call site C — folder-template fallback (no commit exists)

```rust
// resolved.approved is a Digest token when the folder was added with trust.
let surface = HookSurface::read(&resolved.folder, &doc)?;
let installed = HookRevision::Digest(surface.digest());       // recomputed live from disk
match evaluate_trust(resolved.approved.as_ref(), &installed) {
    Trust::Trusted => /* content byte-identical to what was approved */,
    Trust::NeedsReview => /* folder edited since approval -> exit 3 with the surface */,
}
```

## Shape

### Data structures (single source of truth per invariant)

```rust
// toha::hook::surface  (new; pure, no UI, answer-independent)

/// One hook node exactly as authored, before Jinja render. Hashing the whole
/// parsed node (not enumerated fields) means a future field — e.g. design
/// 1078's `results:` — is covered automatically and cannot silently escape.
pub struct HookNodeSource(serde_norway::Value); // canonical (sorted-key) encoding

/// A file inside the template root a hook can execute: every `script:` target,
/// plus (policy) every `run:`/`args` token that resolves to an in-tree file.
pub struct ScriptFile { pub rel: PathBuf, pub bytes: Vec<u8> }

/// The reviewable executable surface of a template. Independent of answers.
pub struct HookSurface { pub hooks: Vec<HookNodeSource>, pub scripts: Vec<ScriptFile> }
impl HookSurface {
    pub fn read(root: &Path, doc: &TemplateDoc) -> Result<Self, SurfaceError> { unimplemented!() }
    pub fn digest(&self) -> HookDigest { unimplemented!() } // sha256 over canonical encoding
}
pub struct HookDigest([u8; 32]);

/// The identity that binds approval. git -> the resolved commit (cheap, already
/// recorded); folder -> a digest of the executable surface. Variants never
/// compare equal, so a commit approval can never satisfy a folder gate.
#[derive(PartialEq, Eq)]
pub enum HookRevision { Commit(String), Digest(HookDigest) }
impl HookRevision {
    pub fn token(&self) -> String { /* "git:<commit>" | "sha256:<hex>" */ unimplemented!() }
    pub fn parse(token: &str) -> Option<Self> { unimplemented!() }
    /// git templates: the recorded commit; folder templates: the live digest.
    pub fn of(t: &ResolvedTemplate, tpl: &Template) -> Result<Self, SurfaceError> { unimplemented!() }
}

/// The only constructor of "trusted". Equality is the whole policy.
pub enum Trust { Trusted, NeedsReview }
pub fn evaluate_trust(approved: Option<&HookRevision>, installed: &HookRevision) -> Trust {
    if approved == Some(installed) { Trust::Trusted } else { Trust::NeedsReview }
}

/// Presentation input, computed by the library, rendered by drivers.
pub struct HookReview { pub surface: HookSurface, pub prior: Option<HookSurface> }
```

Registry entry — replace the boolean with the approved revision token (the SoT;
`trusted` becomes derived, so the stale-trust state is unrepresentable):

```rust
pub struct Entry {
    pub name: String, pub source: String, pub reference: Option<String>,
    pub commit: Option<String>,      // installed revision (SoT: what is on disk)
    pub path: PathBuf, pub aliases: Vec<String>,
    pub trusted_at: Option<String>,  // approved revision token (SoT: what was reviewed)
}
```

`Listed`/`Resolved` no longer carry a bare `trusted: bool`; they carry
`approved: Option<HookRevision>` (parsed from `trusted_at`) for named, non-local
entries only. `ResolvedTemplate` gains `approved: Option<HookRevision>` and keeps
`commit`; its `trusted` field is removed.

Registry ops (mechanical, sparse round-trip preserved via `FieldPresence` renamed
`trusted -> trusted_at`): `set_approved(formal, HookRevision)`,
`clear_approved(formal)`; `add` carries `trusted_at = new.trusted_at.or(old.trusted_at)`
(a carried token that no longer matches the installed commit simply reads as
NeedsReview — safe by construction). Local layer still rejects the field; merge
precedence follows presence exactly as `trusted` did.

### Seam map

```
template.yml + in-tree files
        │  read (pure, answer-independent)
        ▼
toha::hook::surface ── HookSurface ─ digest() ─► HookRevision
        │                    │
        │                    └─► HookReview (surface + prior)  ─┐
        ▼                                                        │ presented by
cli::resolve  ─ ResolvedTemplate{ commit, approved } ─┐          │ drivers only
        │                                             │          ▼
        ▼                                             │   terminal / dry-run / headless
evaluate_trust(approved, installed) ─► Trust ─────────┴─► ApplyOptions{ trusted }
        │                                                        │
registry.trusted_at (SoT)  ◄── set_approved/clear_approved       ▼
   ▲ add / update reconcile                            Plan::apply gate (unchanged)
```

### Error / results contract

- Exit 3 (`NeedsTrust`) unchanged: hooks present and gate not satisfied → dry
  run listing hooks, now with the reviewable surface, names `--trust` and (for a
  by-name template) `templates add --trust`. `--dry-run` prints the plan + surface
  and exits 0.
- `SurfaceError` (io / non-UTF-8 script / path escapes root) surfaces as an
  ordinary `Outcome::Error`; a template whose surface cannot be read cannot be
  trusted.
- `templates update` prints `updated <name>` (trust carried) or
  `updated <name>, review required` (trust dropped).

### Invariants in types

- "Trusted" is producible only by `evaluate_trust` (equality of revisions) or the
  explicit `--trust` flag; there is no `trusted: bool` to set directly.
- git-approval vs folder-gate can never match (distinct `HookRevision` variants).
- New hook fields are hashed whether or not code knows them (`HookNodeSource`
  hashes the whole node), so field-level review coverage cannot silently regress.
- Installed revision (`commit`) and approved revision (`trusted_at`) are separate
  fields; `set_commit` alone can never re-grant trust.

### What the interview engine must NOT gain

No surface/digest/review/trust knowledge, no prompt or diff port, no I/O. It
still only accepts answers and returns questions. Review is computed in
`hook::surface` and rendered by drivers; trust is decided in `resolve`/gate.
`HookRunner` stays the one library port.

## Tradeoffs accepted

- **A stored digest is avoided for git.** Update reconciliation reads the old
  install tree (present on disk before the swap) and the new clone, so the
  approved digest is recomputed, never persisted. One field (`trusted_at`) is the
  whole store.
- **Commit as the primary key** keeps the gate O(1) for git templates (string
  compare, no file reads) and reuses identity Toha already records. The digest is
  paid only at approval, folder gating, and update reconciliation.
- **Doc-only updates keep trust** because reconciliation advances the approval
  only when the surface digest is unchanged — the direction's central tension,
  resolved as "commit changed AND surface changed → drop".
- **Folder templates cannot show a diff on re-review** (no prior content is
  stored); they show the full current surface. Only git updates show a change.

## Alternatives considered

**Whole-tree / commit-only digest (rejected).** Bind approval to the commit alone
and drop trust on any new commit. Tiny surface, but re-reviews every doc typo —
the over-invalidation the constraints forbid — and for folders there is no commit
at all. Adding the surface-digest reconciliation is the minimum that fixes both.

**Content-addressed trust keyed only by digest, no commit (rejected).** Store the
surface digest for every template and never look at the commit. Uniform, but pays
a full template read on every `apply` of a git template just to gate, discards the
identity already recorded, and makes the gate depend on reading executable files
before it has decided to trust them. Interface is not deeper — it trades a free
key for a computed one on the hot path.

## Open questions and risks (for the human)

1. **Re-review on every commit vs surface-only.** Confirmed direction:
   trust drops only when the executable surface digest changes, not on every new
   commit. Is "surface unchanged ⇒ trust silently advances to the new commit"
   acceptable, or should even a carried update print a one-line notice?
2. **`run: [interpreter, in-tree-file]` bytes escape.** Policy proposed: the
   surface reader resolves each `run`/`args` token against the template root and
   hashes the bytes of any that names an existing in-tree file. This is heuristic
   (a path-shaped arg that is coincidentally a file; a file named via env/stdin we
   cannot see). Accept the heuristic, or restrict in-tree execution to declared
   `script:` only?
3. **Capability narrowing (needs approval).** `add --trust` and `--trust` are
   preserved and still work headlessly, but persisted trust now **drops when the
   executable surface changes** — previously it survived any update. This narrows
   the "trust forever" capability. Approve this as the intended fix?
4. **Future hook fields that reference new files.** Field *presence* is always
   hashed, but a new field (e.g. 1078 `results:`) that introduces a *new*
   executable-file reference needs the surface reader taught to pull those bytes,
   or the referenced file's body (not its reference text) escapes the digest. Gate
   the surface reader behind a test that fails when a hook field is added without a
   coverage decision?
5. **Future bundled template that ships hooks.** The current bundled demo ships no
   hooks, so trust never engages. A future bundled template with hooks has no
   registry entry and no commit — it would fall to the folder digest path against
   the shipped bytes. Is that the intended binding, or do bundled hooks get
   implicit trust?
6. **Where the persist-trust command plugs in (owned by 1071).** After a review,
   persisting the approved revision is `set_approved(formal, installed_revision)`.
   The command surface that triggers it is 1071's to name; this design only
   exposes the library op.

## Proposed edits (shapes, not applied)

**Registry schema** (`template-registry.schema.yml`): replace `trusted:
{type: boolean}` on a registry entry with `trusted-at: {type: string}` (a
`git:<40-hex>` or `sha256:<hex>` token); local-registry `additionalProperties`
continues to reject it.

**CLI spec — Trust paragraph** (replace): "The hooks of a template run when the
template's approved revision equals its installed revision, or when `--trust` is
given. A git template's revision is its resolved commit; a folder template's
revision is a digest of its executable surface (the hook nodes and the bytes of
every in-tree file they can run). Templates from a local templates path and
templates given by address or folder are trusted only through `--trust`. When the
template has hooks and is not trusted, `apply` performs a dry run, lists the hooks
**and their executable content**, states that `--trust` is required, names the
same command with `--trust`, and exits 3; for a template resolved by name it also
names `templates add --trust`."

**CLI spec — Updates paragraph** (replace last sentence): "The formal name and
aliases of an updated template are unchanged. Trust is re-evaluated against the
new commit: an update that does not change the executable surface advances the
approval to the new commit and keeps trust; an update that changes it drops trust
until reviewed."

## Adapter outcomes

- **Interactive terminal:** gate unchanged; NeedsReview + hooks + no `--trust` →
  exit 3 dry run showing the surface. `--trust` runs this once.
- **Headless `--answers`:** identical gate; `--trust` works headlessly (no new
  interactive gate); exit 3 otherwise. `add --trust` binds to the fetched commit.
- **Staged resume:** `resume_template` recomputes `approved` (from `trusted_at`)
  and the installed revision (commit for git; live digest for folder) and calls
  `evaluate_trust`; both inputs are persisted, so the decision survives across
  processes and modalities.
- **Crate / library callers:** `Plan::apply(.., ApplyOptions{trusted}, runner)`
  unchanged; `HookSurface::read`/`digest`, `HookRevision`, and `evaluate_trust`
  are public so a caller computes its own trust decision. Engine untouched.

## Next implementation step

Add `toha::hook::surface` (`HookSurface::read` + `digest`, `HookRevision`,
`evaluate_trust`) with fixture tests that (a) prove a changed `script:` body and a
changed `run:` line both change the digest, (b) prove a doc-only change does not,
and (c) fail when a new hook field is added without a coverage decision. Then
rename `Entry.trusted -> trusted_at` and wire the three call sites.
