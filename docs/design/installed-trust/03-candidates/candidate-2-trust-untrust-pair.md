# Candidate 2: two plain verbs — `trust` and `untrust`

## Problem

A caller must grant and revoke hook-trust for an *already-installed* template
without fetching, recloning, or moving the installed commit. Trust reads the
installed template folder (`entry.path`) and writes exactly one file — the user
registry — through the same sparse, atomic path every other mutating `templates`
arm uses. The datum written is `entry.approval: Option<ReviewDigest>` from design
1069: a content fingerprint over the executable surface, independent of name and
commit. The operation must stay non-interactive (scripts must pass it), must route
the trust equality through `evaluate_trust` (no re-implementation, no leak of the
rule), and must reuse the established refusals — not-in-user-registry (exit 1) and
ambiguous name (exit 5). The apply gate and its exit-3 contract are not touched;
this command only writes the datum that gate reads.

## Usage (caller's view)

README / quickstart:

```console
# Approve the hooks of a template you already installed, as they sit on disk now.
toha templates trust acme-service

# Revoke that approval; the next apply that plans hooks refuses again.
toha templates untrust acme-service

# See which installed templates are effectively trusted right now.
toha templates list
```

Real call sites:

```console
$ toha templates trust gh:acme/scaffold#service
trusted gh:acme/scaffold#service

$ toha templates trust gh:acme/scaffold#service      # idempotent, no write
already trusted gh:acme/scaffold#service

$ toha templates untrust svc                          # short name resolves
untrusted gh:acme/scaffold#service

$ toha apply svc ./out                                # hooks now refused
hooks will not run without --trust
to run them this time: toha apply --trust svc ./out
to trust gh:acme/scaffold#service for every run: toha templates trust gh:acme/scaffold#service
```

`run()`-level dispatch (the whole crate seam this task adds):

```rust
TemplatesCommand::Trust   { template } => trust(&ctx, &template),
TemplatesCommand::Untrust { template } => untrust(&ctx, &template),
```

## Shape

### Why two verbs, not options on an existing command

I evaluated expressing trust as options before committing to verbs:

- **Relax `add` to accept an installed name (`add --trust <name>`).** `add`'s
  body *is* a fetch: it parses an `Address`, clones a sibling, swaps it in, and
  refuses a bare name on purpose (`add_needs_address`). Re-approving installed
  content is the one thing `add` must *not* do (no fetch on trust). Threading a
  non-fetch branch through `add` would split its single responsibility and
  contradict its own refusal. Loses.
- **A `--trust` / `--untrust` flag on `list`.** `list` is a pure read with layer
  filters; a mutating flag makes a read write the registry, and `list` has no
  single-template operand to name. Loses.
- **Alias-style dual mode (`trust <name>` + `trust --remove <name>`).** This is
  the closest idiom, and the brief's foil. `alias` earns its `--remove` flag
  because add and remove operate over a *two-operand relation* (template, alias)
  with genuine operand asymmetry — remove needs only the alias, add needs both —
  so clap carries three conflict rules (`conflicts_with_all`,
  `required_unless_present`, `requires`) plus a runtime re-check. Trust and untrust
  each take exactly **one** operand, a template name. There is no operand
  asymmetry to encode, so a `--remove` flag would be pure ceremony hiding one verb
  behind another. Worse, it would paper over a *real* asymmetry the flag can't
  show: trust reads the installed surface and can fail on it; untrust reads nothing
  and cannot. Two sibling verbs make that asymmetry legible in `--help` and let the
  refusal name `templates trust <formal>` with no flag.

So the verb pair is not just minimal-ceremony by taste — for a one-operand inverse
pair with divergent reads, two arms are *less* clap machinery than one dual-mode
arm, and they encode the read/no-read split in the surface itself.

### clap subcommands

```rust
/// Approve the installed hooks of a template so its apply runs them.
Trust {
    /// Formal or short name of an installed template.
    template: String,
},
/// Revoke approval of a template's hooks.
Untrust {
    /// Formal or short name of an installed template.
    template: String,
},
```

No option groups, no conflicts, no `required_unless_present`, no runtime
exclusivity check. A missing operand is clap's own exit-2.

### Signatures and bodies (sketch)

