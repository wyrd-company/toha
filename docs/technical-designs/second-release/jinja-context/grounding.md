---
relationships:
  realizes: toha
  references:
    - template-format
    - interview-protocol
    - template-registry
---

# Jinja context values — grounding

## Question and scope

Design one Toha-owned Jinja context for template identity, target, host,
execution status, and five explicitly trusted environment values. The design
must serve terminal, headless, staged/resumed, and direct crate callers through
the same pure interview engine. It must not implement runtime behavior or edit
the shared template specification, schema, or Jinja guide.

## Actual caller-to-result flow

1. `main::run` or `main::stage` calls `setup`, which calls
   `staging::canonical_target(&Path) -> Result<PathBuf, StagingError>` and uses
   the returned path for staged-record identity (`src/main.rs:427-429`,
   `src/staging.rs:40-68`). `main::run` later passes the original CLI `path`, not
   that canonical value, to `Plan::build` and `Plan::apply_reporting`
   (`src/main.rs:1031`, `src/main.rs:1072`).
2. `cli::resolve::resolve_template` produces `ResolvedTemplate { formal_name,
   commit, folder, approval, named }` (`src/cli/resolve.rs:18-27,179-218`). The
   selected formal name is stored in `StagedRecord.template`; the raw operand
   and aliases used to select it are not stored (`src/main.rs:465-473`).
3. `Template::load` canonicalizes the template root, resolves YAML includes,
   validates the schema, and builds `Template { name, data, interview, root,
   ... }` (`src/template.rs:41-58,727-913`). `Template.name` is the short name
   from `template.yml`; it is not the selected formal name.
4. The command driver creates `Seed { now, defaults }` and starts
   `Interview::start(&Template, Seed)` (`src/main.rs:412-425`,
   `src/interview.rs:37-40`). `Pending` owns the seed throughout interview
   progression; `Completed` retains answers, rendered interview hooks, and
   `now`, but no selected-template, target, host, or trust state
   (`src/interview.rs:48-70`).
5. Every interview expression or template rebuilds a map through
   `context_from_answers`: template `data`, then accumulated answers, then the
   private `__toha_now` value. `now()` reads that private value
   (`src/jinja.rs:18-47,192-209`; `src/interview.rs:278-303`). Readiness checks
   separately accept only data keys, prior answers, and registered MiniJinja
   globals (`src/interview.rs:284-323`).
6. `Plan::build(&Template, &Completed, &Path)` independently rebuilds the same
   map and uses it for rendered directory/file names, file bodies, explicit
   file rules, top-level hooks, and apply messages (`src/plan.rs:146-272,
   331-390`). Interview hooks were already rendered as their node was reached.
7. `Plan::apply_reporting` writes files and runs hooks. Its `trusted` option
   gates hook execution only; Jinja has already rendered before this call
   (`src/apply.rs:66-106`; `src/main.rs:1018-1078`).
8. `stage --async` serializes `StagedRecord { target, template, commit, named,
   now, submissions }`. Resume reloads the selected template and live config,
   reconstructs `Seed` with the recorded instant and replays submissions
   (`src/staging.rs:14-28,130-158`; `src/main.rs:635-740,890-1011`).
9. Protocol `Context { target, template, commit }` is output metadata only. It
   comes from the staged record and is not the Jinja evaluation context
   (`src/protocol.rs:13-22`; `src/main.rs:431-437`).
10. Direct crate callers use the public `Template`, `Seed`, `Interview`,
    `Completed`, and `Plan::build` surfaces exported by `src/lib.rs:19-29`.
    They do not have CLI `ResolvedTemplate`, terminal detection, or registry
    state unless they supply equivalent domain values themselves.

The durable input must therefore be captured before `Interview::start`, owned
through `Pending` and `Completed`, and consumed again by `Plan::build`. Merely
adding MiniJinja globals or extending the plan map would not make interview
readiness, interview-node rendering, replay, and direct callers agree.

## Existing names and collision ownership

