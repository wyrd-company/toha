# Cross-tool precedent: managed-block injection and JSON in-place editing

Research for Toha's content-injection design. All citations are against locally
shallow-cloned repos under `/workspaces/references/toha/`.

---

## 1. Ansible core — `blockinfile` (MARKER-BLOCK)

- Repo: https://github.com/ansible/ansible
- Local clone: `/workspaces/references/toha/ansible` (sparse-checked out to `lib/ansible/modules`)
- Cloned revision: `7ec731bfd2dbfd7b5a11ac53bf32ad2a4bf57ed4` (2026-09-25 05:56:20 +1000)
- File: `lib/ansible/modules/blockinfile.py`

### Mechanism: MARKER-BLOCK

The module wraps an arbitrary multi-line text block between two single-line
marker strings and treats "the region between the markers" as the managed
region — the strongest available precedent for Toha's managed-region-marker
base.

**Default marker template** (`blockinfile.py:41`):
```python
default: '# {mark} ANSIBLE MANAGED BLOCK'
```
`{mark}` is substituted with `marker_begin` (default `BEGIN`, `blockinfile.py:80`)
and `marker_end` (default `END`, `blockinfile.py:87`), producing e.g.:
```
# BEGIN ANSIBLE MANAGED BLOCK
...block content...
# END ANSIBLE MANAGED BLOCK
```
Built at `blockinfile.py:323-324`:
```python
marker0 = re.sub(r'{mark}', params['marker_begin'], marker) + line_separator
marker1 = re.sub(r'{mark}', params['marker_end'], marker) + line_separator
```
The docstring (`blockinfile.py:36-38`) is explicit about the idempotency
contract: the `{mark}` token must appear in the marker, markers must not be
multi-line, or "the block will be repeatedly inserted on subsequent playbook
runs."

**Idempotency — find existing block by markers, replace in place**
(`blockinfile.py:334-367`, function `main()`):
```python
n0 = n1 = None
for i, line in enumerate(lines):
    if line == marker0:
        n0 = i
    if line == marker1:
        n1 = i

if None in (n0, n1):
    n0 = None
    # ... markers not found: fall through to insertafter/insertbefore/EOF logic
elif n0 < n1:
    lines[n0:n1 + 1] = []          # existing block found in normal order: cut it out
else:
    lines[n1:n0 + 1] = []          # markers reversed (corrupted/edited file): cut out, self-heal
    n0 = n1
...
lines[n0:n0] = blocklines           # (re)insert fresh block at n0
```
This is a full "locate by exact marker-line match, excise old region including
both markers, splice new region back in at the same position" cycle — every
run recomputes the block and only reports `changed=True` if the resulting text
differs (`blockinfile.py:402-413`, comparing `original == result`). This gives
byte-for-byte idempotency without needing to diff the block content itself.

**First-time placement — insertafter/insertbefore/EOF**
(`blockinfile.py:313-321, 341-362`):
```python
if insertbefore is None and insertafter is None:
    insertafter = 'EOF'
...
if insertre is not None:
    ... n0 = <line after/before last regex match> ...
elif insertbefore is not None:
    n0 = 0  # insertbefore=BOF
else:
    n0 = len(lines)  # insertafter=EOF
```
`insertafter`/`insertbefore` accept a regular expression (with an optional
`(?m)` multiline flag, `blockinfile.py:344-350`) or the sentinels `EOF`/`BOF`.
If the regex has no match, insertion falls back to `EOF` (documented at
`blockinfile.py:52-53, 61`).

**Optional blank-line padding** (`blockinfile.py:378-392`, `prepend_newline`/
`append_newline` params, added in 2.16) inserts a blank line before/after the
block only if one isn't already present — avoids blank-line creep on repeat
runs.

**Write path is atomic**: content is written to a tempfile then
`module.atomic_move()`'d into place (`blockinfile.py:207-225`, `write_changes`),
with optional `backup_local()` before the move (`blockinfile.py:417-418`) and
an optional external `validate` command run against the tempfile before commit
(`blockinfile.py:214-223`).

### Idempotency approach (summary)
Deterministic recompute-and-compare: every run rebuilds the full expected
block content, locates the old block (if any) purely by literal marker-line
equality (`line == marker0`), replaces the byte range between (and including)
the markers, and only writes if the final text differs from the original.

---

## 2. Ansible core — `lineinfile` (single-line variant of the same family)

- Same repo/revision as above.
- File: `lib/ansible/modules/lineinfile.py`
- Function: `present()`, `lineinfile.py:295-479`

### Mechanism
Manages a single line rather than a block. Idempotency key is a `regexp`
(`lineinfile.py:31-44`) or literal `search_string` (`lineinfile.py:45-55`,
added 2.11) that must match "both the initial state of the line as well as its
state after replacement ... to ensure idempotence" (`lineinfile.py:39-40` —
this doc comment is the module's own idempotency contract, put in the user's
hands rather than enforced structurally, unlike `blockinfile`'s marker pair).