```rust
fn trust(ctx: &Context, name: &str) -> Result<Vec<String>, CommandError> {
    let (formal, entry) = ctx.resolve_user(name)?;            // exit 1 / exit 5, reused
    let template = Template::load(&entry.path).map_err(CommandError::text)?;
    let surface = HookSurface::of(&template)                 // reads entry.path bytes only
        .map_err(|e| CommandError::text(guidance::unreadable_surface(&formal, &e)))?;
    let live = surface.digest();
    if matches!(evaluate_trust(entry.approval.as_ref(), &live), Trust::Trusted) {
        return Ok(vec![format!("already trusted {formal}")]); // no write; equality via chokepoint
    }
    let next = ctx.user.set_approval(&formal, Some(live))     // single writer of approval
        .map_err(CommandError::text)?;
    ctx.write(&next)?;                                        // user_data/templates.yml, atomic, sparse
    Ok(vec![format!("trusted {formal}")])
}

fn untrust(ctx: &Context, name: &str) -> Result<Vec<String>, CommandError> {
    let (formal, entry) = ctx.resolve_user(name)?;
    if entry.approval.is_none() {
        return Ok(vec![format!("already untrusted {formal}")]); // no write, idempotent
    }
    let next = ctx.user.set_approval(&formal, None).map_err(CommandError::text)?;
    ctx.write(&next)?;
    Ok(vec![format!("untrusted {formal}")])                  // NEVER touches HookSurface
}
```

New registry method — the single source of truth for writing the approval field,
immutable-style like `set_commit` / `add_alias` / `remove`:

```rust
impl RegistryFile {
    /// Set or clear the approval of one entry; `None` writes nothing (absent = untrusted).
    /// Marks FieldPresence so the sparse round-trip persists the change.
    pub fn set_approval(&self, formal: &str, approval: Option<ReviewDigest>)
        -> Result<RegistryFile, RegistryError>; // not implemented — err if formal absent
}
```

### Module / seam map

```text
                 clap TemplatesCommand
                 ├── Trust   { template }
                 └── Untrust { template }
                          │  run()  (short chain: dispatch → fn → library)
                          ▼
        ┌───────────────── templates.rs ─────────────────┐
        │ trust(ctx, name)              untrust(ctx, name) │
        │   resolve_user ───────────────── resolve_user    │  (exit 1 / exit 5 shared)
        │   Template::load(entry.path)        │            │
        │   HookSurface::of ─┐                 │            │
        │   evaluate_trust ──┤            (no surface read) │
        │   set_approval(Some)            set_approval(None)│
        └─────────┬──────────┴────────────────┬────────────┘
                  ▼                            ▼
             review.rs                    registry.rs
   HookSurface::of / digest        RegistryFile::set_approval
   evaluate_trust  (the ONLY       (writes user_data/templates.yml
   trust equality)                  atomically, sparse round-trip)
```

`list` gains an *effective-trust* read (no new command): for each listed entry it
computes `evaluate_trust(entry.approval.as_ref(), &HookSurface::of(load)?.digest())`
and prints `Trusted` / `NeedsReview` in the `TRUSTED` column; `--json` carries both
the stored `approval` string and the effective `trust`. A row whose surface is
unreadable or escaping degrades to `NeedsReview` for that row only — it never fails
the whole listing, and it can never read as `Trusted` when the content can't be read.

### Error / results contract

| Situation | Result | Exit |
|---|---|---|
| trusted (write happened) | `trusted {formal}` | 0 |
| untrusted (write happened) | `untrusted {formal}` | 0 |
| already trusted (approval == live) | `already trusted {formal}`, no write | 0 |
| already untrusted (approval None) | `already untrusted {formal}`, no write | 0 |
| name not in user registry | `guidance::not_in_user_registry(name)` | 1 |
| unreadable/escaping/missing surface (trust only) | `guidance::unreadable_surface(formal, err)` | 1 |
| ambiguous name | `CommandError::Ambiguous` → `guidance::ambiguous` | 5 |
| missing operand | clap usage error | 2 |

Idempotency is defined *through* `evaluate_trust`: "already trusted" is exactly
`Trust::Trusted`, so a template whose surface changed since its approval is **not**
"already trusted" — `trust` re-records the current digest and reports `trusted`.
The command never compares two digests with `==` itself.

### Invariants encoded

- **Only a user-layer entry can hold approval** — `resolve_user` returns from the
  user `RegistryFile`; `set_approval` is keyed by that formal. A system or local
  template not in the user registry hits not-in-user-registry (exit 1); the local
  layer still cannot grant trust.
- **Approval binds real installed content, never name/commit** — the digest comes
  only from `HookSurface::of(entry.path)`; no `Address`, clone, or `commit` is in
  scope. Type `ReviewDigest`.
