#!/usr/bin/env python3
"""An exported SQLite/Artifact snapshot stays independently verifiable after source removal."""
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory(prefix='devdesk-evidence-') as temp:
    root = Path(temp)
    project = root / 'data/projects/project-fixture'
    artifact = project / 'artifacts/project-project-fixture/outputs/result.json'
    artifact.parent.mkdir(parents=True)
    content = b'{"actual":"sealed artifact"}'
    artifact.write_bytes(content)
    (project / 'worker-state').mkdir()
    (project / 'worker-state/auth.json').write_text('must never be copied')
    sha = hashlib.sha256(content).hexdigest()
    db = sqlite3.connect(project / 'engineering.sqlite3')
    assert db.execute('PRAGMA journal_mode=WAL').fetchone() == ('wal',)
    with db:
        db.execute('CREATE TABLE artifacts(id TEXT,relative_path TEXT,sha256 TEXT)')
        db.execute('INSERT INTO artifacts VALUES(?,?,?)', ('artifact', 'artifacts/project-project-fixture/outputs/result.json', sha))
    output = root / 'evidence'
    subprocess.run([sys.executable, str(Path(__file__).resolve().parents[1] / 'live/export_evidence.py'), '--data', str(root/'data'), '--project', 'project-fixture', '--output', str(output)], check=True)
    with db:
        db.execute('INSERT INTO artifacts VALUES(?,?,?)', ('unsafe', 'worker-state/auth.json', 'unknown'))
    artifact.write_bytes(b'changed after sealed registration')
    unsafe = root / 'unsafe-evidence'
    rejected = subprocess.run([sys.executable, str(Path(__file__).resolve().parents[1] / 'live/export_evidence.py'), '--data', str(root/'data'), '--project', 'project-fixture', '--output', str(unsafe)], capture_output=True)
    assert rejected.returncode == 1
    rejected_manifest = json.loads((unsafe/'MANIFEST.json').read_text())
    assert not rejected_manifest['artifacts_verified']
    assert any(error['artifact_id'] == 'artifact' and error['error'] == 'content_drift' for error in rejected_manifest['errors'])
    assert any(error['artifact_id'] == 'unsafe' and error['error'] == 'missing_or_unsafe_artifact' for error in rejected_manifest['errors'])
    assert not list(unsafe.rglob('auth.json'))
    db.close()
    shutil.rmtree(root/'data')
    manifest = json.loads((output/'MANIFEST.json').read_text())
    assert manifest['artifacts_verified']
    assert not list(output.rglob('auth.json'))
    assert not list(output.glob('project.sqlite3-*')), 'Archive must not depend on transient SQLite sidecars'
    assert not any(entry['path'].startswith('project.sqlite3-') for entry in manifest['files'])
    for entry in manifest['files']:
        assert hashlib.sha256((output/entry['path']).read_bytes()).hexdigest() == entry['sha256']
    with sqlite3.connect(output/'project.sqlite3') as db:
        row = db.execute('SELECT sha256 FROM artifacts').fetchone()
        assert row == (sha,)
    assert (output/'artifacts/project-project-fixture/outputs/result.json').read_bytes() == content
    print('PASS evidence archive: independent SQLite snapshot, exact Artifact contents, file hashes, authentication exclusion')
