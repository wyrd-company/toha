---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Rationale — Toha-owned Jinja values

## Problem

The same Jinja evaluation happens during interview and planning, while command
drivers, staged replay, and direct crate callers have different access to host
facts. A value used by an interview condition must be fixed before its first
evaluation and survive through planning. The existing target normalizer and
selected formal name already have owners; the design consumes their results.

## Usage (caller's view)

A template can write `{{ toha_target_name }}`, `{{ toha_template_formal_name
}}`, and `{{ toha_env_user }}` in any supported Jinja field. `toha_env_user` is
`none` without a grant. A command driver starts `toha apply sample ./output`
after capturing the invocation snapshot; `toha apply --answers answers.json
sample ./output` captures `interactive = false`; a crate caller constructs
`InvocationContext`, places it in `Seed`, completes the interview, then calls
`Plan::build`. These call sites require one context input and no Jinja setup by
the caller.

## Shape

`InvocationContext` is an immutable domain snapshot containing the canonical
target, selected template identity, host facts, run facts, and a
denied-or-granted five-value environment record. `Seed` owns it, `Pending`
carries it, `Completed` retains it, and the staged record serializes it. One
context builder projects the snapshot and `Template.name` into reserved
variables for both interview and plan. The command adapter observes host and
process state and evaluates trust before it starts the engine; direct callers
supply domain values. Load validation and readiness share the exact
reserved-name predicate. This encodes the single-snapshot invariant in the seed
type and validates external input at its adapter, per boundary discipline. The
public seam hides projection, collision rules, and replay continuity behind one
required value, per interface depth and locality. The canonical target is
supplied by its existing owner; the context module never normalizes it.

## Synthesis decision

This is an isolated candidate, so no cross-candidate synthesis occurred. The
chosen base is the invocation snapshot shape because it joins early interview
evaluation, completed planning, and durable replay at one interface. A synthesis
pass can compare this whole shape with other candidates without treating this
document as approval of the environment grant.

## Tradeoffs accepted

- We accept a required `Seed.context` crate migration in exchange for explicit
  facts on every caller path.
- We accept a staged snapshot containing five plaintext environment values when
  granted in exchange for deterministic replay; staged-record storage protection
  needs review.
- We accept a renewed access grant before replay in exchange for revocation
  taking effect before stored values can be used.
- We accept a mode-change refusal for staged continuation in exchange for stable
  interview branching.

## Alternatives considered

- MiniJinja globals that read the process on demand have a small apparent
  interface but leak process state into the pure engine and can change values
  between interview and plan.
- A separate `Plan::build` context parameter keeps `Seed` smaller but exposes
  synchronization to every caller and lets interview and plan disagree.
- Re-capturing host and environment values on resume avoids stored values but
  permits a different question path under the same recorded submissions.

## Open questions and risks

- Does Bob approve recommended option 1: a current named-registry hook approval
  or a one-run `--trust` grants the five environment values before interview
  evaluation, with renewal on resume?
- Does the current staged-record storage model adequately protect the five
  plaintext values, or should persistence use a different approved mechanism?
- Which runtime host adapters are available on supported platforms for OS,
  architecture, hostname, and effective administrative identity? The contract
  supplies absence values when an adapter cannot obtain a fact.

## Next implementation step

After the access decision, add the context domain type and shared Jinja
projection, then wire `Seed` and staged replay through it.
