# Arena framing — content injection

Phase B setup. The rubric is the picker's tool in Phase D; **candidates receive
the task and the grounding, not this rubric**.

## Artifact each candidate produces

One design package for content injection into existing files, shaped per the
architect `rationale-template.md`: caller's usage first (README-style + two or
three real call sites — a `template.yml` author view and a crate/CLI view),
then data/type sketch, function signatures, module/seam map, error/results
contract, and prose rationale with alternatives and tradeoffs. Sketches only —
`not implemented` bodies, no production code, no schema edits committed.

## Gradeable criteria (rubric)

1. **Structural idempotency.** A second apply against an unchanged prior result
   is byte-identical (a no-op), guaranteed by construction — a managed region or
   a structured set — not by matching the injected text against possibly-drifted
   surroundings. Falsifiable: the twice-apply fixture changes the file once.
2. **Explicit, legible ownership.** The design states precisely which bytes of a
   target file Toha manages versus the user, and that ownership is expressible as
   a **mutation contract** that 1066 (git updates) and 1031 can consume. A reader
   can point at a span and name its owner.
3. **Purity & replay preserved.** Injection output is a pure function of
   `(Template, Completed, target state)`; the engine reads no target bytes;
   plan-before-write holds; staging replay reproduces identical results. Read of
   the existing target happens only at plan/apply.
4. **Coherent conflict / failure / recovery.** Defines refusal cases (missing
   anchor, ambiguous anchor, drifted managed region, malformed structured doc),
   their reporting and exit codes, per-file write atomicity/recovery, and how
   `--force`, trust, and `--dry-run` output extend — **without** weakening the
   no-silent-overwrite guarantee for user-owned files or the `.git`/symlink/
   escape guards.
5. **Interface depth / small surface.** The `template.yml` author surface and the
   domain types hide marker/anchor/serialize complexity behind a small interface;
   the `TargetPath` safety newtype is reused; policy is not scattered across
   modules; the addition composes with whole-file writes in one `Plan`.
6. **Scope honesty on formats.** States which target formats each mechanism
   serves (arbitrary text vs structured), whether it is one mechanism or several,
   and justifies any strategy seam by real variation (one adapter = a
   hypothetical seam to reject). Treats jaq/structured-merge as bounded and
   optional, not load-bearing.

## Runners (Phase B) and champion directions

Default architect slots are four `inherit-parent`. To exhaust the design space
rather than converge, each runner is seeded to *champion and fully develop* one
structurally distinct mechanism while still resolving every decision above.
Cross-family runners maximize divergence. Each writes to an isolated directory;
no shared writable output path.

| Runner | Model family | Champions | Output dir |
|---|---|---|---|
| candidate-1 | Claude (opus, `general-purpose`) | Managed-region markers (begin/end sentinels; Toha owns the span between) | `/tmp/arena-content-injection/candidate-1/` |
| candidate-2 | GPT (`ocx-gpt-5-6-sol`) | Anchor-relative insertion with a presence/identity guard | `/tmp/arena-content-injection/candidate-2/` |
| candidate-3 | GPT (`ocx-gpt-6-astra`) | Structured-document merge (serde/jaq path set), format-aware | `/tmp/arena-content-injection/candidate-3/` |
| candidate-4 | GPT (`ocx-gpt-5-6-terra`) | Unified typed mutation/edit object on `Plan` (mechanism as data) | `/tmp/arena-content-injection/candidate-4/` |

Cross-judge (Phase C): one readonly judge on `ocx-gpt-5-6-luna` (a family
different from the parent), scoring every candidate against every criterion and
recommending a base. Self-scoring by the parent (opus) in parallel; disagreement
reconciled explicitly in `04-synthesis.md`.

Runners launched **foreground** (`run_in_background: false`) in one message; the
parent collects every result before any judging or synthesis, and stays
nonterminal while any child is active — background children would be killed on
parent completion.
