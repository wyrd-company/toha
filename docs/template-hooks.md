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
dry run to review planned hook invocations while authoring the template.

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
Jinja substitutions from answers or computed values. Toha rejects an absolute
`cwd`, a path that climbs outside the target, or a path through a symlink when
it applies the template. `cwd` only sets the command's starting directory;
trusted hooks can still access files outside the target.

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

A flow `stop` or `abort`, or a flow `dry-run`, writes no files, so no hooks
run. See [Controlling the interview](/docs/toha/template-flow).

## Use a hook's result

Give a hook an `id` so a later hook, or the after-apply message, can read what
it did. A result has three fields: `exit_code`, `stdout`, and `stderr`.

Branch a later hook on an earlier one's exit code:

```yaml
hooks:
  - id: lint
    run: [ lint-tool, --check, . ]
    allow-failure: true            # tolerate a nonzero exit so it can be read
  - run: [ lint-tool, --fix, . ]
    when: "lint.exit_code != 0"    # runs only when lint found problems
  - run: [ test-tool ]
    when: "lint.exit_code == 0"
```

A hook stops the apply on a nonzero exit unless it sets `allow-failure: true`,
which tolerates a nonzero exit code so a later surface can read the result. A
signal still fails the apply. `allow-failure` requires an `id` whose result is
read.

To read a stream, declare it in `capture` so Toha pipes it instead of letting
it go to the terminal:

```yaml
hooks:
  - id: version
    run: [ git, describe, --tags, --always ]
    capture: [ stdout ]
  - run: [ git, notes, add, -m, "scaffolded at {{ version.stdout }}" ]
messages:
  after-apply: "Created at {{ version.stdout }}."
```

Toha never prints a captured stream. A stream you read must be captured, and a
stream you capture must be read; either mismatch is a load error. Trailing
newlines are stripped, as a shell `$(…)` does.

A result is readable only in a later top-level hook's `when`, `run[1..]`,
`args`, or `cwd`, and in `messages.after-apply`. It cannot be read in a file, a
path, `before-apply`, an interview field, an interview hook, the program name
`run[0]`, the script path, or `each`. An interview hook can produce a result
for a later top-level hook, but never reads one.

A producer that does not run — a false `when`, or an interview hook whose
position is skipped — gives `none` for all three fields, so guard on whether it
ran:

```yaml
interview:
  - id: use_vcs
    type: confirm
    prompt: Initialize version control?
  - hook:
      id: vcs_init
      run: [ vcs, init ]
      capture: [ stdout ]
    when: use_vcs
hooks:
  - run: [ note-tool, "{{ vcs_init.stdout }}" ]
    when: "vcs_init.exit_code is not none"   # none means vcs_init did not run
```

A hook without an `id` behaves exactly as before: its output goes to the
terminal and nothing can read it. A result exists for one apply only; Toha
never writes it to a file or persists it.

## Read a hook's JSON output

Add `parse: json` to read a hook's stdout as JSON directly at its `id`. It
requires `id` and `capture: [stdout]`:

```yaml
hooks:
  - id: pkg
    run: [ pkg-tool, info, --json ]   # prints {"name":"demo","version":"1.4.0"}
    parse: json
    capture: [ stdout ]
  - run: [ note-tool, "built {{ pkg.name }} {{ pkg.version }}" ]
messages:
  after-apply: "Created {{ pkg.name }} at {{ pkg.version }}."
```

`pkg` is the parsed value: read an object key as `pkg.name` (or `pkg['a-key']`),
an array item as `pkg[0]`, and a scalar as itself. JSON `null` reads as `none`.

Execution metadata moves to a `status-id` you name, which holds `exit_code`,
`stdout`, `stderr`, and `parsed`:

```yaml
hooks:
  - id: findings
    run: [ lint-tool, --format, json ]  # exits nonzero with a JSON issue list
    parse: json
    capture: [ stdout ]
    status-id: lint
    allow-failure: true
  - run: [ lint-tool, --fix, . ]
    when: "lint.exit_code != 0"
messages:
  after-apply: "{% if lint.parsed %}{{ findings | length }} findings{% endif %}"
```

Declare a `status-id` when the producer might not run — it has a `when`, sits
under a group `when` or a skippable interview position, or sets `allow-failure`
— so `<status-id>.exit_code is none` can tell "did not run" from a parsed
value. On a clean exit, empty or malformed JSON fails the apply. When
`allow-failure` tolerates a nonzero exit and the output does not parse, the
value is `none`, `<status-id>.parsed` is false, and the raw text stays in
`<status-id>.stdout`.

## Trust a template before running hooks

Hooks execute programs, so Toha requires trust. A dry run lists every planned
invocation with its rendered arguments; a hook that reads a result is listed
in source form, with `{{ id.field }}` placeholders, because the result is not
known until the hook runs. Without trust, applying a template with hooks shows
a dry run and does not run them.

For a template you know and trust, use `--trust` for one run:

```sh
toha apply ./note-template ./output --trust
```

When installing a template, `toha templates add <address> --trust` records
trust in your user registry, so later runs do not need `--trust`. Local
project aliases cannot grant trust.

For a template that is already installed in your user registry, review the
hooks and approve its current executable surface:

```sh
toha templates trust daily-note
```

The approval covers the installed hook nodes and executed in-template files.
If an update changes that executable surface, the approval lapses. Review the
changed surface and run `templates trust daily-note` again. This re-approves
the installed surface without fetching content or changing the installed
commit.

To revoke approval, run:

```sh
toha templates untrust daily-note
```

Revocation does not remove or update the template. It records a denial in your
user registry, so an approval from a lower registry layer cannot make the
template trusted again. A later `templates trust daily-note` removes the
denial and approves the current installed surface.

Both commands require the template to be present in the user registry. They
refuse a template supplied only by the system registry or local layer. See
[Managing templates](/docs/toha/templates-management) for installation,
updates, aliases, and removal.

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
