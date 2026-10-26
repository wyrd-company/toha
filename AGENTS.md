Toha is a project scaffolding tool and Rust crate. A template folder contains `template.yml`, a source subdirectory, and support material. Toha interviews the caller and renders source files and paths with Jinja.

The interview engine is a state machine shared by crate callers and command drivers. This slice supports text questions and headless answers files.

## Why this exists

Toha generates files from reusable templates and lets people, scripts, and agents provide answers through the same interview rules.

## Principles

- Use one interview engine for every driver.
- Parse external input once at its boundary into domain types.
- Keep the interview engine pure and make plans before applying them.
- Treat specifications and schemas as the contract.
- Exercise the full path with fixtures.
- When a command refuses a valid request made at the wrong time or in the wrong form, and nothing is faulty, the message names the command or commands that do what the caller meant.

## Glossary

- **Template:** A folder that defines questions and generated files.
- **Template root:** The folder that contains `template.yml`.
- **Source subdirectory:** The folder of files to render; `template/` by default.
- **Interview:** The state machine that accepts answers and returns questions.
- **Node:** One entry in an interview.
- **Batch:** The questions available before an unanswered dependency.
- **Answers document:** A JSON object keyed by question id.
- **Staged interview:** An interview saved for later continuation.
- **Formal name:** A template's source address.
- **Short name:** The `name` in `template.yml`.
- **Alias:** A user-chosen template name.
- **Trust:** Permission to run template hooks.
- **Plan:** The files and conflicts computed before writing.
- **Applied record:** The committed `.toha.json` at the repository root, or the `TOHA_RECORD` path, recording each template application's identity, answers, parts, context and ownership.
- **Application:** One source and target pair in the applied record.
- **Merge base:** The ancestor bytes used in a three-way update, rendered from the application's recorded inputs and verified against its blob identities.
- **Baseline:** An adoption against an empty merge base that writes an application entry.
- **Re-baseline:** An explicit `--baseline` adoption when an application has no reproducible merge base.
- **Generator:** An apply pattern that writes one template into project subpaths; each source and target pair has its own application entry.
- **Seed selector:** The `--like` target naming another application of the same source, or a bare person-route picker.
- **Application-seeded default:** A raw answer copied from the seed application; a route answer overrides it and a wrong-kind value is dropped.

## Thar be dragons

Nothing is recorded yet.

## Repository orientation

- `docs/user/` holds the public docs that wyrd.foo publishes: `docs.yml`, the pages and their assets. Everything else under `docs/` is contributor docs.
- `docs/specifications/` holds the contract and JSON Schemas for the binary.
- `tests/fixtures/` holds template folders, answers documents, expectations, and expected trees.
- `task ci` runs the local gate.
