#!/usr/bin/env python3
"""Real candidate PRs, stale-target rejection, exact CAS/lease, Feature closure and mainline delivery."""
from client import Client
from pathlib import Path
import argparse,json,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--output',type=Path,default=Path('docs/plans/real-engineering-integration/R7-merges.json'));p.add_argument('--browser-final',action='store_true');p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));a=p.parse_args();a.output=a.reports/a.output.name if a.output.parent==Path('docs/plans/real-engineering-integration') else a.output
source=json.loads((a.reports/'R6-implementation.json').read_text());assert source['passed'];c=Client(a.url);c.open(source['project_id']);fid=source['feature_id']
r=json.loads(a.output.read_text()) if a.output.exists() else {'project_id':source['project_id'],'feature_id':fid,'merges':{},'passed':False}
def save(stage):r['stage']=stage;a.output.write_text(json.dumps(r,indent=2,ensure_ascii=False)+'\n');print(stage,flush=True)
def view():return c.feature(fid)
def revision(owner):
 v=view();return next(x for x in v['tasks']+v['merges']+[v['feature']] if x['id']==owner)['revision']
def request(key,owner,provider):
 if key not in r['merges']:r['merges'][key]={'id':c.command('merge.request',owner,revision(owner),{'provider':provider})['id'],'owner':owner,'provider':provider};save('requested_'+key)
 return r['merges'][key]['id']
def ready(mid):
 def done(v):
  m=next(x for x in v['merges'] if x['id']==mid)
  failed=[x for x in v['runs'] if x.get('merge_id')==mid and x['status'] in ['failed','cancelled','interrupted']]
  if failed:raise AssertionError('candidate verification failed '+json.dumps(failed))
  repo=c.rpc('query.repository',{'limit':100});unknown=[op for op in repo['operations'] if op['status']=='needs_reconciliation' and op['action'].get('merge')==mid]
  if unknown:raise AssertionError('repository operation unresolved '+str([(op['action']['kind'],op['reason'])for op in unknown]))
  return m['status'] in ['waiting_review','accepted','completed'] and m.get('change_request',{}).get('provider')=='github'
 return c.wait(view,done,300)
def review(mid,provider):
 v=ready(mid);m=next(x for x in v['merges'] if x['id']==mid)
 if m['status'] in ['accepted','completed']:return v
 d=next(x for x in v['deliveries'] if x.get('merge_id')==mid)
 existing=next((x for x in v['runs'] if x['node']=='review:'+d['id']),None);rid=existing['id'] if existing else c.command('review.request',d['id'],payload={'provider':provider})['id']
 def done(v):
  x=next(x for x in v['runs'] if x['id']==rid)
  if x['status'] in ['failed','interrupted','cancelled']:raise AssertionError('review failed '+str(x['result']))
  return x['status']=='succeeded'
 v=c.wait(view,done,400);assert d['id'] in v['accepted_deliveries'],'Reviewer requested changes';return v
