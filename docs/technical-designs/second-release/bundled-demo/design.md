# Bundled offline toha-demo — design

Make the small inspectable demo template resolvable and runnable **by name**,
**offline**, from **any directory**, on **first run** — no registry entry, no
cache warm-up, no network, no git. All embedding lives behind one deep seam in the
CLI resolution layer (`src/cli/bundled.rs`); the pure interview engine,
`Template`, `Plan`, `apply`, `protocol`, and `staging` learn nothing new, and
`ResolvedTemplate` gains no field or variant.

This is the synthesized design. Base: Candidate 1 (Direction A). Grafts from
Candidate 3 (the resume seam, the canonical digest) and Candidate 2
(discoverability), recorded in `synthesis.md`. Evidence: `grounding.md`,
`rubric.md`, `candidates/`, `cross-judge.md`, `verification.md`.

## Caller's usage (the spec)

```text
Try toha without installing a template. The demo ships inside the binary and runs
offline from any directory on first run — no registry entry, no cache, no network,
no git.

    toha apply toha-demo ./my-notes            # generate now
    toha apply --dry-run toha-demo ./my-notes  # preview only (no `preview` subcommand)
    toha stage toha-demo ./my-notes            # interview now…
    toha continue ./my-notes                   # …resume later, another shell, offline
                                               # (or: toha apply ./my-notes)

`toha-demo` is a reserved fallback name. If you install or alias a template as
`toha-demo`, YOUR template wins; the bundled demo only answers when nothing else
does.
```

Concrete call sites:

```console
# A — first-run apply, arbitrary cwd, empty registry, no network
$ cd /tmp/scratch
$ toha apply toha-demo ./out
Note title: Sample Title
Topic: Sample Topic
create note.txt

# B — headless preview then headless apply
$ printf '{"title":"Sample Title","topic":"Sample Topic"}' > answers.json
$ toha apply --dry-run --answers answers.json toha-demo ./out
create note.txt
$ toha apply --answers answers.json toha-demo ./out
create note.txt

# C — stage in one process, resume in another, offline, identical output
$ toha stage toha-demo ./out --async batch.json     # process 1
$ toha continue ./out answers.json                  # process 2 (separate, offline)
$ toha apply ./out                                  # process 3: writes files
create note.txt
```

The caller types only `toha-demo`. Nothing in the caller's view mentions
"embedded", a cache path, or a synthetic address. Crate callers of the `toha`
library see no new surface — bundling is a binary/CLI concern behind the existing
`cli` feature that already owns `include_dir`.

## Module and seam map

```text
                         caller types "toha-demo"
                                   │
        main.rs (stage / run / continue_run / progress / staged_refusal)
                                   │  arg: &str
                                   ▼
        ┌───────────────────── src/cli/resolve.rs ──────────────────────┐
        │ source::parse(arg) → Address::Name("toha-demo")   (order unchanged)
        │ resolve_template  Name arm:                                    │
        │   registry.resolve(name) ── Ok ─────────► existing entry(...)  │ (unchanged
        │        └─ Err(NotFound & reserved) ─┐                          │  outcomes)
        │ resume_template  before git-parse:  │                          │
        │   bundled::resume(formal,commit)? ──┤                          │
        │ formal_name  Name arm:              │                          │
        │   Err(NotFound & reserved) ─────────┤                          │
        └─────────────────────────────────────┼──────────────────────────┘
                                              ▼
                                   src/cli/bundled.rs      ← only "embedded" lives here
                                   include_dir!(docs/examples/demo)
                                   commit() = sha256(canonical tree)
                                   resolve()/resume() → materialize()
                                   → cache/sources/toha-demo/<commit>/
                                              │
                                              ▼  ResolvedTemplate { folder = real dir }
        ─────────────────── UNCHANGED FROM HERE DOWN ───────────────────
        Template::load(folder) → Interview (pure) → Plan::build(folder)
        → apply_reporting → protocol / staging   (never hear the word "bundled")

        src/cli/templates.rs (listing): appends one read-only bundled row
                                        (presentation only; not a registry entry)  [D3]
```

Trace: caller → `resolve.rs` → `bundled.rs` → back to the unchanged pipeline.
Three files, one new; the flow below the seam is byte-for-byte the existing path.

## Public interfaces expected (signatures)

New module `src/cli/bundled.rs` (behind the `cli` feature, sibling of `skills.rs`):

