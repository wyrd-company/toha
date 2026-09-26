#!/usr/bin/env python3
# ---
# relationships:
#   implements: architecture
# ---
"""Render a Homebrew formula from the release checksums."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
parser.add_argument("version")
parser.add_argument("output", type=Path)
args = parser.parse_args()
checksums = {}
for line in (args.directory / "SHA256SUMS").read_text().splitlines():
    digest, name = line.split(maxsplit=1)
    checksums[name.lstrip("*")] = digest
base = f"https://github.com/wyrd-company/toha/releases/download/toha@{args.version}"
lines = ["class Toha < Formula", '  desc "Generate projects and files from templates"',
         '  homepage "https://github.com/wyrd-company/toha"', f'  version "{args.version}"',
         '  license "Apache-2.0"']
for system, entries in (("macos", (("arm", "macos_aarch64"), ("intel", "macos_x86_64"))),
                        ("linux", (("arm", "linux_aarch64"), ("intel", "linux_x86_64")))):
    lines.append(f"  on_{system} do")
    for arch, suffix in entries:
        filename = f"toha_{args.version}_{suffix}.tar.gz"
        if filename not in checksums:
            raise SystemExit(f"missing checksum: {filename}")
        lines += [f"    on_{arch} do", f'      url "{base}/{filename}"',
                  f'      sha256 "{checksums[filename]}"', "    end"]
    lines.append("  end")
lines += ["  def install", '    bin.install "toha"', "  end", "end"]
args.output.write_text("\n".join(lines) + "\n")
