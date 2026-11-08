#!/usr/bin/env python3
"""Native Task DAG -> actual Docker Red/Green -> immutable Delivery + independent native Review."""
from client import Client
from pathlib import Path
import argparse,json,subprocess
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--output',type=Path,default=Path('docs/plans/real-engineering-integration/R6-implementation.json'));p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));a=p.parse_args();a.output=a.reports/a.output.name if a.output.parent==Path('docs/plans/real-engineering-integration') else a.output
source=json.loads((a.reports/'R5-planning.json').read_text());assert source['passed'];c=Client(a.url);c.open(source['project_id']);fid=source['feature_id']
r=json.loads(a.output.read_text()) if a.output.exists() else {'project_id':source['project_id'],'feature_id':fid,'tasks':{},'passed':False}
if 'failure' in r:
 r.setdefault('previous_failures',[]).append({'stage':r.get('stage'),'failure_type':r.pop('failure_type',None),'failure':r.pop('failure')})
r['passed']=False
def save(stage):r['stage']=stage;a.output.write_text(json.dumps(r,indent=2,ensure_ascii=False)+'\n');print(stage,flush=True)
def view():return c.feature(fid)
try:
 for key,provider in [('add','codex'),('multiply','dsh')]:
  if key in r['tasks']:continue
  tid=source['tasks'][key];v=view();task=next(t for t in v['tasks'] if t['id']==tid)
  receipt=c.command('attempt.start',tid,task['revision'],{'provider':provider});r['tasks'][key]={'id':tid,'attempt':receipt['id'],'provider':provider};save('started_'+key)
 for key in ['add','multiply']:
  row=r['tasks'][key];aid=row['attempt'];save('waiting_'+key)
  def done(v):
   runs=c.attempt_runs(v,aid)
   failures=[x for x in runs if x['status'] in ['failed','interrupted','cancelled']]
   if failures:raise AssertionError(json.dumps([{'id':x['id'],'node':x['node'],'result':x['result']} for x in failures],ensure_ascii=False))
   return bool(runs) and all(x['status']=='succeeded' for x in runs)
  v=c.wait(view,done,900);runs=c.attempt_runs(v,aid)
  red=next(x for x in runs if x['node']=='red')['result'];green=next(x for x in runs if x['node']=='green')['result'];verify=next(x for x in runs if x['node']=='verify')['result'];code=next(x for x in runs if x['node']=='implement')['result']['code_revision'];assert red['assertion_failed'] and red['expected_failure'] and red['exit_code']!=0
  assert green['candidate']==verify['candidate']==code and green['exit_code']==verify['exit_code']==0
  assert red['candidate']!=code and all(x['origin']=='external' for x in runs)
  row['evidence']=[{'id':x['id'],'node':x['node'],'result_artifact':x['result_artifact'],'result':x['result']} for x in runs]
  attempt=next(x for x in v['attempts'] if x['id']==aid);row['baseline']=attempt['baseline'];row['candidate']=code
  d=next((x for x in v['deliveries'] if x.get('attempt_id')==aid and x['kind']=='implementation'),None)
  if d is None:
   did=c.command('delivery.submit',aid,attempt['revision'])['id'];v=view();d=next(x for x in v['deliveries'] if x['id']==did)
  row['delivery']=d['id'];save('delivery_'+key)
  if d['id'] not in v['accepted_deliveries']:
   review=next((x for x in c.latest_runs(v['runs']) if x['id']==row['review_run']),None) if row.get('review_run') else next((x for x in c.latest_runs(v['runs']) if x['node']=='review:'+d['id']),None)
   rid=review['id'] if review else c.command('review.request',d['id'],payload={'provider':row.get('review_provider',row['provider'])})['id']
   def reviewed(v):
    run=next(x for x in v['runs'] if x['id']==rid)
    if run['status'] in ['failed','interrupted','cancelled']:raise AssertionError(str(run['result']))
    return run['status']=='succeeded'
   v=c.wait(view,reviewed,400);assert d['id'] in v['accepted_deliveries'],'Independent Reviewer requested changes'
  row['review']=next(x for x in reversed(v['reviews']) if x['delivery_id']==d['id'] and x['decision']=='pass')['id'];save('accepted_'+key)
 r['passed']=True;save('finished')
except Exception as e:r['failure_type']=type(e).__name__;r['failure']=str(e);save('failed');raise
