---
relationships:
  references: template-format
---

# `template.yml` field map

Toha validates `template.yml` against these types when it loads a template. Strings marked **template** render with Jinja delimiters; **expression** strings evaluate as Jinja expressions without delimiters. For a non-string value, a string can be an expression in place of the literal type. `!include` can supply any value.

| Path | Type | Meaning |
| --- | --- | --- |
| `name` | slug string, required | Short name (`[a-z0-9]+` with optional hyphen-separated parts). |
| `description` | string | Template description. |
| `source` | string, default `template` | Source subdirectory relative to the template root. |
| `data` | object | Static Jinja values; top-level keys are identifiers. |
| `interview` | array of nodes | Ordered interview. |
| `files` | array of file rules | Render support files for each item. |
| `ignore` | array of glob strings | Skip source files. |
| `static` | array of glob strings | Copy matching source files byte for byte. |
| `hooks` | array of hook commands | Run after interview hook nodes. |
| `messages.before-apply` | template string | Show before writing files. |
| `messages.after-apply` | template string | Show after hooks. |

Each `interview[]` node is exactly one of these shapes:

| Path | Type | Meaning |
| --- | --- | --- |
| `id` | identifier string, required for question or computed | Answer or computed value key. |
| `type` | `text`, `multiline`, `confirm`, `select`, `multiselect` | Question type. |
| `prompt` | template string, required for question | Question title. |
| `description` | template string | Question help. |
| `placeholder` | template string | Text input hint. |
| `required` | boolean or expression | Reject an empty answer. |
| `default` | template string for non-looped text, multiline, and select; boolean literal or expression for confirm; string array literal or expression for looped text and multiselect | Fallback answer. |
| `options` | string array or expression | Choices for select or multiselect. |
| `loop.min`, `loop.max` | nonnegative integer or expression | Bounds on a looped text question's item count. |
| `validate.min`, `validate.max` | nonnegative integer or expression | String length or multiselect count bounds; on looped text, bound each item's length. |
| `validate.regex` | regex string | Pattern for string answers; on looped text, check each item. |
| `format` | expression | Transform a validated answer via `value`; on looped text, transform each item. |
| `when` | expression | Include node only when true. |
| `computed` | expression, required with `id` | Store derived value. |
| `group` | identifier string, required with `nodes` | Diagnostic name; not an answer id. |
| `nodes` | array of nodes | Group children. |
| `hook` | hook command object | Schedule an interview hook. |
| `message` | template string | Show text during interview. |

A hook command has exactly one of `run` or `script`. `run` is a nonempty array of template strings (argument vector). `script` is a path string relative to the template root. `args` is an array of template strings. `cwd` is a template string relative to the target directory. Optional `each` (`<expression> as <identifier>`) runs the hook once per item, in item order, with the item bound while `run`, `args`, and `cwd` render; an empty sequence runs nothing. Top-level hooks also accept `when` (expression), evaluated once before `each`; interview hook nodes accept `each` inside `hook` and `when` on the node.

Each `files[]` rule has `each` (`<expression> as <identifier>`), `source` (support file path relative to the template root), and `path` (template string relative to target), all required. Optional `when` is an expression.
