"""Save offline before/after timelines from verified, immutable benchmark files."""
import argparse
import hashlib
import json
from html import escape
from pathlib import Path

from workflow_timeline import build_spans, clock, elapsed
from workflow_timing import union_ms


COLORS = {'agent': '#377eb8', 'business_tools': '#269778',
          'workflow': '#8060ab', 'external': '#d28927'}
LABELS = {'agent': 'Agent 服务', 'business_tools': '业务工具',
          'workflow': 'Workflow 编排与运行时', 'external': '外部排除'}


def ledger(report, costs):
    """Subdivide the existing exclusive ledger; never add overlapping spans."""
    controls = [(t['started'], t['finished']) for t in costs['native_tools']
                if t['group'] in ('controller', 'native_platform_bridge')]
    # Platform execution wins over its enclosing native bridge, as in workflow_costs.
    business = [(t['started'], t['finished']) for t in costs['native_tools']
                if t['group'] == 'shell']
    business += [(t['started'], t['finished']) for t in report['platform_calls']]
    points = sorted({point for s in costs['slices'] for point in (s['started'], s['finished'])}
                    | {point for a, b in controls + business for point in (a, b)})
    result = []
    for a, b in zip(points, points[1:]):
        source = next((s['category'] for s in costs['slices']
                       if s['started'] <= a and s['finished'] >= b), None)
        if source is None:
            continue
        category = source
        if source == 'tools':
            category = 'business_tools' if any(x <= a and y >= b for x, y in business) else 'workflow'
        elif source in ('lifecycle', 'internal'):
            category = 'workflow'
        if result and result[-1]['category'] == category and result[-1]['finished'] == a:
            result[-1]['finished'] = b
        else:
            result.append({'started': a, 'finished': b, 'category': category})
    totals = {key: sum(s['finished'] - s['started'] for s in result if s['category'] == key)
              for key in COLORS}
    assert {k: totals[k] for k in costs['component_allocation_ms']} == costs['component_allocation_ms']
    assert totals['external'] == report['timing_audit']['excluded_union_ms']
    return result


def timeline(report, costs, title, basis, common_duration):
    dataset = build_spans(report)
    start = report['started_at_ms']
    excluded = dataset['excluded']
    slices = ledger(report, costs)

    def offset(relative):
        removed = union_ms([(a, min(relative, b)) for a, b in excluded if a < relative]) if basis == 'net' else 0
        return relative - removed

    def x(relative):
        return 515 + offset(relative) / common_duration * 760

    spans = dataset['spans']
    by_id = {s['id']: s for s in spans}
    width, top, row_height = 1640, 235, 27
    height = top + row_height * len(spans) + 45
    identity = report['root']+'-'+basis
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title-{identity} desc-{identity}">',
             f'<title id="title-{identity}">{escape(title)} · 完整时序 · {basis}</title>',
             f'<desc id="desc-{identity}">阶段、步骤、步骤间隔、审批准备、人工响应及显式完成均逐行展示。颜色来自互斥时间账本，聚合行不能相加。净轴移除外部响应与有证据的休眠。</desc>',
             '<style>text{font:13px system-ui,sans-serif;fill:#202b3d}.small{font-size:12px}.grid{stroke:#d9dee5;stroke-width:1}g[data-span-id]{cursor:pointer}g[data-span-id]:hover .row-bg,g[data-span-id]:focus .row-bg{fill:#edf2f7}</style>',
             f'<rect width="{width}" height="{height}" fill="#fff"/>',
             f'<text x="18" y="30" style="font-size:21px">{escape(title)} · {"净流程" if basis == "net" else "原始墙钟"} · {len(spans)} 项完整时序</text>',
             f'<text x="18" y="56">原始 {elapsed(report["wall_ms"])} − 外部响应 {elapsed(report["timing_audit"]["external_approval_response_ms"])} − 休眠约 {elapsed(report["timing_audit"]["host_suspension_ms"])} = 净 {elapsed(dataset["netMs"])}</text>',
             '<text x="18" y="79" class="small">所有颜色沿用 Host 观测账本；Agent 包括服务/传输，workflow 包括运行时。阶段与子流程行包含后代，不能累计求和。</text>']
    for index, (key, label) in enumerate(LABELS.items()):
        px = 18 + index * 260
        parts += [f'<rect x="{px}" y="94" width="14" height="12" fill="{COLORS[key]}"/>',
                  f'<text x="{px+21}" y="105" class="small">{label}</text>']
    step = 60000 if common_duration <= 1200000 else 180000
    for at in range(0, int(common_duration) + 1, step):
        px = 515 + at / common_duration * 760
        parts += [f'<line class="grid" x1="{px:.2f}" x2="{px:.2f}" y1="128" y2="{height-28}"/>',
                  f'<text x="{px:.2f}" y="124" text-anchor="middle" class="small">{at/60000:g} 分</text>']
    for index, (key, label) in enumerate(LABELS.items()):
        y = 144 + 18 * index
        parts.append(f'<text x="18" y="{y+6}" class="small">{label} · 互斥总账</text>')
        for s in slices:
            if s['category'] != key:
                continue
            a, b = s['started']-start, s['finished']-start
            px, size = x(a), x(b)-x(a)
            if size > 0:
                parts.append(f'<rect x="{px:.2f}" y="{y-5}" width="{size:.2f}" height="11" fill="{COLORS[key]}"/>')
        total = costs['component_allocation_ms'].get(key, report['timing_audit']['excluded_union_ms'])
        parts.append(f'<text x="1295" y="{y+6}" class="small">{elapsed(total)}{"（净轴不显示）" if key=="external" and basis=="net" else ""}</text>')
    parts.append('<text x="1295" y="224" class="small">原始 / 净跨度；聚合行不累计</text>')
    for index, span in enumerate(spans):
        a, b = span['startMs'], span['startMs']+span['durationMs']
        net = span['durationMs'] - union_ms([(max(a, x), min(b, y)) for x, y in excluded if y>a and x<b])
        component = {key: sum(max(0, min(start+b, s['finished'])-max(start+a, s['started']))
                              for s in slices if s['category'] == key) for key in COLORS}
        assert sum(component.values()) == span['durationMs']
        assert sum(component[k] for k in costs['component_allocation_ms']) == net
        span['attributes']['净跨度'] = elapsed(net)
        span['attributes'].update({LABELS[k]: elapsed(v) for k, v in component.items()})
        parent, depth = span.get('parentId'), 0
        while parent:
            depth += 1
            parent = by_id[parent].get('parentId')
        y = top + index * row_height
        name = span['name'].replace('经理准备审批请求', '审批请求准备')
        info = f'{name} | {span["attributes"]["开始"]} → {span["attributes"]["结束"]} | 原始 {elapsed(span["durationMs"])} / 净 {elapsed(net)}'
        parts += [f'<g tabindex="0" role="button" data-span-id="{escape(span["id"], quote=True)}" aria-label="{escape(info, quote=True)}"><title>{escape(info)}</title>',
                  f'<rect class="row-bg" x="0" y="{y-12}" width="{width}" height="25" fill="transparent"/>',
                  f'<text x="{18+depth*12}" y="{y+5}">{index+1:02d}　{escape(name)}</text>']
        for s in slices:
            began, ended = max(a, s['started']-start), min(b, s['finished']-start)
            if ended <= began:
                continue
            px, size = x(began), x(ended)-x(began)
            if size > 0:
                parts.append(f'<rect x="{px:.2f}" y="{y-7}" width="{max(.15,size):.2f}" height="12" fill="{COLORS[s["category"]]}"/>')
        if offset(a) == offset(b):
            parts.append(f'<circle cx="{x(a):.2f}" cy="{y-1}" r="2" fill="{COLORS["external"]}"/>')
        parts += [f'<text x="1295" y="{y+5}" class="small">{elapsed(span["durationMs"])} / {elapsed(net)}</text></g>']
    parts += [f'<text x="18" y="{height-8}" class="small">原始起点 {clock(start)}；共同横轴上限 {elapsed(common_duration)}。点击或键盘选择行可在 HTML 查看确切时间。</text>', '</svg>']
    return '\n'.join(parts), dataset


