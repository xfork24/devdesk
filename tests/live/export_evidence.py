#!/usr/bin/env python3
"""Archive a consistent project snapshot and registered artifacts; never copy authentication state."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
from datetime import datetime, timezone

SECRET_FIELDS = {'credential_ref', 'credential_reference', 'access_token', 'refresh_token', 'api_key', 'password', 'authorization', 'secret'}

def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()

def decode(value):
    if isinstance(value, bytes):
        value = value.decode('utf-8')
    if isinstance(value, str) and value[:1] in ('{', '['):
        try:
            value = json.loads(value)
        except json.JSONDecodeError:
            pass
    return value

def sanitize(value):
    value = decode(value)
    if isinstance(value, dict):
        return {key: '[redacted]' if key.lower() in SECRET_FIELDS else sanitize(item) for key, item in value.items()}
    if isinstance(value, list):
        return [sanitize(item) for item in value]
    return value

def export(data, project, output, binary=None):
    if not project or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_' for c in project):
        raise ValueError('invalid project identifier')
    project_root = data.resolve() / 'projects' / project
    database = project_root / 'engineering.sqlite3'
    if not database.is_file():
        raise FileNotFoundError(database)
    for path in [data.resolve()/'projects', project_root, database, project_root/'artifacts']:
        if path.is_symlink():
            raise ValueError('symlink in evidence source')
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    manifest = {'project_id': project, 'captured_at': datetime.now(timezone.utc).isoformat(), 'consistent_project_snapshot': True,
                'excluded': ['global credentials/settings', 'authentication files', 'worker state', 'environment variables'], 'files': [], 'errors': []}
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as original:
        with sqlite3.connect(output / 'project.sqlite3') as snapshot:
            original.backup(snapshot)
    with sqlite3.connect(output / 'project.sqlite3') as snapshot:
        snapshot.row_factory = sqlite3.Row
        tables = [row[0] for row in snapshot.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
        records = {}
        for table in tables:
            quoted = '"' + table.replace('"', '""') + '"'
            records[table] = [sanitize(dict(row)) for row in snapshot.execute('SELECT * FROM ' + quoted)]
        (output / 'project-records.json').write_text(json.dumps(records, ensure_ascii=False, indent=2) + '\n')
        artifacts = list(snapshot.execute('SELECT id,relative_path,sha256 FROM artifacts')) if 'artifacts' in tables else []
        manifest['table_counts'] = {name: len(rows) for name, rows in records.items()}
    artifact_root = (project_root / 'artifacts').resolve()
    for artifact in artifacts:
        relative = Path(artifact['relative_path'])
        if relative.is_absolute() or '..' in relative.parts:
            manifest['errors'].append({'artifact_id': artifact['id'], 'error': 'unsafe_path'})
            continue
        # Artifact.relative_path is relative to the project, including artifacts/.
        # Never reinterpret it relative to the artifact subdirectory or copy other
        # project files even if a corrupted database registers them as artifacts.
        source = project_root / relative
        if source.is_symlink() or not source.is_file() or artifact_root not in source.resolve().parents:
            manifest['errors'].append({'artifact_id': artifact['id'], 'error': 'missing_or_unsafe_artifact'})
            continue
        actual = digest(source)
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(source.read_bytes())
        if actual != artifact['sha256']:
            manifest['errors'].append({'artifact_id': artifact['id'], 'error': 'content_drift', 'expected': artifact['sha256'], 'actual': actual})
    global_database = data.resolve() / 'global.sqlite3'
    if global_database.is_file():
        with sqlite3.connect(global_database.as_uri() + '?mode=ro', uri=True) as global_db:
            global_db.row_factory = sqlite3.Row
            global_db.execute('BEGIN')
            facts = {'installations': [sanitize(dict(row)) for row in global_db.execute('SELECT * FROM runtime_installations')],
                     'observations': [sanitize(dict(row)) for row in global_db.execute("SELECT key,value FROM settings WHERE key LIKE 'runtime.%'")]}
        (output / 'runtime-facts.json').write_text(json.dumps(facts, ensure_ascii=False, indent=2) + '\n')
    if binary:
        manifest['binary'] = {'path': str(binary.resolve()), 'sha256': digest(binary)}
    for path in sorted(output.rglob('*')):
        if path.is_file():
            manifest['files'].append({'path': str(path.relative_to(output)), 'bytes': path.stat().st_size, 'sha256': digest(path)})
    manifest['artifacts_verified'] = not manifest['errors']
    (output / 'MANIFEST.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
    return manifest

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data', type=Path, required=True)
    parser.add_argument('--project', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path)
    args = parser.parse_args()
    result = export(args.data, args.project, args.output, args.binary)
    print(json.dumps({'output': str(args.output.resolve()), 'artifacts_verified': result['artifacts_verified'], 'errors': result['errors']}, ensure_ascii=False))
    raise SystemExit(0 if result['artifacts_verified'] else 1)
