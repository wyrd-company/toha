# Candidate 1 — Reserved-name resolution branch + on-demand materialization behind one seam

**Direction A.** All embedding lives behind one deep seam in the CLI resolution
layer: a new `src/cli/bundled.rs` plus two branches inside the existing
`resolve_template` / `resume_template` (and a one-line fallback in
`formal_name`). The pure engine, `Plan`, `apply`, `protocol`, and `staging`
learn nothing new. The word "embedded"/"bundled" never crosses out of
`src/cli/`.

---

## 1. Caller's usage first

### README / quickstart (the spec)

```text
Try toha without installing a template. The demo ships inside the binary and
runs offline from any directory on first run — no registry entry, no cache
warm-up, no network, no git.

    # Generate straight away
    toha apply toha-demo ./my-notes

    # Preview only; write nothing (there is no `preview` subcommand)
    toha apply --dry-run toha-demo ./my-notes

    # Interview now, generate later — even in a separate shell, offline
    toha stage toha-demo ./my-notes
    toha continue ./my-notes            # or: toha apply ./my-notes

`toha-demo` is a reserved name. If you install or alias a template as
`toha-demo`, YOUR template wins; the bundled demo only answers when nothing
else does.
```

### Call site A — first-run apply (terminal), clean machine

```console
$ cd /tmp/scratch                 # arbitrary cwd, empty registry, no network
$ toha apply toha-demo ./out
Note title: Weekly sync
Topic: planning
create note.txt
$ cat ./out/note.txt
Weekly sync
Topic: planning
```

### Call site B — headless preview, then headless apply (agent/script)

```console
$ printf '{"title":"Weekly sync","topic":"planning"}' > answers.json
$ toha apply --dry-run --answers answers.json toha-demo ./out
create note.txt
$ toha apply --answers answers.json toha-demo ./out
create note.txt
```

### Call site C — stage in one process, resume in another (offline, identical output)

```console
$ toha stage toha-demo ./out --async batch.json   # process 1: emits first batch
$ cat batch.json
{ "context": { "template": "toha-demo",
               "commit": "3f2c…<64-hex sha256 of the embedded demo tree>" },
  "questions": [ /* title, topic */ ] }

# ...hours later, different shell, still offline, no git, no registry entry...
$ printf '{"title":"Weekly sync","topic":"planning"}' > answers.json
$ toha continue ./out answers.json                 # process 2
{ "context": { "template": "toha-demo", "commit": "3f2c…" }, "answers": { … } }
$ toha apply ./out                                 # process 3: writes files
create note.txt
```

The caller never types anything but `toha-demo`. Nothing in the caller's view
mentions "embedded", a cache path, or a synthetic address. The bundled demo is
indistinguishable from a by-name template except that it needs no setup.

---

## 2. Shape derived from that usage

### 2.1 The unchanged seam

The only value that flows from resolution into the rest of the binary is
`ResolvedTemplate`. It is **not extended** — no new field, no new variant:

```rust
// src/cli/resolve.rs  (UNCHANGED struct)
pub struct ResolvedTemplate {
    pub formal_name: String, // stable selected-template identity
    pub commit: String,      // content version of that identity
    pub folder: PathBuf,     // a REAL directory on disk
    pub trusted: bool,       // registry trust for hooks
    pub named: bool,         // came from a registry name
}
```

Every downstream caller (`stage`, `run`, `continue_run`, `progress`,
`load_template`, and designs 1073/1075) keeps consuming these five fields with
no knowledge of where the folder came from. `Template::load(&resolved.folder)`
canonicalizes and walks the folder exactly as today — which is why the bundled
demo **must be materialized to a real folder** (grounding's load-bearing
constraint: `Plan::build` walks `source_dir`, `apply` re-reads static files and
per-file permissions, hooks run from `root`).

### 2.2 The new module (the whole seam)

