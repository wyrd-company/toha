# In-project generators — rationale

## The shape in one sentence

A generator is the existing apply spine run again into a subpath, so the design
adds a thin verb (`toha generate`) and exactly one engine seam
(`Resolution::start_generated`) that layers a prior application's snapshot
answers over configured defaults — everything else is reused.

## Key decisions and tradeoffs

### D1 — A first-class verb, not a flag on `apply`

`generate` is a distinct command. The update axis (`apply --from`) re-renders and
three-way-merges **in place, same target**; the generate axis **creates a new
application, new subpath, answers defaulted from elsewhere**. These are opposite
contracts: one preserves and merges an existing target, the other writes a fresh
one. Putting both on `apply` would either overload `--from` (whose contract is
"replay and merge in place" — the grounding explicitly forbids this) or add a
mode flag that silently flips `apply` between merge and create. A separate verb
makes the axis legible at the call site, lets `generate` own a generate-specific
selector (`--defaults-from`) without colliding with `--from`, and keeps every
refusal able to name the command that does what the caller meant.

Tradeoff: a second verb to document and test. Accepted — the two axes are
genuinely different operations, and a shared flag would be the more expensive
lie.

### D2 — Selection matches on `source`, never `target`

The prior application lives at a *different* subpath, so its snapshot's `target`
differs from the new target. Matching on `Snapshot::source()` (formal name
without `@reference`) is the only correct key, and it is the same identity the
template-defaults design keys on. "Auto" defers to
`Project::likely_bases(&same_source)` rather than reinventing an ordering, so
generate and the update axis agree on what "likely base" means. Exactly one
snapshot ever seeds — never a union across snapshots — so cross-snapshot
precedence is "pick one deterministically," which is total and needs no
tie-break beyond the reader contract's likely-base rule.

### D3 — Precedence resolved at the seam; the single-occupant bank is preserved

The engine's `DefaultBankEntry` holds one occupant per id. Rather than fight
that, `start_generated` **resolves precedence before it builds the bank**:
per id, snapshot-if-kind-valid, else configured, else template default, else
ask. So snapshot and configured never coexist for one id — there is exactly one
winner, and the bank invariant is untouched. This is honest about the
constraint the grounding flagged ("say exactly how they coexist") — they do not;
the seam chooses.

I add one bank variant, `Snapshot { raw, from }`, not for coexistence but for
**provenance**: a prompt and a rejection can say "default from snapshot 01J9Z…"
instead of the generic "configured default." The variant carries only raw JSON
and a display string — no snapshot or gitoxide type — so the engine stays
git-agnostic and pure. This is the whole engine change.

### D4 — Stale snapshot answers are dropped, not fatal

A snapshot answer whose id the new template no longer defines, or whose kind
changed, is dropped with a warning and falls through to configured/template.
Snapshots are *optional* defaults; a cross-version drift should not brick a
generate. I kind-check at the seam (the same check `render_default` applies to a
bank default) but deliberately **do not** run full-constraint validation there:
a value that passes kind but fails a constraint behaves exactly as a configured
default does today (person edits it; script reports it rejected, exit 4). This
keeps snapshot defaults behaviorally identical to a hand-written seed and avoids
duplicating constraint/`when` logic outside the engine.

### D5 — The staged seed is a pinned pointer, not persisted answers

The agent route must resolve the same seed across `stage → continue → apply
PATH`. I pin the chosen snapshot **id** in the staged record (`seed_snapshot:
Option<SnapshotId>`). This is a pointer into the existing snapshot lifecycle, not
a copy of answers, so it does not create the second persisted-answers lifecycle
the task forbids; the answers stay in the snapshot and are re-read live on
resume, alongside re-read configured defaults. Pinning (vs re-deriving the likely
base at resume) makes resume deterministic even if project snapshots change
between steps, and makes a deleted seed fail clearly rather than silently swap.

It is a *distinct* member from the update axis's proposed `base` (which drives
replay-in-place); I do not assume or reuse that member.

### D6 — No `template.yml` marker

