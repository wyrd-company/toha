# Hook review and content-addressed trust — candidate 1

Structural direction: **content-addressed trust digest stored on the registry
entry**. Approval records the digest of a template's executable surface; hooks
run only while that digest still matches the installed content. `templates
update` needs no special "drop trust" step — a changed surface produces a
different digest, so the stored approval simply stops matching.

## Problem

Trust today is a whole-template `trusted: bool` bound to the template's *name*
in a writable registry. `templates add --trust` sets it blind, and `templates
update` re-clones a branch to a new commit while leaving `trusted` untouched
(`command-line-interface.yml:55-56`; `templates.rs:505-566`). So an update that
rewrites a `script:` body or a hook `run:` line keeps stale trust, and the next
`apply <name>` runs the changed code with no review and no `--trust`. We need
approval bound to the **reviewed executable content**, review possible before a
trusted install and before an update keeps trust, coverage that survives future
hook-syntax growth, and every adapter plus interview-engine purity preserved.

## Usage (caller's view)

Quickstart (README voice):

> Toha runs a template's hooks only after you approve them. Approval is
> recorded as a fingerprint of the template's *executable surface* — its hook
> definitions and the bytes of any `script:` file. Review the hooks with
> `toha apply <template> <dir> --dry-run`, then approve with `--trust` (this
> run only) or `toha templates add <addr> --trust` (recorded). When a later
> `templates update` changes any hook or script, the fingerprint changes, the
> recorded approval no longer matches, and the next apply asks you to review
> and approve again.

Call site A — library caller checks/reviews a template (crate):

```rust
let template = Template::load(&folder)?;              // post-!include
let surface = HookSurface::of(&template)?;            // pure, UI-free
let live = surface.digest();                          // ReviewDigest

match stored_seal {                                   // Option<TrustSeal> from registry
    Some(seal) if seal.digest == live => { /* trusted: run */ }
    Some(seal) => {                                    // approved, but changed
        let prior = HookSurface::of(&template_at(seal))?; // optional: reconstruct old
        present(surface.diff(&prior));                 // driver renders what changed
    }
    None => present(&surface),                         // never approved: show content
}
```

Call site B — the binary's apply gate (`main.rs`), replacing the `bool` seam:

```rust
let template = load_template(&resolved)?;
let live = HookSurface::of(&template)?.digest();
let trusted = trust                                    // --trust one-run flag
    || resolved.seal.as_ref().is_some_and(|s| s.digest == live);
// apply.rs is unchanged: it still takes a decided bool and returns NeedsTrust.
plan.apply_reporting(path, ApplyOptions { force, trusted }, &ProcessRunner, &mut on_written)
```

On the not-trusted path the existing exit-3 flow lists the hooks; when a seal
exists but no longer matches, the listing marks them "changed since approval".

Call site C — `templates update` re-evaluates trust for free:

```rust
// after swap_clone installs the new commit:
let new = HookSurface::of(&Template::load(&install)?)?.digest();
if entry.seal.as_ref().is_some_and(|s| s.digest != new) {
    lines.push(format!("updated {formal} (hooks changed; approval lapsed)"));
    // The re-approval command surface is owned by design 1071; name it there.
}
// The seal is NOT rewritten. Effective trust is always the live compare.
```

## Shape

Data structures.

```rust
// src/review.rs — NEW library module. Pure, no I/O beyond reading script bytes.
pub struct HookSurface { hooks: Vec<HookView> }        // interview nodes, then top-level
struct HookView {
    origin: HookOrigin,          // Interview{index} | TopLevel{index}
    node: serde_json::Value,     // canonical hook node, post-!include, PRE-render
    scripts: Vec<ScriptDigest>,  // every `script:` file the node references
}
struct ScriptDigest { path: RelPath, sha256: [u8; 32] }

pub struct ReviewDigest(String);          // "sha256:<64 hex>", constructor-guarded
pub struct HookSurfaceDiff { /* added / removed / changed HookViews */ }

impl HookSurface {
    pub fn of(template: &Template) -> Result<Self, ReviewError>;
    pub fn digest(&self) -> ReviewDigest;                 // JCS-canonical, deterministic
    pub fn diff(&self, prior: &HookSurface) -> HookSurfaceDiff;
}
```

`node` is the **raw canonical JSON of the parsed hook node** (RFC 8785: sorted
keys, minimal forms), not a re-serialization of the Rust domain type. This is
the load-bearing choice for requirement 5: a future opt-in field (e.g. Jinja
"hook results") is present in the parsed node by construction, so it enters the
digest **without any change to review code**. `Template::load` retains the
post-`!include` hook nodes as `Vec<Value>` (the single source the digest reads);
they are already parsed and validated, so `additionalProperties:false` keeps the
node set to exactly the known schema at any version.

