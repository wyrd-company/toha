---
docs: true
title: Configuration
order: 10
relationships:
  describes: toha
  references: config
---

Toha reads configuration at three levels: system, user, and the current
directory. You can use it to find template folders, set default answers, and
define Git host shortcodes. A missing file acts as an empty configuration.

## Find the configuration files

| Layer | Linux and macOS | Windows |
| --- | --- | --- |
| System | `/etc/toha/config.yml` | `%PROGRAMDATA%\toha\config.yml` |
| User | `$XDG_CONFIG_HOME/toha/config.yml`, or `~/.config/toha/config.yml` | `%APPDATA%\toha\config.yml` |
| Local | `.toha.yml` in the current directory | `.toha.yml` in the current directory |

On Linux and macOS, `XDG_CONFIG_HOME` must be an absolute path; otherwise
Toha uses `~/.config`. Set `TOHA_USER_CONFIG` to point to another user file.
Set `TOHA_CONFIG` to point to another local file. System or user configuration
can change the local filename with `local-config-name`:

```yaml
local-config-name: .project-toha.yml
```

The local file cannot set `local-config-name`. It can contain only
`templates-paths` and `defaults`.

## Add template search paths

Use `templates-paths` to name directories Toha should scan for folders with
`template.yml`:

```yaml
templates-paths:
  - shared-templates
  - ~/more-templates
```

Each path is relative to the configuration file's directory unless it is
absolute or starts with `~`. Toha scans each listed directory for templates.
If a layer has no `templates-paths`, it uses that layer's default location:

| Layer | Default template path |
| --- | --- |
| Local | `.templates` in the current directory |
| User on Linux | `$XDG_DATA_HOME/toha/`, or `~/.local/share/toha/` |
| User on macOS | `~/Library/Application Support/toha/` |
| User on Windows | `%APPDATA%\toha\` |
| System on Linux and macOS | `/usr/local/share/toha/` |
| System on Windows | `%PROGRAMDATA%\toha\` |

Toha combines paths from all layers in local, user, system order. One layer's
paths do not hide another layer's paths. See
[Template registries](/docs/toha/template-registries) for how installed
names, aliases, and trust are recorded beside these paths.

## Set default answers

Use `defaults` to map a question id to an answer. For example:

```yaml
defaults:
  title: Untitled
  include_summary: false
```

If a template asks `title`, `Untitled` replaces that question's own default.
If it asks `include_summary`, `false` becomes the default. The value must
match the question's answer type: a string, boolean, or list of strings as
appropriate. The person running an interactive interview can still change
the default.

Defaults apply by id to every template that asks that question. Choose ids
with that in mind when you set user-wide or system-wide defaults.

## Define Git host shortcodes

Use `hosts` in a system or user file to map a prefix to a base URL:

```yaml
hosts:
  custom: https://git.example.invalid
```

With this setting, `custom:example/collection#notes` addresses that host.
Toha includes `gh` for GitHub, `gl` for GitLab, `bb` for Bitbucket, `cb` for
Codeberg, and `ge` for Gitee. A configured value can replace a built-in
prefix. Local configuration cannot set `hosts`, so a formal Git name resolves
the same way throughout your directories.

## How the layers combine

For `defaults` and `hosts`, Toha merges individual keys. The local value wins
over the user value, and the user value wins over the system value when the
same key occurs in more than one layer. For example, a local `title` default
replaces a user `title` default without removing other user defaults.

`templates-paths` works differently: Toha joins the lists in local, user,
system order. `local-config-name` uses the user value when present, then the
system value, then `.toha.yml`. `hosts` and `local-config-name` are available
only in the system and user layers.