I reject adding any key to `template.yml`, including an opt-in "generator-
friendly" marker. Reasons: (1) the schema is closed (`additionalProperties:
false`), so any key is a spec+schema break for zero functional gain — generate
works on *any* template because a template is already "one source + one
interview" and subpaths already work as targets; (2) an advisory-only marker
that refused nothing would be noise, and a marker that *refused* unmarked
templates would remove a capability (forbidden without explicit approval); (3)
per-subpath variation already comes from `toha_target_name` and answers, so
nothing about being a generator needs declaring. The one thing a marker could buy
— listing "which templates are generators" — is not worth breaking a closed
schema before there is a caller asking for it.

### D7 — No `--reanswer`

The update axis needs `--reanswer` because `--from` *auto-replays* recorded
answers and you need a way to re-open them. A generator never replays: it runs a
normal interview where the snapshot only supplies *defaults*. The person is
always asked (defaults pre-filled); the script route auto-takes defaults for
unanswered questions and `--answers` overrides them; the agent route batches
them. There is no replay to suppress, so `--reanswer` has no meaning here.
`--no-defaults` covers the "ignore the prior answers" intent.

## Alternatives considered and rejected

- **A flag on `apply` (`apply TEMPLATE PATH --generate`, or overloading
  `--from`).** Rejected: conflates the merge-in-place contract with the
  create-new contract; overloading `--from` is explicitly forbidden; a mode flag
  hides the axis at the call site. (See D1.)

- **A purely conventional pattern (no verb): tell people to run `apply` into
  subpaths and pass answers by hand.** Rejected: it gives no path to "default
  from the prior application," which is the feature. The snapshot-as-default seam
  has to live *somewhere*; a verb is the smallest honest home for it.

- **Declaring generators in `template.yml` (a `kind: generator` or a sub-template
  list).** Rejected: closed schema, no functional need, and it bakes a second
  entrypoint concept into template load that the "one source, one interview"
  assumption does not support. (See D6.)

- **Extending `DefaultBankEntry` to an ordered fallback list per id
  `[snapshot, configured, template]` and letting the engine try each in order.**
  Rejected: it pushes precedence *into* the pure engine and changes the bank
  invariant for every caller, when the only case that needs a fallback (a stale
  snapshot value) is cleanly handled by dropping it at the seam. The seam already
  has the template in hand to kind-check. Resolving precedence at the boundary
  keeps the engine's single-occupant contract and one bank-lookup path. (See D3,
  D4.)

- **Unioning answers across all same-source snapshots (most-recent-wins per
  id).** Rejected: it invents answer identity across applications by shared id —
  exactly the inference the grounding lists under *Avoid* — and makes the seed
  non-obvious. One selected snapshot is legible and total. (See D2.)

- **Re-deriving the staged seed at resume instead of pinning it.** Rejected:
  non-deterministic across `stage → continue` if snapshots change, and it hides a
  deleted seed. Pinning a pointer is deterministic and fails loudly. (See D5.)

- **A new reserved context name for the subpath (e.g. `toha_subpath`).**
  Rejected as out of scope: `toha_target_name` already gives the per-application
  distinction the required fixture needs; a repo-root-relative name is a separate
  ask with no caller yet.

## Where this pushes back on the existing shape

Very little, by design. The only engine edit is two additive private variants and
one new `start_generated` entry beside the existing `start_with_context`; the
pure walk below the bank is byte-for-byte unchanged, so the mandated engine
parity across routes is preserved. The apply spine, the conflict/trust/plan
machinery, `canonical_target`, `toha_target_name`, and `configured_defaults` are
reused whole. The one concession is the staged-record pointer — the minimum state
needed to make the async route deterministic without persisting answers.

## Interface-depth argument

The public surface a caller touches is four names: the `generate` verb (with its
selector flags), `toha::generate::select_seed` → `SeedChoice`, and
`Resolution::start_generated`. Behind those, hidden:

- **Snapshot reading and selection** — opening the project, filtering by source,
  the likely-base ordering, prefix/ambiguity/wrong-source resolution, and the
  person-vs-script interaction budget — all behind `select_seed`; no gitoxide,
  `Snapshot`, or `Project` type appears on a generate signature except the
  read-only reader contract it consumes.
- **The submissions→defaults transform** — flattening `Vec<IndexMap<Id, Value>>`
  batches (including looped arrays) into the flat seed shape, kind-checking each
  against the live template, dropping stale ids, and layering over configured
  defaults into the single-occupant bank — all behind `start_generated`, which
  hands the engine one `Option<SnapshotDefaults>` and returns an `Interview`.

A crate caller, a script, and the terminal all cross the same two seams and get
identical results. The engine never learns what a snapshot is: `SnapshotDefaults`
is raw JSON plus a provenance string parsed at the boundary, so the "parse
external input once at its boundary into domain types" principle holds and the
call chain from CLI to engine stays under three files (`generate.rs` →
`interview.rs` → the shared spine).

## Single biggest risk

The mandated twice-in-one-project fixture is genuinely end-to-end only when the
update-projects task's snapshot **capture** is wired into the shared apply spine
at runtime: the first `generate` must *produce* the snapshot the second reads.
This design consumes only the reader contract and must not assume capture exists,
so the fixture's first leg depends on a sibling task's runtime. Mitigation: the
fixture supplies the first application's snapshot through the same reader
contract (a fixture ref), so the seam and selection are provable without the
capture implementation; the fully end-to-end run lands when both tasks meet.
</content>
