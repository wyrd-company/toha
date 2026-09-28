---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Jinja context synthesis

## Arena execution

No configured architect-runner file or cross-judge pool was present. Four
independent foreground Codex CLI sessions used the configured default
`gpt-6-sol` model in isolated worktrees from grounding-only commit `57e60e9`.
They received the common task and grounding, not the rubric. All four completed;
there were no dropouts. They converged on a context snapshot carried by `Seed`
and `Completed`.

A fifth blind session used the same model and grounding-only base with a
directed whole-shape constraint: a `Run` session, not `Seed` or `Completed`,
must own the context and lifecycle. It completed without rubric access or
dropout. This supplied the required structurally distinct shape. The read-only
cross-judge then used an independent Codex CLI session because the configured
pool was absent and the Claude service limit was still active. The judge edited
nothing.

## Architect scores

The architect read all five designs and rationales end to end before scoring.
Scores use the rubric's 1–5 scale and weights.

| Candidate | Contract | Flow | Trust and replay | Depth and locality | Integration and proof | Weighted total | Disposition |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 4 | 5 | 4 | 4 | 4 | 4.25/5 | Base |
| 2 | 3 | 4 | 3 | 3 | 3 | 3.25/5 | Reject as written: mode-change refusal narrows staged use. |
| 3 | 4 | 4 | 2 | 4 | 3 | 3.45/5 | Reject policy: proposes a new grant outside `--trust`. |
| 4 | 4 | 4 | 2 | 4 | 3 | 3.45/5 | Reject as written: root refusal and stale-grant replay. |
| 5 | 3 | 3 | 1 | 2 | 2 | 2.30/5 | Reject shape: dual public workflows violate caller parity. |

### Criterion evidence

- **Candidate 1:** complete variable table and strongest replay check; one
  snapshot follows the actual ownership path. It wrongly makes unavailable host
  OS/architecture an error, duplicates the short name, keeps `ID_LIKE` scalar,
  and reserves more than the requested exact names.
- **Candidate 2:** exact trust revalidation and useful target consistency, but
  invented source values and a continuation-mode refusal weaken contract and
  compatibility.
- **Candidate 3:** a coherent typed snapshot and strong denied-read rule, but a
  separate permission is outside the required `--trust` gate and a persisted
  grant survives revocation.
- **Candidate 4:** the clearest exact names, host fallbacks, and platform admin
  rules, but root-target rejection removes supported input and replay never
  reauthorizes stored values.
- **Candidate 5:** the session façade owns the lifetime, but its six-operation
  interface wraps existing modules while retaining a context-free legacy path.
  It also proposes a separate permission and restricts non-Unicode targets.

## Judge reconciliation

The judge and architect select Candidate 1 at the same 4.25 score and agree on
every viability concern. Score differences do not change the ordering:

- The architect gives Candidate 2 trust/replay 3 rather than 2 because it does
  revalidate stored access; the separate mode-change refusal remains a kill.
- The architect gives Candidate 3 depth 4 rather than 3 because its snapshot
  interface is small even though its access policy is wrong.
- The architect gives Candidate 4 contract and depth 4 rather than 3 because
  most value semantics and its typed seam are precise; the root restriction and
  stale authorization still reject it as written.
- The architect gives Candidate 5 flow 3 rather than 2 because its new path is
  internally traceable; the second legacy path still breaks the universal
  contract.

## Synthesis decision

Candidate 1 is the base. The final shape keeps one immutable
`InvocationContext` on `Seed`, moves it through `Pending` to `Completed`, stores
it with staged state, and projects it through one private Jinja context builder
for interview and planning.

Hand grafts:

- Candidate 2: array-valued `ID_LIKE`, target consistency before planning, and
  a legacy-record reference check.
- Candidate 3: denied access performs no reads of the five gated sources;
  staged target and formal identity must agree with the saved record.
- Candidate 4: `toha_is_admin` / `toha_is_interactive`, Unix effective-UID and
  Windows elevated-token semantics, host-data fallbacks, and a typed target
  mismatch.
- Candidate 5: planning must consume the context attached to the completed
  interview, not a newly captured context.

Corrections made by the architect:

- Preserve the registry's effective alias order instead of sorting again.
- Reserve only the seventeen exact public names, so this design does not
  silently forbid unrelated existing `toha_` identifiers.
- Take the short name only from `Template.name`; the selected identity carries
  formal name, aliases, and source.
- Treat root and non-Unicode target basenames as `null`, not an error or lossy
  text.
- Keep OS and architecture as stable Rust target-vocabulary strings and make
  optional host metadata non-failing.
- Add `--trust` to `stage` and `continue` as the existing permission spelling,
  subject to Bob's Phase C approval. No new permission name is introduced.

Rejected choices:

- MiniJinja globals or process reads during evaluation: hidden state and broken
  replay.
- A separate environment permission: outside the requested `--trust` gate.
- Re-reading environment values on resume: changes prior branch decisions.
- Replaying stored granted values after current authorization is lost: exposes
  data after revocation.
- Refusing terminal continuation of a headless stage: removes a documented
  modality change.
- A new `Run` façade: broad lifecycle redesign with a second public workflow.
- A second target normalizer or stronger canonical-target type: owned by the
  error-attribution design and unnecessary here.
- A Jinja `preset` or `presets`: conflicts with the approved config namespace.

## Red-flag result

The final interface adds one required domain value to an existing deep seam.
The context module hides seventeen names, typed absence, trust-gated capture,
collision policy, and replay serialization. The CLI adapter is a real adapter
because command and crate callers obtain facts differently. No public
load/validate/render pipeline, pass-through service, process map, or alternate
caller workflow remains.
