# Bundled offline toha-demo — verification

Verifies the synthesized design in `design.md` against caller usage, the task
requirements and constraints, failure behaviour, the real code seams, and
predecessor compatibility.

## Code-seam verification (against the tree at epic head `cfab3286`)

Every integration point the design edits was confirmed to exist as described:

- `ResolvedTemplate` is exactly `{ formal_name, commit, folder, trusted, named }`
  (`src/cli/resolve.rs:17`) — the design adds no field, confirmed possible.
- `resolve_template` has the `Address::Name(name)` arm
  (`src/cli/resolve.rs:201`); `formal_name` resolves `Address::Name` via
  `registry.resolve(name)?.formal_name` (`:220`) — both take the fallback branch.
- `resume_template(formal, commit, named, config, registry, dirs)`
  (`src/cli/resolve.rs:224`) — signature matches the resume graft.
- `registry::ResolveError` has `NotFound(String)` and `Ambiguous`
  (`src/registry.rs:85`) — the fallback branches on `NotFound`; `Ambiguous`
  propagates unchanged.
- `src/cli/resolve.rs:26` `ResolveError { Error(String), Ambiguous {…} }` with a
  private `text()` helper (`:31`) — reused instead of a new constructor.
- Embedding precedent: `include_dir!` (`src/cli/skills.rs:17`) and
  `export(dir, destination)` (`:110`) — the materialization pattern to
  generalize.
- `source::install_key` (`src/source.rs:209`) sanitizes to lowercase/hyphen;
  `install_key("toha-demo") == "toha-demo"` — deterministic cache path component.
- `docs/examples/demo/` holds `template.yml` (two text questions `title`,
  `topic`, no hooks) and `template/note.txt` — the single embedded source.

The epic advanced during recovery to `cfab3286` (`fix(cli): parse arguments
before resolving directories`). That change moves argument parsing ahead of
`Dirs::resolve` in `main()` and documents that `--version`/`--help` resolve no
directory. It does not alter the resolution seam, the `ResolvedTemplate` shape,
or the registry contract; the design is unaffected and aligns to this head.

## Caller usage reconciled against the sketch

The three README call sites and the two/three concrete call sites agree with the
signatures in `design.md` §Interfaces:

- `toha apply toha-demo <dir>` and `apply --dry-run toha-demo <dir>` → the
  `Address::Name` fallback → `bundled::resolve(dirs) -> Result<ResolvedTemplate>`
  → unchanged `Template::load`/interview/`Plan::build`/`apply`.
- `apply --answers <doc> toha-demo <dir>` → same resolution; answers flow through
  the unchanged engine.
- `stage toha-demo <dir>` records `StagedRecord{ template:"toha-demo",
  commit:<digest>, named:false }`; a later process calls
  `resume_template("toha-demo", <digest>, false, …)` → `bundled::resume` matches
  identity → re-materializes → identical output. Usage and signatures match.

## Requirements and constraints checked against the design

| Requirement / constraint | Status | Where satisfied |
|---|---|---|
| First-run offline by name from any cwd, no registry/cache/network/git | Met | Fallback resolve materializes from embedded bytes; cwd-independent absolute folder |
| Covers `apply`, `apply --dry-run`, `stage`→`continue`/`apply <PATH>` | Met | design.md §Behaviors; dry-run still materializes (Plan reads the folder) then writes nothing |
| `<TEMPLATE>` classification order preserved | Met | No change to `source::parse`; bundled fires only inside the `Name` arm |
| Every current name-resolution outcome preserved (installed/aliased/discovered `toha-demo`) | Met | Registry consulted unchanged; bundled reachable only on `NotFound("toha-demo")`; no registry entry to conflict with an alias or short name |
| Collision precedence stated, not implied | Met | Explicit fallback-only rule; reserved-name-wins presented as approval-gated option (D1) |
| Selected-template identity stable + offline-reconstructable | Met | `formal_name="toha-demo"` + content-digest `commit`; `bundled::resume` rebuilds from identity alone |
| Shared identity pinned for 1073/1075 | Met | design.md §Identity; confirmed as D4 |
| Single maintained source + enforcing check | Met | `include_dir!` (one source); `embed_equals_source`, `commit_derives_from_source`, offline e2e fixture |
| Pure engine / Template / Plan / apply / protocol / staging unchanged | Met | "embedded" confined to `src/cli/bundled.rs`; no field/variant leaks |
| Trust boundary preserved (untrusted; hooks need `--trust`) | Met | `trusted=false`, `named=false`; demo ships no hooks |
| Path safety + no-overwrite-without-`--force` | Met | Unchanged `Plan`/`apply` enforce both |
| No new permission/timeout/pinned-check/subprocess | Met (with D2) | Writes only where git already writes; runtime digest is a content match, not a version gate; D2 flags the only new-write-location question |
| No production/spec/schema edits in this design task | Met | All output is design artifacts; contract edits are described, not applied |

## Failure cases → falsifiable tests

| Failure / edge | Designed behaviour | Falsifiable check |
|---|---|---|
| Clean env first run | Materialize + apply | `demo_applies_offline` (isolated HOME/XDG, empty cache, no net, arbitrary cwd) asserts output tree |
| Cross-process staged resume, content unchanged | Re-materialize, identical output | staged variant of `demo_applies_offline` in a second process asserts byte-identical tree |
| Resume after a build that changed the demo | `commit` mismatch → clear "stage again", offline | unit test on `bundled::resume` returns the mismatch error / `None`→existing replay-failed guidance |
| Installed/aliased/discovered `toha-demo` present | Registry wins; bundled never fires | unit test: seeded registry `toha-demo` resolves to the registry entry, bundled untouched |
| Ambiguous short name | Existing `Ambiguous` (exit 5) | unchanged registry test; bundled fires only on `NotFound`, not `Ambiguous` |
| Source ↔ embed drift | Build/test failure | `embed_equals_source` + `commit_derives_from_source` |
| Concurrent materialization | Both succeed; loser reuses winner's slot | idempotent tempdir-then-rename; reuse-existing test |

## Compatibility with predecessors

The task records no predecessor-design prerequisite ("may start independently").
No approved predecessor revision is consumed, so none can be violated. The design
is a consumer-side contributor to the shared selected-template identity that 1073
(defaults) and 1075 (Jinja context) consume; it changes no existing shared
interface, so no dependent returns to review on its account. Both dependency
graphs (design and implementation, via paired task 1059) are unaffected beyond
the new identity surface, which is additive.

## Residual risks

- Cache-unwritable first run fails unless D2 selects the temp-dir degrade.
- A content-digest `commit` invalidates a staged interview across a build that
  edits the demo — intended, degrades to a clear "stage again".
- Reserving the resolution token `toha-demo` (distinct from the demo's short name
  `demo`) assumes no user mints the literal formal key `toha-demo`; formal keys
  are address-derived and never equal the bare token, so this is theoretical.
