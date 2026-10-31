# Changelog

## 0.4.1

### Features

- Preserve named user sections inside generated files on apply and update.
- Create empty directories from rendered template paths with optional each and
  when rules. Preserve existing directories and their contents, preview missing
  directories, and create them during apply, baseline and update.

### Fixes

- Allow applying templates into unborn worktrees of bare repositories and honor their working-tree attributes.
- Validate the locked public-library consumer in hosted CI and document refreshing both release lockfiles.

## 0.4.0

### Features

- Select template parts with --with and --without on apply, stage, and update; report revision and part changes.
- Library callers can run an operation with a credential resolver that answers the HTTPS Git remotes it connects to, scoped to the challenging origin and repository, without a credential helper process or a file on disk.
- Expose template resolution, catalog, session and update operations to embedded Rust callers. Preserve rejected update inputs across requests and replay recorded optional empty answers.
- Record template applications in committed `.toha.json` files with a published schema.
  
  Removed: `init`, `snapshots list`, `snapshots clean`, `--from` (use `--update`), and `--like latest` or snapshot ids (use an application target). Remove old `refs/toha/` refs with Git.
- Accept JSON documents as interview answers with schema URI validation, vendored schemas, default files, and --input overrides.

### Fixes

- Remove internal test names from the updating projects guide.
- Ask newly introduced part questions on updates, validate part flags before prompting, and preserve dangling operator links at occupied paths.
- Protect recorded managed regions through update transitions with explicit region force, and refuse whole-file takeovers that replace unowned content.
- Preserve hook symlinks and operator files during template updates, including transaction rollback.
- Clarify applied-record guides, part update commits, and explicit Git cleanup of snapshot refs.
- Clarify how updates ask first-added part questions and how part flags change their defaults.
- Keep the Basic encoding of a supplied HTTPS credential out of library errors when a remote sends it back, as Toha already does for the password.
- Merge template updates correctly when automatic line-ending conversion is in effect and the repository already holds CRLF content. Preserve operator content edits and Git attributes. Refuse templates that overwrite the applied record on Windows.
  
  Keep read-only comparison and in-memory merge-input normalization independent of safecrlf check-in checks while retaining the repository policy for candidate admission. Update preparation can write internal Git objects.

## 0.3.0

### Features

- A `git` question initializes the target as a git repository with an optional `origin` remote. Toha names the branch `main` when `init.defaultBranch` is unset or `master`, and stops before writing when the remote already has history so you can clone it first.
- Templates can list the programs their hooks need in `requires`; Toha confirms each is on PATH before the interview starts and names any that are missing.

### Fixes

- Generate Homebrew formulae that pass strict audit and style checks.
- Fetch HTTPS templates with platform certificate roots when Git has no explicit CA bundle.
- Build Linux release binaries with a glibc 2.17 floor so they run on older supported distributions.

## 0.2.0

### Features

- Try Toha with the bundled `toha-demo` template from any working directory,
  offline and without installing a template. The demo supports preview, apply,
  staged interviews, and project snapshots.
- Give scripts, agents, and people explicit interview routes: one-shot `--answers`
  returns one JSON result, while staged agents receive question batches and
  next-command instructions and people receive prompts. External answers documents
  require a `template` identity and an `answers` object; mismatches are rejected
  before evaluation. Incomplete submissions report the remaining questions and
  recovery commands.
- Review a template's executable hook content before granting trust. Approval is
  bound to the reviewed content, and changes to hook declarations or referenced
  in-template scripts require renewed approval before execution.
- Grant or revoke hook trust for installed templates with `templates trust` and
  `templates untrust`, without reinstalling, fetching, or changing the installed
  revision.
- Control interviews with conditional `flow` nodes that stop, abort and discard
  staged state, preview without applying, or skip the remaining interview or
  current group. Flow actions work across interactive, scripted, staged, and crate
  callers.
- Reuse file-body fragments with Jinja `{% include %}` in generated files,
  `files:` sources, and injection source files. Includes support nesting, ordered
  fallback targets, and `ignore missing`, with all paths confined to the template
  root.
- Use an opted-in hook result in later hooks and after-apply messages through its
  exit code and captured stdout or stderr. Hooks can parse stdout as JSON and
  explicitly tolerate nonzero exits, so later hooks can respond to earlier results
  without exposing captured output in generated files.
- Expose reserved `toha_` Jinja values for the selected template, target basename,
  host platform, and execution context. Trusted templates can also read the
  supported user, hostname, editor, shell, and visual environment values; staged
  interviews preserve the captured context.
- Record rendered files and their answers as Git snapshots after applies into
  clean, committed repositories. List and remove snapshots with `snapshots list`
  and `snapshots clean`, and use `init` to configure a remote fetch refspec for
  sharing snapshots across clones. The Rust crate exposes snapshot capture,
  lookup, and merge APIs.
- Update existing projects with `apply --from`: replay saved answers and merge
  template changes three-way while preserving operator edits. Use `--baseline` to
  adopt an existing project, `--reanswer` to revisit answers, and staged commands
  to continue an update. Previews leave the project unchanged; human and
  staged-agent completion summaries report merge actions, conflicts, messages, and
  snapshot outcomes.
- Seed repeated template applications with `apply --like` or `stage --like`,
  selecting a prior snapshot of the same template as the defaults for a new
  target. Explicit answers override snapshot defaults, which override configured
  and template defaults; staged interviews retain the selected snapshot.
- Report template render faults with their source file, field, and expression.
  Configured-default errors retain the winning mapping and preset locations
  through prompts and staged recovery, so callers can identify and replace the
  rejected value.
- Edit typed values at declared paths in existing JSON, JSONC, and JSON5 files
  while preserving unrelated values and formatting, including supported comments
  and trailing commas. Project updates remove retired owned values and preserve
  files after ownership is released. Building the crate requires Rust 1.88 or
  later.
- Configure independent defaults for each template through `template-defaults`
  mappings keyed by formal name, with reusable values in named `presets`. System,
  user, and local layers merge per template and question. Replace question-id-only
  `defaults` configuration with explicit template mappings.
- Inject content into existing text files with checksum-marked regions placed at
  an anchor or the end of the file. Repeated applies update only the managed
  region, preserve surrounding content, and report operator edits inside the
  region before replacement. Project updates retract removed regions and preserve
  files after ownership is released.

### Fixes

- Remove redundant trailing separators from existing-directory targets so staged
  state, protocol context, and file planning use the same identity. Preserve
  resume and abort of 0.1.0 staged records. Rust target-consuming APIs use
  `CanonicalTarget` from `canonical_target` in place of raw paths.
- Validate early answers according to whether their questions remain active, are
  skipped, or are not yet resolved. Omit errors for questions proven skipped even
  when another answer fails, and keep rejected submissions atomic without leaking
  messages, warnings, or hook effects.
- Allow `--help`, `--version`, `skills list`, and `skills view` to run when
  neither `HOME` nor `USERPROFILE` is set.
- Report template-authored defaults that violate question constraints as template
  faults instead of caller answer rejections. Check skipped-question defaults
  against ready constraints without evaluating unavailable dependencies or prompt
  fields.
- Resolve native Windows absolute template paths, including canonical
  extended-length paths, as folders while preserving lookup of installed templates
  by their registered formal names.

## 0.1.0

### Features

- Generate projects and files from templates through an interview that people or agents can answer.
