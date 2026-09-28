---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Arena rubric — error attribution and early-answer behavior

This rubric is withheld from candidate runners. Candidates receive the common
task and grounding only. Score each criterion from 1 to 5.

## C1 — Exact, local fault attribution

The design produces `template error in <field> `<expression>`: <message>` for
`files[i].when`, `files[i].each`, explicit `files[i].path`, and ordinary
rendered target-path segments, while retaining the source-path prefix where it
helps locate the file. A template-authored default that violates its own known
constraint is a template fault rather than caller rejection. Literal and
expression defaults have actionable field identity. The representation
concentrates field/source/message knowledge and does not create pass-through
error layers.

## C2 — Configured-default provenance and recovery

The design composes with the approved 1073 selector/provenance contract and its
repaired origin carrier: exact config file, `template-defaults` mapping site,
and optional `presets` hop/value survive until later constraint validation. Kind
and missing-reference faults terminate with exact attribution; a
constraint-invalid configured default remains a recoverable batch/terminal error
that the caller can override. The pure engine does not learn config layers,
formal-name selection, or preset resolution, and no origin/value sidecars can
silently drift.

## C3 — Early-answer policy is explicit and atomic

The design presents the two real product decisions with recommendations and
shows the resulting state transitions for active, skipped, and not-yet-decidable
early questions, both alone and beside a current-batch failure. It preserves
document atomicity, warning timing, carried-error behavior, and template-fault
precedence. No tentative classification step leaks state from a rejected
document.

## C4 — Canonical target has one authority

One public interface owns absolute lexical normalization, existing-ancestor
canonicalization, and separator-free output for both existing and non-existing
targets. Staged-state identity, protocol context, planning, and the future Jinja
context consume that result without a second normalizer. The interface has
depth: it prevents invalid construction or duplicate work rather than merely
forwarding a `PathBuf`.

## C5 — Path parity, replay, and compatibility are falsifiable

The design names concrete tests through terminal, headless direct apply, staged
continue/apply, replay, and crate interfaces. Apply/continue outcomes match for
every changed interview case; direct and staged planning emit the same fault
form; canonical target identities and state keys are stable. It names
compatibility with 1073 and 1075, proposed canonical document updates, and the
behavior of existing staged records. Each claim has a test that fails if the
load-bearing guard is removed or inverted.

## C6 — Deep module and bounded change

The design hides policy behind small interfaces, keeps short call chains and
locality, uses real seams only where adapters vary, and passes the
shallow-module, information-leakage, temporal-decomposition, and pass-through
screens. It contains all six items without expanding into permissions, trust,
timeouts, pinned checks, subprocess integration, or unrelated protocol redesign.
Public types and errors expose only what callers need.
