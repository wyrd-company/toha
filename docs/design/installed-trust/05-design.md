# Design: trust management for installed templates

The final design for task 1071 (paired implementation 1054), Toha 0.2.0. It is a
design artifact: it describes the target system and the contract edits the
implementation will make. It does not change canonical specifications, schemas, or
runtime code. Grounding is in `01-grounding.md`; the criteria, candidate
exploration, scores, and the base/graft/reject reconciliation are in
`02-arena-rubric.md`, `03-candidates/`, and `04-synthesis.md`.

## Problem

After design 1069, hook-trust is `entry.approval: Option<ReviewDigest>` — a
fingerprint of a template's executable surface, granted for a freshly added
template by `templates add --trust`, and re-evaluated live at every apply through
`evaluate_trust`. What is missing is a way to manage that approval for a template
that is **already installed**: to grant it (after reviewing the hooks, or after a
`templates update` changed the surface and lapsed trust) and to revoke it, **without
fetching, recloning, or moving the installed commit**. Today the only path to
persist trust for an installed template is `templates add --trust <formal>`, which
re-parses an address, re-clones, and in fact refuses a bare installed name — the
wrong operation. And the untrusted-hooks refusal points there. This task adds the
command surface that grants and revokes approval on installed content and redirects
the refusal to it, consuming the 1069 policy without redefining it.

## Usage (caller's view)

### README / quickstart

> Toha runs a template's hooks only after you approve the exact code that would run
> (see *Trust*). To approve the hooks of a template you already installed, first see
> what they are — `toha apply <template> <dir> --dry-run` lists them — then record
> your approval with `toha templates trust <template>`. Approval is a fingerprint of
> the template's *executable surface* as it sits on disk now; `templates trust`
> reads only the installed folder and never fetches or moves the commit. Stop
> trusting it with `toha templates untrust <template>`. When a later `templates
> update` changes any hook or script the fingerprint changes, approval lapses, and
> the next apply asks you to review and `templates trust` again; a documentation-only
> update changes nothing executable, so approval stands. `templates list` shows which
> installed templates are trusted right now.

### Call site A — recover from the apply refusal (interactive, no reclone)

```console
$ toha apply acme-service ./out
hooks will not run without --trust
  hook 1  script: scripts/postgen.sh  args: [--ci]
  hook 2  run: [git, init]
to run them this time: toha apply --trust acme-service ./out
to trust acme-service for every run: toha templates trust acme-service
                                                       # (exit 3; the hooks are listed above)
$ toha templates trust acme-service                    # reads the INSTALLED surface; no fetch
approved surface (stderr):
  hook 1  script: scripts/postgen.sh  args: [--ci]
  hook 2  run: [git, init]
trusted acme-service                                   # (stdout)
$ toha apply acme-service ./out                        # hooks run; approval persists
```

### Call site B — re-approve after an update, then revoke (scriptable)

```console
$ toha templates update acme-service      # surface changed -> approval lapses by policy (no bookkeeping here)
updated acme-service
$ toha templates trust acme-service        # re-records the CURRENT installed surface digest
trusted acme-service
$ toha templates untrust acme-service      # clears approval; reads nothing, cannot fail on content
untrusted acme-service
$ toha templates trust acme-service && toha templates trust acme-service
trusted acme-service
already trusted acme-service               # idempotent; second call writes nothing
```

### Call site C — the binary's dispatch and the crate seam (`main.rs` / `run()`)

```rust
// src/cli/templates.rs — new arms on TemplatesCommand, dispatched by run():
TemplatesCommand::Trust   { template } => trust(&ctx, &template),
TemplatesCommand::Untrust { template } => untrust(&ctx, &template),

// A crate caller manages trust through the same library the command uses:
let surface  = HookSurface::of(&Template::load(&entry.path)?)?;   // installed folder; 1069 lib
let next      = user_registry.set_approval(&formal, Some(surface.digest()))?;  // 1071 mutator
```

## Shape

### Data structures

No new domain type. Trust management is a registry mutation like `alias`: it writes
the `approval` field 1069 already put on `Entry`, through one new immutable mutator.

