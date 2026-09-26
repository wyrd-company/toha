#!/usr/bin/env python3
# ---
# relationships:
#   implements: architecture
# ---
"""Create the immutable repo.wyrd.foo handoff from published release assets."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
parser.add_argument("version")
parser.add_argument("commit")
parser.add_argument("output", type=Path)
args = parser.parse_args()
tag = f"toha@{args.version}"
formats = [("archive", "tar.gz"), ("package", "deb"), ("package", "rpm")]
artifacts = []
for kind, fmt in formats:
    for arch, suffix in (("amd64", "x86_64"), ("arm64", "aarch64")):
        name = f"toha_{args.version}_linux_{suffix}.{fmt}"
        path = args.directory / name
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        artifacts.append({"kind": kind, "format": fmt, "os": "linux", "arch": arch,
                          "filename": name,
                          "url": f"https://github.com/wyrd-company/toha/releases/download/{tag}/{name}",
                          "sha256": digest})
manifest = {
    "schema_version": 1, "product": "toha", "version": args.version, "tag": tag,
    "source": {"repository": "wyrd-company/toha", "commit": args.commit},
    "package": {"name": "toha", "binary": "toha",
                "description": "Generate projects and files from templates",
                "homepage": "https://github.com/wyrd-company/toha", "license": "Apache-2.0",
                "maintainer": "Wyrd Company <support@wyrd.company>"},
    "publish": {"apt": {"suite": "stable", "component": "main"},
                "rpm": {"channel": "stable"}, "aur": {"package": "toha-bin"}},
    "artifacts": artifacts,
}
args.output.write_text(json.dumps(manifest, indent=2) + "\n")
