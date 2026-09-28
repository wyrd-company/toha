# Grounding addendum — JSON-family injection and structured mutation

Extends `01-grounding.md`. Records the JSON-family gap the base design leaves
open and the source-cited research into `jaq` and every alternative mechanism,
so the choice of mechanism is grounded in actual implementations rather than
description. Every tool claim cites a locally shallow-cloned repository under
`/workspaces/references/toha/` by revision and `file:line`/symbol.

## The gap the marker base leaves open

The base mechanism writes comment-delimited begin/end markers and owns the byte
span between them. It is format-agnostic and validated by long-standing
precedent — Ansible `blockinfile` is the same locate-excise-splice-write-if-
differs cycle (`ansible/lib/ansible/modules/blockinfile.py:41,323-324,334-367`,
revision `7ec731b`). Comment markers are valid in every text format that has a
comment syntax, **including JSONC and JSON5** (both permit `//`).

**Strict JSON has no comment syntax.** A begin/end marker written as a `//` line
makes a `.json` file invalid JSON. So the marker mechanism cannot inject into a
strict `.json` target, even though JSON-family files (package.json,
tsconfig.json, settings.json, composer.json) are among the most common injection
targets. The focused design therefore needs a second, structure-aware mechanism
for the whole JSON family.

## The three mechanism families observed in real implementations

Every tool that edits a JSON-family file in place falls into exactly one of
three families:

### 1. Marker-block (text splice) — what the base already does

- **Ansible `blockinfile`** (`blockinfile.py:334-367`, rev `7ec731b`): default
  marker `# {mark} ANSIBLE MANAGED BLOCK`; locates the exact marker-line pair,
  excises the old block including markers, splices the fresh block, writes only
  if the result differs. Atomic tempfile+move (`blockinfile.py:207-225`). The
  strongest precedent for Toha's base and its structural idempotency.
- **Ansible `lineinfile`** (`lineinfile.py:295-479`): single-line variant;
  idempotency keyed on a caller-supplied `regexp`/`search_string` — a *contract*
  the author must keep, weaker than `blockinfile`'s structural marker pair.
- **`scaffold`** (Go, `scaffold/app/scaffold/injector.go:27-82`, rev `475e9bc`):
  the only peer scaffolding tool with region injection — `inject: {path, at,
  mode: before|after, template}`, a line scanner inserting at the first `at`
  substring. But it has **no marker and no idempotency guard**: re-running
  re-inserts. It is a one-shot primitive, not an idempotent managed region.

Works on any text format with comments (so JSONC/JSON5), not strict JSON.

### 2. Surgical CST edit (format-preserving) — the structured answer

Parse the whole document into a concrete syntax tree that retains every comment
and whitespace token, mutate the targeted node, and re-emit; nothing outside the
touched node is regenerated.

- **`jsonc-parser`** (Rust, crate `0.34.0`, feature `cst`, rev `c7d4cf5`): the
  `cst` module (`jsonc-parser/src/cst/mod.rs`) states its own intent — "keeps
  every comment and every piece of whitespace, so a document can be edited and
  written back out with everything the author wrote still in place" (`mod.rs:1-5`).
  Surgical API: `CstRootNode::parse`/`set_value` (`mod.rs:1146,1212`),
  `CstObject::get/append/insert/remove/sort_properties` (`mod.rs:1764-1885`),
  per-leaf `set_value`; `Display for CstRootNode` (`mod.rs:1356`) re-emits the
  original bytes plus the edit. Parses JSON5-style input by default
  (single-quoted strings, hex, trailing/missing commas, `Infinity`/`NaN`), with
  a strict-JSON opt-out via `ParseOptions` (`src/lib.rs:99-124`). This is the
  JSON-family analogue of `toml_edit`.
- **`toml_edit`** (Rust, `0.25.15+spec-1.1.0`, rev `e4b8bda`): the gold-standard
  format-preserving editor — `DocumentMut` + `Item` + a `Decor` (prefix/suffix
  comments+whitespace) on every node (`toml_edit/src/repr.rs:187-190`,
  `document.rs:133`). Its own description: "Yet another format-preserving TOML
  parser." The reference for what a good structured editor looks like; not a JSON
  tool, but the pattern `jsonc-parser`'s CST mirrors.
- **VS Code `node-jsonc-parser`** (TypeScript, rev `dba4356`): the same pattern
  in the tool millions use to edit `settings.json`/`tsconfig.json`. `modify()`
  computes a minimal `{offset,length,content}` splice (`src/impl/edit.ts:15-140`);
  `applyEdits()` throws on overlapping edits (`src/main.ts:439`); `withFormatting`
  reformats only the changed range so comments elsewhere are never touched. Cross-
  language confirmation that surgical CST editing is the accepted way to edit
  JSON-family files without destroying them.

Works on JSON, JSONC, and JSON5 uniformly, strict JSON included, preserving
comments/order/whitespace.

