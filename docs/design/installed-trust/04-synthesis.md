# Synthesis

How the final design (`05-design.md`) was assembled from the three candidates. The
per-criterion scores, dropouts, and reconciliation are in
`03-candidates/cross-judge-report.md`; the candidates are in `03-candidates/`.

## Base

**Candidate 2 — the `templates trust <name>` / `templates untrust <name>` verb
pair.** It scored highest on fidelity to the 1069 policy (idempotency routed
through `evaluate_trust`, no raw digest equality), scriptability, and refusal/error
legibility, with the least speculative surface. Its argument that a one-operand
inverse pair is *less* clap machinery than a dual-mode toggle — and that two verbs
make the trust-reads-and-can-fail / untrust-reads-nothing asymmetry legible — is the
load-bearing shape decision, and it holds.

## Grafts

- **Approved-surface echo to stderr on a successful grant** (from Candidate 1).
  `templates trust` records the digest *and* prints the executable surface it
  approved to standard error, so a human log shows exactly what was trusted while
  standard output stays the machine result line. The renderer is presentation only;
  it never compares digests, so the trust rule stays solely in `evaluate_trust`.
- **See-what-you-approve served by the existing dry-run** (adapted from Candidate 3).
  Candidate 3's insight — trust is a statement about *reviewed* content — is
  honored without its `review` verb: `apply`'s exit-3 refusal already lists the
  hooks, and `apply --dry-run` shows them on demand, so the caller reaches
  `templates trust` having already seen the surface. The refusal names
  `templates trust` for persistence and keeps pointing at the dry-run listing for
  review.
- **`--digest <sha>` freshness pin, recorded and offered (from Candidate 3).** The
  scriptable drift gate — approve only if the installed surface still equals a
  supplied digest — is presented to Bob as an optional decision and otherwise kept
  as a backward-compatible future extension. It is not in the baseline because the
  baseline's single atomic read→digest→write already closes the review→approve
  window by construction, and `evaluate_trust` re-checks at apply time.

## Rejections

- **Candidate 1's dual-mode toggle** — a degenerate mirror of `alias` (it drops the
  `all-or-none` group), with an awkward `--remove <TEMPLATE>` breadcrumb — and its
  raw `entry.approval == Some(&digest)` idempotency check, which re-implements the
  trust equality outside `evaluate_trust`.
- **Candidate 3's `review` verb** — redundant with `apply --dry-run`, and it
  overloads a read-named verb with a mutation — together with its
  non-zero-on-`NeedsReview` proposal and its several proposed new exit codes and
  spec'd machine-JSON questions (speculative accretion).

## Dropouts and limitations

The intended cross-family runner/judge pool was unavailable (every non-Claude proxy
model returned a terminal API error; `ocx-self` failed a harness-version gate). All
three candidates and the cross-judge ran on the Claude family, seeded toward
distinct directions. No cross-model-family perspective was obtained; this is
recorded as a limitation of the exploration, not a gap in the design.

## Verification (summary; full falsifiable set in `05-design.md`)

- Caller usage reconciled against the signatures: the three call sites in
  `05-design.md` type-check against `trust`/`untrust`, `resolve_user`,
  `HookSurface::of`, `evaluate_trust`, and `RegistryFile::set_approval`.
- Every rubric criterion is met by the synthesized design: idiom fit (verb pair),
  no fetch (installed-folder load only), 1069 fidelity (`evaluate_trust` sole
  comparator), scriptability (no gate; user-layer-only), refusal/errors (names
  `templates trust`; full error set), and review integrity (dry-run listing + stderr
  echo; TOCTOU closed by atomic grant).
- Compatibility with the approved 1069 revision (`132b2f4`) checked field by field:
  `entry.approval`, `HookSurface`, `evaluate_trust`, the effective-trust `list`
  column, and the local-layer ban are consumed unchanged.
