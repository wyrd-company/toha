<!-- ---
relationships:
  realizes: template-format
--- -->
# Injecting into existing files

`files` and the source subdirectory write whole files. `inject` changes one
bounded part of a file that already exists, and leaves the rest alone. Each rule
names its target with `into`, a path relative to the target directory, and is one
of two kinds: a visible **region** in a text file, or a typed **value** at a path
in a JSON, JSONC, or JSON5 file.

Toha resolves every injection in memory before it writes anything, so a failure
leaves every file unchanged, and it replaces each changed file atomically. A rule
that changes nothing writes nothing. A rule fails when its target is missing
unless you set `create: true`.

## A managed region

A `region` rule owns a block of lines, marked by comments and a checksum:

```yaml
inject:
  - into: "config/app.toml"
    region: features
    content: |
      analytics_enabled = true
      analytics_sample_rate = 0.1
    anchor: { after: "[application]", occurrence: only }
```

The first apply places the block after the anchor line:

```toml
[application]
# >>> toha:region features >>>
analytics_enabled = true
analytics_sample_rate = 0.1
# <<< toha:end features sha256:a7f3d… <<<
name = "sample-service"
```

Toha owns the marker pair and the bytes between them. On later applies it finds
the markers rather than the anchor, so the block does not duplicate. If the
template body is unchanged, the apply is a no-op. If you change the template body,
Toha replaces only that block. If someone edits inside the block, that is drift:
`apply` refuses it, prints `config/app.toml (features)`, and names the same
command with `--force`. `--force` restores the owned body and never invents a
missing anchor.

The block's body comes from an inline `content` template or, with `source`, the
rendered body of a support file. The comment style is inferred from the target's
extension; set it explicitly with `marker` — a string is a line-comment prefix,
and `{ open, close }` is a block comment:

```yaml
inject:
  - into: "index.html"
    region: analytics
    marker: { open: "<!--", close: "-->" }
    content: "<script src=\"/analytics.js\"></script>"
```

`anchor.occurrence` is `only` by default, which requires exactly one matching
line; use `first` or `last` to pick among several. With no anchor, the block is
appended at the end of the file.

## A typed value in a JSON-family file

A `struct` rule owns one value at a path in a `.json`, `.jsonc`, or `.json5`
file:

```yaml
inject:
  - into: "package.json"
    struct:
      path: "scripts.build"
      value: "tsc"
```

Given this file:

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js"
  }
}
```

the apply adds the owned value and keeps everything else:

```json
{
  "name": "sample-app",
  "scripts": {
    "start": "node index.js",
    "build": "tsc"
  }
}
```

Toha owns the value at `scripts.build`. A second apply changes no bytes. If
someone changes that owned value, the next apply converges it back to `"tsc"` —
no `--force` needed, because a declared value is not drift. Every unrelated key,
and in JSONC and JSON5 every comment, key order, quote style, and trailing comma,
is preserved. Toha adds no marker comment to a JSON-family file.

`path` is dot-separated object keys and bracketed array indexes, such as
`routes[0].name`; a backslash escapes `.`, `[`, `]`, and `\` in a key. Missing
object parents are created. An array in the path must already exist and each
index must be in range. `value` is any JSON type; its string leaves are
templates, so `value: "{{ name }}"` renders and stays a JSON string, while
`value: true` or `value: 42` keeps its type.

A `region` rule cannot target a `.json`, `.jsonc`, or `.json5` file — use
`struct` for a typed value, or a whole-file `files` rule. Whole-file JSON
generation is unchanged.

## Previewing

`apply --dry-run` reports what each rule would do without writing:

```text
inject package.json (scripts.build)
update config/app.toml (features)
```

`inject` marks a region or value it would place, `update` one it would change,
and the owner in parentheses is the region key or the typed path.

For commands to run after writing files, continue with
[Hooks and messages](/docs/toha/template-hooks).
