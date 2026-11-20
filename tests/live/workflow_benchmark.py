"""Export only timing/identity/count metadata from the observed run, read-only."""
import argparse, json, sqlite3, subprocess, platform, statistics, time, sys, shutil, hashlib
from datetime import datetime
from pathlib import Path
from client import Client
from workflow_observer import read_workflow_tree

from verify_project_regression import verify
from workflow_timing import approval_windows, suspension_windows, audit

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', required=True)
parser.add_argument('--data', required=True, type=Path)
parser.add_argument('--project', required=True)
parser.add_argument('--root', required=True)
parser.add_argument('--report', required=True, type=Path)
parser.add_argument('--host-power-log', type=Path, help='Explicit local OS sleep/wake evidence; never infer suspension from idle time alone')
parser.add_argument('--host-metadata', type=Path, help='Launcher receipt for binary identity, profile and cache mode')
args = parser.parse_args()
DATA, PROJECT, ROOT = args.data.resolve(), args.project, args.root
client = Client(args.url, PROJECT)
acceptance = verify(client, ROOT, DATA, unassisted=True)
environment = {'platform': platform.platform(), 'processor': platform.processor(),
    'notes': ['Post-completion local HTTP samples; one authenticated client; sequential reads.'], 'api': {}}
if args.host_metadata:
    metadata=json.loads(args.host_metadata.read_text())
    environment['host']={key:metadata[key] for key in ('binary_sha256','profile','cross_project_cache_shared')}
if sys.platform == 'darwin':
    environment['cpu'] = subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'], text=True).strip()
    environment['memory_bytes'] = int(subprocess.check_output(['sysctl','-n','hw.memsize'], text=True))
for method, params in [('query.workflow_instance', {'id':ROOT,'view':'status'}),
                       ('query.orchestration', {'root_instance_id':ROOT})]:
    samples=[]
    for _ in range(20):
        began=time.perf_counter();value=client.rpc(method,params)
        samples.append({'ms':round((time.perf_counter()-began)*1000,3),
                        'response_json_bytes':len(json.dumps(value,ensure_ascii=False,separators=(',',':')).encode())})
    durations=sorted(s['ms'] for s in samples)
    environment['api'][method]={'samples':samples,'median_ms':statistics.median(durations),
                               'p95_ms':durations[18],'max_ms':durations[-1]}
tree = read_workflow_tree(client, ROOT)
directory = DATA / 'projects' / PROJECT
repository = directory / 'repository'
browser_results = []
for member in acceptance['instances']:
    if member['independent_tests'] is None:continue
    revision=member['regressions'][0]['candidate']
    listing=subprocess.check_output(['git','-C',str(repository),'ls-tree','-rlz',revision])
    for entry in listing.split(b'\0'):
        if not entry:continue
        header,path=entry.split(b'\t',1);size=header.split()[-1]
        if not path.endswith(b'/result.json') or size==b'-' or int(size)>1024*1024:continue
        try:result=json.loads(subprocess.check_output(['git','-C',str(repository),'show',revision+':'+path.decode()]))
        except (ValueError,UnicodeDecodeError):continue
        if not isinstance(result,dict) or result.get('schemaVersion')!=1 or result.get('status')!='passed' or not result.get('entrySha256'):continue
        for file,digest in [('index.html',result['source']['sha256']),('test/browser-acceptance.cjs',result['entrySha256'])]:
            blob=subprocess.check_output(['git','-C',str(repository),'show',acceptance['mainline']+':'+file])
            if hashlib.sha256(blob).hexdigest()!=digest:raise AssertionError('Browser evidence differs from delivered '+file)
        duration=(datetime.fromisoformat(result['finishedAt'])-datetime.fromisoformat(result['startedAt'])).total_seconds()*1000
        browser_results.append({'revision':revision,'path':path.decode(),'duration_ms':round(duration),
            **{k:result.get(k) for k in ('status','startedAt','finishedAt','browser','playwright','node','entrySha256','records')},
            'source_sha256':result['source']['sha256']})
