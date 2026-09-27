---
docs: true
title: Getting started
order: 3
relationships:
  describes: toha
  references:
    - command-line-interface
    - template-format
---

Install Toha with Homebrew:

```sh
brew install wyrd-company/tools/toha
```

Other package managers and release archives are listed in the
[README](https://github.com/wyrd-company/toha#install).

The
[demo template](https://github.com/wyrd-company/toha/blob/main/docs/examples/demo/template.yml)
is a small local template. From the repository root, run:

```sh
toha apply ./docs/examples/demo ./notes
```

Answer **Note title** and **Topic**. Toha writes `notes/note.txt`. Preview the
same operation without writing files:

```sh
toha apply ./docs/examples/demo ./preview --dry-run
```

Toha leaves existing files unchanged unless you pass `--force`. A template with
hooks needs trust before the hooks can run. For a template you know, `--trust`
allows hooks for one run; an installed trusted template keeps that choice in the
user registry. Without trust, Toha prints a dry run and exits with code 3. See
the
[CLI specification](https://github.com/wyrd-company/toha/blob/main/docs/specifications/command-line-interface.yml)
for the full contract.
