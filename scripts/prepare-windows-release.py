"""Verify a signed Windows NSIS build and stage its separate updater feed.

Run against the artifact from the native Windows CI job. Upload the EXE and
signature before publishing windows-x86_64.json on the binary repo's updates
branch. The stable macOS latest.json is never changed by a Windows preview.
"""
import argparse
import base64
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = json.loads((ROOT / 'package.json').read_text())['version']
REPO = 'iOSDevSK/html2wp-desktop-releases'
FEED = f'https://raw.githubusercontent.com/{REPO}/updates/windows-x86_64.json'


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--artifact-dir', required=True, type=Path,
                        help='Extracted Windows CI bundle/nsis directory')
    args = parser.parse_args()
    release = json.loads((ROOT / 'runtime/runtime-release.json').read_text())
    runtime = release['platforms']['x86_64']
    if not runtime.get('published') or not re.fullmatch(
            r'docker.io/[a-z0-9_./-]+@sha256:[0-9a-f]{64}', runtime.get('image', '')):
        raise RuntimeError('Publish and pin the Linux x86_64 Docker runtime first')
    installers = list(args.artifact_dir.glob(f'html2wp_{VERSION}_x64-setup.exe'))
    if len(installers) != 1:
        raise RuntimeError(f'Expected one NSIS installer for version {VERSION}')
    installer = installers[0]
    signature = installer.with_name(installer.name + '.sig')
    if not signature.is_file() or installer.stat().st_size >= 200_000_000:
        raise RuntimeError('Signed compact Windows installer was not produced')
    if installer.open('rb').read(2) != b'MZ':
        raise RuntimeError('Windows installer has no PE header')
    config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
    if config['version'] != VERSION or config['plugins']['updater']['endpoints'] != [FEED]:
        raise RuntimeError('Windows updater must use this version and the isolated Windows feed')
    with tempfile.TemporaryDirectory() as directory:
        public = Path(directory) / 'updater.pub'
        sig = Path(directory) / 'updater.sig'
        public.write_bytes(base64.b64decode(config['plugins']['updater']['pubkey'], validate=True))
        sig.write_bytes(base64.b64decode(signature.read_text().strip(), validate=True))
        verified = subprocess.run(['minisign', '-Vm', str(installer), '-x', str(sig), '-p', str(public)],
                                  check=True, capture_output=True, text=True)
        if f'version:{VERSION}' not in verified.stdout:
            raise RuntimeError('Updater signature does not bind this app version')
    out = ROOT / 'release-assets' / f'{VERSION}-windows-preview'
    out.mkdir(parents=True, exist_ok=True)
    for item in (installer, signature):
        shutil.copy2(item, out / item.name)
    feed = {
        'version': VERSION,
        'notes': 'Signed by BELNEM s.r.o. (Authenticode, timestamped): Windows names the publisher instead of "Unknown publisher".',
        'platforms': {'windows-x86_64': {
            'url': f'https://github.com/{REPO}/releases/download/v{VERSION}/{installer.name}',
            'signature': signature.read_text().strip(),
        }},
    }
    (out / 'windows-x86_64.json').write_text(json.dumps(feed, indent=2) + '\n')
    (out / 'SHA256SUMS.windows').write_text(''.join(
        f'{sha256(out / name)}  {name}\n' for name in (installer.name, signature.name, 'windows-x86_64.json')))
    print(f'Staged verified Windows installer and isolated updater feed in {out}')


if __name__ == '__main__':
    main()
