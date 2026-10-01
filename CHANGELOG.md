# Changelog

## 0.2.0

### Features

- Report template, configured-default, early-answer, and target faults at their source
- Expose opt-in hook results to later Jinja: a hook with `id`/`capture` exposes `exit_code`/`stdout`/`stderr` (and, with `parse: json`, its parsed stdout at `<id>` plus metadata at `status-id`) to a later top-level hook's when/run[1..]/args/cwd and to after-apply; default failure behavior and hook trust are unchanged, and captured output never reaches generated files or public output.
- Toha 0.2.0 expands templating end to end: update an existing project by re-applying its installed template; seed a new application's answers from a prior application; richer Jinja with template, host, environment, and context values plus root-confined includes; opt-in Jinja access to hook results; content injection into existing text and JSON, JSONC, and JSON5 files; a confirm-controlled interview and apply flow; recovery from an incomplete headless interview; template-specific configured defaults; template, configured-default, early-answer, and target faults reported at their source; hook-script review and trust management for installed templates; a bundled offline demo; and help and version without a home directory.
- Add root-confined Jinja `{% include %}` in rendered file bodies (source-tree files and `files:` sources), with literal targets, ordered-list fallback, `ignore missing`, and template-root confinement.
- Add in-project generators: an additive `--like [SELECTOR]` flag on `apply` and `stage` that seeds a new application's answer defaults from a snapshot of a prior application of the same source (the generate axis, a sibling of `--from`). Selection is repository-wide and source-filtered; precedence is route answer > snapshot > configured/template default; the staged record pins the chosen snapshot and re-reads it on resume; `applied`/`questions` documents gain an optional `seed` member. No `--like` leaves every route byte-identical.
- Add content injection: inject content into existing files with visible checksum-marked regions for text and typed-path mutation for JSON, JSONC, and JSON5

## 0.1.0

### Features

- Generate projects and files from templates through an interview that people or agents can answer.
