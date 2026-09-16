#!/usr/bin/env python3
"""Check local Markdown link targets in public documentation, without network I/O."""
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

root = Path(__file__).resolve().parents[1]
files = [root / name for name in ('README.md', 'CHANGELOG.md', 'SECURITY.md')]
files += list((root / 'npm').glob('*.md')) + list((root / 'docs').rglob('*.md'))
missing = []
for source in files:
    for target in re.findall(r'\]\(([^)\s]+)(?:\s+[^)]*)?\)', source.read_text()):
        parsed = urlsplit(target.strip('<>'))
        if parsed.scheme or parsed.netloc or not parsed.path:
            continue
        if not (source.parent / unquote(parsed.path)).exists():
            missing.append(f'{source.relative_to(root)}: {target}')
if missing:
    raise SystemExit('Missing documentation targets:\n' + '\n'.join(missing))
print(f'Local links checked in {len(files)} documentation files.')
