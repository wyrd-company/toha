---
docs: true
title: Scripts and agents
order: 4
relationships:
  describes: toha
  references:
    - interview-protocol
    - command-line-interface
---

`stage --async` saves an interview and prints its first question batch. The
batch contains a JSON Schema for the questions available now. A script or agent
sends a JSON answers document keyed by question id. `continue` returns either
the next batch or a completed result. `apply` writes files only after the
interview is complete.

```sh
toha stage ./docs/examples/demo ./notes --async
echo '{"title":"Sample note","topic":"Research"}' | toha continue ./notes -
toha apply ./notes
```

`continue` on a complete interview prints the complete result again and exits
0. `apply <template> <target>` with an interview of that template staged at the
target applies it as `apply <target>` does. A command refused for the staged
state of the target names the command that fits on standard error.

`stage --async` exits 4 when it emits a batch. `continue` exits 4 when questions
remain or answers are rejected, and 0 when complete. `apply` exits 0 after
writing, 3 when hooks need trust, 4 for an incomplete staged interview, and 1
for a conflict or other failure. An ambiguous installed template name exits 5.
See the
[CLI specification](https://github.com/wyrd-company/toha/blob/main/docs/specifications/command-line-interface.yml)
for all exit codes.

For a headless run, pass a JSON answers file to `apply`:

```sh
printf '%s\n' '{"title":"Sample note","topic":"Research"}' > answers.json
toha apply ./docs/examples/demo ./notes --answers answers.json
```

A headless run with a missing required answer stages the interview and emits a
batch; with `--dry-run` it emits the batch and stages nothing. Answers for later
questions can be supplied early; unknown ids are errors. The
[interview protocol](https://github.com/wyrd-company/toha/blob/main/docs/specifications/interview-protocol.yml)
defines batch, result, and answers documents.

The binary carries agent guidance. Use `toha skills list` to discover it,
`toha skills view <name>` to read one skill, or
`toha skills view <name> --export <path>` to export it for an agent.
