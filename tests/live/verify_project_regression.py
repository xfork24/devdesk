#!/usr/bin/env python3
"""Read-only acceptance of a completed delivery, including actual project tests.

Unlike a green workflow counter, this checks the original contract, exact Human
decisions, independent Red/Green, all source/merge regressions and Git receipts.
Requires the observed Host's local data directory for durable corroboration.
"""
import argparse
import hashlib
import json
import sqlite3
import subprocess
from contextlib import closing
from pathlib import Path, PurePosixPath

from client import Client
from engineering_evidence import frozen_document, verify_source_contract
from workflow_observer import read_workflow_tree


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def check_confirmation(decision, step):
    require(decision.get('actor', {}).get('kind') == 'human', 'Missing actual Human decision')
    require(decision.get('execution_id') == step['id'] and
            decision.get('input_digest') == step['input_digest'] and
            decision.get('outputs') == step['outputs'], 'Stale Human decision')
    require(not any(decision.get(k) for k in
                    ('override_completion', 'manual_approval', 'exception_scope')),
            'Completion exception cannot substitute for evidence')


def check_test(result, candidate, red=False):
    require(result.get('status') == 'succeeded' and result.get('skipped') is not True,
            'Test was skipped or unsuccessful')
    require(result.get('candidate') == candidate, 'Test candidate mismatch')
    require(result.get('container_id') and result.get('writer_revoked') is True and
            result.get('image_digest', '').startswith('sha256:'), 'Missing isolated execution proof')
    execution = result.get('execution')
    require(isinstance(execution, dict) and execution.get('command') and
            execution.get('files') and execution.get('cwd') == '/work', 'Missing test execution')
    counts = result.get('counts', {})
    require(counts.get('tests', 0) > 0, 'Zero tests cannot prove regression')
    if red:
        require(result.get('expected_failure') is True and result.get('assertion_failed') is True and
                counts.get('failed', 0) > 0 and result.get('exit_code') not in (0, None),
                'Red must fail a business assertion')
    else:
        require(result.get('expected_failure') is False and result.get('exit_code') == 0 and
                execution.get('exit_code') == 0 and counts.get('failed') == 0 and
                counts.get('passed', 0) > 0 and counts.get('skipped', 0) == 0 and
                counts.get('cancelled', 0) == 0 and counts.get('todo', 0) == 0 and
                counts['passed'] == counts['tests'], 'Tests did not all pass')
    require(result.get('report') and hashlib.sha256(result['report'].encode()).hexdigest() ==
            result.get('report_sha256'), 'Test report digest mismatch')


def check_merge(merged, publication):
    revision = publication['revision']
    require(merged.get('publication') == publication, 'Merge used a different publication')
    require(merged.get('pull_request', {}).get('merged') is True and
            merged['pull_request'].get('merge_commit') == revision and
            merged.get('revision') == revision, 'PR did not merge the tested candidate')
    require(merged.get('synchronized') is True and
            merged.get('sync', {}).get('synchronized') is True and
            merged['sync'].get('revision') == revision, 'Missing exact target synchronization')


def check_unassisted(records, root, members, controller):
    """Normal Human confirmations are expected; maintenance controls are not."""
    require(controller['epoch'] == 1, 'Manager required a manual takeover/restart')
    for kind, definition, item in records:
        if kind == 'controller_input' and definition == controller['id']:
            raise AssertionError('Manager required supplemental operator input')
        if kind == 'workflow_change' and definition in members:
            raise AssertionError('Workflow required a runtime plan repair')
        if kind == 'workflow_human_decision' and item.get('request', {}).get('instance_id') in members:
            require(item['request']['operation'] in ('workflow.step.confirm', 'workflow.step.accept'),
                    'Human approved or rejected an extra maintenance request')
        if kind == 'workflow_decision' and item.get('root_instance_id') == root:
            if item.get('actor', {}).get('kind') == 'human':
                require(item['operation'] == 'confirm', 'Manual maintenance control: ' + item['operation'])


def read_output(client, instance, identity):
    offset, chunks, digest, metadata = 0, [], None, None
    for _ in range(256):
        row = client.rpc('workflow.outputs.read', {
            'id': instance, 'ids': [identity], 'offset': offset, 'max_bytes': 65536})['outputs'][0]
        require(row['id'] == identity, 'Output identity changed')
        data = row['data']
        if 'value' in data:
            require(offset == 0, 'Output representation changed during read')
            return data
        if digest is None:
            digest, metadata = data['provenance']['value_digest'], data
        require(data['provenance']['value_digest'] == digest and row['offset'] == offset,
                'Output cursor/digest changed')
        chunk = row['value_chunk']
        chunks.append(chunk)
        offset += len(chunk.encode())
        require(offset <= 16 * 1024 * 1024, 'Output exceeds acceptance reader budget')
        if row.get('next_offset') is None:
            body = ''.join(chunks)
            require(offset == row['total_bytes'] and hashlib.sha256(body.encode()).hexdigest() == digest,
                    'Incomplete output')
            return {**metadata, 'value': json.loads(body)}
        require(chunk and row['next_offset'] == offset, 'Output cursor did not advance')
    raise AssertionError('Output exceeds acceptance chunk budget')


