#!/usr/bin/env python3
"""Explicit real acceptance runner. Existing-evidence verification is named as such.
No credential provisioning, implicit repository creation, or skipped success.
"""
from client import Client
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import argparse,hashlib,json,os,platform,re,subprocess,sys,time,uuid
p=argparse.ArgumentParser();p.add_argument('--providers',default='codex,dsh');p.add_argument('--root',type=Path,required=True);p.add_argument('--github-repo',required=True);p.add_argument('--groups',default='git,docker,harness,github,web,workflow,recovery');p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--reports',type=Path);p.add_argument('--mode',choices=['full','verify-existing'],default='full');p.add_argument('--allow-host-restart',action='store_true');p.add_argument('--resume',action='store_true',help='explicitly resume a failed full report after correcting the failed stage; preserve original report and logs');a=p.parse_args()
assert re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+',a.github_repo),'explicit owner/test-repo required'
project_root=Path(__file__).resolve().parents[2];os.chdir(project_root);a.root=a.root.resolve();assert a.root!=project_root and (a.root/'.devdesk-dataset').is_file(),'root must be a dedicated initialized dataset'
assert a.github_repo!='xfork24/devdesk','never use the DevDesk source repository'
providers=a.providers.split(',');assert providers and set(providers)<=set(['codex','dsh'])
groups=a.groups.split(',');assert groups and set(groups)<=set(['git','docker','harness','github','web','workflow','recovery'])
reports=a.reports or (project_root/'docs/plans/real-engineering-integration' if a.mode=='verify-existing' else project_root/'docs/plans/real-engineering-integration'/('live-'+uuid.uuid4().hex[:10]));reports=reports.resolve();reports.mkdir(parents=True,exist_ok=True)
r={'passed':False,'mode':a.mode,'platform':platform.platform(),'machine':platform.machine(),'providers':providers,'groups':groups,'root':str(a.root),'github_repository':a.github_repo,'steps':[],'started':time.time()};output=reports/('live-check-'+a.mode+'-'+'-'.join(groups)+'.json');previous={};resume_id=uuid.uuid4().hex[:10] if a.resume else None
if a.resume:
 assert a.mode=='full' and output.is_file(),'--resume requires an existing failed full report'
 old=json.loads(output.read_text());assert not old['passed'],'use a fresh report directory after a completed full'
 for key in ['mode','providers','groups','root','github_repository']:assert old[key]==r[key],'resume context mismatch: '+key
 previous={row['name']:row for row in old['steps'] if row['passed']}
 history=reports/'history';history.mkdir(exist_ok=True);archive=history/(output.stem+'-before-'+resume_id+'.json');archive.write_bytes(output.read_bytes());r['resumed_from']=str(archive);r['previous_failure']=old.get('error')
 if 'restarted_host_pid' in old:r['restarted_host_pid']=old['restarted_host_pid']
elif a.mode=='full':assert not output.exists(),'full report already exists; use --resume or a fresh report directory'
elif output.exists():
 history=reports/'history';history.mkdir(exist_ok=True);archive=history/(output.stem+'-'+uuid.uuid4().hex[:10]+'.json');archive.write_bytes(output.read_bytes());r['previous_verification']=str(archive)
c=Client(a.url)
def save():output.write_text(json.dumps(r,indent=2)+'\n')
def step(name,fn):
 # Reuse only successful side effects, never the final persisted/remote checks.
 # The original bytes remain in history and every reused command log is hashed.
 if name in previous and (name.startswith('real-') or name.startswith('native-') or name in ['browser-requirement','repository','planning','implementation','merges','lifecycle','crash-arm','owned-host-crash-and-restart','crash-check']):
  row=json.loads(json.dumps(previous[name]));evidence=row.get('evidence',{})
  if 'log' in evidence:assert hashlib.sha256(Path(evidence['log']).read_bytes()).hexdigest()==evidence['sha256'],'recorded log changed: '+name
  row['reused_from']=r['resumed_from'];r['steps'].append(row);save();print(name+' (recorded successful execution; persisted facts are rechecked below)',flush=True);return
 row={'name':name,'passed':False,'started':time.time()};r['steps'].append(row);save();print(name,flush=True)
 try:row['evidence']=fn();row['passed']=True
 except Exception as e:row['error']=str(e);raise
 finally:row['finished']=time.time();save()