```rust
/// The one reserved resolution token and the demo's stable formal name.
/// Distinct from the demo template's own short name (`name: demo`).
pub const RESERVED: &str = "toha-demo";

/// The single maintained source, embedded at compile time. `include_dir!` reads
/// `docs/examples/demo/` directly, so the embedded copy IS the source — no drift.
static DEMO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo");

/// Content version of the embedded tree: lowercase sha256 hex (64 chars), derived
/// once at runtime from the embedded bytes (LazyLock). Changes iff the demo
/// content changes; a toha version bump that leaves the demo untouched keeps the
/// same commit, so staged interviews still resume.
pub fn commit() -> &'static str;

/// Resolve the reserved demo on first run: materialize + describe.
/// Called only after every registry layer returned NotFound for exactly RESERVED.
/// Invariants: formal_name==RESERVED, commit==commit(), folder is a real complete
/// directory identical to docs/examples/demo/, trusted==false, named==false.
pub fn resolve(dirs: &Dirs) -> Result<ResolvedTemplate, ResolveError>;

/// Reconstruct the reserved demo from a staged identity alone, offline. Returns
/// None when the record is not the bundled identity, so resume_template falls
/// through to its existing paths. Some(Err(..)) with a clear "stage again"
/// message when the staged commit != this build's commit.
pub fn resume(formal: &str, commit: &str, dirs: &Dirs)
    -> Option<Result<ResolvedTemplate, ResolveError>>;

/// A read-only descriptor for the discoverability row (see D3). Not a registry
/// entry; never consulted by resolution.
pub fn listing_row() -> BundledRow;   // { formal: "toha-demo", short: "demo", trusted: false }
```

Wiring edits (the entire blast radius outside the new module):

```rust
// src/cli/resolve.rs — resolve_template, Address::Name arm
Address::Name(name) => match registry.resolve(name) {
    Ok(resolved) => Ok(entry(resolved.formal_name, registry).expect("resolved registry entry")),
    Err(registry::ResolveError::NotFound(n)) if n == bundled::RESERVED => bundled::resolve(dirs),
    Err(e) => Err(e.into()),   // NotFound(other) and Ambiguous propagate unchanged
}

// src/cli/resolve.rs — formal_name, Address::Name arm (so `apply toha-demo` finds its own staged run)
Address::Name(name) => match registry.resolve(name) {
    Ok(resolved) => resolved.formal_name,
    Err(registry::ResolveError::NotFound(n)) if n == bundled::RESERVED => n,
    Err(e) => return Err(e.into()),
}

// src/cli/resolve.rs — resume_template, immediately before the git-parse fallback
if let Some(result) = bundled::resume(formal, commit, dirs) {
    return result;
}
```

`ResolveError::text` (`src/cli/resolve.rs:31`, promoted to `pub(crate)`) builds
the bundled module's error messages; no new error type, no new exit code.

## Data structures expected

`ResolvedTemplate` is **unchanged** (`src/cli/resolve.rs:17`):

```rust
pub struct ResolvedTemplate {
    pub formal_name: String,  // stable selected-template identity
    pub commit: String,       // content version of that identity
    pub folder: PathBuf,      // a REAL directory on disk
    pub trusted: bool,        // registry trust for hooks
    pub named: bool,          // came from a registry name
}
```

The bundled identity (the shared coordination surface for 1073/1075):

