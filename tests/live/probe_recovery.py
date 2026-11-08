#!/usr/bin/env python3
"""Real provider cancellation, typed questions and consumed-input restart probes.
The crash arm/check phases deliberately leave and inspect owned workers; the caller
must restart only its dedicated acceptance host between those phases.
"""
from client import Client
from pathlib import Path
import argparse,json,time,uuid
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));p.add_argument('--phase',choices=['exercise','crash-arm','crash-check'],default='exercise');a=p.parse_args();a.reports.mkdir(parents=True,exist_ok=True)
f=a.reports/'R8-recovery.json';r=json.loads(f.read_text()) if f.exists() else {'passed':False,'source':'actual HTTP, native Codex/DSH and owned Docker workers'};c=Client(a.url)
def save():f.write_text(json.dumps(r,indent=2)+'\n')
def state(sid):return c.rpc('query.session',{'id':sid,'limit':100})
def send(sid,prompt):
 s=state(sid);return c.command('conversation.send',sid,s['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':prompt})['id']
def turn(s,tid):return next(t for t in s['turns'] if t['id']==tid)
def wait(sid,tid,predicate,timeout=240):return c.wait(lambda:state(sid),lambda s:predicate(s,turn(s,tid)),timeout)
def completed(sid,tid):return wait(sid,tid,lambda s,t:t['status'] in ['completed','failed','cancelled','needs_reconciliation'])
def opened(owner,role='analyst',provider=None,model=None):
 payload={'agent':'builtin-agent-'+role+'-v1'}
 if provider:payload['provider']=provider
 if model:payload['model']=model
 revision=c.rpc('query.workspace')['active_project']['revision']
 return c.command('session.open',owner,revision,payload)['id']
def assert_stopped(s,tid,status):
 t=turn(s,tid);assert t['status']==status,(tid,t['status'])
 ex=s['executions'];assert ex
 assert all(e['writer_revoked'] and e['protocol_artifact'] and e['status']=='released' for e in ex),ex
 return {'turn':tid,'status':t['status'],'native_session':s['session']['external_session'],'executions':[{'id':e['id'],'container':e['container_id'],'protocol_artifact':e['protocol_artifact'],'status':e['status']} for e in ex]}
