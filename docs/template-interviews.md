---
docs: true
title: Questions and values
order: 4
relationships:
  describes: toha
  references: template-format
---

`interview` is an ordered list of questions and other nodes. Answers, static
`data`, and computed values become variables for later questions and file
rendering. A question or expression can refer only to values already defined
at its position in the interview.

## Questions

Each question needs an `id`, `type`, and `prompt`. The id becomes the answer
key and a Jinja variable. The available types are:

| Type | Answer | Use |
| --- | --- | --- |
| `text` | String | A single line of text. |
| `multiline` | String | Text that can span lines. |
| `confirm` | Boolean | A yes or no choice. |
| `select` | String | One item from `options`. |
| `multiselect` | Array of strings | Several items from `options`. |

All questions can use `description`, `placeholder`, `required`, `default`,
`validate`, `format`, and `when`. `description` and `placeholder` provide
additional prompt text. `required: true` rejects an empty answer. `when` is a
Jinja expression; when false, Toha skips the question. A skipped question uses
its `default` if present. Otherwise it supplies `[]` for a looped `text` or
`multiselect` question, and `none` for the other types.

```yaml
interview:
  - id: title
    type: text
    prompt: Note title
    description: Shown at the top of the note
    placeholder: Enter a title
    required: true
    validate:
      min: 3
      max: 80

  - id: slug
    type: text
    prompt: File name
    default: "{{ title | kebab }}"
    validate:
      regex: '^[a-z0-9-]+$'

  - id: include_summary
    type: confirm
    prompt: Include a summary?
    default: false

  - id: summary
    type: multiline
    prompt: Summary
    when: include_summary
```

For string answers (`text`, `multiline`, and `select`), `default` is a string
that Toha renders as a template. For `confirm` and `multiselect`, supply a
literal boolean or array, or a Jinja expression that returns that type.
`required` can likewise be a literal boolean or an expression.

`validate.min` and `validate.max` limit string length or multiselect item
count. `validate.regex` checks a string. These bounds can be literal
nonnegative integers or expressions. `format` is an expression over `value`
that transforms an answer after validation, for example
`format: value | lower`.

## Choices and repeated answers

`select` and `multiselect` require `options`. Supply an array of strings or an
expression that returns one. A `multiselect` default is an array of strings.

```yaml
- id: priority
  type: select
  prompt: Priority
  options: [ low, normal, high ]
  default: normal

- id: labels
  type: multiselect
  prompt: Labels
  options: [ personal, shared, archived ]
  default: []
```

A `text` question with `loop` asks for one string at a time. An empty answer
ends the loop. Its final answer is an array of strings. `loop.min` and
`loop.max` set the item count; `loop.max` ends the loop when reached.
Validation and formatting apply to each item.

```yaml
- id: tags
  type: text
  prompt: Tag (empty to finish)
  loop:
    min: 0
    max: 5
  format: value | lower
```

## Conditions, groups, computed values, and messages

Each interview node can have a `when` expression. A group applies one
condition to all its child `nodes`; `group` is a name for diagnostics, not an
answer id. A computed node stores the result of its `computed` expression
under its `id`. A `message` node shows rendered text when the interview reaches
it. A `hook` node is also allowed; see
[Generated files and hooks](/docs/toha/template-files).

```yaml
- group: extra_details
  when: include_summary
  nodes:
    - id: detail
      type: text
      prompt: Detail for {{ title }}

- id: output_name
  computed: "title | kebab"

- message: "Creating {{ output_name }}"
```

Question ids, computed ids, and top-level data keys share a namespace. Each
must be a lowercase Jinja identifier such as `output_name`, and no two may
match. A group name does not create an answer. Toha rejects references to an
unknown or later id.

## Jinja values

The Jinja context contains top-level `data`, answers given so far, and
computed values evaluated so far. Toha includes Minijinja's built-ins plus
`kebab`, `snake`, `camel`, `pascal`, `constant`, and `title` case filters;
`tojson` and `toyaml`; and `now() | dateformat` for the interview time.
`dateformat` uses strftime syntax and defaults to `%Y-%m-%d`. `tojson` accepts
an optional indent. `toyaml` omits the document marker and final document
newline. Templates can use macros and loop controls. Jinja `import`,
`include`, and `extends` are unavailable; a macro is visible only in the value
that defines it.
