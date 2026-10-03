---
toha: minor
---

Give scripts, agents, and people explicit interview routes: one-shot `--answers`
returns one JSON result, while staged agents receive question batches and
next-command instructions and people receive prompts. External answers documents
require a `template` identity and an `answers` object; mismatches are rejected
before evaluation. Incomplete submissions report the remaining questions and
recovery commands.
