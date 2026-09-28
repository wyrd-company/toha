---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Jinja context cross-judge

The read-only cross-judge recommends Candidate 1 as the base. Its immutable
`InvocationContext` follows the existing `Seed → Pending → Completed → Plan`
path and survives staged replay. The scores assess candidate designs as
written; they do not approve a permissions policy.

## Scores

| Candidate | Contract | Flow | Trust and replay | Depth and locality | Integration and proof | Weighted total | Viability kill |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 4 | 5 | 4 | 4 | 4 | 4.25/5 | No |
| 2 | 3 | 4 | 2 | 3 | 3 | 3.05/5 | Yes: refuses staged continuation when interaction mode changes. |
| 3 | 4 | 4 | 2 | 3 | 3 | 3.30/5 | No, subject to approval of its separate grant. |
| 4 | 3 | 4 | 2 | 3 | 3 | 3.05/5 | Yes: rejects a filesystem-root target. |
| 5 | 3 | 2 | 1 | 2 | 2 | 2.05/5 | Yes: keeps a direct-crate path without the new context. |

Candidate 1 loses one contract point because it turns unavailable OS or
architecture observation into an invocation error instead of a host-data
fallback. Its source-breaking `Seed` change is explicit and traceable.

## Decisive seams

### Interface depth and locality

Candidates 1–4 place one typed snapshot on `Seed`, carry it through completion,
and use one Jinja projection for interview and planning. Candidate 1 gives the
clearest ownership. Candidate 5's `Run` prevents some mismatches, but adds six
operations around the still-public engine and retains a second legacy workflow.
That adds coordination instead of hiding it.

### Compatibility and staged replay

Candidate 1 states the crate source break and does not invent historical facts.
Candidate 2 preserves some old records but refuses a valid change in
continuation mode. Candidate 3 detects legacy incompatibility only when prior
submissions stop fitting, which can miss a changed branch. Candidate 4 keeps
legacy names unavailable but rejects a valid root target. Candidate 5
reconstructs legacy facts and leaves existing direct callers without the common
context.

### Trust semantics

Candidates 1, 2, and 4 recommend deriving access from `--trust` or a live
approved named-registry review. Candidate 1 alone both rechecks a saved grant
before replay and keeps a saved denial denied. Candidate 2 rechecks but adds
the invalid mode-change refusal. Candidate 4 does not recheck, so revoked
approval can expose stored values. Candidates 3 and 5 recommend a separate
grant and replay stored granted values without current authorization.

### Exact public contract

All candidates choose variables and propose seventeen names, but disagree on
material details:

- Candidate 1 makes `toha_target_name` nullable for root and non-Unicode paths;
  Candidate 4 uses lossy text; Candidate 5 rejects the target.
- Candidate 1 exposes registry identity without inventing source values for
  folder selections; Candidate 2 invents folder and direct-Git source values.
- Candidate 1 keeps `ID_LIKE` as a nullable string; Candidates 2–5 split it
  into an array.
- Candidates 1–3 use `toha_admin` and `toha_interactive`; Candidates 4–5 use
  `toha_is_admin` and `toha_is_interactive`.
- Candidate 1 treats empty gated values as absent and uses a host observation;
  Candidate 5 keeps empty strings and reads `HOSTNAME`.

Candidate 1 reserves the whole `toha_` prefix. Candidates 3–5 reserve exact
names. Every candidate keeps config `presets` outside Jinja.

## Red-flag screen

`P` means the candidate addresses the flag, `C` means a concern remains, and
`F` means the shape exhibits the flag.

| Candidate | Shallow module | Information leakage | Temporal decomposition | Pass-through method |
| --- | --- | --- | --- | --- |
| 1 | P: one snapshot hides projection and absence rules. | C: copied short name can drift from `Template.name`. | P: grouped by domain facts. | P: projection adds policy. |
| 2 | C: callers also coordinate target and mode contracts. | C: source and target policy spread across adapters and planning. | P | P |
| 3 | P | C: target equality and legacy identity are duplicated. | P | C: `capture` must own real access policy. |
| 4 | P | C: target identity sits beside planner destination. | P | P, except its invalid root rule. |
| 5 | F: six session operations plus a legacy workflow. | F: legacy empty facts violate the universal contract. | C: it coordinates existing modules by lifetime. | C: the dual path weakens the wrapper. |

Candidate self-screens miss Candidate 2's and Candidate 4's supported-path
restrictions, Candidate 3's stale-grant replay, and Candidate 5's dual contract.

## Recommendation, grafts, and rejections

Use Candidate 1's seed-owned snapshot, single projection, denied/granted
five-value type, saved denial, and current-grant check before replay. Replace
its required host-observation error with fallbacks and take the short name only
from `Template.name`.

- From Candidate 2, graft the pre-render target consistency check and the
  legacy-record reference check. Reject inferred folder source values and the
  continuation-mode refusal.
- From Candidate 3, graft the rule that denied access performs no gated reads
  and the staged identity consistency check. Reject a separate grant as the
  base, stale-grant replay, and submission-fit legacy detection.
- From Candidate 4, graft concrete Unix/Windows admin observation and a typed
  target mismatch. Reject root refusal and replay after approval revocation.
- From Candidate 5, retain the invariant that planning uses the completed
  interview's template and target at the existing seam. Reject the `Run`
  workflow, context-free legacy caller path, and non-Unicode target refusal.

The remaining consequential decision is the exact access grant for five
environment values. Bob must approve it before implementation.
