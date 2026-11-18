#!/usr/bin/env python3
"""Bounded offline workflow HTTP profile on an explicit compatible dataset copy.

No scheduler, model or business commands. This is a diagnostic, not a capacity
matrix or a replacement for native transport timing.
"""
import argparse
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import sqlite3
import subprocess
import time
import urllib.request
from configuration_loading import copy_dataset
from joint_frontends import stats


def measure_rpc(client, origin, project, method, params):
    """Every attempted read has an outcome, including transport/decoding errors."""
    request = urllib.request.Request(origin + '/rpc', json.dumps({
        'jsonrpc': '2.0', 'id': 1, 'method': method,
        'params': {'project_id': project, **params}}).encode(), {'Content-Type': 'application/json'})
    began = time.monotonic()
    row = {'method': method, 'bytes': 0, 'request_id': None, 'error': None, 'digest': None}
    try:
        with client.open(request, timeout=30) as response:
            body = response.read()
            # Keep content verification outside the measured HTTP interval.
            row['ms'] = (time.monotonic() - began) * 1000
            row.update(bytes=len(body), request_id=response.headers.get('x-devdesk-request-id'))
        value = json.loads(body)
        row['error'] = value.get('error')
        row['digest'] = hashlib.sha256(json.dumps(value.get('result'), sort_keys=True).encode()).hexdigest()
    except Exception as error:
        row['error'] = {'transport_or_decode': repr(error)}
    finally:
        row.setdefault('ms', (time.monotonic() - began) * 1000)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-binary', type=Path, required=True)
    parser.add_argument('--dataset', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=200)
    args = parser.parse_args()
    if not 1 <= args.samples <= 1000:
        parser.error('samples must be 1..1000')
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    dataset = output / 'dataset'
    copy_dataset(args.dataset.resolve(), dataset)
    binary = output / 'host-tests'
    shutil.copy2(args.test_binary, binary)
    shutil.copy2(__file__, output / Path(__file__).name)
    manifest = {str(p.relative_to(dataset)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in dataset.rglob('*.sqlite3')}
    (output / 'identity.json').write_text(json.dumps({
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'input_databases': manifest, 'scheduler': False, 'samples': args.samples,
        'formal_capacity': False, 'measurement': 'Warm HTTP, explicit offline backup; no display-cache eligible methods'}, indent=2))
    with (output / 'server.log').open('w') as log:
        server = subprocess.Popen([str(binary), 'loading_performance::configuration_load_server',
                                   '--ignored', '--exact', '--nocapture'],
                                  env={**os.environ, 'DEVDESK_PERF_DATASET': str(dataset),
                                       'DEVDESK_PERF_LOG': '1'}, stdout=subprocess.PIPE, stderr=log)
        try:
            pending = ''
            origin = None
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline and server.poll() is None:
                if not select.select([server.stdout], [], [], .1)[0]:
                    continue
                pending += os.read(server.stdout.fileno(), 65536).decode()
                match = re.search(r'PERF_ORIGIN=(\S+)', pending)
                if match:
                    origin = match[1]
                    break
            if not origin:
                raise RuntimeError('Offline profile server did not start')
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            client.open(origin).close()
            with sqlite3.connect(dataset / 'global.sqlite3') as db:
                project = db.execute('SELECT id FROM project_catalog WHERE project IS NOT NULL ORDER BY id LIMIT 1').fetchone()[0]
            with sqlite3.connect(dataset / 'projects' / project / 'engineering.sqlite3') as db:
                root, owner, execution = db.execute("SELECT id,json_extract(data,'$.owner_id'),json_extract(data,'$.steps[0].id') FROM governance_records WHERE type='workflow_instance' ORDER BY rowid LIMIT 1").fetchone()
            cases = [('query.workflow_control', {'id': root}),
                     ('query.workflow_instance', {'id': root, 'view': 'status'}),
                     ('query.workflow_executions', {'owner_id': owner, 'execution_id': execution})]
            rows = []
            digests = {}
            def read(method, params):
                return measure_rpc(client, origin, project, method, params)
            with (output / 'rpc.jsonl').open('w') as stream:
                for method, params in cases:
                    warm = read(method, params)
                    with (output / 'warmup.jsonl').open('a') as warmups:
                        warmups.write(json.dumps(warm) + '\n')
                    if warm['error']:
                        raise RuntimeError(warm)
                    digests[method] = warm['digest']
                    for sample in range(args.samples):
                        row = read(method, params)
                        row['sample'] = sample
                        rows.append(row)
                        stream.write(json.dumps(row) + '\n')
                        stream.flush()
            summary = {method: {**stats([r['ms'] for r in rows if r['method'] == method]),
                                'bytes': sorted({r['bytes'] for r in rows if r['method'] == method}),
                                'errors': sum(bool(r['error']) for r in rows if r['method'] == method),
                                'stable_content': all(r['digest'] == digests[method] for r in rows if r['method'] == method),
                                'digest': digests[method]} for method, _ in cases}
            (output / 'summary.json').write_text(json.dumps(summary, indent=2))
            print(json.dumps(summary, indent=2))
            if any(row['error'] for row in rows):
                raise RuntimeError('Profile contained errors')
        finally:
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == '__main__':
    main()
