#!/usr/bin/env python3
"""Run the UI harness using cached GPUI, without its unused Git fixture feature.

Only a temporary Cargo manifest changes. Application and GPUI Rust source files
are untouched. The normal CI harness uses the unmodified upstream manifest.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

root = Path(__file__).resolve().parents[1]
host = next(line.split(": ", 1)[1] for line in subprocess.check_output(
    ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--offline", "--locked", "--format-version", "1",
    "--filter-platform", host,
], cwd=root, text=True))
package = next(p for p in metadata["packages"] if p["name"] == "gpui")
source = Path(package["manifest_path"]).parent
repository = source.parents[1]
destination = root / "target" / "offline-gpui-test-runtime"
shutil.copytree(source, destination, dirs_exist_ok=True)
manifest = tomllib.loads((source / "Cargo.toml").read_text())
workspace = tomllib.loads((repository / "Cargo.toml").read_text())["workspace"]

# Materialize workspace inheritance so the copied package stands alone.
for name, value in list(manifest["package"].items()):
    if isinstance(value, dict) and value.get("workspace"):
        manifest["package"][name] = workspace["package"][name]
manifest.pop("lints", None)
for section in [manifest, *manifest.get("target", {}).values()]:
    section.pop("dev-dependencies", None)  # Not building GPUI's own test suite.
    for key in ("dependencies", "build-dependencies"):
        for name, value in list(section.get(key, {}).items()):
            if isinstance(value, dict) and value.get("workspace"):
                base = workspace["dependencies"][name]
                base = {"version": base} if isinstance(base, str) else base.copy()
                if "path" in base:
                    base["path"] = str(repository / base["path"])
                extra = {k: v for k, v in value.items() if k != "workspace"}
                if "features" in extra:
                    extra["features"] = base.get("features", []) + extra["features"]
                base.update(extra)
                section[key][name] = base

# GPUI's simulated platform doesn't call util::test. That feature otherwise
# pulls in git2 solely for repository fixtures, unavailable in this cache.
manifest["features"]["test-support"].remove("util/test-support")


def value_text(value):
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, list):
        return "[" + ", ".join(map(value_text, value)) + "]"
    if isinstance(value, dict):
        return "{" + ", ".join(json.dumps(k) + " = " + value_text(v) for k, v in value.items()) + "}"
    return str(value)


lines = []
for name, value in manifest.items():
    if name == "target":
        for target, sections in value.items():
            for section, items in sections.items():
                lines.append("[target." + json.dumps(target) + "." + section + "]")
                lines.extend(json.dumps(k) + " = " + value_text(v) for k, v in items.items())
    elif name == "example":
        continue
    elif isinstance(value, dict):
        lines.append("[" + name + "]")
        lines.extend(json.dumps(k) + " = " + value_text(v) for k, v in value.items())
    else:
        raise ValueError(f"Unsupported manifest section: {name}")
(destination / "Cargo.toml").write_text("\n".join(lines))
print("Testing unchanged GPUI source with util/test-support omitted from its temporary manifest.", flush=True)
subprocess.run([
    "cargo", "test", "--offline", "--manifest-path", "ui-tests/Cargo.toml",
    "--config", 'patch."https://github.com/zed-industries/zed".gpui.path=' + json.dumps(str(destination)),
    *sys.argv[1:],
], cwd=root, check=True)
