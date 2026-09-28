---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Readonly cross-judge

## Judgment

Candidate 1 is the recommended base. Candidate 3 is a valid blind alternative
and supplies important compatibility improvements. Candidate 2 scored well, but
is contaminated and ineligible.

Scores use 1–5, where 5 fully satisfies the criterion.

| Criterion                                 |                                                                                                                                                                                                                                      Candidate 1 |                                                                                                                                                                       Candidate 3 |                                                                                                                             Candidate 2 — contaminated/ineligible |
| ----------------------------------------- | -----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------: | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------: |
| C1 — Exact, local fault attribution       |                                  **4** — One `TemplateFault` owns field/source/message and covers all four planning sites. Literal and expression defaults are distinguished. It explicitly does not constraint-check skipped template defaults. |          **4** — Exact local planning faults and authored defaults, but uses `target path` rather than `path` and also limits constraint attribution to active template defaults. |                     **5** — Covers all planning sites and validates template defaults on active and skipped paths. Literal values get canonical JSON source text. |
| C2 — Configured provenance and recovery   | **4** — One value-plus-origin carrier preserves both config sites and supports replacement. Changing public `Seed.defaults` to configured defaults loses the existing flat-default caller path; warning handling is absent from caller examples. |         **5** — Best origin ownership: opaque atomic `Resolution`, retained flat `Seed` for application defaults, consuming start/replay operations, and no value/origin sidecar. | **4** — Exact provenance and recovery, but its configured-only `Seed` also narrows the current crate interface and examples do not surface prerequisite warnings. |
| C3 — Explicit, atomic early-answer policy |                                                                                              **5** — Both decisions, all three dispositions, current-failure combinations, warning timing, fault precedence, and discarded effects are explicit. |                                             **5** — `SubmissionTxn` makes commit ownership particularly clear; rejected current values are explicitly unavailable to skip proofs. |                                                                                                                   **5** — Complete matrix and atomicity contract. |
| C4 — One canonical-target authority       |                                                                                                                                                   **5** — Opaque, unforgeable `CanonicalTarget`; every identity-sensitive interface requires it. |                     **2** — Returns `PathBuf` and downstream interfaces accept `&Path`; command-call-site discipline does not prevent crate callers from bypassing normalization. |                                                                    **5** — Opaque target with no unchecked constructor or `From<PathBuf>`, enforced at consumers. |
| C5 — Falsifiable parity and compatibility |                                    **4** — Strong direct, continue, replay, provenance, migration, and compile-fail coverage. The matrix does not explicitly exercise the staged `apply` route, and it misses flat-`Seed`/warning compatibility. |               **4** — Strong behavioral and replay matrix. The target “single authority” test is a repository audit, not proof that public callers cannot pass an arbitrary path. |                                     **5** — Explicit terminal, direct apply, continue, staged apply, replay, crate, origin, migration, and compile-fail coverage. |
| C6 — Deep module and bounded change       |                                                                                     **4** — Deep target and fault modules, short call paths, good locality. The prerequisite-owned resolved-default type leaks into the public `Seed` interface. | **3** — `Resolution` and `SubmissionTxn` are deep, but `check_default` splits one default operation across engine and terminal, and target correctness remains caller discipline. |         **3** — Deep target and fault values, but the reachability probe risks becoming a second walker and its `Seed` removes ordinary caller-supplied defaults. |
| **Total**                                 |                                                                                                                                                                                                                                        **26/30** |                                                                                                                                                                         **23/30** |                                                                                                                                            **27/30 — ineligible** |

Relevant evidence:
[Candidate 1](/tmp/arena-error-attribution/candidate-1/design.md:188),
[Candidate 3](/tmp/arena-error-attribution/candidate-3/design.md:108),
[Candidate 2](/tmp/arena-error-attribution/candidate-2/design.md:271).

## Red-flag findings

### Candidate 1

- **Module depth:** Pass. `CanonicalTarget` hides normalization and makes
  invalid construction unavailable. `TemplateFault` removes repeated
  field/source/message formatting.
- **Information leakage:** Red flag. `Seed.defaults: ResolvedDefaults` makes the
  public interview seed depend on the configured-default representation and
  removes the present application-default shape.
- **Temporal decomposition:** Pass. Default selection, rendered constraints, and
  authorship meet in prompt preparation.
- **Pass-through surfaces:** No material pass-through. Replay adds adaptation
  and policy.
- **Locality:** Strong. Default classification stays in `interview`; source-path
  context stays in `plan`; normalization stays in `staging`.
- **Real seams:** `TemplateFault` serves interview, hook, and plan consumers.
  `CanonicalTarget` serves all identity consumers.
- **Testability:** Strong, with the staged-apply and warnings gaps noted above.

### Candidate 3

- **Module depth:** `Resolution` and `SubmissionTxn` are deep. The target
  interface is shallow because its invariant disappears into `PathBuf`.
- **Information leakage:** `PreparedDefault` state beside the public prompt and
  `check_default` expose the timing and source of default validation to the
  terminal adapter.
- **Temporal decomposition:** Red flag. Configured-default handling is split
  between prompt preparation, terminal `check_default`, and omitted-answer
  processing.
- **Pass-through surfaces:** `check_default` is a likely adapter-specific
  pass-through around the same validation used by `answer`.
- **Locality:** Good for provenance construction; weak for target correctness
  and terminal/headless parity.