### 3. Reserialize (parse → transform → print) — destroys formatting

Parse to a value, mutate, print fresh. The value model has no slot for
comments/whitespace, so they are lost; key order may be lost too.

- **`jaq`** (Rust, jaq-core `3.1.1` / jaq-json `2.0.3`, rev `f167ad4`): a
  pure-Rust jq filter engine. It **discards `#` comments at lex time**
  (`jaq-json/src/read.rs:10-19`) and **regenerates all output** from a pretty
  printer (`jaq-json/src/write.rs:202-260`). Its object is an `IndexMap`
  (`jaq-json/src/lib.rs:117`) so key order survives *in memory*, but output is
  freshly printed and `sort_keys` can reorder. No CST, no trivia channel, no
  ranges. `jaq-fmts` adds YAML/TOML/XML/CBOR/CSV, all via the same reserializing
  round trip. **jaq is a query/transform engine, not an editor.**
- **`serde_json`** (Rust, `1.0.151`, rev `afdf6fc`): reserializes always. The
  parser rejects comments and trailing commas (`src/de.rs:255-266`); the
  `preserve_order` feature swaps in `indexmap::IndexMap` (`src/map.rs:26,36`) for
  *insertion* order, not a durable link to source order; `PrettyFormatter`
  (`src/ser.rs:1945-2048`) always regenerates layout; `Value` (`src/value/mod.rs`)
  has no trivia slot.
- **`json5-rs`** (Rust, `1.3.1`, rev `fd55f90`): parses the JSON5 grammar but
  `skip_comment` (`src/de.rs:156-178`) discards comments like whitespace; it has
  no DOM and drives straight through serde, so any re-emit loses everything.
- **npm `package-json`** (JS, rev `a7dafdb`): reserializes with `JSON.stringify`
  but threads back the *detected* indent/newline captured at parse time via
  `Symbol.for('indent')`/`Symbol.for('newline')`
  (`json-parse-even-better-errors/lib/index.js:99-113`, rev `098b8d0`), writing
  only if the whole-file text differs (`lib/index.js:259`). No comment
  preservation (strict JSON has none). The pragmatic reserialize compromise the
  ecosystem accepts when it does not use a CST — and evidence that even the
  canonical package.json editor does not attempt surgical strict-JSON edits.

Root cause the serde family cannot preserve comments: serde's data model is a
closed set of primitives with no trivia slot. Any comment-preserving editor
(`toml_edit`, `jsonc-parser` cst) necessarily bypasses that model with its own
CST.

## Peer scaffolding tools: none inject structured content

Inspected the tools already cloned under `/workspaces/references/toha/`:

- `cookiecutter` (rev `c88fbe9`) and `kickstart` (rev `6f5fd80`): whole-file
  generation only (`cookiecutter/generate.py:175-260`; `kickstart/src/utils.rs:26-31`
  truncate-writes).
- `copier` (rev `01c6832`) and `cruft` (rev `33f6b72`): the "update" pattern is
  regenerate-old + regenerate-new template renders, `git diff` them, then
  `git apply --reject` / `git merge-file` 3-way with inline conflict markers
  (`copier/_main.py:1530-1669`; `cruft/_commands/update.py:194-278`). This is the
  prior art for the git-update sibling design (1066), **not** for in-file
  injection.
- `scaffold` (rev `475e9bc`): line-based marker-less injection (above).

None does a structured (parse/mutate/serialize) JSON/YAML/TOML merge into a
target. The field is split between marker injection (Ansible, now Toha) and
surgical CST edits (VS Code) — no scaffolding tool ships structured JSON merge.

## What this means for the design (Preserve / Change / Avoid / Risk delta)

- **Preserve.** The marker base and its structural idempotency — validated by
  Ansible `blockinfile` — for non-JSON text files.
- **Change.** Route JSON, JSONC, and JSON5 through one structured mechanism:
  `jsonc-parser` CST. JSON-family rules own a typed value at a path and do not
  add Toha marker comments.
- **Avoid.** `jaq` and every parse/transform/print approach as the structured
  mechanism. They destroy source fidelity and put a query language on the path
  for a bounded value update.
- **Risk.** Typed-path ownership has a weaker drift story than marked regions.
  Strict JSON cannot carry a checksum, so re-apply cannot distinguish an
  operator edit at the owned path from any other differing value. The contract
  is declarative convergence: Toha re-sets exactly the path it owns and
  preserves everything else.

## Conclusion carried into the arena and synthesis

The structured question was re-framed and re-run as a focused arena
(`02a-arena-json-family.md`) with three structurally distinct candidates —
embedded jaq, format-preserving jsonc-parser CST, and markers via JSONC — scored
by a readonly cross-judge and reconciled in
`04a-synthesis-json-family.md`. The final design selects jsonc-parser for the
JSON family and retains markers for other text formats.
