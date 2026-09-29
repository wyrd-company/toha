# Base refresh and approval record — opt-in Jinja hook results

Evidence record. The approved package (`design.md`, `design.yml`,
`verification.md`, `brief.source.html`, `brief.deck.html`, and the arena
artifacts) is byte-identical to the approved revision; this file adds
provenance only and changes no design semantics.

## Approval

- Bob, verbatim: "hook-results: all recommendatons accepted. approved" and
  "hook results v2: approved".
- Binds Postplan draft `yzh93iexvlil` version 2
  (<https://yzh93iexvlil.postplan.dev>), approval anchor
  `999ab89f1d186a08af01dbe681350dbe97643c0d`, design SHA-256
  `a64850ff7711a9a2b6e1868d9c1866e41260aceb1bd6679565e5ec9c9a35dd69`.
- All five recommendations accepted:
  1. Readable surfaces: later top-level hook `when`/`run[1..]`/`args`/`cwd`
     and `messages.after-apply`; not `run[0]`, the script path, or `each`.
  2. A hook `id` is an ordinary identifier in the authored-id space; no new
     reserved name.
  3. Strict UTF-8 decode; on exit 0, empty or malformed JSON is fatal; every
     output fault is byte-free.
  4. `parse: json` puts the parsed value directly at `<id>`; execution metadata
     is at an author-declared `status-id` `{exit_code, stdout, stderr, parsed}`.
  5. `parse: json` requires an explicit `capture: [stdout]`.

## Base

- Base and merge-base: epic `e762570bd6169ed1c5097bab354371e991d9ee69`.
- The branch holds eight design-only commits; every changed path is under
  `docs/technical-designs/second-release/hook-results/`.
- The approved design-directory tree `8006175f9683dd0ac27c29965617daa35013d5c5`
  is identical at the approval anchor and after the refresh, before this record
  was added.

| Artifact | SHA-256 (unchanged) |
| --- | --- |
| `design.md` | `a64850ff7711a9a2b6e1868d9c1866e41260aceb1bd6679565e5ec9c9a35dd69` |
| `design.yml` | `d90af16a7daa8f520c59d71dff989aa92f821f02e9c702d2fdae2fa8173ce6d1` |
| `verification.md` | `df947e0bae3dcd120d83a25a30445e441560d9850d7dc047ac31756afd6ce3f6` |
| `brief.source.html` | `6795a7c91d261b7726c20818186a75bd9cf8e8318ac99bc8e4a35993d2da46a6` |
| `brief.deck.html` | `b1575c046d4c6752df056c670d16d53168ab5ce3cabd8ac8577178719a8771d6` |
| `01-grounding.md` | `c0e6d7affb24059c6db1b953416c977939ba782944357abcfa5ee802ca70f6c9` |
| `05-synthesis.md` | `26a4c4980aaecbd3ead634291d964eb637c91ab19579c1c77527efe8c829a951` |
| `06-json-revision-grounding.md` | `a0642010ef5d820db6d9a072014440b02b27beedc562deb510f4d38c90a29900` |
| `07-json-cross-judge.md` | `53ee3bccaf6f9308ac983acd2642353e262a2a9199aa90694960e6570a278c28` |
| `08-json-synthesis.md` | `f1749102b96972e0c9ef18f2503e481bcc3fbf999e95452126bb584a7a16aeab` |

The published Postplan v2 raw HTML is byte-identical to the committed
`brief.deck.html` (318262 bytes) and contains one `Reveal.initialize`.

## Composition with the landed runtime

The base contains the landed context, confirm-flow, and include runtimes. Each
premise the approved design relies on holds at `e762570`. Line citations in
`01-grounding.md` and `design.md` are anchored to the grounding base; the
current locations are:

| Premise | Current location | Holds |
| --- | --- | --- |
| Hooks run last, after every file write; nonzero stops and fails | `src/apply.rs` `Plan::apply_reporting` (file unchanged) | yes |
| `HookOutcome { success, code }`; `ProcessRunner` uses `status()` | `src/hook.rs` (file unchanged) | yes |
| `Plan::build` renders hooks and messages before any write | `src/plan.rs:170-315` (`render_hooks` at 302, after-apply at 315) | yes |
| `Plan { hooks, before_apply, after_apply }` | `src/plan.rs:91-96` | yes |
| Hook fields are `Tmpl`/`Expr`; a hook has no `id` | `src/template.rs:245-258` | yes |
| Load computes the environment need over interview, files, hooks, messages, and the retained render program | `src/template.rs:1191-1192` | yes; a hook-result name adds no need |
| Seventeen reserved names; authored-id collision check | `src/context.rs:37`; `src/template.rs:559-567` | yes; a hook `id` in the authored-id space gets this check |
| `Completed::step()`, `Step`, `Ended`, `Disposition` | `src/interview.rs:71-154` | yes |
| Includes only through `Partials::compile` → `FileTmpl`; loaderless `Tmpl::compile` | `src/jinja.rs:101-112`, `788-977` | yes; hook fields stay `Tmpl` |

Implementation note for the paired build: the approved contract names
`undeclared_variables(true)` as the mechanism for the attribute-form
`<id>.exit_code` guard; the landed `Tmpl::compile` still records
`undeclared_variables(false)`, and the landed conservative AST walk has
distinct `Expr::GetAttr` and `Expr::GetItem` arms (`src/jinja.rs:493-496`).
The required behavior is the `GetAttr`/`GetItem` distinction with degradation
to a key read for aliased, looped, and non-variable roots. Choosing the landed
walk over `undeclared_variables(true)` is a mechanism deviation to surface in
Phase D, not to make silently.

Result: faithful base refresh. No semantic, interface, capability, trust,
permission, timeout, dependency, or subprocess change; no new checkpoint is
required.