Digest input, precisely — "changed executable input" is:
1. each hook node's canonical JSON (all fields: `run`/`script`/`args`/`cwd`/
   `when`/`each` and any future field), in surface order, and
2. the bytes of every referenced `script:` file (as `sha256`), framed with its
   template-relative path.
Everything else in the template (files, questions, messages) is **excluded**, so
a doc-only or unrelated update leaves the digest — and the approval — intact.
The identity bound to approval is this content digest, per entry; it is
independent of formal name, branch, or commit.

Registry datum (replaces the bool — single source of truth, nothing to sync):

```rust
pub struct Entry { /* name, source, ref, commit, path, aliases */
    pub trust: Option<TrustSeal>,   // was: pub trusted: bool
}
pub struct TrustSeal { pub digest: ReviewDigest }
```

`FieldPresence.trusted` becomes `.trust`; sparse round-trip, merge precedence
(local < user < system, higher-omitted inherits lower), and `add`'s
inherit-on-re-add all carry over unchanged in mechanism. `add`'s
`entry.trusted |= old.trusted` becomes `entry.trust = entry.trust.or(old.trust)`
— safe, because an inherited seal grants trust only if content still matches.

Module / seam map.

```
                 approval datum (digest)            executable-surface digest
 templates.yml ────────────────► registry.rs ──► resolve.rs (ResolvedTemplate.seal)
                                                        │
 template.yml ──load(+!include)──► template.rs ──► review.rs  HookSurface::of / digest / diff
   (raw hook nodes Vec<Value>)          │                    (pure, UI-free)
                                        ▼                          │
                                    main.rs apply gate: trusted = --trust || seal==live
                                        │
                                        ▼
                            apply.rs  (bool in → NeedsTrust out; UNCHANGED)
                                        │
                                        ▼
                            HookRunner (the one port; UNCHANGED)
```

Error / results contract. `ReviewError` = I/O reading a `script:` file (missing
/ unreadable). `digest()` is infallible once the surface is built. `apply.rs`
keeps `Applied::{Written, NeedsTrust}` and exit code 3; `--trust`, the dry-run
listing, and target-path printing are unchanged. `templates list`'s TRUSTED
column becomes an *effective* value: `seal.is_some_and(|s| s.digest == live)`,
computed by loading each entry's template (unreadable install ⇒ `false`).

Invariants in types (hard-to-misuse first).
- No stored `trusted:bool` can drift from content: trust is **always** the live
  compare of a stored digest to a computed digest.
- `ReviewDigest` is constructed only by the canonical function or parsed from
  disk under a `sha256:<64hex>` pattern — not hand-forgeable elsewhere.
- `HookSurface::of` reads only a loaded `Template`, so it cannot miss an
  `!include`d hook, and its input is whole-node JSON, so a new field cannot
  silently escape review. Test-encoded: mutating **any** field of any hook node
  in `template.yml` changes the digest.

What the interview engine must NOT gain: no review/prompt port, no trust or
digest knowledge, no UI. It stays answer→question. Review and digest live in
`review.rs` and the binary; `HookRunner` remains the only library port.

## Tradeoffs accepted

- Trust is recomputed at each apply (and per row in `list`). Cost is a small
  parse the apply already does plus `sha256` of a few script files — negligible,
  and it buys a single source of truth with no update-time trust bookkeeping.
- Approval survives cosmetic churn (whitespace inside a script changes the
  digest; renaming an unrelated question does not) — intended, matches "avoid
  over-invalidation" and "don't miss a changed script body".
- Content-addressing means two entries with byte-identical hook surfaces still
  hold independent seals (approval is per entry), so no cross-template trust
  leakage.

## Alternatives considered

- **Keep `trusted:bool`; invalidate on `commit` change.** Rejected: over-
  invalidates on doc-only updates (a new commit that touches no hook drops
  trust), misses folder templates that have no commit, and leaks git identity
  into a trust decision. Shallow interface — the datum answers the wrong
  question.
- **Digest the rendered `PlannedHook` argv.** Rejected: rendered argv is
  answer-dependent, so trust would vary per interview run and could not be
  sealed at `add` time. Couples trust to the interview; wrong layer.
- **Digest via `#[derive(Serialize)]` over the domain `HookNode`.** Rejected
  against raw canonical JSON: a future `#[serde(skip)]`, rename, or
  `skip_serializing_if` would silently drop a field from the digest, so a new
  hook field could escape review — exactly the failure requirement 5 forbids.
  Raw whole-node JSON needs no code change when the schema grows.

## Open questions and risks (for the human)

