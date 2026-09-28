---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Candidate task — Toha-owned Jinja context values

Produce one complete candidate design package for Toha 0.2.0. This is a design
artifact only: do not edit runtime code, canonical specifications, schemas, or
guides.

Read `grounding.md`, the architect runner prompt, rationale template, design red
flags, and deep-module guidance supplied by the orchestrator. Write caller usage
first, including two or three real call sites spanning command and crate use.
Then derive exact data types, signatures, module/seam diagram, error/results
contract, compatibility, canonical-document proposals, falsifiable validation,
and one-page rationale.

The design must choose variable versus function form and exact reserved
`toha_` names/types for:

- canonical target basename;
- template short name, selected formal name, aliases, and source, including no
  aliases and multiple aliases;
- OS, architecture, and Linux os-release `NAME`, `ID`, `ID_LIKE`, including
  absent/malformed behavior;
- admin and interactive status across terminal, headless, staged/resumed, and
  direct callers;
- user, hostname, editor, shell, and visual environment values, accessible only
  under an explicitly supplied trust/access decision.

Define collision rejection against `data`, question ids, computed ids, and
`each` bindings. Make every value available in interview prompts/defaults/
conditions/computed/messages/hooks and in rendered paths, contents, explicit
file rules, top-level hooks, and apply messages. Preserve resumed execution.

Constraints:

- Keep config-side `presets` and `{ preset: <name> }` distinct from Jinja
  context; do not name a Jinja concept `preset` or `presets`.
- Consume the selected `formal_name` contract, the approved hook-review trust
  policy, and error-attribution's existing
  `staging::canonical_target(&Path) -> Result<PathBuf, StagingError>` result.
  Add no second normalizer and no implementation dependency on 1056.
- Preserve the pure engine and every supported terminal/headless/staged/direct/
  crate path. External input is parsed once at its boundary into domain types.
- Do not expose the complete process environment. Every `toha_env_` value,
  including aliases and resumed execution, requires the exact approved access
  semantics.
- Do not introduce a timeout, pinned version check, application subprocess,
  unsupported restriction, or new permission outside the requested access gate.
- Use generic, non-identifying examples.
- The design must present Bob with explicit options and a recommendation for
  permissions/access semantics before paired implementation 1060 can start.

The candidate is viable only if a maintainer can trace one value from caller
capture through interview, staged replay, completed planning, and rendered
output without hidden state or duplicated policy.
