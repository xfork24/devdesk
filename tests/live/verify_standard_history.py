#!/usr/bin/env python3
"""Read-only verification of actual Standard adoption and historical facts after a live run."""
import argparse
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
        rows = connection.execute('SELECT id,kind,data FROM governance_records').fetchall()
        sources = connection.execute("SELECT scope,data FROM source_definitions WHERE kind='standard'").fetchall()
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
        assert json.loads(scope)['kind'] != 'feature'
        assert set(json.loads(raw)) == {'schema_version', 'root_workflow_refs'}
        assert json.loads(raw)['schema_version'] == 4
    initial_calls = [data for _, data in by_kind['workflow_invocation']]
    assert next(data for data in initial_calls if data['child_instance_id'] == before['a']['id'])['standard_ref'] == context['sa']['id']
    assert next(data for data in initial_calls if data['child_instance_id'] == before['b']['id'])['standard_ref'] == context['sb']['id']
    successful = [data for _, data in by_kind['platform_call'] if data['status'] == 'succeeded']
    names = [data['request']['definition']['qualified_name'] for data in successful]
    assert names.count('tests.run') >= 6 and names.count('git.advance') == 3 and names.count('git.publish') == 3
    return {'passed': True, 'models_called_by_verifier': False, 'business_steps_completed': 62,
            'waivers': 0, 'standard_changes': 1, 'preserved_outputs': old_outputs,
            'initial_standards': [context['sa']['id'], context['sb']['id']],
            'original_plan': original_plan, 'active_plan': task_a['active_plan_revision_id'],
            'test_receipts': names.count('tests.run'), 'git_advances': 3, 'git_publications': 3}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    result = verify(args.directory.resolve())
    (args.directory / 'integrity-report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps(result, ensure_ascii=False, indent=2))
