---
relationships:
  implements: architecture
---

# Contributing to Toha

Open an issue to report a defect or propose a change. State the observed
behavior, expected behavior, and a small reproduction. For a pull request,
link the issue when one exists and explain the user-visible change and how you
tested it.

Use Rust stable (minimum supported version 1.88) and [Task](https://taskfile.dev).
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

### Preparing a release

Prepare the release on the integration branch and hand the built package to
acceptance. Each step names the tool that owns it; see
[`Taskfile.yml`](Taskfile.yml), [`.intentional/config.yml`](.intentional/config.yml),
[`.github/workflows/cd.yml`](.github/workflows/cd.yml), and
[`scripts/release/nfpm.yml`](scripts/release/nfpm.yml) for their definitions.

1. Record one consumer-facing intent for the release and commit it alone, so its
   later consumption has the addition in history:

   ```sh
   intentional add --release-unit toha:minor --message "<consumer summary>"
   git add .intentional/intents/<generated>.md
   git commit -m "chore(intent): record the <version> release summary intent (toha:minor)"
   ```

2. Seal the plan and keep its bytes unchanged. `intentional plan` reports
   `<version>` for release unit `toha`:

   ```sh
   intentional plan > release-plan.json
   ```

3. Apply the release and refresh the lock file. `intentional apply` writes
   `Cargo.toml` and `CHANGELOG.md` and consumes the intents; `cargo update -p
   toha` refreshes the `toha` entry in `Cargo.lock` that Intentional does not
   project:

   ```sh
   intentional apply --dry-run
   intentional apply
   cargo update -p toha
   ```

4. Commit the applied release as one commit: `Cargo.toml`, `Cargo.lock`,
   `CHANGELOG.md`, and the consumed intent deletions.

5. Validate the applied commit. `intentional status` reports no pending intents,
   tag authority at the previous version, and manifest drift at the new version;
   `task ci` runs `fmt-check`, `lint`, `test`, and `release:check`; the tag dry
   run proposes the bare `<version>` annotated tag without creating it:

   ```sh
   intentional status
   intentional check
   task ci
   intentional tag --plan release-plan.json --dry-run
   ```

6. Build the host Linux x86_64 package with the same steps the CD build job runs:

   ```sh
   cargo build --release --locked --target x86_64-unknown-linux-gnu
   mkdir -p stage dist
   cp target/x86_64-unknown-linux-gnu/release/toha LICENSE README.md stage/
   tar -C stage -czf dist/toha_<version>_linux_x86_64.tar.gz .
   cp stage/toha ./toha
   RELEASE_VERSION=<version> PACKAGE_ARCH=amd64 nfpm package \
     --config scripts/release/nfpm.yml --packager deb \
     --target dist/toha_<version>_linux_x86_64.deb
   RELEASE_VERSION=<version> PACKAGE_ARCH=amd64 nfpm package \
     --config scripts/release/nfpm.yml --packager rpm \
     --target dist/toha_<version>_linux_x86_64.rpm
   ```

7. Retain the handoff in a durable location outside the working tree and record
   checksums, so it survives working-tree cleanup:

   ```sh
   cp release-plan.json dist/toha_<version>_* <durable-dir>/
   (cd <durable-dir> && sha256sum toha_* > SHA256SUMS && sha256sum -c SHA256SUMS)
   ```

   The retained handoff holds the sealed `release-plan.json`, the three package
   files (`toha_<version>_linux_x86_64.tar.gz`, `.deb`, and `.rpm`), and
   `SHA256SUMS`. Remove the generated `dist/`, `stage/`, and repository-root
   `toha` from the working tree. Then stop.

Only Bob creates the release tag, after acceptance and the merge. On the merged
commit he runs `intentional tag --plan release-plan.json --dry-run`, then the
same command without `--dry-run`, confirms the annotated bare `<version>` tag,
and pushes it. That tag push triggers the CD workflow.

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