def metrics(report, costs):
    completion = sum(m['completion_gap_net_ms'] for m in report['completion_milestones'])
    observation_only = [t for t in costs['native_turns'] if t['role'] == 'manager'
                        and t['controller_methods'] and 'workflow.controller.wait' in t['controller_methods']
                        and set(t['controller_methods']) <= {'workflow.inspect', 'workflow.outputs.read', 'workflow.controller.wait'}]
    return {'workflow_net_ms': report['timing_audit']['workflow_net_ms'],
            'approval_preparation_ms': report['timing_audit']['manager_approval_preparation_ms'],
            'instance_completion_ms': completion, 'settlement_ms': report['settlement_overhead_ms'],
            'manager_turns': costs['turn_counts']['manager'],
            'manager_wait_only_turns': costs['manager_wait_only']['count'],
            'manager_observation_only_turns': len(observation_only),
            'manager_wait_only_exclusive_ms': costs['manager_wait_only']['exclusive_agent_ms'],
            **{k+'_ms': v for k, v in costs['component_allocation_ms'].items()}}


def generate(before, before_costs, after, after_costs, directory):
    directory.mkdir(parents=True, exist_ok=True)
    reports, costs = [before, after], [before_costs, after_costs]
    titles = ['优化前', '优化后']
    datasets, views = [], []
    for i, report in enumerate(reports):
        rendered = {}
        for basis in ('raw', 'net'):
            duration = max(r['wall_ms'] if basis=='raw' else r['timing_audit']['workflow_net_ms'] for r in reports)
            svg, dataset = timeline(report, costs[i], titles[i], basis, duration)
            rendered[basis] = svg
            (directory/f'workflow-timeline-2026-11-20-{("before", "optimized")[i]}-{basis}.svg').write_text(svg+'\n')
        datasets.append(dataset)
        views.append(rendered)
    rows = []
    old, new = metrics(before, before_costs), metrics(after, after_costs)
    for key in old:
        rows.append({'metric': key, 'before': old[key], 'after': new[key],
                     'saved': old[key]-new[key], 'reduction_percent': round((old[key]-new[key])*100/old[key],3) if old[key] else None})
    data = {'titles': titles, 'views': views, 'datasets': datasets, 'metrics': rows}
    payload = json.dumps(data, ensure_ascii=False).replace('<', '\\u003c')
    template = Path(__file__).with_name('workflow_performance.html').read_text()
    (directory/'workflow-comparison-2026-11-20.html').write_text(template.replace('__PERFORMANCE_DATA__', payload))
    return {'schema_version': 1, 'before_root': before['root'], 'after_root': after['root'],
            'metrics': rows, 'notes': ['Each row is observed before/after, not a guaranteed causal saving.',
                                     'Phase windows overlap component totals; do not add them.']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ('before', 'before-costs', 'after', 'after-costs', 'directory'):
        parser.add_argument('--'+key, required=True, type=Path)
    args = parser.parse_args()
    paths = [args.before, args.before_costs, args.after, args.after_costs]
    result = generate(*(json.loads(p.read_text()) for p in paths), args.directory)
    result['sources'] = [{'path': p.name, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]
    (args.directory/'workflow-comparison-2026-11-20.json').write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