def verify(client, root, data_root, unassisted=False):
    tree = read_workflow_tree(client, root)
    require(len(tree) >= 2, 'Expected an actual nested Task delivery')
    project = tree[root]['runtime']['project_id']
    directory = Path(data_root).resolve() / 'projects' / project
    repository = directory / 'repository'
    database = directory / 'engineering.sqlite3'
    report = {'root': root, 'project': project, 'instances': [], 'passed': False}
    sessions = set()
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
        def stored(kind, identity):
            row = db.execute('SELECT data FROM governance_records WHERE type=? AND id=?',
                             (kind, identity)).fetchone()
            require(row is not None, 'Missing durable ' + kind + ': ' + identity)
            return json.loads(row[0])

        if unassisted:
            rows = [(kind, definition, json.loads(raw)) for kind, definition, raw in db.execute(
                'SELECT type,definition_id,data FROM governance_records WHERE type IN '
                "('workflow_controller','controller_input','workflow_change','workflow_decision','workflow_human_decision')")]
            controller = next(item for kind, _, item in rows if kind == 'workflow_controller' and
                              item.get('root_instance_id') == root)
            cid = db.execute("SELECT id FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.root_instance_id')=?", (root,)).fetchone()[0]
            check_unassisted(rows, root, set(tree), {**controller, 'id': cid})
            report['unassisted'] = True
        for identity in tree:
            view = client.rpc('query.workflow_instance', {'id': identity})
            runtime, plan = view['runtime'], view['plan']
            sessions.update(s['session_id'] for s in runtime['steps'] if s.get('session_id'))
            require(not view.get('next_step_offset'), 'Acceptance requires complete step definitions')
            require(runtime['status'] == 'completed' and
                    all(s['status'] == 'completed' for s in runtime['steps']), 'Workflow is incomplete')
            revision, seen = runtime['active_plan_revision_id'], set()
            while revision:
                require(revision not in seen, 'Plan ancestry cycle')
                seen.add(revision)
                frozen = stored('workflow_plan_revision', revision)
                require(frozen['instance_id'] == identity, 'Plan belongs to another workflow')
                source = frozen_document(db, frozen)
                revision = frozen.get('parent_revision_id')
            verify_source_contract(source, plan, unassisted)
            require(runtime['role_agent_bindings'] == frozen['role_agent_bindings'],
                    'Frozen role bindings changed')
            for kind in ('workflow_waiver', 'workflow_change'):
                for raw, in db.execute('SELECT data FROM governance_records WHERE type=?', (kind,)):
                    item = json.loads(raw)
                    if item.get('instance_id') == identity:
                        require(kind != 'workflow_waiver' and not item.get('bypassed'), 'Bypassed mandatory work')

            steps = {s['step']: s for s in runtime['steps']}
            cache = {}
            def output(key, port='result', tool=None):
                step = steps[key]
                oid = step['outputs'][port]
                if oid not in cache:
                    cache[oid] = read_output(client, identity, oid)
                item = cache[oid]
                require(item['execution_id'] == step['id'] and
                        item['input_digest'] == step['input_digest'], 'Stale output binding')
                if tool:
                    provenance = item['provenance']
                    require(provenance['kind'] == 'platform' and provenance['tool'] == tool and
                            provenance['call_id'] == step['tool_call_id'], 'Missing platform provenance')
                    call = stored('platform_call', provenance['call_id'])
                    require(call['status'] == 'succeeded' and call['handler'] == tool and
                            call['receipt_digest'] == provenance['value_digest'], 'Durable receipt mismatch')
                return item['value']

            approvals = 0
            for definition in plan['steps']:
                if definition['confirmation'] == 'human':
                    step = steps[definition['key']]
                    row = db.execute("SELECT data FROM governance_records WHERE type='workflow_confirmation' AND definition_id=?",
                                     (step['id'],)).fetchone()
                    require(row is not None, 'Missing Human confirmation')
                    check_confirmation(json.loads(row[0]), step)
                    approvals += 1
                if definition['key'].endswith('review'):
                    review = output(definition['key'])
                    require(review.get('approve') is True and review.get('checks') and
                            all(c.get('passed') is True for c in review['checks']),
                            'Independent review rejected or has failed checks')

            policy = plan['delivery_policy']
            config = runtime['inputs'].get('test_configuration', {}).get('regression')
            require(config and config['runner'] == 'node_test' and config.get('files'),
                    'Expected frozen Node project regression configuration')
            publication = output('publication', tool='git.delivery.open')
            published = output(policy['source_publication_step'], tool='git.source.publish')
            require(published['revision'] == publication['source_revision'], 'Published source changed')
            regressions = []
            for key, candidate in ((policy['source_regression_step'], publication['source_revision']),
                                   (policy['merge_regression_step'], publication['revision'])):
                result = output(key, tool='tests.project')
                check_test(result, candidate)
                files = result['execution']['files']
                require(sorted(f['path'] for f in files) == sorted(config['files']), 'Regression omitted configured files')
                for item in files:
                    path = PurePosixPath(item['path'])
                    require(not path.is_absolute() and '..' not in path.parts, 'Invalid test path')
                    blob = subprocess.run(['git', '-C', str(repository), 'show', candidate + ':' + str(path)],
                                          capture_output=True, check=True, timeout=15).stdout
                    require(blob == item['content'].encode(), 'Test file differs from tested Git candidate')
                regressions.append({'step': key, 'candidate': candidate, 'counts': result['counts'],
                                    'container': result['container_id'], 'operation': result['operation_id'],
                                    'image': result['image_digest'], 'limits': result.get('permissions', {}).get('limits')})
            independent = None
            if policy.get('independent_green_step'):
                red = output('red', tool='tests.run')
                green = output(policy['independent_green_step'], tool='tests.run')
                check_test(red, red['candidate'], red=True)
                check_test(green, publication['source_revision'])
                require(red['execution']['files'] == green['execution']['files'] and
                        red['recipe'] == green['recipe'], 'Independent tests changed between Red and Green')
                require(red['candidate'] != green['candidate'] and red['container_id'] != green['container_id'],
                        'Red and Green did not use distinct actual executions')
                for result in (red, green):
                    for item in result['execution']['files']:
                        path = PurePosixPath(item['path'])
                        require(not path.is_absolute() and '..' not in path.parts, 'Invalid independent test path')
                        blob = subprocess.run(['git', '-C', str(repository), 'show', result['candidate'] + ':' + str(path)],
                                              capture_output=True, check=True, timeout=15).stdout
                        require(blob == item['content'].encode(), 'Independent test differs from Git candidate')
                independent = {phase: {'candidate': result['candidate'], 'counts': result['counts'],
                                       'container': result['container_id']} for phase, result in (('red', red), ('green', green))}
            merged = output('merge', tool='git.delivery.merge')
            check_merge(merged, publication)
            listing = subprocess.run(['git', '-C', str(repository), 'ls-tree', '-rlz', publication['source_revision']],
                                     capture_output=True, check=True, timeout=15).stdout
            sizes = []
            for entry in listing.split(b'\0'):
                if not entry:
                    continue
                header, path = entry.split(b'\t', 1)
                parts = PurePosixPath(path.decode()).parts
                require(not set(parts) & {'node_modules', '.venv', '__pycache__', '.npm', '.cache'},
                        'Runtime dependencies entered the source snapshot: ' + path.decode())
                size = header.split()[-1]
                if size != b'-':
                    sizes.append(int(size))
            report['instances'].append({'id': identity, 'owner': runtime['owner_id'],
                'steps': len(steps), 'human_confirmations': approvals, 'regressions': regressions,
                'independent_tests': independent,
                'source_tree': {'files': len(sizes), 'bytes': sum(sizes), 'largest_blob_bytes': max(sizes, default=0)},
                'pull_request': merged['pull_request']['url'], 'revision': merged['revision']})
        root_row = next(r for r in report['instances'] if r['id'] == root)
        mainline = directory / 'execution/worktrees/mainlines' / project
        head = subprocess.run(['git', '-C', str(mainline), 'rev-parse', 'HEAD'],
                              capture_output=True, text=True, check=True, timeout=15).stdout.strip()
        require(head == root_row['revision'], 'Mainline is not the accepted Feature revision')
        report['mainline'] = head
        regressions = [r for member in report['instances'] for r in member['regressions']]
        require(len({r['container'] for r in regressions}) == len(regressions) and
                len({r['operation'] for r in regressions}) == len(regressions),
                'Project regression reused an earlier execution receipt')
        controller_row = db.execute("SELECT data FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.root_instance_id')=?", (root,)).fetchone()
        require(controller_row is not None, 'Missing project manager')
        sessions.add(json.loads(controller_row[0])['session_id'])
        for session, status in db.execute('SELECT session_id,status FROM turns'):
            if session in sessions:
                require(status in ('completed', 'failed', 'cancelled'), 'Native turn is still active')
        for session, revoked in db.execute('SELECT session_id,writer_revoked FROM executions'):
            if session in sessions:
                require(revoked, 'Native worker cleanup has not completed')
        report['writer_cleanup_verified'] = True
    manager = client.rpc('query.orchestration', {'root_instance_id': root})
    require(manager['record']['data']['status'] == 'closed', 'Project manager is not closed')
    require(not any(d.get('current') is not False and d.get('data', {}).get('status') == 'pending'
                    for d in manager.get('decisions', [])), 'Unresolved current Human request')
    report['passed'] = True
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', required=True)
    parser.add_argument('--project', required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--data', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--unassisted', action='store_true',
                        help='Reject maintenance inputs, plan repairs and Human controls beyond ordinary confirmations')
    args = parser.parse_args()
    result = verify(Client(args.url, args.project), args.root, args.data, args.unassisted)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(args.report)
