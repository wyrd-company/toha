# Rationale — template-declared generators (candidate 3)

## The honest verdict on the schema change (read this first)

My seed direction is **template-declared generators**, keyed — the seed hoped —
by `(source, generator-name)`. I explored that shape fully and must report two
findings that shrink its advantage, because the arena needs the real tradeoff,
not a sales pitch.

**Finding 1 — the snapshot cannot carry a generator name, so the seed's keying
premise fails.** The snapshot producer contract (task 1029, approved, frozen for
me) records `source`, `target`, `submissions` and nothing that identifies which
generator produced an application. Its schema denies unknown members. I may not
edit it. Therefore "snapshot defaults keyed by `(source, generator-name)`" is
**not realizable**. For defaulting, a generator's identity collapses to
`(source, target)` — exactly what a design with **no** template declaration would
use. The declared concept buys **zero** snapshot-keying leverage.

**Finding 2 — the core feature needs no declaration at all.** The grounding is
explicit: "Subpaths already work as apply targets with no new path machinery."
Repeated application into subpaths, per-application identity (the target path),
and snapshot-seeded defaults are all deliverable against the existing
`template.yml` with a verb and a seam. The declaration adds only **authoring
ergonomics**: a scoped interview and a bundled source subdirectory, so "add a
component" is one short command against one template folder instead of a second,
separately-addressed template.

So: **does the authoring power justify opening a closed schema and the
"one source, one interview" loader assumption?** My verdict is **a qualified no
for 0.2.0**. The ergonomics are real and pleasant, but they are (a) severable —
they can arrive later without breaking anything, because the absent `generators`
key is inert — and (b) not load-bearing for any of the five things the task
actually demands. A candidate built on a flag (`apply --generator NAME`) or a
convention (a component is its own template applied into a subpath) delivers the
same five demands for a smaller, reversible change.

Per the task, I nonetheless present the **strongest** declared-concept design
(`design.md`) so the arena can weigh it against the derived shapes. Where it
wins is author intent made explicit and discoverable (`toha generate web-app` can
list its generators); where it loses is a closed-schema edit and a loader that
must learn a base view plus N named views, paid for benefits that do not touch
the snapshot seam at all.

The one thing the declared design does **not** compromise, regardless of the
verdict, is the seam: the `(source, target)` selection, the
`submissions → IndexMap<Id, RawAnswer>` flatten, and the `DefaultBankEntry`
precedence extension are identical whether a generator is declared, flagged, or
conventional. That seam is the durable part; the declaration is the skin.

## Alternatives considered and rejected

### A. `apply --generator NAME` (a flag, not a verb or a concept)
Rejected as the primary shape only because the seed asked for the declared
concept; it is my recommended fallback. A flag reuses `apply` entirely, needs no
new verb in the CLI spec, and still selects a scoped interview/source. It is
strictly less code than the `generate` verb. I keep the `generate` verb in the
design because it names the **generate axis** distinctly from the **update axis**
(`apply --from`), which the grounding lists as a risk to guard; a reader who
weights that risk highly prefers the verb, one who weights surface-area highly
prefers the flag. Both drive the same spine and seam.

### B. A separate template per component (pure convention, no schema change)
A `gh:owner/react-component` template, applied with plain `apply` into a subpath,
defaulting from a snapshot of that template elsewhere in the project. This needs
**no** schema change and already works. Rejected as the *primary* because it
scatters an author's related generators across separately-addressed templates and
gives the caller no discoverability ("what can I generate here?"). But it is the
honest low-ceremony baseline the declared concept must beat, and it beats it only
on ergonomics — which is exactly Finding 2.

### C. Reusing `--from` for generation
Rejected outright and called out in `design.md`. `--from` is update-in-place
(same target, three-way merge), owned by task 1029. Overloading it for
create-new-in-a-subpath would conflate two axes whose contracts differ (merge vs.
fresh apply) and whose targets differ (same vs. new). `generate` has no `--from`.

### D. Recording a generator name in the snapshot
This would restore the seed's `(source, generator-name)` keying and let selection
prefer "snapshots made by *this* generator." Rejected because it requires editing
the frozen snapshot schema and capture path, which the task forbids. It is the
single change that would most strengthen the declared concept — I flag it as the
first thing to revisit if generators and snapshots are ever designed together.

### E. Extending `DefaultBankEntry` to an unbounded ordered `Vec`
The grounding floats "an ordered fallback rather than a single occupant." I
rejected the unbounded `Vec` for a **two-field struct** (`seed`, `configured`)
because the precedence is fixed and shallow — snapshot over configured over the
template's own default — and a `Vec` would admit orderings the domain never
produces. The struct encodes the exact invariant (at most one snapshot seed and
at most one configured default per id, seed wins) in the type, so no code can
express an illegal precedence. This also preserves configured-default attribution
(the `ResolvedDefault` origin), which a flatten-outside-the-engine approach would
discard for generator runs.

