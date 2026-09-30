# Grounding: trust management for installed templates

Design package for task 1071 (paired implementation 1054), Toha 0.2.0. This is a
design artifact. It describes the current system and the contract edits the
implementation will make. It does not change canonical specifications, schemas,
or runtime code.

Task 1071 owns the **trust-management command syntax** for already-installed
templates. It consumes the approved trust **policy** from design 1069 (paired
implementation 1033) without redefining it: 1069 fixes what approval binds to and
how it lapses; 1071 designs the commands a caller uses to grant and revoke that
approval, and the refusal that points at them.

## Prerequisite: the approved 1069 contract (consumed, not reopened)

Exact approved revision: `docs/design/hook-review-trust/05-design.md` at commit
`132b2f449633ee0db89b29b80c63bdc2eca92e8a`. Bob approved all
four of its decisions. The parts 1071 builds on:

- **Approval is a content fingerprint, not a name or commit.** The registry entry
  carries `approval: Option<ReviewDigest>` in place of `trusted: bool`.
  `ReviewDigest` is `"sha256:<64 hex>"` over the template's *executable surface* —
  the canonical encoding of every parsed hook node plus the bytes of every file
  the surface executes (declared `script:` targets and, per 1069 Decision 2,
  literal in-tree `run`/`args` paths).
- **The library operations 1071 calls.** `src/review.rs` (new in 1033) exposes:
  - `HookSurface::of(&Template) -> Result<HookSurface, ReviewError>` — pure, reads
    the executed-file bytes; `ReviewError` on a missing/unreadable/escaping file.
  - `HookSurface::digest(&self) -> ReviewDigest` — deterministic, answer-independent.
  - `HookSurface::diff(&self, prior: &HookSurface) -> HookSurfaceDiff` — for
    presenting *changed since approval*.
  - `evaluate_trust(approved: Option<&ReviewDigest>, live: &ReviewDigest) -> Trust`
    — the single chokepoint: `Trusted` iff `approved == Some(live)`, else
    `NeedsReview`.
- **The plug-in point 1071 owns.** 1069 states 1071 "owns the trust-management
  command syntax (review / approve / untrust / list-trust); this design only
  exposes the library operation and names the plug-in point (`entry.approval`
  write + `HookSurface`/`evaluate_trust`)." So 1071 writes `entry.approval` on a
  user-registry entry and reads the installed content through `HookSurface`.
- **Settled, do not reopen (1071 Question 2).** "Does trusting record the current
  commit, so a later `templates update` needs trust again?" 1069 already answers:
  approval binds the *executable-surface digest*, independent of commit. An update
  that changes the surface lapses approval; a doc-only update keeps it. 1071 records
  the digest of the currently installed surface; it must not bind the commit.

## Traced model: the command surface today

### The `templates` command tree (`src/cli/templates.rs`)

`TemplatesCommand` (clap `Subcommand`, `templates.rs:18-68`) has five arms:
`Add`, `List`, `Update`, `Remove`, `Alias`. `run` (`:669-703`) loads a `Context`
and dispatches to a per-arm function. Every mutating arm writes **one** file: the
user registry (`Context::write`, `:127-130`, always `user_data/templates.yml`).
The user layer is the only writable layer (`alias`, `:643` comment).

- **`Alias { template, alias, remove }`** (`:56-67`, fn `:596-653`) is the
  existing model for a dual-mode subcommand. `templates alias <template> <alias>`
  adds; `templates alias --remove <ALIAS>` (short `-r`) removes. clap enforces the
  shape: `remove` `conflicts_with_all=["template","alias"]`; `template` is
  `required_unless_present="remove"` and `requires="alias"`. The spec expresses
  this as two groups: `add-alias` (`all-or-none`) and `operation`
  (`exactly-one`) (`command-line-interface.spec.yml`, alias subcommand). Runtime
  re-checks the exclusivity and errors `"-r cannot be combined with a template or
  alias"` (`:602-607`).
- **`Add { address, alias, trust }`** (`:21-30`, fn `:285-439`). `--trust` sets
  `entry.trusted = trust` (`:388`) blind, on freshly cloned/parsed content. `add`
  requires a **git address or folder**; a bare installed name is refused with
  `guidance::add_needs_address` (`:312-317`). So `add --trust` cannot re-approve an
  installed template without re-cloning.
- **`Remove`** (`:52-55`, fn `:567-587`), **`Update`** (`:46-50`, fn `:505-566`),
  **`List`** (`:31-45`, fn `:440-504`).

### How a name resolves to a writable entry

`Context::resolve_user(name)` (`:131-139`) resolves `name` through the merged
registry to a formal name, then requires that formal name to exist in the **user**
`RegistryFile`; otherwise it errors with `guidance::not_in_user_registry(name)`
(`guidance.rs:746-755`). `remove`, `update`, and `alias` all go through it. A
trust command over an installed template takes the same path: only a user-registry
entry can carry an approval, because only the user layer is writable and the local
layer forbids trust.

`ResolveError::Ambiguous` maps to `CommandError::Ambiguous` (`:74-80`), which the
binary renders through `guidance::ambiguous` with a `retry` closure
(`templates.rs:656-668`, `guidance.rs:758-777`) so each match is offered as the
same command with its formal name (exit code 5).

### The trust datum and where it is read/written

