# Rubric — template-specific configured defaults

Derived in Phase B(Frame) from this task's observable outcomes and invariants.
**Withheld from candidates**: candidates receive only the common task and the
grounding artifact. This rubric is the picker's tool for the readonly cross-judge
and the orchestrator's own criterion-by-criterion scoring in synthesis.

Score each criterion 1–5 (5 = fully satisfied, gradeable evidence in the
candidate's own sketch and rationale).

## C1 — Unambiguous identity selection

Every selection form resolves to exactly one default set: bare folder, `./x`,
absolute path, git address, installed formal name, alias, a **duplicate short
name**, and the bundled `toha-demo` identity. Two templates that share a short
name — or share a question title — receive **separate** defaults. The design keys
on template identity (formal name), never on short name alone or an ambiguous
selector. States what happens when a selector matches nothing and when a selector
is itself ambiguous.

## C2 — Global-by-id preserved with total, explicit precedence

The existing global-by-question-id capability is preserved, not silently removed,
narrowed, or reinterpreted. Precedence is total and deterministic across two
dimensions: specificity (global-by-id vs template-specific) and config layer
(system < user < local). The design presents disposition/migration **options with
a recommendation** for existing global-by-id settings, and flags any narrowing as
requiring Bob's explicit approval.

## C3 — Engine purity and path parity

The pure interview engine's `Seed.defaults: IndexMap<Id, RawAnswer>` contract is
unchanged; the engine stays identity-unaware. Template-specific selection is
resolved at the boundary and flattened to the same map. Results are identical
across terminal, headless, staged, direct, and crate paths for identical inputs.
Configured defaults are re-resolved against live config on resume, not frozen into
the staged record. No production stubs, runtime changes, or premature shared
schema/spec edits in the design task.

## C4 — Boundary validation and error attribution

External input is parsed once at its boundary into domain types. Validation covers
matching and nonmatching templates, all layers, aliases, and invalid value types.
Each invalid configured default is attributed to its config file, its selector,
and its question id (extending the current `configuration key defaults.<id>`
contract). A selector that never matches any template is handled predictably
(defined, not a silent surprise).

## C5 — Interface depth and contained surface

The public/config surface added is small relative to the capability it hides
(deep module, not shallow). No information leakage of internal representation
across modules; no transport/registry types on the public surface. No new trust,
permission, timeout, pinned-version-check, or subprocess surface. Locality: the
change concentrates where identity is already resolved and where config is already
parsed. Passes the design red-flags screen (shallow module, information leakage,
temporal decomposition, pass-through method).

## C6 — Future reusable-default direction

At least two plausible broader reusable-default mechanisms are compared on:
accidental cross-template effects, user control, template-author dependence,
identity stability, and configuration complexity. A recommended future direction
is given, and the design does **not** expand this slice's implementation into a
global mechanism without Bob's approval. The near-term design leaves room for the
recommended direction without committing to it.
