# Bundled offline toha-demo — Candidate C: embedded-as-source

Direction: materialize the compiled-in demo into the *ordinary* source cache
(`cache/sources/<install_key>/<commit>`) exactly like a git fetch, so the
existing `(formal_name, commit)` resume path re-materializes it offline and the
rest of the pipeline sees a normal resolved folder. One deep seam
(`src/cli/bundled.rs`) hides the fact that the demo is compiled in; resolution
touches it at three small call sites and nothing downstream changes.

---

## 1. Caller's usage (the spec)

### Quickstart (README fragment)

`toha-demo` is a small, inspectable template compiled into `toha`. It needs no
install, no cache, no network, and no git. It works on first run, from any
directory.

```console
$ cd /any/where
$ toha apply toha-demo ./out
Note title: Sample Title
Topic: Sample Topic
create note.txt
$ cat ./out/note.txt
Sample Title
Topic: Sample Topic
```

Preview without writing (there is no `preview` subcommand; `--dry-run` is the
preview path):

```console
$ toha apply --dry-run toha-demo ./out
create note.txt
```

Stage now, finish later in a different process, still offline — byte-identical
output:

```console
$ toha stage toha-demo ./out --async first-batch.json      # emits the first question batch, exit 4
$ toha continue ./out answers.json                          # separate process, offline
$ toha apply ./out                                          # writes ./out/note.txt
```

### Concrete call sites (inside `toha`)

The three edits are the entire blast radius. Everything else in
`resolve_template`, `resume_template`, `run`, `stage`, `continue_run`, and the
whole apply pipeline is unchanged.

**Call site 1 — first-run resolution by name (`src/cli/resolve.rs`,
`resolve_template`, `Address::Name` arm):**

```rust
Address::Name(name) => match registry.resolve(name) {
    Ok(resolved) => Ok(entry(resolved.formal_name, registry).expect("resolved registry entry")),
    // Fallback-only precedence: the bundled demo answers *only* when every
    // registry layer already returned NotFound for exactly this reserved token,
    // so it never shadows an installed / aliased / discovered `toha-demo`.
    Err(registry::ResolveError::NotFound(_)) if name == bundled::RESERVED => bundled::resolve(dirs),
    Err(e) => Err(e.into()),
},
```

**Call site 2 — cross-process resume (`resume_template`), inserted after the
existing registry-entry match and before the git-parse fallback:**

```rust
// A staged bundled-demo record re-materializes from the embedded bytes,
// offline. Returns None (falls through) unless this *is* the bundled identity
// and this binary's demo still has the staged content digest.
if let Some(result) = bundled::resume(formal, commit, dirs) {
    return result;
}
```

**Call site 3 — the name→formal projection used by `staged_refusal`
(`formal_name`):**

```rust
Address::Name(name) => match registry.resolve(name) {
    Ok(r) => r.formal_name,
    Err(registry::ResolveError::NotFound(_)) if name == bundled::RESERVED => bundled::RESERVED.into(),
    Err(e) => return Err(e.into()),
},
```

### Crate callers

Unchanged. `toha-demo` is a binary affordance layered inside `src/cli/`. Crate
callers keep resolving folders/addresses themselves and never see the reserved
name or the word "embedded". The pure engine, `Template`, `Plan`, `apply`,
`protocol`, and `staging` are untouched.

---

## 2. The identity (shared coordination surface for 1073 / 1075)

The selected-template identity a bundled resolve produces, and that a staged
record reconstructs, is exactly:

| field | value |
| --- | --- |
| `formal_name` | `"toha-demo"` — the constant `bundled::RESERVED`. What you type *is* the formal name *is* the staged identity. |
| `commit` | 64-char lowercase-hex SHA-256 over a canonical serialization of every embedded file (see §5). Non-empty, stable per demo content. |
| `folder` (root) | `<cache>/sources/toha-demo/<commit>/` — or a process-local temp dir when the cache is unwritable. |
| source subdir | `<root>/template` (the `template.yml` default). |
| `trusted` | `false` — like a fetched source; the demo's hooks (it ships none) would need `--trust`. |
| `named` | `false` — there is no registry entry, so it can never be registry-trusted, and `trustable()` returns `None` for it. |

Downstream designs consume `(formal_name, commit)` off `ResolvedTemplate` and
`StagedRecord`. Both are stable strings; `install_key("toha-demo") ==
"toha-demo"`, so the cache path is deterministic and readable.

