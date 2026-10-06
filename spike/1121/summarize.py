"""Throwaway spike: summarize jinja-probe output per template."""
import collections, json, re, sys
for path in sys.argv[1:]:
    rows = [json.loads(l) for l in open(path)]
    compile_fail = {r["name"] for r in rows if r["stage"] == "compile"}
    render = [r for r in rows if r["stage"] == "render"]
    total = len(render) + len(compile_fail)
    ok = sum(r["kind"] == "ok" for r in render)
    causes = collections.Counter()
    for r in rows:
        if r["kind"] == "ok":
            continue
        d = re.sub(r"\(in .*", "", r["detail"]).strip()
        d = re.sub(r"'[^']*'", "'…'", d) if "undefined" in d.lower() else d
        causes[f'{r["stage"]}/{r["kind"]}: {d[:110]}'] += 1
    print(f"## {path.split('/')[-1]}: {ok}/{total} surfaces render ({100*ok//max(total,1)}%)")
    for c, n in causes.most_common(8):
        print(f"   {n:3d}  {c}")
