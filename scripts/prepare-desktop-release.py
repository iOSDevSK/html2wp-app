"""Validate the small signed macOS bundle and stage public release assets.

Run after `TAURI_SIGNING_PRIVATE_KEY=... npm run bundle:mac`. The output is
only binaries, signatures, checksums and metadata; never a source archive.
"""
import hashlib
import base64
import json
import os
import plistlib
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = json.loads((ROOT / 'package.json').read_text())['version']
REPO = 'iOSDevSK/html2wp-desktop-releases'
BASE = ROOT / 'src-tauri/target/aarch64-apple-darwin/release/bundle'
APP = BASE / 'macos/html2wp.app'
DMG = BASE / f'dmg/html2wp_{VERSION}_aarch64.dmg'
ARCHIVE = BASE / 'macos/html2wp.app.tar.gz'
SIGNATURE = BASE / 'macos/html2wp.app.tar.gz.sig'
OUT = ROOT / 'release-assets' / VERSION


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def app_files(path):
    result = {}
    for root, dirs, files in os.walk(path, followlinks=False):
        for name in dirs + files:
            item = Path(root) / name
            relative = item.relative_to(path).as_posix()
            if item.is_symlink():
                result[relative] = ('link', os.readlink(item))
            elif item.is_file():
                result[relative] = ('file', sha256(item))
            elif item.is_dir():
                result[relative] = ('dir', '')
    return result


def verify_app(path, expected):
    info = plistlib.loads((path / 'Contents/Info.plist').read_bytes())
    if info.get('CFBundleShortVersionString') != VERSION:
        raise RuntimeError(f'Artifact app version does not match {VERSION}')
    if app_files(path) != expected:
        raise RuntimeError('DMG or updater archive does not contain the current app build')


def verify_signature():
    config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
    with tempfile.TemporaryDirectory() as directory:
        public = Path(directory) / 'updater.pub'
        signature = Path(directory) / 'updater.sig'
        public.write_bytes(base64.b64decode(config['plugins']['updater']['pubkey'], validate=True))
        signature.write_bytes(base64.b64decode(SIGNATURE.read_text().strip(), validate=True))
        verified = subprocess.run(['minisign', '-Vm', str(ARCHIVE), '-x', str(signature), '-p', str(public)],
                                  check=True, capture_output=True, text=True)
        if f'version:{VERSION}' not in verified.stdout:
            raise RuntimeError('Updater signature does not bind this app version')


def main():
    release = json.loads((ROOT / 'runtime/runtime-release.json').read_text())
    if release.get('published') is not True or '@sha256:' not in release.get('image', ''):
        raise RuntimeError('Publish and pin the public Docker Hub runtime first')
    if not all(path.is_file() for path in (DMG, ARCHIVE, SIGNATURE)) or not APP.is_dir():
        raise RuntimeError('Build app, DMG and signed updater archive for this version first')
    if DMG.stat().st_size >= 200_000_000 or ARCHIVE.stat().st_size >= 200_000_000:
        raise RuntimeError('Desktop bundle is still too large; check for an embedded Docker archive')
    resources = APP / 'Contents/Resources/runtime'
    if not resources.is_dir() or any(path.name.startswith('prebuilt-runtime') for path in resources.iterdir()):
        raise RuntimeError('The app contains a bundled Docker runtime archive')
    if json.loads((resources / 'runtime-release.json').read_text()) != release:
        raise RuntimeError('The app does not contain this release’s Docker Hub digest')
    expected = app_files(APP)
    verify_app(APP, expected)
    verify_signature()
    with tempfile.TemporaryDirectory() as directory:
        with tarfile.open(ARCHIVE, 'r:gz') as archive:
            members = archive.getmembers()
            if any('prebuilt-runtime' in member.name for member in members):
                raise RuntimeError('Updater archive contains the old Docker runtime')
            if any(member.name.startswith('/') or '..' in Path(member.name).parts for member in members):
                raise RuntimeError('Updater archive has an unsafe path')
            archive.extractall(directory, filter='data')
        verify_app(Path(directory) / 'html2wp.app', expected)
    with tempfile.TemporaryDirectory() as directory:
        mount = Path(directory) / 'mounted'
        mount.mkdir()
        subprocess.run(['hdiutil', 'verify', str(DMG)], check=True, capture_output=True)
        subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(DMG)], check=True, capture_output=True)
        try:
            verify_app(mount / 'html2wp.app', expected)
        finally:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True, capture_output=True)
    OUT.mkdir(parents=True, exist_ok=True)
    names = {DMG: DMG.name, ARCHIVE: ARCHIVE.name, SIGNATURE: SIGNATURE.name,
             ROOT / 'notices/desktop.cdx.json': 'desktop.cdx.json'}
    for source, name in names.items():
        shutil.copy2(source, OUT / name)
    manifest = {'version': VERSION,
                'notes': 'Preview controls now verify project ownership before changing Docker resources.',
                'platforms': {'darwin-aarch64': {
                    'url': f'https://github.com/{REPO}/releases/download/v{VERSION}/{ARCHIVE.name}',
                    'signature': SIGNATURE.read_text().strip()}}}
    (OUT / 'latest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (OUT / 'SHA256SUMS').write_text(''.join(f'{sha256(OUT / name)}  {name}\n' for name in names.values()))
    print(f'Staged {OUT}: DMG {DMG.stat().st_size / 1_000_000:.1f} MB, updater {ARCHIVE.stat().st_size / 1_000_000:.1f} MB')


if __name__ == '__main__':
    main()
