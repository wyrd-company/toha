---
docs: true
title: Configuration
order: 10
relationships:
  describes: toha
  references: config
---

Toha reads configuration at three levels: system, user, and the current
directory. You can use it to find template folders, set template-specific default answers, and
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
`templates-paths`, `presets`, and `template-defaults`.

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

## Presets and template defaults

Use `presets` to name values you want to reuse. Use `template-defaults` to map
one template's question to a preset or an inline value:

```yaml
presets:
  primary_contact: contact@example.invalid
  default_labels: [review, shared]

template-defaults:
  "gh:example/collection":
    email: { preset: primary_contact }
    labels: { preset: default_labels }
    include_summary: false
  "toha-demo":
    topic: Sample Topic
```

The outer key is the template's exact formal name. Folder templates use their
canonical absolute path. Git templates use their normalized address. Installed
aliases and short names resolve to the installed entry's formal name; write that
formal name as the key. The bundled template uses `toha-demo`. Toha compares the
key directly and does not resolve it as an alias or short name.

`{ preset: primary_contact }` is a reference. A bare scalar or list is an inline
literal. For example, `email: primary_contact` supplies the text
`primary_contact`; it does not read the preset. Presets are literals and cannot
refer to other presets.

A mapping reaches only the named template and question. A question with the same
id in another template keeps its own default unless that template has its own
mapping. The value must match the question's answer type: string, boolean, or
list of strings. One preset can be reused by questions that have the same answer
type, even when their ids differ. Name presets for the value's meaning, such as
`primary_contact`; do not copy a question id as the preset name.

Toha reports a warning when the selected template does not define a mapped
question. A missing preset or wrong answer type stops the run and names the
configuration file and mapping. A wrong type from a preset also names the preset
file and entry. Mappings for other templates and presets that are not referenced
by the selected template are not checked during that run.

The former `defaults:` property is rejected with conversion guidance. Move each
value into a named preset or an inline mapping, then map it under every template
formal name that should receive it.

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

For `presets` and `hosts`, Toha merges individual keys. For
`template-defaults`, it merges each template-and-question pair. The local value
wins over the user value, and the user value wins over the system value. Other
presets, templates, and questions remain in the merged configuration. A preset
reference uses the fully merged preset store, even when the reference and preset
come from different layers. The winning value and its source file stay together
for diagnostics.

`templates-paths` works differently: Toha joins the lists in local, user,
system order. `local-config-name` uses the user value when present, then the
system value, then `.toha.yml`. `hosts` and `local-config-name` are available
only in the system and user layers.
