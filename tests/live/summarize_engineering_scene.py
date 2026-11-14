#!/usr/bin/env python3
"""Index actual actors, decisions and outputs from a stopped acceptance snapshot.

This is an evidence reader, never a workflow driver or an acceptance authority.
The original database, artifacts, native journals and Git worktrees remain intact.
"""
import argparse
import json
from pathlib import Path


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
              '| 路径 / 执行 | 状态 | 实施者 / 模型 | 控制 Actor | 产出端口 |', '|---|---|---|---|---|']
    for item in steps:
        step = item['step']
        actor = item['implementer']
        executor = (item['definition'] or {}).get('executor', {})
        performer = actor['agent_ref'] or executor.get('tool_ref') or executor.get('type', 'unknown')
        controls = ', '.join(f"{row['data']['operation']}:{row['data']['actor']['kind']}" for row in item['decisions'])
        lines.append(f"| {step['path']} / {item['execution_id']} | {step['status']} | {performer} / {actor['model_ref'] or '—'} | {controls} | {', '.join(step.get('outputs', {}))} |")
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
