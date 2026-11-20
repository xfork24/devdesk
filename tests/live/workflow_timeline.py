"""Render the audited workflow metadata as a chronological SVG and span dataset."""
import argparse
import json
from datetime import datetime, timezone, timedelta
from html import escape
from pathlib import Path

from workflow_timing import union_ms


def elapsed(ms):
    minutes, seconds = divmod(ms / 1000, 60)
    return f'{int(minutes):02d}:{seconds:06.3f}'


def clock(ms):
    return datetime.fromtimestamp(ms / 1000, timezone(timedelta(hours=8))).isoformat(timespec='milliseconds')


def build_spans(report):
    start, finish = report['started_at_ms'], report['completed_at_ms']
    spans = []

    def add(identity, name, began, ended, service='流程', parent='workflow', **attributes):
        if began is None or ended is None or ended < began:
            raise ValueError(f'Invalid span: {identity}')
        spans.append({'id': identity, 'name': name, 'startMs': began - start,
            'durationMs': ended - began, 'service': service,
            **({'parentId': parent} if parent else {}),
            'attributes': {'开始': clock(began), '结束': clock(ended),
                '原始耗时': elapsed(ended-began), **attributes}})

    add('workflow', 'Feature 端到端（含 Task）', start, finish, parent=None)
    stages = {'requirements': '需求', 'design': '设计', 'planning': '任务规划',
              'implementation': '实施', 'delivery': 'Feature 交付'}
    feature = sorted((s for s in report['steps'] if s['layer']=='Feature'), key=lambda s:s['started'])
    stage_order = list(dict.fromkeys(s['plan_stage'] for s in feature))
    for index, stage in enumerate(stage_order):
        began = start if index==0 else min(s['started'] for s in feature if s['plan_stage']==stage)
        ended = min(s['started'] for s in feature if s['plan_stage']==stage_order[index+1]) if index+1<len(stage_order) else finish
        add('stage-'+stage, stages.get(stage,stage), began, ended)
    human = {a['execution']:a for a in report['timing_audit']['approvals']}
    implementation = next(s for s in feature if s['step']=='implementation')
    child = next(m for m in report['completion_milestones'] if m['instance']!=report['root'])
    task_steps = sorted((s for s in report['steps'] if s['layer']=='Task'),key=lambda s:s['started'])
    add('task', 'Task 子流程', task_steps[0]['started'], child['workflow_completed_at_ms'], '流程', implementation['id'])
    add('task-dispatch','创建 Task 并首次派发',implementation['started'],task_steps[0]['started'],'内部等待',implementation['id'])
    for layer, ordered in [('Feature',feature),('Task',task_steps)]:
        for index, step in enumerate(ordered):
            parent = 'stage-'+step['plan_stage'] if layer=='Feature' else 'task'
            accepted = max(step['finished'], step['accepted_at_ms'] or step['finished'])
            if step['id']==implementation['id']:
                accepted = max(accepted,child['workflow_completed_at_ms'])
                add(step['id'], step['name'], step['started'], accepted, '流程', parent,
                    **{'执行ID':step['id'], '说明':'子流程聚合；不能与内部步骤相加'})
            else:
                add(step['id'], step['name'], step['started'], accepted, '步骤', parent,
                    **{'执行ID':step['id'],'生产者结束':clock(step['finished']),
                       '生产者原始耗时':elapsed(step['finished']-step['started']),
                       '结束至接纳原始耗时':elapsed(accepted-step['finished'])})
                approval = human.get(step['id'])
                if approval:
                    add(step['id']+'-prepare','经理准备审批请求',approval['ready_at_ms'],approval['requested_at_ms'],'内部等待',step['id'])
                    add(step['id']+'-approval','代审批响应（净耗时排除）',approval['requested_at_ms'],approval['signed_at_ms'],'外部排除',step['id'])
            if index+1<len(ordered):
                following=ordered[index+1]
                if following['started']>accepted:
                    add(step['id']+'-gap','步骤间调度 → '+following['name'],accepted,following['started'],'内部等待',parent)
    add('initial-dispatch','启动及首次派发',start,feature[0]['started'],'内部等待','stage-'+feature[0]['plan_stage'])
    add('task-close','Task 最后一步结束 → 显式完成',child['last_step_finished_at_ms'],child['workflow_completed_at_ms'],'内部等待','task',
        **{'排除休眠后':elapsed(child['completion_gap_net_ms'])})
    root = next(m for m in report['completion_milestones'] if m['instance']==report['root'])
    add('root-close','最后一步接纳 → Feature 显式完成',root['last_step_accepted_at_ms'],root['workflow_completed_at_ms'],'内部等待','stage-'+feature[-1]['plan_stage'])
    add('settlement','经理关闭及 worker 静止',root['workflow_completed_at_ms'],finish,'内部等待','stage-'+feature[-1]['plan_stage'])
    for index, pause in enumerate(report['timing_audit']['host_suspensions']):
        add('sleep-'+str(index),'主机休眠（净耗时排除）',pause['started_at_ms'],pause['finished_at_ms'],'外部排除',
            **{'边界精度':pause['precision']})
    # Model/tool/dispatch time is already inside the step spans; it is not summed again.
    ids = {span['id'] for span in spans}
    assert len(ids)==len(spans)
    by_id={span['id']:span for span in spans}
    for span in spans:
        assert span['startMs']>=0 and span['startMs']+span['durationMs']<=finish-start
        if span.get('parentId'):
            parent=by_id[span['parentId']]
            assert parent['startMs']<=span['startMs'], span['name']
            assert parent['startMs']+parent['durationMs']>=span['startMs']+span['durationMs'], span['name']
    excluded=[(a['requested_at_ms']-start,a['signed_at_ms']-start) for a in human.values()]
    excluded += [(p['started_at_ms']-start,p['finished_at_ms']-start) for p in report['timing_audit']['host_suspensions']]
    assert union_ms(excluded)==report['timing_audit']['excluded_union_ms']
    return {'spans':sorted(spans,key=lambda span:(span['startMs'],-span['durationMs'])),
        'collapsed':[],
        'excluded':excluded,'durationMs':finish-start,'netMs':report['timing_audit']['workflow_net_ms']}


