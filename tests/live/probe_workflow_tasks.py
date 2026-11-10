#!/usr/bin/env python3
"""Real Codex task instructions, Role/Skill/Spec provenance and controlled tool receipts.

Runs two Workflow revisions in a disposable dataset. Only the explicitly provided
Codex auth file is copied; original projects/configuration are never opened.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import tempfile
import threading
import urllib.request
import uuid

from client import Client


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--auth-file', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--model', default='gpt-6.1-sol')
    args = parser.parse_args()
    hits = []
    report = {'passed': False, 'model': args.model, 'provider': 'codex',
              'platform': platform.system(), 'architecture': platform.machine(), 'turns': []}

    class Canary(BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            body = b'WORKFLOW_TASK_TOOL_OK'
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *unused):
            pass

    server = ThreadingHTTPServer(('0.0.0.0', 0), Canary)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    child = None
    try:
        with tempfile.TemporaryDirectory(prefix='devdesk-task-model-') as temporary:
            root = Path(temporary)
            root.chmod(0o700)
            dataset = root / 'data'
            with (root / 'private-host.stderr').open('w') as stderr:
                child = subprocess.Popen([str(args.binary.resolve()), '--data', str(dataset), '--port', '0'],
                                         stdout=subprocess.PIPE, stderr=stderr, text=True)
                child.stdout.readline()
                url = child.stdout.readline().strip()
                assert url.startswith('http://127.0.0.1:'), 'Host did not start'
                c = Client(url)
                auth = dataset / 'credentials/codex/auth.json'
                shutil.copyfile(args.auth_file, auth)
                auth.chmod(0o600)
                project = c.command('project.create', payload={'title': 'Isolated Workflow task probe'})['id']
                c.project = project
                role_marker = 'ROLE_CANARY_' + uuid.uuid4().hex
                skill_marker = 'SKILL_CANARY_' + uuid.uuid4().hex
                spec_marker = 'SPEC_CANARY_' + uuid.uuid4().hex
                role = c.command('role.create', payload={'name': 'Controlled task executor', 'data': {
                    'execution_role': 'analyst', 'visibility': {'kind': 'global'},
                    'instructions': '# ' + role_marker + '\n\nExecute the frozen controlled task literally. Use only the requested tool. Do not delegate or inspect credentials/runtime state.'}})
                skill = c.command('skill.create', payload={'name': 'Controlled task method', 'data': {
                    'body': '# ' + skill_marker + '\n\nUse the exact supplied shell command once, capture actual output, and report only observed results.', 'rules': []}})
                spec = c.command('spec.create', payload={'name': 'Controlled task criteria', 'data': {
                    'body': '# ' + spec_marker + '\n\nThe requested scratch write and HTTP receipt must succeed. Return a valid requirement ProposalContent; do not adopt it.',
                    'applicable_actions': [{'kind': 'agent.interact', 'phase': 'requirement'}], 'rules': []}})
                model = c.command('model.create', payload={'name': 'Controlled task model', 'data': {
                    'provider': 'openai', 'model': args.model, 'parameters': {'reasoning_effort': 'low'}, 'capabilities': []}})
                cap = c.command('capability.create', payload={'name': 'Controlled task capability', 'data': {
                    'model_ref': model['id'], 'harness_ref': 'builtin-harness-codex-managed-v1',
                    'skills': [skill['id']], 'tools': ['builtin-tool-codex-native-v1']}})
                agent = c.command('agent.create', payload={'name': 'Controlled task Agent', 'data': {
                    'role_ref': role['id'], 'capability_ref': cap['id'], 'permission_selection': {'sandbox_ref': 'builtin-permission-sandbox-docker-v1', 'harness': {'mode': 'follow_target'}, 'sandbox': {'mode': 'follow_target'}}}})
                c.rpc('runtime.prepare_harness', {'id': 'builtin-harness-codex-managed-v1'})
                defaults = c.rpc('query.configuration_defaults')
                original = c.rpc('query.workflow', {'id': defaults['workflow_ref']})
                assert original['data']['schema_version'] == 3
                base = original['data']
                host = 'host.docker.internal'
                if platform.system() == 'Linux':
                    host = subprocess.check_output(['docker', 'network', 'inspect', 'bridge', '--format',
                        '{{(index .IPAM.Config 0).Gateway}}'], text=True, timeout=15).strip()
                previous = None
                for label in ['A', 'B']:
                    nonce = uuid.uuid4().hex
                    marker = 'TASK_CANARY_' + label + '_' + nonce
                    endpoint = f'http://{host}:{server.server_port}/{nonce}'
                    js = "const fs=require('fs');fs.writeFileSync('/tmp/devdesk-task-" + nonce + "','" + marker + "');fetch('" + endpoint + "').then(r=>r.text()).then(t=>console.log('" + marker + " '+t)).catch(()=>process.exit(1))"
                    task_text = '# ' + marker + '\n\nExecute this exact native shell command once:\n```sh\nnode -e ' + shlex.quote(js) + '\n```\n\nThen return one valid requirement ProposalContent JSON matching the supplied schema. This is a controlled tool probe; do not inspect credentials, delegate or adopt a proposal.'
                    doc = json.loads(json.dumps(base))
                    # The automatic workflow stops at a system checkpoint; only the
                    # explicitly opened Step Session below makes model requests.
                    doc['steps'][0]['advance_mode'] = 'human_confirmation'
                    step = next(s for s in doc['steps'] if s['key'] == 'requirement_discussion')
                    step.update(role_ref=role['definition_id'], spec_refs=[spec['id']], task_instructions=task_text)
                    step['interaction']['required_topics'] = ['Actual controlled tool output']
                    draft = c.command('workflow.create', payload={'name': 'Controlled task ' + label, 'document': doc})
                    workflow = c.command('workflow.publish', draft['id'], draft['revision'])
                    bindings = {**defaults['role_agent_bindings'], role['definition_id']: agent['id']}
                    p = c.rpc('query.workspace')['active_project']
                    c.command('project.configuration.update', project, p['revision'], {
                        'workflow_ref': workflow['id'], 'role_agent_bindings': bindings})
                    feature = c.command('feature.create', payload={'project_id': project, 'title': 'Task revision ' + label})['id']
                    state = c.rpc('query.workspace', {'feature_id': feature})
                    instance = next(r for r in state['configurations'] if r['kind'] == 'workflow_instance' and r['definition_id'] == feature)
                    step_instance = next(s for s in instance['data']['steps'] if s['key'] == 'requirement_discussion')['id']
                    current_feature = next(f for f in state['features'] if f['id'] == feature)
                    opened = c.command('session.open', feature, current_feature['revision'], {
                        'agent': agent['id'], 'workflow_step_instance_id': step_instance,
                        'work_type': 'discussion', 'force_new': True})
                    sid = opened['id']
                    current = c.rpc('query.session', {'id': sid})
                    command = {'command_id': str(uuid.uuid4()), 'target': sid,
                               'expected_revision': current['session']['revision'],
                               'payload': {'client_message_id': str(uuid.uuid4()), 'input': 'Execute the configured frozen Step task.'}}
                    sent = c.rpc('conversation.send', command)
                    result = c.wait(lambda: c.rpc('query.session', {'id': sid, 'limit': 100}),
                        lambda s: next(t for t in s['turns'] if t['id'] == sent['id'])['status'] in
                        ['completed', 'failed', 'cancelled', 'needs_reconciliation'], 300)
                    status = next(t for t in result['turns'] if t['id'] == sent['id'])['status']
                    assert status == 'completed', 'Real task Turn ended with ' + status
                    task = c.rpc('query.execution_task', {'turn_id': sent['id']})['task']
                    assert task['content']['task']['instructions'] == task_text
                    assert role_marker in task['base']['agent']['body']
                    assert task['base']['agent']['body'].count(role_marker) == 1
                    assert skill_marker in task['base']['skills'][0]['body']
                    assert spec_marker in task['content']['specifications'][0]['body']
                    assert task['definition']['workflow_revision_id'] == workflow['id']
                    assert any(nonce in hit for hit in hits), 'Missing actual HTTP receipt'
                    outputs = [json.loads(e['content']) for e in result['events'] if e['turn_id'] == sent['id']
                               and e['kind'] == 'tool' and (str(e.get('provider_item', '')).startswith('tool/result:')
                               or e['deduplication_key'].endswith(':complete'))]
                    assert any(o.get('exitCode') == 0 and marker in o.get('aggregatedOutput', '')
                               and 'WORKFLOW_TASK_TOOL_OK' in o.get('aggregatedOutput', '') for o in outputs), 'Missing native tool output'
                    assert c.rpc('conversation.send', command) == sent
                    if previous:
                        assert c.rpc('query.execution_task', {'turn_id': previous['turn_id']})['task'] == previous['task']
                        assert previous['task']['definition_digest'] != task['definition_digest']
                    row = {'label': label, 'session_id': sid, 'turn_id': sent['id'], 'task': task,
                           'native_tool_output': marker, 'http_receipt': True, 'command_replay_exact': True}
                    report['turns'].append(row)
                    previous = row
                    print('Task ' + label + ': native scratch write and HTTP receipt passed', flush=True)
                report.update(passed=True, role_skill_spec_delivered=True, old_task_preserved=True, host_restarted=False)
                c.http.open(urllib.request.Request(url + '/shutdown', data=b'{}',
                    headers={'Content-Type': 'application/json'}), timeout=10).close()
                assert child.wait(timeout=10) == 0
    except Exception as error:
        report.update(failure_type=type(error).__name__, failure=str(error))
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            child.wait(timeout=10)
        server.shutdown()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'turns'}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
