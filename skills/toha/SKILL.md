---
name: toha
description: Use when an agent needs to find, install, interview, or apply a toha template, including a staged interview passed between an agent and a person.
relationships:
  references: command-line-interface
---

# Use toha

1. Resolve the template. `toha templates list --json` shows installed formal names, short names, aliases, and trust. Use an alias, unique short name, formal name, git address (`<repository>[@<ref>][#<path>]`), or local folder. Git URLs and configured host prefixes are addresses; paths beginning `.`, `/`, or `~` are folders. Add a template with `toha templates add <address>`, optionally `--alias <name>` for one template, or `--trust` to record trust. `templates update` refreshes branch-following installs; `templates remove` uninstalls; `templates alias` manages aliases. If a short name is ambiguous (exit 5), use a formal name or add an alias. For the exact resolution order, read the CLI specification.
2. Choose the interview mode. For known answers, write a JSON object keyed by question id and run `toha apply <template> <target> --answers answers.json --dry-run`. Use `--answers -` to read standard input. Review the file, message, and hook plan, then run without `--dry-run`.
3. For answers gathered over time, run `toha stage <template> <target> --async`. Exit 4 means the output is a question batch, not a failure. Read `schema.properties`, `schema.required`, and `messages`; submit a JSON object for the current batch with `toha continue <target> -` on standard input. Follow each returned batch until `status: complete` and exit 0. If `errors` appears, correct those ids and submit the batch again; the rejected document recorded no answers. Answers for later questions may be supplied early. See [the tested exchange](references/protocol.md).
4. Apply a complete staged interview with `toha apply <target> --dry-run`, then `toha apply <target>`. `toha abort <target>` deletes staged state. A person can take over with `toha continue <target>` (no answers argument); an agent can resume with `continue <target> -`.

An explicit JSON `null` is an empty answer and does not use a default. Omit a key to let `--answers` use configured or question defaults. A required question rejects `null`. Show `messages` to the caller in order. A batch's `schema` is JSON Schema 2020-12; do not infer answer types from prompt text.

Hooks need registry trust or `--trust` for this run. A folder or address supplied directly is not trusted by default. Inspect the dry run before granting trust. `--force` permits overwriting target files; without it a conflict writes nothing.

| Exit | Action |
| --- | --- |
| 0 | Done; check output or the `status: complete` document. |
| 1 | Read the error: load, missing stage, conflict, render, or hook failure. Correct it before retrying. |
| 2 | Correct the command syntax (`toha <command> --help`). |
| 3 | Hooks need trust; inspect the printed dry run, then use `--trust` if approved. |
| 4 | Read the batch, answer or correct it with `continue`; an incomplete staged interview cannot apply. |
| 5 | Resolve the ambiguous template name with a formal name or alias. |
