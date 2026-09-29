<!-- rumdl-disable MD013 -->

# Cross-judge: Toha project updates

## Scores

Each cell gives the score and its justification.

| Criterion | Candidate 1 | Candidate 4 |
|---|---|---|
| **C1 — Applied record** | **5/5.** “Decision 1” selects project-resident `.toha/applied/*.json`, explains travel and rejects git-only carriers. It stores formal name, commit, frozen generation instant, completed answers, and per-identity fingerprints; target is derived from the containing project, and XDG staging stays distinct. | **4/5.** “Durable model” selects `.toha-project.json` and stores formal name, commit, answers, and actual merge bases, with boundary validation; it avoids `.toha.yml` and does not use staging. It says “target-relative source-root identity” but does not define the target field or why a source-root identity supplies the required project target. |
| **C2 — Merge semantics** | **5/5.** “Decision 2” defines old render, disk, and new render and gives separate Whole, Region, and JsonValue outcomes, including drift, removals, deleted operator files, and no-op idempotence. Fingerprint degradation restricts retraction, and the region checksum treatment preserves the approved drift boundary. | **4/5.** “Update semantics” defines base/theirs/ours and distinct behavior for Whole, Region, JsonValue, removal, and second-run idempotence. The Whole text merge is specified but conflict output has no actual resolution workflow, and the `JsonValue` preservation operation is only referred to as an approved path that does not exist in the prerequisite interface. |
| **C3 — Merge engine** | **5/5.** “Merge engine” names gitoxide `gix-merge` through the existing `gix` merge feature, an in-process implementation with no subprocess, and notes the precise API needs confirmation. It adds no separate merge dependency and includes `diffy` only as a named fallback. | **3/5.** The semantic section names `diffy` and explicitly says it is pure Rust and has no subprocess, satisfying the core engine constraint. It does not justify `diffy` against the existing `gix` merge capability or give the build-vs-buy evidence requested for a new dependency. |
| **C4 — Composition and routes** | **4/5.** “Decision 4” retains mutation kinds, answer envelope identity, staged-record separation, and one-apply hook results; it also describes all caller modalities and adoption. It nevertheless proposes an extra applied-record identity authority and an update refusal when any staged interview exists, and the unresolved-marker preflight adds a new restriction. | **3/5.** The design keeps the project record distinct from the external answers envelope and staged state, retains hook-result lifetime, and adopts the prescribed mutation kinds. It is silent on adoption’s operational behavior beyond saying no interview is needed, does not show all driver routes, and its `reconcile`/`ProcessRunner` sketch leaves the hook/trust seam unclear. |
| **C5 — Interface and proof** | **4/5.** Caller usage, domain types, signatures, module map, results, and a detailed version-A-to-B fixture shape are supplied; the public project module hides most machinery. It exposes several caller-ordered phases, omits canonical document/schema impacts, and adds substantial unapproved policy (notably reserved `.toha/`, unresolved-marker refusal, and exit 6). | **2/5.** Usage, types, signatures, and ownership rules are concise and mostly consistent, and it gives a simple conflict retry flow. The proposed two-stage public API is shallow orchestration, errors/results are not exhaustive, there is no falsifiable fixture or canonical artifact impact, and it leaves key seams hypothetical. |
| **Total** | **23/25** | **16/25** |

## Red-flag screen

| Red flag | Candidate 1 | Candidate 4 |
|---|---|---|
| Shallow module | **Partial hit.** `ProjectRecord::open` → `begin_update` → `Begun::plan` → `UpdatePlan::apply` exposes multiple workflow stages, but the private reconciler hides the hard merge and persistence decisions. | **Hit.** `ProjectUpdate::prepare(...).reconcile(...)` is described as one deep operation, yet the caller must sequence it and handle workflow outcomes; the implementation boundary is mostly a façade over unspecified resolver, mutation, and apply work. |
| Information leakage | **Hit.** Public `ProjectRecord`, `Application`, `Revision`, and `answers_document()` expose applied-record concepts and answer-envelopes; the wire type stays private, but project storage and identity are visible as crate API. | **Hit.** The public-looking `ProjectRecord` and `PreparedUpdate` include record state and merged entries; error reports also expose base/operator/proposed file payloads. |
| Temporal decomposition | **Hit.** The module diagram and public flow are ordered as baseline → interview → plan → apply, which tracks execution time as well as responsibility. | **Hit.** The public method split is explicitly `prepare` then `reconcile`, and the seam map follows those sequential phases. |
| Pass-through methods | **Partial hit.** `UpdatePlan::changes()` and `Application::template()/revision()` are getters, but there is no broad chain of methods forwarding to another module. | **Hit.** `ProjectUpdate::prepare` then `.reconcile(&ProcessRunner)` reads as a thin staged pass-through; the design does not justify why callers need the split. |

## Constraint check