---

## 3. Data / type sketch

```rust
// src/cli/bundled.rs  — new module, behind the `cli` feature, sibling of skills.rs.
//! The demo template compiled into the binary, materialized on demand into the
//! ordinary source cache so the rest of the pipeline sees a normal folder.
//!
//! Invariant: nothing outside this module and its three call sites in `resolve`
//! knows the demo is compiled in. The materialized folder is an ordinary
//! template root; `(formal_name, commit)` is an ordinary source identity.

use include_dir::{Dir, DirEntry, File, include_dir};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use super::resolve::{ResolveError, ResolvedTemplate};
use crate::Dirs;

/// The single source of the demo. `include_dir` embeds the very folder the
/// crate ships and the docs example uses, so the embedded copy is that folder
/// at compile time and cannot drift from it.
static DEMO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo");

/// The reserved `<TEMPLATE>` token and the demo's stable formal name.
pub const RESERVED: &str = "toha-demo";

/// The content identity of the embedded demo: the role a git commit plays for a
/// fetched source. Lowercase hex SHA-256 over a canonical serialization of every
/// embedded file, computed once. Stable for one content: it survives binary
/// upgrades that do not touch the demo, and changes only when the demo does.
pub fn commit() -> &'static str {
    static COMMIT: LazyLock<String> = LazyLock::new(|| digest(&DEMO));
    &COMMIT
}

/// Resolve the reserved name to the materialized demo. Called only after every
/// registry layer returned NotFound for exactly `RESERVED` (fallback-only
/// precedence), so it never shadows an installed / aliased / discovered
/// template. Offline, from any cwd, with zero prior disk state.
pub fn resolve(dirs: &Dirs) -> Result<ResolvedTemplate, ResolveError> {
    let folder = materialize(dirs, commit()).map_err(ResolveError::message)?;
    Ok(ResolvedTemplate {
        formal_name: RESERVED.into(),
        commit: commit().into(),
        folder,
        trusted: false, // like a fetched source: hooks need --trust
        named: false,   // no registry entry: never registry-trusted
    })
}

/// Re-materialize the demo for a *staged* identity, offline. Returns `None` when
/// the record is not the bundled identity, so `resume_template` falls through to
/// its existing paths. The `commit` comparison is the meaningful guard: it holds
/// only when this binary's embedded demo still has the staged content, which is
/// exactly the condition under which re-materialization reproduces byte-identical
/// files.
pub fn resume(
    formal: &str,
    commit_recorded: &str,
    dirs: &Dirs,
) -> Option<Result<ResolvedTemplate, ResolveError>> {
    (formal == RESERVED && commit_recorded == commit()).then(|| resolve(dirs))
}

// ---- internals: identity + materialization (the deep part of the seam) ----

fn files<'a>(dir: &'a Dir<'a>) -> Vec<&'a File<'a>> {
    let mut out = Vec::new();
    fn walk<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a File<'a>>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => walk(child, out),
                DirEntry::File(file) => out.push(file),
            }
        }
    }
    walk(dir, &mut out);
    out
}

/// Canonical, order-independent content digest. Path and byte lengths are
/// length-prefixed so no rename/split of files can collide.
fn digest(dir: &Dir<'_>) -> String {
    let mut files = files(dir);
    files.sort_by(|a, b| a.path().cmp(b.path()));
    let mut hasher = Sha256::new();
    for file in files {
        let path = file.path().to_string_lossy();
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((file.contents().len() as u64).to_le_bytes());
        hasher.update(file.contents());
    }
    format!("{:x}", hasher.finalize())
}

/// Write the embedded demo where a git fetch of the same identity would land:
/// `<cache>/sources/toha-demo/<commit>/`. Idempotent and race-safe exactly as
/// `cached()` is (tempdir under the parent, then atomic rename; an existing root
/// is reused). Writing the cache is **not** a new capability — a git fetch
/// already writes here. When the cache cannot be created or written
/// (unavailable, read-only), falls back to a process-local temp dir so first run
/// still works with no cache at all.
fn materialize(dirs: &Dirs, commit: &str) -> Result<PathBuf, String> {
    // The guard that keeps the identity honest, mirroring cached()'s
    // `fetched.commit != commit`: only ever materialize the content we have.
    if commit != self::commit() {
        return Err(format!("recorded demo content {commit} differs from this build"));
    }
    let root = dirs.cache.join("sources").join(RESERVED).join(commit);
    match into_cache(&root) {
        Ok(()) => Ok(root),
        Err(_unavailable) => into_temp(), // read-only / missing cache: degrade, still offline
    }
}

fn into_cache(root: &Path) -> Result<(), String> {
    if root.is_dir() {
        return Ok(()); // idempotent: a prior process (or run) already wrote it
    }
    let parent = root.parent().ok_or("cache path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let staging = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let built = staging.path().join("root");
    write_tree(&DEMO, &built)?;
    if !root.exists() {
        fs::rename(&built, root).map_err(|e| e.to_string())?; // atomic publish
    }
    Ok(())
}

fn into_temp() -> Result<PathBuf, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root = dir.path().join("root");
    write_tree(&DEMO, &root)?;
    // Persist for the process lifetime; the OS temp reaper cleans it. The temp
    // fallback is per-process, but resume never depends on it: it re-materializes
    // from the embedded bytes, which are the durable source of truth.
    Ok(dir.keep().join("root"))
}

/// Create-new writes with blocking checks, generalized from `skills::export`.
/// No overwrite of anything that already exists; parents created as needed.
fn write_tree(dir: &Dir<'_>, dest: &Path) -> Result<(), String> {
    unimplemented!("materialize the embedded tree under `dest` with create_new(true)")
}
```

