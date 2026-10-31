---
title: Generating files
order: 7
product: toha
relationships:
  describes: toha
  references: template-format
---

As a template author, put files under the source directory to make Toha
generate them. Use `ignore`, `static`, and `files` in `template.yml` when a file
needs different handling.

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

## Create directories

Use `directories` to create directories without placeholder files:

```yaml
directories:
  - path: cache/empty
  - each: tags as tag
    path: "tags/{{ tag | kebab }}"
    when: tags | length > 0
```

Each rule has a rendered `path` relative to the target. `each` is optional;
without it, Toha creates one directory. `when` can skip a rule. Toha creates
parents, combines repeated paths, and keeps existing directories and contents.
A file or symlink in the directory path is an error, including with `--force`.
Paths cannot escape the target, enter `.git`, or use the applied record path
(or its descendants), including a custom `TOHA_RECORD` path. Dry runs list
missing directories without creating them. Apply reports only directories it
creates. If baseline or update fails, Toha removes newly created empty
directories and keeps existing directories and contents added by another writer.

Apply, baseline and update create these directories. Toha does not remove a
directory when its rule is removed or skipped. Directories do not carry file
ownership in `.toha.json`. Git does not preserve empty directories on clone;
apply or update the template to create them again.

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

If file generation cannot evaluate a value, the error names the source file,
the exact field, and the authored value. For example:

<!-- rumdl-disable MD013 -->

```text
/templates/sample/part.txt: template error in files[0].path `{{ item.name }}.txt`: item has no attribute named name
```

<!-- rumdl-enable MD013 -->

The fields are `files[i].when`, `files[i].each`, and `files[i].path`. A fault
in a normal source-tree path uses `path` and shows only the path segment that
failed. A file-content fault continues to name the source file.

To change part of a file that already exists instead of writing a whole file,
see [Injecting into existing files](/docs/toha/template-injection). For commands
to run after writing files, continue with
[Hooks and messages](/docs/toha/template-hooks).

## Reserve sections for user edits

A rendered file can leave named sections for the caller to edit. Put a matching
marker pair on separate lines in the source file:

```toml
version = 1
# >>> toha:user settings >>>
enabled = false
# <<< toha:user settings <<<
```

Toha generates the whole file. On first creation, the section contains the
rendered default (`enabled = false` here). On later applies and updates, Toha
copies the existing section body byte for byte, while the template supplies the
marker lines and everything outside them. Changing a template's section default
does not replace an existing body, including an empty body. The key connects a
section across template versions, so its position may change.

Keys use lowercase letters, digits, `_`, and `-`, and are unique within each
file. Use the file's comment syntax around the markers, such as
`// >>> toha:user settings >>>` in Rust or
`<!-- >>> toha:user settings >>> -->` in Markdown. The end marker uses `<<<` at
both ends. Both marker lines must have the same prefix and suffix. Sections
cannot nest. Malformed pairs stop the apply before files are written, including
with `--force`.

New sections use their template defaults. To remove or rename an existing
section, first move any content you want to keep and remove its markers in the
target. While the file remains generated, a template that drops an existing
section otherwise fails, so an update
does not silently discard user content. Preserve the marker pair when editing a
section; deleting both markers makes the section new on the next apply.

This works for rendered source files and `files` rules. `static` files are
copied unchanged. Ordinary whole-file conflict checks still apply: use
`apply --force` when replacing an existing file, or commit edits and use
`apply --update` for a recorded application. File removal follows the normal whole-file update rules.
Force preserves user sections. Injections in the same file must stay outside
user-section bodies.
For the inverse ownership, see [managed injection regions](template-injection.md).
