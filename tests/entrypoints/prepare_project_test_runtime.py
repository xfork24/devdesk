#!/usr/bin/env python3
"""Prepare the fixed Node/Python project-test image without touching Harness tags."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
root = Path(__file__).resolve().parents[2]
recipe = root / 'tests/fixtures/project-runtime/Dockerfile'
reference = os.environ.get('DEVDESK_TEST_PROJECT_IMAGE', 'devdesk/project-tests:1')
identity = hashlib.sha256(recipe.read_bytes()).hexdigest()
existing = subprocess.run(['docker', 'image', 'inspect', reference], capture_output=True, text=True)
if existing.returncode == 0:
    rows = json.loads(existing.stdout)
    if os.environ.get('DEVDESK_TEST_PROJECT_IMAGE') or rows[0]['Config'].get('Labels', {}).get('devdesk.test-recipe-sha256') == identity:
        print('Project test image:', rows[0]['Id'])
        raise SystemExit(0)
subprocess.run(['docker', 'build', '--label', 'devdesk.test-recipe-sha256=' + identity,
                '--tag', reference, '--file', str(recipe), str(recipe.parent)], check=True)
