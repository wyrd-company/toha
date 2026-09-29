---
docs: true
title: Jinja and values
order: 6
relationships:
  describes: toha
  references: template-format
---

Toha uses [MiniJinja](https://docs.rs/minijinja/latest/minijinja/) to render
text and evaluate expressions. MiniJinja follows the
[Jinja template language](https://jinja.palletsprojects.com/en/stable/templates/).
This page explains the values and additions available inside a Toha template.
The linked language guides cover loops, conditions, filters, and other general
Jinja syntax.

## Where values come from

A template can define static values under `data`:

```yaml
data:
  category: personal
  priorities: [ low, normal, high ]
```

Each top-level key is a Jinja variable. Interview question answers and
computed values also become variables. For example, after asking a question
with `id: title`, a generated file can contain:

```jinja
# {{ title }}
Category: {{ category }}
```

If the answer to `title` is `Weekly notes`, the first line becomes
`# Weekly notes`. `category` comes from `data`.

Top-level `data` keys, question ids, and computed ids share one namespace.
They must each match the same identifier pattern: start with a lowercase
letter or underscore, followed by lowercase letters, digits, or underscores.
No two can have the same name. A name introduced in the interview is available
to later nodes and to file rendering.

## Text templates and typed expressions

Use `{{ ... }}` when a field or file needs text. Toha renders string fields,
file contents, and each file path segment as text:

```yaml
prompt: "File name for {{ title }}"
path: "notes/{{ title | kebab }}.txt"
```

Some fields need a boolean, number, or list instead of text. For those fields,
you can supply a literal value or a quoted Jinja expression that returns the
right type:

```yaml
required: true
when: include_summary
options: "priorities | sort"
```

`when`, `computed`, and `format` are always expressions. Write the expression
directly, without `{{ ... }}`. For example, `when: include_summary` uses a
boolean value, while `prompt: "Summary for {{ title }}"` renders text.

## Case filters

Toha adds six case filters. The input `Weekly Notes` produces these results:

| Filter | Result |
| --- | --- |
| `kebab` | `weekly-notes` |
| `snake` | `weekly_notes` |
| `camel` | `weeklyNotes` |
| `pascal` | `WeeklyNotes` |
| `constant` | `WEEKLY_NOTES` |
| `title` | `Weekly Notes` |

Use them in text templates, expressions, or generated file names. For example,
`{{ title | kebab }}.txt` produces `weekly-notes.txt`.

## JSON and YAML output

`tojson` serializes a value to compact JSON. Pass an indent when you want
formatted JSON. `toyaml` serializes a value as YAML without a leading `---` or
a final document newline, so you can place it inside a larger file.

```jinja
JSON: {{ priorities | tojson }}
YAML:
{{ priorities | toyaml | indent(2) }}
```

Object keys appear in sorted order. If a value itself ends with a newline,
`toyaml` preserves that newline.

## The interview time

`now()` returns the instant captured when the interview started. Use
`dateformat` to format it in that instant's time zone:

```jinja
Created: {{ now() | dateformat }}
Year: {{ now() | dateformat('%Y') }}
```

Without an argument, `dateformat` uses `%Y-%m-%d`. Its format string uses
[strftime syntax](https://docs.rs/jiff/latest/jiff/fmt/strtime/index.html#conversion-specifications).
The same interview instant is used throughout one run, so several calls to
`now()` agree.

## Toha context values

Toha supplies seventeen reserved variables before the first question. Each name
begins with `toha_` and is present at every render surface — questions, computed
values, messages, hooks, file paths, and file bodies. An unavailable value is
Jinja `none` (never undefined). These names are reserved: a `data` key, question
id, computed id, or `each` binding may not reuse one, but other `toha_` names
remain free.

| Name | Type | Value |
| --- | --- | --- |
| `toha_target_name` | string or none | Final component of the target directory; `none` for a filesystem root or non-Unicode name. |
| `toha_template_name` | string | Short name from the selected `template.yml`. |
| `toha_template_formal_name` | string | Stable formal name of the selection, never an alias or short spelling. |
| `toha_template_aliases` | list of strings | Effective aliases of a selected named template in registry order; `[]` for a folder, Git address, or bundled selection. |
| `toha_template_source` | string or none | Source of the selected named entry; `none` when the selection has no registry entry. |
| `toha_host_os` | string | Lowercase OS name from Rust's target vocabulary, such as `linux`. |
| `toha_host_arch` | string | Architecture from Rust's target vocabulary, such as `x86_64`. |
| `toha_host_os_name` | string or none | Linux `/etc/os-release` `NAME`; `none` elsewhere or when unavailable. |
| `toha_host_os_id` | string or none | Linux `/etc/os-release` `ID`; `none` elsewhere or when unavailable. |
| `toha_host_os_id_like` | list of strings | Linux `/etc/os-release` `ID_LIKE`, split on whitespace; `[]` elsewhere or when unavailable. |
| `toha_is_admin` | boolean | Whether the run has effective administrative authority. |
| `toha_is_interactive` | boolean | Whether the originating driver could prompt a human. |
| `toha_env_user` | string or none | Captured user name. |
| `toha_env_hostname` | string or none | Captured host name. |
| `toha_env_editor` | string or none | Captured `EDITOR`. |
| `toha_env_shell` | string or none | Captured `SHELL`. |
| `toha_env_visual` | string or none | Captured `VISUAL`. |

```jinja
{% if toha_is_interactive %}
Preparing {{ toha_template_name }} for {{ toha_target_name }}.
{% endif %}
{% if toha_env_editor is not none %}
editor = {{ toha_env_editor }}
{% endif %}
```

### The trusted environment values

The five `toha_env_` values — user, host name, editor, shell, and visual — are
the only values read from the environment, and only under trust. Before it asks
a question, `stage` analyzes every Jinja surface. If any of them can be
referenced, `stage` records nothing and refuses until you re-run with `--trust`:

```console
# No reference and no flag: the values are recorded unavailable (none).
toha stage sample ./output --async

# An explicit grant records all five once, even before a reference exists.
toha stage sample ./output --async --trust

# Later batches reuse the recorded decision and values; continue has no flag.
toha continue ./output answers.json
```

`apply` with a template grants the same access through an explicit `--trust` or a
current registry approval that matches the template's reviewed hooks. `continue`
never re-reads the environment: it replays the recorded snapshot, so changing
`EDITOR` or the host between batches does not change a value. A template folder
edited after staging renders its current bytes under the recorded decision: with
no grant a newly added `toha_env_` reference is `none`; with a grant it reads the
recorded value.

A staged interview created before these values existed keeps the earlier
behavior: none of the seventeen names are injected, reserved, or shown by
`debug()`.

## Supported Jinja features

Toha includes MiniJinja's built-in filters, tests, and functions, the `tojson`
filter, macros (`{% macro %}` and `{% call %}`), and loop controls
(`{% break %}` and `{% continue %}`). A macro is visible only in the value
that defines it. `{% include %}` is available in rendered **file bodies only**,
described under Partials below. Jinja `import`, `from ... import`, and
`extends` are unavailable everywhere.

## Partials

A rendered file body can pull in another file inside the template root with
`{% include %}`:

```jinja
{% include "partials/frontmatter.md" %}
# {{ title }}
```

The included file is a **partial**. Includes work in rendered file bodies only —
source-tree files and `files:` rule sources. They do nothing in prompts,
defaults, `when`/`computed`/`format`/`options` expressions, target path
segments, apply messages, or hook fields; a `{% include %}` written on one of
those surfaces is reported as unavailable outside file bodies.

The rules an author needs:

- **The name is a `/`-separated path relative to the template root.** It can
  name a root-level file (`"notice.txt"`), a file in any nested directory
  (`"shared/legal/license.inc"`), or a file with any extension. There is no
  required `partials/` prefix or directory allowlist; `partials/` is only a
  convention. The name is the same string in every file that includes it; it is
  not resolved relative to the including file.
- **A partial sees the same variables** — answers, `data`, computed values, and
  the `toha_` context values — as the file that includes it. `with context` and
  `without context` are accepted but change nothing.
- **A partial placed outside the source subdirectory is pulled in but never
  emitted** as its own output file. A partial inside the source subdirectory is
  emitted like any other source file unless an `ignore` glob excludes it.
- **The target is a string literal or an ordered literal list of strings.**
  Computed names, and lists holding a computed or non-string item, are rejected.
  A list selects the first candidate that exists, in author order; an earlier
  missing name is a fallback. If no candidate exists the include fails, unless
  `ignore missing` is present, which renders empty text. An empty list also
  renders empty text. `ignore missing` suppresses only the all-not-found result;
  an escape, symlink, unreadable, non-UTF-8, syntax, nested-include, or cycle
  error still fails.
- **Includes are confined to the template root.** Absolute names, `..`
  traversal, backslash separators, and symlinked partials are refused. A partial
  may itself include others; a cycle is reported by name.
- **Includes resolve when the template loads**, so a missing partial, an escape,
  or a variable reached only through a partial is reported before the interview
  begins.

`template.yml` and every document loaded through its YAML `!include` tag receive
**no** Jinja include processing: they are configuration, parsed as data, never a
render surface. A file's role — body or configuration — follows how it is
referenced, not its extension, so a generated `.yml` or `.yaml` file in the
source tree (or named as a `files:` `source:`) is a file body and does get
includes. Naming a configuration document as a partial target simply inlines its
literal bytes; it does not parse or process it. Toha's YAML `!include` tag is a
separate feature for composing configuration data and is unchanged.
