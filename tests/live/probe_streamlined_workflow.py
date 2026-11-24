#!/usr/bin/env python3
"""Observe a Feature workflow from the released software development ZIP. Human decisions are never generated here."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time
from client import Client
from workflow_observer import read_workflow_tree, observe_human_request, check_human_request, workflow_settled

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--codex-auth-source', type=Path, required=True)
parser.add_argument('--github-repo', required=True)
parser.add_argument('--mainline', default='main')
parser.add_argument('--mode', choices=('manual', 'rust', 'agent'), default='agent')
parser.add_argument('--web-dir', type=Path, help='Serve release Web Core for browser review')
parser.add_argument('--domain-zip', type=Path, required=True, help='Software development release ZIP')
parser.add_argument('--title', default='Normalize, format and compose with concurrent DAG')
parser.add_argument('--report-directory', type=Path, required=True)
parser.add_argument('--requirement-file', type=Path, required=True)
parser.add_argument('--resume', action='store_true')
parser.add_argument('--url', help='Observe an already running Host using report-directory/data; leave it running on exit')
parser.add_argument('--timeout-seconds', type=int, default=1800, help='Bound observation of this same workflow instance')
parser.add_argument('--performance', action='store_true', help='Record native timing spans and observer query latency without changing workflow controls')
args = parser.parse_args()
assert 'test' in args.github_repo.lower(), 'use a dedicated test repository'
out = args.report_directory.resolve()
out.mkdir(parents=True, exist_ok=True)
assert args.resume == (out / 'context.json').exists()
private = out / 'devdesk-host'
replacement = out / ('devdesk-host.' + str(time.time_ns()))
shutil.copy2(args.binary.resolve(), replacement)
replacement.replace(private)
identity = hashlib.sha256(private.read_bytes()).hexdigest()
(out / ('binary-' + identity + '.json')).write_text(json.dumps({'sha256': identity}))
archive = out / 'binaries' / identity / 'devdesk-host'
archive.parent.mkdir(parents=True, exist_ok=True)
if not archive.exists():
    shutil.copy2(private, archive)
attempt = out / 'attempts' / str(time.time_ns())
attempt.mkdir(parents=True)
for filename in ['probe_streamlined_workflow.py', 'client.py', 'workflow_observer.py']:
    shutil.copy2(Path(__file__).parent / filename, attempt / filename)
(attempt / 'identity.json').write_text(json.dumps({'binary_sha256': identity, 'resume': args.resume, 'performance': args.performance, 'time': time.time()}))
domain_archive = args.domain_zip.read_bytes()
domain_digest = hashlib.sha256(domain_archive).hexdigest()
web = ['--web-dir', str(args.web_dir.resolve())] if args.web_dir else []
child = None if args.url else subprocess.Popen([str(private), '--data', str(out / 'data'), '--port', '0', *web],
                         stdout=(out / ('host-' + str(time.time_ns()) + '.log')).open('w'), stderr=subprocess.STDOUT, env={**os.environ, **({'DEVDESK_PERF_LOG': '1'} if args.performance else {})})
try:
    deadline = time.monotonic() + 180
    urls = [args.url] if args.url else []
    while not urls:
        logs = sorted(out.glob('host-*.log'))
        urls = [line for line in logs[-1].read_text().splitlines() if line.startswith('http://')]
        if urls:
            break
        assert child.poll() is None, 'Host stopped before listening'
        assert time.monotonic() < deadline, 'Host startup timeout'
        time.sleep(.1)
    client = Client(urls[0])
    auth = out / 'data/credentials/codex/auth.json'
    auth.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(args.codex_auth_source.resolve(), auth)
    auth.chmod(0o600)
    if args.resume:
        context = json.loads((out / 'context.json').read_text())
        assert context['domain_digest'] == domain_digest, 'Resume must retain the original domain digest'
        client.project = context['project']
        client.domain_epoch = context.get('domain_epoch')
        context['url'] = urls[0]
        assert context.get('mode', 'agent') == args.mode, 'Resume must retain the original driver mode'
        context.setdefault('resumptions', []).append({'binary_sha256': identity, 'time': time.time()})
    else:
        status = client.rpc('query.domain.active')
        assert status['active'] is None, 'new dataset must start in the empty domain'
        installed = client.rpc('domain.install', {'archive': base64.b64encode(domain_archive).decode()})
        assert installed['digest'] == domain_digest
        status = client.rpc('domain.activate', {'digest': domain_digest, 'epoch': status['epoch']})
        client.domain_epoch = status['epoch']
        defaults = status['defaults']
        project = client.rpc('domain.project.create', {
            'epoch': status['epoch'], 'command_id': 'software-domain-live-project',
            'title': 'Software domain acceptance',
            'connection': {'repository': args.github_repo, 'mainline': args.mainline},
        })['id']
        assert defaults['workflow_ref'].rsplit(':', 1)[0] == 'software-development.workflow-feature-development'
        assert defaults['task_workflow_ref'].rsplit(':', 1)[0] == 'software-development.workflow-task-development'
        assert defaults['standard_ref'].rsplit(':', 1)[0] == 'software-development.standard-engineering-delivery'
        client.project = project
        client.wait(lambda: client.rpc('query.workspace'), lambda v: v['repository_ready'], 300)
        client.timeout = 1250
        runtime = client.rpc('runtime.prepare', {'provider': 'codex'})
        client.timeout = 180
        discovery = client.rpc('query.project_test_configuration')
        (out / 'test-discovery.json').write_text(json.dumps(discovery, ensure_ascii=False, indent=2))
        current = client.rpc('query.workspace')['active_project']
        client.command('project.update', project, current['revision'], {'concurrency': 4})
        current = client.rpc('query.workspace')['active_project']
        configuration_fields = ['workflow_ref', 'standard_ref', 'role_agent_bindings',
                                'harness_permission_ref', 'sandbox_ref', 'sandbox_permission_ref', 'orchestration']
        configured = {key: defaults[key] for key in configuration_fields}
        configured['orchestration'] = {**configured['orchestration'], 'mode': args.mode}
        configuration = client.command('project.configuration.update', project, current['revision'], configured)
        test_configuration = configuration['data']['test_configuration']
        feature = client.command('feature.create', payload={'project_id': project, 'title': args.title,
            'initial_requirement': args.requirement_file.read_text(), 'start_workflow': False, 'configuration_revision_id': configuration['id']})['id']
        client.command('workflow.start', feature, client.feature(feature)['feature']['revision'])
        root = client.rpc('query.workflow_instance', {'owner_id': feature})['record']['id']
        context = {'url': urls[0], 'project': project, 'feature': feature, 'root': root, 'binary_sha256': identity,
                   'defaults': defaults, 'domain_epoch': client.domain_epoch, 'domain_digest': domain_digest, 'test_configuration': test_configuration, 'suite': 'streamlined', 'mainline': args.mainline, 'mode': args.mode, 'human_decisions_by_probe': 0}
    (out / 'context.json').write_text(json.dumps(context, ensure_ascii=False, indent=2))
    def manual_actions(tree):
        # This client only drives the public manual scheduling actions. Human
        # confirmations, requests for input and exceptional repairs are separate.
        for identity, instance in reversed(list(tree.items())):
            if instance['runtime']['status'] in ('completed', 'cancelled'):
                continue
            view = client.rpc('query.workflow_control', {'id': identity, 'view': 'summary', 'limit': 64})
            command = None
            payload = {}
            if 'complete' in view['allowed_actions']:
                command = 'workflow.complete'
            else:
                for row in view['steps']:
                    step = row['execution']
                    if row['actions']['dispatch']['allowed']:
                        command = 'workflow.step.dispatch'
                    elif step['status'] == 'awaiting_acceptance' and row['actions']['accept']['allowed']:
                        command = 'workflow.step.accept'
                    if command:
                        payload = {'step': step['step'], 'execution_id': step['id']}
                        if command == 'workflow.step.accept':
                            payload.update(input_digest=step['input_digest'], outputs=step['outputs'])
                        break
            if command:
                try:
                    result = client.command(command, identity, view['record']['revision'], payload)
                except RuntimeError as failure:
                    if not any(code in str(failure) for code in (': revision_conflict:', ': resource_conflict:')):
                        raise
                    continue
                with (out / 'manual-actions.jsonl').open('a') as actions:
                    actions.write(json.dumps({'time':time.time(), 'method':command, 'instance':identity,
                                              'payload':payload, 'result':result}) + '\n')

    seen = set()
    deadline = time.monotonic() + args.timeout_seconds
    while True:
        assert time.monotonic() < deadline, 'Observation deadline exceeded; original workflow remains available for inspection/resume'
        query_started = time.perf_counter()
        tree = read_workflow_tree(client, context['root'])
        tree_finished = time.perf_counter()
        manager = client.rpc('query.orchestration', {'root_instance_id': context['root']})
        if args.performance:
            with (out / 'observer-query-timings.jsonl').open('a') as measurements:
                measurements.write(json.dumps({'time': time.time(), 'instances': len(tree), 'tree_ms': (tree_finished-query_started)*1000, 'manager_ms': (time.perf_counter()-tree_finished)*1000}) + '\n')
        (out / 'latest-tree.json').write_text(json.dumps(tree, ensure_ascii=False, indent=2))
        (out / 'latest-manager.json').write_text(json.dumps(manager, ensure_ascii=False, indent=2))
        for request in manager.get('decisions', []):
            if request['data']['status'] == 'pending' and request['id'] not in seen:
                manager_url = urls[0] + '/?tab=execution&detail=manager&project_id=' + context['project'] + '&feature_id=' + context['feature']
                observation = observe_human_request(out, request, manager_url,
                    lambda saved: check_human_request(client, tree, saved))
                if not observation['diagnostic_passed']:
                    print('Human review diagnostic:', observation['diagnostic'], flush=True)
                print('Human review pending:', request['id'], observation['path'], flush=True)
                seen.add(request['id'])
        statuses = [(node['record']['id'], step['step'], step['status'], step['id'], step.get('round')) for node in tree.values()
                    for step in node['record']['data']['steps'] if step['status'] not in ['completed', 'queued', 'skipped']]
        status_digest = hashlib.sha256(json.dumps(statuses).encode()).hexdigest()
        if status_digest not in seen:
            with (out / 'timeline.jsonl').open('a') as history:
                history.write(json.dumps({'time': time.time(), 'statuses': statuses, 'instances': [{'id': key, 'status': value['runtime']['status'], 'owner_id': value['runtime']['owner_id']} for key, value in tree.items()]}, ensure_ascii=False) + '\n')
            print(json.dumps(statuses, ensure_ascii=False), flush=True)
            seen.add(status_digest)
        complete = (workflow_settled(tree, manager, context['root']) if args.mode == 'agent'
                    else all(view['runtime']['status'] == 'completed' for view in tree.values()))
        if complete:
            (out / 'completed.json').write_text(json.dumps({'time': time.time(), 'root': context['root'],
                'mode': args.mode, 'controller_status': 'closed' if args.mode == 'agent' else None,
                'human_decisions_by_probe': 0}))
            print('Real default workflow completed; evidence audit still required.', flush=True)
            break
        if args.mode == 'manual':
            manual_actions(tree)
        time.sleep(3)
finally:
    if child is not None:
        child.send_signal(signal.SIGINT)
        try:
            child.wait(timeout=60)
        except subprocess.TimeoutExpired:
            child.terminate()
            child.wait(timeout=15)
        private_auth_files = list((out / 'data/credentials').rglob('auth.json'))
        private_auth_files.extend((out / 'data/projects').glob('*/execution/sessions/*/state/codex/auth.json'))
        for private_auth in private_auth_files:
            private_auth.unlink(missing_ok=True)
    (attempt / 'observation-ended.json').write_text(json.dumps({'time_ms': round(time.time() * 1000)}))
    (out / 'observation-ended.json').write_text((attempt / 'observation-ended.json').read_text())
