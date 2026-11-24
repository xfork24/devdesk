#!/usr/bin/env python3
"""Check current documentation links and regenerate crate file inventories."""
import argparse
import re
import subprocess
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
START = '<!-- crate-files:start -->'
END = '<!-- crate-files:end -->'


def source_files(crate: Path) -> list[Path]:
    """Include tracked and new source assets, but not ignored local build output."""
    if (ROOT / '.git').exists():
        result = subprocess.run(
            ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z',
             '--', crate.relative_to(ROOT).as_posix()],
            cwd=ROOT, check=True, stdout=subprocess.PIPE,
        )
        return sorted({ROOT / p.decode() for p in result.stdout.split(b'\0')
                       if p and (ROOT / p.decode()).is_file()})
    # Source archives have no Git metadata. Do not decode binary assets below.
    return sorted(p for p in crate.rglob('*') if p.is_file()
                  and not any(part.startswith('.') or part in {'target', 'node_modules', '__pycache__'}
                              for part in p.relative_to(crate).parts))


def inventory(crate: Path, index: Path) -> str:
    text = index.read_text()
    # Keep the curated topic links for files already explained in the handbook.
    topics = dict(re.findall(r'\| \[([^\]]+)\]\(../../../crates/[^)]+\) \| (.+) \|', text))
    rows = ['| 文件 | 阅读入口或源码职责 |', '|---|---|']
    for path in source_files(crate):
        relative = path.relative_to(crate).as_posix()
        topic = topics.get(relative)
        if topic is None:
            try:
                content = path.read_text()
            except UnicodeDecodeError:
                content = ''
            lines = content.splitlines()
            comments = []
            for line in lines:
                if not line.startswith('//!') or not line[3:].strip():
                    break
                comments.append(line[3:].strip())
            if comments:
                topic = ' '.join(comments)
            else:
                symbols = re.findall(r'\b(?:fn|struct|enum|trait|type|const)\s+(\w+)', content)
                topic = '、'.join(f'`{s}`' for s in dict.fromkeys(symbols[:3])) or relative
        rows.append(f'| [{relative}](../../../{path.relative_to(ROOT).as_posix()}) | {topic.replace(chr(124), "&#124;")} |')
    return START + '\n\n' + '\n'.join(rows) + '\n\n' + END


def current_documents() -> list[Path]:
    return [
        ROOT / 'README.md', ROOT / 'AGENT.md',
        *sorted((ROOT / 'docs').glob('*.md')),
        *sorted((ROOT / 'docs/design').rglob('*.md')),
        *sorted((ROOT / 'docs/crates').rglob('*.md')),
        *sorted((ROOT / 'docs/benchmarks').glob('*.md')),
        *(ROOT / path for path in (
            'containers/README.md', 'permissions/README.md', 'schemas/README.md',
            'configurations/README.md', 'configurations/software-development/README.md',
            'scripts/README.md', 'tests/README.md',
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


def check_inventory(crate: Path, write: bool = False) -> list[str]:
    index = ROOT / 'docs/crates' / f'devdesk-{crate.name}' / 'files.md'
    if not index.is_file():
        return [f'{index.relative_to(ROOT)}: missing crate file index']
    text = index.read_text()
    if text.count(START) != 1 or text.count(END) != 1 or text.index(START) > text.index(END):
        return [f'{index.relative_to(ROOT)}: invalid crate-files inventory markers']
    expected = inventory(crate, index)
    before, rest = text.split(START, 1)
    _, after = rest.split(END, 1)
    updated = before + expected + after
    if updated != text:
        if write:
            index.write_text(updated)
        else:
            return [f'{index.relative_to(ROOT)}: file inventory is stale; run with --write']
    return []


def check_links(documents: list[Path]) -> list[str]:
    errors = []
    for document in documents:
        if not document.is_file():
            errors.append(f'{document.relative_to(ROOT)}: missing document')
            continue
        for target in re.findall(r'\]\(([^)\s]+)\)', document.read_text()):
            url = urlsplit(target)
            if url.scheme or url.netloc or url.path.startswith('/'):
                continue
            path = document.parent / unquote(url.path) if url.path else document
            if not path.exists():
                errors.append(f'{document.relative_to(ROOT)}: missing link target {target}')
            elif path.suffix == '.md' and url.fragment and unquote(url.fragment) not in anchors(path):
                errors.append(f'{document.relative_to(ROOT)}: missing heading or anchor {target}')
    return errors


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='regenerate crate inventory blocks')
    args = parser.parse_args()
    errors = []
    for crate in sorted((ROOT / 'crates').iterdir()):
        if (crate / 'Cargo.toml').is_file():
            errors.extend(check_inventory(crate, args.write))
    errors.extend(check_links(current_documents()))
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'docs: OK — {len(current_documents())} current documents, crate inventories and local links')


if __name__ == '__main__':
    main()
