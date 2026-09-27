---
docs: true
title: Authoring templates
order: 3
relationships:
  describes: toha
  references: template-format
---

A Toha template is a folder that contains `template.yml` and the files you
want Toha to generate. `template.yml` names the template and defines any
questions. Toha renders the files into a target directory using the answers.

## Make a first template

Create this folder:

```text
note-template/
├── template.yml
└── template/
    └── {{ title | kebab }}.txt
```

Put this in `note-template/template.yml`:

```yaml
name: note
interview:
  - id: title
    type: text
    prompt: Note title
    required: true
```

Put this in `note-template/template/{{ title | kebab }}.txt`:

```jinja
# {{ title }}
```

From the directory containing `note-template`, preview the result:

```sh
toha apply ./note-template ./notes --dry-run
```

Enter a title such as `Weekly notes`. Toha plans a file named
`notes/weekly-notes.txt` containing `# Weekly notes`. Remove `--dry-run` to
write it:

```sh
toha apply ./note-template ./notes
```

The name between `{{` and `}}` comes from the question's `id`. `| kebab`
changes the title into a file-friendly name. See
[Jinja and values](/docs/toha/template-jinja) for template syntax and Toha's
filters.

## Name and describe the template

Every `template.yml` needs `name`. This is the **short name** Toha shows when
listing installed templates. Use lowercase letters and digits, separated by
single hyphens. For example, `meeting-note` is valid.

`description` is optional text that explains what the template creates:

```yaml
name: meeting-note
description: Create a note with a title and optional summary
```

Toha uses the folder's location as its source address, separately from this
short name. See [Managing templates](/docs/toha/templates-management) for
formal names and aliases.

## Choose which files to render

By default, Toha reads files from the `template/` subdirectory. This is the
**source directory**. It writes each file under the target at the same
relative path. Toha renders every path segment and each file's contents.

Set `source` when your files live in a different directory:

```yaml
name: meeting-note
source: files
```

Here Toha renders files under `files/`. The `source` path is relative to the
template root, the folder containing `template.yml`. `source: .` uses the
whole template root, although Toha never renders `template.yml` itself. If
you use `source: .`, use `ignore` to exclude support files you do not want to
copy. See [Generating files](/docs/toha/template-files) for `ignore`, `static`,
and repeated file rules.

## Supply static data

Put values under `data` when files or questions need information that does not
come from the person running the template:

```yaml
data:
  category: personal
  priorities: [ low, normal, high ]
```

Each top-level key becomes a variable. A file can use `{{ category }}`; a
question can use `priorities` to offer choices. Values can be strings,
numbers, booleans, arrays, or objects. A top-level key must use the same
identifier form as a question id, such as `priorities` or `default_category`.

For larger data, keep a file beside `template.yml` and load it with
`!include`:

```text
note-template/
├── template.yml
├── data/
│   └── priorities.yml
└── template/
    └── note.txt
```

```yaml
data:
  priorities: !include data/priorities.yml
```

The path is relative to the template root and must stay inside it. You can
use `!include` on any value in `template.yml`, not only `data`. Toha parses
`.yml` and `.yaml` files as YAML, `.json` as JSON, and `.toml` as TOML.
This is different from a Jinja `include` statement in a generated file.

## Continue building

Use `interview` to ask questions; [Asking questions](/docs/toha/template-interviews)
explains each question option. Use
[Controlling the interview](/docs/toha/template-flow) for conditions and
calculated values. Use [Generating files](/docs/toha/template-files) to choose
what Toha writes, and [Hooks and messages](/docs/toha/template-hooks) for
commands and apply-time messages.

The [basic](https://github.com/wyrd-company/toha/tree/main/docs/examples/basic),
[branching](https://github.com/wyrd-company/toha/tree/main/docs/examples/branching), and
[generated files](https://github.com/wyrd-company/toha/tree/main/docs/examples/generated-files)
examples show complete template folders.
