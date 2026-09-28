# Bundled offline `toha-demo` — Candidate 2 (Direction B: synthetic lowest-precedence "Bundled" registry layer)

The embedded demo is modelled as a new registry layer **below `System`**. Name
resolution is unchanged in shape: `registry.resolve("toha-demo")` returns the
bundled demo **only when no higher layer supplies it**. Collision behaviour is
therefore fallback-only as an *emergent property of layer precedence*, not a
special case bolted onto the resolver. Everything downstream — `ResolvedTemplate`,
`Template::load`, the pure interview, `Plan`, `apply`, `staging` — is untouched
and never learns the word "embedded".

---

## 1. Usage (caller's view — written first)

### README / quickstart

> **Try Toha in ten seconds, offline.** Every Toha binary carries one small,
> inspectable demo template. From any directory, with no install and no network:
>
> ```console
> $ toha apply toha-demo ./my-notes
> Note title: Weekly Sync
> Topic: planning
> create note.txt
> ```
>
> Preview without writing (there is no `preview` subcommand — `--dry-run` is the
> preview path):
>
> ```console
> $ toha apply --dry-run toha-demo ./my-notes
> create note.txt
> ```
>
> Stage now, finish later in another shell — still offline:
>
> ```console
> $ toha stage toha-demo ./my-notes           # process 1: interview, saved
> $ toha continue ./my-notes                   # process 2: resumes, same output
> ```
>
> `toha-demo` is a **reserved fallback name**. If you install, alias, or discover
> your own `toha-demo`, *yours* wins — the bundled demo only answers when nothing
> else claims the name. Inspect it any time:
>
> ```console
> $ toha templates list | grep toha-demo
> toha-demo    demo         (none)    false
> ```

### Three concrete call sites

**A. First-run apply, headless, arbitrary cwd, offline.**

```console
$ cd /tmp/anywhere
$ toha apply -A answers.json toha-demo ./out
create note.txt
```

`answers.json` is `{"title": "...", "topic": "..."}`. No registry entry, no
cache seeded by the user, no git. The bundled layer supplies the template.

**B. Dry-run preview (the "preview" path).**

```console
$ toha apply --dry-run toha-demo ./out
create note.txt
```

Prints the plan; writes nothing. Because the bundled layer is untrusted and the
demo has no hooks, no trust prompt appears.

**C. Cross-process staged resume, offline, separate process.**

```console
$ toha stage toha-demo ./out < /dev/null --async     # emits first batch as JSON
$ toha apply ./out -A batch-answers.json             # later process, offline
create note.txt
```

The staged record holds `template: "toha-demo"`, `commit: "<digest>"`,
`named: true`. The second process reconstructs the folder from that identity
alone, through the same bundled layer, and renders identically.

### Rust crate caller (unchanged)

Crate callers of `toha` (the library) see **no new surface**. Embedding and
materialization live entirely in the binary (`cli`, gated by the existing `cli`
feature that already owns `include_dir`). The library gains only one public enum
variant, `registry::Layer::Bundled`, and one seeding entry point.

---

## 2. Data & type sketch (derived from the usage above)

### 2.1 The layer variant (library, `src/registry.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Bundled,     // NEW: lowest precedence, below System. Serializes as "bundled".
    System,
    User,
    Local,
    Discovered,
}
```

`Bundled` is listed first to read as the bottom of the precedence chain, but the
ordering that matters is enforced by *insertion-if-absent* (below), not by enum
order.

### 2.2 The single load-bearing invariant, encoded as a predicate

The whole design rests on one rule:

> **The Bundled layer is reachable only as the exact formal-name key, and only
> when no higher layer occupies that key. It never participates in alias
> matching, short-name matching, or alias-uniqueness checks.**

Encoded once, consumed everywhere:

```rust
impl Listed {
    /// A bundled entry answers only to its exact formal name; it is invisible to
    /// alias resolution, short-name resolution, and alias-uniqueness checks, so
    /// that adding it changes no existing name-resolution outcome.
    fn formal_only(&self) -> bool {
        self.layer == Layer::Bundled
    }
}

