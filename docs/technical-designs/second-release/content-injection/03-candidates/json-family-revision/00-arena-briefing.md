# Arena briefing (revision) — content injection into existing files, JSON-family / structured-mutation question

You are one candidate in a design arena for **Toha** (a Rust project-scaffolding
tool + crate). Toha renders templates into files. This slice adds **content
injection into existing files** (writing into a file Toha does not own). A prior
arena already selected a base mechanism for arbitrary text; the project owner
(Bob) has now asked us to specifically reconsider the **JSON-family / structured
mutation** part of the design with real source evidence.

## What is already settled (the retained base — do NOT re-litigate)

The base mechanism for **arbitrary text files** is **managed-region markers**:
Toha writes comment-delimited `begin`/`end` marker lines around a rendered body;
on re-apply it finds the region by key and replaces only the span between the
markers. Idempotency and drift are structural: the end marker records a `sha2`
checksum of the body Toha last wrote, so re-apply is a no-op when unchanged, a
clean replace when the template body changed, and a refused **drift** when the
user edited inside the region. This is validated by real precedent — Ansible
`blockinfile` (`# {mark} ANSIBLE MANAGED BLOCK` BEGIN/END, locate-excise-splice,
write-only-if-differs). Read the full base design at
`/workspaces/worktrees/toha/design-content-injection/docs/technical-designs/second-release/content-injection/05-design.md`
and the grounding at `.../01-grounding.md`. Assume the marker base stays.

## The problem you must resolve (the shape that changed)

**Strict JSON has no comments.** So comment-delimited markers literally cannot be
written into a `.json` file — yet JSON-family files (package.json, tsconfig.json,
settings.json, composer.json, *.jsonc, *.json5) are among the most common
injection targets. The prior design deferred "structured merge" as out of scope
(assumed). Bob wants that reconsidered on evidence, and specifically wants `jaq`
(an embedded jq clone) evaluated. **Do not assume structured mode is out of
scope, and do not assume it is in scope — argue it from the evidence.**

## The research evidence you MUST use and cite (already gathered, on disk)

Read these before designing. Cite the exact repo revision + `file:line`/symbol
they contain when you make a claim about a tool:

- `/tmp/injection-research/rust-structured.md` — jaq, jsonc-parser, toml_edit
- `/tmp/injection-research/rust-serde.md` — serde_json (preserve_order), json5
- `/tmp/injection-research/cross-lang.md` — Ansible blockinfile/lineinfile, VS Code node-jsonc-parser, npm package-json
- `/tmp/injection-research/scaffolding-tools.md` — cookiecutter/copier/cruft/kickstart/scaffold
- Cloned source under `/workspaces/references/toha/{jaq,jsonc-parser,toml-rs,serde_json,json5-rs,ansible,node-jsonc-parser,npm-package-json}` — inspect directly if you need more.

Key established facts (verify/cite from the files above):
- **jaq** (jaq-json 2.0.3 @ `f167ad4`): RESERIALIZES. Discards `#` comments at lex
  time (`jaq-json/src/read.rs:10-19`), regenerates all output from a pretty
  printer (`jaq-json/src/write.rs:202-260`); IndexMap keeps in-memory order but
  output is fresh. It is a jq-language filter engine, not an editor.
- **jsonc-parser** (0.34.0 @ `c7d4cf5`): FORMAT-PRESERVING via its `cst` feature
  module (`src/cst/mod.rs`): `CstRootNode::parse/set_value`,
  `CstObject::get/append/insert/remove/sort_properties`, per-leaf `set_value`. Doc
  comment: "keeps every comment and every piece of whitespace, so a document can
  be edited and written back out with everything the author wrote still in
  place." Parses JSON5-style input by default; strict opt-out via `ParseOptions`.
  The JSON-family analogue of `toml_edit`.
- **toml_edit** (0.25.15 @ `e4b8bda`): FORMAT-PRESERVING gold standard (`Decor` on
  every node). The reference for "what good looks like."
