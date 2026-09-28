# Rubric — template-specific configured defaults (reframed shape)

Re-derived for the reframed model: named stored values + explicit per-template
references, replacing the implicit global-by-id override. **Withheld from
candidates**: candidates receive only the common task and the grounding. This
rubric is the readonly cross-judge's tool and the orchestrator's own scoring tool.

Score each criterion 1–5 (5 = fully satisfied with gradeable evidence in the
candidate's sketch and rationale).

## C1 — No implicit id coupling; references are explicit

No stored value reaches a question except through an explicit mapping. The design
proves that two questions with **different** ids can reference one stored value,
and that a question whose id equals another template's id is **unaffected** unless
its own mapping references a value. The config author never relies on question-id
equality across authors. Template selection keys on template **identity**
(formal name), never a bare/ambiguous short name; the bundled `toha-demo` identity
is reusable as a selector.

## C2 — Reference vs literal representation is unambiguous and well named

A default entry cleanly distinguishes "reference the stored value named N" from
"use this inline literal." A literal that happens to spell a value name cannot be
silently misread as a reference (and vice versa). The top-level stored-values
property has a clear name that is **not** `defaults`; the reference syntax is named
and legible in real YAML. The representation is parsed once at the boundary into
domain types.

## C3 — Resolution safety: missing, typing, cycles

A reference to a missing stored value is a clear, attributed error — never a silent
empty default. A stored value whose type does not match the referencing question's
answer kind is caught and attributed to the config site and the question. If
references can chain (a value referencing another), cycles are detected and refused
with a clear error; if chaining is disallowed, the design states that and why. Only
the value actually used for a question is validated (no spurious errors from unused
store entries).

## C4 — Layering, engine purity, path parity

The stored-values store and the per-template mappings each merge across
system/user/local with a total, deterministic precedence, and the interaction
between them is defined. Resolution happens at the config/resolve boundary and
flattens to the unchanged `Seed.defaults: IndexMap<Id, RawAnswer>`; the pure engine
stays identity- and store-unaware. Results are identical across terminal, headless,
staged, direct, and crate paths, and defaults are re-resolved against live config
on resume, never frozen. No production stubs or premature shared schema/spec edits.

## C5 — Safe transition of existing `defaults:` with no silent data loss

Existing global-by-id `defaults:` config is handled by an explicit transition
disposition, presented as options with a recommendation: keep working under a
deprecation window, auto-translate to the new shape, or reject with a clear message
and a documented conversion. Existing values are never silently dropped or silently
reinterpreted; the user is told what happened and what to do. The disposition is a
named product decision for the checkpoint.

## C6 — Interface depth, contained surface, future direction

The config surface added is small relative to the capability it hides (deep module,
not shallow); no information leakage of internal representation; no new trust,
permission, timeout, pinned-version-check, or subprocess surface; locality at the
existing config/resolve boundary; passes the red-flag screen (shallow module,
information leakage, temporal decomposition, pass-through method). At least two
broader reusable-value/reference directions are compared (on accidental effects,
user control, author dependence, identity/name stability, configuration
complexity) with a recommended future direction that is not over-built now.
