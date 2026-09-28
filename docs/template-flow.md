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

## Steer the run with a flow node

A `flow` node carries one control action and an optional `when`. When Toha
reaches it and its `when` is true — or it has no `when` — the action fires.
Like a message or a hook, it has no `id` and records no answer; the action is
data you declare, never inferred from a prompt. An optional `label` names the
node in the notice Toha prints.

```yaml
interview:
  - id: proceed
    type: confirm
    prompt: Ready to scaffold?
  - flow: stop
    when: "not proceed"
    label: declined at the proceed gate

  - id: mode
    type: select
    options: [apply, preview]
    prompt: Mode?
  - flow: dry-run
    when: "mode == 'preview'"
```

The actions are:

- `stop` ends the run where it fires. Toha writes no files and runs no hooks,
  and exits 0 with a short notice. Any staged interview stays and can be
  resumed.
- `abort` stops **and** discards the staged interview for the target, for a
  "cancel and throw this away" gate. It is the same removal the `abort`
  command performs. On a run with nothing staged it simply stops.
- `dry-run` completes the interview and shows the plan without writing
  anything, exactly as the `apply --dry-run` flag does. Setting both still
  writes nothing.
- `{ skip: rest }` skips every remaining question in the interview.
- `{ skip: group }` skips the rest of the current group. It is only valid
  inside a group; at the top level, use `{ skip: rest }`.

Because the trigger is the ordinary `when`, a flow node reads any earlier
answer or computed value, not just one confirm — and an ordinary `confirm`
question keeps its plain true/false behavior. A flow node whose `when` reads a
question still in the current batch waits until that answer is given, so it
never races the answer it depends on.

```yaml
interview:
  - group: telemetry
    when: features.telemetry
    nodes:
      - id: configure_now
        type: confirm
        prompt: Configure telemetry now?
      - flow: { skip: group }
        when: "not configure_now"
      - id: telemetry_key
        type: text
        prompt: Telemetry write key
  - flow: { skip: rest }
    when: minimal
```

A question skipped by a flow node takes the same answer a `when`-skipped
question takes: its `default`, or the empty answer when it has none. See
[Asking questions](/docs/toha/template-interviews).

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
