#!/usr/bin/env python3
"""Warm Docker's build cache from the exact packaged resource used by browser tests."""
import json
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2] / 'resources/configuration'
manifest = json.loads((root / 'manifest.json').read_text())
defaults = json.loads((root / manifest['defaults']).read_text())
reference = defaults['runtime_resources']['codex']
resources = [json.loads((root / name).read_text()) for name in manifest['types']['runtime_resource']]
source = next(source for source in resources if f"{source['identity']}:{source['revision']}" == reference)
resource = source['data']
# The application still registers and verifies the actual selected image itself.
# This step exposes missing Docker/network prerequisites before a UI timeout.
if resource.get('image'):
    subprocess.run(['docker', 'pull', resource['image']], check=True)
else:
    with tempfile.TemporaryDirectory(prefix='devdesk-browser-runtime-') as directory:
        context = Path(directory)
        for item in resource['files']:
            target = context / item['path']
            target.resolve().relative_to(context.resolve())
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(item['content'])
        subprocess.run(['docker', 'build', '--tag', 'devdesk/browser-acceptance:codex', str(context)], check=True)
print(f'PASS browser Docker build cache: {reference}')
