---
toha: minor
---

Update existing projects with `apply --from`: replay saved answers and merge
template changes three-way while preserving operator edits. Use `--baseline` to
adopt an existing project, `--reanswer` to revisit answers, and staged commands
to continue an update. Previews leave the project unchanged; human and
staged-agent completion summaries report merge actions, conflicts, messages, and
snapshot outcomes.
