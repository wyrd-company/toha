"""Throwaway spike: convert a Cookiecutter template into a Toha template.

Usage: convert_cookiecutter.py COOKIECUTTER_TEMPLATE OUT_DIR [--context-file NAME]

The converter keeps file bodies untouched wherever it can.  It binds a
computed object named `cookiecutter` after the last question, so every
`{{ cookiecutter.x }}` in a file body, a path, or a hook still resolves.
Everything it cannot carry faithfully is written to `conversion-report.json`.
"""
import json
import re
import shutil
import sys
from pathlib import Path

import yaml

src = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
context_file = sys.argv[sys.argv.index("--context-file") + 1] if "--context-file" in sys.argv else "cookiecutter.json"

report = {"preserved": [], "translated": [], "unsupported": []}
ctx = json.loads((src / context_file).read_text(), object_pairs_hook=lambda kv: dict(kv))
prompts = ctx.pop("__prompts__", {})

ID = re.compile(r"^[a-z_][a-z0-9_]*$")
REF = re.compile(r"""cookiecutter\.(\w+)|cookiecutter\[['"](\w+)['"]\]""")
NOW = re.compile(r"""\{%-?\s*now\s+['"][^'"]*['"]\s*(?:[+-][^,]*)?,\s*['"]([^'"]*)['"]\s*-?%\}""")


def body_rewrites(text: str, where: str) -> str:
    """Rewrite Cookiecutter-only syntax that has a Toha equivalent."""
    new = NOW.sub(lambda m: "{{ now() | dateformat('" + m.group(1) + "') }}", text)
    if new != text:
        report["translated"].append(f"{where}: `{{% now %}}` tag -> `now() | dateformat` (Toha formats in the seed time zone, not the tag's zone)")
    if "| jsonify" in new or "|jsonify" in new:
        new = re.sub(r"\|\s*jsonify\b", "| tojson", new)
        report["translated"].append(f"{where}: `jsonify` -> `tojson` (indent and escaping differ)")
    for name in ("wordwrap", "slugify", "random_ascii_string", "uuid4", "to_nice_yaml", "strftime"):
        if re.search(rf"\b{name}\b", new):
            report["unsupported"].append(f"{where}: `{name}` has no Toha equivalent")
    return new


def default_expr(value: str, where: str) -> str:
    """A default may reference earlier answers as `cookiecutter.x`; the
    `cookiecutter` object does not exist yet at question time, so use the id."""
    return body_rewrites(REF.sub(lambda m: m.group(1) or m.group(2), value), where)


nodes, data = [], {}
for key, value in ctx.items():
    if not ID.match(key):
        report["unsupported"].append(f"variable `{key}`: not a Toha identifier")
        continue
    if key in ("_extensions", "_jinja2_env_vars", "_new_lines", "_copy_without_render", "_template"):
        continue
    prompt = prompts.get(key, key.replace("_", " ").capitalize())
    if isinstance(prompt, dict):
        report["unsupported"].append(f"`__prompts__.{key}`: per-option labels dropped")
        prompt = prompt.get("__prompt__", key)
    if key.startswith("__"):
        # Rendered private variable: a skipped text question takes its default,
        # and a default is a template.
        nodes.append({"id": key, "type": "text", "prompt": key, "when": "false", "default": default_expr(str(value), key)})
        report["translated"].append(f"`{key}`: rendered private variable -> skipped question with a template default")
    elif key.startswith("_"):
        data[key] = value
        report["preserved"].append(f"`{key}`: private variable -> data")
    elif isinstance(value, bool):
        nodes.append({"id": key, "type": "confirm", "prompt": prompt, "default": value})
        report["preserved"].append(f"`{key}`: boolean -> confirm")
    elif isinstance(value, list):
        if any("{" in str(v) for v in value):
            report["unsupported"].append(f"`{key}`: templated choice items (Toha options are literal or one expression)")
        nodes.append({"id": key, "type": "select", "prompt": prompt, "options": [str(v) for v in value], "default": str(value[0])})
        report["preserved"].append(f"`{key}`: choice list -> select")
    elif isinstance(value, dict):
        data[key] = value
        report["unsupported"].append(f"`{key}`: dict variable -> fixed data (Toha has no object question)")
    else:
        if not isinstance(value, str):
            report["translated"].append(f"`{key}`: number -> text (Cookiecutter also stringifies)")
        nodes.append({"id": key, "type": "text", "prompt": prompt, "default": default_expr(str(value), key)})
        report["preserved"].append(f"`{key}`: string -> text with template default")