space=shutil.disk_usage(directory)
environment['data_volume']={'total_bytes':space.total,'free_bytes':space.free}
runtime_cache=directory/'execution/runtime-cache'/PROJECT
if runtime_cache.is_dir():
    environment['external_project_cache_bytes']=int(subprocess.check_output(['du','-sk',str(runtime_cache)],text=True).split()[0])*1024
db = sqlite3.connect((directory / 'engineering.sqlite3').as_uri() + '?mode=ro', uri=True)
records = [(i,t,d,json.loads(s),ts) for i,t,d,s,ts in db.execute(
    'SELECT id,type,definition_id,data,created_at FROM governance_records')]
members = set(tree)
steps = []
for identity, view in tree.items():
    for s in view['runtime']['steps']:
        definition = next(item for item in view['plan']['steps'] if item['node_id']==s['node_id'])
        steps.append({**{k:s.get(k) for k in ('id','step','status','session_id','turn_id','started','finished','round','sequence','budget_waited_ms')},
                      'name':definition.get('name',s['step']), 'plan_stage':definition.get('stage'),
                      'instance':identity,'layer':'Feature' if identity==ROOT else 'Task'})
historical = [d for _,t,_,d,_ in records if t=='workflow_execution_history' and d.get('workflow_instance_id') in members and d.get('started')]
step_ids = {s['id'] for s in steps} | {s['id'] for s in historical}
waits = [{'execution':definition,**d} for _,t,definition,d,_ in records if t=='workflow_wait_interval' and definition in step_ids]
confirmations = [{'record_id':i,'request_id':d.get('request_id'),'execution':d['execution_id'],'confirmed_at':ts,'actor':d['actor']['kind']} for i,t,_,d,ts in records if t=='workflow_confirmation' and d['execution_id'] in step_ids]
calls = [{k:d.get(k) for k in ('handler','status','started','finished','root_instance_id')} | {'id':i}
         for i,t,_,d,_ in records if t=='platform_call' and d.get('root_instance_id')==ROOT]
controller = next(d for _,t,_,d,_ in records if t=='workflow_controller' and d.get('root_instance_id')==ROOT)
session_ids = {s['session_id'] for s in steps+historical if s.get('session_id')} | {controller['session_id']}
turns = [{**dict(zip(('id','session','status','started','finished'),row))} for row in db.execute(
    'SELECT id,session_id,status,started,finished FROM turns') if row[1] in session_ids]
executions = [{**dict(zip(('id','session','kind','status','image','started','finished','writer_revoked'),row[:8])),
               'permission_selection':{k:v for k,v in json.loads(row[8] or '{}').items() if k in ('harness_choice','sandbox_choice')}} for row in db.execute(
    'SELECT id,session_id,kind,status,image_digest,started,finished,writer_revoked,permissions FROM executions') if row[1] in session_ids]
usage = {}
commands = {}
models = {}
for sid in session_ids:
    for path in (directory/'execution/sessions'/sid/'coordinator').glob('*.jsonl'):
        for line in path.open():
            frame = json.loads(line).get('frame',{})
            if frame.get('method') in ('thread/start','thread/resume','turn/start'):
                selection={k:v for k,v in frame.get('params',{}).items() if k in ('model','effort','modelProvider') and v is not None}
                if selection:models.setdefault(sid,{}).update(selection)
            item=frame.get('params',{}).get('item',{})
            if item.get('type')=='commandExecution' and frame.get('method') in ('item/started','item/completed'):
                key=(sid,item['id'])
                command=commands.setdefault(key,{'session':sid,'item':item['id']})
                command['started' if frame['method']=='item/started' else 'finished']=frame.get('emittedAtMs')
                if frame['method']=='item/completed':
                    command.update(exit_code=item.get('exitCode'),duration_ms=item.get('durationMs'),status=item.get('status'))
            if frame.get('method')!='thread/tokenUsage/updated':continue
            p = frame['params']; key=p['threadId']; at=frame.get('emittedAtMs',0)
            if key not in usage or at>=usage[key]['at']:
                usage[key]={'at':at,'session':sid,'thread':key,'total':p['tokenUsage']['total']}
