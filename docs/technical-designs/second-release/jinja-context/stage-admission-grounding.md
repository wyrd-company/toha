---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
    - error-attribution
    - jinja-includes
---

# Stage environment admission — targeted grounding

## Fixed product decisions

- Direct and new-apply callers use option 1A: environment access is granted by
  an explicit `--trust` flag or by a current matching reviewed approval for an
  eligible named user/system registry entry.
- A stage is stricter. A template that can observe an environment value
  requires the explicit `stage --trust` spelling. Registry approval does not
  substitute for that flag.
- Stage discovers environment references before interview or staged-state
  progress. If access is required and `--trust` is absent, stage fails before
  `Seed`, `Interview::start`, any question or message render, and any staged
  write.
- The admitted decision and captured values carry through all later batches.
  `continue` has no trust option and performs no environment or host read.
  Staged apply does not elevate or revoke environment admission. Its existing
  `--trust` remains a separate hook-execution decision.
- Only user, hostname, editor, shell, and visual are access-gated. The process
  environment as a collection is never exposed.
- Direct crate callers supply typed facts. The interview engine performs no
  CLI, registry, filesystem, process-environment, host, or privilege lookup.
- The producer-owned target factory is
  `staging::canonical_target(&Path) -> Result<CanonicalTarget, StagingError>`.
  `CanonicalTarget::as_path()` is the only projection. There is no unchecked
  constructor, `From<PathBuf>`, consumer normalizer, or `TargetError`.

The exact staged wire representation and its plaintext consequence remain a
manager decision. The design must propose the smallest deterministic snapshot
that satisfies the fixed behavior without treating storage as already
approved.

## Earliest common admission seam

`Template::load` is the first operation that knows the selected template and
has parsed its configuration fields. Today `stage` loads the template at
`src/main.rs:578`, then constructs `Seed` and starts the interview at
`src/main.rs:582`. New apply has the same seam at `src/main.rs:830-834`.

Admission therefore belongs after complete template analysis and before seed
construction. A normal denied direct/apply decision performs none of the five
gated reads and projects nulls. A stage whose analysis requires access is not
a denied run: it is refused without `stage --trust`.

The staged store writes only after interview evaluation or accepted
submissions (`src/main.rs:591,610,698`). Moving admission to the seam above
keeps the required refusal ahead of state progress. Canonical target, live
configuration, selected-template resolution, and template-load failures can
still occur first because they own prerequisite facts.

## Complete Jinja surface inventory

Template load currently compiles and records references for these
configuration-derived surfaces:

- question prompt, description, placeholder, default, required, when,
  computed value, format, options, and numeric validation bounds;
- interview message text and condition;
- interview and top-level hook conditions, iteration expressions, command
  arguments, argument lists, and working directories;
- explicit `files:` iteration, condition, target path, and source body;
- before-apply and after-apply messages.

`validate.regex`, hook script paths, file-rule source paths, source-directory
names, globs, names, descriptions, answers, configured defaults, and data
values are not recursively interpreted as Jinja source.

Two rendered source-tree surfaces are not inspected by template load today:

- each non-static source-tree path segment; and
- each non-static source-tree file body.

They compile only during `Plan::build` (`src/plan.rs:154-165,378-381`). The
admission design must move their reference discovery to template load without
rendering them or changing the ownership of canonical target identity.

The approved Jinja-include design adds literal includes only to rendered file
bodies. An explicit `files:` body retains one compiled `FileTmpl` and unions
the transitive literal-include closure during template load. An ordinary
source-tree body and its closure are still discovered only when planning walks
that file. Dynamic include names, imports, extends, and blocks are rejected.
Admission must inspect ordinary bodies and their literal closure before the
interview starts. YAML `!include` remains structured configuration loading,
not a Jinja include surface.

## Reference-analysis defect

`Tmpl::compile` and `Expr::compile` currently store MiniJinja
`undeclared_variables(false)`. That is not a sound admission oracle.

1. Toha enables MiniJinja's `builtins` feature. Its `debug()` built-in formats
   the whole current context. A template can observe all injected environment
   values without spelling any `toha_env_*` name. The undeclared set reports
   only the registered global `debug`.
2. MiniJinja's undeclared walk records assignment targets before walking the
   right-hand side, while code generation evaluates the right-hand side first.
   `{% set toha_env_user = toha_env_user %}` therefore performs a gated read
   that the current undeclared set can omit. Equivalent ordering matters for
   self-shadowing constructs.
3. MiniJinja does not follow includes when computing undeclared variables.
   Every selected literal include closure must be unioned by Toha.

