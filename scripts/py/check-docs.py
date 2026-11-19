#!/usr/bin/env python3
"""Check current documentation links and regenerate crate file inventories."""
import argparse
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
START = '<!-- crate-files:start -->'
END = '<!-- crate-files:end -->'


def inventory(crate: Path, overview: Path) -> str:
    text = overview.read_text()
    # Keep the curated topic links for files already explained in the handbook.
    topics = dict(re.findall(r'\| \[([^\]]+)\]\(../../../crates/[^)]+\) \| (.+) \|', text))
    rows = ['| 文件 | 阅读入口或源码职责 |', '|---|---|']
    files = sorted(
        p for p in crate.rglob('*')
        if p.is_file() and not any(part.startswith('.') for part in p.relative_to(crate).parts)
    )
    for path in files:
        relative = path.relative_to(crate).as_posix()
        topic = topics.get(relative)
        if topic is None:
            lines = path.read_text().splitlines()
            comments = []
            for line in lines:
                if not line.startswith('//!') or not line[3:].strip():
                    break
                comments.append(line[3:].strip())
            if comments:
                topic = ' '.join(comments)
            else:
                symbols = re.findall(r'\b(?:fn|struct|enum|trait|type|const)\s+(\w+)', path.read_text())
                topic = '、'.join(f'`{s}`' for s in dict.fromkeys(symbols[:3])) or relative
        rows.append(f'| [{relative}](../../../{path.relative_to(ROOT).as_posix()}) | {topic.replace(chr(124), "&#124;")} |')
    return START + '\n\n' + '\n'.join(rows) + '\n\n' + END


def current_documents() -> list[Path]:
    return [
        ROOT / 'README.md', ROOT / 'AGENT.md',
        *sorted((ROOT / 'docs').glob('*.md')),
        *sorted((ROOT / 'docs/design').rglob('*.md')),
        *sorted((ROOT / 'docs/crates').rglob('*.md')),
        *(ROOT / path for path in (
            'containers/README.md', 'permissions/README.md', 'schemas/README.md',
            'configurations/README.md', 'scripts/README.md', 'tests/README.md',
            'tests/fixtures/README.md',
        )),
    ]


def anchors(document: Path) -> set[str]:
    text = document.read_text()
    result = set(re.findall(r'<a\s+id="([^"]+)"', text))
    counts: dict[str, int] = {}
    for heading in re.findall(r'^#{1,6}\s+(.+)$', text, re.MULTILINE):
        slug = re.sub(r'[^\w\- ]', '', heading.strip().lower()).replace(' ', '-')
        count = counts.get(slug, 0)
        result.add(f'{slug}-{count}' if count else slug)
        counts[slug] = count + 1
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='regenerate crate inventory blocks')
    args = parser.parse_args()
    errors = []
    for crate in sorted((ROOT / 'crates').iterdir()):
        if not crate.is_dir():
            continue
        overview = ROOT / 'docs/crates' / f'devdesk-{crate.name}' / 'overview.md'
        text = overview.read_text()
        expected = inventory(crate, overview)
        if START not in text or END not in text:
            errors.append(f'{overview.relative_to(ROOT)}: missing crate-files inventory markers')
            continue
        before, rest = text.split(START, 1)
        _, after = rest.split(END, 1)
        updated = before + expected + after
        if updated != text:
            if args.write:
                overview.write_text(updated)
            else:
                errors.append(f'{overview.relative_to(ROOT)}: file inventory is stale; run with --write')
    for document in current_documents():
        for target in re.findall(r'\]\(([^)\s]+)\)', document.read_text()):
            url = urlsplit(target)
            if url.scheme or url.netloc or url.path.startswith('/'):
                continue
            path = document.parent / unquote(url.path) if url.path else document
            if not path.exists():
                errors.append(f'{document.relative_to(ROOT)}: missing link target {target}')
            elif path.suffix == '.md' and url.fragment and unquote(url.fragment) not in anchors(path):
                errors.append(f'{document.relative_to(ROOT)}: missing heading or anchor {target}')
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'docs: OK — {len(current_documents())} current documents, crate inventories and local links')


if __name__ == '__main__':
    main()
