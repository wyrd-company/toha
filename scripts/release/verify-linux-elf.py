#!/usr/bin/env python3
# ---
# relationships:
#   validates: architecture
#   references: cd
# ---
"""Verify the Linux release's glibc floor and bundled native dependencies."""
import argparse
import re
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("binary")
args = parser.parse_args()
output = subprocess.check_output(
    ["readelf", "--wide", "--version-info", "--dynamic", args.binary], text=True
)
versions = set(re.findall(r"\bGLIBC_[A-Za-z0-9_.]+", output))
if not versions:
    raise SystemExit("No GLIBC version requirements found")
for symbol in sorted(versions):
    version = symbol.removeprefix("GLIBC_")
    if (
        not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", version)
        or tuple(map(int, version.split("."))) > (2, 17)
    ):
        raise SystemExit(f"GLIBC floor exceeds 2.17: {symbol}")
allowed = {
    "libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2",
    "librt.so.1", "libgcc_s.so.1", "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1",
}
needed = set(re.findall(r"\(NEEDED\).*Shared library: \[([^\]]+)\]", output))
if "libc.so.6" not in needed:
    raise SystemExit("Missing libc dynamic dependency")
unexpected = needed - allowed
if unexpected:
    raise SystemExit(f"Unexpected dynamic dependencies: {', '.join(sorted(unexpected))}")
print(f"{args.binary}: GLIBC <= 2.17; NEEDED: {', '.join(sorted(needed))}")
