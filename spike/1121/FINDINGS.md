# Converting or consuming other template ecosystems

Research spike for Toha. Code in this folder is throwaway evidence, not product code.

## Question

Can Toha convert templates from other ecosystems into Toha templates
(**conversion**), or read them as they are (**direct consumption**)? Which
ecosystems are worth it?

## Answer

- Only **Cookiecutter** and **Copier** are relevant. Cookiecutter has the
  largest corpus (about 10,400 `cookiecutter.json` files on GitHub). Copier has
  the closest model to Toha (about 1,800 `copier.yml` files). Both use Jinja.
- **Conversion is viable** for both. Three real templates converted by
  prototype converters rendered byte-identical trees through Toha. The only
  differences were explained by hooks or by a Python extension. This needs one
  Toha engine change: Python-compatible string methods (minijinja `pycompat`).
- **Direct consumption is technically possible but not recommended now.** It
  uses the same mapping and has the same gaps. It also puts two external
  formats, which change on their own schedules, inside Toha's contract and
  error attribution.
- Recommended next step: an **author-facing, one-time converter** that writes a
  Toha template plus a conversion report. An agent or person then closes the
  residue in the report. Enabling `pycompat` in Toha comes first.

## Candidate ecosystems

| Ecosystem | Model | Engine | Corpus (GitHub) | Verdict |
|---|---|---|---|---|
| Cookiecutter | `cookiecutter.json` + project dir + hooks | Jinja2 | ~10,400 manifests; 25.1k stars | candidate |
| Copier | `copier.yml` + subdirectory + tasks | Jinja2 | ~1,800 manifests; 3.6k stars | candidate |
| Backstage templates | YAML entity + steps | Nunjucks | ~7,000 entities | payloads are mostly Cookiecutter-compatible; steps do not convert |
| kickstart, moon | `template.toml` / `template.yml` | Tera | under 100 public | plausible, too small |
| cargo-generate | `cargo-generate.toml` | Liquid + Rhai | ~950 | needs syntax translation; Rhai hooks do not convert |
| scaffold, boilerplate, boilr, giter8, ffizer, `dotnet new` | declarative | Go template, StringTemplate, Handlebars, literal replace | small or hard | partial |
| Yeoman, Nx, Angular, Plop, Hygen, projen, PyScaffold, Spring Initializr | generator code | EJS, Handlebars, code | large but imperative | implausible |
| degit, GitHub template repos | plain copy | none | n/a | nothing to convert |

Sources: the subagent landscape survey (`gh api` metrics on 2026-10-06), and the
local clones in `/workspaces/references/toha/`.

## Evidence

### E1. Jinja file-body compatibility (engine level)

`run.sh` renders every file body, templated path segment, and hook source of 9
real templates with an environment configured like `src/jinja.rs`. It then
compares each output with the output of the template's own tool.

| Template | Kind | Renders, stock | Renders, `pycompat` |
|---|---|---|---|
| audreyfeldroy/cookiecutter-pypackage | Cookiecutter | 34/36 | 34/36 |
| cookiecutter/cookiecutter-django | Cookiecutter | 188/196 | 190/196 |
| drivendataorg/cookiecutter-data-science | Cookiecutter (own CLI) | 33/34 | 33/34 |
| ionelmc/cookiecutter-pylibrary | Cookiecutter | 50/55 | 50/55 |
| simonw/click-app | Cookiecutter | 12/12 | 12/12 |
| pawamoy/copier-uv | Copier | 47/63 | 47/63 |
| superlinear-ai/substrate | Copier | 33/37 | 36/37 |
| NLeSC/python-template | Copier | 56/66 | 57/66 |
| Tecnativa/doodba-copier-template | Copier | 30/33 | 30/33 |
| **Total** | | **483/532 (91%)** | **489/532 (92%)** |

Byte comparison, with `pycompat`, of each file the native tool also generated:
238 of 247 were identical. Of the 9 that differ, 8 were changed by the
cookiecutter-django post-generation hook (secrets, `uv add`, pre-commit
edits). 1 (copier-uv) used a global from a missing Python extension. minijinja
rendered it as an empty string and did not report an error.