### F. Flattening snapshot-over-configured outside the engine into a plain `Seed`
The lowest-blast-radius seam: resolve precedence in `generator.rs`, hand the
engine one `IndexMap<Id, RawAnswer>` of `Seed` entries, change nothing in
`interview.rs`. Rejected because surviving configured defaults would lose their
config-site attribution (the template-defaults design's whole point), degrading
error messages in generator runs. The two-field `DefaultBankEntry` costs a small,
contained change in one file and keeps attribution intact. This is the one place
I chose a slightly larger change for a correctness/diagnostics win.

### G. Content-based or sibling-subpath seed selection
"Pick the snapshot whose subpath is nearest the new one," or "whose files most
resemble the target." Rejected: both are non-total heuristics that make the
script and agent routes non-deterministic or route-dependent. "Latest snapshot of
this source" is total (ULID creation order), needs no interaction, and is
identical across routes — the determinism the grounding demands.

### H. Answer-interpolated target patterns (`target: "src/components/{{ name }}"`)
Attractive for ergonomics, rejected on an architectural fact: the apply spine
computes `canonical_target` from the PATH argument **before** the interview runs,
and `Plan` is single-target by contract. A target that depends on answers cannot
be known when the target is fixed. So `suggested_target` is a static person-route
prompt default, and SUBPATH is an explicit argument on the deterministic routes.
This is a real constraint the grounding encodes, not a shortcut.

## Key decisions and tradeoffs

1. **A generator resolves to an effective `Template`.** The entire generator
   concept collapses to "produce a `Template` with the generator's overlay + pick
   a seed snapshot." Everything downstream — engine, `Plan::build`, `apply`,
   staging — consumes a `&Template` and never learns generators exist. This is
   the interface-depth core (below).

2. **Source identity, never the formal name.** Selection and `--defaults-from`
   compare `Snapshot::source()` (formal name without `@reference`) to the
   template's source identity, mirroring the update design's own sole-kill
   ("compare the formal name with its reference instead of the source identity").
   A `web-app@v1` snapshot legitimately seeds a `web-app@v2` generate.

3. **Seed-by-id is safe here, and is not the forbidden inference.** The fixed
   policy forbids inferring semantic identity from matching ids *across
   templates*. My seam maps a snapshot's submissions to defaults by id only after
   filtering to the same `source` — the same template identity, the same question
   language. That is what the update replay already does; the forbidden case
   (seeding from a different source) is refused.

4. **Staging gains two optional members, not a lifecycle.** `generator` and
   `defaults_from` on `StagedRecord` are Toha's transient state so the agent route
   can resume deterministically. No answers are persisted into the project; the
   record re-reads the snapshot from git on resume rather than freezing values —
   consistent with the template-defaults "nothing is frozen into the record"
   rule.

5. **`generate` is a distinct verb with no `--from`.** It keeps the generate axis
   visibly separate from the update axis and refuses the overload the grounding
   warns against.

## Interface-depth argument

The public surface a caller touches is three things: the `generate` verb, the
optional `generators` map in `template.yml`, and one new flag `--defaults-from`.
Behind that surface I hide:

- **gitoxide, refs, and snapshot wire format** — reached only through the
  consumed `toha::snapshot` reader; a `Snapshot` is reduced to
  `IndexMap<Id, RawAnswer>` inside `generator.rs` before anything interview-facing
  sees it. No gitoxide, wire, or snapshot-internal type appears on any generator
  signature (`candidates`, `latest`, `require`, `seed_from`, `start_with_seed`).
- **The whole generator concept** — collapsed into an effective `Template`, so the
  engine, `Plan`, and `apply` are literally unchanged and unaware. The concept is
  one module (`generator.rs`) plus one optional loader field plus one engine
  constructor.
- **The snapshot-vs-configured precedence** — resolved at one seam
  (`start_with_seed`) and encoded in one private type (`DefaultBankEntry`), so the
  pure engine stays identity-, preset-, and snapshot-unaware, exactly as the
  template-defaults design left it.

Tracing any generator application is ≤ 3 files: `generator.rs` (resolve view,
pick seed, flatten), `interview.rs` (one constructor + `render_default`'s bank
lookup), and the unchanged spine. The snapshot reader is a fourth file only as a
consumed boundary, never a control-flow detour.

Where the design pushes back on the existing shape: it opens the closed
`template.yml` schema (the 12th key) and changes the private `DefaultBankEntry`
from a one-occupant enum to a two-occupant struct. The first is the cost I judge
**not** clearly justified (see the verdict); the second is a clean, contained
correctness win the grounding already anticipated, and it is required by **any**
candidate that must let a snapshot default and a configured default coexist for
one id.