Matching precedence, coded directly in `present()`:
1. `regexp` match wins if present (`lineinfile.py:348-355`).
2. Else literal `search_string` substring match (`lineinfile.py:358-365`).
3. Else exact full-line match against the literal `line` value, tracked via
   `exact_line_match` (`lineinfile.py:369-373`).
4. Else fall through to `insertafter`/`insertbefore` positional search
   (`lineinfile.py:375-386`), same `EOF`/`BOF` sentinels as `blockinfile`.

Replace-in-place happens by index (`lineinfile.py:439-442`):
```python
elif lines[index[0]] != new_line:
    lines[index[0]] = new_line
    msg = 'line replaced'
    changed = True
```
`backrefs` (`lineinfile.py:70-83`) lets `line` be a regex backreference
template expanded via `match.expand(line)` (`lineinfile.py:393-394`) — i.e. the
replacement can be derived from the matched line's own captured groups.

### Idempotency approach (summary)
Content-addressed by regex/substring/exact-match against existing lines, not
by structural marker. Weaker guarantee than `blockinfile`: correctness depends
on the caller writing a `regexp` that matches both pre- and post-state.

---

## 3. VS Code `jsonc-parser` (SURGICAL-CST-EDIT)

- Repo: https://github.com/microsoft/node-jsonc-parser
- Local clone: `/workspaces/references/toha/node-jsonc-parser`
- Cloned revision: `dba4356548089b594dd12b324d0547e1c2c5ddb8` (2026-09-18 10:01:42 -0700)
- Files: `src/impl/edit.ts`, `src/main.ts`

### Mechanism: SURGICAL-CST-EDIT

`modify()` (`src/main.ts:414-416`) delegates to `setProperty()`
(`src/impl/edit.ts:15-140`), which:

1. Parses the **whole document into a CST/AST** via `parseTree()` (comments
   and all — `src/impl/edit.ts:18`).
2. Walks the `JSONPath` to find the target `Node` (`findNodeAtLocation`,
   `src/impl/edit.ts:24`).
3. Computes a **minimal text splice** — an `{offset, length, content}` triple —
   rather than rewriting the document. E.g. setting an existing property value
   (`src/impl/edit.ts:65-68`):
   ```ts
   // set value of existing property
   return withFormatting(text, { offset: existing.offset, length: existing.length, content: JSON.stringify(value) }, options);
   ```
   Inserting a new object property computes the insertion point relative to
   the previous sibling and injects a leading comma as needed
   (`src/impl/edit.ts:73-84`). Deleting a property computes the byte range to
   remove including the correct side's comma so the result stays valid JSON
   (`src/impl/edit.ts:49-64`).
4. The `Edit` type (`src/main.ts:315-328`) is exactly `{offset, length,
   content}` — "offsets refer to the original state of the document"
   (`src/main.ts:303`). `EditResult = Edit[]` (`src/main.ts:310`).
5. `applyEdits()` (`src/main.ts:425-...`) sorts edits by offset and applies them
   back-to-front over the original string, throwing `Error('Overlapping
   edit')` (`src/main.ts:439`) if two edits collide — a structural guard
   against unsafe concurrent edits.
6. `withFormatting()` (`src/impl/edit.ts:142-174`) optionally re-runs the
   `format()` formatter (`src/impl/format.ts`) over **only the changed range**
   (widened to whole lines, `src/impl/edit.ts:150-159`) and folds the
   formatting edits back into one consolidated `Edit`, so untouched parts of
   the file — including comments — are never touched at all.
7. `applyEdit()` itself is a one-line string splice
   (`src/impl/edit.ts:176-178`):
   ```ts
   export function applyEdit(text: string, edit: Edit): string {
       return text.substring(0, edit.offset) + edit.content + text.substring(edit.offset + edit.length);
   }
   ```