```rust
// src/registry.rs — the SINGLE writer of the approval field on an existing entry.
impl RegistryFile {
    /// Return a copy with `formal`'s `approval` set (`Some`) or cleared (`None`),
    /// marking FieldPresence so the sparse round-trip persists the change: `Some`
    /// writes the `approval` key, `None` omits it (absent == untrusted, never
    /// `approval: null`). Errs `RegistryError` if `formal` is absent. Touches no
    /// other field — `source`, `ref`, `commit`, `path`, `aliases` are unchanged,
    /// so trust never moves or re-fetches the installed template.
    pub fn set_approval(&self, formal: &str, approval: Option<ReviewDigest>)
        -> Result<RegistryFile, RegistryError>;   // not implemented (1054)
}
```

`ReviewDigest`, `HookSurface`, `evaluate_trust`, and `Entry.approval` are the 1069
types, consumed unchanged. `add`'s existing `entry.approval = entry.approval.or(old.approval)`
re-add behavior is untouched; `set_approval` is the *installed-template* path that
`add` cannot serve.

### Command functions

```rust
// src/cli/templates.rs
fn trust(ctx: &Context, name: &str) -> Result<Vec<String>, CommandError> {
    let (formal, entry) = ctx.resolve_user(name)?;            // exit 1 not-in-user; exit 5 ambiguous
    let template = Template::load(&entry.path)                // INSTALLED folder only; NO source::fetch
        .map_err(CommandError::text)?;
    let surface  = HookSurface::of(&template)                 // ReviewError: missing/unreadable/escaping -> exit 1
        .map_err(|e| CommandError::text(guidance::surface_unreadable(&formal, &e)))?;
    let live     = surface.digest();
    if matches!(evaluate_trust(entry.approval.as_ref(), &live), Trust::Trusted) {
        return Ok(vec![format!("already trusted {formal}")]); // no write; equality ONLY via evaluate_trust
    }
    let next = ctx.user.set_approval(&formal, Some(live)).map_err(CommandError::text)?;
    ctx.write(&next)?;                                        // one atomic write, user layer only
    eprintln!("{}", guidance::approved_surface(&surface));    // audit echo to STDERR; presentation only
    Ok(vec![format!("trusted {formal}")])
}

fn untrust(ctx: &Context, name: &str) -> Result<Vec<String>, CommandError> {
    let (formal, entry) = ctx.resolve_user(name)?;            // same refusals
    if entry.approval.is_none() {
        return Ok(vec![format!("already untrusted {formal}")]); // no write, idempotent
    }
    let next = ctx.user.set_approval(&formal, None).map_err(CommandError::text)?;
    ctx.write(&next)?;                                        // NEVER touches HookSurface
    Ok(vec![format!("untrusted {formal}")])
}
```

