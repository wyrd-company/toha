# Candidate 1: symmetric `templates trust` (mirror of `templates alias`)

## Problem

A caller has an installed template and wants to grant or revoke hook-trust for
it **without fetching, recloning, or moving the commit**: trust reads the
*installed* template folder and writes the *user* registry only. The approval
model is already fixed by design 1069 — approval is a content fingerprint
(`entry.approval: Option<ReviewDigest>` over the executable surface), produced
through `HookSurface::of`/`digest` and judged by the single chokepoint
`evaluate_trust`. This task owns only the *command surface*: how a caller
grants approval to an installed template, how a caller revokes it, and how the
untrusted-hooks refusal names that operation instead of the current
`templates add --trust`. Constraints inherited hard: user-layer-only writes,
sparse round-trip, local layer cannot grant trust, no new interactive gate a
script cannot pass, no re-implementation of the trust equality, no name- or
commit-bound trust.

## Usage (caller's view)

README / quickstart lines:

```console
$ toha templates trust acme-service        # approve the installed surface
trusts gh:acme/templates#service
approved surface (to stderr):
  hook 1  script: scripts/postgen.sh  args: [--ci]
  hook 2  run: [git, init]

$ toha templates trust --remove acme-service   # revoke; no surface read
untrusts gh:acme/templates#service

$ toha templates trust acme-service         # idempotent, no write
gh:acme/templates#service already trusts the installed surface
```

Real call sites:

1. **Refusal-driven, scriptable.** `toha apply acme-service ./out` refuses
   untrusted hooks (exit 3) and its stderr now names `toha templates trust
   acme-service`. A CI script runs exactly that line, then re-runs `apply`. The
   grant prints the approved surface to **stderr**; stdout stays empty, so the
   script is unaffected while a human reading the log sees what was approved.

2. **Re-approve after an update.** `toha templates update acme-service` moves a
   branch and changes a hook script; trust lapses by policy (digest mismatch in
   `evaluate_trust`). The operator reviews `git diff` in the installed repo,
   then `toha templates trust acme-service` re-records the *current* surface
   digest — no fetch, no clone.

3. **Crate / `run()` level.** The binary dispatches
   `TemplatesCommand::Trust { template, remove }` into
   `trust(&ctx, template, remove) -> Result<Vec<String>, CommandError>`,
   identical in shape to the existing `alias(&ctx, template, name, remove)`.

## Shape

**Load-bearing decision:** one dual-mode subcommand, the `alias` idiom *minus
its second operand*. `alias` needs two groups because it has two positionals
(`add-alias` all-or-none over `[template, alias]`, `operation` exactly-one over
`[template, remove]`). `trust` binds a single name, so the all-or-none group has
no analogue and `conflicts_with_all` collapses to `conflicts_with`. Keeping only
what a single operand requires is the honest minimal mirror; inventing a second
operand for symmetry's sake would be cargo-culting.

clap definition (new arm on `TemplatesCommand`):

```rust
/// Grant or revoke hook trust for an installed template.
Trust {
    /// Formal or short name of an installed template to trust.
    #[arg(required_unless_present = "remove")]
    template: Option<String>,
    /// Installed template to untrust.
    #[arg(short, long, conflicts_with = "template", value_name = "TEMPLATE")]
    remove: Option<String>,
},
```

Command function (mirrors `alias`, `remove`, `update` — same `resolve_user`
path, same immutable `RegistryFile` update, same single user-file write):

```rust
fn trust(
    ctx: &Context,
    template: Option<String>,
    remove: Option<String>,
) -> Result<Vec<String>, CommandError> {
    // Untrust: no surface read, cannot fail on content.
    if let Some(name) = remove {
        if template.is_some() {
            return Err(CommandError::text("-r cannot be combined with a template"));
        }
        let (formal, entry) = ctx.resolve_user(&name)?;
        if entry.approval.is_none() {
            return Ok(vec![format!("{formal} is already untrusted")]);
        }
        let next = ctx.user.set_approval(&formal, None).map_err(CommandError::text)?;
        ctx.write(&next)?;
        return Ok(vec![format!("untrusts {formal}")]);
    }

    // Grant: read the *installed* surface, record its digest.
    let name = template.ok_or_else(|| CommandError::text("a template is required"))?;
    let (formal, entry) = ctx.resolve_user(&name)?;
    let loaded = Template::load(&entry.path).map_err(CommandError::text)?; // installed folder only
    let surface = HookSurface::of(&loaded)?;   // ReviewError -> exit 1; no fetch
    let digest = surface.digest();
    if entry.approval.as_ref() == Some(&digest) {
        return Ok(vec![format!("{formal} already trusts the installed surface")]);
    }
    let next = ctx.user.set_approval(&formal, Some(digest)).map_err(CommandError::text)?;
    ctx.write(&next)?;
    eprintln!("{}", render_surface(&loaded));      // presentation only; not the digest rule
    Ok(vec![format!("trusts {formal}")])
}
```