try:
 if a.phase=='exercise':
  if 'project' not in r:r['project']=c.command('project.create',payload={'title':'Real lifecycle and isolation acceptance'})['id'];save()
  c.open(r['project']);owner=r['project'];sessions=r.setdefault('sessions',{})
  before=c.rpc('query.runtime_installations');assert before['default_harness']=='codex'
  for provider in ([] if r.get('default_switch') else ['codex','dsh','codex']):
   c.rpc('settings.update',{'default_harness':provider});sid=opened(owner);s=state(sid);assert s['session']['runtime_binding']['installation']['key']['provider']==provider
   if provider in sessions:assert s['session']['predecessor'] is not None
   sessions[provider]=sid;save()
  if r.pop('refresh_sessions',False):
   for provider in list(sessions):sessions[provider]=opened(owner,provider=provider)
   save()
  assert state(sessions['dsh'])['session']['runtime_binding']['installation']['key']['provider']=='dsh'
  r['default_switch']={'passed':True,'old_binding_preserved':True}
  # Three actual rounds per provider, resuming the same external native identity.
  r.setdefault('continuity',{})
  for provider,sid in sessions.items():
   if r['continuity'].get(provider,{}).get('passed'):continue
   ids=[];native=[]
   for index in range(3):
    prompt='Do not use tools. Reply only LIFECYCLE_OK. This is continuity round '+str(index+1);prior=next((t for t in state(sid)['turns'] if t['input']==prompt),None);tid=prior['id'] if prior else send(sid,prompt);s=completed(sid,tid);assert_stopped(s,tid,'completed');ids.append(tid);native.append(s['session']['external_session'])
   assert len(set(native))==1 and native[0];r['continuity'][provider]={'session':sid,'native_session':native[0],'turns':ids,'passed':True};save()
  if not r.get('role_isolation'):
   other=opened(owner,'architect','codex');tid=send(other,'Do not use tools. Reply only ARCHITECT_OK.');s=completed(other,tid);assert_stopped(s,tid,'completed');assert s['session']['external_session']!=r['continuity']['codex']['native_session'];r['role_isolation']={'session':other,'native_session':s['session']['external_session'],'passed':True};save()
  # A deliberately invalid model exercises a real provider failure, never a Sim fallback.
  if not r.get('provider_failure'):
   bad=opened(owner,'reviewer','codex','devdesk-nonexistent-model-'+uuid.uuid4().hex);tid=send(bad,'Reply only OK.');s=completed(bad,tid);assert_stopped(s,tid,'failed');assert s['session']['origin']=='external';r['provider_failure']={'session':bad,'turn':tid,'status':'failed','no_sim_fallback':True};save()
  r.setdefault('cancellation',{})
  for provider,sid in sessions.items():
   if provider in r['cancellation']:continue
   prior=next((t for t in state(sid)['turns'] if t['status'] in ['running','waiting_interaction']),None)
   tid=prior['id'] if prior else send(sid,'Use the confined shell tool to run exactly: sleep 120 . Wait for it to finish. Do not modify any files.');s=wait(sid,tid,lambda s,t:t['consumed'] and t['status']=='running');time.sleep(2);s=state(sid)
   c.command('conversation.cancel',tid,turn(s,tid)['revision']);s=wait(sid,tid,lambda s,t:t['status']=='cancelled');proof=assert_stopped(s,tid,'cancelled');proof['stop_events']=[e['content'] for e in s['events'] if e['turn_id']==tid and e['kind']=='status'];r['cancellation'][provider]=proof;save()
  # The SDK question callback is distinct from text asking the user a question.
  sid=opened(owner,'tester','dsh');tid=send(sid,'Use ask_user_question now. Call the tool with exactly this argument: {"questions":[{"id":"delivery","question":"Which deliverables?","detail":"Choose all required outputs","multi_select":true,"options":[{"label":"Documentation"},{"label":"Tests"}]}]}. Wait for the user answer before proceeding. Then return only the labels selected by the user. Do not ask in plain assistant text.');s=wait(sid,tid,lambda s,t:t['status']=='waiting_interaction' or t['status'] in ['failed','completed']);assert turn(s,tid)['status']=='waiting_interaction',turn(s,tid)
  i=next(i for i in s['interactions'] if i['turn_id']==tid and i['status']=='pending');q=i['questions'][0];assert q['multi_select'] and len(q['options'])==2,q
  payload={'session_id':sid,'turn_id':tid,'answer':{'kind':'user_questions','answers':[{'id':q['id'],'selected':q['options'],'custom':None}],'cancelled':False}}
  c.command('interaction.answer',i['id'],i['revision'],payload);s=completed(sid,tid);assert_stopped(s,tid,'completed')
  try:c.command('interaction.answer',i['id'],i['revision'],payload);raise AssertionError('late answer accepted')
  except RuntimeError:pass
  r.pop('failure',None);r['question']={'session':sid,'turn':tid,'interaction':i['id'],'multi_select':True,'late_answer_rejected':True,'passed':True};r['exercise_passed']=True;save()
 elif a.phase=='crash-arm':
  c.open(r['project'])
  if r.get('crash'):r.setdefault('crash_history',[]).append(r['crash'])
  r.pop('restart_observed_active_project',None);r['crash']=[]
  for provider,role in [('codex','developer'),('dsh','architect')]:
   sid=opened(r['project'],role,provider);tid=send(sid,'Use the confined shell tool to run exactly: sleep 120 . Wait for completion. Do not modify files.');s=wait(sid,tid,lambda s,t:t['consumed'] and t['status']=='running');r['crash'].append({'provider':provider,'project':r['project'],'session':sid,'turn':tid,'message':turn(s,tid)['external_message'],'native_turn':turn(s,tid)['external_turn'],'event_count':len(s['events'])});save()
  # Leave another project active, so startup must recover a background project too.
  other=c.command('project.create',payload={'title':'Active project while another owns native workers'})['id'];r['crash_active_project']=other;r['crash_armed']=True;save()
 else:
  assert r['crash_armed'];active=c.rpc('query.workspace')['active_project']['id'];assert active in [r['crash_active_project'],r['project']];r.setdefault('restart_observed_active_project',active);c.open(r['project'])
  for record in r['crash']:
   sid=record['session'];tid=record['turn'];s=c.wait(lambda:state(sid),lambda s:bool(s['executions']) and all(e['writer_revoked'] and e['protocol_artifact'] and e['status']=='released' for e in s['executions']),30);t=turn(s,tid);assert t['status'] in ['needs_reconciliation','cancelled'],t
   assert t['external_message']==record['message'] and t['external_turn']==record['native_turn'];assert len([x for x in s['turns'] if x['id']==tid])==1
   # Stop proof is required, but an unknown consumed result stays unknown until explicit cancellation.
   ex=s['executions'];assert ex and all(e['writer_revoked'] and e['protocol_artifact'] for e in ex)
   if t['status']!='cancelled':c.command('conversation.cancel',tid,t['revision'])
   s=wait(sid,tid,lambda s,t:t['status']=='cancelled');record.update(assert_stopped(s,tid,'cancelled'));record['not_resent']=True;save()
  r.pop('failure',None);r['background_project_recovered']=True;r['crash_passed']=True;r['passed']=r.get('exercise_passed',False);save()
 print(json.dumps({'phase':a.phase,'passed':r.get(a.phase.replace('-','_')+'_passed',True),'report':str(f)}))
except Exception as e:
 r['failure']={'phase':a.phase,'type':type(e).__name__,'message':str(e)};save();raise
