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
strftime syntax. The same interview instant is used throughout one run, so
several calls to `now()` agree.

## Supported Jinja features

Toha includes MiniJinja's built-in filters, tests, and functions, the `tojson`
filter, macros (`{% macro %}` and `{% call %}`), and loop controls
(`{% break %}` and `{% continue %}`). A macro is visible only in the value
that defines it. Jinja `import`, `include`, and `extends` are unavailable in
rendered templates. Use Toha's `!include` in `template.yml` to load support
data files; it is separate from Jinja's `include` statement.
