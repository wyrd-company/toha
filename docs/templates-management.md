---
docs: true
title: Managing templates
order: 9
relationships:
  describes: toha
  references:
    - command-line-interface
    - template-registry
---

You can run a template directly from a folder or Git address. Install one when
you want Toha to remember it, find it by name, and update its Git content
later. `toha templates` manages the templates in your user registry.

## Use a template without installing it

Pass a local folder or Git address to `apply`:

```sh
toha apply ./note-template ./output
toha apply gh:example/collection#notes ./output
```

Toha uses that source for this run. The target is the last argument. Add
`--dry-run` to see planned files and hooks before writing anything.

## Understand template names

A template can have three names:

- Its **formal name** identifies its source. For a Git template, it includes
  the repository and optional ref and path. For a local folder, it is the
  folder's canonical absolute path.
- Its **short name** is the `name` inside `template.yml`, such as `note`.
- An **alias** is an additional name you choose, such as `daily-note`.

After installation, you can pass any unambiguous name to `apply`. If two
templates share a short name, Toha lists their formal names. Use a formal name
or give one of the templates an alias. Two templates cannot share an alias.
An alias can match another template's short name; Toha resolves the alias first.

## Install from a folder or Git repository

`templates add` records the template in your user registry:

```sh
toha templates add ./note-template
toha templates add gh:example/collection#notes
```

When you add a local folder, Toha records its absolute path. It does not copy
the folder. Later runs use the files at that path, so edits to the folder
affect the installed template.

A repository can contain several templates. Without `#path`, Toha finds and
installs all templates in it. Add `#path` to select one folder containing
`template.yml`. Toha requires that path to select exactly one template.

Add a Git ref before `#path` to use a branch, tag, or commit:

```sh
toha templates add gh:example/collection@main#notes
```

If you omit the ref, Toha follows the repository's default branch. A branch
can later be updated; a tag or commit remains pinned to that revision.

You can give a single selected template an alias as you install it:

```sh
toha templates add gh:example/collection#notes --alias daily-note
```

`--alias` cannot be used when the address installs several templates.

## Choose a Git address

Toha accepts full HTTPS and Git SSH repository addresses. It also has these
shortcodes:

| Prefix | Host | Example |
| --- | --- | --- |
| `gh:` | GitHub | `gh:example/collection#notes` |
| `gl:` | GitLab | `gl:example/collection#notes` |
| `bb:` | Bitbucket | `bb:example/collection#notes` |
| `cb:` | Codeberg | `cb:example/collection#notes` |
| `ge:` | Gitee | `ge:example/collection#notes` |

Each shortcode expands to that host's repository URL. System or user
configuration can add a prefix or replace a built-in host. See
[Configuration](/docs/toha/configuration).

## List available templates

```sh
toha templates list
```

The list shows each formal name, short name, aliases, and trust state. Use
`--system`, `--user`, or `--local` to inspect one layer. Use `--json` when a
script needs the source, ref, commit, path, and layer as well.

Toha also discovers folders containing `template.yml` under configured
template paths. You can use a discovered template without adding it to the
user registry. It starts untrusted unless a system or user registry entry
provides trust. See [Template registries](/docs/toha/template-registries).

## Add or remove an alias

An alias gives one user-installed template an alternate name:

```sh
toha templates alias note daily-note
toha apply daily-note ./output
```

Use the formal name in the first command if the short name is ambiguous. To
remove an alias, use:

```sh
toha templates alias --remove daily-note
```

Aliases are unique across available templates. The `alias` command writes
only your user registry; it does not change a system administrator template.

## Update Git templates

```sh
toha templates update
toha templates update daily-note
```

With no name, `update` checks user-installed Git templates. With a name, it
checks that one template. It moves branch-following templates to the latest
commit. A template pinned to a tag or commit stays at that revision. Locally
installed folders are not Git updates.

## Remove a user-installed template

```sh
toha templates remove daily-note
```

`remove` deletes the user registry entry and uninstalls the Git content when
nothing else uses it. It cannot remove a system administrator template. See
[Template registries](/docs/toha/template-registries) for how system and local
templates are provided.

## Trust hooks

Toha does not run a template's hooks unless they are trusted. Use `--trust`
with `apply` for one run. If you know an installed template and want its hooks
to run on later uses, pass `--trust` when adding it:

```sh
toha templates add gh:example/collection#notes --trust
```

This records approval for the template's current executable hook surface in
your user registry. Read [Hooks and messages](/docs/toha/template-hooks) before
trusting a template.

To approve a template that is already installed in your user registry, review
its hooks and then run:

```sh
toha templates trust daily-note
```

This approves the executable hook surface that is currently installed. It does
not fetch content or change the installed commit. If an update changes a hook
or an executed in-template script, the approval no longer matches. Review the
new surface and run the same command to approve it again. You do not need to
remove or add the template again.

To revoke approval, run:

```sh
toha templates untrust daily-note
```

Revocation does not remove or update the template. It records a denial in your
user registry, so an approval from a lower registry layer cannot make the
template trusted again. A later `templates trust daily-note` removes that
denial and approves the current installed surface.

Both commands change only a template that is present in the user registry.
They refuse a template supplied only by the system registry or local layer.