New immutable registry method (mirrors `set_commit`; single source of truth for
the `approval` field + its `FieldPresence`):

```rust
impl RegistryFile {
    /// Record `approval` for `formal` (or clear it), marking the field written.
    /// `None` clears it, so a sparse round-trip omits `approval`.
    pub fn set_approval(&self, formal: &str, approval: Option<ReviewDigest>)
        -> Result<RegistryFile, RegistryError>; // not implemented
}
```

`render_surface` is **presentation, not policy**: it lists the loaded
template's hook nodes and executed files for a human reading stderr. It never
touches `evaluate_trust` and never compares digests — no leakage of the trust
rule.

Seam map:

```
clap TemplatesCommand::Trust { template, remove }
        |                         (exit 2 on missing/both — clap groups)
        v
run() ---> trust(ctx, template, remove)
        |         |
        |         +--> Context::resolve_user  --> not_in_user_registry (1) / Ambiguous (5)
        |         +--> Template::load(entry.path)  [installed folder — NO fetch]
        |         +--> review.rs: HookSurface::of --> ReviewError (1)
        |         |               HookSurface::digest  (the whole policy)
        |         +--> RegistryFile::set_approval  --> Context::write (atomic, user only)
        |         +--> eprintln! render_surface  (stderr; scriptable)
        v
guidance::retry  (adds "templates trust" arm)
list() ---> evaluate_trust(entry.approval, live_digest)  per entry  (effective column)
```

**Error / results contract.** `Ok(Vec<String>)` → stdout lines: `trusts {formal}`,
`untrusts {formal}`, `{formal} already trusts the installed surface`,
`{formal} is already untrusted`. Errors: `CommandError::Error` (exit 1) for
not-in-user-registry, `Template::load` failure, and `ReviewError`
(missing/unreadable/escaping executed file); `CommandError::Ambiguous` (exit 5)
for an ambiguous name; clap (exit 2) for a missing operand or both operand and
`-r`. No-ops report and exit 0 **without writing** (no atomic-write mtime
churn).

**Invariants encoded in types.** `approval: Option<ReviewDigest>` where
`ReviewDigest` is 1069's validated `sha256:<64hex>` newtype — a malformed digest
is unrepresentable. `set_approval` exists only on `RegistryFile` and `trust`
calls it on `ctx.user` alone, so the user-layer-only invariant is structural;
the schema forbidding `approval` in the local `$def` keeps local un-trustable.
`resolve_user` requiring the name in the *user* file means a system/local-only
template cannot be trusted — it errors with the existing
`not_in_user_registry` pattern. The trust *decision* lives only in
`evaluate_trust`; this command produces and stores a digest but never judges
one.

**Refusal wording (guidance.rs).** `trust_lines` changes its installed line from
`templates add --trust` to the new operation:

```rust
lines.push(format!(
    "to trust {formal} for every run: {}",
    toha("templates trust", &[], &[value(formal_for("templates trust", formal))])
));
```

Rendered:

> `hooks will not run without --trust`
> `to run them this time: toha apply acme-service ./out --trust`
> `to trust gh:acme/templates#service for every run: toha templates trust gh:acme/templates#service`

`formal_for` gains `"templates trust"` in its registry-form arm (alongside
`templates remove | update | alias`): trust looks a name up in the registry, it
does not read an address, so no drive-path munging. `retry` gains
`TemplatesCommand::Trust { .. } => ("templates trust", None)` so an ambiguous
trust re-offers each match as `toha templates trust <formal>` (exit 5).

**Effective-trust `list` column.** Post-1069 `Listed.trusted` is computed, not
stored. `list` resolves each entry's installed surface
(`HookSurface::of` + `digest`) and calls
`evaluate_trust(entry.approval.as_ref(), &live)`; a `ReviewError` on an entry
renders as `NeedsReview` (an unreadable surface can never be `Trusted`, which
is the safe reading). The `TRUSTED` text column and `--json` `"trusted"` field
become this effective boolean. Cost is one parse-plus-hash per listed entry;
`list` is not a hot path, so this candidate pays it rather than adding a
`--fast` flag that would fork the surface.