def apply(key):
 row=r['merges'][key];mid=row['id'];v=review(mid,row['provider']);m=next(x for x in v['merges'] if x['id']==mid)
 if m['status']!='completed':
  if key=='mainline' and a.browser_final:
   output=a.output.parent/'R7-browser-final.json';subprocess.run(['node','tests/live/browser_accept.mjs','--url',a.url,'--project',c.project,'--feature',fid,'--merge',mid,'--candidate',m['candidate'],'--output',str(output)],check=True);r['browser_final']=json.loads(output.read_text())
  else:c.command('merge.apply',mid,m['revision'])
 def done(v):
  m=next(x for x in v['merges'] if x['id']==mid)
  if any(i['id']==mid and i['status']=='needs_reconciliation' for i in v.get('advances',[])):raise AssertionError('local CAS outcome needs reconciliation')
  repo=c.rpc('query.repository',{'limit':100});unknown=[op for op in repo['operations'] if op['status']=='needs_reconciliation' and op['action'].get('merge')==mid]
  if unknown:raise AssertionError('remote unresolved '+str([(op['action']['kind'],op['reason'])for op in unknown]))
  return m['status']=='completed'
 v=c.wait(view,done,240);m=next(x for x in v['merges'] if x['id']==mid);assert m['committed_revision']==m['candidate']
 repo=c.rpc('query.repository',{'limit':100});path=repo['project']['workspace'];parents=subprocess.check_output(['git','-C',path,'rev-list','--parents','-n','1',m['candidate']],text=True).strip().split();assert parents==[m['candidate'],m['expected_target']]
 pr=json.loads(subprocess.check_output(['gh','api',f'repos/{m["change_request"]["repository"]}/pulls/{m["change_request"]["external_id"]}'],text=True));assert pr['merged'] and pr['head']['sha']==m['candidate'] and pr['merge_commit_sha']==m['candidate'];row.update(candidate=m['candidate'],expected_target=m['expected_target'],pull_request=pr['html_url'],merged=True);save('merged_'+key)
try:
 if not (r['merges'].get('add',{}).get('merged') and r['merges'].get('multiply',{}).get('merged')):
  first=request('add',source['tasks']['add']['id'],'codex');second=request('multiply_obsolete',source['tasks']['multiply']['id'],'dsh')
  review(first,'codex');review(second,'dsh');v=view();m1=next(x for x in v['merges'] if x['id']==first);m2=next(x for x in v['merges'] if x['id']==second)
  assert m1['expected_target']==m2['expected_target'];apply('add')
  if not r.get('stale_candidate_rejected'):
   try:c.command('merge.apply',second,revision(second))
   except RuntimeError:r['stale_candidate_rejected']=True
   else:raise AssertionError('stale candidate must not advance target')
   save('stale_target_rejected')
  request('multiply',source['tasks']['multiply']['id'],'dsh');apply('multiply')
 save('feature_closure')
 if 'closure_attempt' not in r:r['closure_attempt']=c.command('attempt.start',fid,revision(fid),{'phase':'closure','provider':'codex'})['id'];save('closure_started')
 aid=r['closure_attempt']
 def closure_done(v):
  runs=[x for x in v['runs'] if x.get('attempt_id')==aid and not x['node'].startswith('review:')]
  failed=[x for x in runs if x['status'] in ['failed','cancelled','interrupted']]
  if failed:raise AssertionError('closure failed '+json.dumps(failed))
  return bool(runs) and all(x['status']=='succeeded' for x in runs)
 v=c.wait(view,closure_done,600);attempt=next(x for x in v['attempts'] if x['id']==aid);d=next((x for x in v['deliveries'] if x.get('attempt_id')==aid and x['kind']=='closure'),None)
 if not d:
  did=c.command('delivery.submit',aid,attempt['revision'])['id'];v=view();d=next(x for x in v['deliveries'] if x['id']==did)
 if d['id'] not in v['accepted_deliveries']:
  existing=next((x for x in v['runs'] if x['node']=='review:'+d['id']),None);rid=existing['id'] if existing else c.command('review.request',d['id'],payload={'provider':'codex'})['id'];v=c.wait(view,lambda v:next(x for x in v['runs'] if x['id']==rid)['status'] in ['succeeded','failed','cancelled','interrupted'],400);assert d['id'] in v['accepted_deliveries'],'closure Reviewer requested changes'
 r['closure_delivery']=d['id'];request('mainline',fid,'codex');apply('mainline');v=view();assert v['feature']['status']=='completed';r['final_snapshots']=[s['id'] for s in v['snapshots'] if s['kind'] in ['final','merged']];assert r['final_snapshots'];r['passed']=True;r.pop('failure',None);r.pop('failure_type',None);save('finished')
except Exception as e:r['failure_type']=type(e).__name__;r['failure']=str(e);save('failed');raise
