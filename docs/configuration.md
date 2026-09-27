---
docs: true
title: Configuration
order: 7
relationships:
  describes: toha
  references:
    - config
    - template-registry
---

Toha reads system, user, and local configuration. This lets an administrator
provide shared templates, a user keep personal templates, and a directory set
its own defaults and template paths.

## Configuration files

| Layer | Linux and macOS | Windows |
| --- | --- | --- |
| System | `/etc/toha/config.yml` | `%PROGRAMDATA%\toha\config.yml` |
| User | `$XDG_CONFIG_HOME/toha/config.yml`, or `~/.config/toha/config.yml` | `%APPDATA%\toha\config.yml` |
| Local | `.toha.yml` in the current directory | `.toha.yml` in the current directory |

Set `TOHA_USER_CONFIG` to choose another user configuration file. Set
`TOHA_CONFIG` to choose another local configuration file. In system or user
configuration, `local-config-name` changes the default local filename. A
missing file contributes no settings.

A system or user file can contain all four options:

```yaml
local-config-name: .toha.yml
templates-paths:
  - shared-templates
defaults:
  title: Untitled
hosts:
  custom: https://git.example.invalid
```

A local file can contain only `templates-paths` and `defaults`:

```yaml
templates-paths:
  - .templates
defaults:
  title: Untitled
```

A relative template path resolves from its configuration file's directory.
`~` expands to the home directory. Toha scans every directory in each template
path for template folders containing `template.yml`.

## How values combine

Local values take precedence over user values, which take precedence over
system values. `defaults` and `hosts` merge by key; a later layer replaces
only matching keys. `templates-paths` concatenate in local, user, system
order. `local-config-name` and `hosts` come only from system and user files, so
a local directory cannot change how a formal Git name resolves.

`defaults` maps question ids to answers. A configured default replaces the
`default` in any template question with the same id and must have the answer's
type. `hosts` maps a Git shorthand prefix to a base URL. The built-in prefixes
are `gh`, `gl`, `bb`, `cb`, and `ge`.

## Template paths and registries

Without configuration, Toha scans `.templates` in the current directory, the
user data directory, and the system data directory. The system data directory
is `/usr/local/share/toha/` on Linux and macOS, or `%PROGRAMDATA%\toha\` on
Windows. The user data directory is `$XDG_DATA_HOME/toha/` (default
`~/.local/share/toha/`) on Linux, `~/Library/Application Support/toha/` on
macOS, and `%APPDATA%\toha\` on Windows.

Each layer also has a `templates.yml` registry. The system registry is in the
system data directory; the user registry is in the user data directory. The
local registry is in the first local template path, usually
`.templates/templates.yml`. Registry entries merge by formal name, with local
fields taking precedence over user fields, then system fields.

The system registry is provisioned by an administrator. Toha commands write
the user registry. The local registry holds aliases only; it cannot grant
trust. A system or user registry entry records a template's short name, source,
path, optional ref and commit, aliases, and whether its hooks are trusted.
For example, an administrator can place a template under the system template
path and register it in `/usr/local/share/toha/templates.yml`:

```yaml
templates:
  /usr/local/share/toha/note:
    name: note
    source: /usr/local/share/toha/note
    path: /usr/local/share/toha/note
    aliases: [ shared-note ]
    trusted: false
```

An unregistered template in a scanned path is still discoverable by its folder
path and starts untrusted. Use `toha templates list --system` to see system
registry entries, or `toha templates list --local` for local templates and
aliases.
