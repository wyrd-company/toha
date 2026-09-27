---
docs: true
title: Managing templates
order: 6
relationships:
  describes: toha
  references:
    - command-line-interface
    - template-registry
---

You can apply a local template folder directly, or install templates so Toha
can find them by name. Toha also discovers templates in configured template
paths. See [Configuration](/docs/toha/configuration) for those paths and the
system, user, and local layers.

## Add a template

`toha templates add` accepts a local folder or a Git address. A repository can
hold several templates: without `#path`, Toha installs all templates it finds.
Use `#path` to select one template folder. Add `@branch`, `@tag`, or `@commit`
before `#path` to select a Git ref.

```sh
toha templates add ./note-template
toha templates add gh:example/collection#notes
toha templates add gl:example/collection@main#notes
```

The built-in host shortcodes are:

| Prefix | Git host |
| --- | --- |
| `gh:` | GitHub |
| `gl:` | GitLab |
| `bb:` | Bitbucket |
| `cb:` | Codeberg |
| `ge:` | Gitee |

Full HTTPS and Git SSH addresses also work. System or user configuration can
set additional host prefixes or replace these mappings.

Add `--alias <name>` when the address selects exactly one template. Add
`--trust` only when you want the installed template's hooks to run without a
per-run trust flag. You can always use `toha apply <template> <target> --trust`
for one run.

## Find and use a template

```sh
toha templates list
toha templates list --system
toha templates list --user
toha templates list --local
toha templates list --json
toha apply notes ./output
```

The **formal name** is the source address. The **short name** is `name` in
`template.yml`. An **alias** is a name you choose. If several templates have
the same short name, Toha shows their formal names so you can use one directly
or assign an alias. `list` shows formal names, short names, aliases, and trust;
`--json` also includes the source, ref, commit, path, and layer.

Toha discovers each directory with a `template.yml` in the configured template
paths. A discovered folder uses its canonical absolute path as the formal name
and is untrusted unless a system or user registry entry supplies trust.

## Update, rename, and remove

```sh
toha templates update
toha templates update notes
toha templates alias notes writing-notes
toha templates alias --remove writing-notes
toha templates remove notes
```

`update` refreshes user-installed Git templates that follow a branch or the
default branch. A template pinned to a tag or commit stays at that revision.
`alias` adds or removes an alias in the user registry. `remove` removes a
user-installed template and its registry entry. These commands do not modify
system administrator templates; the administrator provisions those through the
system registry and template paths.