impl Registry {
    /// Entries eligible for alias and short-name matching. Excludes formal-only
    /// (bundled) layers so they act purely as a formal-key fallback.
    fn by_name(&self) -> impl Iterator<Item = &Listed> {
        self.entries.values().filter(|listed| !listed.formal_only())
    }
}
```

`resolve`, `check_aliases`, and the `templates alias` guard all route their
name-matching scans through `by_name()`; only the final `entries.get(name)`
formal-key lookup sees the bundled entry. This is the *single source of truth*
for "bundled = formal-only" (per encode-lessons-in-structure): the rule cannot
drift across the four call sites because there is one iterator.

### 2.3 The bundled entry descriptor (binary, new `src/cli/bundled.rs`)

The library stays ignorant of `include_dir`. The binary owns the embed and hands
the library a plain `(formal, Entry)` to seed. `Entry` already carries every
field the schema requires (`name`, `source`, `path`) plus optional
`commit`/`trusted`; no new field, so no schema change and no `additionalProperties`
violation.

```rust
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use toha::registry::{Entry, Layer, Listed, Registry};
use toha::source;

/// The one maintained source. `include_dir!` binds the embedded bytes to exactly
/// `docs/examples/demo/`, so the embedded copy cannot diverge from the example.
static DEMO: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo");

/// The reserved formal name. Stable across every run of a build; the shared
/// selected-template identity consumed by defaults (1073) and Jinja (1075).
pub const FORMAL: &str = "toha-demo";

/// The demo's short name, from its `template.yml` (`name: demo`). Held so the
/// listing row is honest; NOT used for resolution (bundled is formal-only).
pub const SHORT: &str = "demo";

/// Content digest of the embedded tree, set by build.rs. 64 lowercase hex chars
/// (sha256), matching the documented commit pattern `^[0-9a-f]{40}([0-9a-f]{24})?$`.
/// It is the `commit` of the bundled identity and the anchor of the sync check.
pub const COMMIT: &str = env!("TOHA_BUNDLED_DEMO_DIGEST");

/// The directory the bundled demo materializes into for `cache`. Mirrors the git
/// source cache layout (`cache/sources/<key>/<commit>`) so no new directory
/// concept is introduced: `cache/bundled/<key>/<commit>/`.
pub fn folder(cache: &Path) -> PathBuf {
    cache
        .join("bundled")
        .join(source::install_key(FORMAL)) // "toha-demo" -> "toha-demo"
        .join(COMMIT)
}

