#!/usr/bin/env python3
"""Private HTTP time-limit contract and optional <=60s idle CPU diagnostic.

Fresh datasets only. No model, successful Human signature, Git or Feature run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'live'))
from client import Client


def cpu(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().split(') ', 1)[1].split()
    status = Path(f'/proc/{pid}/status').read_text().splitlines()
    return {'monotonic': time.monotonic(),
            'cpu_seconds': (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK'),
            'rss_kib': int(next(line for line in status if line.startswith('VmRSS:')).split()[1])}


def gate(client, human_seconds, wall_seconds):
    client.project = None
    project = client.command('project.create', payload={'title': 'Time-limit HTTP fixture'})['id']
    client.project = project
    draft = client.command('workflow.create', payload={'name': 'Explicit time-limited Control',
        'document': {'schema_version': 6, 'stages': [{'key': 'gate', 'name': 'Gate'}],
                     'steps': [{'key': 'confirm', 'name': 'Confirm', 'stage': 'gate',
                                'executor': {'type': 'control'}, 'confirmation': 'human',
                                'budget': {'max_turns': 8, 'timeout_seconds': 1, 'retry_limit': 1,
                                           'time_limits': {'version': 1, 'wall_seconds': wall_seconds,
                                                           'human_wait_seconds': human_seconds}}}]}})
    workflow = client.command('workflow.publish', draft['id'], draft['revision'])
    root = client.command('workflow.start', project, 1,
                          {'workflow_ref': workflow['id'], 'role_agent_bindings': {}})['id']
    view = client.rpc('query.workflow_control', {'id': root})
    client.command('workflow.step.dispatch', root, view['record']['revision'],
                   {'execution_id': view['runtime']['steps'][0]['id'], 'step': 'confirm'})
    return project, root


def run(args):
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    binary = output / 'devdesk-host'
    shutil.copy2(args.binary, binary)
    shutil.copy2(__file__, output / Path(__file__).name)
    summary = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'formal_capacity': False, 'idle_seconds': args.idle_seconds,
               'scope': 'One private production Host, fresh Control gates; no external writer or model'}
    error = None
    with (output / 'host.log').open('w') as log:
        environment = dict(os.environ)
        environment.pop('DEVDESK_ACCESS_TOKEN', None)
        environment.pop('DEVDESK_PERF_LOG', None)
        child = subprocess.Popen([str(binary), '--data', str(output / 'dataset'), '--port', '0'],
                                 env=environment, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                origins = [line for line in (output / 'host.log').read_text().splitlines()
                           if line.startswith('http://')]
                if origins:
                    break
                if child.poll() is not None:
                    raise RuntimeError('Private Host exited before startup')
                time.sleep(.05)
            else:
                raise TimeoutError('Private Host startup')
            client = Client(origins[0])
            project, root = gate(client, 2, 15)
            initial = client.rpc('query.workflow_control', {'id': root})
            execution = initial['runtime']['steps'][0]
            assert execution['time_budget']['origin_execution_id'] == execution['id']
            assert execution['time_budget']['next_deadline_at'] is not None
            ended = client.wait(lambda: client.rpc('query.workflow_control', {'id': root}),
                lambda view: view['runtime']['steps'][0].get('failure', {}).get('code') == 'human_wait_timeout', 10)
            try:
                client.command('workflow.step.confirm', root, ended['record']['revision'],
                    {'execution_id': execution['id'], 'step': 'confirm', 'input_digest': execution['input_digest'],
                     'outputs': execution['outputs'], 'reason': 'Expired fixture must reject',
                     'control_version': ended['runtime']['control_version'],
                     'plan_revision_id': ended['runtime']['active_plan_revision_id'],
                     'expected_step_revision': ended['steps'][0]['step_revision']})
            except RuntimeError as rejected:
                # A coordinator may finish the stop/header commit after the read.
                # Preserve the exact rejected command; never rebase or retry it.
                assert any(code in str(rejected) for code in ['human_wait_timeout', 'revision_conflict']), rejected
                summary['late_signature_rejection'] = str(rejected)
            else:
                raise AssertionError('Expired Human signature was accepted')
            summary['deadline_root'] = {'project_id': project, 'root_id': root, 'before': initial, 'after': ended}
            if args.idle_seconds:
                project, root = gate(client, args.idle_seconds + 30, args.idle_seconds + 60)
                client.wait(lambda: client.rpc('query.workflow_control', {'id': root}),
                            lambda view: view['runtime']['status'] == 'running', 10)
                time.sleep(1)  # Settle the initial header/rule-hint projections.
                before = client.rpc('query.workflow_control', {'id': root})
                samples = [cpu(child.pid)]
                deadline = samples[0]['monotonic'] + args.idle_seconds
                with (output / 'idle-samples.jsonl').open('w') as stream:
                    while time.monotonic() < deadline:
                        time.sleep(min(1, max(0, deadline - time.monotonic())))
                        samples.append(cpu(child.pid))
                        stream.write(json.dumps(samples[-1]) + '\n')
                        stream.flush()
                after = client.rpc('query.workflow_control', {'id': root})
                elapsed = samples[-1]['monotonic'] - samples[0]['monotonic']
                summary['idle'] = {'elapsed_seconds': elapsed,
                    'host_one_core_cpu_percent': 100 * (samples[-1]['cpu_seconds'] - samples[0]['cpu_seconds']) / elapsed,
                    'peak_rss_kib': max(s['rss_kib'] for s in samples), 'project_id': project,
                    'root_id': root, 'before': before, 'after': after,
                    'boundary': 'Host process only; no UI/SSE/real worker; memory is RSS, not process-tree sum'}
                summary['idle']['two_percent_reference_met'] = summary['idle']['host_one_core_cpu_percent'] <= 2
                assert after['runtime']['steps'][0]['status'] == 'waiting_confirmation'
                assert before['steps'][0]['step_revision'] == after['steps'][0]['step_revision']
                assert before['runtime']['steps'][0]['time_budget'] == after['runtime']['steps'][0]['time_budget']
        except Exception as failure:
            error = failure
            summary['error'] = repr(failure)
        finally:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                child.wait(timeout=15)
            summary['host_exit'] = child.returncode
    summary['databases'] = []
    for path in sorted((output / 'dataset').rglob('*.sqlite3')):
        with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as connection:
            item = {'database': str(path.relative_to(output)),
                    'version': connection.execute('PRAGMA user_version').fetchone()[0],
                    'quick_check': connection.execute('PRAGMA quick_check').fetchall(),
                    'foreign_keys': connection.execute('PRAGMA foreign_key_check').fetchall()}
            tables = {r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            if 'executions' in tables:
                item['unrevoked_writers'] = connection.execute('SELECT count(*) FROM executions WHERE writer_revoked=0').fetchone()[0]
                item['human_signatures'] = connection.execute("SELECT count(*) FROM governance_records WHERE type='workflow_confirmation'").fetchone()[0]
                assert item['unrevoked_writers'] == 0 and item['human_signatures'] == 0
            summary['databases'].append(item)
            assert item['quick_check'] == [('ok',)] and not item['foreign_keys']
    (output / 'summary.json').write_text(json.dumps(summary, indent=2))
    if error:
        raise error
    assert summary['host_exit'] == 0, summary
    print(json.dumps({'contract_result': 'PASS', 'idle': {k: v for k, v in summary.get('idle', {}).items()
          if k not in ['before', 'after']}, 'binary_sha256': summary['binary_sha256']}, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--idle-seconds', type=int, default=0)
    args = parser.parse_args()
    if not 0 <= args.idle_seconds <= 60:
        parser.error('idle-seconds must be 0..60')
    run(args)
