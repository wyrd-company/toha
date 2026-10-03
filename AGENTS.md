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
- **Snapshot:** The stored output of one apply, kept as a parentless git commit under a Toha-owned ref (`refs/toha/snapshots/<id>`), holding the rendered files and the answers that produced them.
- **Base snapshot:** The snapshot an update merges from, three-way, to carry a template's changes into a project while the operator's edits survive.
- **Baseline:** An apply that adopts an existing project against an empty base, recording the first snapshot without a prior one.
- **Generator:** An apply pattern that writes one template repeatedly into project subpaths, each application carrying its own answers and identity `(source, target)`; not a `template.yml` object.
- **Seed selector:** The `--like` value that names a snapshot whose recorded answers pre-fill a new application's defaults — `latest`, a snapshot id or prefix, or a person-route picker.
- **Snapshot-seeded default:** A default that enters the interview engine as a `Snapshot` bank entry, folded from a selected snapshot's submissions; a route answer still overrides it and a wrong-kind value is dropped.

## Thar be dragons

Nothing is recorded yet.

## Repository orientation

- `docs/specifications/` holds the contract and JSON Schemas for the binary.
- `tests/fixtures/` holds template folders, answers documents, expectations, and expected trees.
- `task ci` runs the local gate.
