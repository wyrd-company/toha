---
toha: minor
---

Expose opt-in hook results to later Jinja: a hook with `id`/`capture` exposes `exit_code`/`stdout`/`stderr` (and, with `parse: json`, its parsed stdout at `<id>` plus metadata at `status-id`) to a later top-level hook's when/run[1..]/args/cwd and to after-apply; default failure behavior and hook trust are unchanged, and captured output never reaches generated files or public output.
