# In-project generators — grounding

Phase A of the architect workflow. Traced caller-to-result flow, types, state
ownership, error paths, persistence/replay, and trust seams for the code and
contracts an in-project generator touches, with file/symbol evidence. This is
the constraint set every candidate design must honor.

Scope of the feature (from the task): **repeated template applications at
project subpaths, each with its own answers and identity, with explicit access
to available project snapshots as optional defaults.** The accepted startup
policy (Bob, A) is fixed: available snapshots seed defaults when present; when
absent, fall back to explicit configured presets / template defaults / normal
questions; an explicitly required snapshot reference that cannot be satisfied
fails clearly. No independent second persisted-answers lifecycle; no implicit
matching of answers by question id.

## What exists today (how)

### Applying a template into a target (and a subpath)

`toha apply TEMPLATE PATH` runs one uniform five-step spine, shared by every
route (`src/main.rs:624` `setup` → `src/main.rs:641` `environment` →
`cli::resolve::resolve_template` → `src/main.rs:638` `load_template` →
`src/main.rs:604` `resolution`), then drives the interview to a `Plan` and
applies it.

- PATH becomes a `CanonicalTarget` in exactly one place, `canonical_target`
  (`src/staging.rs:164`). It makes the path absolute, lexically normalizes
  `.`/`..` **without touching disk**, walks up to the nearest existing
  ancestor, canonicalizes that ancestor, and re-appends the not-yet-existing
  suffix. So a subpath like `./src/components/foo` where `foo` does not exist is
  a well-defined target: `src/components` is real, `foo` is appended.
- Every emitted file path is a `TargetPath` (`src/plan.rs:31`), validated by
  `TargetPath::parse` (`src/plan.rs:33`): no absolute, no backslash, no drive
  letter, no `.git` segment, and `..` may never escape the target. A second
  runtime guard `has_symlink_component` (`src/plan.rs:85`) rejects a symlink
  anywhere in the chain, at plan time (`src/plan.rs:603`) and again at apply
  (`src/apply.rs:177`). A file is always written at `target.join(TargetPath)`.

**Subpaths already work as apply targets with no new path machinery.** The
target/subpath is purely an `apply` argument; it is never declared in
`template.yml` (contrast the git `#path` fragment in `src/source.rs:39`, which
selects which *source* subdirectory of a repo is the template — a different
axis).

### Protecting existing files

Conflict detection is two-phase: `Plan::add` (`src/plan.rs:611`) records a
`conflicts` entry when the destination already exists, and apply re-scans every
planned file against disk (`src/apply.rs:162`) to close the build→apply window.
Then `!options.force && !conflicts.is_empty()` is a hard `ApplyError::Conflicts`
that writes nothing (`src/apply.rs:170`). `--force` overwrites whole files
wholesale (`src/apply.rs:232`). `ApplyOptions` is just `{ force, trusted }`
(`src/apply.rs:18`).

The only per-file mechanism that coexists with existing content is `inject`
(managed text regions and typed JSON values), resolved fully in memory first
(`src/apply.rs:368` `resolve_edits`) with drift detection for operator-edited
regions. There is **no** merge, rename, or skip-existing mode for whole files.

### Per-application identity today

There is identity, but it is keyed **entirely by the canonical target path**;
there is no separate "application instance" object.

- `InvocationContext` (`src/context.rs:73`) binds one `CanonicalTarget` plus the
  selected template and host/execution facts. It projects reserved Jinja names
  (`src/context.rs:37`); the only target-derived one is `toha_target_name`, the
  **final path component** of the target (`src/context.rs:301`). So
  `./src/components/foo` yields `toha_target_name = "foo"` — the entire built-in
  per-subpath distinction available to templates.
- `Plan::build` re-derives the context and refuses a plan whose
  `context.target()` differs from the target it is applied to (`src/plan.rs:401`,
  `PlanError::ContextTarget`). A plan is single-target by contract; it cannot be
  retargeted.
- Staging keys a `StagedRecord` by `Sha256` of the canonical target path
  (`src/staging.rs:205`). Applying one template into two subpaths writes **two
  distinct records**, distinguished only by target path. Re-applying into the
  same subpath overwrites the same record.

There is **no** notion of a named application, an application id, an instance
registry, or a record that a template was applied N times. Only staged
interviews (one per target) exist.

### The interview engine, defaults, and raw submissions

The engine is a pure, UI-free state machine `Interview` (`src/interview.rs:60`):
`Asking(Pending)`, `Complete(Completed)`, `Ended(Ended)`. It imports no I/O.
Adapters read `Pending::batch()`, pre-check with `Pending::check`, and submit a
`RawAnswers` map through `Pending::answer` (`src/interview.rs:1946`); all four
routes converge there and parity is asserted in-tree (`src/cli/terminal.rs:694`).

