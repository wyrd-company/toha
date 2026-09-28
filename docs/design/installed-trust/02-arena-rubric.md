# Arena framing and rubric

The common task given to every candidate, the runner pool, and the scoring rubric
used for cross-judgment and synthesis. Candidates receive the task and the
grounding (`01-grounding.md`) and the runner discipline; they do **not** receive
this rubric.

## Design task given to candidates

Design the command surface a caller uses to grant and revoke hook-trust for an
**already-installed** template in Toha 0.2.0, consuming the approved 1069 policy
(`entry.approval: Option<ReviewDigest>`; `HookSurface::of/digest/diff`;
`evaluate_trust`). Trust and untrust must **not** fetch, reclone, or change the
installed commit — they read the installed surface and write the user registry.
Resolve: (1) the command shape; (3) whether the untrusted-hooks refusal names the
new operation; (4) registry persistence and the error set. Decision (2) — what
approval binds — is fixed by 1069 (the executable-surface digest, independent of
commit) and must not be reopened. Write caller usage first, then types, signatures,
module/seam map, and the error/results contract. No production stubs or runtime
changes; sketches live in the design package.

## Runner pool (selected)

Default architect slots are four `inherit-parent`. To force structural divergence
and enable a cross-family judge, a diverse pool was selected across model families:

| Slot | Runner (agent) | Model family | Direction seeded |
| --- | --- | --- | --- |
| 1 | `ocx-gpt-5-6-sol` | GPT-5.6 | free (no seed) |
| 2 | `ocx-gpt-6-astra` | GPT-6 | free (no seed) |
| 3 | `general-purpose` (Claude) | Claude (parent family) | free (no seed) |

Each runner works in an isolated directory (`/tmp/arena-installed-trust/candidate-<n>/`);
candidates never share a writable output path. Dropouts and re-runs are recorded
in `04-synthesis.md`.

## Criteria (3–6, gradeable)

Each scored 1–5 against the candidate's design, by an independent cross-judge and
by the orchestrator.

1. **Command-shape fit and discoverability.** The surface fits the existing
   `templates` idiom (mirrors `alias`'s dual-mode or `add`'s flag where apt),
   adds the minimum new surface for the capability, is symmetric for grant/revoke,
   and is discoverable from `--help` and from the refusal. *Observable:* a caller
   who knows `templates alias`/`add` can grant and revoke trust without surprise.

2. **No fetch / reclone / commit change (hard constraint).** Trust and untrust
   read only the installed template folder (`entry.path`) and mutate only the
   registry; no network, no clone-swap, no `set_commit`. *Observable:* running the
   command offline on an installed template succeeds and leaves `commit`/`path`
   byte-identical.

3. **Fidelity to the 1069 policy.** Approval is the executable-surface digest via
   `HookSurface::of(installed).digest()`; the trust decision is produced only by
   `evaluate_trust` (no re-implemented equality); approval is not bound to name or
   commit. *Observable:* an update that leaves the surface unchanged keeps trust;
   one that changes it lapses trust — with no bookkeeping in the trust command.

4. **Scriptability and adapter/layer preservation.** The command runs headless
   with no new interactive gate a script cannot pass; it writes only the user layer
   (sparse round-trip preserved); the local layer still cannot grant trust.
   *Observable:* a CI script grants trust non-interactively; a local-layer entry is
   refused.

5. **Refusal and error legibility.** The untrusted-hooks refusal names the new
   operation instead of `templates add --trust`; the error set covers name not in
   user registry, ambiguous name (exit 5), unreadable/escaping installed surface
   (cannot trust), and a no-op (already trusted / not trusted), each naming the
   command that fits. *Observable:* every wrong-form/wrong-time request yields a
   message that names the command the caller meant.

6. **Review integrity (see-what-you-approve, TOCTOU).** The shape lets a caller
   review the exact executable surface being approved and does not silently record
   a surface the caller never saw or one that raced a change between review and
   approve — without sacrificing scriptability. *Observable:* there is a defined,
   non-racy path from "review these hooks" to "approve exactly those".