- **serde_json / json5-rs**: RESERIALIZE / PARSE-ONLY; serde's data model has no
  trivia slot, so neither can preserve comments/whitespace.
- **VS Code node-jsonc-parser**: SURGICAL-CST-EDIT — `modify()` computes a minimal
  `{offset,length,content}` splice, preserving comments and untouched formatting
  (`src/impl/edit.ts:15-140`, `src/main.ts`). Same pattern as the Rust cst crate.
- **npm package-json**: RESERIALIZE-PRESERVE-INDENT — reserializes with
  `JSON.stringify` but threads back detected indent/newline; no comment
  preservation (JSON has none).

## Hard constraints (from grounding — every candidate must honor)

- Interview engine stays PURE; injection output is a pure function of `(Template,
  Completed, target state at apply time)`; staging replay reproduces the same
  result. Reading the target's current bytes is allowed only at plan/apply time,
  never in the engine.
- `plan-before-write`: `Plan::build` reads no target bytes (only the directory
  listing today); the read-modify-write happens at apply.
- Reuse `TargetPath` (rejects abs/`..`/`.git`/backslash/drive) and
  `has_symlink_component`. No new path type.
- No new timeout, permission, subprocess, or pinned-version-check mechanic
  (each needs separate explicit owner approval; you have none). **No application
  subprocess/CLI integration is authorized** — any structured engine must be an
  EMBEDDED crate, not a spawned `jq`/`dasel`/etc.
- No heuristic idempotency (re-matching injected text against drifted
  surroundings is forbidden).
- Must compose with whole-file writes inside one `Plan` and with repeated applies.
- Adding a crate dependency is allowed if justified (well-regarded, maintained);
  state the justification. Do not put a query engine on the critical path if a
  format-preserving editor serves better.

## Your champion direction

SEE YOUR PER-CANDIDATE PROMPT for the specific mechanism you must champion and
fully develop. Resolve every decision below for the JSON-family question, and
state how your mechanism composes with the retained marker base.

## Deliverable (write ONLY into your assigned output dir)

A focused design fragment (Markdown, `design.md`) covering:
1. **Author surface** — the `template.yml` shape for your mechanism (how a
   template author declares a JSON-family injection). Keep it a small surface;
   show it composing with the existing `inject:` marker list, not a rival syntax
   unless you justify it.
2. **Concrete BEFORE/AFTER examples** (this is required and central). For a
   GENERIC, non-identifying target file, show: the template config; the target
   file BEFORE; the target file AFTER first apply; the target file after a SECOND
   apply with the same answers (prove idempotency — what bytes change, if any);
   and the drift case (user edits the managed value/region — what happens). Use
   at least one real JSON-family example (`.json` and/or `.jsonc`). Show comment
   and key-order preservation (or loss) explicitly in the AFTER bytes.
3. **Ownership / drift / idempotency model** — precisely which bytes Toha owns,
   how a re-apply is a no-op when unchanged, how drift is detected or why it
   cannot be, and how this expresses as a mutation contract a downstream
   git-update design can consume.
4. **Data/type sketch + key signatures** (`not implemented` bodies; no runtime
   code) for the plan/apply additions and the pure resolve function.
5. **Error/results contract** — refusal cases (malformed doc, missing path,
   drift, unsupported format), exit-code behavior, `--force`/`--dry-run`
   interaction, atomicity.
6. **Format/comment/order preservation** — state exactly what your mechanism
   preserves and destroys, with cited evidence for the crate you rely on.
7. **Dependency justification** — name the crate(s), version, why adopt vs build,
   and where it sits (never on the pure engine path).
8. **Scope recommendation** — should this ship in the 0.2.0 slice (paired impl
   1031) now, or be a decided-but-deferred fast-follow with a reserved interface?
   Recommend, with reasoning.
9. **Rationale, alternatives, tradeoffs** — honestly confront your mechanism's
   worst weakness.

Use generic non-identifying example values (no real names/domains). Sketches
only — no production code, no schema edits. Cite real source evidence for every
tool claim. Do not read the other candidates' output; do not read any scoring
rubric.
