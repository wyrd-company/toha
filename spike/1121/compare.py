"""Throwaway spike: compare probe output with the native tool's output tree.

Usage: compare.py cookiecutter|copier PROBE_JSONL NATIVE_OUT_DIR [SUBDIR] [SUFFIX]
Rendered body paths are template-root-relative.  For Cookiecutter the native
output dir holds the rendered project dir; for Copier, SUBDIR is stripped and
SUFFIX removed.
"""
import collections, json, sys
from pathlib import Path

kind, probe, native = sys.argv[1], sys.argv[2], Path(sys.argv[3])
subdir = sys.argv[4] if len(sys.argv) > 4 else ""
suffix = sys.argv[5] if len(sys.argv) > 5 else ""
tally = collections.Counter()
diffs = []
for line in open(probe):
    r = json.loads(line)
    if r["kind"] != "ok" or r["name"].startswith(("path:", "hooks/")) or r.get("path") is None:
        continue
    rel = r["path"]
    if subdir:
        rel = rel[len(subdir) + 1:] if rel.startswith(subdir + "/") else rel
    if suffix and rel.endswith(suffix):
        rel = rel[: -len(suffix)]
    if any(seg == "" for seg in rel.split("/")):
        tally["empty-name (not generated)"] += 1
        continue
    target = native / rel
    if not target.is_file():
        tally["absent natively (hook-deleted or conditional)"] += 1
        continue
    if target.read_bytes() == r["output"].encode():
        tally["byte-identical"] += 1
    else:
        tally["differs"] += 1
        diffs.append(rel)
print(dict(tally))
for d in diffs[:12]:
    print("   differs:", d)
