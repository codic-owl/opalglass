#!/usr/bin/env python3
"""Collect the locked dependency inventory and license files for a target."""
import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--offline", action="store_true")
parser.add_argument("--target", help="Rust target triple; defaults to the compiler host")
parser.add_argument("--output", default="target/THIRD_PARTY_NOTICES.txt")
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
target = args.target or next(
    line.split(": ", 1)[1]
    for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
    if line.startswith("host: ")
)
command = ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target]
if args.offline:
    command.append("--offline")
metadata = json.loads(subprocess.check_output(command, cwd=root, text=True))
sections = [f"Opalglass dependency notices\nTarget: {target}\n"]
missing = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    directory = Path(package["manifest_path"]).parent
    files = set()
    if package.get("license_file"):
        candidate = directory / package["license_file"]
        if candidate.is_file():
            files.add(candidate)
    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "license*", "licence*"):
        files.update(p for p in directory.glob(pattern) if p.is_file())
    sections.append(
        f"\n{'=' * 72}\n{package['name']} {package['version']}\n"
        f"License expression: {package.get('license') or 'See license files'}\n"
        f"Source: {package.get('source') or 'local application'}\n"
    )
    if not files:
        missing.append(f"{package['name']} {package['version']}")
    for file in sorted(files):
        sections.append(f"\n--- {file.name} ---\n{file.read_text(errors='replace')}\n")
sections.append("\nPackages without a discovered license file:\n" + "\n".join(missing))
output = root / args.output
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text("\n".join(sections), encoding="utf-8")
print(f"Wrote notices for {len(metadata['packages'])} packages to {output}")
if missing:
    print("Review missing license files: " + ", ".join(missing))