- A question's default is resolved by `render_default` (`src/interview.rs:517`),
  which consults the **seed default bank first** (`src/interview.rs:525`) and
  only then evaluates the template's own `default` expression. Provenance is
  tracked as `PreparedDefaultSource::{Template, Configured, Seed}`
  (`src/interview.rs:292`). The bank entry enum is
  `DefaultBankEntry::{Seed, Configured}` (`src/interview.rs:53`): **today a Seed
  default and a Configured default cannot both occupy one id.**
- `Seed { now, defaults, context }` (`src/interview.rs:38`);
  `defaults: IndexMap<Id, RawAnswer>` where `RawAnswer(Value)` is raw, pre-parse,
  pre-validate JSON (`src/interview.rs:36`). Only the crate path and
  `replay_with_defaults` construct a `Seed` directly.
- Configured defaults enter through
  `configured_defaults(formal_name, template, presets, mappings) -> Resolution`
  (`src/interview.rs:1073`), keyed strictly on the **formal name**.
  `Resolution { defaults, warnings }` (`src/interview.rs:956`) carries origin
  provenance; `Resolution::start_with_context` (`src/interview.rs:1042`) moves
  each default into `DefaultBankEntry::Configured`, and
  `Resolution::into_flat_defaults` (`src/interview.rs:1062`) flattens to
  `(IndexMap<Id, RawAnswer>, warnings)` — the exact `Seed.defaults` shape.
- **Raw submissions** are recorded as `Vec<IndexMap<String, Value>>`, one entry
  per accepted batch, pre-`format` (`src/staging.rs:31`; captured at
  `src/cli/terminal.rs:262`, `src/protocol.rs:558`). A looped question's values
  live as one JSON array under one id. Replay re-derives state by re-running
  `Pending::answer` per submission (`src/staging.rs:428`), so nothing formatted
  is ever stored and the new application's own `format`/`validate`/`when` re-run.

`StagedRecord` (`src/staging.rs:23`) holds `target`, `template` (formal name),
`commit`, `named`, `now`, `submissions`, and a versioned `context`. **It has no
`base`/`reanswer` member in this base commit** — that is design 1066's proposed
addition, implemented in the concurrent task 1029, and must not be assumed here.

### CLI surface and the result/exit contract

The command surface is fixed by `docs/specifications/command-line-interface.yml`
and `.spec.yml`, mirrored by `enum Command` (`src/main.rs:193`). The exit-code
vocabulary is load-bearing and shared across verbs:

- `0` success (or dry-run plan, or flow stop/abort);
- `1` template load / file conflict / render or hook failure / staged-record
  mismatch;
- `2` invalid command line;
- `3` template has hooks and is not trusted (`planned`, `trusted: false`);
- `4` questions remain / answers rejected (the batch is returned);
- `5` the template name is ambiguous (a short-name collision).

`--json` appears only on `templates list`; the scripted machine route is
selected by `--answers FILE`, not `--json`, and emits exactly one result
document (`applied`/`planned`/`questions`/`error`/`ended`). Template resolution
is target-independent: `resolve_template` and `formal_name`
(`src/cli/resolve.rs:191`, `:236`) classify `<TEMPLATE>` (git address / folder /
installed-or-alias-or-short name / bundled `toha-demo`) once, and ambiguity
(exit 5) is purely a short-name collision in `registry.resolve`
(`src/registry.rs:567`).

### Configured presets / template-defaults (approved predecessor)

`Config` holds `presets: IndexMap<PresetName, ConfigEntry<Value>>` and
`template_defaults: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>`
(`src/config.rs:63`), keyed by **formal name**, each field a
`DefaultSource::{Ref(PresetName), Literal(Value)}`. Reuse never comes from a
shared question id; a `{ preset: <name> }` reference vs a literal is decided
once at the boundary. Because the key is the formal name, a generator applying
one template into N subpaths gets **identical** configured defaults for every
subpath — per-subpath variation must come from answers, not config.

### Template format — no generator concept exists

The `template.yml` schema is a **closed set of 11 keys**
(`docs/specifications/template-format.schema.yml:12`, `required: [name]`,
`additionalProperties: false`): `name`, `description`, `source`, `data`,
`interview`, `files`, `inject`, `ignore`, `static`, `hooks`, `messages`
(struct `src/template.rs:46`). There is **no** `kind`/type discriminator, no
`generator`/sub-template key, no multiple entrypoints (one `source`
subdirectory, one interview list), and no "repeatable" marker. The only
repetition primitives — `files[].each` (`src/template.rs:290`) and
`hooks[].each` — repeat *within one apply*. No bundled example is a generator in
the "apply repeatedly into a subpath" sense; `generated-files` fans out via
`files.each`, and `injection` edits an existing project in place.

## The snapshot producer contract (consumed, approved 1066 rev 3 + 1110)

The generator design **consumes** the approved project-updates design; its
implementation is the concurrent task 1029 and need not exist. Relevant surface
(from `docs/technical-designs/second-release/project-updates/design.md`):