def run(name,cmd,env=None):
 log=reports/(name+('-resume-'+resume_id if resume_id else '')+'.log')
 with log.open('w') as stream:
  result=subprocess.run(cmd,stdout=stream,stderr=subprocess.STDOUT,env=env,timeout=3600)
 assert result.returncode==0,f'{name} failed, see {log}'
 return {'log':str(log),'sha256':hashlib.sha256(log.read_bytes()).hexdigest(),'command':cmd}
def read(name):
 v=json.loads((reports/name).read_text());assert v['passed'],name+' did not pass';return v
try:
 view=c.rpc('query.workspace');assert Path(view['data_root']).resolve()==a.root,'HTTP host uses a different dataset'
 if a.mode=='full':
  if 'git' in groups:step('real-git',lambda:run('real-git',['cargo','test','--locked','-p','devdesk-adapters','--test','git_resources']))
  if 'docker' in groups:step('real-docker',lambda:run('real-docker',['cargo','test','--locked','-p','devdesk-adapters','--test','command_tests','--test','docker_resources','--','--include-ignored']))
  if 'harness' in groups:
   for provider in providers:step('native-'+provider,lambda provider=provider:run('native-'+provider,['python3','tests/live/probe_application_conversations.py','--url',a.url,'--provider',provider,'--output',str(reports/('R3-'+provider+'-application.json'))]))
  if any(g in groups for g in ['web','workflow']):
   assert set(providers)==set(['codex','dsh']) and 'harness' in groups,'full workflow requires fresh both-provider discussion inputs'
   env=dict(os.environ,DEVDESK_LIVE_REPORTS=str(reports),DEVDESK_LIVE_URL=a.url)
   step('browser-requirement',lambda:run('browser-requirement',['node','tests/live/browser_conversation.mjs'],env))
   step('repository',lambda:run('repository',['python3','tests/live/probe_repository.py','--url',a.url,'--reports',str(reports),'--github-repo',a.github_repo]))
  if 'workflow' in groups:
   prefix='live-'+uuid.uuid4().hex[:10]+'-'
   for name,args in [('planning',['--case-prefix',prefix]),('implementation',[]),('merges',['--browser-final'])]:step(name,lambda name=name,args=args:run(name,['python3','tests/live/probe_'+name+'.py','--url',a.url,'--reports',str(reports)]+args))
  if 'recovery' in groups:
   assert a.allow_host_restart,'recovery requires --allow-host-restart for this dedicated host only'
   step('lifecycle',lambda:run('lifecycle',['python3','tests/live/probe_recovery.py','--url',a.url,'--reports',str(reports),'--phase','exercise']))
   step('crash-arm',lambda:run('crash-arm',['python3','tests/live/probe_recovery.py','--url',a.url,'--reports',str(reports),'--phase','crash-arm']))
   def crash():
    global c
    port=a.url.rsplit(':',1)[1];lines=subprocess.check_output(['ps','-axo','pid=,args='],text=True).splitlines();matches=[]
    import shlex,signal
    for line in lines:
     parts=line.strip().split(None,1)
     if len(parts)!=2:continue
     argv=shlex.split(parts[1])
     if not argv or Path(argv[0]).name!='devdesk-host' or '--data' not in argv or '--port' not in argv:continue
     if Path(argv[argv.index('--data')+1]).resolve()==a.root and argv[argv.index('--port')+1]==port:matches.append((int(parts[0]),argv))
    assert len(matches)==1,'cannot prove a unique dedicated host identity';pid,argv=matches[0];os.kill(pid,signal.SIGKILL)
    time.sleep(.5)
    with (reports/'restarted-host.log').open('w') as log:child=subprocess.Popen(argv,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    c=Client(a.url);assert Path(c.rpc('query.workspace')['data_root']).resolve()==a.root
    r['restarted_host_pid']=child.pid;return {'killed_owned_pid':pid,'new_pid':child.pid,'dataset':str(a.root),'local_authentication_refreshed':True}
   step('owned-host-crash-and-restart',crash)
   step('crash-check',lambda:run('crash-check',['python3','tests/live/probe_recovery.py','--url',a.url,'--reports',str(reports),'--phase','crash-check']))
 # Verify persisted facts separately from generating new model work.
 if 'harness' in groups:
  def harness():
   evidence=[]
   for provider in providers:
    report=read('R3-'+provider+'-application.json');c.open(report['project_id']);s=c.rpc('query.session',{'id':report['session_id'],'limit':100});assert s['session']['external_session']==report['native_session']
    for row in report['turns']:
     t=next(t for t in s['turns'] if t['id']==row['id']);assert t['status']=='completed' and t['consumed']
    assert s['session']['runtime_binding']['installation']['key']['provider']==provider;evidence.append({'provider':provider,'session':report['session_id'],'turns':[t['id'] for t in report['turns']]})
   return evidence
  step('persisted-native-facts',harness)
 if 'workflow' in groups or (a.mode=='verify-existing' and 'git' in groups):
  def workflow():
   source=read('R7-merges.json');c.open(source['project_id']);v=c.feature(source['feature_id']);assert v['feature']['status']=='completed';repo=c.rpc('query.repository',{'limit':100});assert repo['project']['workspace_definition']['repository']==a.github_repo;workspace=repo['project']['workspace'];checked=[]
   for key in ['add','multiply','mainline']:
    m=next(m for m in v['merges'] if m['id']==source['merges'][key]['id']);assert m['status']=='completed' and m['candidate']==m['committed_revision'];parents=subprocess.check_output(['git','-C',workspace,'rev-list','--parents','-n','1',m['candidate']],text=True).split();assert parents==[m['candidate'],m['expected_target']];checked.append({'id':m['id'],'candidate':m['candidate'],'parent':m['expected_target']})
   closure=next(d for d in v['deliveries'] if d['id']==source['closure_delivery']);assert len(closure['source_deliveries'])==2;assert closure['id'] in v['accepted_deliveries'];assert all(t['status']=='completed' for t in v['tasks']);return {'merges':checked,'closure_sources':closure['source_deliveries'],'snapshots':source['final_snapshots']}
  step('persisted-complete-feature',workflow)
 if 'github' in groups:
  def github():
   if not (reports/'R7-merges.json').is_file():
    policy=c.rpc('github.diagnose',{'repository':a.github_repo});assert policy['compatible'];return {'policy':policy,'scope':'repository compatibility only; remote delivery requires the workflow group'}
   source=read('R7-merges.json');rows=[]
   for key in ['add','multiply','mainline']:
    url=source['merges'][key]['pull_request'];number=url.rsplit('/',1)[1];pr=json.loads(subprocess.check_output(['gh','api',f'repos/{a.github_repo}/pulls/{number}'],text=True));assert pr['merged'] and pr['head']['sha']==source['merges'][key]['candidate']==pr['merge_commit_sha'];rows.append({'url':url,'candidate':pr['head']['sha'],'merged_at':pr['merged_at']})
   policy=c.rpc('github.diagnose',{'repository':a.github_repo});assert policy['compatible'];return rows
  step('remote-github-facts',github)
 if 'docker' in groups or 'workflow' in groups:
  def artifacts():
   # Hash every committed Artifact, not orphan temporary files. Never print body/protocol content.
   import sqlite3
   total=0;executions=0;never_started=[]
   for db in (a.root/'projects').glob('*/engineering.sqlite3'):
    conn=sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True);conn.row_factory=sqlite3.Row
    for row in conn.execute('select * from artifacts'):
     relative=Path(row['relative_path']);assert not relative.is_absolute() and '..' not in relative.parts;path=(db.parent/relative).resolve();assert path.is_relative_to(db.parent.resolve());assert hashlib.sha256(path.read_bytes()).hexdigest()==row['sha256'];total+=1
    for row in conn.execute('select * from executions'):
     if row['container_id'] is None and row['writer_revoked'] and row['status']=='interrupted':
      never_started.append(row['id']);continue
     assert row['writer_revoked'] and row['protocol_artifact'] and row['status']=='released',('unreleased execution',row['id']);executions+=1
    conn.close()
   return {'verified_artifacts':total,'stopped_exported_released_executions':executions,'failed_before_worker_receipt':never_started}
  step('artifact-hashes-and-execution-release',artifacts)
 if 'web' in groups:
  def web():
   for name in ['R5-browser-proposal.json']+(['R7-browser-final.json'] if 'workflow' in groups else []):
    read(name);assert (reports/name.replace('.json','.png')).is_file()
   return {'requirement':'R5-browser-proposal.json','final_acceptance':'R7-browser-final.json','scope':'recorded real Chromium actions, backend facts and screenshots; not a new UI run'}
  step('browser-evidence',web)
 if 'recovery' in groups:step('recovery-evidence',lambda:read('R8-recovery.json'))
 r['passed']=True;r['finished']=time.time();save();print(json.dumps({'passed':True,'mode':a.mode,'report':str(output)}))
except Exception as e:r['error']=str(e);r['finished']=time.time();save();raise
