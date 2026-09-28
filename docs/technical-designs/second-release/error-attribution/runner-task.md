---
relationships:
  references:
    - architecture
    - interview-protocol
    - template-format
---

# Candidate task — error attribution and early-answer behavior

Produce one complete candidate design package for Toha design 1072. This is an
architect sketch, not implementation. Read these files first:

- `/workspaces/references/skills/pi-pstack/skills/architect/SKILL.md`
- `/workspaces/references/skills/pi-pstack/skills/architect/references/runner-prompt.md`
- `/workspaces/references/skills/pi-pstack/skills/architect/references/rationale-template.md`
- the supplied Phase A `grounding.md`
- approved configured-defaults design
  `/workspaces/worktrees/toha/design-template-defaults/docs/technical-designs/second-release/template-defaults/design.md`

The approved 1073 behavior uses `presets`, `{ preset: <name> }`, exact
`formal_name` mappings, and errors that name the config file, mapping site, and
optional preset hop. Its review is repairing internal origin carriage, caller
coverage, and verification without changing that approved behavior. State the
integration assumption you need; do not invent a competing origin carrier as a
second source of truth.

Design all six items as one coherent shape:

1. Attribute `files[i].when`, `files[i].each`, explicit `files[i].path`, and
   ordinary target-path render faults in the established
   `template error in <field> `<expression>`: <message>` form.
2. Classify a template-authored default that fails its own constraint as a
   template fault.
3. Carry the exact selected configured-default provenance through later
   constraint validation while preserving terminal/headless recovery.
4. Present and recommend a policy for a known literal-constraint failure on an
   early answer when the question's skip cannot yet be decided.
5. Present and recommend whether an invalid early answer for a question proven
   skipped by the same document is omitted beside a current-batch failure.
6. Own one separator-free canonical-target interface used by staged identity,
   protocol context, planning, and future Jinja context. Do not create a second
   normalizer or an implementation dependency on design 1075.

Write caller usage first with at least three concrete call sites: one library
caller, one staged/headless flow, and one completed-plan flow. Then derive:

- domain data/type sketches;
- public and crate-visible function signatures;
- a module/seam diagram;
- error and result contracts with exact examples;
- early-answer state transitions and ordering;
- persistence/replay and compatibility behavior;
- proposed specification/schema/guide impact;
- falsifiable test scenarios, including apply/continue equivalence;
- a rationale shaped exactly by the rationale template, with real alternatives
  and tradeoffs.

Keep pseudocode and `unimplemented!()` bodies inside the design artifact. Do not
edit production code, shared specifications, schemas, or guides. Use generic,
non-identifying examples. Name any conflict with the approved prerequisite
rather than silently changing it.

Write `design.md` and `rationale.md` in the isolated output directory supplied
by the arena orchestrator.
