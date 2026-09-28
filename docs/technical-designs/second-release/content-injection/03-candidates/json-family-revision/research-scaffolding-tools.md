# Scaffolding tools: content injection into existing files

Evidence gathered from actual source of the clones under `/workspaces/references/toha/`. No files under those repos were modified.

---

## cookiecutter

- Revision: `c88fbe921c97c58b65f1883ba90a0ab53cc91b34` (2026-03-04 16:13:03 +0800)
- Verdict: **WHOLE-FILE-ONLY**

`generate_file()` in `cookiecutter/generate.py:175-260` renders each template file with Jinja and writes it with a plain truncating open:

```
cookiecutter/generate.py:256-257
    with open(outfile, 'w', encoding='utf-8', newline=newline) as fh:
        fh.write(rendered_file)
```

The only existing-file awareness is `skip_if_file_exists` (`generate.py:213-215`), which *skips* the file entirely if it already exists — it never edits/merges. No merge, patch, marker, or region logic anywhere in the package. JSON/YAML use (`config.py:70`, `generate.py:144`, `replay.py:37/45`) is limited to config/replay-context files, not to merging generated output into existing files.

---

## copier

- Revision: `01c68328a384777da8a4297b9e511b0e2e57f8c1` (2026-09-18 20:11:55 +0200)
- Verdict: **UPDATE-VIA-DIFF + GIT 3-WAY MERGE** (regenerate-then-patch, not marker injection)

Copier's `update` subcommand (`copier/_cli.py:375-396`, doc string: *"this command will do its best to respect the diff that you have generated since the last `copier` execution"*) does NOT do structural/marker injection. Mechanism, all in `copier/_main.py` inside the update path:

1. Regenerate the template output at the **old** answers/commit into a temp dir (`old_copy`) and at the **new** commit into another temp dir (`new_copy`) — both are ordinary whole-file `run_copy()` passes (`_main.py:1465-1502`).
2. Compute a real `git diff-tree` between the two temp trees:
   ```
   copier/_main.py:1530-1535
   diff_cmd = git[
       "diff-tree",
       f"--unified={self.context_lines}",
       "HEAD",
       subproject_head,
   ]
   ```
3. Apply that diff to the actual subproject with `git apply --reject --exclude ...` (`_main.py:1566-1588`).
4. If `conflict == "inline"` (the default — `_main.py:266`, docstring `_main.py:220-221` "One of \"inline\" (default), \"rej\""), any `.rej` rejected hunks are resolved with a real **3-way merge** using `git merge-file`:
   ```
   copier/_main.py:1621-1634
   # 3-way-merge the file directly
   git(
       "merge-file",
       "-L", "before updating",
       "-L", "last update",
       "-L", "after updating",
       fname,
       old_path / fname,
       new_path / fname,
       retcode=None,
   )
   ```
   producing standard `<<<<<<< before updating` / `>>>>>>> after updating` conflict markers left in the file for the user to resolve manually (checked at `_main.py:1641-1650`), with index stages 1/2/3 populated (`_main.py:1652-1669`) so `git status`/mergetool workflows work.
5. `context_lines` (default 3, `_main.py:267`) tunes diff context to trade off conflict precision vs. respecting subproject drift (docstring `_main.py:223-230`).
6. `copier recopy` (`_cli.py:288-311`) is the non-diff sibling: it ignores subproject evolution and just re-renders, `--overwrite` to clobber.

So: copier's "update" is **regenerate old + regenerate new, git-diff the two, git-apply/3-way-merge that diff into the real subproject** — a textual, git-native diff/patch approach, not a structural (JSON/YAML-aware) merge and not marker-based injection. It fully reserializes nothing; all edits are line-based patches, so existing formatting/comments in untouched regions are preserved by construction (only the diffed hunks change), same as any git patch.

---

## cruft

- Revision: `33f6b722fc6fe4b5d26a351e487372e5e4375ab2` (2024-12-25 14:07:10 +0000)
- Verdict: **UPDATE-VIA-DIFF + GIT 3-WAY MERGE** (same family as copier, simpler)

`cruft/_commands/update.py`:

1. Render the cookiecutter template twice into temp dirs: once pinned at the project's currently-recorded template commit (`current_template_dir`, `update.py:99-107`) and once at the latest template commit (`new_template_dir`, `update.py:119-127`) — both whole-file cookiecutter runs.
2. `_apply_project_updates()` (`update.py:281-318`) computes the diff between those two directories via `utils/diff.py`:
   ```
   cruft/_commands/utils/diff.py:12-25
   def _git_diff(*args):
       return ["git", "-c", "diff.noprefix=", "diff", "--no-index", "--relative",
               "--binary", f"--src-prefix={DIFF_SRC_PREFIX}/", f"--dst-prefix={DIFF_DST_PREFIX}/", *args]
   ```
   (`get_diff`, `diff.py:28-68`, uses `git diff --no-index` between the two rendered trees.)
