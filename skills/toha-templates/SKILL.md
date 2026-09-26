---
name: toha-templates
description: Use when authoring or checking a toha template folder, its interview, generated files, hooks, or Jinja expressions.
relationships:
  references: template-format
---

# Author a toha template

Create `template.yml` at the template root with a `name` slug. Put rendered files under `template/`, or set `source` to another relative subdirectory. `source: .` uses the root; `template.yml` itself is excluded. Support files outside `source` are read only when referenced. Use `!include <path>` for YAML, JSON, or TOML data inside the template root; `data` keys enter the Jinja context. See [the complete field map](references/format.md).

Build `interview` in dependency order. A question has `id`, `type`, and `prompt`. Types are `text`, `multiline`, `confirm`, `select`, and `multiselect`; choices need `options`. `required` rejects empty answers. `validate.min`, `validate.max`, and `validate.regex` constrain answers; `format` transforms a validated answer using `value`. A `text` question with `loop` produces an array of strings, ending on an empty submission or `loop.max`; `loop.min` constrains item count. For a loop, `validate` and `format` apply to each string item, and its default is an array literal or expression. `when` skips a node or a whole `group` of `nodes`; skipped questions receive their default or `none`. A `computed` node stores an expression result under its `id`. A `message` node displays rendered text. A `hook` node schedules a command at that point. All ids and `data` keys share a namespace; a node may reference only earlier ids.

Files under `source` render at matching relative paths. Each path segment is a Jinja template; an empty rendered file or directory name omits it. `ignore` skips source files and `static` copies bytes unchanged; both use globs relative to `source`. Use top-level `files` to render a support file for each item of `<expression> as <name>`, with a rendered target `path` and optional `when`.

A string-typed field is a Jinja template (`{{ value }}`). A boolean, number, array, or object field can be a literal or a Jinja expression without delimiters. `when`, `computed`, and `format` are expressions. Available case filters: `kebab`, `snake`, `camel`, `pascal`, `constant`, `title`. `tojson` emits JSON (for example a boolean in a JSON file); `toyaml` emits YAML with no leading `---` and no trailing document newline (a trailing newline that is part of the value is kept). `indent` leaves the first line in place, so write its indentation in the template: `root:\n  {{ value | toyaml | indent(2) }}` nests the value under `root`. Object keys serialize in sorted order. Macros (`{% macro %}`, `{% call %}`) work within one value; `import`, `include`, and `extends` do not. `{% break %}` and `{% continue %}` work in loops. `now()` returns the interview's fixed instant; `now() | dateformat('%Y-%m-%d')` uses jiff strftime syntax and the seed time zone. `none` renders as an empty string.

Hooks run after files are written. Use `run` as an argument vector (no shell), or `script` relative to the template root with optional `args`; `cwd` is relative to the target. Add `each: <expression> as <name>` to run a hook once per item with the item bound while its arguments and `cwd` render. Interview hook nodes run first in order, then top-level `hooks`. A failing hook stops the rest. Hooks require registry trust through a name or `toha apply --trust`; a direct folder or address needs `--trust`. Use `messages.before-apply` and `messages.after-apply` for text around the write and hooks; empty messages are hidden.

Test from a separate target with an answers JSON object: `toha apply ./my-template ./target --answers answers.json --dry-run`. Check paths, messages, hooks, and conflicts. Apply to a disposable target to verify the rendered tree. Use `--force` only when overwriting is intended.