1. **`run: [interpreter, in-tree-file]` bytes escape.** `run: [python,
   scripts/x.py]` digests the argv template but not `scripts/x.py`'s bytes; an
   update that rewrites `scripts/x.py` without touching the `run:` line keeps a
   matching digest and runs changed code. Args are Jinja, so files referenced
   through substitution are undetectable pre-render. Recommendation (needs
   approval — it bounds a capability): additionally hash any `run` argv token
   that is a **literal** path (no Jinja) resolving inside the template root.
   Accept residual gap for substituted paths. Approve, or accept declared-
   `script:`-only coverage?
2. **Future bundled template that ships hooks.** A no-registry bundled template
   has no entry to seal (design 1074 confirms today's bundled demo ships no
   hooks, so trust never engages). If a future bundled template ships hooks,
   how does it carry approval — a built-in seal shipped with the binary, or
   `--trust` each run? Flagged, not settled here.
3. **`list` effective-trust cost / display.** Computing live digests to show the
   TRUSTED column loads every listed template. Acceptable, or show "sealed" vs
   "sealed (changed)" without a full recompute?
4. **Legacy `trusted: true` on disk.** Pre-v1, no users; a bare `trusted:true`
   would be re-approved rather than migrated. Confirm no migration is wanted.
5. **Dead seals after update.** Keep a now-mismatched seal (harmless; re-matches
   if content reverts) or prune it? Pruning nudges `list` toward "not trusted"
   but loses the revert convenience. Recommend keep.

Capability check: `templates add --trust` and the `--trust` one-run flag are
unchanged and remain fully headless (they seal / assert without an interactive
gate), so no existing grant capability is narrowed. The one deliberate
behavioral change is `update` no longer carrying stale trust — the intended fix,
realized as a spec edit below.

## Proposed spec edits (shapes, not applied)

Registry schema (`template-registry.schema.yml`), `$defs.entry.properties`:

```yaml
# remove:  trusted: { type: boolean, default: false }
trust:
  type: object
  additionalProperties: false
  required: [digest]
  properties:
    digest: { type: string, pattern: '^sha256:[0-9a-f]{64}$' }
```

Local-registry `$def` still forbids `trust` (as it forbids `trusted`), so a
project never grants trust.

CLI spec (`command-line-interface.yml`) — **Trust** paragraph (revised):

> The hooks of a template run when the caller has approved the template's
> current executable surface in the user or system template registry, or when
> `--trust` is given. Approval is recorded as a digest of the executable surface
> — the template's hook nodes and the bytes of every `script` file — and
> authorizes the hooks only while that digest equals the digest of the
> installed content. Templates from a local templates path never carry approval,
> and templates given by address or folder to `stage` or `apply` run hooks only
> through `--trust`. When the template has hooks and is not trusted, `apply`
> performs a dry run, lists the hooks, states that `--trust` is required, names
> the same command with `--trust`, and exits with code 3; for a template
> resolved by name it also names `templates add --trust` with the formal name.
> When a template was approved but its executable surface has since changed, the
> dry run and the needs-trust listing mark the hooks as changed since approval.
> [remaining dry-run / exit-0 sentences unchanged.]

CLI spec — **Updates** paragraph (revised last sentence):

> The formal name and aliases of an updated template are unchanged. Its recorded
> approval is unchanged and continues to authorize the hooks only while the
> executable surface is unchanged; an update that changes the executable surface
> makes the template untrusted until it is approved again.

Registry spec (`template-registry.yml`) **Entries**: replace "`trusted: true`
lets the hooks of the template run without `--trust`." with "`trust` records the
digest of the executable surface the caller approved; the hooks run without
`--trust` only while that digest equals the digest of the installed content."
**Discovery** / **Ownership**: "is not trusted" → "carries no approval".

## Adapter outcomes

- **Interactive terminal:** apply loads the template, computes the live digest,
  compares to the seal; unchanged approval runs hooks, changed/absent hits the
  same exit-3 review path (now marked "changed since approval").
- **Headless `--answers`:** identical gate; `--trust` still approves for the run,
  `templates add --trust` still seals — no new interactive gate, fully scriptable.
- **Staged resume across processes:** on resume the entry's seal and the template
  at the staged commit are re-resolved and re-digested; a between-stage update
  that changed hooks naturally needs trust at apply. `ResolvedTemplate` carries
  `seal: Option<TrustSeal>` instead of `trusted: bool`.
- **Crate/library callers:** `HookSurface::of/digest/diff` is the public port for
  computing and presenting review; `Plan::apply(.., ApplyOptions{trusted})` is
  unchanged. The library computes; the caller presents.

## Next implementation step

Add `src/review.rs` with `HookSurface`, `ReviewDigest`, and `digest()` over the
canonical JSON of the post-`!include` hook nodes plus `script:` byte hashes;
have `Template::load` retain those raw nodes; add the property test that any
hook-field mutation changes the digest. Types and signatures land first with
`not implemented` bodies so the seam is reviewable before wiring the gate.
