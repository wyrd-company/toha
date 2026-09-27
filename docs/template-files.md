---
docs: true
title: Generating files
order: 7
relationships:
  describes: toha
  references: template-format
---

After the interview, Toha plans the output from the template's source
directory. A dry run shows that plan without writing files:

```sh
toha apply ./note-template ./output --dry-run
```

When you apply the template, Toha writes the planned files under `./output`.
It does not overwrite an existing file unless you use `--force`.

## Render the source directory

By default, Toha renders every file under `template/` into the target at the
same relative path. If the source directory contains:

```text
template/
└── notes/
    └── {{ title | kebab }}.txt
```

and `title` is `Weekly notes`, Toha creates
`output/notes/weekly-notes.txt` when the target is `./output`.

Each path segment is a Jinja template. Toha also renders the contents of each
file. If a rendered file or directory name is empty, Toha does not generate
that path. Rendered paths cannot escape the target directory or enter its
`.git` directory.

## Exclude files with `ignore`

Use `ignore` for source files that should not appear in the output. Its values
are glob patterns relative to the source directory:

```yaml
ignore:
  - "**/*.orig"
  - "drafts/**"
```

Those files are skipped. `ignore` is useful when `source: .` includes support
material or when you keep draft files beside generated files.

## Copy files unchanged with `static`

Toha normally renders file contents. Use `static` when a file must be copied
byte for byte, such as an image or another binary asset:

```yaml
static:
  - "assets/**"
```

These glob patterns are also relative to the source directory. A static
file's contents are not rendered; its path still follows the normal path
rendering rules.

## Make one file for each item

Use `files` when one support file should produce several output files. For
example, suppose `tags` is a list collected by a repeated question and the
template folder contains `parts/tag.txt`:

```yaml
files:
  - each: tags as tag
    source: parts/tag.txt
    path: "tags/{{ tag | kebab }}.txt"
```

Toha renders `parts/tag.txt` once for each tag. The `each` expression has the
form `<list expression> as <item name>`. In this example, `tag` is the current
item. Toha makes it available while rendering both the support file and the
output `path`.

`source` names a support file relative to the template root. `path` names an
output path relative to the target directory. A file rule's `when` expression
can skip the entire rule:

```yaml
files:
  - each: tags as tag
    source: parts/tag.txt
    path: "tags/{{ tag | kebab }}.txt"
    when: tags | length > 0
```

An empty list also produces no files. The item name after `as` must be a
lowercase Jinja identifier and cannot match a data key, question id, or
computed id.

For commands to run after writing files, continue with
[Hooks and messages](/docs/toha/template-hooks).
