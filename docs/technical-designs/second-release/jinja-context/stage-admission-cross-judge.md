---
relationships:
  realizes: toha
  references:
    - stage-admission-grounding
    - stage-admission-rubric
---

# Stage environment admission cross-judge

The read-only Claude Opus judge recommends Candidate 2 as the base after
removing its cross-process source-identity gate. It created or edited no files.
The first CLI launch put the prompt after a variadic option and exited before
reading evidence; the corrected launch completed. This is a judge-launch retry,
not a candidate dropout.

## Scores

| Candidate | Analysis | Program ownership | Trust and lifecycle | Replay state | Integration and proof | Weighted total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 3 | 3 | 4 | 3 | 3 | 3.20/5 |
| 2 | 4 | 4 | 5 | 4 | 4 | 4.20/5 |
| 3 | 4 | 3 | 4 | 4 | 3 | 3.65/5 |

No candidate triggers an automatic viability kill. Candidate 1 comes close
because it proposes failing template load for a supported syntax form that the
analyzer does not model. A sound fallback must mark an uncertain form as able
to observe all five values.

## Decisive findings

- Candidate 1 has the deepest admission interface and the best location
  retention, but its referenced-only wire has no complete cross-process source
  identity. Its proposed replay refusal is therefore undefined.
- Candidate 2 has the clearest fixed-five wire, direct trust rule, and
  uncertainty fallback. It pays twice by capturing all five and also refusing
  changed sources.
- Candidate 3 leaks the analysis pipeline through `environment_needs`,
  `stage_access`, `capture_needed`, and `stage_context`. Its program identity
  also covers unrelated static bytes.

All source-identity replay refusals would narrow mutable folder-template
behavior. Today a resumed folder stage loads current bytes. Candidate 2's
`SourceChanged` and Candidate 3's `ProgramChanged` would instead discard that
supported path unless the operator aborts and restages. Named and Git templates
are already commit-pinned, so the extra gate is redundant there. The change is
not approved and is rejected.

## Smallest deterministic snapshot without a new replay restriction

Capture all five fixed optional values when, and only when, the initial loaded
template has any environment need and access is granted.

- A strict subset cannot support a later mutable-folder edit that adds another
  fixed reference without either a new refusal or a false null.
- An admission flag without values would authorize later ambient reads.
- Persisting the source program is larger and would silently ignore folder
  edits.
- Five is the closed vocabulary. `debug()` already needs all five.

When the initial template has no need, record an unavailable snapshot and read
nothing, even if `--trust` was present. A later folder edit that adds an
environment reference sees nulls. When the initial template had a need and was
granted, later folder edits can see only the five values already captured. Both
outcomes require explicit manager disclosure and tests.

## Red-flag screen

| Candidate | Shallow module | Information leakage | Temporal decomposition | Pass-through |
| --- | --- | --- | --- | --- |
| 1 | Pass: one admission method. | Pass. | Pass. | Pass. |
| 2 | Concern: duplicate `admit` methods. | Fail: caller threads source identity and target twice. | Pass. | Concern: supplied-value and target forwarding. |
| 3 | Fail: caller coordinates four stages. | Fail: needs mask crosses CLI, context, wire, and store. | Concern. | Concern. |

## Recommendation

Use Candidate 2's fixed-five wire and exact trust rules. Graft Candidate 1's
single `Template::admit_environment` seam and attributed reference locations.
Remove source identity from the caller and staged wire, remove changed-source
refusal, make uncertain AST forms require all five rather than fail, and keep
the producer's exact target carrier interface. Reject Candidate 3's wrapper and
public analysis pipeline.
