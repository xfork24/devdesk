#!/usr/bin/env python3
"""Read-only verification of actual Standard adoption and historical facts after a live run."""
import argparse
import hashlib
import subprocess
import json
from pathlib import Path
import sqlite3
import uuid


def verify(directory: Path):
    context = json.loads((directory / 'context.json').read_text())
    uuid.UUID(context['project'])
    before = json.loads((directory / 'before-switch.json').read_text())
    database = directory / 'data/projects' / context['project'] / 'engineering.sqlite3'
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
        rows = connection.execute('SELECT id,type,data FROM governance_records').fetchall()
        sources = connection.execute("SELECT scope,data FROM source_definitions WHERE type='standard'").fetchall()
    by_id = {key: (kind, json.loads(raw)) for key, kind, raw in rows}
    by_kind = {}
    for key, kind, raw in rows:
        by_kind.setdefault(kind, []).append((key, json.loads(raw)))
    assert not by_kind.get('workflow_waiver'), 'Live acceptance cannot waive any business gate'
    instances = [(key, data) for key, data in by_kind['workflow_instance'] if data['root_instance_id'] == context['root']]
    assert len(instances) == 3
    assert all(data['status'] == 'completed' for _, data in instances)
    assert all(step['status'] == 'completed' for _, data in instances for step in data['steps']), 'No skipped business steps'
    assert sum(len(data['steps']) for _, data in instances) == 62
    task_a = by_id[before['a']['id']][1]
    task_b = by_id[before['b']['id']][1]
    assert task_a['owner_id'] != task_b['owner_id']
    assert task_a['plan']['standard_ref'] == task_b['plan']['standard_ref'] == context['sb']['id']
    original_plan = before['a']['data']['active_plan_revision_id']
    assert by_id[original_plan][1]['standard_ref'] == context['sa']['id']
    assert by_id[task_a['active_plan_revision_id']][1]['standard_ref'] == context['sb']['id']
    changes = by_kind.get('workflow_change', [])
    assert len(changes) == 1 and changes[0][1]['status'] == 'applied'
    operations = changes[0][1]['changeset']['operations']
    assert len(operations) == 1 and operations[0]['type'] == 'adopt_standard'
    assert operations[0]['standard_ref'] == context['sb']['id']
    assert changes[0][1]['changeset']['plan_revision_id'] == original_plan
    preserved = {step['id']: step for step in task_a['steps']}
    preserved.update({data['id']: data for _, data in by_kind.get('workflow_execution_history', [])})
    old_outputs = []
    for old in before['a']['data']['steps']:
        if not old['started']:
            continue
        current = preserved[old['id']]
        for field in ['plan_revision_id', 'input_digest', 'inputs', 'outputs']:
            assert current[field] == old[field], (old['id'], field)
        for output in old['outputs'].values():
            assert by_id[output][1]['value'] == before['outputs'][output]['value']
            old_outputs.append(output)
    for scope, raw in sources:
        assert not json.loads(scope).startswith('feature:')
        assert set(json.loads(raw)) == {'schema_version'}
        assert json.loads(raw)['schema_version'] == 4
    initial_calls = [data for _, data in by_kind['workflow_invocation']]
    assert next(data for data in initial_calls if data['child_instance_id'] == before['a']['id'])['standard_ref'] == context['sa']['id']
    assert next(data for data in initial_calls if data['child_instance_id'] == before['b']['id'])['standard_ref'] == context['sb']['id']
    successful = [data for _, data in by_kind['platform_call'] if data['status'] == 'succeeded']
    names = [data['request']['definition']['qualified_name'] for data in successful]
    assert names.count('tests.run') >= 6 and names.count('git.advance') == 3 and names.count('git.publish') == 3
    repository = directory / 'data/projects' / context['project'] / 'repository'
    def blob(revision, path):
        assert len(revision) in [40, 64] and all(ch in '0123456789abcdef' for ch in revision)
        assert not path.startswith('/') and '..' not in path.split('/')
        return subprocess.run(['git', '-C', str(repository), 'show', '--no-textconv', revision + ':' + path], check=True, capture_output=True, timeout=15).stdout
    publications = []
    pairs_by_task = {}
    task_test_files = set()
    task_test_contents = {}
    for _, child in instances:
        if child['parent_execution_id'] is not None:
            red_step = next(step for step in child['steps'] if step['step'] == 'red')
            red_receipt = by_id[red_step['outputs']['result']][1]['value']
            task_test_files.update(red_receipt['recipe']['files'])
            task_test_contents.update({file['path']: file['content'] for file in red_receipt['execution']['files']})
    assert len(task_test_files) == 2
    add_test = next(path for path in task_test_files if '-add/behavior.test.mjs' in path)
    expected_feature_tests = task_test_files | {add_test.replace('-add/behavior.test.mjs', '-feature/behavior.test.mjs')}
    for _, instance in instances:
        def execution(key):
            return next(step for step in instance['steps'] if step['step'] == key)
        def receipt(key):
            return by_id[execution(key)['outputs']['result']][1]['value']
        candidate, tests, publication = (receipt(key) for key in ['candidate', 'candidate_tests', 'publish'])
        assert candidate['parents'] == [candidate['target_revision']] and candidate['diff']
        assert tests['status'] == 'succeeded' and tests['exit_code'] == 0 and tests['container_id']
        assert tests['candidate'] == candidate['revision']
        for file in tests['execution']['files']:
            assert hashlib.sha256(file['content'].encode()).hexdigest() == file['sha256']
            assert blob(candidate['revision'], file['path']) == file['content'].encode()
            if file['path'] in task_test_contents:
                assert file['content'] == task_test_contents[file['path']], 'Frozen independent Task tests changed during Feature integration'
        pr = publication['pull_request']
        assert pr['merged'] and pr['merge_commit'] == pr['head'] == candidate['revision']
        publications.append({'owner_id': instance['owner_id'], 'candidate': candidate['revision'], 'pull_request': pr})
        if instance['parent_execution_id'] is None:
            assert instance['owner_id'] == context['feature'] and instance['plan']['standard_ref'] == context['sa']['id']
            assert set(tests['recipe']['files']) == expected_feature_tests, 'Feature candidate must run both independent Task tests and its full test'
            continue
        assert set(tests['recipe']['files']) == set(receipt('red')['recipe']['files'])
        review = receipt('review')
        assert review['approve'] and any(check['key'] == 'standard_b_trace' and check['passed'] for check in review['checks']), 'The selected Standard B check must actually pass in review'
        red, green = receipt('red'), receipt('green')
        assert red['status'] == 'succeeded' and red['expected_failure'] and red['assertion_failed'] and red['counts']['failed'] > 0 and red['container_id']
        assert 'ERR_MODULE_NOT_FOUND' not in red['report']
        assert green['status'] == 'succeeded' and green['exit_code'] == 0 and green['container_id']
        assert red['execution']['files'] == green['execution']['files'], 'Independent tests changed between Red and Green'
        implementation = execution('implementation')
        runs = by_id[implementation['outputs']['evidence']][1]['value']['runs']
        observed = []
        journals = directory / 'data/projects' / context['project'] / 'execution/sessions' / implementation['session_id'] / 'coordinator'
        def records(value):
            if isinstance(value, dict):
                yield value
                for child in value.values():
                    yield from records(child)
            elif isinstance(value, list):
                for child in value:
                    yield from records(child)
        for journal in journals.glob('*.jsonl'):
            for line in journal.open():
                frame = json.loads(line)['frame']
                item = frame.get('params', {}).get('item', {})
                if frame.get('method') != 'item/completed' or item.get('type') != 'commandExecution' or item.get('status') != 'completed':
                    continue
                body = item.get('aggregatedOutput') or ''
                for fragment in [body, *body.splitlines()]:
                    try:
                        value = json.loads(fragment)
                    except (ValueError, TypeError):
                        continue
                    observed.extend(record for record in records(value) if record in runs)
        pending, pairs = None, 0
        for run in runs:
            assert run in observed, 'Incremental run absent from actual native command output'
            for file in run['files']:
                assert hashlib.sha256(file['content'].encode()).hexdigest() == file['sha256']
            if run['phase'] == 'red':
                assert run['exit_code'] not in [0, None] and 'ERR_MODULE_NOT_FOUND' not in run['stdout'] + run['stderr']
                pending = run
            elif run['phase'] == 'green' and run['exit_code'] == 0 and pending:
                assert run['command'] == pending['command'] and run['cwd'] == pending['cwd']
                def tests_in(record):
                    return {file['path']: file['content'] for file in record['files'] if file['path'].removeprefix('/work/').removeprefix('./').startswith(('test/', 'tests/'))}
                assert tests_in(run) and tests_in(run) == tests_in(pending)
                pairs += 1
                pending = None
        assert pairs >= 2, 'Both requested incremental Red/Green pairs must be preserved'
        pairs_by_task[instance['owner_id']] = pairs
    (directory / 'publication-evidence.json').write_text(json.dumps(publications, ensure_ascii=False, indent=2))
    return {'passed': True, 'models_called_by_verifier': False, 'business_steps_completed': 62,
            'waivers': 0, 'standard_changes': 1, 'preserved_outputs': old_outputs,
            'initial_standards': [context['sa']['id'], context['sb']['id']],
            'original_plan': original_plan, 'active_plan': task_a['active_plan_revision_id'],
            'test_receipts': names.count('tests.run'), 'git_advances': 3, 'git_publications': 3,
            'increment_pairs_by_task': pairs_by_task, 'exact_tested_publications': publications}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    result = verify(args.directory.resolve())
    (args.directory / 'integrity-report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps(result, ensure_ascii=False, indent=2))
