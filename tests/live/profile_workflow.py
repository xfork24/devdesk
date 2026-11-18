"""Read-only timing report for a probe_streamlined_workflow evidence directory.

SDK duration includes model, network, context handling and dynamic tools. It is
not pure inference time. Tool round trips are subsets, not additive overhead.
Raw protocol text and credentials are never copied into this report.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3
import time


def union_ms(intervals, start, end):
    total = 0
    cursor = start
    for left, right in sorted(intervals):
        left, right = max(left, start, cursor), min(right, end)
        if right > left:
            total += right - left
            cursor = right
    return total


def profile(scene):
    context = json.loads((scene / 'context.json').read_text())
    project = scene / 'data/projects' / context['project']
    db = sqlite3.connect('file:' + str(project / 'engineering.sqlite3') + '?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    db.execute('BEGIN')
    now = round(time.time() * 1000)
    logs = []
    for path in sorted(scene.glob('host-*.log')):
        for line in path.open():
            if not line.startswith('{'):
                continue
            try:
                logs.append(json.loads(line))
            except ValueError:
                pass
    startups = {r['operation_id']: r['measurement'] for r in logs if r.get('type') == 'native_startup'}
    rpcs = [r for r in logs if r.get('type') == 'native_protocol' and r.get('phase') == 'reply_written']
    stops = [dict(r) for r in db.execute("SELECT type,created_at,data FROM governance_records WHERE type IN ('worker_stop_intent','worker_stop_observation')")]
    for row in stops:
        row['data'] = json.loads(row['data'])
    workers = {}
    for row in db.execute('SELECT * FROM executions ORDER BY started'):
        w = dict(row)
        w.pop('permissions', None)
        w.pop('container_id', None)
        w['elapsed_ms'] = (w['finished'] or now) - w['started']
        w['startup'] = startups.get(w['operation_id'])
        calls = [r for r in rpcs if r['operation_id'] == w['operation_id']]
        w['tool_roundtrips'] = {'count': len(calls), 'total_ms': sum(r['roundtrip_ms'] for r in calls)}
        # observed_at_ms on reply_written is the original request timestamp.
        w['tool_intervals'] = [[r['observed_at_ms'], r['observed_at_ms'] + r['roundtrip_ms']] for r in calls]
        begins = [r['created_at'] for r in stops if r['type'] == 'worker_stop_intent' and r['data'].get('worker_operation_id') == w['operation_id']]
        ends = [r['created_at'] for r in stops if r['type'] == 'worker_stop_observation' and r['data'].get('worker_basis', {}).get('resource', {}).get('id') == w['id']]
        w['stop_ms'] = ends[-1] - begins[-1] if begins and ends else None
        artifact = db.execute('SELECT relative_path FROM artifacts WHERE id=?', (w['protocol_artifact'],)).fetchone()
        items = {}
        turns = {}
        requests = []
        usage = None
        if artifact:
            for line in (project / artifact['relative_path']).open():
                try:
                    envelope = json.loads(line)
                    frame = envelope['frame']
                except (ValueError, KeyError):
                    continue
                params = frame.get('params', {})
                method = frame.get('method')
                if envelope.get('direction') == 'out' and method in ['thread/start', 'thread/resume', 'turn/start']:
                    requests.append({'method': method, 'fields_utf8_bytes': {key: len(json.dumps(value, ensure_ascii=False).encode()) for key, value in params.items()}, 'dynamic_tool_count': len(params['dynamicTools']) if isinstance(params.get('dynamicTools'), list) else None})
                if method in ['turn/started', 'turn/completed']:
                    turn = params['turn']
                    observed = turns.setdefault(turn['id'], {})
                    observed.update({k: turn.get(k) for k in ['id', 'status', 'durationMs', 'startedAt', 'completedAt']})
                    if method == 'turn/started':
                        observed['started_at_ms'] = frame.get('emittedAtMs')
                if method == 'thread/tokenUsage/updated':
                    usage = params.get('tokenUsage')
                if method in ['item/started', 'item/completed']:
                    item = params['item']
                    items[item['id']] = {'type': item['type'], 'tool': item.get('tool'), 'started': item.get('startedAtMs'), 'finished': item.get('completedAtMs'), 'text_bytes': len(item.get('text', '').encode()), 'argument_bytes': len(json.dumps(item.get('arguments', {}), ensure_ascii=False).encode())}
        w['sdk_turns'] = list(turns.values())
        w['sdk_request_sizes'] = requests
        w['sdk_ms'] = sum(t['durationMs'] or 0 for t in turns.values())
        w['sdk_token_usage_cumulative'] = usage
        w['item_counts'] = dict(Counter(i['type'] for i in items.values()))
        commands = [[i['started'], i['finished']] for i in items.values() if i['type'] == 'commandExecution' and i['started'] is not None and i['finished'] is not None]
        w['sdk_native_command_ms'] = union_ms(commands, w['started'], w['finished'] or now)
        w['output_text_bytes'] = sum(i['text_bytes'] for i in items.values() if i['type'] == 'agentMessage')
        workers[w['id']] = w
    phases = {}
    for entry in logs:
        if entry.get('type') == 'native_phase':
            phase = phases.setdefault(entry['phase'], {'count': 0, 'total_ms': 0})
            phase['count'] += 1
            phase['total_ms'] += entry['elapsed_ms']
    manager_calls = Counter()
    for row in db.execute("SELECT data FROM governance_records WHERE type='controller_tool_call'"):
        manager_calls[json.loads(row['data'])['method']] += 1
    report = {'generated_at_ms': now, 'root': context['root'], 'project': context['project'], 'limitations': ['SDK includes model/network/context/tool waiting; no pure inference breakdown.', 'Parallel durations overlap; sums are not end-to-end wall time.', 'Human waits, recovery and intentional test loads require separate annotation.', 'Other step time includes Human/input waits, not just runtime overhead.', 'Step finished timestamps describe producer work; instance wall time uses the actual completion decision.'], 'measured_phases': phases, 'manager_call_counts': dict(manager_calls), 'instances': [], 'workers': list(workers.values())}
    for row in db.execute("SELECT id,name,data,created_at FROM governance_records WHERE type='workflow_instance' AND (id=? OR json_extract(data,'$.root_instance_id')=?)", (context['root'], context['root'])):
        instance = json.loads(row['data'])
        steps = []
        historical = []
        current_ids = {s['id'] for s in instance['steps']}
        history = {}
        for raw in db.execute("SELECT data FROM governance_records WHERE type='workflow_execution_history' AND json_extract(data,'$.workflow_instance_id')=? ORDER BY rowid", (row['id'],)):
            attempt = json.loads(raw[0])
            if attempt['id'] not in current_ids:
                history[attempt['id']] = attempt
        for s in [*history.values(), *instance['steps']]:
            step = {k: s.get(k) for k in ['id', 'step', 'status', 'round', 'retry_of', 'started', 'finished', 'session_id', 'tool_call_id', 'budget_waited_ms']}
            ws = [w for w in workers.values() if w['session_id'] == s.get('session_id')]
            step['workers'] = [w['id'] for w in ws]
            if s.get('started'):
                step['elapsed_ms'] = (s.get('finished') or now) - s['started']
            if ws and s.get('finished') and all(w['finished'] and w['sdk_ms'] for w in ws):
                harness = sum((w['startup'] or {}).get('total_ms', 0) + (w['stop_ms'] or 0) for w in ws)
                step['measured_harness_ms'] = harness
                step['sdk_ms'] = sum(w['sdk_ms'] for w in ws)
                step['other_step_ms'] = step['elapsed_ms'] - harness - step['sdk_ms']
            (steps if s['id'] in current_ids else historical).append(step)
        ordered = sorted((s for s in [*historical, *steps] if s.get('started')), key=lambda s: s['started'])
        gaps = []
        for before, after in zip(ordered, ordered[1:]):
            start, end = before.get('finished'), after['started']
            if start is None or end <= start:
                continue
            manager = [w for w in workers.values() if not any(w['id'] in s['workers'] for s in [*historical, *steps]) and w['started'] < end and (w['finished'] or now) > start]
            tool_intervals = [i for w in manager for i in w['tool_intervals']]
            sdk_intervals = [[t['started_at_ms'], t['started_at_ms'] + t['durationMs']] for w in manager for t in w['sdk_turns'] if t.get('started_at_ms') is not None and t['durationMs'] is not None]
            startup_intervals = [[w['started'], w['started'] + w['startup']['total_ms']] for w in manager if w['startup']]
            gaps.append({'from': before['step'], 'from_execution': before['id'], 'to': after['step'], 'to_execution': after['id'], 'start': start, 'end': end, 'elapsed_ms': end-start, 'manager_sdk_ms_including_tools': union_ms(sdk_intervals, start, end), 'manager_tool_roundtrip_ms': union_ms(tool_intervals, start, end), 'manager_startup_ms': union_ms(startup_intervals, start, end), 'unattributed_ms': end-start-union_ms(sdk_intervals+tool_intervals+startup_intervals, start, end)})
        completed = db.execute("SELECT max(created_at) FROM governance_records WHERE type='workflow_decision' AND definition_id=? AND json_extract(data,'$.operation')='complete'", (row['id'],)).fetchone()[0]
        report['instances'].append({'id': row['id'], 'name': row['name'], 'status': instance['status'], 'created_at_ms': row['created_at'], 'completed_at_ms': completed, 'wall_ms': (completed or now) - row['created_at'], 'steps': steps, 'historical_steps': historical, 'handoffs': gaps})
    db.rollback()
    db.close()
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence_directory', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = profile(args.evidence_directory.resolve())
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps({'instances': len(report['instances']), 'workers': len(report['workers']), 'output': str(args.output)}, ensure_ascii=False))