names = [n["id"] for n in nodes] + list(data)
nodes.append({"id": "cookiecutter", "computed": "{" + ", ".join(f"'{n}': {n}" for n in names) + "}"})

for ext in ctx.get("_extensions", []):
    report["unsupported"].append(f"`_extensions`: Python extension `{ext}`")
for k in ("_jinja2_env_vars", "_new_lines"):
    if k in ctx:
        report["unsupported"].append(f"`{k}`: Jinja environment options")

# Files: the rendered project directory becomes the source subdirectory's only child,
# so Toha writes `<target>/<project dir>/...` exactly as Cookiecutter writes into its output dir.
shutil.rmtree(out, ignore_errors=True)
(out / "template").mkdir(parents=True)
project = next(p for p in src.iterdir() if p.is_dir() and "cookiecutter" in p.name and "{{" in p.name)
flatten = "--flatten" in sys.argv
if flatten:
    # The target is the project root, as Toha targets usually are; the rendered
    # project directory name is not reproduced.
    shutil.rmtree(out / "template")
    shutil.copytree(project, out / "template", symlinks=True)
    report["translated"].append(f"project directory `{project.name}` -> target root (its rendered name is not reproduced)")
else:
    shutil.copytree(project, out / "template" / project.name, symlinks=True)
for f in (out / "template").rglob("*"):
    if f.is_file() and not f.is_symlink():
        try:
            text = f.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        new = body_rewrites(text, str(f.relative_to(out / "template")))
        if new != text:
            f.write_text(new, encoding="utf-8")

doc = {"name": re.sub(r"[^a-z0-9]+", "-", src.name.lower()).strip("-"), "description": f"Converted from Cookiecutter template {src.name}", "interview": nodes}
if data:
    doc["data"] = data
if ctx.get("_copy_without_render"):
    doc["static"] = ["*/" + g for g in ctx["_copy_without_render"]]
    report["translated"].append("`_copy_without_render` -> `static` (globs re-rooted under the project directory)")

hooks = []
for stage in ("pre_prompt", "pre_gen_project", "post_gen_project"):
    for h in sorted((src / "hooks").glob(f"{stage}.*")) if (src / "hooks").is_dir() else []:
        if stage == "pre_prompt":
            report["unsupported"].append(f"hook `{h.name}`: runs before the interview")
            continue
        if h.suffix != ".py":
            report["unsupported"].append(f"hook `{h.name}`: only Python hooks are converted")
            continue
        # Cookiecutter renders hook source through Jinja; Toha renders each `run`
        # argument, so the source travels as one argument to `python3 -c`.
        hook = {"run": ["python3", "-c", body_rewrites(h.read_text(), h.name)]}
        if not flatten:
            hook["cwd"] = project.name
        hooks.append(hook)
        if stage == "pre_gen_project":
            report["translated"].append(f"hook `{h.name}`: runs after files are written, not before (cannot veto generation)")
        else:
            report["translated"].append(f"hook `{h.name}`: post-generation hook -> top-level hook")
if hooks:
    doc["hooks"] = hooks

(out / "template.yml").write_text(yaml.safe_dump(doc, sort_keys=False, width=1000, allow_unicode=True))
(out / "conversion-report.json").write_text(json.dumps(report, indent=2))
print(json.dumps({k: len(v) for k, v in report.items()}))
