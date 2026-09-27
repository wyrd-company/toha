---
docs: true
title: Installation
order: 2
install: true
relationships:
  describes: toha
  references: command-line-interface
---

Choose one method for your platform. The commands below install the `toha`
command-line tool.

## Homebrew (macOS or Linux)

```sh
brew install wyrd-company/tools/toha
```

## APT (Debian or Ubuntu)

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

## RPM (Fedora and other DNF systems)

```sh
sudo curl -fsSL https://repo.wyrd.foo/wyrd.repo -o /etc/yum.repos.d/wyrd.repo
sudo dnf install toha
```

## Arch Linux (AUR)

```sh
paru -S toha-bin
```

## Cargo (any supported Rust platform)

If you have Rust and Cargo installed, build the CLI from the published crate:

```sh
cargo install toha
```

## Release archive (Linux, macOS, or Windows)

Download the archive for your system from the
[latest GitHub release](https://github.com/wyrd-company/toha/releases/latest).
The release includes Linux and macOS tarballs, Windows zip archives, and
`SHA256SUMS`. Check the download against `SHA256SUMS`, extract it, and put the
`toha` executable on your `PATH`.

## Rust library

To use Toha as a Rust library, add the crate without CLI dependencies:

```sh
cargo add toha --no-default-features
```
