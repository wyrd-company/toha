<!--
---
relationships:
  describes: toha
---
-->

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/user/assets/logo-dark.svg">
    <img src="docs/user/assets/logo.svg" alt="Toha logo" width="160">
  </picture>
</p>

# Toha

Generate projects and files from templates.

[![CI](https://github.com/wyrd-company/toha/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/wyrd-company/toha/actions/workflows/ci.yml?query=branch%3Amain)
[![Latest release](https://img.shields.io/github/v/release/wyrd-company/toha)](https://github.com/wyrd-company/toha/releases/latest)

![Toha terminal demo](docs/assets/demo.gif)

Toha is a project scaffolding tool and Rust library. Templates ask questions,
then render file paths and contents with Jinja. Preview the plan before writing.

- Use the same interview from a terminal, scripts, agents, or Rust.
- Resume interviews and supply headless answers, including document answers.
- Keep application inputs and file ownership in a committed `.toha.json` record.
- Select template parts and update generated projects while preserving edits.
- Render typed Jinja values, file paths, loops, and static files.
- Use Git templates, short names, aliases, and hooks gated by trust.
- Read embedded agent guidance with `toha skills`.

Read the [Toha documentation](https://wyrd.foo/docs/toha) for detailed guides.

## Install

With Homebrew on macOS or Linux:

```sh
brew install wyrd-company/tools/toha
```

See [installation methods](https://wyrd.foo/docs/toha/install) for other platforms.

## First run

Toha includes an offline [demo template](docs/examples/demo/template.yml):

```sh
toha apply toha-demo ./notes
```

Answer **Note title** and **Topic**. To preview without writing, run
`toha apply toha-demo ./preview --dry-run`.

## Use from Rust

Use Toha without CLI dependencies to run interviews, manage templates, and
apply or update projects. See [Using Toha from Rust](https://wyrd.foo/docs/toha/using-toha-from-rust)
for the dependency, session flow, and a complete consumer example.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and changes.
