# Candidate 3: review is the verb, trust is its outcome

## Problem

A caller must grant and revoke hook-trust for an *already-installed* template
without any fetch, reclone, or commit change: trust reads the installed template
folder and writes only the user registry (`user_data/templates.yml`, atomic,
sparse). Approval is the fixed 1069 policy — `entry.approval:
Option<ReviewDigest>`, a `sha256` fingerprint over the executable surface,
produced and compared only through `src/review.rs` (`HookSurface::of`,
`.digest()`, `evaluate_trust`). The design must preserve interview-engine purity,
user-layer-only writes, the local-layer trust ban, `evaluate_trust` as the single
trust chokepoint, and the existing ambiguity (exit 5) and not-in-user-registry
(exit 1) refusals. It must add no interactive gate (scriptability), must not
rebind trust to a name or commit, and must not leak or re-implement the trust
equality. It must also answer three risks: TOCTOU between reviewing content and
approving it, blind approval of content the caller never saw, and the cost of an
effective-trust listing.

The obvious shape mirrors `alias`: `templates trust <name>` / `templates trust
--remove <name>`. That is a *boolean toggle* that writes a digest the caller never
sees — it reproduces today's blind `add --trust` and needs a *second* command to
inspect the surface. This candidate refuses that split. Because 1069 makes trust a
statement about *content the caller reviewed*, the command that grants trust
should be the command that *shows what is being trusted*.

## Usage (caller's view)

README lines:

```sh
# Show the hooks an installed template runs, and whether they are trusted now.
toha templates review acme/web

# Trust the installed executable surface for every run.
toha templates review acme/web --approve

# Trust it only if it still matches the surface you reviewed (fails if it drifted).
toha templates review acme/web --approve=sha256:0f1e...<64 hex>

# Stop trusting it; hooks need trust again.
toha templates review acme/web --revoke
```

Real call sites:

1. **Recovering from the apply refusal.** `toha apply acme/web ./out` prints
   `hooks will not run without --trust` and now names `toha templates review
   acme/web`. The caller runs that (reads the hook list), then `toha templates
   review acme/web --approve`, then re-runs `apply` — no reclone, trust persists.

2. **CI drift-gate (scriptable, no prompt).** A pipeline reviews a surface once,
   stores the digest, and thereafter runs
   `toha templates review acme/web --approve=sha256:<pinned>`. If an operator
   updated the template and the surface changed, the pinned digest no longer
   matches the installed surface and the command exits `1` — trust is not silently
   moved to content nobody reviewed.

3. **Crate / `run()` level.** The binary constructs
   `templates::run(TemplatesArgs { command: TemplatesCommand::Review { template,
   approve, revoke } }, dirs, cwd)`; `run` dispatches to `review(&ctx, &template,
   op)`, where `op: TrustOp` is derived from `(approve, revoke)` at the clap
   boundary.

## Shape

### Load-bearing decision

**One subcommand, `review`, with a three-way operation whose default is
read-only.** Where `alias` uses an `exactly-one` operation group (add xor
remove), `review` uses a *mutually-exclusive* group `{approve, revoke}` with a
meaningful empty default: inspect. Inspecting *is* the trust surface — it renders
the executable surface and the `evaluate_trust` verdict for one entry. Granting
and revoking are options on that same inspection, so the caller approves exactly
what the command just showed. This is the structural divergence from the
mirror-alias shape and the answer to the blind-trust risk without a gate.

### clap definition (new arm on `TemplatesCommand`)

```rust
/// Show the hooks an installed template runs, and grant or revoke trust.
Review {
    /// Formal, short, or alias name of an installed template.
    template: String,

    /// Trust the installed executable surface. Bare: trust it as installed now.
    /// `--approve=<digest>`: trust only if the surface still matches <digest>.
    #[arg(
        long,
        value_name = "DIGEST",
        num_args = 0..=1,
        require_equals = true,
        value_parser = ReviewDigest::parse,      // parse at the boundary; bad syntax -> exit 2
        conflicts_with = "revoke",
    )]
    approve: Option<Option<ReviewDigest>>,

    /// Remove any approval, so hooks need trust again.
    #[arg(long, conflicts_with = "approve")]
    revoke: bool,

    /// Print the surface and verdict as JSON.
    #[arg(long)]
    json: bool,
},
```

### Types (invariants encoded)

