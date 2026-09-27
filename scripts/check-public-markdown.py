#!/usr/bin/env python3
"""Fail a public source build if owner-only Markdown was accidentally tracked."""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = {
    'README.md',
    'WINDOWS_BUILD.md',
    'docs/automatic-setup.md',
    'docs/desktop-licensing.md',
    'docs/updates.md',
    'tests/fixtures/written-out/CONVERSION-REPORT.md',
}
PUBLIC_PREFIXES = ('notices/',)

tracked = subprocess.check_output(
    ['git', 'ls-files', '-z', '--', '*.md'], cwd=ROOT
).decode().split('\0')
unexpected = sorted(
    name for name in tracked if name and name not in PUBLIC
    and not name.startswith(PUBLIC_PREFIXES)
)
if unexpected:
    raise SystemExit('Private Markdown tracked in public source:\n' +
                     '\n'.join(f'  {name}' for name in unexpected))
print('Public Markdown allowlist OK')