3. That diff is applied to the real project directory with a 3-way merge first, falling back to reject files:
   ```
   cruft/_commands/update.py:221-244 (_apply_three_way_patch)
       git_apply = ["git", "apply", "-3"]
       ...
   cruft/_commands/update.py:194-218 (_apply_patch_with_rejections, fallback)
       git_apply = ["git", "apply", "--reject"]
   cruft/_commands/update.py:268-278 (_apply_patch)
       if _is_git_repo(expanded_dir_path):
           _apply_three_way_patch(...)
       else:
           _apply_patch_with_rejections(...)
   ```
4. `cruft diff` (`_commands/diff.py`) exposes the same underlying diff machinery standalone, for previewing drift before updating.

Identical shape to copier: regenerate-old, regenerate-new, `git diff --no-index` the two renders, then `git apply -3` (falls back to `--reject`, i.e. `.rej` files, no inline markers unlike copier). No structural JSON/YAML/TOML awareness — pure text patch application.

---

## kickstart

- Revision: `6f5fd80a1910ebee6c2e2039f56c59f6df2edcf2` (2025-12-21 23:49:56 +0100)
- Verdict: **WHOLE-FILE-ONLY**

`write_file()` in `src/utils.rs:26-31` uses `File::create`, which truncates/overwrites unconditionally:

```
kickstart/src/utils.rs:26-31
pub fn write_file(p: &Path, contents: &str) -> Result<()> {
    let mut f = map_io_err(File::create(p), p)?;
    map_io_err(f.write_all(contents.as_bytes()), p)?;
    Ok(())
}
```

Called from `generate()` in `src/generation.rs:334` for every rendered file — no existence check, no merge/patch/marker logic anywhere in `src/*.rs`. No `update` subcommand exists at all (Cargo.toml/src has no such concept); kickstart only ever generates a fresh project tree.

---

## scaffold

- Revision: `475e9bc32b126359740c774a0ededaf5318214da` (2026-08-30 14:24:01 -0500)
- Verdict: **INJECTS-REGIONS** (marker-based textual line injection; the only tool of the five that injects into pre-existing files as a first-class template feature)

Scaffold has a dedicated `inject` feature in the `scaffold.yaml`/`ProjectScaffoldFile` schema (`app/scaffold/project_scaffold_file.go:16,82-88`):

```go
app/scaffold/project_scaffold_file.go:75-88
type Mode string

const (
    Before Mode = "before"
    After  Mode = "after"
)

type Injectable struct {
    Name     string `yaml:"name"`
    Path     string `yaml:"path"`
    At       string `yaml:"at"`
    Mode     Mode   `yaml:"mode"`
    Template string `yaml:"template"`
}
```

The injector itself, `app/scaffold/injector.go:24-82`, is a line-scanner: it reads the target file line by line, finds the first line containing the `at` marker string (`strings.Contains`), and inserts the rendered template's lines immediately before (default) or after that line, preserving the marker line's leading whitespace as indentation for the inserted lines:

```go
app/scaffold/injector.go:27,56-70
func Inject(r io.Reader, data string, at string, mode Mode) ([]byte, error) {
    ...
    for scanner.Scan() {
        line := scanner.Text()
        if strings.Contains(line, at) {
            if mode != After {
                writeLines(linesToInsert, indentation(line))
                inserted = true
            }
            found = true
        }
        writeLine(line)
        if mode == After && found && !inserted {
            writeLines(linesToInsert, indentation(line))
            inserted = true
        }
    }
    ...
    if !found {
        return nil, ErrInjectMarkerNotFound
    }
    return buf.Bytes(), nil
}
```

Wired up in the render pipeline, `app/scaffold/render_funcs.go:733-783` (`RenderRWFS` calls `injectInto` for each `args.Project.Conf.Inject` entry, after the normal whole-file render pass at line 720-728):

```go
app/scaffold/render_funcs.go:733-746
    // Do Injection Jobs
    for _, injection := range args.Project.Conf.Inject {
        path, err := eng.TmplString(injection.Path, vars)
        ...
        if err := injectInto(eng, args, injection, path, vars); err != nil {
            return err
        }
    }
```

```go
app/scaffold/render_funcs.go:752-782 (injectInto)
    f, err := args.WriteFS.Open(path)              // opens the EXISTING target file
    ...
    out, err := eng.TmplString(injection.Template, vars)   // render the injected snippet
    if strings.TrimSpace(out) == "" { return nil }          // empty/whitespace-only render = skip
    perm := defaultFilePerm
    if info, err := f.Stat(); err == nil {
        perm = info.Mode().Perm()                            // preserve existing file's mode
    }
    outbytes, err := Inject(f, out, injection.At, injection.Mode)
    ...
    return args.WriteFS.WriteFile(path, outbytes, perm)
```

