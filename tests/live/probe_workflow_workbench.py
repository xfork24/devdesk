#!/usr/bin/env python3
"""Run/observe the current configured workflow in a dedicated GitHub test project.
Model calls and repository changes require explicit invocation. Human gates remain
pending unless --confirm is supplied; rejected completion conditions are never overridden.
"""
import argparse
import json
from pathlib import Path
import time
from client import Client

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--github-repo', required=True)
parser.add_argument('--provider', choices=['codex', 'dsh'], default='codex')
parser.add_argument('--workflow-ref')
parser.add_argument('--bindings', type=Path, help='JSON Role identity -> exact Agent revision')
parser.add_argument('--inputs', type=Path, help='JSON Workflow input values')
parser.add_argument('--supplement', help='Explicit text for a step requesting supplemental input')
parser.add_argument('--confirm', action='store_true', help='Confirm exact successful results in this test run')
parser.add_argument('--resume-project')
parser.add_argument('--resume-feature')
parser.add_argument('--timeout', type=int, default=2400)
parser.add_argument('--report', type=Path, required=True)
args = parser.parse_args()
assert 'test' in args.github_repo.lower(), 'use a dedicated test repository'
client = Client(args.url)
report = ['# Configured Workflow v5 native verification', '', 'Status: running', '']
try:
    if args.resume_project:
        client.open(args.resume_project)
        feature = args.resume_feature
        assert feature, '--resume-feature is required with --resume-project'
    else:
        project = client.command('project.open_github', payload={'repository': args.github_repo, 'mainline': 'main', 'title': 'Workflow v5 verification'})['id']
        client.project = project
        client.wait(lambda: client.rpc('query.workspace'), lambda view: view['repository_ready'], 300)
        defaults = client.rpc('query.configuration_defaults')
        workflow = args.workflow_ref or defaults['workflow_ref']
        bindings = json.loads(args.bindings.read_text()) if args.bindings else defaults['role_agent_bindings']
        # The test uses actual configured capabilities; it does not silently replace models or permissions.
        for agent_id in bindings.values():
            agent = client.rpc('query.agent_revision', {'id': agent_id})
            capability = next(r for r in client.rpc('query.configurations', {'kind': 'capability', 'include_history': True, 'limit': 100})['records'] if r['id'] == agent['data']['capability_ref'])
            harness = next(r for r in client.rpc('query.configurations', {'kind': 'harness', 'include_history': True, 'limit': 100})['records'] if r['id'] == capability['data']['harness_ref'])
            assert harness['data']['provider'] == args.provider, 'configure matching Agents or pass --bindings'
        current = client.rpc('query.workspace')['active_project']
        client.command('project.configuration.update', project, current['revision'], {'workflow_ref': workflow, 'role_agent_bindings': bindings})
        feature = client.command('feature.create', payload={'project_id': project, 'title': 'Workflow v5 verification', 'workflow_inputs': json.loads(args.inputs.read_text()) if args.inputs else {}})['id']
    defaults=client.rpc('query.configuration_defaults')
    initial=client.rpc('query.workflow_instance',{'owner_id':feature})
    manager=client.command('orchestration.open',initial['record']['id'],payload={'role_ref':defaults['orchestrator_role_ref'],'agent_ref':defaults['role_agent_bindings'][defaults['orchestrator_role_ref']],'permission_context_ref':defaults['permission_context_ref']})['id']
    manager_row=client.rpc('query.orchestration',{'id':manager})['record']
    client.command('orchestration.input',manager,manager_row['revision'],{'input':'按当前冻结流程组织派发、接纳及根/子实例完成。保留所有测试、独立性及人工门禁，不得绕过或伪造成功。'+(args.supplement or '')})
    report.extend(['Project: ' + client.project, 'Feature: ' + feature, ''])
    deadline = time.monotonic() + args.timeout
    supplied = set()
    while time.monotonic() < deadline:
        view = client.rpc('query.workspace', {'feature_id': feature})
        root = next(r for r in view['configurations'] if r['kind'] == 'workflow_instance' and r['data']['owner_id'] == feature and r['data']['parent_execution_id'] is None)
        instances = [r for r in view['configurations'] if r['kind'] == 'workflow_instance' and r['data']['root_instance_id'] == root['id']]
        if root['data']['status'] == 'completed':
            assert view['detail']['feature']['status'] == 'completed'
            report.append('Status: passed. Frozen Workflow and all actual child executions completed.')
            break
        for row in instances:
            for step in row['data']['steps']:
                status = step['status']
                if status in ['failed', 'needs_reconciliation', 'cancelled']:
                    raise RuntimeError(f'{step["path"]}: {status}: {step["reason"]}')
                if status == 'waiting_confirmation':
                    if not args.confirm:
                        raise RuntimeError(f'{step["path"]}: human confirmation required; review in the UI, then resume')
                    client.command('workflow.step.confirm', row['id'], row['revision'], {'step': step['step'], 'input_digest': step['input_digest'], 'outputs': step['outputs'], 'reason': 'Explicit dedicated test confirmation'})
                    report.append('- Confirmed exact input/output version: ' + step['id'])
                    # Fetch a new revision before confirming another step in the same instance.
                    break
                if status == 'waiting_input':
                    key = (step['id'], step['sequence'])
                    if not args.supplement or key in supplied:
                        raise RuntimeError(f'{step["path"]}: supplemental input required; review in the UI, then resume')
                    client.command('workflow.step.input', row['id'], row['revision'], {'step': step['step'], 'input': args.supplement})
                    supplied.add(key)
                    break
        time.sleep(1)
    else:
        raise TimeoutError('Workflow execution deadline')
except BaseException as error:
    report.extend(['', 'Status: incomplete', str(error)])
    raise
finally:
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text('\n'.join(report) + '\n')
