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
