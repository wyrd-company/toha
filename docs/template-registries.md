---
docs: true
title: Template registries
order: 11
relationships:
  describes: toha
  references:
    - template-registry
    - config
---

A template registry records the names and trust state of templates that Toha
can find. Each configuration layer has a file named `templates.yml`. Most
people can use `toha templates add`, `alias`, and `remove` instead of editing
a registry. System administrators provision the system registry directly.

## Where Toha reads registries

| Layer | Registry location |
| --- | --- |
| System on Linux and macOS | `/usr/local/share/toha/templates.yml` |
| System on Windows | `%PROGRAMDATA%\toha\templates.yml` |
| User on Linux | `$XDG_DATA_HOME/toha/templates.yml`, or `~/.local/share/toha/templates.yml` |
| User on macOS | `~/Library/Application Support/toha/templates.yml` |
| User on Windows | `%APPDATA%\toha\templates.yml` |
| Local | `templates.yml` in the first local template path, usually `.templates/templates.yml` |

A missing registry is empty. [Configuration](/docs/toha/configuration)
explains how to change template paths and how their layers combine.

## What an entry records

The `templates` map uses a template's formal name as the key. A system or user
entry has these fields:

- `name`: the short name from the template's `template.yml`.
- `source`: the Git repository URL or canonical absolute path of a local
  template folder.
- `path`: the installed directory containing the template.
- `ref`: an optional Git branch, tag, or commit named by the address.
- `commit`: the resolved commit of installed Git content, when applicable.
- `aliases`: optional additional names.
- `approval`: the approved executable-surface digest. It lets Toha run the
  template's hooks without `--trust`, but only while the installed content
  still matches the digest.

`name`, `source`, and `path` are required. For a local folder, `source` and
`path` are usually the same absolute path. For a Git template, the install
command fills in the repository, ref, commit, and installation path.

## Provide a system administrator template

An administrator can put a template under the system template path. On Linux
and macOS, that path defaults to `/usr/local/share/toha/`. A folder with
`template.yml` is discoverable even without a registry entry, but starts
untrusted and has no managed aliases.

To give the folder an alias, add an entry to
`/usr/local/share/toha/templates.yml`:

```yaml
templates:
  /usr/local/share/toha/note:
    name: note
    source: /usr/local/share/toha/note
    path: /usr/local/share/toha/note
    aliases: [ shared-note ]
```

This example assumes `/usr/local/share/toha/note/template.yml` exists and
contains `name: note`. Add an `approval` digest only when the administrator
wants that template's hooks to run without a per-run trust flag; the approval
authorizes the hooks only while the installed content still matches the digest.
On Windows, use the system path under `%PROGRAMDATA%\toha\` instead.

Check system entries with:

```sh
toha templates list --system
```

`toha templates add`, `update`, `alias`, and `remove` write the user registry,
not the system registry. Maintain system entries and their files as part of
the administrator's installation process.

## Use local aliases

The local registry can add aliases to an available template. Its entries
contain only `aliases`; they cannot add a source, path, or approval. For
example, if the formal name `gh:example/collection#notes` is already
available, `.templates/templates.yml` can contain:

```yaml
templates:
  "gh:example/collection#notes":
    aliases: [ project-note ]
```

This makes `project-note` available while working in that directory. If the
formal name is not installed or discovered, Toha warns that the local alias
has no template to name. A local registry never grants hook trust.

## When layers overlap

Toha combines entries by formal name. A local field takes precedence over a
user field, and a user field takes precedence over a system field. The local
registry can affect only aliases. An alias must not name two templates or
match a formal name. A template found by scanning a path without any registry
entry uses its canonical absolute folder path as its formal name and is
untrusted.