Failure causes:

- `{% now %}` tag (Cookiecutter built-in): 9 surfaces, 4 templates. Converts to
  `now() | dateformat(...)`.
- Unknown filters: `python_comment` (local extension, 13), `strftime` (8),
  `to_nice_yaml` (Ansible filters, 4 templates), `jsonify` (3), `wordwrap` (1).
- Python methods: `.lower()`, `.replace()`, `.split()`, `.rstrip()`,
  `.count()`. `pycompat` fixes all of these except `str.rsplit` and `dict.update`.
- Includes of files absent from a shallow clone (doodba vendored submodule): 2.

### E2. End-to-end through Toha

The prototype converters (`convert_cookiecutter.py`, `convert_copier.py`) write
a `template.yml` and a `conversion-report.json`. Toha then applied each
converted template with the scripted answers route.

| Template | Stock Toha | Toha + `pycompat` | Output vs native |
|---|---|---|---|
| click-app (Cookiecutter, 6 vars) | fails: `.lower()` in a default | applies | identical tree |
| pylibrary (Cookiecutter, 45 vars, Python hook, 2 Python extensions) | fails | applies after 1 hand edit (`wordwrap` removed) | identical except `.cookiecutterrc` (custom extension format) and hook-made `.tox/` |
| substrate (Copier, 15 questions, 5 validators, `when`, labelled choices, conditional dir) | fails: `.rstrip()` in a default | applies | identical except Copier's answers file |

Mapping that worked:

- The Cookiecutter `cookiecutter.x` namespace becomes a computed object named
  `cookiecutter` after the last question. File bodies, paths, and hook sources
  stay unchanged.
- Cookiecutter `__x` (rendered private) and Copier `when: false` become a
  skipped question with a template default. Toha already has these semantics.
- Copier `when` becomes Toha `when`. Copier and Toha both skip a file or
  directory whose rendered name is empty.
- A Cookiecutter Python hook becomes a top-level hook,
  `run: [python3, -c, <hook source>]`. Toha renders each `run` argument, so the
  Jinja in the hook source still renders.
- Copier files without the `.jinja` suffix become `static`. Suffixed files are
  renamed.

Update behaviour (pylibrary, flattened layout):

- The snapshot captured the state after the hook ran. Of 46 planned files, the
  8 the hook deleted were absent from it. The file the hook generated
  (`.github/workflows/github-actions.yml`) was present.
- `apply --from <snapshot>` with no template change previewed no changes.
- A template change and an operator edit to the same file both survived a
  trusted update. The hook-deleted files checked (`setup.py`, `tbump.toml`)
  stayed deleted.

Hazard: Cookiecutter writes a new project directory inside the output
directory. Converted as-is, a hook's `git init` made a nested repository inside
the Toha target. Making the project directory the source root (`--flatten`)
removed the problem. The cost is that the rendered project directory name is
no longer reproduced.

## What is preserved, translated, or lost

| Feature | Cookiecutter | Copier | Toha mapping |
|---|---|---|---|
| Text, bool, choice questions | yes | yes | preserved |
| Defaults that reference earlier answers | yes | yes | preserved, with `pycompat` |
| Conditional questions | none | `when` | preserved |
| Hidden computed values | `__x` | `when: false` | preserved (skipped question) |
| Multiselect | override only | `multiselect` | preserved |
| int / float | stringified | typed | translated (`format: value \| int`) |
| yaml / json / dict answers | dict vars | `yaml`, `json` types | lost: no structured question type |
| Choice labels | `__prompts__` dict | dict `choices` | lost: options are values only |
| Validators | in `pre_gen` hook | Jinja `validator` with message | common shapes become `required` or `validate.regex`; the message and arbitrary logic are lost |
| Secret answers | none | `secret: true` | lost, and unsafe: Toha records raw answers in snapshots |
| Conditional files | hook deletes files (5 of 8 sampled) | empty rendered name | Copier: preserved. Cookiecutter: hook runs after write; the plan lists files the hook later deletes |
| Pre-generation veto | `pre_gen_project` | none | lost: Toha hooks run after files are written |
| Pre-prompt hook | `pre_prompt` | none | lost |
| Post-generation commands | hook script | `_tasks` (shell strings) | translated: `python3 -c` / `sh -c`; trust is now required; depends on the host interpreter |
| Migrations | none | `_migrations` | lost: no Toha equivalent |
| Python Jinja extensions | `_extensions` (3 of 8) | `_jinja_extensions` (3 of 13) | lost: not portable outside Python |
| `{% import %}`, `{% from %}`, `{% extends %}` | rare | 6 of 13 (larger templates) | lost: Toha does not offer these |
| `{% include %}` | rare | common | preserved for literal targets only |
| Updates | cruft (re-render + `git apply -3`) | three-way regenerate | Toha snapshots carry this; a converted template updates with Toha's own model |
| Sources | `gh:`, git, zip, `--directory` | git, `gh:`, tags | Toha sources; subdirectory addressing is outside this spike |

