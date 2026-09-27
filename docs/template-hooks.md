---
docs: true
title: Hooks and messages
order: 8
relationships:
  describes: toha
  references: template-format
---

A template can run commands after Toha writes its files. These commands are
**hooks**. A template can also show messages before and after writing. Use a
dry run to see planned hook invocations before applying the template.

## Run a program

Use top-level `hooks` for commands that run after the interview and file
writing. `run` is a list: the first item is the program, followed by its
arguments.

```yaml
hooks:
  - run: [ git, init ]
```

Toha starts `git` directly with `init` as its argument. It does not pass the
list to a shell. Each item can contain a Jinja text template, such as
`"{{ file_name }}"`. A failed hook stops later hooks and makes the apply
fail.

## Run a script stored in the template

Use `script` to run an executable file inside the template root. Put its
arguments in `args`:

```yaml
hooks:
  - script: scripts/check.sh
    args: [ "{{ file_name }}" ]
```

The script path is relative to the folder containing `template.yml` and must
stay inside it. Make the script executable on the systems where the template
will run. `run` and `script` are two ways to specify a hook; use one per hook.

## Choose the working directory

By default, a hook runs with the target directory as its working directory.
Set `cwd` to a path relative to that target when it must run elsewhere inside
the generated output:

```yaml
hooks:
  - run: [ git, status ]
    cwd: notes
```

Here `notes` means the `notes` subdirectory of the target. `cwd` can contain
Jinja substitutions from answers or computed values.

## Run a hook conditionally or repeatedly

Use `when` to run a hook only when its expression is true:

```yaml
hooks:
  - run: [ git, init ]
    when: initialize_git
```

Use `each` to run it once for each item of a list. Its form is
`<list expression> as <item name>`:

```yaml
hooks:
  - run: [ tool, "{{ tag }}" ]
    each: tags as tag
```

The current item is available while Toha renders `run`, `args`, and `cwd`.
Toha evaluates `when` once before `each`; `when` cannot refer to the item.
Replace `tool` with the program your template needs. An empty list runs no
commands. Hooks run in list order.

A hook can also be a node inside `interview`:

```yaml
interview:
  - id: initialize_git
    type: confirm
    prompt: Initialize Git?
  - hook:
      run: [ git, init ]
    when: initialize_git
```

An interview hook is added to the plan when Toha reaches that node. It still
runs after files are written. Interview hooks run in interview order before
top-level hooks. A hook node can appear inside a group.

## Trust a template before running hooks

Hooks execute programs, so Toha requires trust. A dry run lists every planned
invocation with its rendered arguments. Without trust, applying a template
with hooks shows a dry run and does not run them.

For a template you know and trust, use `--trust` for one run:

```sh
toha apply ./note-template ./output --trust
```

When installing a template, `toha templates add <address> --trust` records
trust in your user registry, so later runs do not need `--trust`. Local
project aliases cannot grant trust. See
[Managing templates](/docs/toha/templates-management) for installed templates.

## Show messages around file writing

Top-level `messages` has two optional fields. `before-apply` appears before
Toha writes any file. `after-apply` appears after the last hook:

```yaml
messages:
  before-apply: "Creating {{ file_name }}"
  after-apply: "Created {{ file_name }}"
```

Both values are Jinja text templates. Toha omits a message that renders to
empty or whitespace-only text. A dry run does not show `after-apply`. To show
a message during the interview instead, use a `message` node; see
[Controlling the interview](/docs/toha/template-flow).
