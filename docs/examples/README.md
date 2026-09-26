---
relationships:
  exemplifies: template-format
---

# Template examples

Each directory holds a `template.yml` that shows one part of the template
format.
These examples are the seed for the fixture suite.

| Example | Shows |
| --- | --- |
| [basic](basic/template.yml) | Question types, validation, a looped text question, defaults |
| [branching](branching/template.yml) | `when`, groups, computed values, options from an expression, static data |
| [generated-files](generated-files/template.yml) | `each` file generation, ignore and static globs, messages, hooks |

## Hooks

Hooks run after all files are written. Hook nodes in the interview run first,
in interview order, when their `when` is true. A hook runs once, also in the
scope of a looped question. The template-level `hooks` list runs last.

A template must be trusted before its hooks run. Without trust, `apply` acts as
a dry run, lists the hooks, and exits non-zero.

## Jinja rules

All logic is Jinja. A field is either a template or an expression, by the type
of its value:

- A field with a string value is a **template**. It is rendered, and the
  result is a string.
- A field with a non-string value (bool, number, array, object) is an
  **expression** when you give a string. Any other YAML value is a literal.
- `computed` is always an expression.

Answers are flat. Every question id and computed id is unique in the template.
Groups do not make a namespace.
