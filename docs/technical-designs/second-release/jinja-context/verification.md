---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
    - error-attribution
    - jinja-includes
    - stage-admission-synthesis
---

# Jinja context design verification

## Exact predecessor binding

- **Configured defaults:** integrated design base
  `4738042a766dc7b2d1d3c76a9c83c41c03e803d2`, package tree
  `9d2d185b7bf54ec63297eeb6f390b2686be1c14e`, design SHA-256
  `0538251bb0c51310d0e288c8a44449085bf66e0a3e3547cf7d354f29c49580a5`.
  The context keeps presets, formal name, `Resolution { defaults, warnings }`,
  and flat `Seed.defaults` distinct.
- **Hook review:** approved design revision
  `132b2f449633ee0db89b29b80c63bdc2eca92e8a`, subtree
  `580119ed1fae42ac053469b26d55844886631076`, with implementation integrated.
  The context consumes `HookSurface::of`, `ReviewDigest`, and
  `evaluate_trust` without changing the digest or hook gate.
- **Canonical target:** approved producer revision
  `067c8d2e7c95cf2b16ab3a4103f8b1a8fda331af`, design SHA-256
  `72a564799b95ab1c187c913ac14ef14938f880bf2ab24ec337c03f887991457b`.
  The design is integrated at revision
  `dfe7ba017ebef525310db8b8ab4ead58fae2d147`, design SHA-256
  `9560139e48798429a95e06a695dea703817b673d2e18f19e25f3fe4a3efe9b`.
  The exact interface is
  `canonical_target(&Path) -> Result<CanonicalTarget, StagingError>`,
  `Store::load(&CanonicalTarget)`, `Plan::build(..., &CanonicalTarget)`, and
  `CanonicalTarget::as_path()`. There is no `TargetError`, unchecked
  constructor, or `From<PathBuf>`.
- **Jinja includes:** integrated design revision
  `b87a5482b97e81524272bee1e526698c862754db`, final-design SHA-256
  `3c996d09a6f845ba1125f1447ca8ff110dfb6149a0862f21845cff9c7b126eeb`.
  The context keeps literal includes confined to rendered file bodies, unions
  their transitive closure, and does not turn YAML `!include` or configuration
  fields into loader surfaces.

The integration base is `dfe7ba017ebef525310db8b8ab4ead58fae2d147`.

## Requirement matrix

| Requirement | Authoritative evidence | Result |
| --- | --- | --- |
| Seventeen exact values | Variable table fixes names, types, source, and absence. | Pass |
| Target ownership | Producer carrier only; staged text validated; no normalizer. | Pass |
| Template identity | Short/formal names, registry-order aliases, source, and bundled rules. | Pass |
| Host types | Rust OS/arch, Linux os-release fallbacks, no execution. | Pass |
| Admin and interaction | Effective authority; originating driver; frozen on resume. | Pass |
| Five fixed environment values | Named optional fields; no map, dynamic lookup, or whole environment. | Pass |
| Direct/new-apply trust | Explicit flag or eligible current matching review. | Approved |
| Stage trust | Missing flag refuses a need; explicit flag captures and carries five even without a need. | Approved |
| Continue/replay | No trust option, reauthorization, ambient read, or access abort/retry. | Pass |
| Complete analysis | Config, paths, bodies, includes, aliases, shadowing, dynamic operands, and `debug()`. | Pass |
| Supported syntax | Uncertain forms conservatively need all five; no syntax restriction. | Pass |
| Failure order | Need without flag fails before capture, seed, render, or write. | Pass |
| Minimal deterministic wire | Approved Unavailable or five fixed plaintext optional values; no carrier or grant token. | Approved |
| Mutable folders | Current bytes use the approved recorded decision; no source or program gate. | Approved |
| Legacy stages | Pre-context projection, no new names or invented facts. | Pass |
| Collision contract | Seventeen exact names only; legacy keeps prior set. | Pass |
| Presets distinct | Config resolution and flat defaults unchanged. | Pass |
| Every caller and surface | Terminal, headless, staged/resumed, direct; one projection. | Pass |
| No prohibited mechanism | No timeout, runtime version check, subprocess, or new permission name. | Pass |

## Caller-to-type consistency

- Every current caller constructs `InvocationContext` before engine entry.
- The ordinary `Seed.context` route reaches `Pending`, `Completed`, staged wire,
  and planning.
- Configured start and replay retain the origin-bearing `Resolution` until
  engine entry; only the ordinary caller route uses flat `Seed.defaults`.
- `Template::admit_environment` owns the need/decision/capture matrix; callers
  do not receive an analysis mask.
- `FixedEnvironmentSource` exposes exactly five named optional values. It is
  called for an explicit carried stage grant even without a need, and never for
  no-flag/no-need, denial, or missing stage trust.
- `Plan::build` retains the producer's exact target parameter and checks it
  against the completed current context before rendering.
- `Store::load` receives the producer carrier and reconstructs context from
  record text plus wire facts; the wire cannot deserialize a carrier.
- Legacy replay selects the prior available/reserved set and projection before
  template building, so existing data identifiers and `debug()` behavior do
  not acquire the current contract.

## Reference-analysis proof boundary

The analyzer does not use `undeclared_variables(false)` as an access oracle.
MiniJinja's enabled `debug()` can enumerate the whole context, and its current
undeclared walk can miss a self-shadowing right-hand-side read. The design uses
the parser AST, evaluation-order walking, lexical resolution, possible-callable
tracking, and include-closure union instead.

Soundness rules:

- any direct fixed-name root read marks a need;
- any possible call of the built-in `debug` marks all five;
- every dynamic operand is visited, while literal property text is not treated
  as a root read;
- every branch and macro body is considered reachable;
- every supported literal include contributes its closure; and
- uncertainty marks all five rather than allowing access or rejecting syntax.

The retained render program closes the same-process time-of-check/time-of-use
gap. Cross-process folder mutation is handled by the fixed five-value snapshot,
not a new content-identity refusal.

## Failure pre-mortem

| Failure | Prevention | Falsifier |
| --- | --- | --- |
| `debug()` exposes uncaptured values | It marks all five and granted need captures all five. | Trusted debug fixture omits a captured value. |
| Self-shadow hides an environment read | Right-hand side is visited before target binding. | Self-shadow stage succeeds without trust. |
| Included body bypasses admission | Literal closure needs union into its containing body. | Stage without trust reaches interview for an included environment reference. |
| Source-tree path/body is first seen during planning | Load compiles all non-static path/body Jinja. | Interview starts before a bad or gated tree source is found. |
| Missing stage trust progresses state | `RequireStageGrant` fails before source capture, seed, interview, render, submission, and store. | Any corresponding spy counter increments. |
| Explicit no-need stage grant is discarded | `CarryStageGrant` captures all five once. | A later-added reference is null or an adapter spy count is not five. |
| Later batch rereads host/environment | Wire carries the complete snapshot. | Changed ambient state changes output or spy records a read. |
| Staged apply trust elevates Jinja access | Hook and environment decisions are separate. | `apply --trust` changes a saved unavailable snapshot. |
| Mutable folder edit forces restage | No content identity gate exists. | A source edit returns SourceChanged/ProgramChanged. |
| Mutable edit asks for an uncaptured fixed value | Any captured stage grant or admitted direct/apply need captures the closed set of five. | Later-added fixed name renders fresh or false data. |
| Legacy `debug()` leaks new facts | Legacy projection injects none of the new names. | Legacy debug contains a new context key. |
| Target path is reconstructed unsafely | Producer carrier enters Store, context, plan, and apply. | Wire or consumer constructs `CanonicalTarget`. |
| Plaintext leaks into diagnostics | Redacted Debug and field-only errors. | Sentinel appears in stderr, protocol, or logs. |
| Presets become Jinja metadata | Context has no preset field. | Preset appears without a mapped answer/default. |

## Red-flag verification

- **Shallow module:** pass. One admission method hides source discovery,
  analysis, failure ordering, and capture.
- **Information leakage:** pass. Analysis masks and source identities remain
  private; wire does not duplicate target or formal name.
- **Temporal decomposition:** pass. `Template` owns analyzed/rendered sources;
  context owns facts and projection.
- **Pass-through method:** pass. No session facade or supplied-value duplicate
  admission route is added.

## Approved closure and policy audit

- Direct/new-apply effective trust, explicit stage-only trust, and the producer
  target carrier are approved inputs.
- The bounded plaintext fixed-five snapshot and mutable-folder outcomes are
  approved by the exact prior deck approval plus the stage-only carried-grant
  amendment. This closure adds no broader environment, permission, or storage
  policy.
- The design adds no access recheck, later trust flag, source-change refusal,
  generic environment map, encryption policy, or staged-file permission rule.
- MiniJinja `unstable_machinery` is an existing-dependency feature isolated
  behind a compile-time adapter. No runtime pinned-version check is proposed.
- Runtime, shared specifications, schemas, and guides remain untouched on this
  branch.

## Implementation proof

The paired implementation must falsify the fourteen behavior groups in
[design.md](design.md), including two mutable-folder transition tests and a
legacy `debug()` golden fixture. It must run the repository gate after the
include design is present on its base.

### Named sole-kill obligations

| Named implementation test | Plausible mutation that it must kill |
| --- | --- |
| `stage_no_refs_without_trust_records_unavailable_without_reads` | Route no-flag/no-need through capture or perform one fixed-source read. |
| `stage_no_refs_with_trust_captures_fixed_five_once` | Collapse `CarryStageGrant` into the no-need short circuit. |
| `stage_refs_without_trust_fails_before_progress` | Move refusal after seed, interview start, render, submission, or store. |
| `stage_refs_with_trust_captures_fixed_five_once` | Capture only referenced fields or read one field twice. |
| `continue_replays_snapshot_without_ambient_access` | Invoke trust evaluation or the fixed source during replay. |
| `mutable_folder_no_flag_added_reference_stays_null` | Read the newly referenced ambient value or introduce an access gate. |
| `mutable_folder_carried_grant_supplies_later_reference` | Drop an unreferenced captured field or introduce a source/program identity refusal. |
| `canonical_target_factory_is_the_only_constructor` | Add an unchecked conversion or replace a consumer carrier with `PathBuf`. |
| `configured_context_start_preserves_winning_origin` | Destructure `Resolution` into flat defaults before engine start. |

Each mutation must reach and fail the named assertion, then be restored before
the full gate runs. A compile error or unrelated failing test is not a kill.

## Checkpoint artifact gates

The exact revision checkpoint records:

- Markdown and YAML validation;
- repository `task ci`;
- working-tree and diff cleanliness;
- design, machine-readable design, verification, source, committed deck, and
  publication-evidence SHA-256 values;
- same-draft Postplan publication and local/published byte equality;
- one `Reveal.initialize` call; and
- every horizontal and vertical slide rendered at 1280×720, checked for
  viewport/body overflow, and visually inspected.

Those exact values live in [publication.md](publication.md) and the canonical
checkpoint record so that this clean design verification does not narrate old
publication revisions.