- `Id::parse` accepts `^[a-z_][a-z0-9_]*$` (`src/template.rs:16-31`). Top-level
  `data`, question ids, and computed ids share one authored namespace. Duplicate
  ids and `each` bindings that shadow ids are load errors
  (`src/template.rs:370-417,785-831`).
- Load-time reference validation and runtime readiness both treat registered
  MiniJinja globals as available (`src/template.rs:388-417`,
  `src/interview.rs:284-323`, `src/jinja.rs:72-80`). A reserved Toha context
  name must be rejected at template load for `data`, question, computed, and
  `each` names; relying on map insertion order would silently shadow a value.
- Config `presets` are a separate config-side namespace. They become question
  defaults only through an explicit per-formal-name mapping and never become
  Jinja context values. No Jinja name may be `preset` or `presets` for this
  feature.

## Identity inputs

- `Template.name` is the template's short name.
- `ResolvedTemplate.formal_name` is the stable selected identity. For an
  installed alias or short name, resolution returns the registry formal key;
  the selecting spelling is not retained. Registry `Entry.aliases` holds every
  installed alias for the formal entry (`src/registry.rs:24-42,554-592`).
- `Entry.source` is the stored source address and `ResolvedTemplate.folder` is
  the selected template root. Folder and direct Git resolutions have no
  registry entry and therefore no registry alias set.
- The approved bundled demo contract is
  `formal_name = "toha-demo"`, `commit = content digest`, `named = false`. It
  uses the same selected identity seam without a special Jinja path
  (`docs/technical-designs/second-release/bundled-demo/design.md`).
- The approved configured-defaults product contract keys `template-defaults`
  by exact `formal_name`, uses `presets` plus `{ preset: <name> }`, and keeps
  presets out of the Jinja namespace. Its internal origin/caller repair is
  still under review while this grounding is written; refresh the exact final
  head and content hashes before Phase C.

## Trust seam

- Approved hook review revision `132b2f449633ee0db89b29b80c63bdc2eca92e8a`
  has content identity tree
  `580119ed1fae42ac053469b26d55844886631076`. Implementation 1033 is integrated.
- A registry entry carries an optional approved executable-surface digest;
  `evaluate_trust(stored, live)` is the policy. `main::run` performs the live
  compare only after the interview is complete, then combines its result with
  the invocation's `--trust` flag (`src/main.rs:813-815,1018-1078`).
- Registry trust is available only for a selected named user/system entry.
  Folder, direct Git, local-registry, and bundled selections require the
  one-run `--trust` flag for trusted behavior (`src/main.rs:438-444`,
  `src/cli/resolve.rs:179-218,241-301`).
- The new trusted environment values are a separate permission. The context
  must receive an explicit access decision derived by the driver; it must not
  reach into registry/process state or infer access from identity. Bob must
  approve the exact access semantics at Phase C.

## Host and execution observations

- The current library reads no username, hostname, OS release, shell, editor,
  or visual-session environment into Jinja. The binary reads `VISUAL`/`EDITOR`
  only to choose a multiline editor (`src/cli/terminal.rs:99-108`).
- `std::env::consts::{OS, ARCH}` supplies compile-target strings, not a
  caller-provided host record. `/etc/os-release` is Linux-specific and may be
  absent, unreadable, malformed, or omit individual keys. The design must state
  exact fallbacks rather than turn missing host metadata into template failure.
- The binary currently determines interaction separately at decision sites:
  new apply uses stdin terminal status; resumed apply uses stdin and stdout;
  headless answers and `stage --async` do not prompt (`src/main.rs:571-572,
  683-690,820-821,982-1007`). The pure engine does not know the driver.
- No current admin detector exists. Direct callers need to state admin and
  interactive status explicitly; a pure engine must not inspect the process.
- Environment strings are not guaranteed Unicode. A design that exposes
  strings must define absent/non-Unicode behavior for `USER`/`USERNAME`,
  hostname, `EDITOR`, `SHELL`, and `VISUAL`.

## Error and replay constraints

- Template-load problems aggregate as `LoadError { problems }`; render faults
  use `EvalError` during interview and `PlanError::Render` during planning.
  Reserved-name collisions are template-load errors because they do not depend
  on an invocation.
