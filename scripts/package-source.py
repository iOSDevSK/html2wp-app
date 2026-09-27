#!/usr/bin/env python3
"""Create a portable source snapshot with all native build inputs and notices."""
import hashlib
import json
import stat
import subprocess
import zipfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = json.loads((ROOT / 'package.json').read_text())['version']
PREFIX = f'html2wp-windows-source-{VERSION}'
DEST = ROOT / 'release' / f'{PREFIX}.zip'
FILES = [
    '.gitignore', '.gitmodules', 'LICENSE', 'README.md',
    'WINDOWS_BUILD.md', 'build-windows.cmd', 'index.html', 'package.json',
    'package-lock.json', 'playwright.config.ts', 'tsconfig.json', 'vite.config.ts',
]
DIRECTORIES = [
    '.github', 'assets', 'docs', 'notices', 'runtime', 'schemas', 'scripts',
    'src', 'src-tauri', 'tests', 'vendor',
]
SKIP_TREES = {'runtime/plugin', 'runtime/skills', 'src-tauri/gen'}
PUBLIC_DOCS = {'automatic-setup.md', 'desktop-licensing.md', 'updates.md'}
PUBLIC_MARKDOWN_PREFIXES = ('notices/', 'vendor/html2wp/', 'vendor/html2wp-to-gutenberg/')
PUBLIC_MARKDOWN = {
    'README.md', 'WINDOWS_BUILD.md',
    *(f'docs/{name}' for name in PUBLIC_DOCS),
    'tests/fixtures/written-out/CONVERSION-REPORT.md',
}
PLUGIN_SKILL = ROOT / 'vendor/html2wp/plugins/html2wp/skills/html2wp/SKILL.md'
if not PLUGIN_SKILL.is_file():
    raise RuntimeError('Initialize the pinned vendor/html2wp submodule before packaging source')

def tracked_files(directory):
    return [name.decode() for name in subprocess.check_output(
        ['git', 'ls-files', '-z'], cwd=directory
    ).split(b'\0') if name]

tracked = set(tracked_files(ROOT))
for module in ('vendor/html2wp', 'vendor/html2wp-to-gutenberg'):
    if not (ROOT / module / '.git').exists():
        raise RuntimeError(f'Initialize the pinned {module} submodule before packaging source')
    tracked.update(f'{module}/{name}' for name in tracked_files(ROOT / module))
if not set(FILES).issubset(tracked):
    raise RuntimeError('A required source file is not tracked by Git')
paths = [ROOT / name for name in sorted(tracked)
         if (name in FILES or name.split('/')[0] in DIRECTORIES)
         and name not in {'vendor/html2wp', 'vendor/html2wp-to-gutenberg'}
         and not any(name == tree or name.startswith(tree + '/') for tree in SKIP_TREES)
         and (not name.startswith('docs/') or not name.lower().endswith('.md') or name in PUBLIC_MARKDOWN)]

DEST.parent.mkdir(exist_ok=True)
manifest = {}
seen = set()
with zipfile.ZipFile(DEST, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for path in sorted(paths):
        if path.is_symlink() or not path.is_file():
            raise RuntimeError(f'Expected a regular source file: {path.relative_to(ROOT)}')
        relative = path.relative_to(ROOT).as_posix()
        if relative.lower().endswith('.md') and relative not in PUBLIC_MARKDOWN and not relative.startswith(PUBLIC_MARKDOWN_PREFIXES):
            raise RuntimeError(f'Unapproved Markdown in source ZIP: {relative}')
        if relative.casefold() in seen:
            raise RuntimeError(f'Case-insensitive path collision: {relative}')
        seen.add(relative.casefold())
        data = path.read_bytes()
        manifest[relative] = hashlib.sha256(data).hexdigest()
        archive.write(path, f'{PREFIX}/{relative}')
    archive.writestr(f'{PREFIX}/SOURCE-MANIFEST.json', json.dumps({
        'appVersion': VERSION,
        'createdAt': datetime.now(timezone.utc).isoformat(),
        'target': 'x86_64-pc-windows-msvc',
        'kind': 'source snapshot; Windows build not executed',
        'plugin': json.loads((ROOT / 'runtime/versions.json').read_text()),
        'files': manifest,
    }, indent=2) + '\n')

with zipfile.ZipFile(DEST) as archive:
    bad = archive.testzip()
    if bad:
        raise RuntimeError(f'ZIP CRC failed: {bad}')
    for relative, expected in manifest.items():
        actual = hashlib.sha256(archive.read(f'{PREFIX}/{relative}')).hexdigest()
        if actual != expected:
            raise RuntimeError(f'ZIP content mismatch: {relative}')
    if any(stat.S_ISLNK(item.external_attr >> 16) for item in archive.infolist()):
        raise RuntimeError('Unexpected symlink in ZIP')

digest = hashlib.sha256(DEST.read_bytes()).hexdigest()
DEST.with_suffix('.zip.sha256').write_text(f'{digest}  {DEST.name}\n')
print(json.dumps({'archive': str(DEST), 'files': len(manifest) + 1,
                  'bytes': DEST.stat().st_size, 'sha256': digest}, indent=2))