- Registry entry (`src/registry.rs`): `Entry { name, source, ref, commit, path,
  aliases, trusted: bool }`; `FieldPresence` tracks whether `trusted` was written,
  for sparse round-trip and layer-merge precedence. `RegistryFile::add` does
  `entry.trusted |= old.trusted` on re-add. (Per 1069 these become `approval:
  Option<ReviewDigest>` and `entry.approval = entry.approval.or(old.approval)`.)
- `templates list` (`:440-504`) prints a `TRUSTED` column from the resolved
  `Listed.trusted`; `--json` emits `"trusted"`. (Per 1069 this becomes an
  *effective* value computed by `evaluate_trust` over the installed surface.)
- The apply gate is decided in `main.rs` (`registry_trusted || --trust`) and
  enforced in `apply.rs` (`NeedsTrust`, exit 3). 1071 does **not** touch the gate;
  it only writes the `entry.approval` the gate reads. (Grounded fully in 1069's
  `01-grounding.md`.)

### The refusal today (1071 Question 3)

When `apply` (or `apply --dry-run`) plans hooks of an untrusted template, the
binary calls `guidance::needs_trust` / `dry_run_needs_trust`
(`guidance.rs:634-651`), which delegate to `trust_lines` (`:653-672`). For a
template **resolved by name** from a writable/system registry (`installed:
Some(formal)`), `trust_lines` emits a second line:

> `to trust {formal} for every run: toha templates add --trust {formal}`

This names `templates add --trust` (`:661-670`). For an *installed* template that
is exactly the wrong operation: `add` re-parses/re-clones and refuses a bare name.
The refusal should instead name the new operation this task designs, which trusts
the *installed* content with no fetch. The CLI spec's Trust paragraph states the
same "names `templates add --trust` with the formal name" behavior
(`command-line-interface.yml`, Trust), and must change with it.

## What 1071 must decide (from the task)

1. **Command shape.** `templates trust <name>` + `templates trust --remove <name>`;
   options on an existing command; or another justified alternative.
2. **What approval binds and what updates require.** Bind the installed reviewed
   content per 1069 (state it; do not redefine it).
3. **Refusal guidance names the new operation** instead of re-adding.
4. **Registry persistence and errors.** Trust/untrust must **not** fetch, reclone,
   or change the installed commit. Define what is written, and the errors (name not
   in user registry, ambiguous name, unreadable installed surface, no-op).

## Constraints carried into the design (Preserve / Change / Avoid / Risk)

**Preserve**
- Interview-engine purity and every adapter (interactive / headless / staged /
  direct / crate). Trust management is a registry-mutation command like `alias`;
  it never touches the interview engine.
- Scriptability: no new interactive gate a script cannot pass. Existing grant flows
  (`add --trust`, `apply --trust`) keep working.
- The user layer is the only writable layer; the local layer forbids trust;
  system is admin-provisioned. Only a user-registry entry can hold an approval.
- The 1069 policy: approval binds the executable-surface digest; `evaluate_trust`
  is the one chokepoint; the apply gate and exit-3 contract are unchanged.
- Ambiguity (exit 5) and not-in-user-registry refusal patterns, which every
  registry-mutating `templates` command already shares.

**Change**
- Add a command surface to grant approval to an *installed* template (write
  `entry.approval` from the installed surface's digest) and to revoke it.
- Redirect the untrusted-hooks refusal to name that new operation.
- Update the CLI, registry, and registry-schema specs accordingly (owned by the
  paired implementation 1054, described here).

**Avoid**
- Any fetch/reclone/commit change on trust or untrust (explicit task constraint).
- A new interactive-only gate (would break headless scriptability — a
  supported-capability regression needing approval).
- Reintroducing name-bound or blind trust that 1069 removed; trust must record the
  digest of content actually present.
- Information leakage: the trust rule stays behind `evaluate_trust`; commands do
  not re-implement the equality.

**Risk**
- **TOCTOU between review and approve.** If review and approve are separate
  commands, the installed content can change between them; approval could record a
  surface the caller never saw. Whether to bind a caller-confirmed digest is a
  design question.
- **Trusting content the caller has not seen.** `add --trust` is blind today.
  Should the new trust command print the surface it approves, or stay silent for
  scripts? Must not add an interactive gate.
- **Effective-trust listing cost.** A trust-aware `list`/`list-trust` recomputes a
  digest per entry (a parse plus a few hashes); 1069 already flags this.

## Coordination and dependency graph

- **1069 / 1033 (hook review + trust persistence).** Prerequisite *design*,
  approved at commit `132b2f4`. 1071 consumes its policy and library surface; the
  implementation 1033 need not exist for this design. If that approved interface
  changes, 1071 returns to design review.
- **1054 (paired implementation of 1071).** Gated; receives the approved artifact
  revisions, exact scope, interfaces, and falsifiable validation from this design.
- **1033 ↔ 1054 sequencing.** 1054 depends on `src/review.rs` / `entry.approval`
  from 1033. This is an *implementation* ordering, recorded for the implementation
  contract; it does not block design prep. The parallel effort iron-cape (task
  1033) integrates the approved package; review/command interfaces are coordinated
  through the board.
- **1074 (bundled demo), 1075 (Jinja context).** 1069 keys approval on content,
  orthogonal to selected-template identity; 1071 adds no new identity coupling.
