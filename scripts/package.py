#!/usr/bin/env python3
"""Package an already-built native release, without publishing or signing it."""
import argparse
import hashlib
import json
import platform
import plistlib
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, help='Built executable; defaults to target/release')
parser.add_argument('--output', type=Path, default=Path('target/packages'))
parser.add_argument('--notices', type=Path, default=Path('target/THIRD_PARTY_NOTICES.txt'))
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
os_name = platform.system().lower()
if os_name not in ('linux', 'darwin', 'windows'):
    parser.error('Packaging currently supports Linux, macOS and Windows')
version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
binary = args.binary or root / 'target/release' / ('opalglass.exe' if os_name == 'windows' else 'opalglass')
if not binary.is_file() or not args.notices.is_file():
    parser.error('Build the release and run scripts/dependency_notices.py first')
args.output.mkdir(parents=True, exist_ok=True)
name = f'opalglass-{version}-{os_name}-{platform.machine().lower()}'
with tempfile.TemporaryDirectory(prefix='package-', dir=args.output) as temporary:
    stage = Path(temporary) / name
    stage.mkdir()
    if os_name == 'darwin':
        contents = stage / 'Opalglass.app/Contents'
        destination = contents / 'MacOS'
        destination.mkdir(parents=True)
        resources = contents / 'Resources'
        resources.mkdir()
        (contents / 'Info.plist').write_bytes(plistlib.dumps({
            'CFBundleName': 'Opalglass', 'CFBundleDisplayName': 'Opalglass',
            'CFBundleExecutable': 'opalglass', 'CFBundleIdentifier': 'dev.opalglass.app',
            'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': version,
            'CFBundleVersion': version, 'LSMinimumSystemVersion': '13.0',
            'NSHighResolutionCapable': True,
        }))
        shutil.copy2(binary, destination / 'opalglass')
    elif os_name == 'linux':
        destination = stage / 'bin'
        destination.mkdir()
        shutil.copy2(binary, destination / 'opalglass')
        desktop = stage / 'share/applications'
        desktop.mkdir(parents=True)
        shutil.copy2(root / 'packaging/opalglass.desktop', desktop)
        icons = stage / 'share/icons/hicolor/scalable/apps'
        icons.mkdir(parents=True)
        shutil.copy2(root / 'assets/opalglass.svg', icons)
    else:
        shutil.copy2(binary, stage / 'opalglass.exe')
    for filename in ['README.md', 'LICENSE']:
        shutil.copy2(root / filename, stage / filename)
    shutil.copy2(args.notices, stage / 'THIRD_PARTY_NOTICES.txt')
    (stage / 'BUILD.json').write_text(json.dumps({
        'version': version, 'system': os_name, 'architecture': platform.machine(),
        'rustc': subprocess.check_output(['rustc', '-vV'], text=True, encoding='utf-8'),
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'signed': False,
    }, indent=2), encoding='utf-8')
    archive = Path(shutil.make_archive(str(args.output / name), 'gztar' if os_name == 'linux' else 'zip', temporary, name))
    archive.with_name(archive.name + '.sha256').write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + archive.name + '\n', encoding='utf-8')
    print(archive)