- **The trust rule lives once** — `evaluate_trust` is the sole comparator, used
  both for the idempotency check and by `list`. The command holds no equality.
- **untrust cannot fail on the surface** — its body has no `HookSurface` call at
  all; revocation is total and depends on nothing on disk.

### Exact refusal wording (replaces `add --trust` line)

`trust_lines` keeps its first "run once" line and changes only the persist line:

```text
to run them this time: toha apply --trust svc ./out
to trust gh:acme/scaffold#service for every run: toha templates trust gh:acme/scaffold#service
```

Built as `toha("templates trust", &[], &[value(formal_for("templates trust", formal))])`.
`formal_for` gains `"templates trust"` and `"templates untrust"` to the arm that
returns the registry-stored formal (alongside `templates remove/update/alias`),
since these look a name up rather than read an address. `retry()` gains
`Trust`/`Untrust` arms mapping to `"templates trust"` / `"templates untrust"`.

### Spec edits

- **`command-line-interface.spec.yml`**: two new subcommands, each with one
  required positional `template`, zero options, zero groups, exit-codes `0, 1, 2, 5`.
- **`command-line-interface.yml` (prose)**: the Trust paragraph names
  `templates trust` / `templates untrust` in place of `templates add --trust`; the
  Updates paragraph states that an update changing the executable surface lapses
  approval and a `templates trust` re-records it — no commit is bound.
- **`template-registry.schema.yml`**: unchanged beyond 1069 (`approval` optional
  string `^sha256:[0-9a-f]{64}$`; local layer forbids it). Absence means untrusted;
  `untrust` writes absence.

## Tradeoffs accepted

- We accept **two entries in `--help` and the spec** in exchange for a legible,
  flagless surface and *less* clap machinery than an alias-style dual mode.
- We accept **silent, non-interactive approval** (trust prints only the formal
  name, not the surface) in exchange for scriptability and zero rule leakage; a
  caller who wants to see the surface first runs `apply --dry-run`, which already
  prints the untrusted-hooks refusal.
- We accept an **effective-trust `list` recompute** (one `Template::load` +
  `HookSurface::of` per row) in exchange for `list` never showing a stale
  `Trusted`. Cost is bounded by installed count; unreadable rows degrade to
  `NeedsReview`.
- We accept that **trust re-approves a changed surface without a diff prompt**
  (records whatever is installed now) in exchange for a single, deterministic
  command; the diff is available through `apply --dry-run` / a future view.

## Alternatives considered

**Alias-style dual mode `templates trust [--remove] <name>`.** One clap arm,
one spec subcommand — superficially tidier. It lost because the two operations
share a single operand with no asymmetry (so `--remove` buys nothing clap needs),
and it *hides* the one real asymmetry: trust reads and can fail on the installed
surface, untrust reads nothing. A dual-mode verb would need prose to explain "with
`--remove` the surface is not read," where two verbs say it by existing. It also
forces the refusal to emit a flagged form (`templates trust --remove …`) for the
revoke breadcrumb, which reads worse than a plain `untrust`.

**A review-then-approve pair (`templates review` + `templates approve`).** Rejected
up front: it manufactures the TOCTOU the brief warns about — content can change
between the review a caller saw and the approve that records a digest — and adds a
verb whose only job is printing. Collapsing read-and-approve into one `trust`
command removes that window entirely.

## Open questions and risks

- **Blind approval:** Should `trust` echo the recorded digest (e.g.
  `trusted {formal} sha256:1a2b…`) so a script has a comparable record, or stay
  name-only to avoid any hint of the rule? Does echoing a *truncated* digest leak
  anything an attacker could grind against?
- **trust→apply TOCTOU:** Is it enough that `evaluate_trust` at apply time
  recomputes the live digest — so content changed after `trust` re-triggers the
  gate — or does any caller need a "trust is still valid" check between the two?
- **list cost at scale:** At what installed-template count does the per-row
  `HookSurface::of` recompute in `list` become worth caching the last-known
  effective trust, and where would such a cache live without becoming a second
  source of truth beside `evaluate_trust`?
- **untrust of a lapsed approval:** `untrust` reports `untrusted` even when the
  stored approval no longer matches the surface (already effectively `NeedsReview`).
  Is clearing-when-already-lapsed worth a distinct message, or is one word right?

## Next implementation step

Add the `Trust`/`Untrust` clap arms and their `trust`/`untrust` functions in
`src/cli/templates.rs`, backed by a new `RegistryFile::set_approval`, with the
refusal and `formal_for`/`retry` edits in `src/cli/guidance.rs`.
