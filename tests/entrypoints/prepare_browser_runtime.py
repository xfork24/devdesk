#!/usr/bin/env python3
"""Warm the selected Sandbox base; the Host composes and verifies the real recipe."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2] / 'kernel/core'
manifest = json.loads((root / 'manifest.json').read_text())
defaults = json.loads((root / manifest['defaults']).read_text())
reference = defaults['sandbox_ref']
sources = [json.loads((root / name).read_text()) for name in manifest['types']['sandbox']]
source = next(source for source in sources if f"{source['identity']}:{source['revision']}" == reference)
# The application still registers and verifies the actual selected image itself.
# This step exposes missing Docker/network prerequisites before a UI timeout.
subprocess.run(['docker', 'pull', source['data']['build']['base_image']], check=True)
print(f'PASS browser Docker base cache: {reference}')