def render_svg(report, path):
    start, wall = report['started_at_ms'], report['wall_ms']
    rows = sorted(report['steps'], key=lambda s:s['started'])
    width, left, plot, row_height, top = 1560, 480, 800, 30, 155
    height = top + len(rows)*row_height + 110
    def x(at): return left+(at-start)/wall*plot
    elements=[f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">',
        '<title id="title">成功回归的 workflow 端到端时序</title>',
        '<desc id="desc">33 个步骤按开始时间排列。蓝色为生产者执行，浅蓝为内部接纳等待，橙色为代审批响应，灰色竖带为主机休眠。横轴为原始墙钟分钟；代审批和主机休眠从净耗时排除。</desc>',
        '<style>text{font:14px system-ui,sans-serif;fill:#202b3d}.small{font-size:12px}.grid{stroke:#d9dee5;stroke-width:1}</style>',
        f'<rect width="{width}" height="{height}" fill="#fff"/>',
        f'<text x="24" y="32" style="font-size:22px">Workflow 端到端时间线 · {clock(start)[:10]}</text>',
        f'<text x="24" y="61">原始 {elapsed(wall)}　− 代审批 {elapsed(report["timing_audit"]["external_approval_response_ms"])}　− 主机休眠估算 {elapsed(report["timing_audit"]["host_suspension_ms"])}　= 净耗时约 {elapsed(report["timing_audit"]["workflow_net_ms"])}</text>']
    for index,(color,label) in enumerate([('#377eb8','生产者执行'),('#a9c9e2','内部接纳'),('#df8b27','代审批（排除）'),('#dfe2e6','主机休眠（排除）')]):
        px=24+index*230
        elements += [f'<rect x="{px}" y="80" width="15" height="12" fill="{color}"/>',f'<text x="{px+23}" y="91" class="small">{label}</text>']
    for minute in range(0,37,3):
        px=x(start+minute*60000)
        elements += [f'<line class="grid" x1="{px:.2f}" x2="{px:.2f}" y1="130" y2="{height-80}"/>',f'<text x="{px:.2f}" y="121" text-anchor="middle" class="small">{minute} 分</text>']
    for pause in report['timing_audit']['host_suspensions']:
        elements.append(f'<rect x="{x(pause["started_at_ms"]):.2f}" y="130" width="{max(.2,pause["duration_ms"]/wall*plot):.2f}" height="{height-210}" fill="#dfe2e6" fill-opacity=".65"/>')
    approvals={a['execution']:a for a in report['timing_audit']['approvals']}
    elements.append('<text x="1300" y="121" class="small">执行 / 结束至接纳</text>')
    for index,step in enumerate(rows):
        y=top+index*row_height
        accepted=max(step['finished'],step['accepted_at_ms'] or step['finished'])
        tooltip=escape(f'{step["layer"]} {step["name"]} | {clock(step["started"])} → {clock(step["finished"])} | 接纳 {clock(accepted)}')
        elements += [f'<g><title>{tooltip}</title><text x="24" y="{y+5}">{index+1:02d}　{escape(step["layer"]+" · "+step["name"])}</text>',
            f'<rect x="{x(step["started"]):.2f}" y="{y-8}" width="{max(.4,step["duration_ms"]/wall*plot):.2f}" height="14" fill="#377eb8"/>',
            f'<rect x="{x(step["finished"]):.2f}" y="{y-8}" width="{max(0,(accepted-step["finished"])/wall*plot):.2f}" height="14" fill="#a9c9e2"/>',
            f'<text x="1300" y="{y+5}" class="small">{elapsed(step["duration_ms"])} / {elapsed(accepted-step["finished"])}</text>']
        if step['id'] in approvals:
            a=approvals[step['id']]
            elements.append(f'<rect x="{x(a["requested_at_ms"]):.2f}" y="{y-8}" width="{a["external_response_ms"]/wall*plot:.2f}" height="14" fill="#df8b27"/>')
        elements.append('</g>')
    child=next(member for member in report['completion_milestones'] if member['instance']!=report['root'])
    elements += [f'<text x="24" y="{height-45}" class="small">Task 最后一步结束至显式完成：原始 {elapsed(child["completion_gap_ms"])}；扣除休眠后 {elapsed(child["completion_gap_net_ms"])}。Feature 实施行聚合 Task，不能与子步骤相加。</text>',
        f'<text x="24" y="{height-20}" class="small">细小平台步骤保持真实比例；精确时间和全部步骤间等待见同名基准文档及交互时序。结束时间取持久化完成与 worker 静止时间。</text>','</svg>']
    path.write_text('\n'.join(elements)+'\n')


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report',type=Path)
    parser.add_argument('--svg',required=True,type=Path)
    parser.add_argument('--spans',required=True,type=Path)
    args=parser.parse_args()
    report=json.loads(args.report.read_text())
    args.spans.write_text(json.dumps(build_spans(report),ensure_ascii=False,indent=2)+'\n')
    render_svg(report,args.svg)