```rust
// src/cli/bundled.rs  (NEW — the only file that knows the demo is embedded)
use crate::Dirs;
use super::resolve::{ResolveError, ResolvedTemplate};
use include_dir::{Dir, include_dir};
use std::{path::PathBuf, sync::LazyLock};

/// The one reserved resolution name for the binary-embedded demo.
///
/// This is a *resolution* identity, distinct from the demo template's own
/// short name (`name: demo` in its `template.yml`). Reserving the token here
/// costs no existing capability: `templates add` derives formal keys from
/// addresses and never produces the bare token `toha-demo`, and an alias or
/// short name `toha-demo` resolves to *its own* formal key, never to this one.
pub const RESERVED_NAME: &str = "toha-demo";

/// The single maintained source. `include_dir!` reads these bytes at compile
/// time straight from `docs/examples/demo/`, so the "embedded copy" is not a
/// copy: it is the source. Drift between source and embedded tree is
/// impossible by construction (per encode-lessons-in-structure). The crate
/// tarball already ships `docs/examples/demo/**` (Cargo `include`), so the
/// macro resolves both from the repo and from a published-crate build.
static DEMO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo");

/// Content version of the embedded tree: lowercase sha256 hex (64 chars) over a
/// canonical manifest of `(root-relative path, length, bytes)` for every file
/// in sorted path order. Filesystem-safe as a directory component on every
/// platform (no `:` prefix — bundled identity is keyed by `formal_name`, not by
/// commit shape). Changes iff the demo content changes; a toha version bump
/// that leaves the demo untouched keeps the same commit, so staged interviews
/// still resume.
pub fn commit() -> &'static str {
    static COMMIT: LazyLock<String> = LazyLock::new(|| digest(&DEMO));
    &COMMIT
}

/// Whether a name that failed registry resolution is the reserved demo.
pub fn is_reserved(name: &str) -> bool { name == RESERVED_NAME }

/// Resolve the reserved demo on first run: materialize + describe.
///
/// Invariants:
/// - `formal_name == RESERVED_NAME`, `commit == commit()`.
/// - `folder` is a real, complete, immutable directory identical to
///   `docs/examples/demo/`.
/// - `trusted == false`, `named == false` (untrusted, like a folder/by-address
///   source; the demo has no hooks, so trust never gates its apply).
pub fn resolve(dirs: &Dirs) -> Result<ResolvedTemplate, ResolveError> {
    let folder = materialize(dirs)?;
    Ok(ResolvedTemplate {
        formal_name: RESERVED_NAME.into(),
        commit: commit().into(),
        folder,
        trusted: false,
        named: false,
    })
}

/// Reconstruct the reserved demo from staged identity alone, offline.
///
/// `staged_commit` is `StagedRecord.commit`. When it matches the running
/// build's `commit()`, re-materialize idempotently (same content-addressed
/// folder) and return an identity equal to the one `resolve` produced. When it
/// does not match, the running binary embeds a *different* demo and cannot
/// reproduce the staged bytes: return a clear, offline error so the caller
/// starts over. This mirrors the existing git guard
/// (`fetched.commit != commit`) — a content match, not a version gate.
pub fn resume(staged_commit: &str, dirs: &Dirs) -> Result<ResolvedTemplate, ResolveError> {
    if staged_commit != commit() {
        return Err(ResolveError::Error(format!(
            "the bundled {RESERVED_NAME} changed since this interview was staged \
             (staged {staged_commit}, this build {}); \
             abort and stage {RESERVED_NAME} again",
            commit()
        )));
    }
    resolve(dirs)
}

/// Content-address the embedded tree into the cache and return its folder.
///
/// Location: `dirs.cache/sources/<install_key(RESERVED_NAME)>/<commit()>/`
/// i.e. `~/.cache/toha/sources/toha-demo/<64-hex>/`. This is the *same*
/// namespace git fetches already populate (`cached`, `fetch_new`), so it needs
/// no new capability or permission — only bytes written where toha already
/// writes template sources. Content-addressed ⇒ the slot is immutable; a
/// present, complete slot is reused across every process of one build.
///
/// Idempotent + race-safe (per make-operations-idempotent): fill a sibling
/// tempdir, then atomic `fs::rename` into `<commit>`. If the slot already
/// exists, discard the tempdir and reuse it — either result is byte-identical.
fn materialize(dirs: &Dirs) -> Result<PathBuf, ResolveError> {
    // not implemented
    // 1. root = dirs.cache/"sources"/source::install_key(RESERVED_NAME)/commit()
    // 2. if root.is_dir() { return Ok(root) }                 // reuse
    // 3. create_dir_all(root.parent())
    // 4. tmp = tempfile::tempdir_in(root.parent())
    // 5. for file in walk(&DEMO): create parents under tmp, create_new, write bytes
    //    (the skills::export write mechanics; blocking checks are unneeded —
    //     the tempdir is empty and ours)
    // 6. if !root.exists() { fs::rename(tmp, &root) } else { drop(tmp) }   // race
    // 7. Ok(root)
    unimplemented!()
}

/// sha256 hex over the canonical manifest of an embedded dir.
fn digest(_dir: &Dir<'_>) -> String {
    // not implemented — sort files by root-relative path; for each feed
    // path bytes, 0x00, (len as u64 LE), contents into Sha256; hex-encode.
    unimplemented!()
}
```

### 2.3 The two resolution branches (all other changes)

```rust
// src/cli/resolve.rs — inside resolve_template, the Address::Name arm.
// Today: `let resolved = registry.resolve(name)?; Ok(entry(...))`.
// The registry is consulted UNCHANGED and still wins on any match. The demo
// only fills the exact gap the registry leaves — a NotFound for the reserved
// name.
Address::Name(name) => match registry.resolve(name) {
    Ok(resolved) => Ok(entry(resolved.formal_name, registry).expect("resolved registry entry")),
    Err(registry::ResolveError::NotFound(n)) if bundled::is_reserved(&n) => bundled::resolve(dirs),
    Err(e) => Err(e.into()),   // NotFound(other) and Ambiguous propagate unchanged
}
```

```rust
// src/cli/resolve.rs — inside formal_name(), the Address::Name arm.
// Keeps `apply toha-demo <dir>` able to recognize its OWN staged interview:
// staged_refusal() compares this against StagedRecord.template ("toha-demo").
Address::Name(name) => match registry.resolve(name) {
    Ok(resolved) => resolved.formal_name,
    Err(registry::ResolveError::NotFound(n)) if bundled::is_reserved(&n) => n,  // "toha-demo"
    Err(e) => return Err(e.into()),
}
```

```rust
// src/cli/resolve.rs — top of resume_template(), before the empty-commit path.
// Without this the current code parses "toha-demo" to Address::Name and dies at
// the `let Address::Git .. else` guard, so staged bundled interviews could
// never resume. The bundled record is the ONLY producer of
// `StagedRecord.template == "toha-demo"` (installed/aliased toha-demo stage
// under their own address-derived formal key), so this branch is unambiguous.
if bundled::is_reserved(formal) {
    return bundled::resume(commit, dirs);
}
```

### 2.4 Module / seam map

```text
                         caller types "toha-demo"
                                   │
        main.rs (stage / run / continue_run / progress / staged_refusal)
                                   │  arg: &str
                                   ▼
        ┌───────────────────── src/cli/resolve.rs ──────────────────────┐
        │ source::parse(arg) → Address::Name("toha-demo")                │
        │                                                                │
        │ resolve_template  Name arm:                                    │
        │   registry.resolve(name) ── Ok ─────────► existing entry(...)  │  (unchanged
        │        │                                                       │   outcomes)
        │        └─ Err(NotFound & reserved) ─┐                          │
        │ resume_template  top:               │                          │
        │   is_reserved(formal) ──────────────┤                          │
        │ formal_name  Name arm:              │                          │
        │   Err(NotFound & reserved) ─────────┤                          │
        └─────────────────────────────────────┼──────────────────────────┘
                                              ▼
                                   src/cli/bundled.rs        ← only "embedded" lives here
                                   include_dir!(docs/examples/demo)
                                   commit() = sha256(tree)
                                   materialize() → cache/sources/toha-demo/<commit>/
                                              │
                                              ▼  ResolvedTemplate { folder = real dir }
        ─────────────────── UNCHANGED FROM HERE DOWN ───────────────────
        Template::load(folder) → Interview (pure) → Plan::build(folder)
        → apply_reporting → protocol / staging  (never hear the word "bundled")
```

Trace length: caller → `resolve.rs` → `bundled.rs` → back to `resolve.rs`. Three
files, one of them new; the flow below the seam is byte-for-byte the existing
path (per minimize-reader-load).

---

## 3. Resolution precedence rule (explicit)

For an argument that classifies as `Name(n)` (the `<TEMPLATE>` order — git URL →
`host:` → folder → **name** — is untouched; a bare `toha-demo` is always a
`Name`):

1. `registry.resolve(n)` runs first and **always wins on any match**: alias →
   short name → formal key, in that existing order and with the existing
   ambiguity behaviour.
2. Only if step 1 returns `NotFound` **and** `n == "toha-demo"` does the bundled
   demo resolve.
3. Any other `NotFound` and every `Ambiguous` propagate exactly as today.

**Recommended: fallback-only (registry wins).** It preserves *every* current
name-resolution outcome — an installed, aliased, or discovered `toha-demo` keeps
resolving to itself, because the bundled branch is reachable only on the precise
input that fails today (`NotFound("toha-demo")`). No supported resolution is
narrowed, so no separate approval is needed.

**Rejected option — reserved-name-wins (bundled shadows the registry).** This
would change the meaning of an already-installed/aliased `toha-demo`, i.e.
narrow supported resolution → a product decision requiring explicit approval.
Not recommended: it trades a real, supported capability for a marginal
guarantee that a name always means the demo.

---

## 4. Selected-template identity scheme (the coordination surface)

The identity carried in `ResolvedTemplate`, `StagedRecord`, and the protocol
`Context`, and consumed by designs 1073 (defaults) and 1075 (Jinja context):

| field | bundled value | meaning / role |
|-------|---------------|----------------|
| `formal_name` | `"toha-demo"` | **Stable identity.** Never changes across builds. The key downstream designs attach configuration/defaults/context to. Reserved: address-derived registry keys never equal it. |
| `commit` | `sha256` hex (64) of the embedded tree | **Content version.** Changes iff the demo content changes. Drives resume determinism and the content-addressed cache slot. Filesystem-safe as a path component on all platforms, and a valid 64-hex value under the registry schema's `commit` pattern `^[0-9a-f]{40}([0-9a-f]{24})?$` — so the identity stays uniform with git-backed commits even though the bundled demo is never written to a registry. |
| `folder`/`root` | `~/.cache/toha/sources/toha-demo/<commit>/` | Real, immutable, per-content directory. |

**Reconstructable offline from staged identity alone:** `resume_template` needs
only `("toha-demo", <commit>)` from the `StagedRecord`. `is_reserved` recognizes
the name; `bundled::resume` compares the staged commit to the running build's
`commit()` and re-materializes the same content-addressed folder — no registry,
no network, no filesystem state beyond the cache it rebuilds itself.

**Why a content digest and not `TOHA_VERSION`:** the digest ties the identity to
what actually determines output (the demo bytes). A patch release that does not
touch the demo keeps the same commit, so already-staged interviews keep
resuming; a change to the demo correctly invalidates them. Using the version
would break resumes on every release and keep colliding identities during
development when the version has not yet bumped. The digest is a *content match*,
identical in spirit to git's existing `fetched.commit != commit` guard — **not a
pinned-version check** of the kind that needs approval (it gates nothing on the
toha version).

---

## 5. Error / results contract

All errors use the existing `ResolveError` (`Error(String)` /
`Ambiguous{..}`) — no new error type, no new exit code.

| Situation | Result |
|-----------|--------|
| `toha-demo` unresolved by registry, first run | `Ok(ResolvedTemplate)` for the bundled demo (materialized). |
| Installed/aliased/discovered `toha-demo` exists | Registry entry returned; bundled never runs (fallback-only). |
| Other unknown name | `ResolveError::Error("template not found: <name>")` — unchanged. |
| Ambiguous name | `ResolveError::Ambiguous{..}` — unchanged. |
| Resume, staged commit == build commit | `Ok(ResolvedTemplate)`, re-materialized offline. |
| Resume, staged commit != build commit | `ResolveError::Error("the bundled toha-demo changed since this interview was staged … stage toha-demo again")`. Surfaces via the existing `resolve_error` → `Outcome::Error`. |
| Materialization I/O failure | `ResolveError::Error(<io message>)` — same shape as `cached`/`fetch_new` cache errors. |
| Concurrent materialization race | Both processes succeed; loser reuses the winner's byte-identical slot. |

---

## 6. Verification & synchronization

1. **Single source, no drift (structural).** `include_dir!` embeds
   `docs/examples/demo/` directly; there is no second in-repo copy to drift.
   Propose adding `println!("cargo:rerun-if-changed=docs/examples/demo");` to
   `build.rs` so edits to the source force a rebuild of the embed. (Design note;
   not committed here.)
2. **Source ↔ materialized tree (falsifiable test).** A test materializes the
   bundled demo to a tempdir and asserts the tree is byte-identical, path for
   path, to `docs/examples/demo/` located via `env!("CARGO_MANIFEST_DIR")`. Edit
   one without the other → the test fails.
3. **Demo ↔ documented output (falsifiable fixture test).** An end-to-end test
   runs `apply --answers <fixed> toha-demo <tmp>` and compares the result tree
   to an expected tree under `tests/fixtures/`. Ties the demo content to the
   README/quickstart output shown in §1.
4. **Content-identity guard (optional, cheap).** A test asserts `commit()`
   equals a checked-in expected 64-hex value, so any silent change to the demo
   bytes is caught and forces an explicit fixture/commit update.
5. **Package verification.** `docs/examples/demo/**` is already in Cargo
   `include`, so the tarball ships the bytes the macro reads; a
   `cargo package --list` check (or the existing publish CI) confirms presence.

---

## 7. All three first-run paths + cross-process resume (walked)

- **`apply toha-demo <dir>`** — `run()` → `resolve_template` Name arm → registry
  NotFound → `bundled::resolve` materializes → `Template::load` → interview →
  `Plan::build` (walks the materialized folder) → `apply_reporting` writes.
  Offline, any cwd.
- **`apply --dry-run toha-demo <dir>`** — same up to `Plan::build`; `run()`'s
  `dry_run` arm prints plan lines and writes nothing. Materialization is still
  required because `Plan::build` reads the folder; it happens in resolve, so the
  preview is offline. No `preview` subcommand introduced.
- **`stage toha-demo <dir>` then `continue`/`apply <PATH>` (separate process)** —
  `stage()` resolves via `bundled::resolve`, records `StagedRecord{ template:
  "toha-demo", commit: <digest>, named: false, submissions }`. A later process
  loads that record and calls `resume_template("toha-demo", <digest>, false,
  …)` → top branch → `bundled::resume` → same content-addressed folder →
  `replay_with_defaults` produces identical submissions → identical files.
  Offline, no registry/network/git.

## 8. What deliberately does not change

The pure interview engine, `Plan`, `apply`, `protocol`, `staging`, the
`ResolvedTemplate` shape, the `<TEMPLATE>` classification order, the trust
boundary (bundled is untrusted; its hookless demo needs no trust), path safety,
and no-overwrite-without-`--force`. The library crate (`toha::`) is untouched —
bundled resolution is a binary/CLI concern gated behind the existing `cli`
feature that already owns `include_dir`.

**No registry entry is created.** The bundled demo is never written into any
`templates.yml` layer. This sidesteps the registry schema entirely: no synthetic
entry to satisfy `required: [name, source, path]`, no fabricated `source`/`path`
(which the schema accepts as free strings but whose documented meaning —
repository URL or canonical absolute path — the bundled demo cannot honestly
supply), and no `additionalProperties:false` violation from an `embedded:`
marker. The registry remains a truthful record of *installed* templates only.