- The error-attribution design owns the only target normalizer:
  `staging::canonical_target(&Path) -> Result<PathBuf, StagingError>`. Its
  proposed result is a separator-free canonical absolute `PathBuf`, including
  an existing directory. This design consumes that returned value and adds no
  normalizer or implementation dependency. Refresh its final approved carrier
  before implementation 1060 starts.
- Resume re-reads live config and the current host/process environment today,
  but it freezes `now`, selected identity, target, and submissions in the staged
  record. The design must choose and state which new values are staged versus
  re-captured. A value used before staging must not change during replay and
  produce a different interview path without an explicit compatibility rule.

## Why evidence

### Direct evidence

- Commit `77309c0b913c7acfc52df08060131237f658a329` introduced the shared
  context builder, `Seed`, staged replay, plan rendering, and driver structure
  as one first-release change. Git blame shows the relevant seams still derive
  from that commit.
- Pull request 2 states that terminal users, scripts, agents, and staged
  continuation share one interview engine, and that untrusted hooks require an
  explicit grant: <https://github.com/wyrd-company/toha/pull/2>.
- The current template-format specification states that the evaluation context
  contains data, answers, computed values, and the seeded `now()` function
  (`docs/specifications/template-format.yml:25-45`).
- The board records the approved hook-review permissions decisions, the bundled
  identity, the approved presets product contract, and error-attribution's
  canonical-target ownership.

### Inferences

- Because replay is intended to reproduce the same interview from its recorded
  inputs, invocation context used by interview expressions likely belongs in a
  seed-owned immutable domain value, not in ambient global functions.
- Because the current selected identity and canonical target already cross the
  CLI/library seam as ordinary data, extending the explicit caller input likely
  gives better locality and testability than teaching the pure engine to inspect
  host or registry state.

### Sources searched and gaps

- Source control: blame and history for `jinja.rs`, `interview.rs`, `plan.rs`,
  `main.rs`, `staging.rs`; PR 2. Found the first-release seam and stated goals.
- Task/issue tracker: canonical tasks 1060, 1069, 1072, 1073, 1074, 1075 and
  Epic 1065; GitHub issue search. Found board decisions; no GitHub issue for this
  context feature.
- Long-form repository documents: template, protocol, registry, CLI, bundled,
  hook-review, and configured-default designs. Found current contracts.
- Lore repository/task/note/conversation search for the feature terms returned
  no relevant result; semantic retrieval was unavailable, so only keyword search
  ran.
- Real-time chat, infrastructure observability, exception tracking, and product
  analytics were not searchable: no matching MCP or repository source is
  available. These are evidence gaps, not evidence of absence.

## Preserve / Change / Avoid / Risk

### Preserve

- One pure interview engine for terminal, headless, staged/resumed, and direct
  crate paths.
- One context construction rule across interview and plan rendering.
- Approved selected-template identity, presets separation, hook-review trust,
  and `canonical_target` ownership.
- Existing Jinja and error behavior outside the reserved names.

### Change

- Add one explicit immutable context input that survives interview progression
  and supplies planning.
- Reserve the exact `toha_` names against every authored-id surface.
- Add explicit driver adapters for host facts, execution status, and access-
  gated environment values.

### Avoid

- A second target normalizer, identity resolver, trust evaluator, config
  namespace, or process-environment map.
- Hidden process reads inside Jinja/interview/plan modules.
- CLI-only context construction that leaves direct callers inconsistent.
- Treating protocol `context` or config `presets` as the Jinja context.

### Risks

- Capturing host/environment data at the wrong time can make resumed execution
  change branches or leak values after a trust decision changes.
- Function-based values can hide absence and type semantics; many flat variables
  enlarge the collision and documentation surface.
- A design that carries aliases or source by re-resolving the formal name can
  disagree with the selected template, especially for folder, direct Git,
  local, and bundled identities.
- Platform-specific admin, username, hostname, architecture, and os-release
  vocabularies can become accidental policy unless their exact types and
  fallbacks are explicit.
