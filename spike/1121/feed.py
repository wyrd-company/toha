"""Throwaway spike: build jinja-probe input from a real template.

Renders the template with its own tool (defaults, no input) to capture the
render context, then emits every render-surface file plus every templated
path segment for the probe.  Usage: feed.py cookiecutter|copier TEMPLATE_DIR OUT_DIR
"""
import fnmatch, json, os, sys
from pathlib import Path

kind, template, out = sys.argv[1], Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve()
extra = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
context_file = os.environ.get("CONTEXT_FILE", "cookiecutter.json")


def text(p: Path):
    try:
        return p.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return None


files = []


def add_tree(root: Path, rel_base: Path, rendered, skip=lambda r: False):
    """Surfaces are named by template-root-relative path, as Toha registers
    file bodies, so `{% include %}` resolves the way Toha would resolve it."""
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d != ".git"]
        for n in dirnames + filenames:
            if "{{" in n or "{%" in n:
                files.append({"name": f"path:{Path(dirpath, n).relative_to(rel_base)}", "source": n, "surface": True})
        for n in filenames:
            p = Path(dirpath, n)
            if skip(str(p.relative_to(root))) or not rendered(p):
                continue
            s = text(p)
            if s is not None:
                files.append({"name": str(p.relative_to(rel_base)), "source": s, "surface": True})


def add_partials(rel_base: Path):
    """Every other text file under the template root is includable, not rendered."""
    seen = {f["name"] for f in files}
    for dirpath, dirnames, filenames in os.walk(rel_base):
        dirnames[:] = [d for d in dirnames if d != ".git"]
        for n in filenames:
            p = Path(dirpath, n)
            rel = str(p.relative_to(rel_base))
            if rel not in seen and p.is_file() and p.stat().st_size < 1_000_000 and (s := text(p)) is not None:
                files.append({"name": rel, "source": s, "surface": False})


if kind == "cookiecutter":
    from cookiecutter.main import cookiecutter
    from cookiecutter.generate import generate_context
    from cookiecutter.prompt import prompt_for_config

    ctx = generate_context(context_file=str(template / context_file), extra_context=extra)
    if "cookiecutter" not in ctx:
        ctx = {"cookiecutter": next(iter(ctx.values()))}
    ctx["cookiecutter"] = prompt_for_config(ctx, no_input=True)
    ctx["cookiecutter"]["_template"] = str(template)
    try:
        if context_file != "cookiecutter.json":
            raise RuntimeError(f"template needs its own CLI; context from {context_file}")
        cookiecutter(str(template), no_input=True, output_dir=str(out), accept_hooks=True, extra_context=extra)
        status = "native-ok"
    except Exception as e:  # native failures are evidence too
        status = f"native-fail: {type(e).__name__}: {str(e)[:160]}"
    project = next(p for p in template.iterdir() if p.is_dir() and "cookiecutter" in p.name and "{{" in p.name)
    cwr = ctx["cookiecutter"].get("_copy_without_render", [])
    add_tree(project, template, lambda p: True, lambda r: any(fnmatch.fnmatch(r, g) for g in cwr))
    hooks = template / "hooks"
    if hooks.is_dir():
        for h in hooks.iterdir():
            if h.suffix in (".py", ".sh") and text(h) is not None:
                files.append({"name": f"hooks/{h.name}", "source": text(h), "surface": True})
    context = ctx
else:
    from copier import run_copy
    from copier._main import Worker  # private API: acceptable for a throwaway spike

    with Worker(src_path=str(template), dst_path=str(out), defaults=True, unsafe=True, quiet=True, data=extra) as w:
        try:
            w.run_copy()
            status = "native-ok"
        except Exception as e:
            status = f"native-fail: {type(e).__name__}: {str(e)[:160]}"
        try:
            context = json.loads(json.dumps(w._render_context(), default=str))
        except Exception as e:
            context = {}
            status += f"; context-fail: {e}"
        sub = template / (w.template.subdirectory or "")
        suffix = w.template.templates_suffix
    add_tree(sub, template, lambda p: (not suffix) or p.name.endswith(suffix))

add_partials(template)
with open(os.environ["OUT_JSON"], "w") as fh:
    json.dump({"context": context, "files": files}, fh)
print(f"{template.name}: {status}; {sum(f['surface'] for f in files)} surfaces", file=sys.stderr)
