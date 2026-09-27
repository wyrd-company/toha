---
docs: true
title: Getting started
order: 2
relationships:
  describes: toha
  references: command-line-interface
---

## Install

Choose one method for your platform.

### Homebrew (macOS or Linux)

```sh
brew install wyrd-company/tools/toha
```

### APT (Debian or Ubuntu)

Add the Wyrd Company package repository, then install Toha:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://repo.wyrd.foo/pubkey.asc |
  sudo tee /etc/apt/keyrings/wyrd-company.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/wyrd-company.asc] \
https://repo.wyrd.foo/apt stable main" |
  sudo tee /etc/apt/sources.list.d/wyrd-company.list >/dev/null
sudo apt update
sudo apt install toha
```

### RPM (Fedora and other DNF systems)

```sh
sudo curl -fsSL https://repo.wyrd.foo/wyrd.repo -o /etc/yum.repos.d/wyrd.repo
sudo dnf install toha
```

### Arch Linux (AUR)

```sh
paru -S toha-bin
```

### Cargo (any supported Rust platform)

If you have Rust and Cargo installed, build the CLI from the published crate:

```sh
cargo install toha
```

### Release archive (Linux, macOS, or Windows)

Download the archive for your system from the
[latest GitHub release](https://github.com/wyrd-company/toha/releases/latest).
The release includes Linux and macOS tarballs, Windows zip archives, and
`SHA256SUMS`. Check the download against `SHA256SUMS`, extract it, and put the
`toha` executable on your `PATH`.

To use Toha as a Rust library, add the crate without CLI dependencies:

```sh
cargo add toha --no-default-features
```

## Try a template

The Toha repository contains a small demo template. From a directory where
you want to create a note, preview what it will make:

```sh
toha apply gh:wyrd-company/toha#docs/examples/demo ./notes --dry-run
```

Toha asks for **Note title** and **Topic**, then shows the planned file. When
you are ready to write it, run:

```sh
toha apply gh:wyrd-company/toha#docs/examples/demo ./notes
```

The dry run did not save answers, so answer the questions again. Toha writes
`notes/note.txt`. It leaves existing files unchanged unless you pass
`--force`. If a template has hooks, Toha shows them and requires trust before
running them. Use `--trust` for a template you know and trust for one run.

## Ask an agent

You can give an agent a short request and let it read the installed CLI help.
For an existing template:

> Use Toha to generate files from the template I give you in the directory I
> choose. Start with `toha --help`, ask me for the template and target if I have
> not supplied them, and show me the planned changes before applying them.

To create a template:

> Create a Toha template for the files I describe. Start with `toha --help`,
> ask me what the generated files and questions should be, and try the template
> with a dry run before handing it back to me.

Toha also carries agent guidance. Run `toha skills list` to find it and
`toha skills view <name>` to read a skill.