| field | bundled value | role |
|---|---|---|
| `formal_name` | `"toha-demo"` | Stable identity; never changes across builds. Reserved: address-derived registry keys never equal it. |
| `commit` | sha256 hex (64) of the embedded tree | Content version; drives resume determinism and the content-addressed cache slot. Satisfies the registry commit pattern `^[0-9a-f]{40}([0-9a-f]{24})?$`. |
| `folder` | `<cache>/sources/toha-demo/<commit>/` | Real, immutable, per-content directory (git's own source-cache namespace). |
| `trusted` | `false` | Untrusted like a folder/by-address source; the hookless demo never needs trust. |
| `named` | `false` | No registry entry backs it, so it can never be registry-trusted. |

Canonical digest: files sorted by root-relative path; for each file feed
`len(path)‖path‖len(bytes)‖bytes` (lengths as `u64` LE) into sha256. No registry
entry is created; no `templates.yml` layer is written.

## Resolution precedence rule (explicit)

For an argument that classifies as `Name(n)` (the `<TEMPLATE>` order — git URL →
`host:` → folder → alias → short → formal → **bundled reserved name** — is
untouched; a bare `toha-demo` is always a `Name`):

1. `registry.resolve(n)` runs first and **always wins on any match**: alias →
   short name → formal key, with the existing ambiguity behaviour.
2. Only if step 1 returns `NotFound` **and** `n == "toha-demo"` does the bundled
   demo resolve.
3. Any other `NotFound` and every `Ambiguous` propagate exactly as today.

**Recommended: fallback-only.** It preserves every current name-resolution
outcome — an installed, aliased, or discovered `toha-demo` keeps resolving to
itself, because the bundled branch is reachable only on the precise input that
fails today. No supported resolution is narrowed, so no approval is required.
(Bare `demo` is unaffected — only the literal token `toha-demo` triggers the
fallback.) Reserved-name-wins is decision **D1**.

## Behaviors to prove (falsifiable)

- **Offline first run** — `demo_applies_offline`: isolated HOME/XDG, empty cache,
  no network, arbitrary cwd; `apply toha-demo <tmp> --answers <doc>` yields the
  fixture tree (`note.txt` == `"Sample Title\nTopic: Sample Topic\n"`). Fails if
  resolution, materialization, or rendering regresses.
- **Cross-process resume** — a companion case stages in one process and
  continues/applies in a second, offline; asserts byte-identical output. Fails if
  identity reconstruction is not offline or not deterministic.
- **Collision preserved** — a seeded registry `toha-demo` (entry / alias / short)
  resolves to the registry, bundled untouched. Fails if the fallback ever
  shadows a real template.
- **Resume guard** — `bundled::resume` with a mismatched commit returns the clear
  "stage again" error (not silent different output). Fails if the guard is
  removed.
- **Single source, no drift** — `embed_equals_source` (materialized tree byte-
  identical to `docs/examples/demo/`) and `commit_derives_from_source` (recomputed
  digest == `commit()`). Fail on any drift between source, embed, and identity.

## Verification & synchronization

`include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo")` makes the embedded copy
the source — no second in-repo copy to drift. The commit is derived at runtime
from the embedded bytes, so it cannot go stale relative to the embed. Proposed
(described, not applied here): `cargo:rerun-if-changed=docs/examples/demo` in
`build.rs`. Package verification is already covered: `docs/examples/demo/**` is in
the Cargo `include` set, so the tarball ships the bytes the macro reads.

Materialization reuses the git source-cache pattern: fill a sibling tempdir with
create-new writes (the `skills::export` mechanics), then atomic `fs::rename` into
`<commit>`; an existing content-addressed slot is reused. Idempotent and race-safe
across processes; writes only where toha already writes template sources — no new
capability, permission, timeout, pinned-version check, or subprocess.

## Error / results contract

| Situation | Result |
|---|---|
| `toha-demo` unresolved by registry, first run | `Ok(ResolvedTemplate)`, materialized |
| Installed/aliased/discovered `toha-demo` exists | Registry entry; bundled never runs |
| Other unknown name | `template not found: <name>` (unchanged) |
| Ambiguous name | `ResolveError::Ambiguous` (exit 5, unchanged) |
| Resume, staged commit == build commit | `Ok`, re-materialized offline |
| Resume, staged commit != build commit | `Some(Err("the bundled toha-demo changed since this interview was staged … stage toha-demo again"))`, exit 1 |
| Materialization I/O failure | `ResolveError::Error(<io message>)`, same shape as git-cache errors |
| Concurrent materialization race | Both succeed; loser reuses the winner's byte-identical slot |
| Cache dir unwritable | Fails clearly by default; temp-dir degrade is decision **D2** |

Exit codes are the existing pipeline's (0/1/2/3/4/5); the bundled demo introduces
none.

## Canonical-document impact (owned by paired implementation 1059)

Described here, applied by the implementation task — not edited in this design:

- `docs/specifications/command-line-interface.yml`: document `toha-demo` as a
  reserved fallback name in the `<TEMPLATE>` resolution order and the offline
  first-run guarantee.
- `docs/specifications/template-registry.yml`: note that the bundled fallback is
  not a registry entry and appears in unfiltered `templates list` only as a
  read-only row (if D3 = show).
- `docs/technical-designs/architecture.yml`: extend the binary's
  "embedded skills" note to "embedded skills and the bundled demo".
- `README.md`, `docs/getting-started.md`, `docs/overview.md`: the offline
  quickstart above; `docs/demo.tape` / `docs/assets/demo.gif`: refresh to the
  by-name first-run flow.

## Out of scope

The pure interview engine, `Template`, `Plan`, `apply`, `protocol`, `staging`,
the `ResolvedTemplate` shape, the `<TEMPLATE>` classification order, the trust
model, path safety, and no-overwrite-without-`--force`. The `toha` library crate
is untouched. No new subcommand, no `preview` command (`--dry-run` is preview).
No production or specification/schema edits in this design task.

## Decisions needed first (Bob)

- **D1 — Collision precedence:** *fallback-only* (recommended; preserves every
  current outcome, no approval needed) vs *reserved-name-wins* (bundled shadows an
  installed/aliased `toha-demo` — narrows supported resolution, requires explicit
  approval). Recommend fallback-only.
- **D2 — Cache-unwritable behaviour:** *fail with a clear error* (recommended;
  keeps all writes inside the documented cache) vs *degrade to a temp dir* (more
  resilient on read-only/sandboxed cache, but introduces a runtime write location
  outside the documented cache and a temp dir cleaned only by the OS reaper — a
  disclosable capability question). Recommend fail-clearly unless the agent/CI
  read-only-cache scenario is a priority.
- **D3 — Discoverability:** *show one read-only bundled row in unfiltered
  `templates list`* (recommended; supports the "inspectable demo" goal, additive,
  changes no resolution outcome) vs *keep it invisible*. Recommend show.
- **D4 — Downstream identity:** confirm `formal_name="toha-demo"` (stable key) +
  `commit=<content digest>` (content version) is the coordination surface designs
  1073 (defaults) and 1075 (Jinja context) key configuration on.

## Size and complexity

- **Size:** ~M. One new ~150-line module, three small resolution branches, one
  presentation row, and the test suite (two unit + one integration fixture with a
  cross-process variant), plus the canonical-document updates owned by 1059.
- **Complexity:** moderate. The hard parts — content-addressed idempotent
  materialization and offline identity reconstruction — reuse proven patterns
  (`skills::export`, the git source cache, the existing resume path). No new
  concurrency, no new capability, no engine change.