The admission analysis must preserve supported syntax. It may not ban
`debug()`, assignment, aliases, macros, filter/function arguments, or dynamic
attribute/item expressions. A complete conservative AST walk must visit reads
in evaluation order, treat any possible access to the built-in `debug`
callable as observing all five environment values, and union every supported
literal include closure. It is control-flow insensitive: a reference in an
uncalled macro or false branch still proves possible observation.

A string literal such as `object["toha_env_user"]` is not a root lookup. A
dynamic subscript expression such as `object[toha_env_user]` is. Aliasing an
environment value or the `debug` callable must retain the corresponding need.
Any future generic context lookup must define a new sound admission rule; it is
not implicitly allowed by this design.

## Mutable source problem

Pre-interview analysis becomes a security-relevant fact. A folder template or
other mutable source can change between admission and planning. A minimal
referenced-value snapshot is sound only if later rendering consumes the exact
analyzed compiled sources, or if a stable content identity is verified before
rendering. Re-scanning changed bytes during replay and asking for access later
would violate the fixed no-reauthorization behavior.

The design must choose one owner for this relationship. It must not introduce
a second target normalizer, a reusable permission token, or a replay-time
environment decision.

## Context and replay ownership

One immutable invocation context must flow through `Seed`, `Pending`,
`Completed`, and `Plan`. Interview and planning project one private Jinja map.
Admin, originating-interactive state, host facts, selected template identity,
aliases, canonical target, and environment snapshot remain stable across all
renders.

The staged wire record is not the live domain context. `CanonicalTarget`
cannot be deserialized because the approved producer permits construction only
through its factory. `Store::load` validates the serialized target and supplies
the factory-created carrier when it reconstructs the live context.

Replay reconstructs the seed from recorded facts, then replays ordered
submissions. It does not read the current environment, recompute the stage
grant, or require `continue --trust`. Apply-time hook trust remains current and
independent.

## Trust accuracy

For option 1A, an eligible direct/new-apply selection must resolve to a named
user/system registry entry and have a live `HookSurface` whose digest matches
the stored approval through `evaluate_trust`. Folder and direct-Git operands
carry no approval. A local alias overlay must not broaden eligibility merely
because an inherited digest exists.

`HookSurface` covers hook nodes and files executed by hooks. It deliberately
does not bind every Jinja-bearing source file. Therefore the approved policy
can grant environment access to changed non-hook Jinja content while the hook
approval still matches. The design must state this boundary; it must not call
all rendered content reviewed.

## Storage facts and candidate choice

The store serializes pretty JSON and relies on its directory and process umask;
it does not set a new explicit file mode. A captured value remains readable in
the staged record until successful staged apply removes the record, `abort`
removes it, or an operator removes it.

Candidate designs must compare at least these representations:

- only the five referenced values, including referenced-but-absent state;
- all five fixed values after any admitted reference; and
- an admitted source/context snapshot that makes later rendering use the
  analyzed bytes.

An admission boolean without values is not a deterministic snapshot because it
would authorize fresh reads in later processes. Encryption introduces key and
recovery policy outside this design. A raw or open-ended environment map is
forbidden.

## Collision and compatibility constraints

The seventeen exact public context names and their types remain as designed.
They are reserved against data, questions, computed values, local loop names,
and callable/global collisions at template load. The whole `toha_` prefix is
not reserved. Configured presets remain distinct from Jinja context.

Legacy staged records contain no host/context snapshot. Compatibility must not
invent values, deserialize an opaque target, or claim that environment values
are unobservable based on exact-name scanning. Existing supported `debug()`
templates make that claim false. The design must state one attributable legacy
outcome and its recovery without adding a later environment-access retry path
for newly admitted records.

## Preserve / change / avoid / risk

Preserve:

- one pure interview engine and one Jinja projection;
- nullable host and environment facts with exact type semantics;
- approved direct/apply option 1A and explicit stage flag semantics;
- producer-owned canonical target and `StagingError`;
- current configured-default and hook-review ownership.

Change:

- analyze every supported Jinja-bearing source before interview start;
- add `--trust` to `stage` only;
- persist a deterministic admitted snapshot for replay;
- separate staged wire data from the live opaque target carrier.

Avoid:

- whole-environment exposure, later ambient reads, permission tokens, timeouts,
  application subprocesses, or new unsupported template restrictions;
- treating `approval.is_some()` as effective trust;
- letting staged apply trust elevate environment access;
- a second target normalizer or unchecked target constructor.

Risks:

- static analysis and mutable source identity are now security boundaries;
- stored plaintext outlives current registry approval;
- hook review does not attest all environment-reading Jinja content;
- early source-tree parsing changes when existing template errors surface;
- legacy records cannot supply an exact snapshot without inventing facts.
