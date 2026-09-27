---
docs: true
title: Asking questions
order: 4
relationships:
  describes: toha
  references: template-format
---

A template asks questions through the `interview` list in `template.yml`.
Toha asks them in the order you write them. It saves each answer under the
question's `id`, so later questions and generated files can use that answer.

Start with one question:

```yaml
interview:
  - id: title
    type: text
    prompt: Note title
```

Here `title` is the answer's name, `text` says what kind of answer to collect,
and `prompt` is what the person sees. Every question needs all three fields.
An id must start with a lowercase letter or underscore and then contain only
lowercase letters, digits, or underscores. For example, `note_title` is valid.

## Choose an answer type

### `text`: one line

Use `text` for a short string, such as a title or file name.

```yaml
- id: title
  type: text
  prompt: Note title
```

The answer is a string. In a file, `{{ title }}` inserts that string. If you
need several lines, use `multiline` instead. To collect several one-line
answers as a list, use [`loop`](#ask-for-a-list-with-loop).

### `multiline`: several lines

Use `multiline` for a paragraph or another answer that may contain newlines.
The terminal driver opens an editor when it can; its help explains the
fallback when an editor is unavailable.

```yaml
- id: summary
  type: multiline
  prompt: Summary
```

The answer is still one string. Toha keeps its newlines when it renders a file.

### `confirm`: yes or no

Use `confirm` for a decision. Its answer is a boolean: `true` or `false`.

```yaml
- id: include_summary
  type: confirm
  prompt: Include a summary?
  default: false
```

You can use that answer in a later condition, such as
`when: include_summary`.

### `select`: one choice

Use `select` when the person must choose one item from `options`.

```yaml
- id: priority
  type: select
  prompt: Priority
  options: [ low, normal, high ]
  default: normal
```

The answer is the selected string. `options` is required. You can write a
literal list, as above, or an expression that returns a list of strings:

```yaml
options: "priorities | map(attribute='name') | list"
```

In that example, `priorities` must already be available from `data` or an
earlier answer. Toha evaluates the expression before asking the question.

### `multiselect`: several choices

Use `multiselect` when the person can choose multiple items from `options`.
The answer is a list of strings, even when only one item is selected.

```yaml
- id: labels
  type: multiselect
  prompt: Labels
  options: [ personal, shared, archived ]
  default: []
```

As with `select`, `options` is required and can be a literal list or an
expression that returns a list of strings. The default must be a list; `[]`
means no selections.

## Help the person answer

`description` adds explanatory text to a question. `placeholder` shows an
example or hint in the input field. Both can contain Jinja substitutions from
earlier answers.

```yaml
- id: slug
  type: text
  prompt: File name
  description: Used for the note's file name
  placeholder: "{{ title | kebab }}"
```

The prompt asks for the answer. The description explains its use. The
placeholder suggests a value but does not supply one; use `default` for that.

## Supply a default

`default` preselects an answer that the person can accept or change. For
`text`, `multiline`, and `select`, it is a string template. It can include
an earlier answer:

```yaml
- id: slug
  type: text
  prompt: File name
  default: "{{ title | kebab }}"
```

For `confirm`, the default is a boolean. For `multiselect` and a repeated
`text` question, it is a list of strings. These non-string defaults can be
literals or Jinja expressions that return the required type:

```yaml
- id: include_summary
  type: confirm
  prompt: Include a summary?
  default: "title | length > 20"
```

Here the quoted value is evaluated as a boolean expression; Toha does not
render it as text. Configuration can replace a question's default by its id.
See [Configuration](/docs/toha/configuration).

## Require and validate answers

Set `required: true` to reject an empty answer. You can also use an expression
that returns a boolean when the requirement depends on an earlier answer.

```yaml
- id: title
  type: text
  prompt: Note title
  required: true
  validate:
    min: 3
    max: 80
```

`validate.min` and `validate.max` set the allowed length of a string. For
`multiselect`, they set the number of selected items. They accept a
nonnegative integer or an expression that returns one. A `text` question can
also use `validate.regex` to require a pattern:

```yaml
- id: slug
  type: text
  prompt: File name
  validate:
    regex: '^[a-z0-9-]+$'
```

`validate.regex` also works on other string answers. Toha checks an answer
before storing it. A choice question also checks that every chosen item is in
its `options`.

## Change an answer with `format`

Use `format` when Toha should transform a valid answer before storing it.
The expression receives the submitted answer as `value`:

```yaml
- id: slug
  type: text
  prompt: File name
  format: value | kebab
```

If the person enters `Meeting Notes`, later questions and generated files see
`meeting-notes`. Toha validates the submitted value first, then applies
`format`. For a repeated `text` question, it validates and formats each item.

## Ask for a list with `loop`

A `text` question with `loop` asks for one item at a time. The person enters
an empty line to finish. Toha stores the answer as a list of strings.

```yaml
- id: tags
  type: text
  prompt: Tag (empty to finish)
  loop:
    min: 1
    max: 5
  format: value | lower
```

`loop.min` requires at least that many items. `loop.max` ends the loop when
that number is reached. Each bound accepts a nonnegative integer or an
expression. `validate` and `format` apply to each item; `loop.min` and
`loop.max` apply to the whole list. Only `text` questions support `loop`.

## Ask a question only when it applies

Set `when` to an expression that returns a boolean. Toha asks the question
only when it is true:

```yaml
- id: include_summary
  type: confirm
  prompt: Include a summary?

- id: summary
  type: multiline
  prompt: Summary
  when: include_summary
```

If `include_summary` is false, Toha skips `summary`. A skipped question uses
its `default` when it has one. Without a default, Toha records an empty list
for a repeated `text` or `multiselect` question, and `none` for the other
types. Generated files can use that value to decide what to render.

For groups and calculated values, continue with
[Controlling the interview](/docs/toha/template-flow). For the expression
syntax used by `when`, `default`, and other fields, see
[Jinja and values](/docs/toha/template-jinja).
