---
docs: true
title: Generated files and hooks
order: 5
relationships:
  describes: toha
  references: template-format
---

Toha renders the paths and contents under the source directory into the target.
A rendered file or directory name that is empty produces no output. It keeps
values typed while evaluating Jinja expressions; file bodies and path segments
render to text.

## Exclude or copy files

`ignore` and `static` contain globs relative to the source directory.
Ignored files are omitted. Static files are copied byte for byte, which is
useful for binary assets.

```yaml
ignore:
  - "**/*.orig"
static:
  - "assets/**"
```

## Generate one file per item

Use `files` for support files outside the source directory that need to be
rendered once per item. Each entry needs `each`, `source`, and `path`.
`each` has the form `<expression> as <name>`. The name is available while
rendering the support file and output path. `when` can skip the entire rule.

```yaml
files:
  - each: tags as tag
    source: parts/tag.txt
    path: "tags/{{ tag | kebab }}.txt"
    when: tags | length > 0
```

`source` is relative to the template root; `path` is relative to the target.
An `each` name must not match a data, question, or computed id. Toha prevents
rendered paths from leaving the target directory or entering its `.git`
directory.

## Hooks and trust

Hooks run after Toha writes the files. A hook can use `run`, an argument vector
whose items are Jinja templates, or `script`, an executable file inside the
template root, with optional `args`. `run` starts the program directly; it
does not invoke a shell. `cwd` changes the working directory relative to the
target. A hook can use `when` and `each` to select invocations. For `each`,
Toha binds the item name while rendering `run`, `args`, and `cwd`.

```yaml
hooks:
  - run: [ git, init ]
    when: initialize_git

  - script: scripts/check.sh
    args: [ "{{ output_name }}" ]
    cwd: .
```

A `hook` node inside `interview` adds a hook at that point in interview order.
Top-level `hooks` run after those nodes, in list order. A hook in a looped
question can produce several invocations. A failed hook stops the remaining
hooks and fails the apply.

Toha lists every hook invocation in a dry run. Before applying a template with
hooks, use `--trust` for one run or install the template with
`toha templates add <address> --trust` to record trust in the user registry.
Without trust, Toha shows a dry run and does not run the hooks.

## Messages around apply

`messages.before-apply` displays rendered text before any file is written.
`messages.after-apply` displays it after the last hook; a dry run omits that
message. Empty or whitespace-only messages are not shown.

```yaml
messages:
  before-apply: "Creating {{ output_name }}"
  after-apply: "Created {{ output_name }}"
```