```rust
/// The operation the caller asked for. Exhaustive and mutually exclusive by
/// construction: approve and revoke cannot both be present.
enum TrustOp {
    Inspect,
    Approve(Option<ReviewDigest>),  // None = approve live; Some = approve iff live == given
    Revoke,
}

impl TrustOp {
    fn from_flags(approve: Option<Option<ReviewDigest>>, revoke: bool) -> Self {
        match (approve, revoke) {
            (Some(pin), false) => Self::Approve(pin), // pin: None (bare) | Some(digest)
            (None, true)       => Self::Revoke,
            (None, false)      => Self::Inspect,
            (Some(_), true)    => unreachable!("clap conflicts_with rules this out"),
        }
    }
}
```

- `ReviewDigest` is the 1069 newtype; parsing at the clap boundary means an
  ill-formed `--approve=` value fails as an invalid command line (exit 2) before
  any domain logic — external input parsed once, at its edge.
- `Approve(Option<ReviewDigest>)` carries the blind-vs-confirmed choice in *one*
  variant, not a `(bool, Option<String>)` pair that could contradict itself.
- The trust verdict is produced *only* by `evaluate_trust`. The command performs
  exactly one other digest comparison — the `--approve=<digest>` freshness check
  `given == live` — which is a *different* question (did the surface drift since
  the caller reviewed it?) than trust (`approved == live`) and is named as such.

### Command function

```rust
fn review(ctx: &Context, name: &str, op: TrustOp, json: bool)
    -> Result<Vec<String>, CommandError>
{
    let (formal, entry) = ctx.resolve_user(name)?;          // exit 5 ambiguous; exit 1 not-in-user
    let template = Template::load(&entry.path)              // INSTALLED folder only; NO fetch
        .map_err(CommandError::text)?;
    let surface = review::HookSurface::of(&template)        // ReviewError: missing/unreadable/escaping
        .map_err(|e| CommandError::text(guidance::surface_unreadable(&formal, &e)))?;
    let live = surface.digest();
    let trust = review::evaluate_trust(entry.approval.as_ref(), &live); // the ONLY trust rule

    match op {
        TrustOp::Inspect => {
            // read-only; renders the surface + verdict (human or --json). No write.
            Ok(render_review(&formal, &surface, &live, trust, json))
        }
        TrustOp::Approve(pin) => {
            if let Some(given) = &pin {
                if *given != live {                          // freshness check, not trust
                    return Err(CommandError::text(
                        guidance::surface_drifted(&formal, given, &live)));
                }
            }
            if matches!(trust, review::Trust::Trusted) {     // already the approved surface
                return Ok(vec![format!("already trusted {formal}")]); // no-op, exit 0, no write
            }
            let next = ctx.user.set_approval(&formal, Some(live.clone()))
                .map_err(CommandError::text)?;
            ctx.write(&next)?;                               // one atomic user-registry write
            Ok(vec![format!("trusted {formal}")])
        }
        TrustOp::Revoke => {
            if entry.approval.is_none() {
                return Ok(vec![format!("already untrusted {formal}")]); // no-op, exit 0, no write
            }
            let next = ctx.user.set_approval(&formal, None)  // clears field presence: sparse omit
                .map_err(CommandError::text)?;
            ctx.write(&next)?;
            Ok(vec![format!("untrusted {formal}")])
        }
    }
}
```

New registry mutator (single source of truth for the write; mirrors
`set_commit`/`add_alias`):

```rust
impl RegistryFile {
    /// Return a copy with `formal`'s approval set (writes the field) or cleared
    /// (drops it from FieldPresence so the sparse round-trip omits it). Errs if
    /// `formal` is absent. Touches no other field — commit, ref, path, source
    /// are unchanged, so trust never moves the installed template.
    pub fn set_approval(&self, formal: &str, approval: Option<ReviewDigest>)
        -> Result<RegistryFile, RegistryError>;
}
```

### Module / seam map

```
cli::templates::review(name, op, json)
      | Context::resolve_user(name)        -> (formal, Entry)   [exit5 ambiguous | exit1 not-in-user]
      | Template::load(entry.path)         -> Template          [INSTALLED folder only; NO source::fetch]
      | review::HookSurface::of(&template) -> HookSurface        [ReviewError: missing/unreadable/escaping]
      |   .digest()                        -> ReviewDigest (live)
      | review::evaluate_trust(approval, live) -> Trust          [THE single trust chokepoint]
      | match op:
      |    Inspect        -> render surface + verdict            [no write]
      |    Approve(None)  -> set_approval(formal, Some(live))
      |    Approve(pin)   -> pin == live ? set_approval : drift refuse
      |    Revoke         -> set_approval(formal, None)
      v
   Context::write(user RegistryFile) -> user_data/templates.yml  [atomic, sparse, user layer only]
```