```rust
pub struct Project { /* repository, canonical target, repo-relative path */ }
impl Project {
    pub fn open(target: &CanonicalTarget) -> Result<Option<Project>, ProjectError>; // None: not in git
    pub fn cleanliness(&self) -> Result<Cleanliness, ProjectError>;
    pub fn snapshots(&self) -> Result<Vec<Listed>, ProjectError>;   // Valid(Snapshot) | Invalid{ref, reason}
    pub fn find(&self, prefix: &str) -> Result<Snapshot, SnapshotError>;
    pub fn likely_bases(&self, snapshots: &[Snapshot]) -> Result<Vec<LikelyBase>, ProjectError>;
}
impl Snapshot {
    pub fn id(&self) -> &SnapshotId;
    pub fn template(&self) -> &str;    // formal name as applied, including @reference
    pub fn source(&self) -> &str;      // formal name without @reference — the template identity
    pub fn revision(&self) -> &Revision;
    pub fn target(&self) -> &RepoPath; // target directory relative to the repository root
    pub fn created(&self) -> Timestamp;
    pub fn generated(&self) -> &FrozenNow;
    pub fn project(&self) -> &ProjectPoint;
    pub fn built_from(&self) -> Option<&SnapshotId>;
    pub fn submissions(&self) -> &[IndexMap<Id, serde_json::Value>]; // raw accepted submissions, pre-format
}
```

Load-bearing facts a generator must respect:

- A snapshot exists **only** for a git target whose target directory was clean
  when the apply ran. Absence is normal for a non-git target, a dirty target,
  and a fresh clone before `toha init` + fetch. A generator must behave when
  snapshots are absent.
- `source` is the template identity (formal name without `@reference`). Two
  snapshots belong to the same template when their `source` values are equal.
- `target` is the subpath (relative to the repository root) the snapshot was
  applied into. Several applications of one generator into several subpaths are
  several snapshots with equal `source` and different `target`.
- `submissions` is exactly the raw, pre-`format` answer shape the interview
  engine records and replays — so a prior snapshot's submissions project
  straight onto `Seed.defaults` / the default bank, and the new application's
  own validation re-runs on them.
- `apply TEMPLATE PATH --from SNAPSHOT` (project-updates) is an **update in
  place**: it re-renders into the **same** target and three-way merges. That is
  a different axis from a generator, which creates a **new** application, usually
  in a **new** subpath, that may *default its answers from* a snapshot taken
  elsewhere. `--from` and its replay adapter, snapshot capture, the merge, and
  the `snapshots` commands are owned by task 1029; this design must not edit
  those surfaces — only consume the reader contract above.

## Constraints (why): Preserve / Change / Avoid / Risk

**Preserve**

- The pure interview engine and `Seed.defaults` contract. Every adapter
  (terminal, headless, staged, agent, crate) drives the same engine with the
  same results. A generator reuses the spine per subpath; it does not fork the
  engine.
- Snapshots are read-only *data that can be refused, never instructions*. A
  generator reads a snapshot's `source`, `target`, and `submissions`; it never
  re-defines capture, the update merge, or `--from`.
- Formal-name identity, the exit-code vocabulary, and the `--answers`-selected
  scripted route. A generator slots into the existing 0–5 contract, not a new
  one.
- The presets / template-defaults contract, including "never infer semantic
  identity from a shared question id."

**Change (candidate design space)**

- How a generator application is *invoked* (a flag on `apply`, a new verb, a
  template-declared concept, or a purely conventional pattern) and how a prior
  snapshot is *selected* as the default source (explicit id, "latest of this
  source", a person prompt, or none).
- How a snapshot's `submissions` are layered against configured
  presets/template-defaults and the template's own defaults into a single,
  total precedence — which may require extending `DefaultBankEntry` to carry an
  ordered fallback rather than a single occupant per id.
- What per-application "identity" a generator needs beyond the target path, and
  whether it is derived (target subpath + source) or declared.

**Avoid**

- A second persisted-answers lifecycle parallel to snapshots/staging (task
  forbids it). Prior-application answers come from snapshots (git targets) or
  configured defaults (everywhere); nothing new is persisted into the project.
- Implicit answer reuse by matching question ids across templates or
  applications.
- Editing sibling surfaces owned by 1029 (snapshot capture, replay adapter,
  `--from`/`--baseline`, `snapshots` commands) or making the generator require
  1029's runtime to exist to be *designed*.
- Inventing a `template.yml` `kind` unless a candidate shows real, gradeable
  value; the schema is closed and the "one source, one interview" assumption is
  baked into load.

**Risk**

- Precedence ambiguity when several snapshots share a `source` (which one
  defaults the next application?) — must be total and deterministic, with clear
  collision/ambiguity behavior and a clear failure when a required reference is
  absent or ambiguous.
- Conflating the update axis (`--from`, same target) with the generate axis
  (new subpath, answers defaulted from elsewhere): reusing `--from` for both
  would overload a flag whose contract is "re-render and merge in place."
- Scripted/agent determinism: a person may be prompted to pick a snapshot, but
  the script and agent routes must resolve selection without interaction and
  fail clearly when they cannot.
- Non-git and dirty targets have no snapshots; the generator must still apply,
  falling back to configured defaults or normal questions, without pretending a
  snapshot existed.
