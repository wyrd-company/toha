---
relationships:
  describes: toha
---

# toha

A project scaffolding tool and Rust crate.

Toha generates files and directory trees from templates. A template asks an
interview, combines the answers with static and computed data, and renders its
files through [Jinja](https://github.com/mitsuhiko/minijinja). People answer
the interview in the terminal. Scripts and agents answer it with JSON, one batch
at a time, across several processes.

- **One binary** for Linux, macOS, and Windows, with no runtime to install.
- **Looped questions** collect lists without asking anyone to type YAML.
- **Conditions, computed values, and defaults** are all Jinja.
- **Staged interviews** separate answering from writing files.
- **Git templates** install by address, such as `gh:org/repo@v1.2#rust-cli`,
  with several templates in one repository.
- **Embedded agent skills** teach agents to drive toha and to author
  templates.

## A template

```text
rust-cli/
├── template.yml     # name, interview, data, files, hooks, messages
├── template/        # files rendered into the target directory
└── parts/           # support files, rendered only when referenced
```

```yaml
name: rust-cli
interview:
  - id: name
    type: text
    prompt: Crate name
    required: true
  - id: commands
    type: text
    prompt: "Subcommand name (empty to finish)"
    loop: { min: 1 }
files:
  - each: commands as cmd
    source: parts/command.rs
    path: "src/commands/{{ cmd | snake }}.rs"
```

More examples are in [docs/examples](docs/examples/README.md).

## Usage

```sh
# Interview and write files
toha apply gh:org/templates#rust-cli ./my-tool

# Install templates and use them by name
toha templates add gh:org/templates --trust
toha apply rust-cli ./my-tool

# Answer an interview from a script or agent
toha stage rust-cli ./my-tool --async
toha continue ./my-tool answers.json
toha apply ./my-tool
```

Hooks run only for trusted templates. Without trust, `apply` prints a dry run
and exits with code 3.

## Documentation

| Document | Defines |
| --- | --- |
| [Concept](docs/concepts/toha.yml) | What toha is and does |
| [Template format](docs/specifications/template-format.yml) | `template.yml`, the interview, files, hooks, and messages |
| [Interview protocol](docs/specifications/interview-protocol.yml) | Question batches, answers, and staged state |
| [Configuration](docs/specifications/config.yml) | System, user, and local configuration |
| [Command-line interface](docs/specifications/command-line-interface.yml) | Commands, addresses, names, and exit codes |

## License

See [LICENSE](LICENSE).
