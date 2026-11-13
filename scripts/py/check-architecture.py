#!/usr/bin/env python3
"""Check production layer edges and the boundaries most prone to regressions."""
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=ROOT))
ranks = {'devdesk-domain': 0, 'devdesk-core': 1, 'devdesk-adapters': 2,
         'devdesk-host': 3, 'devdesk-credential-helper': 3}
errors = []
for package in metadata['packages']:
    for dependency in package['dependencies']:
        if dependency['name'] in ranks and ranks[dependency['name']] > ranks[package['name']]:
            errors.append(f"{package['name']} -> {dependency['name']} ({dependency['kind'] or 'normal'})")
for path in (ROOT / 'crates/domain/src').glob('*.rs'):
    source = path.read_text().split('#[cfg(test)]')[0]
    if re.search(r'std::(?:fs|process|env|time)::|uuid::|devdesk_(?:core|adapters|host)::', source):
        errors.append(f'{path.relative_to(ROOT)} accesses the environment or an outer layer')
core_build = ROOT / 'crates/core/build.rs'
if core_build.exists():
    errors.append('core has a product build script')
for path in (ROOT / 'crates/adapters/src').glob('*.rs'):
    source = path.read_text().split('#[cfg(test)]')[0]
    if re.search(r'\b(?:compile_workflow_sources|render_source_task)\s*\(', source):
        errors.append(f'{path.relative_to(ROOT)} compiles/renders application state')
for path in (ROOT / 'crates/host/src').glob('*.rs'):
    if path.name.endswith('_tests.rs'):
        continue
    source = path.read_text().split('#[cfg(test)]\nmod tests')[0]
    if re.search(r'\.(?:governance|snapshots|events|documents)\.push\(', source):
        errors.append(f'{path.relative_to(ROOT)} directly mutates business state')
if errors:
    raise SystemExit('\n'.join(errors))
print('architecture: OK — layer dependencies, pure domain, resource boundary, application authority')
