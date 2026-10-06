"""Throwaway spike: convert a Copier template into a Toha template.

Usage: convert_copier.py COPIER_TEMPLATE OUT_DIR

Handles a single `copier.yml` without `!include`.  Everything it cannot carry
faithfully is written to `conversion-report.json`.
"""
import json
import re
import shutil
import sys
from pathlib import Path

import yaml

src = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
report = {"preserved": [], "translated": [], "unsupported": []}
conf_path = next(p for p in (src / "copier.yml", src / "copier.yaml") if p.exists())
conf = yaml.safe_load(conf_path.read_text())

settings = {k: v for k, v in conf.items() if k.startswith("_")}
questions = {k: v for k, v in conf.items() if not k.startswith("_")}
suffix = settings.get("_templates_suffix", ".jinja")
TYPES = {"str": "text", "int": "text", "float": "text", "bool": "confirm"}


def expr(template: str) -> str:
    """Copier `when` is a template rendered then cast to bool; Toha `when` is an expression."""
    m = re.fullmatch(r"\s*\{\{(.*)\}\}\s*", str(template), re.S)
    return m.group(1).strip() if m else json.dumps(template)


def validator(q: dict, key: str) -> dict:
    """Recognize the common validator shapes seen in the corpus; report the rest."""
    v = q.get("validator")
    if not v:
        return {}
    body = re.sub(r"\s+", " ", v)
    if re.search(rf"if not {key}(\.strip\(\))? %", body):
        report["translated"].append(f"`{key}`.validator: non-empty check -> `required: true`")
        return {"required": True}
    m = re.search(rf"""if not {key}\.startswith\(["']([^"']+)["']\)""", body)
    if m:
        report["translated"].append(f"`{key}`.validator: prefix check -> `validate.regex` (custom message lost)")
        return {"validate": {"regex": "^" + re.escape(m.group(1))}}
    m = re.search(rf"""if not ["']([^"']+)["'] in {key}""", body)
    if m:
        report["translated"].append(f"`{key}`.validator: contains check -> `validate.regex` (custom message lost)")
        return {"validate": {"regex": re.escape(m.group(1))}}
    report["unsupported"].append(f"`{key}`.validator: arbitrary Jinja validator")
    return {}


nodes = []
for key, q in questions.items():
    if not isinstance(q, dict):
        q = {"default": q}
    qtype = q.get("type") or ("bool" if isinstance(q.get("default"), bool) else "str")
    if qtype in ("yaml", "json"):
        report["unsupported"].append(f"`{key}`: `{qtype}` question type (structured answer)")
    node = {"id": key}
    choices = q.get("choices")
    if choices is not None:
        if isinstance(choices, dict):
            report["translated"].append(f"`{key}`: labelled choices -> values only (labels lost)")
            choices = list(choices.values())
        if any(isinstance(c, (list, dict)) for c in choices):
            report["unsupported"].append(f"`{key}`: choice with validator or structured value")
        node["type"] = "multiselect" if q.get("multiselect") else "select"
        node["options"] = [str(c) for c in choices]
    else:
        node["type"] = TYPES.get(qtype, "text")
        if qtype in ("int", "float"):
            node["format"] = f"value | {qtype}"
            report["translated"].append(f"`{key}`: `{qtype}` -> text with `format`")
    node["prompt"] = q.get("help", key)
    if "placeholder" in q:
        node["placeholder"] = str(q["placeholder"])
    if "default" in q:
        node["default"] = q["default"] if isinstance(q["default"], (bool, list)) else str(q["default"])
    if "when" in q:
        node["when"] = "false" if q["when"] is False else expr(q["when"])
        if q["when"] is False:
            report["preserved"].append(f"`{key}`: hidden computed value (`when: false`)")
    if q.get("secret"):
        report["unsupported"].append(f"`{key}`: secret answer (Toha records raw answers in snapshots)")
    node.update(validator(q, key))
    nodes.append(node)
    report["preserved"].append(f"`{key}`: question")

shutil.rmtree(out, ignore_errors=True)
sub = src / settings.get("_subdirectory", "")
static = []
for p in sorted(sub.rglob("*")):
    rel = p.relative_to(sub)
    if "_copier_conf.answers_file" in str(rel):
        report["translated"].append(f"`{rel}`: Copier answers file dropped (Toha records answers in snapshots)")
        continue
    if p.is_dir():
        continue
    dest = out / "template" / rel
    if suffix and p.name.endswith(suffix):
        dest = dest.with_name(p.name[: -len(suffix)])
    else:
        static.append(str(rel))
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(p, dest, follow_symlinks=False)

doc = {"name": re.sub(r"[^a-z0-9]+", "-", src.name.lower()).strip("-"), "description": f"Converted from Copier template {src.name}", "interview": nodes}
if static:
    # Copier copies files without the suffix verbatim; their names are still rendered.
    doc["static"] = [re.sub(r"([\[\]{}*?])", r"\\\1", s) for s in static]
    report["translated"].append(f"{len(static)} files without `{suffix}` -> `static`")
for k, v in settings.items():
    if k in ("_subdirectory", "_templates_suffix"):
        continue
    if k == "_tasks":
        doc["hooks"] = [{"run": ["sh", "-c", t if isinstance(t, str) else t["command"]]} for t in v]
        report["translated"].append("`_tasks` -> hooks run through `sh -c` (Toha runs argument vectors without a shell)")
    elif k == "_exclude":
        doc["ignore"] = v
        report["translated"].append("`_exclude` -> `ignore`")
    else:
        report["unsupported"].append(f"`{k}`")

(out / "template.yml").write_text(yaml.safe_dump(doc, sort_keys=False, width=1000, allow_unicode=True))
(out / "conversion-report.json").write_text(json.dumps(report, indent=2))
print(json.dumps({k: len(v) for k, v in report.items()}))