No arm of this touches `source::` (fetch/clone) or the install path — the
"no reclone / no commit change" invariant is structural, not a runtime check.

### Error / results contract

| Situation | Producer | Exit | Message source |
| --- | --- | --- | --- |
| Ambiguous name | `resolve_user` -> `CommandError::Ambiguous` | 5 | `guidance::ambiguous` + `retry` (add `Review` arm) |
| Name not in user registry (system/local/discovered/unknown) | `resolve_user` | 1 | `guidance::not_in_user_registry` (reused) |
| Installed files missing / unreadable / escaping | `HookSurface::of` -> `ReviewError` | 1 | `guidance::surface_unreadable` (new) |
| `--approve=<digest>` no longer matches installed surface | drift check | 1 | `guidance::surface_drifted` (new) |
| Bad `--approve=` digest syntax | clap `value_parser` | 2 | clap |
| `--approve` and `--revoke` together | clap `conflicts_with` | 2 | clap |
| Already trusted (approve) / already untrusted (revoke) | `review` | 0 | no-op line, no write |
| Inspect | `review` | 0 | surface + verdict |

`--json` inspect emits `{ "formal_name", "trusted": bool, "digest":
"sha256:...", "hooks": [ ...surface nodes... ] }` so a script can read the verdict
and the exact surface without parsing human text. `trusted` is derived from
`evaluate_trust` — the command never re-derives it.

### Refusal wording (Question 3)

`trust_lines` in `guidance.rs` currently appends, for a name-resolved installed
template:

> `to trust {formal} for every run: toha templates add --trust {formal}`

Replace it so the refusal names the operation that trusts *installed* content with
no fetch, and points the caller at the review that shows what they are trusting:

> `to see the hooks it runs: toha templates review {formal}`
> `to trust them for every run: toha templates review --approve {formal}`

Built with the existing helpers:

```rust
if let Some(formal) = installed {
    let named = formal_for("templates review", formal); // registry lookup form (like remove/alias)
    lines.push(format!(
        "to see the hooks it runs: {}",
        toha("templates review", &[], &[value(named)])));
    lines.push(format!(
        "to trust them for every run: {}",
        toha("templates review", &["--approve".into()], &[value(named)])));
}
```

Add `"templates review"` to the `formal_for` arm that returns the stored formal
name unchanged (it is a name-lookup command, not an address command, like
`remove`/`update`/`alias`). Add a `TemplatesCommand::Review` arm to `retry()`
returning `("templates review", None)` so an ambiguous name re-offers the review
command with each formal name.

### Spec edits

**Machine CLI spec** (`command-line-interface.spec.yml`) — new subcommand under
`templates`:

```yaml
- name: review
  summary: Show the hooks an installed template runs, and grant or revoke trust.
  mode: non-interactive
  positional-arguments:
    - name: template
      position: 0
      required: true
      description: Formal, short, or alias name of an installed template.
      value: { type: string }
  options:
    - name: approve
      flags: [ --approve ]
      kind: option            # optional value (0..=1, require-equals)
      value:
        type: string
        required: false
        pattern: '^sha256:[0-9a-f]{64}$'
      description: >-
        Trust the installed executable surface. With a digest, trust it only if the
        surface still matches; without, trust it as installed now.
    - name: revoke
      flags: [ --revoke ]
      kind: flag
      description: Remove any approval, so hooks need trust again.
    - name: json
      flags: [ --json ]
      kind: flag
      description: Print the surface and verdict as JSON.
  groups:
    - name: operation
      members: [ approve, revoke ]
      rule: mutually-exclusive   # NOT exactly-one: no option = read-only inspect
  output:
    modes:
      - { name: human-readable, machine-readable: false }
      - { name: json, machine-readable: true }
    exit-codes:
      - code: 0
        meaning: >-
          The surface was shown, or trust was granted or revoked (a no-op when
          already trusted or already untrusted).
      - code: 1
        meaning: >-
          The template is not in the user registry, its installed files cannot be
          read, or --approve=<digest> no longer matches the installed surface.
      - code: 5
        meaning: The template name is ambiguous.
```

