#!/usr/bin/env python3
"""Actual HTTP contract for the read-only Human request observer; no models/signatures."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'live'))
from client import Client
from workflow_observer import check_human_request, preserve_human_request, read_workflow_tree


def run(binary):
    with tempfile.TemporaryDirectory(prefix='devdesk-observer-http-') as temporary:
        output = Path(temporary)
        with (output / 'host.log').open('w') as log:
            environment = dict(os.environ)
            environment.pop('DEVDESK_ACCESS_TOKEN', None)
            child = subprocess.Popen([str(binary.resolve()), '--data', str(output / 'dataset'), '--port', '0'],
                                     env=environment, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    origins = [line for line in (output / 'host.log').read_text().splitlines()
                               if line.startswith('http://')]
                    if origins:
                        break
                    if child.poll() is not None:
                        raise RuntimeError('Private Host exited: ' + (output / 'host.log').read_text())
                    time.sleep(.05)
                else:
                    raise TimeoutError('Private Host startup')
                client = Client(origins[0])
                project = client.command('project.create', payload={'title': 'Observer HTTP contract'})['id']
                client.project = project
                draft = client.command('workflow.create', payload={'name': 'Single explicit Human gate',
                    'document': {'schema_version': 6, 'stages': [{'key': 'gate', 'name': 'Gate'}],
                    'steps': [{'key': 'confirm', 'name': 'Confirm', 'stage': 'gate',
                               'executor': {'type': 'control'}, 'confirmation': 'human'}]}})
                workflow = client.command('workflow.publish', draft['id'], draft['revision'])
                root = client.command('workflow.start', project, 1,
                    {'workflow_ref': workflow['id'], 'role_agent_bindings': {}})['id']
                view = client.rpc('query.workflow_instance', {'id': root, 'view': 'status'})
                execution = view['runtime']['steps'][0]
                client.command('workflow.step.dispatch', root, view['record']['revision'], {
                    'plan_revision_id': view['runtime']['active_plan_revision_id'],
                    'step': 'confirm', 'execution_id': execution['id']})
                view = client.rpc('query.workflow_instance', {'id': root, 'view': 'status'})
                execution = view['runtime']['steps'][0]
                pending = client.command('workflow.decision.request', root, view['record']['revision'], {
                    'plan_revision_id': view['runtime']['active_plan_revision_id'],
                    'operation': 'workflow.step.confirm', 'reason': 'Review exact Control result',
                    'arguments': {'instance_id': root, 'plan_revision_id': view['runtime']['active_plan_revision_id'],
                                  'execution_id': execution['id'], 'step': 'confirm',
                                  'input_digest': execution['input_digest'], 'outputs': execution['outputs']},
                    'interaction': {'title': 'Confirm', 'background': 'Actual control result',
                        'requested_action': 'Review', 'completed_work': 'Dispatched control',
                        'review_summary': 'No independent model required by this fixture', 'materials': [],
                        'evidence': [{'label': 'Execution', 'purpose': 'Inspect', 'instance_id': root,
                                      'execution_id': execution['id']}]}})
                request = next(r for r in client.rpc('query.orchestration', {'root_instance_id': root})['decisions']
                               if r['id'] == pending['id'])
                tree = read_workflow_tree(client, root)
                path = preserve_human_request(output, request, origins[0],
                    lambda saved: check_human_request(client, tree, saved))
                assert json.loads(path.read_text())['request'] == request
                assert client.rpc('query.workflow_instance', {'id': root, 'view': 'status'})['runtime']['steps'][0]['status'] == 'waiting_confirmation'
                print('PASS actual HTTP observer: bounded tree, exact execution/contract, empty materials, preserved original; no signature or model')
            finally:
                if child.poll() is None:
                    child.send_signal(signal.SIGINT)
                    child.wait(timeout=10)


if __name__ == '__main__':
    run(Path(sys.argv[1]))