## Approach comparison

| Concern | Conversion | Direct consumption |
|---|---|---|
| Fit with one interview engine | Output is an ordinary Toha template | A second loader produces the same domain types; fits "parse once at the boundary" |
| Plan before apply | Holds for Copier; Cookiecutter hook deletions make the plan list files that do not land | Same |
| Error attribution | Errors name the converted `template.yml` | Errors must name foreign fields (`copier.yml/x.validator`); new attribution work |
| Upstream template changes | Do not flow; the author re-converts | Flow through Toha sources and updates |
| Residue (validators, hooks, extensions) | A person or agent fixes it once, from the report | Must be resolved at every load, or refused |
| Maintenance | Converter tracks two formats; breakage affects only new conversions | Toha's contract tracks two formats (Cookiecutter 2.x, Copier 9.x); breakage affects users at apply time |
| Dependencies | None beyond `pycompat` (minijinja-contrib, same author, Apache-2.0) | Same |
| Licensing | Toha reads formats only. A converted template is a derived work under the template's own license (BSD, MIT, etc.) | Toha reads formats only |
| Trust | Converted hooks and tasks need Toha trust. Stricter than Cookiecutter, which runs hooks without asking | Same |
| Effort | Small: two converters about 200 lines each in the spike; production needs tests and a report format | Large: two loaders, attribution, a compatibility policy, fixtures per format |

## Toha contract changes this would need (for a later decision)

1. Python-compatible string and map methods in every Jinja surface (minijinja
   `pycompat`). Without it, 2 of 3 end-to-end templates fail. Every template
   gains these methods, so this widens the contract.
2. Optional extra filters: `wordwrap` (minijinja-contrib), and Ansible
   `to_nice_yaml` as an alias of `toyaml`.
3. Optional, for higher Copier fidelity: expression validators with messages,
   labelled options, secret questions that snapshots do not record, and
   structured answer types.
4. Not recommended: pre-write hooks, `import`/`extends`, Copier migrations.
   Each is a large contract change for a small share of templates.

## Assumptions and limits

- The sample is 9 templates for the probe and 3 end to end. They were chosen by
  stars and by feature coverage, not at random.
- The probe checks the minijinja engine. It does not check Toha's load-time
  rules (unknown-id checks, literal-only includes). The end-to-end runs do check them.
- Defaults were used for most answers. Other answer combinations exercise other
  branches.
- Corpus counts come from GitHub code search, which is partial and noisy.
- The minijinja `Lenient` undefined rendered a missing extension global as
  empty. For a converted template, whether Toha's load-time id check catches
  this was not tested.

## Reproduce

```sh
# Clones used (shallow) into /tmp/eco/<owner>_<repo>; see run.sh for the list.
(cd jinja-probe && cargo build && cargo build --features pycompat --target-dir target-py)
./run.sh && python3 summarize.py /tmp/eco-probe/py/*.jsonl
uv run --with pyyaml python convert_cookiecutter.py /tmp/eco/simonw_click-app /tmp/out/click-app
uv run --with pyyaml python convert_copier.py /tmp/eco/superlinear-ai_substrate /tmp/out/substrate
# toha-pycompat.patch is the engine change used for the "+ pycompat" columns.
```