- **Real seams:** The shared diagnostic formatter and consuming `Resolution`
  operations are real seams. Audited command composition is not a target seam.
- **Testability:** Behavioral coverage is strong, but no type-level test can
  reject raw target paths because the interfaces accept them.

The current terminal converts a selected default into a concrete value and calls
`Pending::check`; it has no `TakeDefault` distinction. Candidate 3 therefore
requires additional adapter coordination not present in
[the terminal flow](/tmp/arena-error-attribution/replacement-repo/src/cli/terminal.rs:203).

### Candidate 2 — contaminated/ineligible

- **Module depth:** `CanonicalTarget` and the unified fault carrier are deep.
- **Information leakage:** Its configured-only `Seed` narrows the current public
  contract. Requiring an expression string also represents literals as if they
  had authored expression source.
- **Temporal decomposition:** Red flag. The guard/reachability probe can become
  a second traversal model even though guard evaluation is shared with
  `Advance`.
- **Pass-through surfaces:** `Seed::without_configured_defaults` adds little
  capability and still does not preserve application-supplied defaults.
- **Locality:** Skipped-default validation is well placed; early reachability is
  less local than reusing the full walker.
- **Real seams:** The target and shared fault values are real. The extracted
  guard seam has two users, but does not by itself guarantee identical traversal
  semantics.
- **Testability:** Best explicit route coverage of the three.

## Structural distinctness

**Pass, narrowly.** Candidates 1 and 3 share the same behavioral
spine—origin-bearing defaults, prompt-time authorship retention, speculative
state-machine traversal, and one target-normalization call—but differ on three
load-bearing structural choices:

- public origin-bearing `Seed` versus dual ingress through flat `Seed` and
  opaque `Resolution`;
- one shared fault value versus module-local faults with a shared formatter;
- nominal `CanonicalTarget` enforcement versus retained `PathBuf` interfaces.

These are interface and ownership differences, not cosmetic variations.

## Recommended base and grafts

Use **Candidate 1** as the base.

Graft from Candidate 3:

1. Preserve the existing flat `Seed` path for application-supplied defaults.
2. Add an opaque, atomically constructed configured-default `Resolution` with
   consuming start and replay operations, so configured value and origin cannot
   be separated.
3. Preserve `Resolution::warnings()` and require every supported command path to
   surface warnings before consumption.
4. Use `SubmissionTxn` ownership of the original `Pending` and its explicit
   “rejected current answer is unavailable” rule. Keep Candidate 1’s reuse of
   the full `Advance` walker.

Reject from Candidate 3:

- Raw `PathBuf` target ownership and repository-audit enforcement.
- Terminal-only `check_default`/`TakeDefault` coordination.
- The `target path` field name; use one canonical field, recommended `path`.
- Module-local duplication of the same fault triple where Candidate 1’s shared
  value already serves multiple real consumers.

Graft from contaminated Candidate 2 only after independent validation:

1. Validate a skipped template-authored default against constraints that are
   already ready, without rendering unused presentation fields.
2. Add the explicit staged `apply` route to the parity matrix.
3. Keep its compile-fail proofs for every identity-sensitive target consumer.

Reject from Candidate 2:

- The guard-only reachability probe as a separate traversal model.
- Replacing flat caller defaults with a configured-only `Seed`.
- Inventing canonical JSON “expression source” for typed literals. A literal
  should name `<id>.default` without fabricated authored source.

## Conflicts, gaps, and required decisions

### Blocking prerequisite conflict

The current prerequisite still says:

- configured defaults flatten to `IndexMap<Id, RawAnswer>`;
- `Seed.defaults` remains unchanged;
- behavior below `Seed` remains unchanged;
- `Resolution` is destructured and never passed into `Seed`.

See
[configured-default design](/workspaces/worktrees/toha/design-template-defaults/docs/technical-designs/second-release/template-defaults/design.md:1),
especially its interface at
[line 261](/workspaces/worktrees/toha/design-template-defaults/docs/technical-designs/second-release/template-defaults/design.md:261).

That is incompatible with every candidate’s assumed later origin carrier. The
synthesizer must not invent that contract locally. The prerequisite must first
define the authoritative origin-bearing result and its start/replay integration.

### Test gaps to close

- Exercise actual staged `apply`, not only `stage` plus `continue`.
- Prove warnings are surfaced through direct, progress, continue, staged apply,
  terminal, headless, and crate flows.
- Preserve and test ordinary flat `Seed` callers.
- Test skipped template defaults separately for ready literal constraints and
  unavailable dynamic constraints.
- Test that value and provenance change together when config precedence changes.
- Test legacy target-key migration on each supported separator/root
  representation.
- Compile-fail all public target consumers against raw `PathBuf`.

### Product decisions still required

1. **Early undecidable failure:** recommend reject now.
2. **Proven-skipped failure beside a current failure:** recommend omit it; emit
   no warning until a corrected document commits.
3. **Skipped template defaults:** decide whether ready constraints apply.
   Recommend yes for constraints already knowable at that point; do not force
   unavailable dynamic dependencies.
4. **Public target compatibility:** adopting `CanonicalTarget` is
   source-breaking for crate callers, but is required to satisfy C4. The release
   must explicitly accept that change.
5. **Literal diagnostic form:** recommend no backticked expression for typed
   literals; preserve exact source for expression and string-template defaults.

No repository or candidate files were modified.
