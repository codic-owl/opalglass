"""Exercise release scripts with Unicode data and Windows' legacy pipe encoding."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile


ROOT = Path(__file__).resolve().parents[2]
CHECK_OUTPUT = subprocess.check_output
RUSTC = "rustc 1.90.0\nhost: x86_64-pc-windows-msvc\nbuild: Álvaro\n"
LICENSE = "Copyright © 2026 Álvaro — 工具\nPermission is hereby granted.\n"


def tool_output(metadata):
    def capture(command, **kwargs):
        if command[0] == "rustc":
            data = RUSTC.encode("utf-8")
        elif command[:2] == ["cargo", "metadata"]:
            data = json.dumps(metadata, ensure_ascii=False).encode("utf-8")
        else:
            raise AssertionError(f"Unexpected tool: {command}")
        # Exercise real subprocess pipe decoding. On legacy Windows Python,
        # omitting the encoding selects CP1252, where UTF-8's 0x81 is undefined.
        kwargs.setdefault("encoding", "cp1252")
        return CHECK_OUTPUT(
            [sys.executable, "-c", f"import sys; sys.stdout.buffer.write({data!r})"],
            **kwargs,
        )
    return capture


def run_script(name, arguments, metadata=None):
    with (
        mock.patch.object(sys, "argv", [name, *arguments]),
        mock.patch("subprocess.check_output", side_effect=tool_output(metadata)),
        contextlib.redirect_stdout(io.StringIO()),
    ):
        runpy.run_path(str(ROOT / "scripts" / name), run_name="__main__")


class ReleaseScriptsTests(unittest.TestCase):
    def test_notices_decode_unicode_metadata_and_preserve_license_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary) / "dependency-Á"
            folder.mkdir()
            (folder / "LICENSE").write_text(LICENSE, encoding="utf-8")
            metadata = {"packages": [{
                "name": "unicode-fixture",
                "version": "1.0.0",
                "manifest_path": str(folder / "Cargo.toml"),
                "license": "MIT",
                "authors": ["Álvaro"],
            }]}
            for target in ([], ["--target", "x86_64-pc-windows-msvc"]):
                with self.subTest(target=target):
                    output = folder / "THIRD_PARTY_NOTICES.txt"
                    run_script(
                        "dependency_notices.py",
                        ["--offline", "--output", str(output), *target],
                        metadata,
                    )
                    text = output.read_text(encoding="utf-8")
                    self.assertIn("Target: x86_64-pc-windows-msvc", text)
                    self.assertIn("unicode-fixture 1.0.0", text)
                    self.assertIn(LICENSE, text)
                    self.assertNotIn("\ufffd", text)

    def test_windows_package_keeps_unicode_notices_and_valid_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary) / "build-Á"
            folder.mkdir()
            binary = folder / "opalglass.exe"
            binary.write_bytes(b"MZ packaging fixture")
            notices = folder / "THIRD_PARTY_NOTICES.txt"
            notices.write_bytes(LICENSE.encode("utf-8"))
            output = folder / "packages"
            with (
                mock.patch("platform.system", return_value="Windows"),
                mock.patch("platform.machine", return_value="AMD64"),
            ):
                run_script("package.py", [
                    "--binary", str(binary), "--notices", str(notices),
                    "--output", str(output),
                ])
            archives = list(output.glob("*.zip"))
            self.assertEqual(len(archives), 1)
            archive = archives[0]
            prefix = archive.stem
            with zipfile.ZipFile(archive) as contents:
                self.assertEqual(
                    contents.read(f"{prefix}/THIRD_PARTY_NOTICES.txt").decode("utf-8"),
                    LICENSE,
                )
                self.assertEqual(contents.read(f"{prefix}/opalglass.exe"), binary.read_bytes())
                build = json.loads(contents.read(f"{prefix}/BUILD.json"))
                self.assertEqual(build["rustc"], RUSTC)
                self.assertEqual(build["system"], "windows")
                self.assertEqual(build["binary_sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
            checksum = archive.with_name(archive.name + ".sha256").read_text(encoding="utf-8")
            self.assertEqual(checksum.split()[0], hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
