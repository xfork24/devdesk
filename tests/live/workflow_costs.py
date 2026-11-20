"""Attribute the frozen benchmark using Host observations, never mixed VM clocks."""
import argparse
import hashlib
import json
import sqlite3
from collections import Counter
from contextlib import closing
from pathlib import Path

from workflow_timing import union_ms


def partition(start, finish, groups):
    """First matching group owns each wall-clock slice; concurrency never adds time."""
    if finish<start or any(b<a for _,intervals in groups for a,b in intervals):
        raise ValueError('Reversed cost interval')
    clipped = [(name, [(max(start,a),min(finish,b)) for a,b in intervals
                      if b > start and a < finish]) for name, intervals in groups]
    points = sorted({start, finish} | {point for _, intervals in clipped for pair in intervals for point in pair})
    slices = []
    for a,b in zip(points,points[1:]):
        name = next((name for name,intervals in clipped if any(x <= a and y >= b for x,y in intervals)), 'internal')
        if slices and slices[-1]['category']==name and slices[-1]['finished']==a:
            slices[-1]['finished']=b
        else:
            slices.append({'started':a,'finished':b,'category':name})
    return slices


def allocations(slices, windows):
    groups = {}
    for span in slices:
        name = span['category']
        groups.setdefault(name,[]).extend((max(a,span['started']),min(b,span['finished']))
            for a,b in windows if b>span['started'] and a<span['finished'])
    return {key:union_ms(intervals) for key,intervals in groups.items()}


def observed_native(rows, turn_ids):
    native, tools = {}, {}
    for raw,at in rows:
        data=json.loads(raw)
        turn=data['basis']['turn_id']
        if turn not in turn_ids:
            continue
        for observation in data['value']['observations']:
            kind=observation['type']
            if kind in ('admitted','terminal'):
                native.setdefault(turn,{})['started' if kind=='admitted' else 'finished']=at
            if kind!='message' or observation.get('kind')!='tool':
                continue
            item=json.loads(observation['content'])
            if item.get('type') not in ('commandExecution','dynamicToolCall'):
                raise ValueError('Unsupported native tool type: '+str(item.get('type')))
            key=(turn,observation['item'])
            tool=tools.setdefault(key,{'id':observation['item'],'turn':turn,
                'kind':item['type'],'tool':item.get('tool') or 'shell'})
            field='started' if observation['key'].endswith(':started') else 'finished'
            tool[field]=at
            if field=='finished':
                tool['producer_reported_duration_ms']=(item.get('completedAtMs') or 0)-(item.get('startedAtMs') or 0)
    if set(native)!=set(turn_ids) or any(set(n)!= {'started','finished'} for n in native.values()):
        raise ValueError('Missing Host-observed native admission/termination')
    if any('started' not in tool or 'finished' not in tool for tool in tools.values()):
        raise ValueError('Missing Host-observed tool boundary')
    return native,list(tools.values())


