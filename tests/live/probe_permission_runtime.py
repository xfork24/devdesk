#!/usr/bin/env python3
"""Exercise a real Codex v2 binding switch in a disposable authenticated dataset.

Only the explicitly supplied auth file is copied; its contents are never reported.
The original dataset, projects and configuration are not opened or modified.
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
import time
import uuid
import urllib.request

from client import Client


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--auth-file', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--model', default='gpt-6.1-sol')
    args = parser.parse_args()
    report = {'passed': False, 'provider': 'codex', 'model': args.model,
              'platform': platform.system(), 'architecture': platform.machine()}
    hits = []

    class Canary(BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            body = b'PERMISSION_NETWORK_OK'
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
        with tempfile.TemporaryDirectory(prefix='devdesk-permission-real-') as tmp:
            root = Path(tmp)
            root.chmod(0o700)
            dataset = root / 'dataset'
            # Keep child diagnostics private and out of the report.
            with (root / 'host.stderr').open('w') as stderr:
                child = subprocess.Popen([str(args.binary.resolve()), '--data', str(dataset), '--port', '0'],
                                         stdout=subprocess.PIPE, stderr=stderr, text=True)
                child.stdout.readline()
                url = child.stdout.readline().strip()
                assert url.startswith('http://127.0.0.1:'), 'Host did not start'
                c = Client(url)
                auth = dataset / 'credentials/codex'
                shutil.copyfile(args.auth_file, auth / 'auth.json')
                (auth / 'auth.json').chmod(0o600)
                project = c.command('project.create', payload={'title': 'Isolated v2 permission probe'})['id']
                c.project = project
                resources = c.rpc('query.permission_resources')
                harness = next(t['target'] for t in resources['targets']
                               if t['target']['revision_id'] == 'builtin-harness-codex-managed-v1')
                sandbox = next(t['target'] for t in resources['targets']
                               if t['target']['revision_id'] == resources['default_sandbox_ref'])

                def bind(profile, expected):
                    changes = [{'target': harness, 'permission_revision_id': profile,
                                'expected_revision': expected}]
                    if expected == 0:
                        changes.append({'target': sandbox, 'permission_revision_id':
                                        'permission-sandbox-full-access-v2-r1', 'expected_revision': 0})
                    return c.command('permission.bindings.apply', payload={'bindings': changes})

                bind('permission-harness-full-access-v2-r1', 0)
                model = c.command('model.create', payload={'name': 'Controlled permission probe model', 'data': {
                    'provider': 'openai', 'model': args.model,
                    'parameters': {'reasoning_effort': 'low'}, 'capabilities': []}})['id']
                tool = c.command('tool.create', payload={'name': 'Controlled native tools', 'data': {
                    'qualified_name': 'harness.native', 'implementation': 'harness.native',
                    'input_schema': {}, 'output_schema': {}, 'capabilities': []}})['id']
                agent = c.command('agent.create', payload={'name': 'Follow target permission probe', 'data': {
                    'role_ref': 'builtin-role-analyst-v1', 'instructions': 'Run only the supplied controlled shell probe. '
                    'Do not inspect credentials or runtime state, delegate, or propose changes.',
                    'model_ref': model, 'harness_ref': harness['revision_id'], 'skills': [], 'tools': [tool],
                    'permissions': [], 'approval': 'never', 'permission_selection': {
                        'sandbox_ref': sandbox['revision_id'], 'harness': {'mode': 'follow_target'},
                        'sandbox': {'mode': 'follow_target'}}}})['id']
                c.rpc('runtime.prepare_harness', {'id': harness['revision_id']})
                instance = c.command('agent.instance.create', payload={'name': 'Isolated permission probe',
                    'agent_revision_id': agent, 'scope': {'kind': 'project', 'project_id': project}})['id']
                state = c.rpc('query.workspace')['active_project']
                original = c.command('session.open', project, state['revision'], {
                    'agent_instance_id': instance, 'work_type': 'discussion', 'force_new': True})['id']
                host = 'host.docker.internal'
                if platform.system() == 'Linux':
                    host = subprocess.check_output(['docker', 'network', 'inspect', 'bridge', '--format',
                        '{{(index .IPAM.Config 0).Gateway}}'], text=True, timeout=15).strip()

                def turn(sid, restricted):
                    nonce = uuid.uuid4().hex
                    endpoint = f'http://{host}:{server.server_port}/{nonce}'
                    if restricted:
                        # Use /tmp in both rounds to prove the Harness restriction,
                        # independently of the unchanged read-only /work mount.
                        js = "const fs=require('fs');let denied=false;try{fs.writeFileSync('/tmp/permission-" + nonce + "','bad')}catch{denied=true}fetch('" + endpoint + "',{signal:AbortSignal.timeout(3000)}).then(()=>process.exit(3)).catch(()=>{if(!denied)process.exit(4);console.log('PERMISSION_DENIED_OK')})"
                    else:
                        js = "const fs=require('fs');fs.writeFileSync('/tmp/permission-" + nonce + "','ok');fetch('" + endpoint + "').then(r=>r.text()).then(console.log).catch(()=>process.exit(1))"
                    prompt = 'Execute this exact shell command once, then reply PERMISSION_COMPLETE: node -e ' + shlex.quote(js)
                    current = c.rpc('query.session', {'id': sid})
                    command = {'command_id': str(uuid.uuid4()), 'target': sid,
                               'expected_revision': current['session']['revision'],
                               'payload': {'client_message_id': str(uuid.uuid4()), 'input': prompt}}
                    sent = c.rpc('conversation.send', command)
                    effective = sent.get('effective_session_id', sid)
                    state = c.wait(lambda: c.rpc('query.session', {'id': effective, 'limit': 100}),
                        lambda s: next(t for t in s['turns'] if t['id'] == sent['id'])['status'] in
                        ['completed', 'failed', 'cancelled', 'needs_reconciliation'], 300)
                    status = next(t for t in state['turns'] if t['id'] == sent['id'])['status']
                    assert status == 'completed', 'Real model Turn ended with ' + status
                    assert c.rpc('conversation.send', command) == sent, 'Original send replay differs'
                    outputs = [json.loads(e['content']) for e in state['events']
                               if e['turn_id'] == sent['id'] and e['kind'] == 'tool'
                               and (str(e.get('provider_item', '')).startswith('tool/result:')
                                    or e['deduplication_key'].endswith(':complete'))]
                    marker = 'PERMISSION_DENIED_OK' if restricted else 'PERMISSION_NETWORK_OK'
                    assert any(o.get('exitCode') == 0 and marker in o.get('aggregatedOutput', '')
                               for o in outputs), 'No successful native tool evidence'
                    assert any(nonce in hit for hit in hits) != restricted, 'HTTP canary mismatch'
                    plan = state['session']['runtime_binding']['permissions']
                    assert plan['binding_lock']['harness']['profile']['policy']['approval'] == 'never'
                    row = {'session_id': effective, 'turn_id': sent['id'], 'model_transport_succeeded': True,
                           'tool_evidence': marker, 'http_receipt': not restricted,
                           'permissions': plan, 'replay_exact': True}
                    print(marker, flush=True)
                    return effective, state, row

                sid, before, row = turn(original, False)
                report['allowed'] = row
                profile = c.command('permission.profile.create', payload={
                    'identity': 'runtime-probe-restricted', 'name': 'Real model read-only tools offline',
                    'target_kind': 'harness', 'policy': {'filesystem': {'mode': 'read_only'},
                    'network': {'mode': 'disabled'}, 'tools': {'mode': 'all_available'}, 'approval': 'never'}})
                bind(profile['id'], 1)
                successor, after, row = turn(sid, True)
                report['restricted'] = row
                assert successor != original
                assert after['session']['predecessor'] == original
                assert before['session']['external_session'] != after['session']['external_session']
                old = c.rpc('query.session', {'id': original, 'limit': 100})
                assert old['session']['runtime_binding'] == before['session']['runtime_binding']
                assert [t['id'] for t in old['turns']] == [t['id'] for t in before['turns']]
                report.update(passed=True, successor_adopted=True, history_preserved=True,
                              native_session_changed=True, host_restarted=False)
                req = urllib.request.Request(url + '/shutdown', data=b'{}',
                                             headers={'Content-Type': 'application/json'})
                c.http.open(req, timeout=10).close()
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
    print(json.dumps({k: v for k, v in report.items() if k not in ['allowed', 'restricted']}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
