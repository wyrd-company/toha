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

## Thar be dragons

Nothing is recorded yet.

## Repository orientation

- `docs/specifications/` holds the contract and JSON Schemas for the binary.
- `tests/fixtures/` holds template folders, answers documents, expectations, and expected trees.
- `task ci` runs the local gate.
