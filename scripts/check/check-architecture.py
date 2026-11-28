#!/usr/bin/env python3
"""Check production layer edges and the boundaries most prone to regressions."""
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
RANKS = {'devdesk-domain': 0, 'devdesk-core': 1, 'devdesk-adapters': 2,
         'devdesk-host': 3}
KERNEL_SOURCE_SUFFIXES = {'.rs', '.toml', '.mjs', '.json'}
# Kernel code that still names a domain package or a retired factory identity. Each entry
# is the reason it exists and the rules it is allowed to trip. The list must match exactly:
# a new reference fails, and so does an entry whose code is gone. It is empty today.
KNOWN_DOMAIN_REFERENCES = {}
# The build script embedding the factory bundle is the only kernel code that names its directory.
FACTORY_BUNDLE_LOADER = 'crates/adapters/build.rs'
# Source types the factory bundle no longer publishes. A literal naming one of their old
# factory identities is a leftover of the monolith catalog, not of a kernel definition.
BUSINESS_TYPES = {
    'role': 'builtin-role-', 'skill': 'builtin-skill-', 'spec': 'builtin-spec-',
    'standard': 'builtin-standard-', 'workflow': 'builtin-workflow-',
    'delivery_type': 'builtin-delivery-', 'capability': 'builtin-capability-',
    'agent': 'builtin-agent-',
}
OUT_OF_LINE_MODULE = re.compile(r'\bmod\s+(\w+)\s*;')
ATTRIBUTE_ABOVE = re.compile(r'\s*#\[([^\]]*)\]\s*$')
PATH_ATTRIBUTE = re.compile(r'path\s*=\s*"([^"]+)"')


def layer_errors(root: Path) -> list[str]:
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=root))
    errors = []
    for package in metadata['packages']:
        for dependency in package['dependencies']:
            if dependency['name'] in RANKS and RANKS[dependency['name']] > RANKS[package['name']]:
                errors.append(f"{package['name']} -> {dependency['name']} ({dependency['kind'] or 'normal'})")
    for path in (root / 'crates/domain/src').glob('*.rs'):
        source = path.read_text().split('#[cfg(test)]')[0]
        if re.search(r'std::(?:fs|process|env|time)::|uuid::|devdesk_(?:core|adapters|host)::', source):
            errors.append(f'{path.relative_to(root)} accesses the environment or an outer layer')
    core_build = root / 'crates/core/build.rs'
    if core_build.exists():
        errors.append('core has a product build script')
    for path in (root / 'crates/adapters/src').glob('*.rs'):
        source = path.read_text().split('#[cfg(test)]')[0]
        if re.search(r'\bPath(?:Buf)?::(?:new|from)\(\s*env!\(\s*"CARGO_MANIFEST_DIR"', source):
            errors.append(f'{path.relative_to(root)} looks up runtime assets in the build checkout')
        if re.search(r'\b(?:compile_workflow_sources|render_source_task)\s*\(', source):
            errors.append(f'{path.relative_to(root)} compiles/renders application state')
    for path in (root / 'crates/host/src').glob('*.rs'):
        if path.name.endswith('_tests.rs'):
            continue
        source = path.read_text().split('#[cfg(test)]\nmod tests')[0]
        if re.search(r'\.(?:governance|snapshots|events|documents)\.push\(', source):
            errors.append(f'{path.relative_to(root)} directly mutates business state')
    return errors


def kernel_sources(crates: Path) -> list[Path]:
    """Production sources of the kernel crates. Integration tests are not production."""
    sources = []
    for path in crates.rglob('*'):
        if not path.is_file() or path.suffix not in KERNEL_SOURCE_SUFFIXES:
            continue
        if path.relative_to(crates).parts[1:2] == ('tests',):
            continue
        sources.append(path.resolve())
    return sorted(sources)


def module_declarations(text: str):
    """Yield (path attribute, module name, declared under cfg(test)) for out-of-line modules."""
    for match in OUT_OF_LINE_MODULE.finditer(text):
        attributes = []
        head = text[:match.start()]
        while found := ATTRIBUTE_ABOVE.search(head):
            attributes.append(found.group(1))
            head = head[:found.start()]
        path_attribute = next(
            (found.group(1) for attribute in attributes
             if (found := PATH_ATTRIBUTE.fullmatch(attribute))), None)
        is_test = any(attribute.replace(' ', '') == 'cfg(test)' for attribute in attributes)
        yield path_attribute, match.group(1), is_test


def module_file(parent: Path, path_attribute, name: str):
    if path_attribute:
        return (parent.parent / path_attribute).resolve()
    base = parent.parent if parent.name in {'mod.rs', 'main.rs', 'lib.rs'} else parent.parent / parent.stem
    for candidate in (base / f'{name}.rs', base / name / 'mod.rs'):
        if candidate.exists():
            return candidate.resolve()
    return None


def test_only_modules(sources) -> set[Path]:
    """Files the kernel compiles only under cfg(test), reached through out-of-line modules."""
    known = set(sources)
    test_only = set()
    while True:
        found = set()
        for path in sources:
            if path.suffix != '.rs':
                continue
            for path_attribute, name, is_test in module_declarations(path.read_text()):
                if not (is_test or path in test_only):
                    continue
                target = module_file(path, path_attribute, name)
                if target in known:
                    found.add(target)
        if found <= test_only:
            return test_only
        test_only |= found


def kernel_domain_errors(root: Path) -> list[str]:
    crates, plugins = root / 'crates', root / 'plugins'
    sources = kernel_sources(crates)
    test_only = test_only_modules(sources)
    domains = sorted(p.name for p in plugins.iterdir() if p.is_dir())
    factory = json.loads((root / 'kernel/core/manifest.json').read_text())
    retired = [prefix for kind, prefix in BUSINESS_TYPES.items() if not factory['types'].get(kind)]
    alternation = '|'.join(map(re.escape, domains))
    rules = [
        ('references a domain package path',
         re.compile(rf'plugins/(?:{alternation})\b') if domains else None, False),
        ('names a domain package namespace',
         re.compile(rf'"(?:{alternation})(?:[."])') if domains else None, False),
        ('names the factory directory', re.compile(r'\.\./kernel"|kernel/core[/"]'), True),
        ('names a retired factory identity', re.compile('|'.join(map(re.escape, retired))), False),
    ]
    loader = (root / FACTORY_BUNDLE_LOADER).resolve()
    errors, covered = [], set()
    for path in sources:
        if path in test_only:
            continue
        location = path.relative_to(root).as_posix()
        production = path.read_text().split('#[cfg(test)]')[0]
        allowed = KNOWN_DOMAIN_REFERENCES.get(location, (None, ()))[1]
        for label, pattern, loader_allowed in rules:
            if pattern is None or (path == loader and loader_allowed):
                continue
            hits = list(pattern.finditer(production))
            if not hits:
                continue
            if label in allowed:
                covered.add((location, label))
                continue
            errors.extend(
                f'{location}:{production.count(chr(10), 0, hit.start()) + 1} {label}'
                for hit in hits)
    expected = {(path, label) for path, (_, labels) in KNOWN_DOMAIN_REFERENCES.items() for label in labels}
    errors.extend(f'known domain reference is stale: {path} ({label})'
                  for path, label in sorted(expected - covered))
    return errors


def main() -> None:
    errors = layer_errors(ROOT) + kernel_domain_errors(ROOT)
    if errors:
        raise SystemExit('\n'.join(errors))
    print('architecture: OK — layer dependencies, pure domain, resource boundary, '
          'application authority, kernel/domain boundary')


if __name__ == '__main__':
    main()
