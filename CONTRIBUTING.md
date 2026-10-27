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

Linux release builds require `cargo-zigbuild` 0.23.4 and Zig 0.14.1. Install
`cargo-zigbuild` with `cargo install --locked cargo-zigbuild --version 0.23.4`
and put the Zig executable on `PATH`. The CD workflow installs Zig from
[`scripts/release/zig-requirements.txt`](scripts/release/zig-requirements.txt)
in a Python virtual environment with wheel hash verification.

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

The public docs in `docs/user/` are their own release unit, `toha-docs`. A change
to those docs alone carries an intent for `toha-docs`, so the docs release
without a new `toha` version.

## Releases

A pushed stable SemVer tag that matches the `Cargo.toml` version runs the CD
workflow. The workflow builds the release binaries, creates the GitHub Release,
and then publishes to these channels:

- crates.io: the `toha` crate
- Homebrew: the `wyrd-company/tools` tap
- APT and RPM: `repo.wyrd.foo`
- Documentation: `wyrd.foo/docs/toha`, through the Publish docs workflow after
  the CD workflow succeeds

A run started by hand in the default `build-only` mode builds the binaries and
publishes nothing. The `crates-0.4.0-recovery` mode publishes only the `toha`
0.4.0 crate; see [Recovering the 0.4.0 crate](#recovering-the-040-crate).

A pushed `toha-docs@<version>` tag publishes the docs alone, and a manual run of
the Publish docs workflow publishes the docs at a given tag or commit. A push to
`main` publishes nothing.

### Preparing a release

Prepare the release on the integration branch and hand the built package to
acceptance. Each step names the tool that owns it; see
[`Taskfile.yml`](Taskfile.yml), [`.intentional/config.yml`](.intentional/config.yml),
[`.github/workflows/cd.yml`](.github/workflows/cd.yml), and
[`scripts/release/nfpm.yml`](scripts/release/nfpm.yml) for their definitions.

1. Seal the plan and keep its bytes unchanged. `intentional plan` reports
   `<version>` for release unit `toha`:

   ```sh
   intentional plan > release-plan.json
   ```

2. Apply the release and refresh the lock file. `intentional apply` writes
   `Cargo.toml`, `CHANGELOG.md` and the versions in `docs/user/docs.yml`, and
   consumes the intents; `cargo update -p
   toha` refreshes the `toha` entry in `Cargo.lock` that Intentional does not
   project:

   ```sh
   intentional apply --dry-run
   intentional apply
   cargo update -p toha
   ```

3. Commit the applied release as one commit: `Cargo.toml`, `Cargo.lock`,
   `CHANGELOG.md`, `docs/user/docs.yml`, and the consumed intent deletions.

4. Validate the applied commit. `intentional status` reports no pending intents,
   tag authority at the previous version, and manifest drift at the new version;
   `task ci` runs `fmt-check`, `lint`, `test`, and `release:check`; the tag dry
   run proposes the bare `<version>` annotated tag without creating it:

   ```sh
   intentional status
   intentional check
   task ci
   intentional tag --plan release-plan.json --dry-run
   ```

5. Build the host Linux x86_64 package with the same steps the CD build job runs:

   ```sh
   LIBCURL_NO_PKG_CONFIG=1 LIBZ_SYS_STATIC=1 \
     cargo zigbuild --release --locked --target x86_64-unknown-linux-gnu.2.17
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

6. Retain the handoff in a durable location outside the working tree and record
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
| `FORMULAE_PUBLISH_KEY` | SSH deploy key with write access to `wyrd-company/homebrew-tools` |
| `REPO_WYRD_FOO_PUBLISHER_APP_ID` | Client ID of the `repo.wyrd.foo` publisher GitHub App |
| `REPO_WYRD_FOO_PUBLISHER_PRIVATE_KEY` | Private key of the `repo.wyrd.foo` publisher GitHub App |
| `WYRD_TOOLS_DOCS_PUBLISHER_APP_ID` | App ID of the documentation publisher GitHub App |
| `WYRD_TOOLS_DOCS_PUBLISHER_PRIVATE_KEY` | Private key of the documentation publisher GitHub App |

crates.io publication uses
[Trusted Publishing](https://crates.io/docs/trusted-publishing) and needs no
repository secret. The `toha` crate accepts new versions only through Trusted
Publishing, from repository `wyrd-company/toha` and workflow `cd.yml` with no
environment. Each crate publishing job exchanges the run's GitHub OIDC token for
a temporary crates.io token with the pinned `rust-lang/crates-io-auth-action`,
right before publication. Only the publication step receives the token, and the
action revokes it when the job ends. A rejected exchange stops the job before
publication; there is no stored-token fallback.

The GitHub Release, crates.io, and Homebrew steps can run again for the same
tag. Each leaves a channel that already holds the same release unchanged and
fails when the channel holds different content for that version. The crates.io
step authenticates before it reads the index, so a run that finds matching
package bytes also needs a successful exchange.
Same-tag retries reject formula bytes that differ from the tap. Publish formula
corrections with the next product release.

`task release:check` runs a crates.io publish dry run, so a package that does
not build from its published files fails the local gate.

### Recovering the 0.4.0 crate

The `crates-0.4.0-recovery` mode publishes or verifies the `toha` 0.4.0 crate
from its released source. Dispatch it on `main`:

```sh
gh workflow run cd.yml --ref main -f mode=crates-0.4.0-recovery
```

The run starts only the `crates-recovery` job: no credential preflight, build,
GitHub Release, Homebrew, or package job. The job checks out commit
`e85881a5cab643488fe0000b982177d18784fad1` and stops before authentication
unless the run is on `main`, HEAD is that commit, `0.4.0` is the annotated tag
object `eebc73aa821c9e764f7ec29be8667c7810cb79e7` for that commit, `Cargo.toml`
declares 0.4.0, `Cargo.lock` is committed, the checkout is clean, and the locked
package's `.cargo_vcs_info.json` records that commit. It then runs that commit's
`scripts/release/ensure-crate.sh 0.4.0` with Rust 1.88.0 and logs the package
checksum. The Trusted Publishing provenance of the run names the `main` workflow
commit; the package names the release commit.