| Check | Candidate 1 | Candidate 4 |
|---|---|---|
| Application subprocess | No subprocess for merge. Hooks use the existing hook runner. | No subprocess proposed for merge; `ProcessRunner` is underspecified but appears to be the existing hook runner. |
| New permission/access rule | **Hit.** Refusing updates whenever a staged interview exists and refusing unresolved conflict markers add access/preflight restrictions not required by the task. | None stated. |
| New/changed timeout | None. | None. |
| Pinned-version check | No explicit pinned-version validation beyond parsing a commit identity. | No explicit pinned-version check. |
| Injection semantics/interface | Retains Whole/Region/JsonValue semantics, but proposes additive `retract_*` and `write_region_body` injection functions; these are explicitly raised as open interface changes, not silent changes. The Region marker reinsert/checksum approach needs contract review. | No silent change claimed, but it assumes a source-preserving JsonValue replace operation beyond the approved `FileMutation` producer view; it flags that seam as an open question. |
| `{template, answers}` identity | The stored answers are inner completed answers, while an optional external document keeps the envelope. However, it explicitly adds the applied record as a third identity authority and raises this for approval. | Record answers are explicitly separate from the external envelope and staged submissions; no extra identity rule is stated. |
| Per-apply hook-result lifetime | Preserved: hooks rerun for B under the existing trust gate; output is not stored. | Preserved in prose: hooks rerun per update and outputs are not persisted. |
| Region/JsonValue collapsed to whole-file merge | No. Both remain distinct ownership kinds. | No. Both remain distinct ownership kinds. |
| Applied record conflated with XDG staged record | No; explicitly separate. | No; explicitly separate. |
| `.toha.yml` collision | No direct collision; `.toha/applied/` is a different path. But reserving all `.toha/` template output is a new capability restriction. | No direct collision; `.toha-project.json` differs from `.toha.yml`. |

## Factual errors and unsupported claims

### Candidate 1

- The “Usage” says `toha update ./sample-service --async <ANSWERS>` even though the approved agent route is `update P --async`, and the answer document is supplied as a separate file argument/continuation route; it also invents command grammar without fully reconciling it with the approved route table.
- The map names `source::materialize(address, commit)` as though it is an existing source seam. The pinned source and grounding do not define that function; it is a proposed API and should be labeled as such. The open question later correctly treats it as a needed capability.
- The record example says a folder template has an “empty commit,” while its wire representation uses `null` and its domain uses `Revision::Unversioned`. Those are different representations and should be made consistent.
- “JsonValue ... no three-way merge” and “an operator change ... is overwritten” accurately retain the approved convergent behavior; this is not an error.

### Candidate 4

- The crate example uses `ProcessRunner`, which is not established as the hook-runner type by the provided grounding. This is an unsupported API name, though it is not evidence of an application subprocess.
- The design says JsonValue writes via an “approved source-preserving mutation path,” but the approved interface provided to this design is `Plan::mutations()` and `FileMutation`; no update-time replace-value operation is part of that contract. Candidate 4 does flag this as an open question, so it is an unresolved seam rather than a hidden contract change.
- No other factual claim about the named merge dependency conflicts with the grounding delta; `diffy` is a pure-Rust in-process library, but its suitability/adoption is not evidenced in the design.

## Base recommendation

Use **candidate 1** as the synthesis base. It is substantially stronger on identity-level behavior, old-template re-render verification, safe fallback when the old source is unavailable, region drift, adoption, hook composition, and a concrete fixture plan; the refreshed base also supports its `gix-merge` direction.

## Grafts from candidate 4

- Store merge-base content in the project record, at least for Whole text and Region bodies, or explicitly compare this option during synthesis. This removes dependence on fetching T@A and avoids candidate 1’s degraded fingerprint mode, though size and sensitive-answer/content storage need a deliberate trade-off.
- Use candidate 4’s compact decision notation and explicit conflict payload model as a presentation aid, while making conflict resolution and retry behavior concrete.
- Keep the clear refusal of record-less projects if adoption is not needed, but reconcile this with candidate 1’s useful render-only adoption flow rather than treating refusal as the only safe choice.

## Rejections

- **Candidate 1:** reject reserved `.toha/` output as an implicit capability removal; choose an applied-record path and collision policy that do not silently ban template paths. Reject unresolved-marker preflight and staged-record refusal unless separately justified and approved. Do not add exit code 6 without a stated contract decision. Do not claim `source::materialize` as an existing API until designed.
- **Candidate 1:** reject `--on-conflict template` as a routine policy unless its destructive overwrite effect and approval are resolved; preserve the safe no-write conflict default. Retain fingerprint mode only as an explicit fallback with its non-merge limits made visible.
- **Candidate 4:** reject its assumption that “base content in record” alone makes the operation safe: define all identity validation, conflict retry, JSONC source-preserving edits, path safety, and multi-application ownership before implementation. Reject the unspecified `source_root` field and the public `prepare/reconcile` split until their responsibilities are made coherent.
- **Both:** reject any whole-file treatment of Region or JsonValue and any use of stored hook output. Neither design proposes those violations, but synthesis must preserve the explicit prerequisite contracts.