created = next(ts for i,t,_,_,ts in records if i==ROOT and t=='workflow_instance')
completion = db.execute("SELECT created_at FROM governance_records WHERE type='workflow_completion' AND definition_id=?",(ROOT,)).fetchone()
if not completion:raise RuntimeError('Run is not complete')
workflow_finished = completion[0]
manager_event = db.execute('SELECT max(time) FROM events WHERE target=?',(next(i for i,t,_,d,_ in records if t=='workflow_controller' and d.get('root_instance_id')==ROOT),)).fetchone()[0]
workers_finished = max((e['finished'] or 0 for e in executions), default=0)
turns_finished = max((t['finished'] or 0 for t in turns), default=0)
finished = max(workflow_finished, manager_event or 0, workers_finished, turns_finished)
def duration(row):
    return max(0,(row.get('finished') or finished)-(row.get('started') or finished))
def union(intervals):
    ordered=sorted((max(created,a),min(finished,b)) for a,b in intervals if b>a)
    total=0;end=created
    for a,b in ordered:
        total+=max(0,b-max(end,a));end=max(end,b)
    return total
human_defs={s['node_id'] for s in tree[ROOT]['plan']['steps'] if s.get('confirmation')=='human'}
human_steps={s['id']:s for s in tree[ROOT]['runtime']['steps'] if s['node_id'] in human_defs}
human_intervals=[(human_steps[c['execution']]['finished'],c['confirmed_at']) for c in confirmations if c['execution'] in human_steps]
approvals=approval_windows(human_steps,confirmations,records)
event_times=[row[0] for row in db.execute('SELECT time FROM events WHERE time BETWEEN ? AND ?',(created,finished))]
suspensions=suspension_windows(args.host_power_log.read_text() if args.host_power_log else '',event_times,created,finished)
timing_audit=audit(created,finished,approvals,suspensions)
controller_id=next(i for i,t,_,d,_ in records if t=='workflow_controller' and d.get('root_instance_id')==ROOT)
wakeups=[]
turn_by_id={turn['id']:turn for turn in turns}
for delivery,turn_id,queued in db.execute('SELECT id,turn_id,created_at FROM orchestration_wakeups WHERE controller_id=?',(controller_id,)):
    turn=turn_by_id.get(turn_id)
    if turn and turn['started']:
        wakeups.append({'delivery':delivery,'turn_id':turn_id,'queued_at_ms':queued,
            'started_at_ms':turn['started'],'queue_ms':turn['started']-queued})
segment_events=[]
for event_id,raw,at in db.execute("SELECT id,detail,time FROM events WHERE kind='workflow.runtime' AND time BETWEEN ? AND ?",(created,finished)):
    detail=json.loads(raw)
    if detail.get('root_instance_id')==ROOT and detail.get('change',{}).get('kind')=='segment.changed':
        segment_events.append({'event_id':event_id,'at_ms':at,'instance':detail['instance_id'],'change':detail['change']})
for step in steps:
    accepted=[ts for _,kind,_,data,ts in records if kind=='workflow_decision' and data.get('operation')=='accept' and data.get('execution_id')==step['id']]
    signed=[c['confirmed_at'] for c in confirmations if c['execution']==step['id']]
    published=[ts for _,kind,_,data,ts in records if kind=='workflow_output' and data.get('execution_id')==step['id']]
    step['accepted_at_ms']=max(accepted+signed,default=None)
    step['output_published_at_ms']=min(published,default=None)
native_intervals=[(t['started'],t['finished']) for t in turns if t['started'] and t['finished']]
platform_intervals=[(c['started'],c['finished']) for c in calls if c['started'] and c['finished']]
excluded_intervals=[(a['requested_at_ms'],a['signed_at_ms']) for a in approvals]+[(p['started_at_ms'],p['finished_at_ms']) for p in suspensions]
external=union(excluded_intervals)
external_native=union(excluded_intervals+native_intervals)
external_native_platform=union(excluded_intervals+native_intervals+platform_intervals)
timing_audit['net_partition_ms']={'native_turns':external_native-external,
    'platform_outside_native_turns':external_native_platform-external_native,
    'other_internal_or_unattributed':finished-created-external_native_platform}
