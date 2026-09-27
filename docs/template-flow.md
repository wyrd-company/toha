---
docs: true
title: Controlling the interview
order: 5
relationships:
  describes: toha
  references: template-format
---

Most templates can ask a straight list of questions. When later questions
depend on earlier answers, add conditions, groups, computed values, or messages
to the `interview` list. Toha processes these nodes in document order.

## Ask a set of questions conditionally

A `group` gives several child nodes one condition. Its `nodes` list can hold
questions, computed values, messages, hooks, or other groups.

```yaml
interview:
  - id: include_details
    type: confirm
    prompt: Include details?

  - group: details
    when: include_details
    nodes:
      - id: summary
        type: multiline
        prompt: Summary
      - id: audience
        type: text
        prompt: Intended audience
```

When `include_details` is false, Toha skips both questions. `details` is the
group's name for diagnostics; it is not an answer or a Jinja variable. You
can nest groups when a question inside one group decides whether to show
another set of questions.

Any interview node can use `when`. On a single question, it affects that
question. On a group, it affects the group and every child node. A false
condition skips the entire branch. See
[Asking questions](/docs/toha/template-interviews) for the answer stored when
a question is skipped.

## Calculate a value once

A computed node evaluates a Jinja expression at its place in the interview.
Toha stores the result under its `id`, just as it stores a question answer.
Use it when several later fields need the same derived value.

```yaml
interview:
  - id: title
    type: text
    prompt: Note title

  - id: file_name
    computed: "title | kebab"
```

Here `file_name` can be used in a later prompt, a file path, or a file body:
`{{ file_name }}`. A computed value can also have `when`. If its condition is
false, it does not evaluate its expression.

A computed id follows the same naming rule as a question id. Neither can
reuse a top-level `data` key or another id.

## Show a message during the interview

A message node shows rendered text when Toha reaches it. It does not ask for
an answer or define a variable.

```yaml
interview:
  - id: title
    type: text
    prompt: Note title
  - message: "Preparing the note for {{ title }}"
```

Messages can also use `when`. If a rendered message is empty or contains only
whitespace, Toha does not show it. For messages immediately before or after
files are written, use top-level `messages`; see
[Hooks and messages](/docs/toha/template-hooks).

## Place a hook in the interview

A `hook` node decides whether to include a command at that point in the
interview. Toha runs it after writing the files, not while asking questions.
Its position controls the order of interview hooks; top-level `hooks` follow
them. A hook node can use `when` and can appear inside a group. See
[Hooks and messages](/docs/toha/template-hooks) for hook
syntax and trust.

## Use values in order

A question, computed expression, or template in an interview node can refer to
static `data` and values defined by earlier nodes. It cannot refer to a later
id. This makes the interview predictable:

```yaml
interview:
  - id: title
    type: text
    prompt: Note title

  - id: file_name
    computed: "title | kebab"

  - message: "The file will be {{ file_name }}.txt"
```

`title` exists when Toha evaluates `file_name`, and both exist when it shows
the message. If you move the computed node above the title question, Toha
rejects the template when it loads it.

For the syntax of `{{ ... }}` and expressions such as `title | kebab`, see
[Jinja and values](/docs/toha/template-jinja).
