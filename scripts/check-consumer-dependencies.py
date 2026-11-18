# ---
# relationships:
#   verifies: architecture
# ---
"""Guard the dependency graph compiled by the library consumer without CLI."""
from pathlib import Path
import sys
import tomllib

root = Path(__file__).resolve().parents[1]
packages = tomllib.loads((root / "tests/consumer/Cargo.lock").read_text())["package"]
gix = [package["version"] for package in packages if package["name"] == "gix"]
tagver = [package["version"] for package in packages if package["name"] == "tagver"]
if gix != ["0.90.0"] or tagver:
    sys.exit(f"consumer dependency isolation failed: gix={gix}, tagver={tagver}")
print("consumer dependency isolation passed: one gix 0.90.0, no tagver")
