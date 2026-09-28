---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
  explains:
    - design
---

# Arena synthesis — error attribution and early answers

## Arena integrity

The native runner pool was unavailable at the candidate gate. Two independent
`gpt-5.6-sol/high` CLI runners started in isolated `/tmp` directories. Candidate
2 opened the hidden rubric while exploring the design worktree, so it is
retained as contaminated evidence and excluded from the two-candidate
requirement. A clean replacement ran against a detached epic checkout with the
common task and grounding embedded and no rubric file. Candidate 1 and
replacement Candidate 3 are blind and structurally distinct.

The readonly cross-judge used the same available model family because the
configured Claude and native cross-family slots were unavailable. It ran only
after both valid candidates were terminal. The architect scored the candidates
independently before reading that verdict.

Artifact identities before import:

| Artifact                            | SHA-256                                                            |
| ----------------------------------- | ------------------------------------------------------------------ |
| Candidate 1 design                  | `81d1a0aae106a655c90bc3b9188003e6575bc51836a431911083c41693020dec` |
| Candidate 1 rationale               | `b67aa0ea7affb9a329fdf838cf6de928a62bb459d107f9c4c49c288f5578c8dc` |
| Candidate 2 design, contaminated    | `2eee11362bbeab914c65df86eed97b54d0c0fed889f42df7e394f641314dd3ea` |
| Candidate 2 rationale, contaminated | `f82e5b94f5518b3f65809e9bf97eb7de5ff5efcc34ee375f343608cb8a046773` |
| Candidate 3 design                  | `1cfa01b2a0bf95aaabee6298a3e9719c380d04b1b9ac2b7c38b60efb683d8423` |
| Candidate 3 rationale               | `d6b81580b278ad8a5ac78e1bdcbdb32518876ba1ab82eb92ebb68a45c353b796` |
| Cross-judge verdict                 | `913074ba231285ac8f4e6a24feede29ea800f9bf5721f001753987c78243b085` |

## Score disagreement

The judge selected Candidate 1 at 26/30 and scored Candidate 3 at 23/30. The
architect selected Candidate 3 at 25/30 and scored Candidate 1 at 23/30.

The disagreement is useful. The judge weighted enforceable target identity and
one shared fault value more heavily. The architect weighted compatibility with
the final configured-default contract and bounded public changes more heavily.
Both found the same blocking defect in Candidate 1: it silently replaces the
approved flat `Seed.defaults` contract with a configured-only carrier. Both
found the same target weakness in Candidate 3: a raw `PathBuf` lets crate
callers bypass normalization.

The final shape uses Candidate 3's bounded ingress as the behavioral base and
grafts Candidate 1's enforceable target and prompt preparation. This yields the
judge's strong target invariant without Candidate 1's seed replacement.

## Base and graft ledger

### Base — Candidate 3

Kept:

- a private, origin-bearing configured-default bank built once from the final
  prerequisite's winning `ConfigEntry` values;
- the existing flat `Seed` route for ordinary crate-supplied defaults;
- `SubmissionTxn` ownership of the original pending state;
- rejected current answers are unavailable to skip proofs;
- one effect-free probe followed by either rejection or one real commit;
- module-local planning/interview errors using one crate-private formatting
  value.

Revised:

- the terminal-only `check_default` and `TakeDefault` path is removed;
- configured-default rejection is prepared before prompting, the bad value is
  removed from `Prompt.default`, and the existing batch-error path handles
  terminal and headless replacement;
- `Resolution` retains warnings and gains one consuming configured start/replay
  capability. It does not replace `Seed.defaults`.

### Grafts from Candidate 1

- `CanonicalTarget` becomes the opaque result of the single normalization
  operation, and identity-sensitive storage, protocol, planning, apply, and
  future Jinja-context interfaces require it.
- An empty non-existing suffix returns the canonical ancestor directly.
- The old separator-keyed staged record has one bounded compatibility lookup and
  is moved to the canonical key only after a successful save.
- Prompt preparation validates active defaults where full constraints and
  authorship are both present. A bad template default is terminal. A bad
  configured default becomes a carried recoverable error and is not offered.
- The exact planner fields are `files[i].when`, `files[i].each`,
  `files[i].path`, and `path` for an ordinary source-path segment.

Rejected:

- replacing public `Seed.defaults` with the configured-default carrier;
- exposing a public shared fault hierarchy when one crate-private formatter can
  keep the text contract consistent;
- letting a probe consume invalid current answers merely because they remain
  held.

### Independently checked grafts from contaminated Candidate 2

- staged `apply` is explicit in the parity matrix;
- mapping and preset winners from different files prove two-origin attribution;
- a skipped template-authored default is checked against every constraint
  already ready at that point, without forcing unavailable dynamic dependencies;
- compile-fail checks cover every public consumer of `CanonicalTarget`.

Rejected:

- a separately implemented guard-only reachability walker;
- a configured-only seed and `without_configured_defaults` escape hatch;
- fabricated backticked expression text for typed literal defaults.

## Red-flag and codebase-design screen

- **Shallow modules:** `CanonicalTarget` hides filesystem and legacy-key rules;
  configured resolution hides origin coupling; `SubmissionTxn` hides one atomic
  document transition. Each replaces caller coordination.
- **Information leakage:** config layers, selectors, and preset lookup stop at
  configured resolution. Interview sees only a displayable origin. Planning
  paths do not enter interview errors. The target's inner `PathBuf` is
  read-only.
- **Temporal decomposition:** default selection, full ready constraints, and
  source classification meet in prompt preparation. Early parsing, probing, and
  commit are one transaction owned by `Pending::answer`.
- **Pass-through methods:** no terminal-only default method remains. Consuming
  configured start/replay operations add provenance policy. `as_path` is the
  narrow projection needed by filesystem and serialization adapters.
- **Locality and seams:** only `interview` owns answer policy; only `staging`
  creates canonical targets; only `plan` knows file-rule field taxonomy. The
  shared diagnostic formatter has multiple real producers and one stable text
  contract.
- **Testability:** callers use the same start/replay, `Pending::answer`, target
  type, and plan interfaces exercised by terminal, direct, staged, and crate
  tests.

No red flag remains that requires reframing. The configured-default seam is an
explicit amendment to a final prerequisite and is included in the checkpoint
approval rather than treated as an assumed implementation detail.

## Approved product decisions

1. **Undecidable early skip — reject now.** The value already fails a literal,
   answer-independent rule. A rejected document is not persisted; the caller can
   resubmit when the same document proves the question skipped.
2. **Proven skipped beside another failure — omit it.** The error describes an
   unused value. The other error rejects the document; no skipped warning is
   emitted until a corrected document commits.
3. **Skipped template default — check ready constraints.** The template owns the
   value and the ready rule. Do not block a skipped branch to evaluate a dynamic
   constraint whose references are unavailable.
4. **Canonical target — use the opaque type.** This makes bypass impossible for
   crate callers. It changes public planning, apply, store, record, and context
   signatures from raw paths. The checkpoint approval explicitly includes this
   source API restriction.

Typed literal default faults name `<id>.default` without backticks. Expression
and string-template defaults retain exact authored source in backticks.