/// The synthetic registry entry for the bundled layer. `path` is the (possibly
/// not-yet-materialized) materialization folder; `trusted: false` always — a
/// below-System layer confers no trust (spec: trust is honored only from user or
/// system layers), so the bundled demo is untrusted exactly like a discovered
/// template.
pub fn listed(cache: &Path) -> Listed {
    not implemented // Listed { formal_name: FORMAL, entry: Entry { name: SHORT,
                    // source: FORMAL, path: folder(cache), commit: Some(COMMIT),
                    // reference: None, aliases: [], trusted: false },
                    // layer: Layer::Bundled, trusted: false }
}
```

### 2.4 Seeding the layer (library, `src/registry.rs`)

Precedence is realized by **insert-if-absent after the higher layers merge**, so
a System/User/Local/Discovered entry that already holds the formal key wins, and
the bundled fields never leak into a higher entry's field-inheritance:

```rust
impl Registry {
    /// Add fallback entries at the lowest precedence. An entry is inserted only
    /// when its formal name is absent, so any higher layer that supplies the same
    /// formal name shadows it. Bundled entries do not alter alias/short matching.
    ///
    /// Idempotent: seeding twice is a no-op. Callers seed after `merge`,
    /// `discover`, and `apply_local_aliases`.
    pub fn seed_fallback(&mut self, entries: impl IntoIterator<Item = Listed>) {
        for listed in entries {
            self.entries
                .entry(listed.formal_name.clone())
                .or_insert(listed); // insert-if-absent == lowest precedence
        }
        // No check_aliases needed: bundled entries carry no aliases and are
        // excluded from alias-uniqueness by `by_name()`.
    }
}
```

### 2.5 Resolution, unchanged in shape (library, `src/registry.rs`)

Only the two name-matching scans switch to `by_name()`; the formal-key lookup is
unchanged and is where the bundled entry surfaces:

```rust
pub fn resolve(&self, name: &str) -> Result<Resolved, ResolveError> {
    // aliases: any NON-bundled entry aliased `name` (bundled excluded).
    if let Some(v) = self.by_name().find(|v| v.entry.aliases.iter().any(|a| a == name)) {
        return Ok(v.resolved());
    }
    // short names: NON-bundled entries whose short name == `name` (bundled excluded).
    let shorts: Vec<_> = self.by_name().filter(|v| v.entry.name == name).collect();
    if shorts.len() > 1 {
        return Err(ResolveError::Ambiguous { name: name.into(),
            matches: shorts.iter().map(|v| v.formal_name.clone()).collect() });
    }
    if let Some(v) = shorts.first() { return Ok(v.resolved()); }
    // formal key: the ONLY path that can reach the bundled entry.
    self.entries.get(name).map(Listed::resolved)
        .ok_or_else(|| ResolveError::NotFound(name.into()))
}
```

`check_aliases` gains the same exclusion so a user may alias to `toha-demo`
(shadowing the fallback) without the merge rejecting "alias equals a formal name":

```rust
pub fn check_aliases(&self) -> Result<(), RegistryError> {
    let mut used = IndexMap::new();
    for (formal, entry) in self.by_name().map(|l| (&l.formal_name, l)) {
        for alias in &entry.entry.aliases {
            let clashes_formal = self.entries.get(alias).is_some_and(|l| !l.formal_only());
            if clashes_formal || used.insert(alias.clone(), formal.clone()).is_some() {
                return Err(RegistryError::AliasConflict { alias: alias.clone(),
                    formal: formal.clone() });
            }
        }
    }
    Ok(())
}
```

### 2.6 Materialization (binary, `src/cli/bundled.rs`)

Idempotent, race-safe, reusing the exact pattern the git cache (`resolve::cached`)
and skills `export` already use — temp dir, create-new writes, atomic rename:

```rust
/// Ensure the bundled demo exists as a real folder at `folder(cache)`. Idempotent:
/// if the target directory already exists it returns without writing (a prior
/// process, or this one, already materialized it). Otherwise it writes the whole
/// embedded tree into a sibling temp dir with create-new semantics and renames it
/// into place, so concurrent processes never observe a half-written folder.
///
/// Writes only under `cache/bundled/...`; never into the target project, never
/// into a `.git`. Requires no capability the git-source cache does not already use.
pub fn materialize(cache: &Path) -> Result<PathBuf, String> {
    let root = folder(cache);
    if root.is_dir() {
        return Ok(root); // idempotent no-op; safe across processes and re-runs
    }
    not implemented // create parent; tempdir_in(parent); export(&DEMO, &tmp) with
                    // create-new writes (skills::export pattern); if !root.exists()
                    // fs::rename(tmp, &root); return root.
}
```

### 2.7 Resolution + materialization seam (binary, `src/cli/resolve.rs`)

`resolve_template` and `resume_template` gain one guarded call each. The seam is
tiny and local; nothing else in the flow changes.

```rust
// in resolve_template, Address::Name branch:
Address::Name(name) => {
    let resolved = registry.resolve(name)?;
    let mut out = entry(resolved.formal_name, registry).expect("resolved registry entry");
    if resolved.layer == Layer::Bundled {
        out.folder = bundled::materialize(&dirs.cache)?; // lazy: only on a hit
    }
    Ok(out)
}
```

```rust
// in resume_template, after the registry entry with matching commit is found:
if let Some(found) = entry(formal.into(), registry) {
    if found.commit == commit {
        let mut out = ResolvedTemplate { trusted: named && found.trusted, named, ..found };
        if registry.entries.get(formal).is_some_and(|l| l.layer == Layer::Bundled) {
            out.folder = bundled::materialize(&dirs.cache)?; // reconstruct offline
        }
        return Ok(out);
    }
}
```

`resume_template` never reaches the git-fetch branch for the bundled demo: the
seeded entry is always present and its `commit` matches, so the offline
reconstruction is a pure `entry()` lookup plus an idempotent materialize.

### 2.8 Wiring (binary, `src/cli/resolve.rs` and `src/cli/templates.rs`)

Both registry-building paths seed the bundled layer, so resolution *and* listing
agree:

```rust
// end of load_context (resolve.rs) and Context::load (templates.rs), after
// merge + discover + apply_local_aliases:
registry.seed_fallback([bundled::listed(&dirs.cache)]);
```

### 2.9 Build-time digest (build.rs)

```rust
// build.rs, after TOHA_VERSION:
// Walk docs/examples/demo in sorted order, feed (relative-path, bytes) into
// sha256, emit the 64-hex digest. rerun-if-changed guards staleness.
println!("cargo:rerun-if-changed=docs/examples/demo");
let digest = bundled_digest(&root.join("docs/examples/demo")); // sha256 hex
println!("cargo:rustc-env=TOHA_BUNDLED_DEMO_DIGEST={digest}");
```

---

## 3. Module / seam map

```
                         binary crate (feature = "cli")
  ┌───────────────────────────────────────────────────────────────────────┐
  │ src/cli/bundled.rs   include_dir!(docs/examples/demo)  ← ONE source     │
  │   FORMAL="toha-demo"  SHORT="demo"  COMMIT=env!(TOHA_BUNDLED_DEMO_DIGEST)│
  │   listed(cache) -> Listed        materialize(cache) -> PathBuf          │
  └───────▲───────────────────────────────────────────────▲───────────────┘
          │ seed                                           │ materialize (lazy)
  ┌───────┴───────────────┐                    ┌───────────┴───────────────┐
  │ resolve::load_context │                    │ resolve::resolve_template  │
  │ templates::Context    │  seed_fallback     │ resolve::resume_template   │
  │   ...merge/discover   ├───────────────────►│   Name / staged-resume     │
  └───────────────────────┘                    └───────────┬───────────────┘
                                                            │ ResolvedTemplate
             library crate (toha)                           │ { formal_name,
  ┌──────────────────────────────────────────┐             │   commit, folder,
  │ registry::Layer::Bundled  (new variant)   │             │   trusted:false,
  │ registry::Registry::seed_fallback         │             │   named }
  │ registry::Registry::by_name (formal-only  │             ▼
  │   exclusion) → resolve / check_aliases    │   Template::load(folder)
  └──────────────────────────────────────────┘             │
                                                            ▼
                   Interview::start → Plan::build → apply   (ALL UNCHANGED;
                   staging::StagedRecord (UNCHANGED)         no "embedded" word)
