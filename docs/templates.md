---
docs: true
title: Authoring templates
order: 3
relationships:
  describes: toha
  references: template-format
---

A Toha template is a folder with a `template.yml` file and files to generate.
Toha asks the questions in `interview`, builds a plan, and writes the rendered
files to the target directory.

## Folder layout

```text
note-template/
├── template.yml
├── template/
│   └── {{ slug }}.txt
└── data/
    └── choices.yml
```

The `template/` directory is the default **source** directory. Each file in it
becomes a file under the target directory at the same relative path. Toha
renders every path segment and file body as Jinja. Put support files such as
included data or inputs to repeated file rules outside the source directory.

Set `source` to another path relative to the template root to use a different
source directory. `source: .` uses the template root; Toha still excludes
`template.yml` itself. Paths in `template.yml` are relative to the template
root, except `ignore` and `static` globs, which are relative to the source
directory.

## Template metadata and data

```yaml
name: note
description: A note with a title
source: template

data:
  topics: !include data/choices.yml
  category: general
```

`name` is required. It is the template's short name: lowercase letters and
digits separated by single hyphens. `description`, `source`, and `data` are
optional. Every top-level `data` key becomes a Jinja variable. Data can hold
strings, numbers, booleans, arrays, and objects.

Use `!include <path>` on any YAML value to load a support file inside the
template root. `.yml` and `.yaml` files are parsed as YAML, `.json` as JSON,
and `.toml` as TOML. For example, `topics: !include data/choices.yml`
loads that file as the value of `topics`.

The other top-level options are `interview`, `files`, `ignore`, `static`,
`hooks`, and `messages`. The next pages explain them with examples:
[Questions and values](/docs/toha/template-interviews) and
[Generated files and hooks](/docs/toha/template-files).

## Try your template

From the directory that contains `note-template/`:

```sh
toha apply ./note-template ./output --dry-run
toha apply ./note-template ./output
```

A dry run shows generated files and hooks without changing the target. See the
[basic](https://github.com/wyrd-company/toha/tree/main/docs/examples/basic),
[branching](https://github.com/wyrd-company/toha/tree/main/docs/examples/branching), and
[generated files](https://github.com/wyrd-company/toha/tree/main/docs/examples/generated-files)
examples for complete folders.