Documented at `docs/docs/configuration/scaffold-file.md:200-245`:
- "`inject` is a list of code/text injections to perform on a given file. This is to be used in conjunction with `scaffold templates` and is not supported within a `scaffold project`." (line 202 — i.e. injection is a `scaffold templates`-only feature, not available to whole-project scaffolds)
- `at` — "evaluated using the strings.Contains function. Note that ALL matches will be replaced." (line 216) — NB: despite "ALL matches," the actual `Inject()` loop above stops after the *first* matching line (`inserted` flag prevents re-insertion; loop continues scanning but `found`/`inserted` are never reset) — the doc text appears to overstate the code's behavior; only the first match is used per call. Toha should not assume the doc text is authoritative over the code.
- `mode` defaults to `before` (doc line 233; code default zero-value `Mode("")` also takes the `!= After` branch at `injector.go:57`, matching "before" default in effect if not set to exactly `"after"`).

Confirmed by `injector_test.go` (`TestInject`, lines 10-80+): cases for default/before mode preserving indentation, `after` mode, no-op on empty/whitespace-only injected data, and an error case for missing marker (`ErrInjectMarkerNotFound`, `injector.go:11`).

**Nature of the mechanism**: purely line/text based (`bufio.Scanner` +
`strings.Contains`), format-agnostic — works on YAML, code, anything
line-oriented — but has **no structural awareness** of JSON/YAML/TOML. Text on
each line is copied, and the matched line's leading indentation is reused for
the inserted text. The rewrite is not byte-preserving: `bufio.Scanner` removes
line terminators and `writeLine` appends `\n` to every line
(`app/scaffold/injector.go:24-33,48-72`), so CRLF or mixed line endings become
LF and a missing final newline becomes present. No update/re-sync command reruns
injections — `cmd_update.go` exists (`app/commands/cmd_update.go`) but injection
is invoked only from the render pipeline (`render_funcs.go:733`), i.e.
injection happens at generation time against a target file that must already
exist (typically from a previous scaffold run or hand-authored), not as part of
a diff-based re-sync.

---

## Summary table

| tool | language | injects into existing files? | update mechanism | notes |
|---|---|---|---|---|
| cookiecutter | Python | No | none (no `update` concept) | `generate_file()` always does a truncating `open(outfile, 'w')` write; `skip_if_file_exists` only skips, never merges (`cookiecutter/generate.py:175-260`) |
| copier | Python | No (not marker/region injection) | **regenerate old+new → `git diff-tree` → `git apply --reject` → unresolved hunks get a real `git merge-file` 3-way merge with inline conflict markers** (default `conflict="inline"`) | `copier/_main.py:1465-1669`; `conflict` can be `"rej"` instead (plain `.rej` files, no inline markers); `context_lines` tunes hunk context (`_main.py:220-267`); no JSON/YAML structural merge, pure git text patching |
| cruft | Python | No (not marker/region injection) | **regenerate old+new → `git diff --no-index` → `git apply -3` (3-way), fallback `git apply --reject`** | `cruft/_commands/update.py:194-318`, `cruft/_commands/utils/diff.py`; same family as copier but no inline-conflict-marker mode, just `.rej` fallback |
| kickstart | Rust | No | none (no `update` concept) | `write_file()` uses `File::create` (truncate) unconditionally (`src/utils.rs:26-31`), called from `generate()` (`src/generation.rs:334`) |
| scaffold | Go | **Yes** | N/A for injection itself (not a diff/update mechanism — injection runs once, at render time, against an already-existing target file) | `inject:` config (`Injectable` struct, `project_scaffold_file.go:82-88`) + `Inject()` line-scanner (`injector.go:27-82`) does marker (`at`, `strings.Contains`) + `mode: before\|after` textual insertion, indentation-preserving, skips on empty render; only available for `scaffold templates`, not `scaffold project` (docs `scaffold-file.md:202`); purely textual, no JSON/YAML/TOML structural merge |

**Key takeaway for Toha's design**: none of the five tools does a *structured* (parse/merge/reserialize) merge of JSON/YAML/TOML. The two "update" tools (copier, cruft) both use the same **regenerate-old + regenerate-new + git-diff + git-apply(-3)** pattern — i.e. they don't inject at all in the sense of markers; they replay a computed diff against the live file, falling back to 3-way merge / `.rej` files on conflict. This is the closest prior art to a "git-based-update" sibling design for Toha. Scaffold is the only tool with genuine marker-based content injection into existing files, and it is a narrow, single-shot, line-based (`strings.Contains` + before/after) primitive scoped to its "templates" feature — not integrated with any update/re-sync/diff workflow, and with no structural format awareness.
