#!/usr/bin/env python3
"""Verify Permission assets and APIs in an unpacked binary release, without models."""
import argparse
import hashlib
import http.cookiejar
import json
import re
from pathlib import Path
import sqlite3
import subprocess
import tarfile
import tempfile
import time
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
        for name in ['permission.schema.json', 'permission-profile.schema.json', 'permission-binding.schema.json', 'sandbox.schema.json', 'permission-backend.schema.json', 'rpc.schema.json', 'agent-task.schema.json']:
            assert (package / 'schemas' / name).read_bytes() == (source / 'schemas' / name).read_bytes()
        presets = list((package / 'permissions/presets').glob('*.json'))
        assert len(presets) == 3
        assert all(json.loads(p.read_text())["schema_version"] == 2 for p in presets)
        assert (package / 'scripts/upgrade/harness-v5.py').read_bytes() == (source / 'scripts/upgrade/harness-v5.py').read_bytes()
        assert (package / 'schemas/orchestration.schema.json').read_bytes() == (source / 'schemas/orchestration.schema.json').read_bytes()
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
        resources = rpc('query.permission_resources')
        assert resources['profiles'] and resources['source']['is_builtin']
        assert str(package.resolve()) in resources['source']['path']
        target_row = next(t for t in resources['targets'] if t['provider'] == 'codex' and t['supported'])
        target = target_row['target']
        initial_revision = target_row['active']['revision']
        preview = rpc('permission.binding.preflight', {'target': target, 'permission_revision_id': 'permission-harness-read-only-v2-r1'})
        assert preview['compatible'] and not preview['scope_checked']
        applied = command('permission.bindings.apply', {'bindings': [{'target': target, 'permission_revision_id': 'permission-harness-read-only-v2-r1', 'expected_revision': initial_revision}]})
        assert applied['bindings'][0]['revision'] == initial_revision + 1
        exported = rpc('permission.profile.export', {'id': 'permission-harness-read-only-v2-r1'})
        assert len(exported['profiles']) == 1
        command('permission.profile.import', exported)
        report['v2_profiles_and_version_binding'] = True
        runtime_preset = {**exported['profiles'][0], 'id': 'package-runtime-v2', 'identity': 'package-runtime-v2', 'name': 'Runtime package preset'}
        preset_root = dataset / 'permissions/presets'
        preset_root.mkdir(parents=True)
        (preset_root / 'runtime.json').write_text(json.dumps(runtime_preset))
        command('permission.source.select', {'path': str(preset_root)})
        command('permission.profile.reload', {})
        reloaded = rpc('query.permission_resources')
        assert any(p['id'] == runtime_preset['id'] for p in reloaded['profiles'])
        command('permission.profile.reload', {'builtin': True})
        assert rpc('query.permission_resources')['source']['is_builtin']
        assert next(t for t in reloaded['targets'] if t['target'] == target)['active']['revision'] == initial_revision + 1
        report['runtime_reload_without_restart'] = True
        assert request('query.permissions')['error']['data']['code'] == 'method_not_found'
        report['current_only_permission_api'] = True
        project = command('project.create', {'title': 'Standalone Permission schema'})['id']
        defaults = rpc('query.configuration_defaults')
        workflow = rpc('query.workflow', {'id': defaults['workflow_ref']})
        assert workflow['data']['schema_version'] == 5
        feature_tests = next(step for step in workflow['data']['steps'] if 'release_recipe' in step.get('tags', []))
        assert feature_tests['access']['writable_paths'], 'Feature test author must have actual test-directory access'
        report['feature_test_author_has_explicit_write_scope'] = True
        document = json.loads(json.dumps(workflow['data']))
        step = next(s for s in document['steps'] if s['executor']['type'] == 'agent')
        assert step['task_instructions'].strip()
        original_task = step['task_instructions']
        changed_task = '# Package task\n\n原文 {{literal}}, `code`\n'
        step['task_instructions'] = changed_task
        draft = command('workflow.create', {'name': 'Standalone task instructions', 'document': document})
        published = command('workflow.publish', {}, draft['id'], draft['revision'])
        assert next(s for s in published['data']['steps'] if s['key'] == step['key'])['task_instructions'] == changed_task
        assert next(s for s in rpc('query.workflow', {'id': workflow['id']})['data']['steps'] if s['key'] == step['key'])['task_instructions'] == original_task
        preview = rpc('query.workflow_task_preview', {'project_id': project, 'id': published['id'], 'step': step['key'], 'preview_agent_ref': defaults['role_agent_bindings'][step['executor']['role_ref']]})
        assert preview['task']['task_instructions'] == changed_task
        assert preview['task']['agent'] and preview['task']['specifications']
        assert preview['task']['renderer_version'] == 'agent-task/v5'
        report['workflow_v5_embedded_tasks_edit_publish_preview'] = True
        capture = next(row for row in defaults['records'] if row['kind'] == 'tool' and row['data'].get('qualified_name') == 'materials.capture')
        complete = next(row for row in defaults['records'] if row['kind'] == 'tool' and row['data'].get('qualified_name') == 'tasks.complete')
        task_workflow = rpc('query.workflow', {'id': defaults['task_workflow_ref']})
        task_steps = {step['key']: step for step in task_workflow['data']['steps']}
        feature_steps = {step['key']: step for step in workflow['data']['steps']}
        assert len(feature_steps) == 22 and len(task_steps) == 20
        assert 'engineering_constraints' in feature_steps
        for graph in [workflow['data'], task_workflow['data']]:
            assert all(step.get('confirmation', 'automatic') == 'automatic' for step in graph['steps'])
            assert graph['action_requirements']['git.advance'] == ['tests']
        for steps, review_key, snapshot_key in [(feature_steps, 'closure_review', 'tests_snapshot'), (task_steps, 'review', 'implementation_snapshot')]:
            assert steps[review_key]['dependencies'] == ['candidate_tests']
            assert steps[review_key]['inputs']['candidate']['source']['step'] == 'candidate'
            assert steps[review_key]['inputs']['candidate_tests']['source']['step'] == 'candidate_tests'
            assert steps[review_key]['inputs']['changes']['source'] == {'step': snapshot_key, 'port': 'result', 'pointer': '/changes'}
            assert steps[snapshot_key]['executor']['arguments']['include_diff']['value'] is True
        evidence_type = next(row for row in defaults['records'] if row['id'] == task_steps['implementation']['outputs']['evidence']['delivery_ref'])
        assert {'environment', 'termination'} <= set(evidence_type['data']['content_schema']['properties']['runs']['items']['required'])
        report['merged_main_evidence_and_automatic_policy'] = True
        for key in ['closure_review', 'candidate_review']:
            assert feature_steps[key]['inputs']['tasks']['source'] == {'step': 'implementation', 'port': 'result', 'pointer': ''}
        for method in ['workflow.change.preview', 'workflow.change.apply']:
            tool = next(row for row in defaults['records'] if row['kind'] == 'tool' and row['data'].get('qualified_name') == method)
            operations = tool['data']['input_schema']['properties']['changeset']['properties']['operations']['items']['oneOf']
            assert any(operation['properties']['type'].get('const') == 'revert_change' for operation in operations)
        report['complete_feature_review_evidence_and_bounded_change_recovery'] = True
        assert task_steps['red_baseline']['inputs']['workspace']['source']['step'] == 'tests_snapshot'
        assert task_steps['red_snapshot']['executor']['arguments']['workspace']['source']['step'] == 'tests_snapshot'
        assert task_steps['tests_snapshot']['dependencies'] == ['tests']
        report['explicit_snapshot_between_writers'] = True
        assert task_workflow['data']['steps'][-1]['executor']['tool_ref'] == complete['id']
        implementation_review = next(step for step in task_workflow['data']['steps'] if 'verify' in step.get('inputs', {}))
        verify_step = next(step for step in task_workflow['data']['steps'] if step['key'] == implementation_review['inputs']['verify']['source']['step'])
        ancestors = set()
        def dependencies(key):
            for parent in task_steps[key].get('dependencies', []):
                if parent not in ancestors:
                    ancestors.add(parent)
                    dependencies(parent)
        dependencies(implementation_review['key'])
        assert verify_step['executor']['type'] == 'tool' and verify_step['key'] in ancestors
        # Exercise the actual standalone scheduler and platform handler without a model or fake Session.
        generic = command('workflow.create', {'name': 'Standalone configured capture', 'document': {
            'schema_version': 5, 'stages': [{'key': 'audit', 'name': '自定义包验收'}], 'steps': [
                {'key': 'ready', 'name': '准备', 'stage': 'audit', 'executor': {'type': 'control'}},
                {'key': 'capture', 'name': '记录实际输入', 'stage': 'audit', 'dependencies': ['ready'],
                 'executor': {'type': 'tool', 'tool_ref': capture['id'], 'arguments': {'value': {'type': 'constant', 'value': {'version': 5}}}},
                 'outputs': {'result': {'delivery_ref': 'builtin-delivery-object-v1'}}}
            ]}})
        generic = command('workflow.publish', {}, generic['id'], generic['revision'])
        active = rpc('query.workspace')['active_project']
        run = command('workflow.start', {'workflow_ref': generic['id'], 'role_agent_bindings': {}}, project, active['revision'])
        time.sleep(.2)
        assert all(s['status'] == 'queued' for s in rpc('query.workflow_instance', {'id': run['id']})['runtime']['steps'])
        for _ in range(150):
            execution = rpc('query.workflow_instance', {'id': run['id']})
            if execution['runtime']['status'] == 'completed':
                break
            assert execution['runtime']['status'] not in ['failed', 'needs_reconciliation'], execution
            runtime = execution['runtime']
            base = {'plan_revision_id': runtime['active_plan_revision_id'], 'control_version': runtime['control_version']}
            if all(s['status'] in ('completed', 'skipped') for s in runtime['steps']):
                command('workflow.complete', {**base, 'reason': '独立包实际交付核对完成'}, run['id'], execution['record']['revision'])
                continue
            for step in runtime['steps']:
                if step['status'] == 'awaiting_acceptance':
                    command('workflow.step.accept', {**base, 'step': step['step'], 'execution_id': step['id'], 'input_digest': step['input_digest'], 'outputs': step['outputs'], 'reason': '核对独立包真实工具结果'}, run['id'], execution['record']['revision'])
                    break
                definition = next(s for s in execution['plan']['steps'] if s['key'] == step['step'])
                if step['status'] == 'queued' and all(any(s['step'] == key and s['status'] == 'completed' for s in runtime['steps']) for key in definition.get('dependencies', [])):
                    command('workflow.step.dispatch', {**base, 'step': step['step']}, run['id'], execution['record']['revision'])
                    break
            time.sleep(.1)
        else:
            raise TimeoutError('standalone configured workflow')
        captured = execution['runtime']['steps'][1]
        assert captured['session_id'] is None and captured['turn_id'] is None
        assert execution['outputs'][captured['outputs']['result']]['value']['inputs']['value'] == {'version': 5}
        report['standalone_generic_workflow_execution'] = True
        report['explicit_task_completion_embedded'] = True
        report['delivery_verification_before_review_embedded'] = True
        db = dataset / 'projects' / project / 'engineering.sqlite3'
        expected_schema = int(re.search(r'PRAGMA user_version=(\d+)', (source / 'sql/project/schema.sql').read_text()).group(1))
        with sqlite3.connect('file:' + str(db) + '?mode=ro', uri=True) as conn:
            assert conn.execute('PRAGMA user_version').fetchone()[0] == expected_schema
        assert rpc('query.runtime_installations')['installations'] == []
        req = urllib.request.Request(url + '/shutdown', data=b'{}', headers={'Content-Type': 'application/json'})
        http.open(req, timeout=10).close()
        assert child.wait(timeout=10) == 0
        subprocess.run(['python3', str(package / 'scripts/upgrade/harness-v5.py'), str(dataset), '--binary', str(binary)], cwd=root, check=True, capture_output=True)
        report['standalone_upgrade_entrypoint'] = True
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