```

Trace for `toha apply toha-demo ./out`: `main::run` → `resolve_template("toha-demo")`
→ `registry.resolve` (formal-key hit, layer Bundled) → `bundled::materialize` →
`Template::load` → interview → `Plan::build` → `apply_reporting`. Three files from
argument to written file; the bundled concept lives in exactly one of them
(`bundled.rs`) plus one enum variant.

---

## 4. Resolution precedence rule (stated explicitly)

Argument classification is **unchanged**: git URL → `host:` → folder → name.
For a name, resolution is, highest to lowest:

1. **Alias**, any non-bundled layer (Local/User/System/Discovered), insertion order.
2. **Short name**, any non-bundled layer (ambiguity across ≥2 → exit 5, as today).
3. **Formal-name key**, higher layer wins on the key (Local > User > System);
   **the Bundled layer occupies a formal key only when no higher layer does.**

The bundled demo answers `toha-demo` **iff** no alias, no short name, and no
higher-layer formal key already answers it. Consequences, all emergent from the
rule:

- Install/discover a template whose **short name** is `toha-demo` → step 2 wins.
- `templates alias X toha-demo` → step 1 wins; the merge does **not** reject it,
  because `check_aliases` excludes the bundled formal (per `by_name`).
- Formal names are *derived* (git address or canonical path), so a user cannot
  mint the literal formal key `toha-demo`; the fallback is shadowed via alias or
  short name, never by a hand-set formal — which is exactly why fallback-only is
  safe and preserves every current outcome.
- Bare `demo` is **unchanged**: bundled is excluded from short-name matching, so
  `toha apply demo` behaves exactly as before (it does *not* start resolving to
  the bundled demo and does *not* introduce a new ambiguity with a user's `demo`).

### Alternative product choice: reserved-name-wins

If Toha wanted the bundled demo to **always** answer `toha-demo` regardless of
user configuration, the rule would move the bundled layer *above* alias/short
matching for its reserved key. This **would break an existing installed or
aliased `toha-demo`** — a narrowing of supported name resolution and thus a
product decision requiring explicit approval.

**Recommendation: fallback-only** (the shape above). It closes the offline gap
without changing any current outcome, needs no approval, and keeps the reserved
name a courtesy rather than a seizure. Reserved-name-wins is presented as the
option should the product ever want the demo guaranteed; it is not recommended.

---

## 5. Selected-template identity scheme (shared coordination surface for 1073/1075)

The bundled demo resolves to an ordinary `ResolvedTemplate` — downstream designs
branch on nothing:

| field         | value                                                             |
|---------------|-------------------------------------------------------------------|
| `formal_name` | `"toha-demo"` — the reserved fallback formal key, constant.        |
| `commit`      | `TOHA_BUNDLED_DEMO_DIGEST` — sha256 hex of `docs/examples/demo/`.  |
| `folder`      | `<cache>/bundled/toha-demo/<commit>/` — absolute, cwd-independent. |
| `trusted`     | `false` — untrusted like a discovered template; hooks need `--trust`.|
| `named`       | `true` — resolved by name, like a discovered entry.               |

- **Stable across runs of one build**: `commit` is a compile-time constant; the
  folder is a pure function of `(cache, commit)`.
- **Reconstructable offline from staged identity alone**: `StagedRecord` holds
  `template="toha-demo"`, `commit="<digest>"`, `named=true`. `resume_template`
  finds the seeded entry (present every run), matches `commit`, and materializes
  — no file, no network, no cwd dependence.
- **Contract-shaped commit**: 64-hex sha256 satisfies the documented
  `^[0-9a-f]{40}([0-9a-f]{24})?$`, so the identity reads as a normal pinned
  template to 1073/1075 and to any future registry serialization.

---

## 6. Error / results contract

- **First-run success**: identical to any named template — `create <path>` lines,
  exit 0.
- **`--dry-run`**: prints the plan, writes nothing, exit 0. No trust prompt (the
  bundled layer is untrusted but the demo has no hooks; a hooked bundled template
  would dry-run and require `--trust`, exit 3, exactly like a discovered one).
- **Shadowed name**: if a higher layer supplies `toha-demo`, that entry resolves;
  the bundled demo is silently absent from resolution (still listed, §7).
- **Cross-process resume, content unchanged**: `commit` matches → reconstruct →
  identical output.
- **Cross-process resume across an upgrade that changed the demo**: staged
  `commit` ≠ new `COMMIT`. The seeded entry's commit differs, so
  `resume_template` falls through to `source::parse("toha-demo")` →
  `Address::Name` → `ResolveError::Error("staged git template has invalid formal
  name")`, surfaced through the existing `replay_failed` / exit-1 path with
  "cannot resume, start over" guidance. Accepted degrade (no new error type).
- **Materialization I/O failure**: surfaced as `ResolveError::Error(<message>)`,
  the same channel as a git-cache failure.

Materialization introduces **no** new capability: it writes only under the
existing `cache` directory using create-new + atomic-rename (the git-source cache
pattern), spawns no subprocess, adds no timeout, changes no permission model, and
does no pinned-version check.

---

## 7. Discoverability / listing

The bundled entry is seeded into the merged `Registry`, so **unfiltered**
`toha templates list` shows one honest row:

```
FORMAL NAME   SHORT NAME   ALIASES   TRUSTED
toha-demo     demo                   false
```

- `--system` / `--user` / `--local` read the on-disk registry files directly and
  therefore do **not** show the bundled row — correct, because it lives in no
  file. The listing filter group `[local, system, user]` is unchanged; no new
  flag is added.
- JSON output already serializes `layer`; the bundled row reports
  `"layer": "bundled"`, `"trusted": false`, `"commit": "<digest>"`,
  `"path": "<cache>/bundled/..."`.

**Recommendation: show it** (as above). Direction B's thesis is that the demo is
a layer, so it should be discoverable through the one listing surface; hiding it
would contradict the model and leave users unable to see why `toha-demo` resolves.
Showing it is additive to the listing contract and changes no resolution outcome.

---

## 8. Verification & synchronization

Single source is *structural*: `include_dir!("$CARGO_MANIFEST_DIR/docs/examples/demo")`
binds the embedded bytes to the one example folder; there is no second copy to
drift. Three enforcing, falsifiable checks guard the seams:

1. **Digest freshness (source ↔ embedded).** A test recomputes the sha256 of
   `docs/examples/demo/` on disk and asserts it equals `env!("TOHA_BUNDLED_DEMO_DIGEST")`.
   *Falsifiable*: edit the demo without rebuilding → digest mismatch → test fails.
   `build.rs` `rerun-if-changed=docs/examples/demo` keeps a real build always in
   sync; the test guards against a stale artifact in CI (`task ci`).
2. **Materialized tree equals source.** A test calls `bundled::materialize` into a
   temp dir and asserts the tree is byte-for-byte equal to `docs/examples/demo/`.
   *Falsifiable*: any export bug or partial write diverges → test fails.
3. **End-to-end fixture, offline.** A `tests/fixtures/` case runs
   `toha apply toha-demo <tmp>` (and the staged cross-process variant) under an
   isolated empty `HOME`/XDG with no network, from an arbitrary cwd, and diffs the
   output against `tests/fixtures/.../expected/`. *Falsifiable*: any regression in
   resolution, materialization, or rendering fails the golden tree.

Package verification is unchanged: `Cargo.toml` already ships
`docs/examples/demo/**` in the crate `include` set, so the source the binary
embeds is the source the crate publishes.