def analyze(report, directory):
    start,finish=report['started_at_ms'],report['completed_at_ms']
    turns={turn['id']:turn for turn in report['turns']}
    with closing(sqlite3.connect((directory/'engineering.sqlite3').resolve().as_uri()+'?mode=ro',uri=True)) as db:
        rows=list(db.execute("SELECT data,created_at FROM governance_records WHERE type='provider_poll_observation' AND created_at BETWEEN ? AND ? ORDER BY created_at",(start,finish)))
        native,tools=observed_native(rows,set(turns))
        controller_calls=[json.loads(raw) for raw, in db.execute("SELECT data FROM governance_records WHERE type='controller_tool_call'")]
        controllers={call['call_id'] for call in controller_calls}
        manager=json.loads(db.execute("SELECT data FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.root_instance_id')=?",(report['root'],)).fetchone()[0])['session_id']
        external={external:identity for identity,external in db.execute('SELECT id,external_turn FROM turns') if identity in turns}
        startup={}
        for kind,raw,at in db.execute("SELECT type,data,created_at FROM governance_records WHERE type IN ('conversation_open_intent','conversation_start_observation') AND created_at BETWEEN ? AND ?",(start,finish)):
            item=json.loads(raw);turn=item.get('turn_id') or item.get('origin',{}).get('turn_id')
            if turn not in turns:
                continue
            field='open_intent' if kind=='conversation_open_intent' else item['phase']+'_observed'
            startup.setdefault(turn,{})[field]=at
    for tool in tools:
        tool['group']='shell' if tool['kind']=='commandExecution' else 'controller' if tool['id'] in controllers else 'native_platform_bridge'
        tool['duration_ms']=tool['finished']-tool['started']
        assert native[tool['turn']]['started']<=tool['started']<=tool['finished']<=native[tool['turn']]['finished']
    native_rows=[]
    anomalies=[]
    for turn,window in native.items():
        kernel=turns[turn]
        assert kernel['started']<=window['started']<=window['finished']<=kernel['finished']
        native_rows.append({'turn':turn,'session':kernel['session'],
            'role':'manager' if kernel['session']==manager else 'producer',**window,
            'preparation_ms':window['started']-kernel['started'],
            'settlement_ms':kernel['finished']-window['finished'],**startup.get(turn,{})})
    # Raw timestamps are diagnostic only; a VM clock may resynchronize mid-turn.
    seen=set()
    for sid in {turn['session'] for turn in turns.values()}:
        for path in (directory/'execution/sessions'/sid/'coordinator').glob('*.jsonl'):
            for line in path.open():
                frame=json.loads(line).get('frame',{});params=frame.get('params',{})
                if frame.get('method')!='turn/started' or params['turn']['id'] not in external:
                    continue
                identity=external[params['turn']['id']]
                if identity in seen:
                    continue
                seen.add(identity);emitted=frame['emittedAtMs']
                if emitted<turns[identity]['started'] or emitted>turns[identity]['finished']:
                    anomalies.append({'turn':identity,'producer_emitted_at_ms':emitted,
                        'host_turn_started_ms':turns[identity]['started'],
                        'host_admitted_ms':native[identity]['started'],
                        'action':'Use Host-observed admission and tool boundaries for all turns.'})
    audit=report['timing_audit']
    excluded=[(w['requested_at_ms'],w['signed_at_ms']) for w in audit['approvals']]
    excluded += [(w['started_at_ms'],w['finished_at_ms']) for w in audit['host_suspensions']]
    tool_intervals=[(tool['started'],tool['finished']) for tool in tools]
    tool_intervals += [(call['started'],call['finished']) for call in report['platform_calls']]
    groups=[('external',excluded),('tools',tool_intervals),
        ('agent',[(n['started'],n['finished']) for n in native_rows]),
        ('lifecycle',[(t['started'],t['finished']) for t in turns.values()])]
    slices=partition(start,finish,groups)
    totals=allocations(slices,[(start,finish)])
    assert totals['external']==audit['excluded_union_ms']
    assert sum(totals.values())==finish-start
    assert sum(v for k,v in totals.items() if k!='external')==audit['workflow_net_ms']
    def window(name,kind,a,b,**metadata):
        values=allocations(slices,[(a,b)])
        return {'name':name,'kind':kind,'started':a,'finished':b,
            'wall_ms':b-a,'net_ms':b-a-values.get('external',0),
            'allocation_ms':values,**metadata}
    windows=[]
    for a in audit['approvals']:
        windows.append(window(a['step'],'approval_preparation',a['ready_at_ms'],a['requested_at_ms']))
    for layer in ('Feature','Task'):
        steps=sorted((s for s in report['steps'] if s['layer']==layer),key=lambda s:s['started'])
        for previous,following in zip(steps,steps[1:]):
            accepted=previous['accepted_at_ms'] or previous['finished']
            if following['started']>accepted:
                windows.append(window(previous['step']+' → '+following['step'],'between_steps',accepted,following['started'],layer=layer))
    feature_first=min(s['started'] for s in report['steps'] if s['layer']=='Feature')
    task_first=min(s['started'] for s in report['steps'] if s['layer']=='Task')
    implementation=next(s for s in report['steps'] if s['layer']=='Feature' and s['step']=='implementation')
    windows += [window('首次派发','initial_dispatch',start,feature_first),
        window('创建 Task 并首次派发','task_dispatch',implementation['started'],task_first)]
    for milestone in report['completion_milestones']:
        layer='Feature' if milestone['instance']==report['root'] else 'Task'
        windows.append(window(layer+' 最后生产者结束 → 显式完成','completion',
            milestone['last_step_finished_at_ms'],milestone['workflow_completed_at_ms'],layer=layer))
    windows.append(window('根完成 → 经理关闭及 worker 静止','settlement',report['workflow_completed_at_ms'],finish))
    summary={}
    for kind in {w['kind'] for w in windows}:
        group=[w for w in windows if w['kind']==kind]
        intervals=[(w['started'],w['finished']) for w in group]
        values=allocations(slices,intervals)
        summary[kind]={'count':len(group),'wall_union_ms':union_ms(intervals),
            'net_union_ms':sum(v for k,v in values.items() if k!='external'),'allocation_ms':values}
    tool_groups={}
    for call in report['platform_calls']:
        method=call['handler']
        group='git' if method.startswith('git.') else 'tests' if method.startswith('tests.') else 'workspace' if method.startswith('workspace.') else 'platform_other'
        tool_groups.setdefault(group,[]).append((call['started'],call['finished']))
    for tool in tools:
        tool_groups.setdefault(tool['group'],[]).append((tool['started'],tool['finished']))
    ordered=[(name,tool_groups.get(name,[])) for name in ('git','tests','workspace','platform_other','shell','controller','native_platform_bridge')]
    tool_slices=partition(start,finish,[('external',excluded),*ordered])
    tool_totals=allocations(tool_slices,[(start,finish)])
    control_bridge=tool_totals.get('controller',0)+tool_totals.get('native_platform_bridge',0)
    components={'agent':totals['agent'],'business_tools':totals['tools']-control_bridge,
                'workflow':totals['lifecycle']+totals['internal']+control_bridge}
    assert sum(components.values())==audit['workflow_net_ms']
    for window_row in windows:
        values=window_row['allocation_ms']
        tool_values=allocations(tool_slices,[(window_row['started'],window_row['finished'])])
        control=tool_values.get('controller',0)+tool_values.get('native_platform_bridge',0)
        window_row['component_ms']={'agent':values.get('agent',0),
            'business_tools':values.get('tools',0)-control,
            'workflow':values.get('lifecycle',0)+values.get('internal',0)+control}
        assert sum(window_row['component_ms'].values())==window_row['net_ms']
    role_slices=partition(start,finish,[('external',excluded),('tools',tool_intervals),
        ('producer',[(n['started'],n['finished']) for n in native_rows if n['role']=='producer']),
        ('manager',[(n['started'],n['finished']) for n in native_rows if n['role']=='manager'])])
    role_totals=allocations(role_slices,[(start,finish)])
    assert role_totals.get('producer',0)+role_totals.get('manager',0)==totals['agent']
    for native_turn in native_rows:
        methods=[call['method'] for call in controller_calls if call['turn_id']==native_turn['turn']]
        native_turn['controller_methods']=methods
        native_turn['exclusive_agent_ms']=allocations(role_slices,[(native_turn['started'],native_turn['finished'])]).get(native_turn['role'],0)
    wait_only=[n for n in native_rows if n['role']=='manager' and n['controller_methods']==['workflow.controller.wait']]
    wait_only_stats={'count':len(wait_only),
        'observed_service_aggregate_ms':sum(n['finished']-n['started'] for n in wait_only),
        'exclusive_agent_ms':sum(n['exclusive_agent_ms'] for n in wait_only),
        'turns':[n['turn'] for n in wait_only]}
    return {'schema_version':1,'root':report['root'],'project':report['project'],
        'started_at_ms':start,'completed_at_ms':finish,'workflow_net_ms':audit['workflow_net_ms'],
        'allocation_ms':totals,'runtime_and_internal_ms':totals['lifecycle']+totals['internal'],
        'component_allocation_ms':components,
        'tool_allocation_ms':{k:v for k,v in tool_totals.items() if k not in ('external','internal')},
        'agent_role_allocation_ms':{k:v for k,v in role_totals.items() if k in ('producer','manager')},
        'turn_counts':dict(Counter(n['role'] for n in native_rows)),'manager_wait_only':wait_only_stats,
        'native_tools':tools,'native_turns':native_rows,'clock_anomalies':anomalies,
        'phase_windows':sorted(windows,key=lambda w:w['started']),'phase_summary':summary,'slices':slices,
        'notes':['All attribution boundaries use persisted Host observation times; producer clock fields are not used.',
            'Observed Agent envelope is admission to terminal; excluding tools still includes provider/transport and Host polling latency, not pure model inference.',
            'A tool span includes request/receipt handling observed by Host. Same-batch tool boundaries can be zero; producer duration is diagnostic only.',
            'Wall ownership priority: external, all tools, observed Agent service, kernel turn lifecycle, remaining internal/unattributed.',
            'The three-component ledger assigns controller API calls and native platform request/reply bridging to workflow; business tools retain Git, tests, shell, workspace and other platform handlers.',
            'Runtime lifecycle includes worker/container setup, dispatch, polling and cleanup. Remaining internal time is not a measurement of scheduler CPU.',
            'Phase windows (approval preparation, inter-step, completion) overlap component categories; never add phase totals to component totals.',
            'Concurrent Agent/tools use explicit ownership priority. Time allocation is not a causal critical path or a guaranteed speedup estimate.']}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('benchmark',type=Path)
    parser.add_argument('--data',required=True,type=Path)
    parser.add_argument('--report',required=True,type=Path)
    args=parser.parse_args()
    body=args.benchmark.read_bytes();benchmark=json.loads(body)
    result=analyze(benchmark,args.data/'projects'/benchmark['project'])
    result['benchmark_sha256']=hashlib.sha256(body).hexdigest()
    args.report.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ('component_allocation_ms','allocation_ms','runtime_and_internal_ms','tool_allocation_ms','agent_role_allocation_ms','phase_summary')},ensure_ascii=False,indent=2))
