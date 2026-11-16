#!/usr/bin/env python3
"""Actual Docker/Codex manager: SIGTERM, writer revocation, no input replay, successor recovery.

The sole workflow node is a technical control node in a new dataset. No business
Feature or GitHub repository is opened. Authentication is only for this explicit real-model run.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import time
from client import Client

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--codex-auth-source', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True, help='Must not exist')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir()
binary = out / 'devdesk-host'
shutil.copy2(args.binary.resolve(), binary)
process = None


def boot(attempt):
    global process
    log = out / f'host-{attempt}.log'
    with log.open('w') as stream:
        process = subprocess.Popen([str(binary), '--data', str(out / 'data'), '--port', '0'],
                                   stdout=stream, stderr=subprocess.STDOUT,
                                   env={**os.environ, 'DEVDESK_PERF_LOG': '1'})
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        urls = [s for s in log.read_text().splitlines() if s.startswith('http://')]
        if urls:
            return Client(urls[0])
        assert process.poll() is None, log.read_text()
        time.sleep(.1)
    raise TimeoutError('Host startup')


def controller_command(c, method, controller, payload=None):
    row = c.rpc('query.orchestration', {'id': controller})['record']
    return c.command(method, controller, row['revision'], {'epoch': row['data']['epoch'], **(payload or {})})


try:
    c = boot('before')
    auth = out / 'data/credentials/codex/auth.json'
    auth.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(args.codex_auth_source.resolve(), auth)
    auth.chmod(0o600)
    c.rpc('runtime.prepare', {'provider': 'codex'})
    defaults = c.rpc('query.configuration_defaults')
    project = c.command('project.create', payload={'title': 'Dedicated native shutdown recovery'})['id']
    c.project = project
    draft = c.command('workflow.create', payload={'name': 'Technical control only', 'document': {
        'schema_version': 6, 'stages': [{'key': 'check', 'name': 'Technical check'}],
        'steps': [{'key': 'control', 'name': 'Technical control', 'stage': 'check', 'executor': {'type': 'control'}}]}})
    workflow = c.command('workflow.publish', draft['id'], draft['revision'])
    p = c.rpc('query.workspace')['active_project']
    c.command('project.configuration.update', project, p['revision'], {'workflow_ref': workflow['id'],
              'role_agent_bindings': defaults['role_agent_bindings'], 'harness_permission_ref': defaults['harness_permission_ref']})
    p = c.rpc('query.workspace')['active_project']
    run = c.command('workflow.start', project, p['revision'], {'workflow_ref': workflow['id']})['id']
    manager = c.command('orchestration.open', run, payload={'role_ref': defaults['orchestrator_role_ref'],
              'agent_ref': defaults['role_agent_bindings'][defaults['orchestrator_role_ref']],
              'harness_permission_ref': defaults['harness_permission_ref']})['id']
    controller_command(c, 'orchestration.input', manager, {'input': '这是平台停机恢复的技术验收。当前轮次只实际调用一次 workflow.inspect，读取计划后结束当前 Turn。不要派发、接纳或完成任何节点，不修改计划。'})
    deadline = time.monotonic() + 240
    while time.monotonic() < deadline:
        if '"phase":"request_observed"' in (out / 'host-before.log').read_text():
            break
        assert process.poll() is None
        time.sleep(.01)
    else:
        raise TimeoutError('Actual manager native request')
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=30) == 0
    database = out / 'data/projects' / project / 'engineering.sqlite3'
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        turns_before = db.execute('SELECT id,session_id,status,consumed FROM turns').fetchall()
        assert turns_before and any(row[2] == 'needs_reconciliation' for row in turns_before), turns_before
        assert db.execute('SELECT count(*) FROM executions WHERE writer_revoked=0').fetchone()[0] == 0
    lifecycle = [json.loads(s) for s in (out / 'host-before.log').read_text().splitlines() if s.startswith('{')]
    assert any(e.get('event') == 'runtime_stopped' for e in lifecycle)
    c = boot('after')
    c.project = project
    time.sleep(1)
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        assert db.execute('SELECT id,session_id,status,consumed FROM turns').fetchall() == turns_before, 'Host restart replayed/changed original input'
        assert db.execute('SELECT count(*) FROM executions WHERE writer_revoked=0').fetchone()[0] == 0
    before = c.rpc('query.orchestration', {'id': manager})
    old_session = before['record']['data']['session_id']
    # Unknown input consumption stays unknown after process cleanup. Explicitly
    # cancel that stopped Turn before authorizing a successor; do not relabel it.
    controller_command(c, 'orchestration.takeover', manager)
    c.wait(lambda: c.rpc('query.session', {'id': old_session}),
           lambda v: all(t['status'] not in ('queued','dispatching','running','waiting_interaction','needs_reconciliation') for t in v['turns']) and all(e['writer_revoked'] for e in v['executions']), 30)
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        turns_stopped = db.execute('SELECT id,session_id,status,consumed FROM turns').fetchall()
    assert [(r[0],r[1],r[3]) for r in turns_stopped] == [(r[0],r[1],r[3]) for r in turns_before]
    controller_command(c, 'orchestration.resume', manager)
    controller_command(c, 'orchestration.input', manager, {'input': '现在继续这个独立的技术验收工作流，按冻结计划自主完成唯一节点和根实例，不修改计划。此消息是新的继续指令，不是重发中断轮次的输入。'})
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        view = c.rpc('query.workflow_instance', {'id': run})
        if view['runtime']['status'] == 'completed':
            break
        time.sleep(.5)
    else:
        raise TimeoutError('Successor manager completion')
    after = c.rpc('query.orchestration', {'id': manager})
    assert after['record']['data']['session_id'] != old_session
    new_session=after['record']['data']['session_id']
    c.wait(lambda: c.rpc('query.session', {'id': new_session}),
           lambda v: all(t['status'] not in ('queued','dispatching','running','waiting_interaction','needs_reconciliation') for t in v['turns']) and all(e['writer_revoked'] for e in v['executions']), 120)
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
        assert db.execute('SELECT id,session_id,status,consumed FROM turns WHERE session_id=?', (old_session,)).fetchall() == turns_stopped
    (out / 'proof.json').write_text(json.dumps({'workflow': run, 'manager': manager, 'old_turns': turns_before,
        'old_session': old_session, 'new_session': after['record']['data']['session_id'],
        'actual_sigterm_exit': 0, 'original_writers_revoked': True, 'restart_replayed_input': False,
        'successor_completed_technical_workflow': True}, indent=2) + '\n')
    print('PASS actual Codex/Host SIGTERM, writer stop proof, no replay, successor workflow completion')
finally:
    if process and process.poll() is None:
        process.send_signal(signal.SIGINT)
        process.wait(timeout=30)
    shutil.rmtree(out / 'data/credentials', ignore_errors=True)
    for private_auth in (out / 'data').rglob('auth.json'):
        private_auth.unlink(missing_ok=True)