human=union(human_intervals)
human_native=union(human_intervals+native_intervals)
covered=union(human_intervals+native_intervals+platform_intervals)
completion_milestones=[]
for member in members:
    completed=db.execute("SELECT created_at FROM governance_records WHERE type='workflow_completion' AND definition_id=? ORDER BY created_at DESC LIMIT 1",(member,)).fetchone()[0]
    last=max(s['finished'] or 0 for s in steps if s['instance']==member)
    completion_milestones.append({'instance':member,'last_step_finished_at_ms':last,
        'last_step_accepted_at_ms':max(s['accepted_at_ms'] or 0 for s in steps if s['instance']==member),
        'workflow_completed_at_ms':completed,'completion_gap_ms':completed-last,
        'completion_gap_net_ms':audit(last,completed,[],[
            {**p,'started_at_ms':max(last,p['started_at_ms']),'finished_at_ms':min(completed,p['finished_at_ms'])}
            for p in suspensions if p['finished_at_ms']>last and p['started_at_ms']<completed])['workflow_net_ms']})
wait_summary={}
for reason in {w['reason'] for w in waits}:
    group=[w for w in waits if w['reason']==reason]
    wait_summary[reason]={'count':len(group),'union_ms':union([(w['started'],w['finished'] or finished) for w in group]),
                         'aggregate_ms':sum((w['finished'] or finished)-w['started'] for w in group)}
attempts = [{k:s.get(k) for k in ('id','step','workflow_instance_id','status','session_id','turn_id','started','finished','round','sequence')} for s in historical]
report={'schema_version':2,'root':ROOT,'project':PROJECT,'started_at_ms':created,'completed_at_ms':finished,
    'workflow_completed_at_ms':workflow_finished,'manager_closed_at_ms':manager_event,
    'workers_finished_at_ms':workers_finished,'settlement_overhead_ms':finished-workflow_finished,
    'wall_ms':finished-created,'human_review_wait_ms':union(human_intervals),
    'wall_partition_ms':{'human_review':human,'native_outside_human_wait':human_native-human,
                         'platform_outside_human_and_native':covered-human_native,
                         'other_wait_or_unattributed':finished-created-covered},
    'timing_audit':timing_audit,'manager_wakeups':wakeups,'segment_events':segment_events,
    'completion_milestones':completion_milestones,'wait_summary':wait_summary,
    'non_human_wait_wall_ms':finished-created-union(human_intervals),
    'steps':steps,'superseded_attempts':attempts,'wait_intervals':waits,'human_confirmations':confirmations,
    'platform_calls':calls,'turns':turns,'executions':executions,'native_models':models,
    'native_commands':list(commands.values()),'token_usage':list(usage.values()),
    'token_totals':{k:sum(u['total'].get(k,0) for u in usage.values()) for k in ('inputTokens','cachedInputTokens','cacheWriteInputTokens','outputTokens','reasoningOutputTokens','totalTokens')},
    'aggregate_turn_ms':sum(duration(t) for t in turns),'aggregate_platform_call_ms':sum(duration(c) for c in calls),
    'active_turn_union_ms':union([(t['started'],t['finished']) for t in turns if t['started'] and t['finished']]),
    'environment':environment,'acceptance':acceptance,'browser_results':browser_results,
    'notes':['Single warm-cache real-model run, not a statistically stable throughput estimate.',
             'Feature implementation includes its nested Task: step durations are not additive.',
             'Turn durations include model/tool/dispatch I/O; not pure model inference latency.',
             'Legacy human_review_wait_ms includes manager preparation; use timing_audit for the corrected net workflow cost.',
             'Non-Human wall time is a legacy diagnostic that still includes host sleep; not the corrected benchmark.',
             'Wall partitions use Human, then native turn, then platform call priority to avoid double counting. Residual time includes queueing, orchestration waits and intervals not covered by these observations.',
             'Token totals use final cumulative usage per native thread; cached/reasoning tokens are subsets.']}
for s in report['steps']:s['duration_ms']=duration(s)
for c in report['platform_calls']:c['duration_ms']=duration(c)
for t in report['turns']:t['duration_ms']=duration(t)
out=args.report;out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
db.close()
print(out)