Supporting edits (small, in existing files):

```rust
// src/cli/resolve.rs — construct ResolveError from the bundled module.
impl ResolveError {
    pub(crate) fn message(error: impl ToString) -> Self { Self::Error(error.to_string()) }
}
// `mod bundled;` added to the cli module list in src/main.rs.
```

No changes to `ResolvedTemplate`, `StagedRecord`, `Address`, `Registry`,
`Template`, `Plan`, `apply`, `protocol`, or the engine.

---

## 4. Module / seam map

```
                       toha apply|stage|continue toha-demo <PATH>
                                        │
                                        ▼
        ┌──────────────────────  src/cli/resolve.rs  ──────────────────────┐
        │ resolve_template(arg)                                            │
        │   Address::Git   → (unchanged: registry entry or fetch_new)       │
        │   Address::Folder→ (unchanged)                                    │
        │   Address::Name  → registry.resolve(name)                         │
        │        ├ Ok ─────────────────────► entry()  (unchanged)           │
        │        └ NotFound && name=="toha-demo" ─► bundled::resolve(dirs) ──┼──┐
        │ resume_template(formal, commit, named)                            │  │
        │   empty commit → folder            (unchanged)                    │  │
        │   registry entry & commit matches  (unchanged)  ◄── installed     │  │
        │   bundled::resume(formal, commit, dirs)? ─────────────────────────┼──┤
        │   else → git parse + cached()      (unchanged)                    │  │
        └───────────────────────────────────────────────────────────────────┘  │
                                                                                 ▼
                                              ┌─────────  src/cli/bundled.rs  ─────────┐
                                              │ DEMO = include_dir!(docs/examples/demo)│
                                              │ RESERVED = "toha-demo"                 │
                                              │ commit() = sha256(canonical(DEMO))     │
                                              │ materialize():                         │
                                              │   into_cache(<cache>/sources/…/<commit>)│
                                              │     ─ tempdir_in → write_tree → rename │
                                              │   └ on failure → into_temp()           │
                                              └────────────────────────────────────────┘
                                                                                 │
                            returns an ordinary folder + (formal_name, commit)   │
                                                                                 ▼
   Template::load(folder) → Interview::start → Plan::build(walks source_dir) → apply_reporting
        (every one of these is UNCHANGED and never learns the demo is compiled in)
```

Trace length: a reader follows `resolve.rs` → `bundled.rs` → back to the
unchanged pipeline. Two files, one seam.

---

## 5. Verification & synchronization (C5)

One source: `docs/examples/demo/`. `include_dir!` embeds that exact folder, so
the embedded copy *is* the source at compile time — drift is structurally
impossible at the embed boundary. Three falsifiable tests make the remaining
invariants enforceable and fail loudly on drift:

1. **`embed_equals_source`** (unit): walk `docs/examples/demo/` on disk; assert
   the set of relative paths and the bytes of each equal the corresponding entry
   in `DEMO`. Fails if the include path is ever redirected or the tree diverges.
