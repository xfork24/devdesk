#!/usr/bin/env python3
"""Index actual actors, decisions and outputs from a stopped acceptance snapshot.

This is an evidence reader, never a workflow driver or an acceptance authority.
The original database, artifacts, native journals and Git worktrees remain intact.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path


def scheduling_metrics(records):
    """Cost attribution only; repeated reads are candidates, never proof of waste.

    Manager Turn wall intervals include platform waits. Their union must not be
    subtracted from an unrelated RPC percentile or called pure model time.
    """
    governance = records['governance_records']
    controllers = {row['id']: row for row in governance if row['type'] == 'workflow_controller'}
    calls = [row for row in governance if row['type'] == 'controller_tool_call'
             and row.get('definition_id') in controllers]
    sessions = {row['data'].get('session_id') for row in controllers.values()}
    sessions.update(row['data'].get('session_id') for row in calls)
    sessions.discard(None)
    turns = [row for row in records.get('turns', []) if row.get('session_id') in sessions]
    intervals, unmeasured = [], []
    for turn in turns:
        began, ended = turn.get('started'), turn.get('finished')
        if type(began) is int and type(ended) is int and ended >= began:
            intervals.append((began, ended))
        else:
            unmeasured.append(turn['id'])
    union = []
    for began, ended in sorted(intervals):
        if union and began <= union[-1][1]:
            union[-1][1] = max(union[-1][1], ended)
        else:
            union.append([began, ended])
    reads = Counter()
    for row in calls:
        data = row['data']
        method = data.get('method', '')
        if method in ('workflow.inspect', 'workflow.execution.inspect', 'workflow.outputs.read',
                      'workflow.events.read', 'project.artifact.read', 'configuration.read'):
            digest = hashlib.sha256(json.dumps(data.get('arguments'), sort_keys=True).encode()).hexdigest()
            reads[(method, digest)] += 1
    decisions = [row['data'] for row in governance if row['type'] == 'workflow_decision']
    return {
        'scope': 'All exported controllers and decisions; not a per-root critical path',
        'boundary': 'Offline attribution, not autonomy or SLA acceptance. Turn timestamps are lifecycle fields, not COMMIT boundaries; repeated identical reads may be necessary after events.',
        'manager_session_ids': sorted(sessions), 'manager_turns': len(turns),
        'measured_closed_turns': len(intervals), 'unmeasured_turn_ids': unmeasured,
        'manager_turn_union_ms': sum(ended - began for began, ended in union),
        'manager_turn_intervals': union,
        'unattributed_controller_call_ids': [row['id'] for row in governance
            if row['type'] == 'controller_tool_call' and row.get('definition_id') not in controllers],
        'native_calls_by_method': dict(Counter(row['data'].get('method', 'unknown') for row in calls)),
        'native_calls_by_status': dict(Counter(row['data'].get('status', 'unknown') for row in calls)),
        'repeated_read_candidates': sum(max(0, count - 1) for count in reads.values()),
        'business_actions_by_actor': dict(Counter(d.get('actor', {}).get('kind', 'unknown')
            for d in decisions if d.get('operation') in ('dispatch', 'accept', 'complete'))),
        'segment_grants': sum(row['type'] == 'workflow_segment' for row in governance),
        'missing_native_turn_ids': sorted({row['data'].get('turn_id') for row in calls
            if row['data'].get('turn_id')} - {row['id'] for row in turns}),
    }


def summarize(archive, scene):
    records = json.loads((archive / 'project-records.json').read_text())
    governance = records['governance_records']
    by_id = {row['id']: row for row in governance}
    sessions = {row['id']: row for row in records.get('sessions', [])}
    turns = {row['id']: row for row in records.get('turns', [])}
    sources = {row['reference']: row for row in records.get('source_definitions', [])}
    decisions = [row for row in governance if row['type'] == 'workflow_decision']
    instances = [row for row in governance if row['type'] == 'workflow_instance']
    executions = records.get('executions', [])
    steps = []
    for instance in instances:
        runtime = instance['data']
        view_file = scene / (instance['id'] + '.json')
        resolved_outputs = json.loads(view_file.read_text()).get('outputs', {}) if view_file.exists() else {}
        current = {step['id']: step for step in runtime['steps']}
        histories = {
            row['definition_id']: row['data'] for row in governance
            if row['type'] == 'workflow_execution_history'
            and row['data'].get('workflow_instance_id') == instance['id']
        }
        for execution_id, step in {**histories, **current}.items():
            session = sessions.get(step.get('session_id'))
            turn = turns.get(step.get('turn_id'))
            plan_record = by_id.get(step.get('plan_revision_id'), {})
            plan = plan_record.get('data', {}).get('plan', runtime['plan'])
            document = plan.get('document') or sources[plan['workflow_ref']]['data']
            definition = next((item for item in document['steps'] if item.get('node_id') == step['node_id'] or item['key'] == step['step']), None)
            agent_ref = step.get('agent_revision_id') or (session or {}).get('agent')
            agent = plan.get('definitions', {}).get(agent_ref) or sources.get(agent_ref, {})
            capability_ref = agent.get('capability_ref') or agent.get('refs', {}).get('capability_ref')
            capability = plan.get('definitions', {}).get(capability_ref) or sources.get(capability_ref, {})
            refs = capability.get('refs', capability)
            actor_decisions = [row for row in decisions if row['data'].get('execution_id') == execution_id]
            tools = []
            for row in governance:
                if row['type'] == 'workflow_intent' and row['definition_id'] == execution_id:
                    tools.append(row)
                elif row['type'] == 'platform_call':
                    caller = row['data'].get('request', {}).get('caller') or {}
                    if row['id'] == step.get('tool_call_id') or (
                        step.get('turn_id') and caller.get('turn_id') == step['turn_id']
                    ):
                        tools.append(row)
            outputs = {port: resolved_outputs.get(output_id) or by_id.get(output_id, {'id': output_id})
                       for port, output_id in step.get('outputs', {}).items()}
            steps.append({
                'instance_id': instance['id'], 'owner_id': runtime['owner_id'],
                'execution_id': execution_id, 'current': execution_id in current,
                'step': step, 'definition': definition,
                'implementer': {'executor': (definition or {}).get('executor'),
                                'agent_ref': agent_ref, 'capability_ref': capability_ref,
                                'model_ref': refs.get('model_ref'), 'harness_ref': refs.get('harness_ref'),
                                'session': session, 'turn': turn,
                                'actual_executions': [row for row in executions if row['session_id'] == step.get('session_id')]},
                'decisions': actor_decisions, 'tool_and_intent_records': tools, 'output_records': outputs,
                'conversation_event_ids': [row['id'] for row in records.get('conversation_events', [])
                                           if step.get('turn_id') and row['turn_id'] == step['turn_id']],
            })
    steps.sort(key=lambda item: (item['instance_id'], item['step']['path'], str(item['step'].get('started') or ''), item['execution_id']))
    result = {'snapshot': str(archive), 'scene': str(scene),
              'instances': [{key: row['data'].get(key) for key in ('owner_id', 'workflow_ref', 'status', 'active_plan_revision_id')}
                            | {'id': row['id']} for row in instances],
              'steps': steps, 'all_decisions': decisions,
              'scheduling_metrics': scheduling_metrics(records),
              'repository_operations': records.get('repository_operations', []),
              'events': records.get('events', [])}
    (scene / 'step-ledger.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    lines = ['# 工程执行现场报告', '',
             '本报告直接读取停止后的 SQLite 一致快照；不修改状态，也不把模型自述当作外部执行证明。', '',
             f'完整现场：`{scene}`。一致快照及注册 Artifact：`{archive}`。', '',
             '完整步骤、历史失败、冻结定义、实际 Session/Turn/执行者、调度 Actor、决策理由与输出记录见 `step-ledger.json`；原始记录见快照中的 `project-records.json`。', '',
             '| 实例 | 工作 | 精确 Workflow | 状态 |', '|---|---|---|---|']
    for instance in instances:
        data = instance['data']
        lines.append(f"| {instance['id']} | {data['owner_id']} | {data['workflow_ref']} | {data['status']} |")
    lines += ['', '## 每步实际实施与控制', '',
              '当前执行与历史分别标记。启动前替换的 queued 历史没有实际实施者或产出；历史行数不表示失败或重试次数。', '',
              '| 类型 | 路径 / 执行 | 状态 | 实施者 / 模型 | 控制 Actor | 产出端口 |', '|---|---|---|---|---|---|']
    for item in steps:
        step = item['step']
        actor = item['implementer']
        executor = (item['definition'] or {}).get('executor', {})
        performer = actor['agent_ref'] or executor.get('tool_ref') or executor.get('type', 'unknown')
        controls = ', '.join(f"{row['data']['operation']}:{row['data']['actor']['kind']}" for row in item['decisions'])
        category = '当前' if item['current'] else '历史'
        if not item['current'] and step['status'] == 'queued' and not step.get('started'):
            performer = '未实施'
        lines.append(f"| {category} | {step['path']} / {item['execution_id']} | {step['status']} | {performer} / {actor['model_ref'] or '—'} | {controls} | {', '.join(step.get('outputs', {}))} |")
    lines += ['', '## 当前步骤的决策与交付索引', '',
              '以下理由和身份直接来自原始记录；完整交付内容位于 step-ledger.json 的 output_records，注册文件可用 Artifact ID 追至一致快照。', '']
    for item in steps:
        if not item['current']:
            continue
        step = item['step']
        actor = item['implementer']
        lines += [f"### {item['instance_id']} · {step['path']}", '',
                  f"执行：`{item['execution_id']}`；计划：`{step.get('plan_revision_id')}`；输入摘要：`{step.get('input_digest')}`。", '',
                  f"执行器：`{json.dumps(actor['executor'], ensure_ascii=False, sort_keys=True)}`。", '',
                  f"Agent：`{actor['agent_ref'] or '—'}`；模型：`{actor['model_ref'] or '—'}`；Session：`{step.get('session_id') or '—'}`；Turn：`{step.get('turn_id') or '—'}`。", '']
        for row in item['decisions']:
            decision = row['data']
            control = decision['actor']
            if control['kind'] == 'segment':
                authorization = by_id.get(control.get('authorization_id'), {})
                control = {'segment_actor': control, 'authorizing_actor': authorization.get('data', {}).get('actor')}
            lines += [f"- 决策 `{row['id']}`：`{decision['operation']}`；Actor：`{json.dumps(control, ensure_ascii=False, sort_keys=True)}`。", '',
                      '```text', decision.get('reason') or '（未记录理由）', '```', '']
        for port, output in item['output_records'].items():
            metadata = {key: output[key] for key in ('id', 'content_hash', 'artifact_id', 'delivery_ref', 'value_digest') if key in output}
            output_id = step.get('outputs', {}).get(port)
            metadata['id'] = output_id
            record = by_id.get(output_id, {})
            if record.get('content_hash'):
                metadata['content_hash'] = record['content_hash']
            data = output.get('data', {})
            metadata.update({key: data[key] for key in ('artifact_id', 'delivery_ref', 'value_digest') if key in data})
            lines += [f"- 产出端口 `{port}`：`{json.dumps(metadata, ensure_ascii=False, sort_keys=True)}`。", '']
    metrics = result['scheduling_metrics']
    lines += ['', '## 调度成本诊断', '',
              f"经理 Turn {metrics['manager_turns']} 项，已闭合区间并集 {metrics['manager_turn_union_ms']} ms，缺少完整时间 {len(metrics['unmeasured_turn_ids'])} 项；相同方法/参数重复读取候选 {metrics['repeated_read_candidates']} 项。", '',
              '这些是离线归因，重复读取可能响应了新事件，不自动判为无效回合；Turn 墙钟包含平台等待，不是纯模型推理时间。原生失败及未知状态、缺失 Turn 均保留在 step-ledger.json 的 scheduling_metrics。']
    lines += ['', '## 核验入口', '',
              '- `attempts/`：实际测试脚本、输入绑定与驱动选择回执；`binaries/`：按 SHA256 保留的实际 Host。',
              '- `data/projects/*/execution/sessions/*/coordinator/*.jsonl`：原生模型请求、工具动作及真实观察。',
              '- `data/projects/*/repository`、工作空间和 Git worktree：保留源码、测试、候选与提交现场。',
              '- `raw-evidence-validation.json`、`native-tdd-observations.json`、`publication-evidence.json`：仅在对应核验实际成功后生成；失败原因和历次恢复文件继续保留。',
              '- `project-evidence-*/MANIFEST.json`：一致快照与 Artifact 内容身份；认证文件由外部凭据源重新提供，不纳入分享材料。', '',
              '完成状态和证据核验是不同事实；本报告的表格不代替最终验收结论。', '']
    (scene / 'EXECUTION_SCENE.md').write_text('\n'.join(lines))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--scene', type=Path, required=True)
    args = parser.parse_args()
    report = summarize(args.archive.resolve(), args.scene.resolve())
    print(json.dumps({'steps': len(report['steps']), 'instances': len(report['instances'])}))
