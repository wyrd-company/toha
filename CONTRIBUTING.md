---
relationships:
  implements: architecture
---

# Contributing to Toha

Open an issue to report a defect or propose a change. State the observed
behavior, expected behavior, and a small reproduction. For a pull request,
link the issue when one exists and explain the user-visible change and how you
tested it.

Use Rust stable (minimum supported version 1.85) and [Task](https://taskfile.dev).
Install `actionlint` for GitHub workflow validation, `ryl` for YAML validation,
and `intentional` for release intents. Install `vhs` when editing the terminal
demo.

```sh
task build
task test
task ci
```

Tests use templates and answers under `tests/fixtures/`. Add or update a fixture
when behavior changes, and run `task ci` before sending a pull request.

A user-visible change carries one Change Intent in `.intentional/intents/`.
Create it with `intentional add --release-unit toha:minor --message "..."` or
use the bump that fits the change. Run `intentional status` and
`intentional check`. Do not apply an intent in a feature pull request; applying
it is a release operation.

## Releases

A pushed stable SemVer tag that matches the `Cargo.toml` version runs the CD
workflow. The workflow builds the release binaries, creates the GitHub Release,
and then publishes to these channels:

- crates.io: the `toha` crate
- Homebrew: the `wyrd-company/tools` tap
- APT and RPM: `repo.wyrd.foo`
- Documentation: `wyrd.tools/docs/toha`, when the GitHub Release is published

A run started by hand builds the binaries and publishes nothing.

A release requires these repository secrets. The CD workflow names each
missing secret before it builds.

| Secret | Use |
| --- | --- |
| `CARGO_REGISTRY_TOKEN` | crates.io API token with the `publish-new` and `publish-update` scopes for the `toha` crate |
| `FORMULAE_PUBLISH_KEY` | SSH deploy key with write access to `wyrd-company/homebrew-tools` |
| `REPO_WYRD_FOO_PUBLISHER_APP_ID` | Client ID of the `repo.wyrd.foo` publisher GitHub App |
| `REPO_WYRD_FOO_PUBLISHER_PRIVATE_KEY` | Private key of the `repo.wyrd.foo` publisher GitHub App |
| `WYRD_TOOLS_DOCS_PUBLISHER_APP_ID` | App ID of the documentation publisher GitHub App |
| `WYRD_TOOLS_DOCS_PUBLISHER_PRIVATE_KEY` | Private key of the documentation publisher GitHub App |

The GitHub Release, crates.io, and Homebrew steps can run again for the same
tag. Each leaves a channel that already holds the same release unchanged and
fails when the channel holds different content for that version.

`task release:check` runs a crates.io publish dry run, so a package that does
not build from its published files fails the local gate.