### Idempotency approach (summary)
Not marker-based — idempotency comes from the edit being derived from a fresh
parse of current state each call: if the target value already equals the
desired value, the caller can skip calling `modify` (jsonc-parser itself does
not diff-and-skip; the offset/length/content edit it returns will always
reflect current AST location, so applying it converges to the same file every
time it's invoked with the same inputs). Because the edit is computed from the
CST, comments, key order, and unrelated formatting are structurally
preserved — nothing outside the computed range is ever rewritten.

---

## 4. npm `package-json` (RESERIALIZE-PRESERVE-INDENT via `json-parse-even-better-errors`)

- Repo: https://github.com/npm/package-json
- Local clone: `/workspaces/references/toha/npm-package-json`
- Cloned revision: `a7dafdbdee34c96c672862fadde9ae5b594c938e` (2026-06-18 09:25:49 -0700)
- Companion dependency cloned to verify the indent-capture mechanism:
  https://github.com/npm/json-parse-even-better-errors
  Local clone: `/workspaces/references/toha/json-parse-even-better-errors`
  Cloned revision: `098b8d00e72e4807adba733c2cdde686b2b9bf82` (2026-06-18 08:48:16 -0700)

### Mechanism: RESERIALIZE-PRESERVE-INDENT

Unlike jsonc-parser, `package-json` does **not** do a surgical CST edit — it
fully reparses, mutates the plain JS object, and **reserializes the whole
document** with `JSON.stringify`. Since `package.json` has no comments, the
only preservation concern is indentation and newline style (and, incidentally,
key order via native JS object property order).

**Read/parse path** (`lib/read-package.js:17-25`, `parse()`) calls
`json-parse-even-better-errors` (`lib/index.js:3, 163`,
`this.#manifest = parse(data)`). That library's `parseJson()`
(`json-parse-even-better-errors/lib/index.js:99-113`) does the actual
detection:
```js
const match = txt.match(EMPTY) || txt.match(FORMAT) || [null, '', '']
result[NEWLINE] = match[1] ?? DEFAULT_NEWLINE
result[INDENT] = match[2] ?? DEFAULT_INDENT
```
using `Symbol.for('indent')` / `Symbol.for('newline')`
(`json-parse-even-better-errors/lib/index.js:3-4`) as non-enumerable-looking
(but actually own, symbol-keyed) properties stashed directly on the parsed
object — regexes `FORMAT`/`EMPTY` at
`json-parse-even-better-errors/lib/index.js:14-15` sniff the first line break
and indent whitespace of the raw source text.

**Write path** (`lib/index.js:239-264`, `PackageJson.save()`):
```js
const {
  [Symbol.for('indent')]: indent,
  [Symbol.for('newline')]: newline,
  ...rest
} = this.content

const format = indent === undefined ? '  ' : indent
const eol = newline === undefined ? '\n' : newline

const content = sort ? packageSort(rest) : rest

const fileContent = `${
  JSON.stringify(content, null, format)
}\n`
  .replace(/\n/g, eol)

if (fileContent.trim() !== this.#readFileContent.trim()) {
  const written = await writeFile(this.filename, fileContent)
  ...
}
```
So the mutated object is fully re-stringified (`JSON.stringify(content, null,
format)`) using the *originally observed* indent string and newline sequence,
then only written if the trimmed result differs from the original file
content — a whole-file idempotency check, not a regional one.

### Idempotency approach (summary)
Whole-document diff-before-write (`fileContent.trim() !== this.#readFileContent.trim()`,
`lib/index.js:259`) after a full reserialize; "preservation" is limited to
indent width/newline style detected once at read time and threaded back
through at write time via symbol-keyed properties on the parsed object —
there is no comment or formatting preservation because JSON (unlike JSONC)
has none to preserve.

---

## Summary table

| Tool | Language | Approach | Idempotency mechanism | Preserves comments | Notes |
|---|---|---|---|---|---|
| Ansible `blockinfile` | Python | MARKER-BLOCK | Locate exact marker-line pair, excise old block (incl. markers), recompute+splice fresh block, write only if result text differs | N/A (plain text, markers ARE the "comment") | Strongest precedent for Toha's managed-region-marker design. `{mark}` token requirement + no-multiline-marker rule are its own explicit idempotency contract. Self-heals reversed markers. Atomic write via tempfile+move, optional backup + external `validate` hook. |
| Ansible `lineinfile` | Python | Single-line MARKER-BLOCK analog (regex/substring/exact-match, no structural marker) | Caller-supplied `regexp`/`search_string` must match both pre- and post-state; `backrefs` derives replacement from match groups | N/A (plain text) | Weaker guarantee than `blockinfile` — correctness depends on caller's regex being idempotent by construction, not enforced structurally. |
| VS Code `jsonc-parser` (`modify`/`applyEdits`) | TypeScript | SURGICAL-CST-EDIT | Edit computed fresh from current AST location each call (`{offset,length,content}`); `applyEdits` throws on overlapping edits | Yes — full CST parse, only the touched range (widened to whole lines) is replaced | Cleanest "targeted edit, don't touch the rest" precedent for editing structured formats (would generalize to any file with an AST/CST, e.g. Toha's own future structured-format injectors). `Edit`/`EditResult` types are the reusable vocabulary. |
| npm `package-json` (+ `json-parse-even-better-errors`) | JavaScript | RESERIALIZE-PRESERVE-INDENT | Full reserialize via `JSON.stringify`, write only if whole-file text (trimmed) differs from original | No (JSON has none) — only indent width + newline style preserved via `Symbol.for('indent')`/`Symbol.for('newline')` sniffed at parse time | Shows the pragmatic compromise when no CST/AST-preserving editor is used: reparse, mutate the object, reserialize, but thread back the two purely-cosmetic properties (indent, EOL) that `JSON.stringify` can't infer on its own. |
