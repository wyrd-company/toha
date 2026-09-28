# Cross-judge report and orchestrator scoring

Two independent scorings of the three candidates against the six rubric criteria
(`../02-arena-rubric.md`), then reconciliation. Scores are 1–5 (5 best).

## Runner pool and dropouts

The configured diverse pool (GPT-5.6 `ocx-gpt-5-6-sol`, GPT-6 `ocx-gpt-6-astra`,
Claude) and an intended cross-family judge could not run: every non-Claude model
routed through the opencodex proxy returned a terminal API error (model
inaccessible: `claude-ocx-native--gpt-5.6-sol`, `…gpt-6-astra`; and `ocx-self`
failed a harness-version gate). The three candidates and the cross-judge therefore
ran on the Claude family (opus), seeded toward structurally distinct directions to
preserve divergence. **Recorded limitation:** no cross-model-family perspective was
obtained; the cross-judge is same-family and independent (a separate agent with no
synthesis context), not cross-family.

## Cross-judge scores (independent agent)

| Criterion | C1 toggle | C2 verb pair | C3 review |
| --- | :-: | :-: | :-: |
| 1 Shape fit / discoverability | 4 | 4 | 3 |
| 2 No fetch/reclone/commit | 5 | 5 | 5 |
| 3 Fidelity to 1069 | 3 | 5 | 5 |
| 4 Scriptability / layers | 5 | 5 | 5 |
| 5 Refusal / error legibility | 4 | 5 | 4 |
| 6 Review integrity / TOCTOU | 3 | 2 | 5 |
| **Total** | **24** | **26** | **27** |

Cross-judge base pick: **C2**, grafting C3's `--digest` freshness pin + read-only
inspect and C1's stderr surface echo. Confidence moderate-high (~0.7); C2↔C3 is
weighting-dependent on criterion 6.

## Orchestrator scores (independent read of all three, end to end)

| Criterion | C1 toggle | C2 verb pair | C3 review |
| --- | :-: | :-: | :-: |
| 1 Shape fit / discoverability | 4 | 4 | 3 |
| 2 No fetch/reclone/commit | 5 | 5 | 5 |
| 3 Fidelity to 1069 | 3 | 5 | 5 |
| 4 Scriptability / layers | 5 | 5 | 5 |
| 5 Refusal / error legibility | 4 | 5 | 4 |
| 6 Review integrity / TOCTOU | 3 | 2 | 5 |
| **Total** | **24** | **26** | **27** |

## Reconciliation

The two scorings agree cell-for-cell; there is no disagreement to reconcile. The
substantive judgments both surfaced:

- **C1 re-implements the trust equality.** `candidate-1-trust-toggle.md:109` uses
  `entry.approval.as_ref() == Some(&digest)` for the grant no-op check instead of
  routing through `evaluate_trust`. That is exactly the rule leak the rubric
  penalizes (criterion 3), unacknowledged by the candidate. The synthesis must not
  carry it: idempotency routes through `evaluate_trust` (C2's approach).
- **C1's dual-mode is a degenerate mirror.** It drops `alias`'s `all-or-none`
  group and collapses `conflicts_with_all` to `conflicts_with`, so it is not the
  `alias` idiom it claims — it is a one-operand toggle whose `--remove <TEMPLATE>`
  reads awkwardly beside `alias --remove <ALIAS>`. C2's critique is correct: for a
  one-operand inverse pair, two verbs are *less* clap machinery than a toggle and
  encode the trust-reads / untrust-reads-nothing asymmetry in the surface.
- **C2's one weakness is criterion 6 (blind approval).** It is *additive* to fix:
  graft visibility onto a clean spine rather than prune an overloaded verb.
- **C3 natively wins criterion 6** but overloads a read-named `review` verb with
  mutation, carries the widest option surface, collapses drift and hard errors onto
  exit 1, and leaves five speculative design questions (new exit codes,
  non-zero-on-`NeedsReview`, spec'd machine JSON). Its integrity *mechanism* is the
  prize; its verb is not.

## Synthesis decision

**Base: Candidate 2 (verb pair `templates trust` / `templates untrust`).** Cleanest
spine — strict `evaluate_trust` routing, minimal clap machinery, most legible
`--help` and error table, least speculation. Its criterion-6 gap is closed by graft,
not by adopting a heavier base.

**Grafts**

- *From C1:* on a successful grant, echo the approved executable surface to
  **stderr** (stdout stays the machine result line), so a human has an audit trail
  of exactly what was trusted while scripts are unaffected. Keep the renderer as
  presentation only — it never compares digests.
- *From C3:* the see-what-you-approve requirement is real, but it is already served
  by `apply`'s exit-3 refusal (which lists the hooks) and `apply --dry-run` — the
  natural path to `templates trust` runs *through* a listing of the hooks. So the
  refusal names `templates trust` for persistence and continues to point at the
  dry-run listing for review; no redundant `review` verb is added.
- *From C3 (recorded, optional):* the `--digest <sha>` freshness pin — approve only
  if the installed surface still equals a caller-supplied digest — as a scriptable
  drift gate. It is presented to Bob as an optional decision and otherwise recorded
  as a backward-compatible future extension; the baseline's single atomic
  read→digest→write already closes the review→approve window by construction.

**Rejections**

- *C1's dual-mode toggle* and its raw-`==` idempotency check (fidelity regression).
- *C3's `review` verb* (redundant with `apply --dry-run`; overloads a read name
  with a write), its non-zero-on-`NeedsReview` proposal, and its several new exit
  codes / spec'd JSON questions (speculative accretion).