2. **`commit_derives_from_source`** (unit): recompute the §3 digest over the
   on-disk `docs/examples/demo/` and assert it equals `bundled::commit()`. Pins
   the identity to the one source.
3. **`demo_applies_offline`** (integration, `tests/`): with clean HOME/XDG dirs
   (no `templates.yml` in any layer, empty cache), no network, and an arbitrary
   cwd, run `toha apply toha-demo <tmp> --answers <doc>` where the doc is
   `{ "title": "Sample Title", "topic": "Sample Topic" }`; assert the produced
   tree equals a fixture `expected/note.txt` = `"Sample Title\nTopic: Sample
   Topic\n"`. A companion case runs `toha stage toha-demo <tmp> --async …` in one
   process and `toha continue`/`toha apply <tmp>` in a **second** process and
   asserts byte-identical output (C3).

The artifact/package check is already covered by Cargo: `docs/examples/demo/**`
is in the crate `include` set, so the source ships in the tarball, and
`include_dir!` fails the build if the folder is missing.

---

## 6. Precedence rule (explicit)

`<TEMPLATE>` classification order is preserved verbatim:

> git URL → `host:` shorthand → folder → alias → short name → formal name → **bundled reserved name (`toha-demo`)**

The bundled demo sits strictly **last**, inside the `Address::Name` arm, and
answers only when `registry.resolve` returns `NotFound` for exactly the token
`toha-demo`. Consequences:

- Every current successful name-resolution outcome is preserved unchanged: any
  installed / aliased / discovered / formal `toha-demo` (or any other name)
  still wins, because it resolves *before* the fallback is consulted.
- Ambiguous short names still fail with exit 5; every non-`toha-demo` NotFound
  still fails with "template not found".
- The only behavioral change is that a previously-failing
  `toha apply toha-demo …` in a clean environment now succeeds. No existing
  outcome is narrowed, so **no capability approval is required.**

The bare short name `demo` (the demo's own `name:`) does **not** trigger the
fallback; only the reserved token `toha-demo` does. This keeps the trigger
specific and collision-averse.

### Product decision (present as options)

- **(A) Fallback-only — recommended.** As above. Preserves every existing
  outcome; needs no approval; the demo is a safety net, never a shadow.
- **(B) Reserved-name-wins.** The bundled demo answers `toha-demo` *before* the
  registry, shadowing any installed/aliased `toha-demo`. This *narrows*
  supported resolution (a currently-working installed `toha-demo` would change
  meaning) and is therefore a product decision requiring explicit approval.
  Recommend against: it trades a real, supported capability for a marginal
  guarantee that the demo is always "the real one".

---

## 7. Error / results contract

| Situation | Outcome |
| --- | --- |
| `toha apply/stage toha-demo <dir>`, clean env, cache writable | Materialize into `<cache>/sources/toha-demo/<commit>/`; ordinary interview/plan/apply follows. |
| Same, cache dir unavailable or read-only | Materialize into a process-local temp dir; identical behavior. No error, no new capability. |
| `apply --dry-run toha-demo <dir>` | Materialize, build plan, print `create note.txt`, write nothing. Demo has no hooks, so no `--trust` gate. |
| `stage toha-demo <dir>` then `continue`/`apply <dir>` in a later process | `resume_template` → `bundled::resume` matches `(formal="toha-demo", commit==this build's digest)` → re-materialize → byte-identical output. |
| Resume after a binary upgrade that **changed** the demo | `commit_recorded != commit()` → `bundled::resume` returns `None` → falls through to the git-parse error → surfaced as `guidance::replay_failed` ("cannot resume; `abort` and start over"). Acceptable, documented degradation. |
| An installed/aliased/discovered `toha-demo` exists | Registry wins; the fallback is never reached; unchanged behavior. |
| Both cache and temp materialization fail | `ResolveError::Error(..)` with the underlying I/O message. Rare (no writable filesystem at all). |
| Trust | `trusted=false`, `named=false`; hooks (none shipped) require `--trust`, exactly like a folder/address source. Path-safety and no-overwrite-without-`--force` are enforced by the unchanged `Plan`/`apply`. |

Exit codes are those of the existing pipeline: 0 success / dry-run, 3 needs
trust, 4 incomplete batch, 5 ambiguous — the bundled demo introduces none.