**CLI prose** (`command-line-interface.yml`) — the Trust paragraph, rewritten as
clean copy: "Hooks run only for a trusted template. `templates review <name>`
shows the executable surface an installed template runs and whether it is trusted.
`templates review <name> --approve` trusts the surface as installed; with a digest
it trusts only a surface that still matches. `templates review <name> --revoke`
removes trust. Approval binds the executable surface, not the name or commit, so
an update that leaves the surface unchanged keeps trust and one that changes it
needs review again." The Updates paragraph drops any claim that update re-arms
trust by commit.

**Registry schema / `Entry`** — inherited from 1069, unchanged by 1071:
`approval: {type: string, pattern: '^sha256:[0-9a-f]{64}$'}`, local layer forbids
it. Revoke clears `FieldPresence` for `approval` so the sparse round-trip omits
the key entirely (an absent key, never `approval: null`).

## Tradeoffs accepted

- We accept a **richer single subcommand** (a three-way op with a read-only
  default) instead of two mirror subcommands, in exchange for approving *exactly
  the surface the command just showed* — the direct mitigation of blind trust —
  and a **cheap single-entry trust query** (`review <name>` recomputes one
  digest) that sidesteps the whole-registry recompute cost of a trust-aware
  `list`.
- We accept that **inspect prints but does not gate**: scripts stay unblocked, and
  callers who need TOCTOU safety must opt in with `--approve=<digest>`. The
  one-shot `--approve` is still blind-to-the-eye-but-visible-on-screen, matching
  today's non-interactive contract.
- We accept **no diff-since-approval in this command**: only a digest was
  persisted, not the prior surface, so `HookSurface::diff` cannot run here (it
  belongs to `update`, which holds both surfaces in one session). `review` reports
  *what runs now* and *trusted / needs review*, not *what changed*.
- We accept **one more digest comparison** in the code (`--approve=<digest>`
  freshness) beyond `evaluate_trust`; it is a distinct concept (drift since review)
  and is named so, not a second copy of the trust rule.

## Alternatives considered

- **Mirror `alias`: `templates trust <name>` / `templates trust --remove
  <name>`.** The safe, symmetric shape. It lost because trust here is a statement
  about *reviewed content*: a bare toggle writes a digest the caller never sees
  (blind approval, today's flaw) and needs a *separate* `review`/`list-trust`
  command to inspect the surface — splitting one concern across two verbs. Its
  `--remove <NAME>` operand also reads awkwardly next to `alias --remove <ALIAS>`
  (there you remove the alias string; here you would remove by template name).
- **`templates trust` plus a separate `templates review`.** Two commands, two
  resolutions and loads, and a genuine TOCTOU window: the caller reviews in one
  invocation and trusts in the next, and the installed surface can shift between
  them. Folding both into one command with `--approve=<digest>` closes that window
  in a single invocation.
- **Options on `apply`/`add`.** `add --trust <installed-name>` is exactly what the
  task forbids: `add` refuses a bare name and would reclone. Rejected by
  constraint.

## Open questions and risks

- Should the apply refusal name **both** `review` and `review --approve` (two
  lines, teaches the safe flow) or only `review --approve` (one line, matches the
  terse one-intent-per-line idiom)?
- Should `list --json` inline effective trust + digest (a per-entry digest
  recompute across the whole registry) now that `review <name>` gives a cheap
  targeted answer — or should `list` stay trust-name-only and defer the effective
  verdict to `review`?
- Is the printed surface a **spec'd machine format** (define the `hooks` JSON
  shape as contract) or human-only with `--json` best-effort?
- Should a bare `review <name>` exit non-zero when the verdict is `NeedsReview`, so
  CI can use it as a check — or does that violate "0 = ok" and belong solely to the
  `--approve=<digest>` drift gate (exit 1)?
- Does `--approve=<digest>` drift deserve its **own exit code** (distinct from
  not-in-registry and unreadable) so scripts can tell drift from a hard error?

## Next implementation step

Add the `Review` arm and `TrustOp` to `src/cli/templates.rs` with a `review`
function and a `RegistryFile::set_approval` mutator, wire `retry()` and
`formal_for` for `"templates review"`, rewrite `trust_lines` to name `templates
review --approve`, and land the CLI-prose, machine-spec, and (inherited) registry
edits — all bodies `todo!()` pending the 1033 `src/review.rs` surface.