**Spec edits.**
- *CLI prose (`command-line-interface.yml`) — Trust paragraph:* rewrite to name
  `toha templates trust <name>` as the way to trust an installed template and
  `toha templates trust --remove <name>` to revoke; `add --trust` keeps its role
  for freshly added content only. *Updates paragraph:* state that a
  `templates update` which changes the executable surface lapses trust and one
  that does not keeps it, and that `templates trust <name>` re-approves the
  currently installed surface without fetching.
- *`apply` / `apply --dry-run` exit-code prose:* the untrusted-hooks message for
  an installed template names `templates trust`, not `add --trust`.
- *Machine spec (`command-line-interface.spec.yml`) — new subcommand under
  `templates`:*

```yaml
- name: trust
  summary: Grant or revoke hook trust for an installed template.
  mode: non-interactive
  positional-arguments:
    - name: template
      position: 0
      required: false
      description: Formal or short name of an installed template to trust.
      value:
        type: string
  options:
    - name: remove
      flags: [ -r, --remove ]
      kind: option
      description: Installed template to untrust.
      value:
        type: string
  groups:
    - name: operation
      members: [ template, remove ]
      rule: exactly-one
  notes:
    - Trust reads the installed template folder and writes the user registry; it never fetches or changes the installed commit.
    - Granting trust records the digest of the currently installed executable surface and prints that surface to standard error.
  output:
    exit-codes:
      - code: 0
        meaning: Trust was granted or revoked, or was already in that state.
      - code: 1
        meaning: >-
          The template is not in the user registry, or its installed executable
          surface is missing, unreadable, or escapes the template folder.
      - code: 2
        meaning: Invalid command line.
      - code: 5
        meaning: The template name is ambiguous.
```

- *Registry schema:* no new change beyond 1069's — `approval`
  (`^sha256:[0-9a-f]{64}$`) replaces `trusted`; the local-registry `$def` still
  forbids it. Consumed, not redefined.

## Tradeoffs accepted

- **We accept "trust binds whatever is installed now, printed only to stderr"
  in exchange for full scriptability and a structurally closed TOCTOU.** There
  is no separate "review" command whose output the caller must confirm; the
  grant is atomic read→digest→write. A human sees the approved surface on
  stderr; a script ignores it. We do not add an interactive confirmation,
  because that would be a supported-capability regression.
- **We accept the per-entry digest recompute in `list`** in exchange for a
  truthful effective-trust column and no second code path.
- **We accept a single subcommand doing two things (grant/untrust)** in
  exchange for exact idiom-match with `alias`, which every existing caller and
  the CLI spec already understand.

## Alternatives considered

**Two verbs: `templates trust <name>` + `templates untrust <name>`.** Cleaner
read for the revoke case (no `-r` mode), and untrust reads as its own
operation. It lost because it breaks the established dual-mode idiom: `alias`
already models add/remove as one subcommand with `-r`, and adding a *second*
top-level verb for the inverse would make trust the odd command out and grow
the `TemplatesCommand` enum without new capability. A **`--review`/two-step
approve** alternative (print surface, caller re-runs to approve) was rejected
outright: it *introduces* the review→approve TOCTOU the brief flags, and either
adds an interactive gate or a second flag a script must thread. The single
atomic grant has strictly less surface and strictly less risk.

## Open questions and risks

- **TOCTOU is closed by construction here — is that enough?** Grant is one
  atomic read→digest→write, so there is no review/approve window; the only gap
  is between `templates trust` and a later `apply`, which `evaluate_trust`
  already closes by lapsing on any surface change. Do we still want the grant to
  echo the digest itself (not just the human surface) so a caller can pin it?
- **Should `render_surface` be owned by `review.rs` or by the command?** It is
  presentation, but it reads the same nodes `HookSurface` hashes; is a shared
  renderer worth the coupling, or does the command keeping its own display
  better preserve the "equality is the whole policy" boundary?
- **Effective-`list` on an unreadable entry** renders `NeedsReview` and swallows
  the `ReviewError` — is a silent downgrade the right reading, or should `list`
  surface a per-row note?
- **No-op exit 0 without writing:** acceptable, or should an already-trusted
  grant still be observable (e.g. a distinct message) for scripts that assert on
  output?

## Next implementation step

Add the `Trust { template, remove }` arm to `TemplatesCommand` with the
`exactly-one` operation group and stub `trust(&ctx, ...)` plus
`RegistryFile::set_approval` as `not implemented`, then thread the guidance and
spec edits.