clap arms (no options, no groups, no runtime exclusivity check — a missing operand
is clap's own exit 2):

```rust
/// Approve the installed hooks of a template so its apply runs them.
Trust   { /// Formal, short, or alias name of an installed template.
          template: String },
/// Revoke approval of a template's hooks.
Untrust { /// Formal, short, or alias name of an installed template.
          template: String },
```

### Module / seam map

```
 clap TemplatesCommand::{Trust,Untrust}{ template }
        |  run()  (short chain: dispatch -> fn -> library)
        v
 ┌──────────────── src/cli/templates.rs ─────────────────┐
 │ trust(ctx, name)                 untrust(ctx, name)    │
 │   resolve_user ───────────────────── resolve_user      │  exit 1 not-in-user / exit 5 ambiguous
 │   Template::load(entry.path)  [installed; NO fetch]     │
 │   HookSurface::of ─┐                     │              │
 │   evaluate_trust ──┤               (no surface read)    │
 │   set_approval(Some)               set_approval(None)   │
 │   eprintln! approved_surface (stderr)                   │
 └────────┬───────────┴──────────────────┬────────────────┘
          v                              v
   src/review.rs (1069)            src/registry.rs (1071 mutator)
   HookSurface::of / digest        RegistryFile::set_approval
   evaluate_trust  (the ONE        (writes user_data/templates.yml,
   trust equality)                  atomic, sparse; no other field)
          |
          v
   guidance.rs: trust_lines names `templates trust`; retry() + formal_for gain the arms
   list(): effective TRUSTED column via evaluate_trust per entry (per 1069)
```

No arm touches `source::` (fetch/clone) or `set_commit`; the "no reclone / no commit
change" invariant is structural, not a runtime check.

### Error / results contract

| Situation | Result (stdout) | Exit | Message source |
| --- | --- | :-: | --- |
| trusted (write happened) | `trusted {formal}` + surface echo on stderr | 0 | — |
| untrusted (write happened) | `untrusted {formal}` | 0 | — |
| already trusted (`evaluate_trust` == Trusted) | `already trusted {formal}`, no write | 0 | — |
| already untrusted (`approval` is `None`) | `already untrusted {formal}`, no write | 0 | — |
| name not in user registry (system/local/discovered/unknown) | — | 1 | `guidance::not_in_user_registry` (reused) |
| installed surface missing / unreadable / escaping (trust only) | — | 1 | `guidance::surface_unreadable` (new) |
| ambiguous name | — | 5 | `guidance::ambiguous` + `retry` (new arms) |
| missing operand | — | 2 | clap |

Result lines return through the existing `Ok(Vec<String>)` → stdout path; the
grant's surface echo is the only stderr output and is advisory. Idempotency is
defined *through* `evaluate_trust`: a template whose surface changed since its
approval is not "already trusted" — `trust` re-records the current digest and
reports `trusted`. The command never compares two digests with `==` itself.

### Invariants encoded

- **Only a user-layer entry can hold approval.** `resolve_user` returns from the
  user `RegistryFile`; `set_approval` is keyed by that formal and called on
  `ctx.user` alone. A system/local/discovered-only template hits
  not-in-user-registry (exit 1); the local-registry schema still forbids `approval`.
- **Approval binds installed content, never name or commit.** The digest comes only
  from `HookSurface::of(entry.path)`; no `Address`, clone, or `commit` is in scope.
- **The trust rule lives once.** `evaluate_trust` is the sole comparator — for the
  idempotency check here and for `list`. The commands hold no equality; the surface
  renderer is presentation and compares nothing.
- **`untrust` cannot fail on the surface.** Its body has no `HookSurface` call;
  revocation is total and depends on nothing on disk.
- **Trust never moves the template.** `set_approval` touches only the `approval`
  field, so `commit`/`path`/`source` are byte-identical after a grant or revoke.

### What the interview engine must NOT gain

Nothing. Trust management is a registry-mutation command alongside `alias`,
`remove`, and `update`. It never touches the interview engine, adds no prompt port,
and introduces no interactive gate; the engine stays answer → question.

## Behaviors to prove (falsifiable)

- **Grant records the installed surface, no fetch.** On an installed, untrusted
  template, `templates trust <name>` writes `entry.approval == HookSurface::of(
  installed).digest()` in the user registry and leaves `commit`/`path`/`source`
  byte-identical; run offline, it still succeeds. *Central test.*
- **Revoke clears approval and reads nothing.** `templates untrust <name>` removes
  the `approval` key (sparse round-trip omits it) and succeeds even when the
  installed folder is absent/unreadable.
- **Idempotency through the chokepoint.** A second `trust` with an unchanged surface
  prints `already trusted` and writes nothing; after a surface change the same
  command re-records and prints `trusted`. A second `untrust` prints
  `already untrusted` and writes nothing.
- **Update → lapse → re-trust round-trip.** `trust`; a `templates update` that
  changes a hook/script lapses effective trust (next apply exits 3); `trust` again
  re-approves the new surface with no fetch; a doc-only update keeps trust.
- **Refusal names the new operation.** `apply` and `apply --dry-run` of an
  untrusted installed template name `templates trust <formal>` (not `add --trust`);
  an ambiguous name re-offers `templates trust <each-formal>` (exit 5).
- **Layer boundary.** `trust`/`untrust` on a system-only, local-only, discovered, or
  unknown name fail with `not_in_user_registry` (exit 1) and write nothing.
- **Unreadable surface cannot be trusted.** A `script:` file missing or escaping the
  template root makes `trust` exit 1 with `surface_unreadable`; the registry is
  unchanged.
- **Sole-kill (for implementation 1054).**
  - Route the `trust` idempotency check through raw `==` instead of `evaluate_trust`
    → the "re-trust after a surface change re-records" test must fail.
  - Make `set_approval` also write `commit`/`path` → the "commit/path byte-identical
    after grant" test must fail.
  - Give `untrust` a `HookSurface::of` call → the "untrust succeeds with an
    unreadable installed folder" test must fail.
  - Leave `trust_lines` naming `templates add --trust` → the "refusal names
    `templates trust`" test must fail.
  - Key `set_approval`/`resolve_user` off the merged registry instead of the user
    file → the "system-only template is refused not-in-user-registry" test must fail.

## Compatibility

- **Predecessor (1069 / 1033).** Consumes the approved 1069 revision
  (`05-design.md` @ `132b2f4`) field for field: `entry.approval: Option<ReviewDigest>`,
  `HookSurface::of/digest/diff`, `evaluate_trust`, the effective-trust `list` column,
  and the local-layer `approval` ban. It redefines none of them. The 1069
  implementation (1033) need not exist for this design; 1054 depends on it to build.
  If the approved 1069 interface changes, 1071 returns to design review.
- **`add --trust` and `apply --trust` unchanged.** `add --trust` still records
  approval for freshly added content; `apply --trust` still grants one run. No grant
  flow is removed or narrowed.
- **Legacy on-disk state.** Pre-v1, no users. A legacy entry with no `approval`
  reads as untrusted; `templates trust` grants it. No migration step (consistent
  with 1069 Decision 4).
- **Downstream (1074 bundled demo, 1075 Jinja context).** Approval keys on content,
  orthogonal to selected-template identity; these commands add no new identity
  coupling.

## Canonical-document impact (described, not applied — owned by implementation 1054)

These edits are specified here and made by 1054 in the same change, never in this
design package. They compose with the 1069/1033 edits to the same files.

- **`command-line-interface.spec.yml`** — add two subcommands under `templates`,
  each with one required positional `template` (`type: string`), no options, no
  groups, exit codes `0`, `1`, `2`, `5`:

  ```yaml
  - name: trust
    summary: Approve the installed hooks of a template so its apply runs them.
    mode: non-interactive
    positional-arguments:
      - { name: template, position: 0, required: true,
          description: Formal, short, or alias name of an installed template., value: { type: string } }
    output:
      exit-codes:
        - { code: 0, meaning: Trust was granted, or was already granted. }
        - { code: 1, meaning: The template is not in the user registry, or its installed executable surface is missing, unreadable, or escapes the template folder. }
        - { code: 2, meaning: Invalid command line. }
        - { code: 5, meaning: The template name is ambiguous. }
  - name: untrust
    summary: Revoke approval of a template's hooks.
    mode: non-interactive
    positional-arguments:
      - { name: template, position: 0, required: true,
          description: Formal, short, or alias name of an installed template., value: { type: string } }
    output:
      exit-codes:
        - { code: 0, meaning: Trust was revoked, or was already absent. }
        - { code: 1, meaning: The template is not in the user registry. }
        - { code: 2, meaning: Invalid command line. }
        - { code: 5, meaning: The template name is ambiguous. }
  ```
- **`command-line-interface.yml` — Trust paragraph** — after the 1069 rewrite, add
  that `templates trust <name>` records approval of an installed template's current
  executable surface (reading the installed folder only, never fetching or changing
  the commit) and `templates untrust <name>` removes it; the untrusted-hooks refusal
  for a name-resolved installed template names `templates trust <name>` in place of
  `templates add --trust`.
- **`command-line-interface.yml` — Updates paragraph** — state that
  `templates trust <name>` re-approves the currently installed surface without
  fetching after an update lapses approval (composing with the 1069 wording that an
  update changing the executable surface makes the template untrusted).
- **`template-registry.yml`** — under Ownership, add `templates trust` and
  `templates untrust` to the commands that write the user registry.
- **`template-registry.schema.yml`** — no change beyond 1069's `approval` field; the
  local-registry `$def` still forbids it.

## Out of scope

- The trust *policy* and the digest/`evaluate_trust`/`HookSurface` library — owned by
  design 1069 / implementation 1033; consumed here.
- The apply gate, exit-3 contract, and dry-run listing — unchanged.
- A dedicated read-only `templates review` verb — the hook listing is already served
  by `apply`'s exit-3 refusal and `apply --dry-run`; adding one would duplicate them.
- The `--digest <sha>` freshness pin and any diff-since-approval display — recorded
  as future extensions (Decision B); the single-digest field and single-operand
  verbs grow into them without breaking callers.
- Trusting a system-only template from the user side — every mutating `templates`
  command operates on the user registry only; unchanged.

## Reviewer focus (what the adversarial reviewer will attack)

- Does any path re-implement the trust equality outside `evaluate_trust` (the raw
  `==` regression that sank Candidate 1)?
- Does `set_approval` touch any field but `approval`, or does any path reach
  `source::`/`set_commit`, so a grant could move or re-fetch the template?
- Is the not-in-user-registry boundary keyed off the *user* file, not the merged
  registry, so a system-only template cannot silently gain a user approval?
- Does the refusal name `templates trust` in every adapter (interactive, dry-run,
  ambiguous retry), and does the sparse round-trip truly omit a cleared approval?

## Decisions needed from Bob (options + recommendation)

1. **Command shape (the task's Decision 1).** How a caller grants/revokes trust for
   an installed template.
   - **A. Verb pair `templates trust <name>` + `templates untrust <name>`
     (recommended).** Flagless and legible; two verbs are *less* clap machinery than
     a toggle for a one-operand inverse pair and make the trust-reads / untrust-reads-
     nothing asymmetry visible; the refusal breadcrumb is a clean `templates trust
     <formal>`. Scored highest on fidelity, scriptability, and legibility.
   - B. Dual-mode `templates trust <name>` / `templates trust --remove <name>` (the
     task's first-listed option, mirroring `alias`). Rejected: it is a degenerate
     mirror (drops `alias`'s `all-or-none` group), and `--remove <TEMPLATE>` reads
     awkwardly beside `alias --remove <ALIAS>`.
   - C. Review-centric `templates review <name> [--approve|--revoke]`. Rejected: the
     read-only listing is already `apply --dry-run`; a mutating `review` verb
     overloads a read name and widens the surface.
   Recommendation: **A**.

2. **Scriptable freshness pin `--digest <sha>` on `trust`.** Approve only if the
   installed surface still equals a caller-supplied digest, else fail — a CI drift
   gate (Candidate 3's idea).
   - A. **Defer (recommended).** Keep the baseline the clean verb pair. The atomic
     read→digest→write already closes the review→approve window, and `evaluate_trust`
     re-checks at apply time. Adding `--digest` later is backward-compatible.
   - B. Include now, with a distinct exit code for drift.
   Recommendation: **A (defer)**.

3. **Refusal wording (the task's Question 3).** The untrusted-hooks refusal for a
   name-resolved installed template names `templates trust <formal>` in place of
   `templates add --trust <formal>`, and the hooks stay listed above it (so the
   caller sees what they would trust). No option here beyond confirming the wording;
   this resolves Question 3 as "yes, name the new operation."

*Question 2 ("does trusting record the commit so a later update needs trust
again?") is settled by the approved 1069 policy — approval binds the executable-
surface digest, independent of commit — and is not reopened.*

## Permissions / access and related disclosures

Called out separately because design inclusion alone does not grant
approval:

- **Permissions / access change (requires approval).** These commands grant and
  revoke the permission to run a template's hooks, by writing/clearing
  `entry.approval` in the **user** registry. This is a new operator surface for a
  security-relevant permission. It grants no permission the operator did not already
  hold (`add --trust` and `apply --trust` already grant hook execution); it only adds
  a first-class, no-fetch way to manage it on installed templates, and revocation
  where none cleanly existed.
- **Filesystem read scope (permissions/access).** `templates trust` reads the bytes
  of the installed template's executed files via `HookSurface::of` — a read within
  `entry.path` (the template root) only; no write beyond the user registry, no
  network, no out-of-root access. (The read itself is the 1069 Decision 2 surface,
  already approved; disclosed here as the operation that triggers it.)
- **No supported capability is removed or narrowed.** `add --trust`, `apply
  --trust`, per-run trust, and the layer model all keep working. Redirecting the
  refusal away from `add --trust` (which cannot serve an installed name) is a
  correction, not a removal.
- **No new timeout mechanic, no pinned-version check, and no application-subprocess
  integration** are introduced.

## Open questions and risks

- Should the grant's stderr surface echo be a fixed concise format (count + one line
  per hook) or reuse the exact dry-run hook listing for a single presentation? (Lean:
  reuse the dry-run renderer so there is one surface presentation.)
- `untrust` of an already-lapsed approval reports `untrusted` (it clears a stored
  digest that no longer matched). Is one word right, or is a distinct
  "cleared a lapsed approval" message worth it? (Lean: one word.)
- Effective-trust `list` cost is a per-entry digest recompute (owned by 1069); a
  single-entry query is cheap, which is why no `review`/`list-trust` verb is added.
  Confirm 1069's `list` recompute is acceptable at expected installed counts.

## Next implementation step

Add the `Trust`/`Untrust` arms to `TemplatesCommand` and the `trust`/`untrust`
functions in `src/cli/templates.rs`, backed by a new `RegistryFile::set_approval`,
with `not implemented` bodies first so the seam is reviewable; then wire `retry()`,
`formal_for`, and `trust_lines` in `src/cli/guidance.rs` to name `templates trust`,
and land the CLI-prose, machine-spec, and registry-ownership edits.
