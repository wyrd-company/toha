---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Jinja context design verification

## Exact predecessor binding

- Configured defaults: integrated head
  `4738042a766dc7b2d1d3c76a9c83c41c03e803d2`, package tree
  `9d2d185b7bf54ec63297eeb6f390b2686be1c14e`, `design.md` SHA-256
  `0538251bb0c51310d0e288c8a44449085bf66e0a3e3547cf7d354f29c49580a5`.
  The design preserves `presets`, `{ preset: <name> }`, stable `formal_name`,
  `ConfigEntry<T> { value, origin }`, `Resolution { defaults, warnings }`, and
  flat `Seed.defaults`. The new context is a separate field.
- Hook review: approved-at revision
  `132b2f449633ee0db89b29b80c63bdc2eca92e8a`, approved subtree
  `580119ed1fae42ac053469b26d55844886631076`, integrated implementation. The
  design consumes `HookSurface::of`, `ReviewDigest`, and `evaluate_trust`; it
  does not change their digest or approval semantics.
- Canonical target: producer design remains in progress. The recorded consumer
  interface is the existing
  `staging::canonical_target(&Path) -> Result<PathBuf, StagingError>`, with the
  proposed existing-directory empty-suffix repair returning a separator-free
  canonical absolute `PathBuf`. This design consumes that `PathBuf`, requests no
  stronger type, adds no normalizer, and has no dependency on implementation
  completion.

No producer contract conflicts with the synthesized design. The final
configured-default repair was integrated into the epic base before synthesis
was committed.

## Requirement matrix

| Requirement | Final-design evidence | Result |
| --- | --- | --- |
| Variables versus functions | Exact Jinja contract chooses seventeen variables; `now()` stays a function. | Pass |
| Target basename | Nullable Unicode basename from the sole canonical target; root/non-Unicode behavior explicit. | Pass |
| Template short/formal/aliases/source | Exact types, named-registry source rule, zero/multiple aliases, registry order, bundled identity. | Pass |
| OS and architecture | Stable Rust vocabulary and no ambient engine read. | Pass |
| os-release NAME/ID/ID_LIKE | Nullable strings, array tokens, Linux-only parse, missing/malformed rules. | Pass |
| Admin | Unix effective UID, Windows elevated token, failure false, direct supplied. | Pass |
| Interactive | Exact origin matrix; resume preserves snapshot without refusing modality change. | Pass |
| Five environment values only | Fixed optional fields; denied reads none; no generic lookup or map. | Pass |
| `--trust` gate | Two explicit Bob options; recommendation uses effective live trust. | Pending Bob |
| Reviewed registry relation | Option A extends current matching approval; option B does not. | Pending Bob |
| Staged trusted access | Values frozen, redacted, current access rechecked, denial not elevated. | Pending Bob |
| Collision rejection | Seventeen exact names rejected in data, question, computed, and every `each` binding. | Pass |
| Every render surface | Interview and plan surface list plus one shared projection. | Pass |
| Terminal/headless/staged/direct | Caller examples, mode table, staging flow, crate signature. | Pass |
| Presets remain distinct | Exact predecessor names and flat-default result retained; no Jinja preset name. | Pass |
| Canonical target ownership | Existing function result consumed; no stronger type or normalizer. | Pass |
| Error/results contract | Load, context, plan, staging, eval, and render outcomes explicit. | Pass |
| Compatibility | Crate source break, legacy record rule, exact-name-only collision, no path restriction. | Pass |
| Canonical-document impact | Template, guide, CLI, protocol, and registry-reference proposals named. | Pass |
| Falsifiable implementation proof | Fourteen behavior groups cover required values, callers, failures, and predecessors. | Pass |

## Caller-to-type consistency

- Command usage obtains one canonical target and one selected identity before
  constructing the context; the type sketch accepts exactly those values.
- The configured-default caller destructures `Resolution`, reports warnings,
  and passes only the flat defaults map to `Seed`.
- `Seed.context` reaches `Pending`, `Completed`, staging, and planning; no
  candidate-only `Run` abstraction remains.
- `Plan::build` receives the same canonical value as the context and checks it
  without normalization.
- Direct callers supply every external observation explicitly and can use the
  same public context types without CLI or process state.

## Failure pre-mortem

| Failure | Design prevention | Falsifier |
| --- | --- | --- |
| Interview and plan see different values | Completed owns the Seed snapshot; one projection serves both. | Mutation using a fresh plan context changes a golden fixture. |
| Alias input leaks the alias spelling as identity | Resolver carries selected formal name and complete effective aliases. | Alias/formal runs differ in identity output. |
| Untrusted template reads process data | Denied variant performs no reads and projects nulls. | Spy adapter records a read under denial. |
| Revoked approval still exposes staged values | Current access check runs before replay. | Remove approval and replay succeeds. |
| Resume changes branches after environment change | Recorded snapshot, instant, and submissions are replayed. | Change environment and observe different batch or plan. |
| New context shadows authored data | Load rejects exact collisions before evaluation. | A reserved authored id reaches runtime. |
| Existing target produces a trailing separator/name drift | Single repaired canonical result is the source. | Existing/nonexistent equivalent targets give different basename. |
| Config presets become Jinja globals | Context has no preset field; defaults keep the approved flat seam. | A preset appears without a mapped question default. |
| Sensitive text enters logs/errors | Gated types use redacted Debug and errors name fields only. | A sentinel appears in captured stderr/protocol/debug. |
| Session façade duplicates the engine | Final surface changes only Seed/Completed/context projection. | Implementation adds a second public orchestration path. |

## Red-flag verification

- **Shallow module:** pass. One context input hides projection, fallback,
  collision, redaction, and replay behavior.
- **Information leakage:** pass. Resolver, host adapter, access policy, engine,
  staging, and plan each retain their existing knowledge; only domain values
  cross seams.
- **Temporal decomposition:** pass. The context module is organized around the
  facts and invariants it owns, not load/evaluate/render phases.
- **Pass-through methods:** pass. No public context service or lifecycle wrapper
  forwards the existing engine.

## Scope and policy audit

- New permission/access behavior is isolated in the two Phase C decisions and
  separately disclosed. It is not treated as approved by this document.
- Exact collision rejection and current authorization before replay are required
  security bounds for the requested contract; no unrelated supported
  capability is removed.
- No timeout, pinned-version check, application subprocess, arbitrary
  environment access, or second target normalizer is proposed.
- This branch changes design evidence only. Runtime, shared specifications,
  schemas, and guides remain untouched.

## Gates

The package must pass Markdown/YAML checks, `task ci`, design-schema validation
where available, clean-diff review, deck section assertions, published-byte
verification, one `Reveal.initialize`, and visual review of every slide before
the Phase C revision is recorded.
