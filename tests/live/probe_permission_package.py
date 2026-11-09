#!/usr/bin/env python3
"""Verify Permission assets and APIs in an unpacked binary release, without models."""
import argparse
import hashlib
import http.cookiejar
import json
from pathlib import Path
import sqlite3
import subprocess
import tarfile
import tempfile
import urllib.request
import uuid

p = argparse.ArgumentParser()
p.add_argument('--archive', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
source = Path(__file__).resolve().parents[2]
report = {'passed': False, 'models_called': False, 'source_checkout_required': False}
child = None
try:
    report['archive'] = str(a.archive.resolve())
    report['archive_sha256'] = hashlib.sha256(a.archive.read_bytes()).hexdigest()
    checksum = a.archive.with_name(a.archive.name + '.sha256').read_text().split()[0]
    assert checksum == report['archive_sha256']
    with tempfile.TemporaryDirectory(prefix='devdesk-permission-package-') as tmp:
        root = Path(tmp)
        with tarfile.open(a.archive) as archive:
            for entry in archive.getmembers():
                path = Path(entry.name)
                assert not path.is_absolute() and '..' not in path.parts
                assert entry.isfile() or entry.isdir()
            archive.extractall(root, filter='data')
        package = next(path for path in root.iterdir() if path.is_dir())
        binary = package / 'devdesk-host'
        report['binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
        assert report['binary_sha256'] == hashlib.sha256((source / 'target/release/devdesk-host').read_bytes()).hexdigest()
        manifest = json.loads((package / 'permissions/backends/manifest.json').read_text())
        for name, entry in manifest['assets'].items():
            body = (package / 'permissions/backends' / name).read_bytes()
            assert hashlib.sha256(body).hexdigest() == entry['sha256']
        for name in ['permission.schema.json', 'permission-backend.schema.json', 'rpc.schema.json']:
            assert (package / 'schemas' / name).read_bytes() == (source / 'schemas' / name).read_bytes()
        presets = list((package / 'permissions/presets').glob('*.json'))
        assert len(presets) == 6
        report['assets_verified'] = len(manifest['assets'])
        report['presets_verified'] = len(presets)
        dataset = root / 'isolated data'
        child = subprocess.Popen([str(binary), '--data', str(dataset), '--port', '0'], cwd=root,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child.stdout.readline()
        url = child.stdout.readline().strip()
        assert url.startswith('http://127.0.0.1:')
        http = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                                          urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        http.open(url + '/', timeout=10).close()
        def request(method, params=None):
            body = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}).encode()
            req = urllib.request.Request(url + '/rpc', data=body, headers={'Content-Type': 'application/json'})
            return json.load(http.open(req, timeout=20))
        def rpc(method, params=None):
            result = request(method, params)
            assert 'error' not in result, result.get('error')
            return result['result']
        def command(method, payload, target=None, revision=None):
            return rpc(method, {'command_id': str(uuid.uuid4()), 'target': target,
                                'expected_revision': revision, 'payload': payload})
        settings = rpc('query.permissions')
        config = settings['configuration']
        assert config['approval'] == 'never'
        for provider in ['codex', 'dsh']:
            assert config['harnesses'][provider]['permission']['revision_id'] == 'permission-container-full-access-v1'
            plan = rpc('permission.preflight', {'provider': provider})['resolved']
            assert not plan['isolation']['nested_sandbox'] and plan['isolation']['executable_tmp']
            assert any(b['id'] == plan['enforcement']['harness_backend'] for b in manifest['backends'])
        denied = request('permission.preflight', {'provider': 'codex', 'overrides': {'harness': {
            'overrides': {'network': {'mode': 'allowlist', 'destinations': ['example.com']}}}}})
        assert denied['error']['data']['code'] == 'permission_unsupported'
        policy = {'filesystem': {'mode': 'read_only'}, 'network': {'mode': 'enabled'}, 'tools': {'mode': 'all_available'}}
        created = command('permission.create', {'identity': 'package-smoke', 'name': 'Package smoke', 'policy': policy})
        revised = command('permission.revise', {'name': 'Package immutable revision', 'policy': policy}, created['id'], 1)
        assert next(v for v in rpc('query.permissions')['permissions'] if v['id'] == created['id']) == created
        config['harnesses']['codex'] = {'permission': {'revision_id': revised['id']}, 'overrides': {}}
        binding = {'command_id': str(uuid.uuid4()), 'target': 'permission-configuration',
                   'expected_revision': config['revision'], 'payload': config}
        saved = rpc('permission.bind', binding)
        assert saved == rpc('permission.bind', binding)
        project = command('project.create', {'title': 'Standalone Permission schema'})['id']
        db = dataset / 'projects' / project / 'engineering.sqlite3'
        expected_schema = max(int(path.name.split('_', 1)[0]) for path in (source / 'sql').glob('[0-9]*_*.sql'))
        with sqlite3.connect('file:' + str(db) + '?mode=ro', uri=True) as conn:
            assert conn.execute('PRAGMA user_version').fetchone()[0] == expected_schema
        assert rpc('query.runtime_installations')['installations'] == []
        req = urllib.request.Request(url + '/shutdown', data=b'{}', headers={'Content-Type': 'application/json'})
        http.open(req, timeout=10).close()
        assert child.wait(timeout=10) == 0
        report.update(passed=True, standalone_api=True, immutable_revisions=True,
                      binding_replay=True, project_schema=expected_schema, automatic_downloads=0)
except Exception as error:
    report.update(failure_type=type(error).__name__, failure=str(error))
finally:
    if child is not None and child.poll() is None:
        child.terminate()
        child.wait(timeout=10)
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
raise SystemExit(0 if report['passed'] else 1)
