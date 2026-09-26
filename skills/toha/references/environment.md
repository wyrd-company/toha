---
relationships:
  references: command-line-interface
---

# Environment and directories

Toha reads these environment variables. `toha --help` lists them too.

| Variable | Use |
| --- | --- |
| `TOHA_CONFIG` | Local configuration file. Default: the file named by `local-config-name`, `.toha.yml` unless configured, in the current directory. |
| `TOHA_USER_CONFIG` | User configuration file. Default: `$XDG_CONFIG_HOME/toha/config.yml` on Linux and macOS, `%APPDATA%\toha\config.yml` on Windows. |
| `XDG_CONFIG_HOME` | Linux and macOS: base of the user configuration file. Default: `~/.config`. |
| `XDG_DATA_HOME` | Linux: base of `toha/`, which holds the user registry `templates.yml` and installed templates. Default: `~/.local/share`. |
| `XDG_CACHE_HOME` | Linux: base of `toha/`, which holds git templates fetched by address without `templates add`. Default: `~/.cache`. |
| `XDG_STATE_HOME` | Linux: base of `toha/staged/`, which holds staged interviews. Default: `~/.local/state`. |
| `HOME` | Home directory for `~` in folder addresses and for the defaults above. |
| `USERPROFILE` | Home directory when `HOME` is unset. |
| `APPDATA` | Windows: `%APPDATA%\toha` holds the user `config.yml`, the user registry, and installed templates. |
| `LOCALAPPDATA` | Windows: `%LOCALAPPDATA%\toha\cache` holds fetched git templates and `%LOCALAPPDATA%\toha\staged` holds staged interviews. |
| `PROGRAMDATA` | Windows: `%PROGRAMDATA%\toha` holds the system `config.yml` and `templates.yml`. |
| `VISUAL`, `EDITOR` | Editor for multiline answers, `VISUAL` first. Default: `nano`, or `notepad` on Windows. When the editor is not found, answers end with a line of `.`. |
| `PATH` | Directories searched for the editor and for the program of a `run` hook. |
| `TOHA_NOW` | Instant that `now()` returns for a new interview, such as `2026-01-02T03:04:05+00:00[UTC]`. Default: the current time. |

On macOS, the user registry and installed templates are in `~/Library/Application Support/toha`, staged interviews in `~/Library/Application Support/toha/staged`, and fetched git templates in `~/Library/Caches/toha`. On Linux and macOS, the system configuration is `/etc/toha/config.yml` and the system registry is `/usr/local/share/toha/templates.yml`.

Configuration layers merge local over user over system; a missing file is an empty layer. `local-config-name` and `hosts` come from the user and system layers only. The local registry, `.templates/templates.yml` unless configured, holds aliases only, so trust never comes from a project.

To isolate a run from the caller's configuration, registry, and staged interviews, point these at paths inside one scratch directory:

- Every platform: `HOME` (and `USERPROFILE` on Windows), `TOHA_USER_CONFIG`, and `TOHA_CONFIG`.
- Linux and macOS: `XDG_CONFIG_HOME` matters only when `TOHA_USER_CONFIG` is unset.
- Linux: also `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME`.
- macOS: `HOME` alone moves the registry, cache, and staged interviews, which live under `~/Library`; the `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` variables are not read.
- Windows: also `APPDATA`, `LOCALAPPDATA`, and `PROGRAMDATA`. `PROGRAMDATA` holds the system configuration and registry, so a system trusted entry applies unless it is set.

On Linux and macOS the system files (`/etc/toha/config.yml`, `/usr/local/share/toha/templates.yml`) have no variable and always apply.
